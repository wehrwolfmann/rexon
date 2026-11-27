//! Wallhaven (wallhaven.cc) — the second store of the «HD Wallpapers» section next to
//! the KDE Store.
//!
//! Our own client built from the official API v1 documentation
//! (<https://wallhaven.cc/help/api>): search and listings
//! `GET /api/v1/search`, picture details `GET /api/v1/w/<id>`.
//! Owner's decision of 06.11.2026, made after an analysis of the
//! wallpaper sources.
//!
//! What lives here and why it is done this way:
//!
//! * **SFW only and no key.** Every request carries `purity=100`; Rexon does not
//!   send an API key (`X-API-Key`) at all — Rexon has no place for secrets yet,
//!   and without a key the server does not return NSFW anyway.
//! * **Limit of 45 requests per minute** ("API calls are currently limited to 45
//!   per minute", beyond that — 429). Our own counter ([`Gate`]) lets out no more than
//!   [`OWN_BUDGET_PER_MINUTE`] per sliding minute, after a 429 it stays silent for
//!   [`RATE_LIMIT_PAUSE_SECS`], and it keeps replies in session memory for
//!   [`CACHE_TTL_SECS`]: coming back to the section or paging back needs no network.
//!   Thumbnails (`th.wallhaven.cc`) and the files themselves (`w.wallhaven.cc`) do not go
//!   through the API and do not count.
//! * **Pictures are the property of their authors** ("All images remain property of
//!   their original owners"). Rexon distributes nothing and puts nothing in the package:
//!   the person downloads the file for themselves into `~/.local/share/wallpapers` with
//!   the same installer as KDE Store wallpapers, and it is applied by the same shared
//!   background executor (`appearance_installer::apply_wallpaper_background`).
//!   The card has a link to the picture's page on the site ([`page_url`]).
//! * There is no separate install branch: the card is an ordinary [`ThemeItem`]
//!   of the wallpaper category (299) with a direct link to the file in `download_links`;
//!   the installed journal, «Apply» and «Delete» are shared.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::i18n::{n_, tr, trc, trf};
use crate::models::ThemeItem;
use crate::net::NetError;
use crate::wallpaper_screens::{orientation_warning, WallpaperOrientation};

/// Site address.
pub const HOME_URL: &str = "https://wallhaven.cc";
/// Root of API v1.
pub const API_BASE: &str = "https://wallhaven.cc/api/v1";
/// Source key in the «Store:» switcher.
pub const SOURCE_KEY: &str = "wallhaven";
/// Prefix of the card id and of the installed journal entry: `wallhaven:<id>`.
pub const ID_PREFIX: &str = "wallhaven:";
/// Sections where Wallhaven is one more store.
pub const SECTIONS: &[&str] = &["wallpapers"];
/// Store category under which the picture is installed (KDE Store wallpapers): from
/// it the installer takes the directory and the single-file rule.
pub const CATEGORY_ID: i64 = 299;
/// How many pictures the server returns per listing page (documentation:
/// "Listings are limited to 24 results per page").
pub const API_PAGE_SIZE: i64 = 24;
/// Server limit on API requests per minute.
pub const RATE_LIMIT_PER_MINUTE: usize = 45;
/// Our own ceiling per sliding minute — with headroom for the site opened in
/// a browser on the same machine (the server limit is shared per address).
pub const OWN_BUDGET_PER_MINUTE: usize = 40;
/// How long to stay silent after a 429 reply.
pub const RATE_LIMIT_PAUSE_SECS: u64 = 60;
/// How long an API reply lives in session memory.
pub const CACHE_TTL_SECS: u64 = 10 * 60;
/// How many replies to keep in memory (old ones are evicted).
pub const CACHE_MAX_ENTRIES: usize = 64;
/// API request timeout, seconds.
pub const TIMEOUT_SECS: u64 = 15;
/// We do not read an API reply larger than this (a listing page is ~20 KB).
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;
/// The full-size file goes into the card gallery only if it is no
/// larger than this: the gallery downloads frames right when opened.
pub const GALLERY_FULL_MAX_BYTES: i64 = 8 << 20;

/// Whether Wallhaven is present in section `section`.
pub fn has_section(section: &str) -> bool {
    SECTIONS.contains(&section)
}

/// Is the card from here?
pub fn is_item_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// Label of the «Store:» chip.
pub fn chip_label() -> String {
    "Wallhaven".to_string()
}

/// Chip tooltip.
pub fn chip_tooltip() -> String {
    tr("Wallpapers from wallhaven.cc, safe-for-work only. The pictures belong to their authors: \
        Rexon downloads the one you pick for you and links to its page on the site.")
}

/// Placeholder of the search field.
pub fn search_placeholder() -> String {
    tr("🔍 Search Wallhaven…")
}

// ═══════════════════════════════════════════════════════════════════════
//  Sorting
// ═══════════════════════════════════════════════════════════════════════

/// Wallhaven sort orders in the drop-down list: `(label, key)`. The keys are
/// the same as the KDE Store ones (`appearance_page::SORTS`), so the choice
/// survives switching the store: «New», «Popular», «Best».
pub const SORTS: &[(&str, &str)] = &[
    (n_("🆕 Newest"), "new"),
    (n_("🔥 Most Viewed"), "down"),
    (n_("🏆 Top of the Month"), "high"),
];

/// The sort key that takes effect: one unknown to Wallhaven (the KDE Store
/// has «Alphabetical») — «New».
pub fn effective_sort(sort: &str) -> &'static str {
    SORTS.iter().map(|(_, k)| *k).find(|k| *k == sort).unwrap_or("new")
}

/// The API `sorting` parameter for a Rexon key (and `topRange` for «Best»).
pub fn api_sorting(sort: &str) -> (&'static str, Option<&'static str>) {
    match effective_sort(sort) {
        "down" => ("views", None),
        "high" => ("toplist", Some("1M")),
        _ => ("date_added", None),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Addresses
// ═══════════════════════════════════════════════════════════════════════

/// Encoding of a query parameter value (space — `+`).
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Address of a listing page. `api_page` — from one, as on the server.
///
/// `categories=111` — all three site categories (general, anime, people);
/// `purity=100` — SFW only, always. `orientation` — the "Orientation" filter
/// of the section ([`api_ratios`]); `None` — any.
pub fn search_url(query: &str, sort: &str, orientation: Option<WallpaperOrientation>, api_page: i64) -> String {
    let (sorting, range) = api_sorting(sort);
    let mut url = format!(
        "{API_BASE}/search?categories=111&purity=100&sorting={sorting}&order=desc&page={}",
        api_page.max(1)
    );
    if let Some(r) = range {
        url.push_str("&topRange=");
        url.push_str(r);
    }
    if let Some(o) = orientation {
        url.push_str("&ratios=");
        url.push_str(api_ratios(o));
    }
    let q = query.trim();
    if !q.is_empty() {
        url.push_str("&q=");
        url.push_str(&quote_plus(q));
    }
    url
}

/// The `ratios` value for an orientation: the site groups all aspect ratios
/// into `landscape` (wider than tall) and `portrait` (taller than wide) — the
/// same groups as its own "Ratio" picker. The server filters by them itself,
/// so pages stay full under the filter.
pub fn api_ratios(orientation: WallpaperOrientation) -> &'static str {
    match orientation {
        WallpaperOrientation::Landscape => "landscape",
        WallpaperOrientation::Portrait => "portrait",
    }
}

/// Whether it looks like a Wallhaven picture id (`94x38z`): Latin letters and digits.
/// Only this gets from a foreign reply into an address.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 16 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Address of the picture details.
pub fn detail_url(wid: &str) -> Option<String> {
    valid_id(wid).then(|| format!("{API_BASE}/w/{wid}"))
}

/// Picture page on the site — built from the id, not taken from the reply.
pub fn page_url(wid: &str) -> String {
    format!("{HOME_URL}/w/{wid}")
}

/// Card id.
pub fn item_id(wid: &str) -> String {
    format!("{ID_PREFIX}{wid}")
}

/// Picture id from the card id.
pub fn parse_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(ID_PREFIX).filter(|w| valid_id(w))
}

/// An address only from Wallhaven hosts over https — otherwise empty: the file and
/// thumbnail links come from the server reply and go to the downloader as is.
fn wallhaven_https(url: &str) -> String {
    let ok = ["https://w.wallhaven.cc/", "https://th.wallhaven.cc/"]
        .iter()
        .any(|p| url.starts_with(p));
    if ok && !url.contains("..") {
        url.to_string()
    } else {
        String::new()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Parsing replies
// ═══════════════════════════════════════════════════════════════════════

/// A number from the reply: the server returns counters sometimes as a number, sometimes as a string.
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Thumbs {
    pub large: String,
    pub original: String,
    pub small: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Uploader {
    pub username: String,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tag {
    pub name: String,
}

/// A picture from a listing or from the details (the details have the uploader and
/// tags).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Wallpaper {
    pub id: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub views: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub favorites: i64,
    pub source: String,
    pub purity: String,
    pub category: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub dimension_x: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub dimension_y: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub file_size: i64,
    pub file_type: String,
    pub created_at: String,
    pub path: String,
    pub thumbs: Thumbs,
    pub uploader: Option<Uploader>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
struct Meta {
    #[serde(deserialize_with = "lenient_i64")]
    current_page: i64,
    #[serde(deserialize_with = "lenient_i64")]
    last_page: i64,
    #[serde(deserialize_with = "lenient_i64")]
    total: i64,
}

#[derive(Deserialize)]
struct SearchBody {
    data: Vec<Value>,
    #[serde(default)]
    meta: Meta,
}

#[derive(Deserialize)]
struct DetailBody {
    data: Wallpaper,
}

/// A listing page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchPage {
    pub items: Vec<Wallpaper>,
    pub current_page: i64,
    pub last_page: i64,
    pub total: i64,
}

/// Whether the picture is fit for the showcase: SFW (the server returns nothing else
/// without a key, but we check ourselves), with an id and a file on a Wallhaven host.
fn usable(w: &Wallpaper) -> bool {
    valid_id(&w.id) && w.purity == "sfw" && !wallhaven_https(&w.path).is_empty()
}

/// Parse a `GET /search` reply. Unfit entries (not SFW, without a file,
/// with a foreign address) silently drop out — one by one, without failing the page.
pub fn parse_search(body: &[u8]) -> Result<SearchPage, String> {
    let parsed: SearchBody = serde_json::from_slice(body).map_err(|e| format!("Wallhaven: {e}"))?;
    let items = parsed
        .data
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Wallpaper>(v).ok())
        .filter(usable)
        .collect();
    Ok(SearchPage {
        items,
        current_page: parsed.meta.current_page,
        last_page: parsed.meta.last_page,
        total: parsed.meta.total,
    })
}

/// Parse a `GET /w/<id>` reply.
pub fn parse_detail(body: &[u8]) -> Result<Wallpaper, String> {
    let parsed: DetailBody = serde_json::from_slice(body).map_err(|e| format!("Wallhaven: {e}"))?;
    if !usable(&parsed.data) {
        return Err("Wallhaven: unusable wallpaper".to_string());
    }
    Ok(parsed.data)
}

// ═══════════════════════════════════════════════════════════════════════
//  Card
// ═══════════════════════════════════════════════════════════════════════

/// File extension from the address (`jpg`, `png`) — known pictures only.
fn file_ext(w: &Wallpaper) -> &'static str {
    let tail = w.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match tail.as_str() {
        "png" => "png",
        "webp" => "webp",
        "gif" => "gif",
        _ => "jpg",
    }
}

/// File name in `~/.local/share/wallpapers`: `wallhaven-<id>.<ext>` — as
/// the site itself names it.
pub fn file_name(w: &Wallpaper) -> String {
    format!("wallhaven-{}.{}", w.id, file_ext(w))
}

/// Site category for a human.
pub fn category_label(category: &str) -> String {
    match category {
        "general" => trc("wallpaper category", "General"),
        "anime" => trc("wallpaper category", "Anime"),
        "people" => trc("wallpaper category", "People"),
        other => other.to_string(),
    }
}

/// `2026-10-10 04:08:04` → `2026-10-10T04:08:04` (ISO, as in the store).
fn iso(created: &str) -> String {
    created.trim().replacen(' ', "T", 1)
}

/// Orientation of a picture by its size (`dimension_x` × `dimension_y`).
pub fn orientation(w: &Wallpaper) -> Option<WallpaperOrientation> {
    WallpaperOrientation::of(u32::try_from(w.dimension_x).unwrap_or(0), u32::try_from(w.dimension_y).unwrap_or(0))
}

/// Tile title: the size, and "Vertical" for vertical pictures — the shared
/// tile rule of [`WallpaperOrientation::shown_on_tile`], as for Razer Axon.
fn tile_name(w: &Wallpaper) -> String {
    let size = format!("{} × {}", w.dimension_x, w.dimension_y);
    match orientation(w).filter(|o| o.shown_on_tile()) {
        Some(o) => format!("{size} · {}", o.label()),
        None => size,
    }
}

/// Picture card — an ordinary item of the wallpaper section: the installer sees
/// a single file of category 299 and puts it into `~/.local/share/wallpapers`.
pub fn theme_item(w: &Wallpaper) -> ThemeItem {
    let path = wallhaven_https(&w.path);
    let name = file_name(w);
    let kib = (w.file_size.max(0) + 1023) / 1024;
    let large = wallhaven_https(&w.thumbs.large);
    let original = wallhaven_https(&w.thumbs.original);
    let mut gallery: Vec<String> = Vec::new();
    if !original.is_empty() {
        gallery.push(original);
    }
    if !path.is_empty() && w.file_size > 0 && w.file_size <= GALLERY_FULL_MAX_BYTES {
        gallery.push(path.clone());
    }
    let preview = if large.is_empty() { gallery.first().cloned().unwrap_or_default() } else { large };
    let author = match &w.uploader {
        Some(u) if !u.username.trim().is_empty() => u.username.trim().to_string(),
        _ => "wallhaven.cc".to_string(),
    };
    ThemeItem {
        id: item_id(&w.id),
        name: tile_name(w),
        description: description(w),
        author,
        license: tr("Belongs to its author — see the page on wallhaven.cc"),
        preview_url: preview,
        preview_urls: gallery,
        download_url: path.clone(),
        download_name: name.clone(),
        download_size: kib,
        category_id: CATEGORY_ID,
        created: iso(&w.created_at),
        updated: iso(&w.created_at),
        download_links: vec![json!({ "url": path, "name": name, "size": kib })],
        ..ThemeItem::default()
    }
}

/// Card description: category, tags (they exist only in the details) and
/// the original source, if the site knows it.
fn description(w: &Wallpaper) -> String {
    let mut lines = vec![trf("Category: %1", &[&category_label(&w.category)])];
    let tags: Vec<&str> = w.tags.iter().map(|t| t.name.trim()).filter(|t| !t.is_empty()).collect();
    if !tags.is_empty() {
        lines.push(trf("Tags: %1", &[&tags.join(", ")]));
    }
    let source = w.source.trim();
    if source.starts_with("https://") || source.starts_with("http://") {
        lines.push(trf("Original source: %1", &[&source]));
    }
    lines.push(trf("Page on Wallhaven: %1", &[&page_url(&w.id)]));
    // A vertical picture (taller than wide): what happens on a regular
    // monitor — the warning shared by all wallpaper sources. Pictures are
    // set by the desktop, hence the "up to the desktop" variant.
    if let Some(warning) = orientation(w).and_then(|o| orientation_warning(o, false)) {
        lines.push(warning);
    }
    lines.join("\n")
}

/// Rating line of the tile: «♥ 328 · 👁 39.4K».
pub fn stats_line(w: &Wallpaper) -> String {
    format!(
        "♥ {} · 👁 {}",
        crate::appearance_page::human_count(w.favorites),
        crate::appearance_page::human_count(w.views)
    )
}

/// Tooltip of the rating line.
pub fn stats_tooltip() -> String {
    tr("Favorites and views on wallhaven.cc")
}

/// A catalog reply row: the card plus what only Wallhaven knows
/// (the page on the site and its own rating line) — under `site*` keys, shared by
/// all third-party stores.
pub fn catalog_row(item: &ThemeItem, w: &Wallpaper) -> Value {
    let mut v = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        m.insert("sitePage".to_string(), Value::from(page_url(&w.id)));
        m.insert("siteStats".to_string(), Value::from(stats_line(w)));
        m.insert("siteStatsTooltip".to_string(), Value::from(stats_tooltip()));
    }
    v
}

// ═══════════════════════════════════════════════════════════════════════
//  A Rexon page from server pages
// ═══════════════════════════════════════════════════════════════════════

/// Which server pages (from one) cover Rexon page number
/// `page` (from zero) of `size` cards — plus one extra card, to
/// honestly know whether there is a next one. The server pages by exactly 24 and hides
/// nothing, so the window is computed at once, without walking from the first page.
pub fn api_pages_for(page: i64, size: i64) -> (i64, i64) {
    let size = size.max(1);
    let start = page.max(0) * size;
    let end = start + size + 1;
    (start / API_PAGE_SIZE + 1, (end - 1) / API_PAGE_SIZE + 1)
}

/// A Rexon page: cards, the total count and whether there is more.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub items: Vec<Wallpaper>,
    pub total: i64,
    pub has_more: bool,
}

/// Build a Rexon page from server pages; `fetch(n)` is server
/// page `n` (from one). An error on the first needed page is a page
/// error; an error on the second just means "we don't know what's next" (by `total`).
pub fn assemble_page(
    page: i64,
    size: i64,
    mut fetch: impl FnMut(i64) -> Result<SearchPage, FetchError>,
) -> Result<Page, FetchError> {
    let size = size.max(1);
    let (first, last) = api_pages_for(page, size);
    let skip = (page.max(0) * size - (first - 1) * API_PAGE_SIZE).max(0) as usize;
    let mut stream: Vec<Wallpaper> = Vec::new();
    let mut total = 0i64;
    let mut last_page = i64::MAX;
    let mut broken = false;
    for n in first..=last {
        if n > last_page {
            break;
        }
        match fetch(n) {
            Ok(p) => {
                total = total.max(p.total);
                if p.last_page > 0 {
                    last_page = p.last_page;
                }
                stream.extend(p.items);
            }
            Err(e) if n == first => return Err(e),
            Err(_) => {
                broken = true;
                break;
            }
        }
    }
    let mut items: Vec<Wallpaper> = stream.into_iter().skip(skip).collect();
    let has_more = if broken {
        (page.max(0) + 1) * size < total
    } else {
        items.len() > size as usize
    };
    items.truncate(size as usize);
    Ok(Page { items, total, has_more })
}

// ═══════════════════════════════════════════════════════════════════════
//  Request limit and reply memory
// ═══════════════════════════════════════════════════════════════════════

/// Why there is no reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Our own counter or the server (429) said "too often": wait
    /// `wait_secs`.
    RateLimited { wait_secs: u64 },
    /// Network or server.
    Net(NetError),
    /// The reply cannot be parsed.
    Parse(String),
}

impl FetchError {
    /// Text for the status line.
    pub fn message(&self) -> String {
        match self {
            FetchError::RateLimited { wait_secs } => trf(
                // TRANSLATORS: %1 is a number of seconds.
                "Wallhaven allows only 45 requests per minute, and the limit is used up. \
                 Try again in %1 s.",
                &[&(*wait_secs).max(1)],
            ),
            FetchError::Net(NetError::Status { code: 401, .. }) => {
                tr("Wallhaven refused access to this picture (it is not safe-for-work).")
            }
            FetchError::Net(e) => crate::ui_base::humanize_network_error(&e.to_string(), "Wallhaven"),
            FetchError::Parse(_) => tr("Wallhaven sent an answer Rexon can't read. Try again later."),
        }
    }
}

/// Request counter per sliding minute, pause after 429 and session reply
/// memory. Time is a parameter (ms), so it is testable without a clock.
#[derive(Debug, Default)]
pub struct Gate {
    sent: VecDeque<u64>,
    paused_until: u64,
    cache: HashMap<String, (u64, Vec<u8>)>,
    order: VecDeque<String>,
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh reply from memory.
    pub fn cached(&self, url: &str, now_ms: u64) -> Option<Vec<u8>> {
        let (at, body) = self.cache.get(url)?;
        (now_ms.saturating_sub(*at) < CACHE_TTL_SECS * 1000).then(|| body.clone())
    }

    /// Whether a request may be sent now; if yes, it is counted immediately.
    /// If no — how many seconds to wait.
    pub fn admit(&mut self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.paused_until {
            return Err((self.paused_until - now_ms).div_ceil(1000));
        }
        while self.sent.front().is_some_and(|t| now_ms.saturating_sub(*t) >= 60_000) {
            self.sent.pop_front();
        }
        if self.sent.len() >= OWN_BUDGET_PER_MINUTE {
            let oldest = self.sent.front().copied().unwrap_or(now_ms);
            return Err((60_000 - now_ms.saturating_sub(oldest)).div_ceil(1000));
        }
        self.sent.push_back(now_ms);
        Ok(())
    }

    /// The server replied 429: stay silent for a minute.
    pub fn throttled(&mut self, now_ms: u64) {
        self.paused_until = now_ms + RATE_LIMIT_PAUSE_SECS * 1000;
    }

    /// Remember a reply.
    pub fn store(&mut self, url: &str, body: Vec<u8>, now_ms: u64) {
        if self.cache.insert(url.to_string(), (now_ms, body)).is_none() {
            self.order.push_back(url.to_string());
        }
        while self.order.len() > CACHE_MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
    }
}

/// One API request through [`Gate`]: memory (unless `force`), counting, network.
/// `get` is the transport (in production [`crate::net::UreqNet`], in tests a closure).
/// The counter mutex is not held during the network call.
pub fn fetch_with(
    gate: &Mutex<Gate>,
    url: &str,
    force: bool,
    now_ms: &dyn Fn() -> u64,
    get: &mut dyn FnMut(&str) -> Result<Vec<u8>, NetError>,
) -> Result<Vec<u8>, FetchError> {
    {
        let mut g = gate.lock().unwrap_or_else(|e| e.into_inner());
        if !force {
            if let Some(body) = g.cached(url, now_ms()) {
                return Ok(body);
            }
        }
        g.admit(now_ms()).map_err(|wait_secs| FetchError::RateLimited { wait_secs })?;
    }
    match get(url) {
        Ok(body) => {
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(FetchError::Parse("Wallhaven: answer too large".to_string()));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).store(url, body.clone(), now_ms());
            Ok(body)
        }
        Err(NetError::Status { code: 429, .. }) => {
            gate.lock().unwrap_or_else(|e| e.into_inner()).throttled(now_ms());
            Err(FetchError::RateLimited { wait_secs: RATE_LIMIT_PAUSE_SECS })
        }
        Err(e) => Err(FetchError::Net(e)),
    }
}

/// Process-wide counter — one for all «Appearance» pages.
pub fn live_gate() -> &'static Mutex<Gate> {
    static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
    GATE.get_or_init(|| Mutex::new(Gate::new()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn live_get(token: &crate::net::CancelToken) -> impl FnMut(&str) -> Result<Vec<u8>, NetError> {
    let mut net = crate::net::UreqNet::new().with_cancel(token.clone());
    move |url: &str| crate::net::NetIo::get(&mut net, url, crate::ocs::USER_AGENT, TIMEOUT_SECS)
}

/// A Rexon page from the live API.
pub fn live_page(
    query: &str,
    sort: &str,
    orientation: Option<WallpaperOrientation>,
    page: i64,
    size: i64,
    force: bool,
    token: &crate::net::CancelToken,
) -> Result<Page, FetchError> {
    let mut get = live_get(token);
    assemble_page(page, size, |n| {
        // The orientation is part of the address, so the session memory keeps
        // filtered and unfiltered pages apart by itself.
        let url = search_url(query, sort, orientation, n);
        let body = fetch_with(live_gate(), &url, force, &now_ms, &mut get)?;
        parse_search(&body).map_err(FetchError::Parse)
    })
}

/// Picture details from the live API (uploader, tags).
pub fn live_detail(wid: &str, token: &crate::net::CancelToken) -> Result<Wallpaper, FetchError> {
    let url = detail_url(wid).ok_or_else(|| FetchError::Parse("Wallhaven: bad id".to_string()))?;
    let mut get = live_get(token);
    let body = fetch_with(live_gate(), &url, false, &now_ms, &mut get)?;
    parse_detail(&body).map_err(FetchError::Parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &[u8] = include_bytes!("../tests/data/wallhaven/search_toplist.json");
    const DETAIL: &[u8] = include_bytes!("../tests/data/wallhaven/detail.json");

    #[test]
    fn search_url_is_sfw_and_keyless() {
        let u = search_url("", "new", None, 1);
        assert_eq!(u, "https://wallhaven.cc/api/v1/search?categories=111&purity=100&sorting=date_added&order=desc&page=1");
        assert!(!u.contains("apikey") && !u.contains("ratios"));
        let u = search_url(" sky & clouds ", "high", None, 3);
        assert!(u.contains("sorting=toplist") && u.contains("topRange=1M") && u.contains("page=3"));
        assert!(u.ends_with("&q=sky+%26+clouds"), "{u}");
        assert!(search_url("x", "down", None, 0).contains("sorting=views&order=desc&page=1"));
        // Wallhaven has no «Alphabetical» — «New».
        assert_eq!(effective_sort("alpha"), "new");
        assert!(search_url("", "alpha", None, 1).contains("sorting=date_added"));
    }

    /// The "Orientation" filter goes to the server as `ratios=` (checked
    /// against the live API on 24.11.2026: `portrait` returns only pictures
    /// taller than wide, `landscape` only wider ones); the search word stays
    /// last.
    #[test]
    fn orientation_filter_goes_into_the_request() {
        let tall = search_url("sky", "new", Some(WallpaperOrientation::Portrait), 2);
        assert!(tall.contains("&ratios=portrait&q=sky") && tall.contains("page=2"), "{tall}");
        let wide = search_url("", "high", Some(WallpaperOrientation::Landscape), 1);
        assert!(wide.ends_with("&topRange=1M&ratios=landscape"), "{wide}");
        assert_ne!(tall, search_url("sky", "new", None, 2), "filtered pages are cached apart");
    }

    #[test]
    fn ids_and_urls() {
        assert!(valid_id("94x38z"));
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("A1"));
        assert_eq!(detail_url("yqkl97").as_deref(), Some("https://wallhaven.cc/api/v1/w/yqkl97"));
        assert_eq!(detail_url("a/b"), None);
        assert_eq!(page_url("yqkl97"), "https://wallhaven.cc/w/yqkl97");
        assert_eq!(parse_item_id(&item_id("yqkl97")), Some("yqkl97"));
        assert_eq!(parse_item_id("wallhaven:../x"), None);
        assert_eq!(parse_item_id("spices:theme:x"), None);
        assert!(has_section("wallpapers") && !has_section("icons"));
    }

    #[test]
    fn parses_saved_search_sample() {
        let p = parse_search(SEARCH).expect("образец разбирается");
        assert_eq!((p.current_page, p.last_page, p.total), (1, 98, 2350));
        // The sample has 24 entries; three are broken on purpose (NSFW, foreign host,
        // bad id) — they drop out one by one.
        assert_eq!(p.items.len(), 21);
        assert!(p.items.iter().all(|w| w.purity == "sfw" && valid_id(&w.id)));
        let w = &p.items[0];
        assert_eq!(w.id, "yqkl97");
        assert_eq!((w.dimension_x, w.dimension_y, w.file_size), (2074, 1166, 976307));
        assert_eq!(w.path, "https://w.wallhaven.cc/full/yq/wallhaven-yqkl97.jpg");
        assert_eq!(w.thumbs.large, "https://th.wallhaven.cc/lg/yq/yqkl97.jpg");
        assert!(w.uploader.is_none() && w.tags.is_empty());
    }

    #[test]
    fn parses_saved_detail_sample() {
        let w = parse_detail(DETAIL).expect("образец разбирается");
        assert_eq!(w.id, "yqkl97");
        assert_eq!(w.uploader.as_ref().map(|u| u.username.as_str()), Some("Artix"));
        assert!(w.tags.iter().any(|t| t.name == "clouds"));
        let it = theme_item(&w);
        assert_eq!(it.author, "Artix");
        assert!(it.description.contains("clouds"));
        assert!(it.description.contains("https://wallhaven.cc/w/yqkl97"));
        assert!(it.description.contains("https://www.pixiv.net/en/artworks/150504495"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_search(b"<html>").is_err());
        assert!(parse_search(br#"{"error":"x"}"#).is_err());
        // String counters are numbers too.
        let p = parse_search(br#"{"data":[],"meta":{"current_page":"2","last_page":"5","per_page":"24","total":"100"}}"#).unwrap();
        assert_eq!((p.current_page, p.last_page, p.total), (2, 5, 100));
        assert!(parse_detail(br#"{"data":{"id":"abc","purity":"nsfw","path":"https://w.wallhaven.cc/full/ab/wallhaven-abc.jpg"}}"#).is_err());
    }

    #[test]
    fn vertical_picture_is_warned_in_description() {
        let mut w = Wallpaper { id: "abc123".into(), dimension_x: 1080, dimension_y: 1920, ..Wallpaper::default() };
        let tall = description(&w);
        w.dimension_x = 1920;
        w.dimension_y = 1080;
        let wide = description(&w);
        assert_eq!(tall.lines().count(), wide.lines().count() + 1, "{tall}");
        assert!(tall.lines().last().unwrap().starts_with("⚠️"), "{tall}");
    }

    #[test]
    fn vertical_picture_is_labelled_on_tile() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut w = Wallpaper { id: "abc123".into(), dimension_x: 1080, dimension_y: 1920, ..Wallpaper::default() };
        assert_eq!(orientation(&w), Some(WallpaperOrientation::Portrait));
        assert_eq!(theme_item(&w).name, "1080 × 1920 · Vertical");
        w.dimension_x = 1920;
        w.dimension_y = 1080;
        assert_eq!(theme_item(&w).name, "1920 × 1080", "horizontal ones — unchanged");
        w.dimension_x = 0;
        assert_eq!(orientation(&w), None);
    }

    #[test]
    fn card_is_plain_wallpaper_item() {
        let p = parse_search(SEARCH).unwrap();
        let w = &p.items[0];
        let it = theme_item(w);
        assert_eq!(it.id, "wallhaven:yqkl97");
        assert_eq!(it.category_id, CATEGORY_ID);
        assert_eq!(it.name, "2074 × 1166");
        assert_eq!(it.author, "wallhaven.cc");
        assert_eq!(it.download_name, "wallhaven-yqkl97.jpg");
        assert_eq!(it.download_size, 954);
        assert_eq!(it.created, "2026-10-10T04:08:04");
        assert_eq!(it.preview_url, "https://th.wallhaven.cc/lg/yq/yqkl97.jpg");
        // Gallery: the uncropped thumbnail and the file itself (it is under 8 MiB).
        assert_eq!(it.preview_urls, vec!["https://th.wallhaven.cc/orig/yq/yqkl97.jpg".to_string(), w.path.clone()]);
        // The file link is the one the shared store downloader will take.
        assert_eq!(it.download_links.len(), 1);
        assert_eq!(it.download_links[0]["url"], w.path.as_str());
        assert_eq!(crate::ocs::chosen_file_name(&it).as_deref(), Some("wallhaven-yqkl97.jpg"));
        // A large file does not go into the gallery.
        let big = p.items.iter().find(|w| w.file_size > GALLERY_FULL_MAX_BYTES).expect("в образце есть большой файл");
        assert_eq!(theme_item(big).preview_urls.len(), 1);
        let row = catalog_row(&it, w);
        assert_eq!(row["sitePage"], "https://wallhaven.cc/w/yqkl97");
        assert_eq!(row["siteStats"], "♥ 328 · 👁 39.4K");
        // The installer sees a single picture of the wallpaper section.
        let cat = crate::categories::category_by_id(std::path::Path::new("/home/u"), it.category_id).unwrap();
        assert_eq!(cat.key, "wallpapers");
        assert_eq!(cat.install_dir, "/home/u/.local/share/wallpapers");
    }

    #[test]
    fn page_window_math() {
        assert_eq!(api_pages_for(0, 20), (1, 1));
        assert_eq!(api_pages_for(0, 24), (1, 2)); // the 25th — "is there more"
        assert_eq!(api_pages_for(1, 20), (1, 2));
        assert_eq!(api_pages_for(5, 20), (5, 6)); // cards 100..=120
    }

    fn fake_page(n: i64, last: i64) -> SearchPage {
        let items = (0..API_PAGE_SIZE)
            .map(|i| Wallpaper { id: format!("p{n}i{i}"), ..Wallpaper::default() })
            .collect();
        SearchPage { items, current_page: n, last_page: last, total: last * API_PAGE_SIZE }
    }

    #[test]
    fn assembles_rexon_pages_without_gaps() {
        let mut asked = Vec::new();
        let p = assemble_page(1, 20, |n| {
            asked.push(n);
            Ok(fake_page(n, 10))
        })
        .unwrap();
        assert_eq!(asked, vec![1, 2]);
        assert_eq!(p.items.len(), 20);
        assert_eq!(p.items[0].id, "p1i20");
        assert_eq!(p.items[19].id, "p2i15");
        assert!(p.has_more);
        assert_eq!(p.total, 240);
        // End of the listing: the last server page — we do not ask further.
        let mut asked = Vec::new();
        let p = assemble_page(1, 20, |n| {
            asked.push(n);
            Ok(fake_page(n, 1))
        })
        .unwrap();
        assert_eq!(asked, vec![1]);
        assert_eq!(p.items.len(), 4);
        assert!(!p.has_more);
        // The first needed page did not arrive — a page error.
        let e = assemble_page(0, 20, |_| Err(FetchError::RateLimited { wait_secs: 7 })).unwrap_err();
        assert_eq!(e, FetchError::RateLimited { wait_secs: 7 });
    }

    #[test]
    fn gate_counts_a_sliding_minute() {
        let mut g = Gate::new();
        for i in 0..OWN_BUDGET_PER_MINUTE as u64 {
            assert!(g.admit(1_000 + i).is_ok());
        }
        let wait = g.admit(30_000).unwrap_err();
        assert_eq!(wait, 31); // the oldest one (1 s) leaves the window at 61 s
        assert!(g.admit(61_000).is_ok());
        g.throttled(100_000);
        assert_eq!(g.admit(100_500).unwrap_err(), 60);
        assert!(g.admit(160_000).is_ok());
    }

    #[test]
    fn fetch_uses_memory_and_reports_429() {
        let gate = Mutex::new(Gate::new());
        let clock = std::cell::Cell::new(1_000u64);
        let now = || clock.get();
        let calls = std::cell::Cell::new(0);
        let mut ok = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(b"{}".to_vec())
        };
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 1, "второй раз — из памяти");
        assert!(fetch_with(&gate, "u", true, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 2, "«Обновить» идёт в сеть");
        clock.set(1_000 + CACHE_TTL_SECS * 1000);
        assert!(fetch_with(&gate, "u", false, &now, &mut ok).is_ok());
        assert_eq!(calls.get(), 3, "протухшее — заново");

        let mut too_many = |_: &str| Err(NetError::Status { code: 429, reason: "Too Many Requests".into() });
        let e = fetch_with(&gate, "v", false, &now, &mut too_many).unwrap_err();
        assert_eq!(e, FetchError::RateLimited { wait_secs: RATE_LIMIT_PAUSE_SECS });
        // After a 429 we do not go to the network at all until the pause is over.
        let mut never = |_: &str| -> Result<Vec<u8>, NetError> { panic!("сеть после 429") };
        assert!(matches!(fetch_with(&gate, "w", false, &now, &mut never), Err(FetchError::RateLimited { .. })));
        assert!(e.message().contains("45"));
    }
}
