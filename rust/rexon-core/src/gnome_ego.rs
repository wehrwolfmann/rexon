//! extensions.gnome.org (EGO) — the store of the "GNOME Extensions" section.
//!
//! Our own client following the site's official API v1 schema (OpenAPI 3:
//! <https://extensions.gnome.org/api/schema/?format=json>, page
//! <https://extensions.gnome.org/api/docs/>) and our own measurements of the
//! answers (08.11 and 09.11.2026). Routes:
//!
//! * listing `GET /api/v1/extensions/` — `ordering`, `page`, `page_size`,
//!   `status`, `shell_version`, `uuid` (may be repeated);
//! * search `GET /api/v1/extensions/search/{string}/` — the same, except
//!   `status` and `uuid` (the route does not have them);
//! * card `GET /api/v1/extensions/{uuid}/`;
//! * releases `GET /api/v1/extensions/{uuid}/versions/` (oldest to newest, in
//!   pages), a single release — `…/versions/{N}/?format=json|zip`.
//!
//! What lives here and why it is this way:
//!
//! * **It is GNOME, not Rexon, that installs.** A card is an ordinary
//!   [`ThemeItem`] of category 914 with EMPTY `download_links`: installation is
//!   requested from the shell (`InstallRemoteExtension`), and it fetches the
//!   build for itself from the site. The release zip address
//!   ([`release_zip_url`]) is only for the fallback path when there is no shell.
//! * **Only approved items.** The listing goes out with `status=3`; among
//!   releases the "version for you" is chosen only from active ones
//!   ([`STATUS_ACTIVE`]).
//! * **The server computes compatibility.** With the "only for your version"
//!   filter, `shell_version` in the site's form ([`site_shell_version`]) is
//!   added to the request; without the filter, incompatible ones are marked by a
//!   second request — a listing by a batch of uuids ([`compat_batch_urls`], up to
//!   [`UUID_BATCH`] at a time).
//! * **Politeness.** The site has no published limit, but one page is one
//!   request, answers are kept in the session's memory ([`Gate`]: listings for
//!   [`LIST_TTL_SECS`], cards and releases for [`DETAIL_TTL_SECS`]), and after
//!   a 5xx answer we leave the site alone for [`FAIL_BACKOFF_SECS`] (except for
//!   "Refresh").
//! * **Pictures — only from the site itself** over https ([`ego_https`]): the
//!   screenshot and icon come as an address from the answer and go to the
//!   picture loader as is. Descriptions and screenshots belong to their authors
//!   — Rexon only shows them.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::Value;

use crate::i18n::{n_, tr, trf};
use crate::models::ThemeItem;
use crate::net::NetError;

/// The site address.
pub const HOME_URL: &str = "https://extensions.gnome.org";
/// Root of API v1.
pub const API_BASE: &str = "https://extensions.gnome.org/api/v1";
/// Source key in the "Store:" switch.
pub const SOURCE_KEY: &str = "ego";
/// Card id prefix: `ego:<uuid>`.
pub const ID_PREFIX: &str = "ego:";
/// Provider in the installed-items journal.
pub const PROVIDER_ID: &str = "ego";
/// Sections where EGO is the store.
pub const SECTIONS: &[&str] = &["gnomeext"];
/// Category of the "GNOME Extensions" section (`categories::CATEGORIES`).
pub const CATEGORY_ID: i64 = 914;
/// Cards per storefront page — the tab's shared page size. The server accepts
/// any `page_size`, so a Rexon page = a server page.
pub const PAGE_SIZE: i64 = crate::appearance_page::PAGE_SIZE as i64;
/// Releases per page when paging through versions (the server returns up to 100).
pub const VERSIONS_PAGE_SIZE: i64 = 100;
/// We do not page through more release pages than this (a thousand releases —
/// with a huge margin: the oldest extensions have about a hundred).
pub const VERSIONS_MAX_PAGES: usize = 10;
/// How many uuids to ask for in one listing when checking compatibility.
pub const UUID_BATCH: usize = 50;
/// `status` of an approved (active) release: 0 — unreviewed, 1 — rejected,
/// 2 — inactive, 3 — active, 4 — waiting for the author.
pub const STATUS_ACTIVE: i64 = 3;
/// How long a listing or search answer lives in memory.
pub const LIST_TTL_SECS: u64 = 10 * 60;
/// How long a card and releases live in memory.
pub const DETAIL_TTL_SECS: u64 = 60 * 60;
/// How long to leave the site alone after a 5xx answer (except "Refresh").
pub const FAIL_BACKOFF_SECS: u64 = 2 * 60;
/// How many answers to keep in memory (old ones are evicted).
pub const CACHE_MAX_ENTRIES: usize = 96;
/// Request timeout, seconds.
pub const TIMEOUT_SECS: u64 = 15;
/// We do not read answers larger than this (a page of 100 releases is ~40 KB).
pub const MAX_RESPONSE_BYTES: usize = 2 << 20;

/// Whether section `section` has EGO.
pub fn has_section(section: &str) -> bool {
    SECTIONS.contains(&section)
}

/// Is the card from here?
pub fn is_item_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// Label of the "Store:" chip.
pub fn chip_label() -> String {
    "extensions.gnome.org".to_string()
}

/// Tooltip of the chip.
pub fn chip_tooltip() -> String {
    tr("The official GNOME extensions site. GNOME itself installs the extension you pick \
        and asks you to confirm.")
}

/// Placeholder of the search line.
pub fn search_placeholder() -> String {
    tr("🔍 Search GNOME extensions…")
}

// ═══════════════════════════════════════════════════════════════════════
//  Sort orders
// ═══════════════════════════════════════════════════════════════════════

/// EGO sort orders: `(label, key)`. "Newest" and "Downloads" are the same keys
/// as in the KDE Store (`appearance_page::SORTS`), so the choice survives a
/// change of store.
pub const SORTS: &[(&str, &str)] = &[
    (n_("🔥 Popular"), "popular"),
    (n_("⬇️ Most Downloaded"), "down"),
    (n_("🔄 Recently Updated"), "updated"),
    (n_("🆕 Newest"), "new"),
];

/// The sort key that is in effect: one unknown to EGO ("Best", "Alphabetical"
/// in the KDE Store) becomes "Popular", like the site's own storefront.
pub fn effective_sort(sort: &str) -> &'static str {
    SORTS.iter().map(|(_, k)| *k).find(|k| *k == sort).unwrap_or("popular")
}

/// The `ordering` parameter for a Rexon key.
pub fn api_ordering(sort: &str) -> &'static str {
    match effective_sort(sort) {
        "down" => "-downloads",
        "updated" => "-updated",
        "new" => "-created",
        _ => "-popularity",
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Shell version
// ═══════════════════════════════════════════════════════════════════════

/// The GNOME Shell version as the site counts it: since GNOME 40 — the major
/// number (`51.1` → 51), for 3.x — a pair (`3.38.4` → 3.38). A pre-release
/// (`51.beta`, `51.alpha.2`, `51.rc`) is remembered as a word: the site accepts
/// `51.beta`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellVersion {
    pub major: i64,
    /// Only for 3.x.
    pub minor: Option<i64>,
    /// `alpha` / `beta` / `rc`, if the shell is a pre-release.
    pub pre: Option<String>,
}

/// Parse `ShellVersion` (a D-Bus property or `gnome-shell --version`: "GNOME
/// Shell 51.1" also works). Garbage — `None`.
pub fn parse_shell_version(raw: &str) -> Option<ShellVersion> {
    let s = raw.trim();
    let s = s.rsplit(' ').next().unwrap_or(s);
    let mut parts = s.split('.');
    let major: i64 = parts.next()?.parse().ok()?;
    if major <= 0 {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let pre_of = |p: &str| {
        let low = p.to_ascii_lowercase();
        ["alpha", "beta", "rc"].contains(&low.as_str()).then_some(low)
    };
    if major == 3 {
        let minor: i64 = rest.first()?.parse().ok()?;
        return Some(ShellVersion { major, minor: Some(minor), pre: None });
    }
    if major < 40 {
        return None;
    }
    let pre = rest.first().and_then(|p| pre_of(p));
    if pre.is_none() {
        if let Some(p) = rest.first() {
            p.parse::<i64>().ok()?;
        }
    }
    Some(ShellVersion { major, minor: None, pre })
}

/// Value of the `shell_version` parameter for the site: `51`, `51.beta`, `3.38`.
pub fn site_shell_version(v: &ShellVersion) -> String {
    match (&v.minor, &v.pre) {
        (Some(minor), _) => format!("{}.{minor}", v.major),
        (None, Some(pre)) => format!("{}.{pre}", v.major),
        (None, None) => v.major.to_string(),
    }
}

/// Version label for the person: "51", "3.38" (a pre-release — without the
/// word: in labels like "Does not support GNOME 51" it only gets in the way).
pub fn shell_label(v: &ShellVersion) -> String {
    match v.minor {
        Some(minor) => format!("{}.{minor}", v.major),
        None => v.major.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Addresses
// ═══════════════════════════════════════════════════════════════════════

/// Encoding a query parameter value and a path piece.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Whether it looks like an extension uuid (`dash-to-dock@micxgx.gmail.com`).
/// Only such strings from a foreign answer and from a card id get into an
/// address.
pub fn valid_uuid(uuid: &str) -> bool {
    !uuid.is_empty()
        && uuid.len() <= 200
        && !uuid.starts_with('.')
        && !uuid.contains("..")
        && uuid.bytes().all(|b| b.is_ascii_alphanumeric() || b"._@+-".contains(&b))
}

/// Address of a storefront page. `page` — from zero (as in the tab), `shell` —
/// the value of [`site_shell_version`] or `None` ("show all"). An empty query
/// — the listing of approved items, otherwise the search route.
pub fn listing_url(query: &str, sort: &str, page: i64, size: i64, shell: Option<&str>) -> String {
    // The server confuses a slash in a path piece with a separator — we
    // replace it with a space: there is no point in searching by it.
    let q = query.replace('/', " ");
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut url = if q.is_empty() {
        format!("{API_BASE}/extensions/?status={STATUS_ACTIVE}&")
    } else {
        format!("{API_BASE}/extensions/search/{}/?", quote(&q))
    };
    url.push_str(&format!(
        "ordering={}&page={}&page_size={}",
        api_ordering(sort),
        page.max(0) + 1,
        size.clamp(1, 100)
    ));
    if let Some(s) = shell.filter(|s| !s.is_empty()) {
        url.push_str("&shell_version=");
        url.push_str(&quote(s));
    }
    url
}

/// Addresses of the compatibility check: listings by batches of uuids (no more
/// than [`UUID_BATCH`] in each) with the same `shell_version`. Each answer is
/// those from the batch that fit. Invalid uuids and duplicates drop out.
pub fn compat_batch_urls(uuids: &[&str], shell: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let clean: Vec<&str> = uuids.iter().copied().filter(|u| valid_uuid(u) && seen.insert(*u)).collect();
    clean
        .chunks(UUID_BATCH)
        .map(|chunk| {
            let mut url = format!("{API_BASE}/extensions/?page_size={UUID_BATCH}&shell_version={}", quote(shell));
            for u in chunk {
                url.push_str("&uuid=");
                url.push_str(&quote(u));
            }
            url
        })
        .collect()
}

/// Card address.
pub fn detail_url(uuid: &str) -> Option<String> {
    valid_uuid(uuid).then(|| format!("{API_BASE}/extensions/{}/", quote(uuid)))
}

/// Address of a page of releases (`page` — from one, as on the server).
pub fn versions_url(uuid: &str, page: i64) -> Option<String> {
    valid_uuid(uuid).then(|| {
        format!(
            "{API_BASE}/extensions/{}/versions/?page={}&page_size={VERSIONS_PAGE_SIZE}",
            quote(uuid),
            page.max(1)
        )
    })
}

/// Address of a release archive — only for the fallback path without a shell.
pub fn release_zip_url(uuid: &str, version: i64) -> Option<String> {
    (valid_uuid(uuid) && version > 0)
        .then(|| format!("{API_BASE}/extensions/{}/versions/{version}/?format=zip", quote(uuid)))
}

/// Card id.
pub fn item_id(uuid: &str) -> String {
    format!("{ID_PREFIX}{uuid}")
}

/// uuid from a card id.
pub fn parse_item_id(id: &str) -> Option<&str> {
    id.strip_prefix(ID_PREFIX).filter(|u| valid_uuid(u))
}

/// An address only from the site itself over https — otherwise empty.
pub fn ego_https(url: &str) -> String {
    let url = url.trim();
    if url.starts_with("https://extensions.gnome.org/") && !url.contains("..") && !url.contains(char::is_whitespace)
    {
        url.to_string()
    } else {
        String::new()
    }
}

/// An external link (home page, donations): only http(s).
fn web_link(url: &str) -> String {
    let url = url.trim();
    if (url.starts_with("https://") || url.starts_with("http://")) && !url.contains(char::is_whitespace) {
        url.to_string()
    } else {
        String::new()
    }
}

/// The extension's page on the site: from the answer's `link`
/// (`/extension/9419/x/`) if it looks like a site path, otherwise from the id.
pub fn page_url(e: &Extension) -> String {
    let link = e.link.trim();
    let safe = link.starts_with("/extension/")
        && !link.contains("..")
        && link.bytes().all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b));
    if safe {
        format!("{HOME_URL}{link}")
    } else if e.id > 0 {
        format!("{HOME_URL}/extension/{}/", e.id)
    } else {
        HOME_URL.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Parsing answers
// ═══════════════════════════════════════════════════════════════════════

/// `null` and a missing field are the same thing: the default value.
fn nullable<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// An integer from the answer: a number (including a fractional one) or a
/// string; anything else — 0.
fn lenient_i64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    })
}

/// A float from the answer; anything else — 0.
fn lenient_f64<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    })
}

/// A list of strings; `null`, non-strings inside and a non-list are tolerated.
fn lenient_strings<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a.into_iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    })
}

/// The extension's author.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Creator {
    #[serde(deserialize_with = "lenient_i64")]
    pub id: i64,
    #[serde(deserialize_with = "nullable")]
    pub username: String,
    #[serde(deserialize_with = "nullable")]
    pub display_name: String,
}

/// An extension from a listing, a search or a card (all three have the same fields).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Extension {
    #[serde(deserialize_with = "lenient_i64")]
    pub id: i64,
    #[serde(deserialize_with = "nullable")]
    pub uuid: String,
    #[serde(deserialize_with = "nullable")]
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub creator: Creator,
    #[serde(deserialize_with = "nullable")]
    pub description: String,
    #[serde(deserialize_with = "nullable")]
    pub created: String,
    #[serde(deserialize_with = "nullable")]
    pub updated: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub downloads: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub popularity: i64,
    #[serde(deserialize_with = "nullable")]
    pub screenshot: String,
    #[serde(deserialize_with = "nullable")]
    pub icon: String,
    /// Average rating 0…5.
    #[serde(deserialize_with = "lenient_f64")]
    pub rating: f64,
    /// Number of votes.
    #[serde(deserialize_with = "lenient_i64")]
    pub rated: i64,
    /// The author's home page.
    #[serde(deserialize_with = "nullable")]
    pub url: String,
    #[serde(deserialize_with = "lenient_strings")]
    pub donation_urls: Vec<String>,
    /// Path of the page on the site.
    #[serde(deserialize_with = "nullable")]
    pub link: String,
}

/// GNOME version in a release: `minor`/`point` = −1 — "any".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ReleaseShell {
    #[serde(deserialize_with = "lenient_i64")]
    pub major: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub minor: i64,
    #[serde(deserialize_with = "lenient_i64")]
    pub point: i64,
}

impl ReleaseShell {
    /// Whether it fits the shell: since 40 — by the major number, for 3.x — by the pair.
    pub fn matches(&self, v: &ShellVersion) -> bool {
        if self.major != v.major {
            return false;
        }
        match v.minor {
            Some(minor) => self.minor == minor || self.minor < 0,
            None => true,
        }
    }
}

/// A release of an extension.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Release {
    #[serde(deserialize_with = "lenient_i64")]
    pub version: i64,
    #[serde(deserialize_with = "nullable")]
    pub version_name: String,
    #[serde(deserialize_with = "lenient_i64")]
    pub status: i64,
    #[serde(deserialize_with = "nullable")]
    pub shell_versions: Vec<ReleaseShell>,
    #[serde(deserialize_with = "nullable")]
    pub created: String,
    #[serde(deserialize_with = "lenient_strings")]
    pub session_modes: Vec<String>,
}

impl Release {
    /// Release label: the author's name ("5.1.0"), otherwise the site's number.
    pub fn label(&self) -> String {
        let name = self.version_name.trim();
        if name.is_empty() {
            self.version.to_string()
        } else {
            name.to_string()
        }
    }

    /// Whether it fits the shell.
    pub fn supports(&self, v: &ShellVersion) -> bool {
        self.shell_versions.iter().any(|s| s.matches(v))
    }
}

/// A server answer page: `{count, next, previous, results}`.
#[derive(Deserialize)]
struct PageBody {
    #[serde(default, deserialize_with = "lenient_i64")]
    count: i64,
    #[serde(default)]
    next: Option<String>,
    results: Vec<Value>,
}

/// A page of extensions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExtPage {
    pub items: Vec<Extension>,
    /// How many there are in total for the query.
    pub total: i64,
    /// Whether there is a next page.
    pub has_more: bool,
}

/// A page of releases.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReleasePage {
    pub items: Vec<Release>,
    pub total: i64,
    pub has_more: bool,
}

fn parse_page(body: &[u8]) -> Result<PageBody, String> {
    serde_json::from_slice(body).map_err(|e| format!("EGO: {e}"))
}

/// Parse a listing or search. Entries without a valid uuid drop out one by one
/// without breaking the page.
pub fn parse_listing(body: &[u8]) -> Result<ExtPage, String> {
    let p = parse_page(body)?;
    let items = p
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Extension>(v).ok())
        .filter(|e| valid_uuid(&e.uuid))
        .collect();
    Ok(ExtPage { items, total: p.count, has_more: p.next.is_some_and(|n| !n.is_empty()) })
}

/// Parse a card.
pub fn parse_detail(body: &[u8]) -> Result<Extension, String> {
    let e: Extension = serde_json::from_slice(body).map_err(|e| format!("EGO: {e}"))?;
    if !valid_uuid(&e.uuid) {
        return Err("EGO: extension without uuid".to_string());
    }
    Ok(e)
}

/// Parse a page of releases. Releases without a number drop out.
pub fn parse_versions(body: &[u8]) -> Result<ReleasePage, String> {
    let p = parse_page(body)?;
    let items = p
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Release>(v).ok())
        .filter(|r| r.version > 0)
        .collect();
    Ok(ReleasePage { items, total: p.count, has_more: p.next.is_some_and(|n| !n.is_empty()) })
}

// ═══════════════════════════════════════════════════════════════════════
//  Compatibility and versions
// ═══════════════════════════════════════════════════════════════════════

/// "Version for you": the newest ACTIVE release that declares the current
/// shell. If there is none — `None` (installing is possible, but with a
/// warning).
pub fn release_for<'a>(releases: &'a [Release], v: &ShellVersion) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| r.status == STATUS_ACTIVE && r.supports(v))
        .max_by_key(|r| r.version)
}

/// Position of a GNOME version in the overall release sequence: …3.36, 3.38,
/// 40, 41… follow one another. Odd 3.x (development) versions have no
/// neighbors.
fn shell_position(s: &ReleaseShell) -> Option<i64> {
    match s.major {
        3 if s.minor >= 0 && s.minor % 2 == 0 => Some(s.minor / 2),
        m if m >= 40 => Some(m - 20),
        _ => None,
    }
}

fn shell_text(s: &ReleaseShell) -> String {
    if s.major == 3 && s.minor >= 0 {
        format!("3.{}", s.minor)
    } else {
        s.major.to_string()
    }
}

/// The list of GNOME versions declared by active releases (if there are no
/// active ones — by all), compressed into ranges: "3.36–41, 45, 47–50".
pub fn supported_summary(releases: &[Release]) -> String {
    let active: Vec<&Release> = releases.iter().filter(|r| r.status == STATUS_ACTIVE).collect();
    let pool: Vec<&Release> = if active.is_empty() { releases.iter().collect() } else { active };
    // (order, position in the sequence, text): 3.x before 40+.
    let mut set: BTreeSet<(i64, i64, Option<i64>, String)> = BTreeSet::new();
    for r in pool {
        for s in &r.shell_versions {
            if s.major < 3 || (s.major > 3 && s.major < 40) {
                continue;
            }
            let minor_key = if s.major == 3 { s.minor.max(0) } else { 0 };
            set.insert((s.major, minor_key, shell_position(s), shell_text(s)));
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let mut run: Option<(String, String, i64)> = None; // (start, end, position of the end)
    for (_, _, pos, text) in set {
        match (&mut run, pos) {
            (Some((_, end, last)), Some(p)) if p == *last + 1 => {
                *end = text;
                *last = p;
            }
            _ => {
                if let Some((a, b, _)) = run.take() {
                    parts.push(if a == b { a } else { format!("{a}–{b}") });
                }
                match pos {
                    Some(p) => run = Some((text.clone(), text, p)),
                    None => parts.push(text),
                }
            }
        }
    }
    if let Some((a, b, _)) = run {
        parts.push(if a == b { a } else { format!("{a}–{b}") });
    }
    parts.join(", ")
}

/// Rows of the "Supported GNOME versions" dialog: newest to oldest, each with
/// the release label, its GNOME versions and the "for you" and "installed"
/// marks (`installed` — the site number from the shell's information).
pub fn release_rows(releases: &[Release], v: Option<&ShellVersion>, installed: Option<i64>) -> Vec<Value> {
    let chosen = v.and_then(|v| release_for(releases, v)).map(|r| r.version);
    let mut rows: Vec<&Release> = releases.iter().filter(|r| r.status == STATUS_ACTIVE).collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.version));
    rows.into_iter()
        .map(|r| {
            serde_json::json!({
                "version": r.version,
                "label": r.label(),
                "shells": supported_summary(std::slice::from_ref(r)),
                "forYou": Some(r.version) == chosen,
                "installed": Some(r.version) == installed,
            })
        })
        .collect()
}

/// Which of the shown uuids do NOT fit: those absent from the check answers.
pub fn unsupported_uuids(shown: &[&str], compatible: &HashSet<String>) -> HashSet<String> {
    shown.iter().filter(|u| !compatible.contains(**u)).map(|u| u.to_string()).collect()
}

// ═══════════════════════════════════════════════════════════════════════
//  Labels
// ═══════════════════════════════════════════════════════════════════════

/// The compatibility filter toggle.
pub fn only_compatible_label() -> String {
    tr("Only for your GNOME version")
}

/// The "show all" position.
pub fn show_all_label() -> String {
    tr("Show all")
}

/// Mark of an incompatible one.
pub fn unsupported_label(shell: &str) -> String {
    // TRANSLATORS: %1 is a GNOME version: 51, 3.38.
    trf("Doesn't support GNOME %1", &[&shell])
}

/// Summary of the declared versions.
pub fn supports_label(summary: &str) -> String {
    // TRANSLATORS: %1 is a list of GNOME versions: 47–50, 3.38.
    trf("Supports GNOME %1", &[&summary])
}

/// "Version for your GNOME: 12".
pub fn release_for_you_label(release: &str) -> String {
    // TRANSLATORS: %1 is the extension's release: 12, 5.1.0.
    trf("Version for your GNOME: %1", &[&release])
}

/// The `unlock-dialog` session mode.
pub fn session_modes_label(modes: &[String]) -> String {
    if modes.iter().any(|m| m == "unlock-dialog") {
        tr("Also works on the lock screen")
    } else {
        String::new()
    }
}

/// The site does not answer (5xx).
pub fn site_down_message() -> String {
    tr("The GNOME extensions site isn't responding right now. Outages are shown at status.gnome.org — \
        try again later. Installed extensions keep working.")
}

/// Card license.
pub fn license_label() -> String {
    tr("GPL-compatible, as extensions.gnome.org requires")
}

// ═══════════════════════════════════════════════════════════════════════
//  Card
// ═══════════════════════════════════════════════════════════════════════

/// Author name: the display name, otherwise the account name, otherwise the site.
pub fn author_name(e: &Extension) -> String {
    [&e.creator.display_name, &e.creator.username]
        .into_iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
        .unwrap_or("extensions.gnome.org")
        .to_string()
}

/// Rating 0…5 on the store's 0…100 scale.
pub fn rating_percent(rating: f64) -> i64 {
    if rating.is_finite() {
        (rating.clamp(0.0, 5.0) * 20.0).round() as i64
    } else {
        0
    }
}

/// Site date (`2026-02-25T17:12:27.692638`) without fractions of a second.
fn iso(date: &str) -> String {
    let d = date.trim();
    match d.find('.') {
        Some(i) if d[..i].contains('T') => d[..i].to_string(),
        _ => d.to_string(),
    }
}

/// Extension card. There are no download links: the shell installs it. The
/// gallery holds only a real screenshot (the icon does not go there: a person
/// expects screenshots in the gallery, not a logo).
pub fn theme_item(e: &Extension) -> ThemeItem {
    let shot = ego_https(&e.screenshot);
    let preview = card_picture(e);
    let created = iso(&e.created);
    let updated = if e.updated.trim().is_empty() { created.clone() } else { iso(&e.updated) };
    ThemeItem {
        id: item_id(&e.uuid),
        name: if e.name.trim().is_empty() { e.uuid.clone() } else { e.name.trim().to_string() },
        description: e.description.trim().to_string(),
        author: author_name(e),
        license: license_label(),
        rating: rating_percent(e.rating),
        downloads: e.downloads.max(0),
        preview_url: preview,
        preview_urls: if shot.is_empty() { Vec::new() } else { vec![shot] },
        category_id: CATEGORY_ID,
        created,
        updated,
        download_links: Vec::new(),
        ..ThemeItem::default()
    }
}

/// Picture of a storefront tile: the screenshot, and without it the extension icon.
pub fn card_picture(e: &Extension) -> String {
    let shot = ego_https(&e.screenshot);
    if shot.is_empty() {
        ego_https(&e.icon)
    } else {
        shot
    }
}

/// Storefront pictures that have already arrived with EGO pages and cards:
/// uuid → address of the tile picture ([`card_picture`]). With it an installed
/// extension shows the same screenshot as its card in the storefront — without
/// its own request to the site (live check 15.11: a screenshot in the
/// storefront, a placeholder among the installed).
#[derive(Debug, Default)]
pub struct Pictures {
    by_uuid: HashMap<String, String>,
}

impl Pictures {
    /// How many addresses to keep: process memory, not an on-disk cache.
    const LIMIT: usize = 4096;

    /// Remember the pictures of a page (skip empty ones).
    pub fn remember(&mut self, items: &[Extension]) {
        if self.by_uuid.len() > Self::LIMIT {
            self.by_uuid.clear();
        }
        for e in items {
            let url = card_picture(e);
            if !e.uuid.is_empty() && !url.is_empty() {
                self.by_uuid.insert(e.uuid.clone(), url);
            }
        }
    }

    /// Address of the picture of extension `uuid`; `None` — the storefront did not show it.
    pub fn get(&self, uuid: &str) -> Option<&str> {
        self.by_uuid.get(uuid).map(String::as_str)
    }
}

/// Storefront pictures of this process ([`Pictures`]).
pub fn live_pictures() -> &'static Mutex<Pictures> {
    static PICTURES: OnceLock<Mutex<Pictures>> = OnceLock::new();
    PICTURES.get_or_init(|| Mutex::new(Pictures::default()))
}

fn remember_pictures(items: &[Extension]) {
    if let Ok(mut p) = live_pictures().lock() {
        p.remember(items);
    }
}

/// The tile's rating line: "★ 3.5 (17) · ⬇ 149.9K".
pub fn stats_line(e: &Extension) -> String {
    let dl = crate::appearance_page::human_count(e.downloads.max(0));
    if e.rated > 0 {
        format!(
            "★ {} ({}) · ⬇ {dl}",
            crate::i18n::format_decimal(e.rating.clamp(0.0, 5.0), 1),
            crate::appearance_page::human_count(e.rated)
        )
    } else {
        format!("⬇ {dl}")
    }
}

/// Tooltip of the rating line.
pub fn stats_tooltip() -> String {
    tr("Rating, number of votes and downloads on extensions.gnome.org")
}

/// A catalog answer row: the card plus what only EGO knows. `unsupported` — the
/// label of the shell version the extension does NOT support (per the check),
/// or `None`. Common keys of third-party stores (`sitePage`, `siteStats`) — as
/// for Wallhaven.
pub fn catalog_row(item: &ThemeItem, e: &Extension, unsupported: Option<&str>) -> Value {
    let mut v = serde_json::to_value(item).unwrap_or(Value::Null);
    if let Some(m) = v.as_object_mut() {
        let page = page_url(e);
        let donations: Vec<Value> =
            e.donation_urls.iter().map(|u| web_link(u)).filter(|u| !u.is_empty()).map(Value::from).collect();
        m.insert("egoUuid".into(), Value::from(e.uuid.clone()));
        m.insert("egoRating".into(), Value::from(if e.rating.is_finite() { e.rating.clamp(0.0, 5.0) } else { 0.0 }));
        m.insert("egoRated".into(), Value::from(e.rated.max(0)));
        m.insert("egoDownloads".into(), Value::from(e.downloads.max(0)));
        m.insert("egoUnsupported".into(), Value::from(unsupported.is_some()));
        m.insert(
            "egoUnsupportedText".into(),
            Value::from(unsupported.map(unsupported_label).unwrap_or_default()),
        );
        m.insert("egoPage".into(), Value::from(page.clone()));
        m.insert("egoHomepage".into(), Value::from(web_link(&e.url)));
        m.insert("egoIcon".into(), Value::from(ego_https(&e.icon)));
        m.insert("egoDonations".into(), Value::Array(donations));
        // Only the release knows the session modes — they are filled in by the
        // details ([`with_release`]).
        m.insert("egoSessionModes".into(), Value::Array(Vec::new()));
        m.insert("sitePage".into(), Value::from(page));
        m.insert("siteStats".into(), Value::from(stats_line(e)));
        m.insert("siteStatsTooltip".into(), Value::from(stats_tooltip()));
    }
    v
}

/// Supplement a catalog row with information about releases: "version for
/// you", the summary of GNOME versions, the session modes of the chosen
/// release.
pub fn with_release(row: &mut Value, releases: &[Release], v: Option<&ShellVersion>) {
    let Some(m) = row.as_object_mut() else { return };
    let chosen = v.and_then(|v| release_for(releases, v));
    let summary = supported_summary(releases);
    m.insert("egoSupports".into(), Value::from(if summary.is_empty() { String::new() } else { supports_label(&summary) }));
    m.insert("egoReleaseForYou".into(), Value::from(chosen.map(|r| r.version).unwrap_or(0)));
    m.insert(
        "egoReleaseForYouText".into(),
        Value::from(chosen.map(|r| release_for_you_label(&r.label())).unwrap_or_default()),
    );
    let modes: Vec<String> = chosen.map(|r| r.session_modes.clone()).unwrap_or_default();
    m.insert("egoSessionModesText".into(), Value::from(session_modes_label(&modes)));
    m.insert("egoSessionModes".into(), Value::Array(modes.into_iter().map(Value::from).collect()));
    if let Some(r) = chosen {
        m.insert("version".into(), Value::from(r.label()));
    }
    if let (Some(v), None) = (v, chosen) {
        m.insert("egoUnsupported".into(), Value::from(true));
        m.insert("egoUnsupportedText".into(), Value::from(unsupported_label(&shell_label(v))));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Answer memory and back-off after failures
// ═══════════════════════════════════════════════════════════════════════

/// Why there is no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// There was a 5xx answer recently: leave the site alone for another `wait_secs`.
    BackingOff { wait_secs: u64 },
    /// Network or server.
    Net(NetError),
    /// The answer cannot be parsed.
    Parse(String),
}

impl FetchError {
    /// The site said "no such thing" (404) — for an installed one this is not an
    /// error: the local information is shown.
    pub fn is_not_found(&self) -> bool {
        matches!(self, FetchError::Net(NetError::Status { code: 404, .. }))
    }

    /// Text for the status line.
    pub fn message(&self) -> String {
        match self {
            FetchError::BackingOff { .. } => site_down_message(),
            FetchError::Net(NetError::Status { code, .. }) if *code >= 500 => site_down_message(),
            FetchError::Net(e) => crate::ui_base::humanize_network_error(&e.to_string(), "extensions.gnome.org"),
            FetchError::Parse(_) => tr("The GNOME extensions site sent an answer Rexon can't read. Try again later."),
        }
    }
}

/// The session's answer memory and back-off after 5xx. Time is a parameter (ms),
/// so it can be tested without a clock.
#[derive(Debug, Default)]
pub struct Gate {
    failed_until: u64,
    cache: HashMap<String, (u64, u64, Vec<u8>)>, // address → (when, lifetime in ms, body)
    order: VecDeque<String>,
}

/// Lifetime of an answer by address: listings and search — short, the rest — an hour.
pub fn ttl_secs(url: &str) -> u64 {
    let path = url.split('?').next().unwrap_or(url);
    if path.ends_with("/extensions/") || path.contains("/extensions/search/") {
        LIST_TTL_SECS
    } else {
        DETAIL_TTL_SECS
    }
}

impl Gate {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh answer from memory.
    pub fn cached(&self, url: &str, now_ms: u64) -> Option<Vec<u8>> {
        let (at, ttl, body) = self.cache.get(url)?;
        (now_ms.saturating_sub(*at) < *ttl).then(|| body.clone())
    }

    /// Whether we may go to the network; if not — how many seconds to wait.
    pub fn admit(&self, now_ms: u64) -> Result<(), u64> {
        if now_ms < self.failed_until {
            Err((self.failed_until - now_ms).div_ceil(1000))
        } else {
            Ok(())
        }
    }

    /// The site answered with 5xx.
    pub fn failed(&mut self, now_ms: u64) {
        self.failed_until = now_ms + FAIL_BACKOFF_SECS * 1000;
    }

    /// Remember an answer.
    pub fn store(&mut self, url: &str, body: Vec<u8>, now_ms: u64) {
        let ttl = ttl_secs(url) * 1000;
        if self.cache.insert(url.to_string(), (now_ms, ttl, body)).is_none() {
            self.order.push_back(url.to_string());
        }
        while self.order.len() > CACHE_MAX_ENTRIES {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
        self.failed_until = 0;
    }
}

/// One request through [`Gate`]: memory (unless `force`), back-off (unless
/// `force`), network. `get` — the transport (in production
/// [`crate::net::UreqNet`], in tests a closure). The mutex is not held during
/// the network call.
pub fn fetch_with(
    gate: &Mutex<Gate>,
    url: &str,
    force: bool,
    now_ms: &dyn Fn() -> u64,
    get: &mut dyn FnMut(&str) -> Result<Vec<u8>, NetError>,
) -> Result<Vec<u8>, FetchError> {
    if !force {
        let g = gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(body) = g.cached(url, now_ms()) {
            return Ok(body);
        }
        g.admit(now_ms()).map_err(|wait_secs| FetchError::BackingOff { wait_secs })?;
    }
    match get(url) {
        Ok(body) => {
            if body.len() > MAX_RESPONSE_BYTES {
                return Err(FetchError::Parse("EGO: answer too large".to_string()));
            }
            gate.lock().unwrap_or_else(|e| e.into_inner()).store(url, body.clone(), now_ms());
            Ok(body)
        }
        Err(e) => {
            if matches!(e, NetError::Status { code, .. } if code >= 500) {
                gate.lock().unwrap_or_else(|e| e.into_inner()).failed(now_ms());
            }
            Err(FetchError::Net(e))
        }
    }
}

/// All releases of an extension: pages through while the server says "more"
/// (no more than [`VERSIONS_MAX_PAGES`]). An error on the first page is an
/// error; on a later one — what was gathered so far.
pub fn collect_releases(
    uuid: &str,
    mut fetch: impl FnMut(&str) -> Result<Vec<u8>, FetchError>,
) -> Result<Vec<Release>, FetchError> {
    let mut all = Vec::new();
    for n in 1..=VERSIONS_MAX_PAGES as i64 {
        let url = versions_url(uuid, n).ok_or_else(|| FetchError::Parse("EGO: bad uuid".to_string()))?;
        let page = match fetch(&url).and_then(|b| parse_versions(&b).map_err(FetchError::Parse)) {
            Ok(p) => p,
            Err(e) if n == 1 => return Err(e),
            Err(_) => break,
        };
        all.extend(page.items);
        if !page.has_more {
            break;
        }
    }
    Ok(all)
}

/// Process memory — one for all pages of "Appearance".
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

/// A storefront page from the live API.
pub fn live_page(
    query: &str,
    sort: &str,
    page: i64,
    size: i64,
    shell: Option<&str>,
    force: bool,
    token: &crate::net::CancelToken,
) -> Result<ExtPage, FetchError> {
    let url = listing_url(query, sort, page, size, shell);
    let body = fetch_with(live_gate(), &url, force, &now_ms, &mut live_get(token))?;
    let page = parse_listing(&body).map_err(FetchError::Parse)?;
    remember_pictures(&page.items);
    Ok(page)
}

/// Which of `uuids` fit shell `shell` (checked in batches).
pub fn live_compatible(
    uuids: &[&str],
    shell: &str,
    token: &crate::net::CancelToken,
) -> Result<HashSet<String>, FetchError> {
    let mut get = live_get(token);
    let mut ok = HashSet::new();
    for url in compat_batch_urls(uuids, shell) {
        let body = fetch_with(live_gate(), &url, false, &now_ms, &mut get)?;
        ok.extend(parse_listing(&body).map_err(FetchError::Parse)?.items.into_iter().map(|e| e.uuid));
    }
    Ok(ok)
}

/// A card from the live API.
pub fn live_detail(uuid: &str, token: &crate::net::CancelToken) -> Result<Extension, FetchError> {
    let url = detail_url(uuid).ok_or_else(|| FetchError::Parse("EGO: bad uuid".to_string()))?;
    let body = fetch_with(live_gate(), &url, false, &now_ms, &mut live_get(token))?;
    let e = parse_detail(&body).map_err(FetchError::Parse)?;
    remember_pictures(std::slice::from_ref(&e));
    Ok(e)
}

/// Address of the extension information by the number from a site page
/// (`/extension/<pk>/…`): API v1 searches only by uuid, while links in foreign
/// text carry the number.
pub fn info_by_pk_url(pk: u64) -> String {
    format!("{HOME_URL}/extension-info/?pk={pk}")
}

/// uuid from the `extension-info` answer.
pub fn parse_info_uuid(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let uuid = v.get("uuid")?.as_str()?;
    valid_uuid(uuid).then(|| uuid.to_string())
}

/// Extension uuid by the number from a site page.
pub fn live_uuid_by_pk(pk: u64, token: &crate::net::CancelToken) -> Result<String, FetchError> {
    let body = fetch_with(live_gate(), &info_by_pk_url(pk), false, &now_ms, &mut live_get(token))?;
    parse_info_uuid(&body).ok_or_else(|| FetchError::Parse("EGO: нет uuid в extension-info".to_string()))
}

/// Releases from the live API.
pub fn live_releases(uuid: &str, token: &crate::net::CancelToken) -> Result<Vec<Release>, FetchError> {
    let mut get = live_get(token);
    collect_releases(uuid, |url| fetch_with(live_gate(), url, false, &now_ms, &mut get))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_by_site_number() {
        assert_eq!(info_by_pk_url(615), "https://extensions.gnome.org/extension-info/?pk=615");
        // A site answer (measured 16.11.2026), shortened.
        let body = br#"{"uuid": "appindicatorsupport@rgcjonas.gmail.com", "name": "AppIndicator", "pk": 615}"#;
        assert_eq!(parse_info_uuid(body).as_deref(), Some("appindicatorsupport@rgcjonas.gmail.com"));
        assert_eq!(parse_info_uuid(br#"{"uuid": "../x"}"#), None);
        assert_eq!(parse_info_uuid(b"<html>"), None);
    }

    fn v(raw: &str) -> ShellVersion {
        parse_shell_version(raw).expect(raw)
    }

    fn rel(version: i64, status: i64, shells: &[(i64, i64)]) -> Release {
        Release {
            version,
            status,
            shell_versions: shells.iter().map(|&(major, minor)| ReleaseShell { major, minor, point: -1 }).collect(),
            ..Release::default()
        }
    }

    #[test]
    fn shell_version_to_site_form() {
        assert_eq!(site_shell_version(&v("51.1")), "51");
        assert_eq!(site_shell_version(&v("51")), "51");
        assert_eq!(site_shell_version(&v("51.beta")), "51.beta");
        assert_eq!(site_shell_version(&v("51.alpha.2")), "51.alpha");
        assert_eq!(site_shell_version(&v("52.RC")), "52.rc");
        assert_eq!(site_shell_version(&v("3.38.4")), "3.38");
        assert_eq!(site_shell_version(&v("GNOME Shell 47.2")), "47");
        assert_eq!(shell_label(&v("51.beta")), "51");
        for bad in ["", "abc", "3", "39.1", "0", "-1", "51.x"] {
            assert_eq!(parse_shell_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn listing_and_search_urls() {
        assert_eq!(
            listing_url("", "popular", 0, 20, Some("51")),
            "https://extensions.gnome.org/api/v1/extensions/?status=3&ordering=-popularity&page=1&page_size=20&shell_version=51"
        );
        let u = listing_url("  dash  to/dock & co ", "down", 2, 20, None);
        assert_eq!(
            u,
            "https://extensions.gnome.org/api/v1/extensions/search/dash%20to%20dock%20%26%20co/?ordering=-downloads&page=3&page_size=20"
        );
        assert!(!u.contains("status="), "у поиска нет параметра status");
        assert!(listing_url("", "updated", 0, 500, None).contains("ordering=-updated&page=1&page_size=100"));
        assert!(listing_url("", "new", -3, 0, Some("51.beta")).contains("ordering=-created&page=1&page_size=1&shell_version=51.beta"));
        // Foreign sort keys become "Popular".
        assert_eq!(api_ordering("alpha"), "-popularity");
        assert_eq!(api_ordering("high"), "-popularity");
    }

    #[test]
    fn detail_versions_zip_and_ids() {
        assert!(valid_uuid("dash-to-dock@micxgx.gmail.com"));
        assert!(valid_uuid("a+b@x"));
        for bad in ["", "../x", ".hidden@x", "a/b@x", "a b@x", "a..b@x", "a?b"] {
            assert!(!valid_uuid(bad), "{bad}");
        }
        assert_eq!(
            detail_url("demo@rexon.example").as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/")
        );
        assert_eq!(detail_url("a/b"), None);
        assert_eq!(
            versions_url("demo@rexon.example", 2).as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/versions/?page=2&page_size=100")
        );
        assert_eq!(
            release_zip_url("demo@rexon.example", 7).as_deref(),
            Some("https://extensions.gnome.org/api/v1/extensions/demo%40rexon.example/versions/7/?format=zip")
        );
        assert_eq!(release_zip_url("demo@rexon.example", 0), None);
        assert_eq!(parse_item_id(&item_id("demo@rexon.example")), Some("demo@rexon.example"));
        assert_eq!(parse_item_id("ego:../x"), None);
        assert_eq!(parse_item_id("wallhaven:abc"), None);
        assert!(has_section("gnomeext") && !has_section("wallpapers"));
    }

    #[test]
    fn batches_of_fifty() {
        let names: Vec<String> = (0..120).map(|i| format!("ext{i}@rexon.example")).collect();
        let mut refs: Vec<&str> = names.iter().map(String::as_str).collect();
        refs.push("ext0@rexon.example"); // a duplicate
        refs.push("../bad");
        let urls = compat_batch_urls(&refs, "51");
        assert_eq!(urls.len(), 3);
        let counts: Vec<usize> = urls.iter().map(|u| u.matches("&uuid=").count()).collect();
        assert_eq!(counts, vec![50, 50, 20]);
        assert!(urls[0].starts_with("https://extensions.gnome.org/api/v1/extensions/?page_size=50&shell_version=51&uuid=ext0%40rexon.example"));
        assert!(!urls.iter().any(|u| u.contains("bad")));
        assert!(compat_batch_urls(&[], "51").is_empty());
        let ok: HashSet<String> = ["a@x".to_string()].into();
        assert_eq!(unsupported_uuids(&["a@x", "b@x"], &ok), ["b@x".to_string()].into());
    }

    #[test]
    fn picks_newest_active_release_for_shell() {
        let rs = vec![
            rel(1, STATUS_ACTIVE, &[(45, -1), (46, -1)]),
            rel(2, STATUS_ACTIVE, &[(50, -1), (51, -1)]),
            rel(3, 1, &[(51, -1)]),                    // rejected
            rel(4, STATUS_ACTIVE, &[(52, -1)]),
        ];
        assert_eq!(release_for(&rs, &v("51.1")).map(|r| r.version), Some(2));
        assert_eq!(release_for(&rs, &v("46")).map(|r| r.version), Some(1));
        assert_eq!(release_for(&rs, &v("47")), None);
        let old = vec![rel(9, STATUS_ACTIVE, &[(3, 36), (3, 38)]), rel(10, STATUS_ACTIVE, &[(3, -1)])];
        assert_eq!(release_for(&old, &v("3.38.1")).map(|r| r.version), Some(10));
        assert_eq!(release_for(&old[..1], &v("3.34")), None);
    }

    #[test]
    fn summary_compresses_runs() {
        let rs = vec![
            rel(1, STATUS_ACTIVE, &[(3, 36), (3, 38), (40, -1), (41, -1)]),
            rel(2, STATUS_ACTIVE, &[(45, -1)]),
            rel(3, STATUS_ACTIVE, &[(47, -1), (48, -1), (49, -1), (50, -1)]),
            rel(4, 1, &[(51, -1)]), // a rejected one does not go into the summary
        ];
        assert_eq!(supported_summary(&rs), "3.36–41, 45, 47–50");
        assert_eq!(supported_summary(&[rel(1, STATUS_ACTIVE, &[(3, 3), (3, 4), (51, -1)])]), "3.3, 3.4, 51");
        // Not a single active one — by all.
        assert_eq!(supported_summary(&[rel(1, 0, &[(50, -1), (51, -1)])]), "50–51");
        assert_eq!(supported_summary(&[]), "");
    }

    #[test]
    fn release_rows_mark_chosen_and_installed() {
        let rs = vec![rel(1, STATUS_ACTIVE, &[(50, -1)]), rel(2, STATUS_ACTIVE, &[(51, -1)]), rel(3, 2, &[(51, -1)])];
        let rows = release_rows(&rs, Some(&v("51")), Some(1));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["version"], 2);
        assert_eq!(rows[0]["forYou"], true);
        assert_eq!(rows[0]["shells"], "51");
        assert_eq!(rows[1]["installed"], true);
        assert_eq!(rows[1]["forYou"], false);
    }

    #[test]
    fn nulls_and_garbage_are_tolerated() {
        let body = br#"{"count":"3","next":null,"previous":null,"results":[
            {"id":1,"uuid":"a@rexon.example","name":null,"creator":null,"description":null,
             "created":null,"updated":null,"downloads":null,"popularity":"5","screenshot":null,
             "icon":null,"rating":null,"rated":null,"url":null,"donation_urls":null,"link":null},
            {"id":2,"uuid":"../evil"},
            "not an object",
            {"uuid":"b@rexon.example","donation_urls":["https://x.example/d", 5, null],"rating":"4.5","unknown_new_field":{"a":1}}
        ]}"#;
        let p = parse_listing(body).unwrap();
        assert_eq!((p.total, p.has_more, p.items.len()), (3, false, 2));
        let a = &p.items[0];
        assert_eq!((a.popularity, a.rating, a.creator.id), (5, 0.0, 0));
        let it = theme_item(a);
        assert_eq!(it.name, "a@rexon.example");
        assert_eq!(it.author, "extensions.gnome.org");
        assert!(it.preview_url.is_empty() && it.preview_urls.is_empty());
        assert_eq!(p.items[1].donation_urls, vec!["https://x.example/d".to_string()]);
        assert_eq!(p.items[1].rating, 4.5);
        assert!(parse_listing(b"<html>").is_err());
        assert!(parse_listing(br#"{"detail":"No Extension matches the given query."}"#).is_err());
        assert!(parse_detail(br#"{"detail":"No Extension matches the given query."}"#).is_err());
        let vs = parse_versions(br#"{"count":2,"next":"https://x","results":[
            {"version":3,"version_name":null,"status":3,"shell_versions":null,"session_modes":null},
            {"version":null}]}"#)
        .unwrap();
        assert_eq!((vs.items.len(), vs.has_more), (1, true));
        assert_eq!(vs.items[0].label(), "3");
    }

    #[test]
    fn image_hosts_and_links() {
        assert_eq!(ego_https("https://extensions.gnome.org/extension-data/s.webp"), "https://extensions.gnome.org/extension-data/s.webp");
        for bad in [
            "http://extensions.gnome.org/x.png",
            "https://extensions.gnome.org.evil.example/x.png",
            "https://evil.example/https://extensions.gnome.org/x.png",
            "https://extensions.gnome.org/../x.png",
            "/extension-data/x.png",
        ] {
            assert_eq!(ego_https(bad), "", "{bad}");
        }
        let mut e = Extension { id: 42, link: "/extension/42/demo/".into(), ..Extension::default() };
        assert_eq!(page_url(&e), "https://extensions.gnome.org/extension/42/demo/");
        e.link = "https://evil.example/".into();
        assert_eq!(page_url(&e), "https://extensions.gnome.org/extension/42/");
        assert_eq!(web_link("javascript:alert(1)"), "");
    }

    #[test]
    fn card_and_catalog_row() {
        let e = Extension {
            id: 7,
            uuid: "demo@rexon.example".into(),
            name: "Demo".into(),
            creator: Creator { id: 1, username: "demo_user".into(), display_name: " ".into() },
            description: "Line one\nLine two".into(),
            created: "2026-01-02T03:04:05.123456".into(),
            downloads: 149_933,
            screenshot: "https://extensions.gnome.org/extension-data/screenshots/demo.gif".into(),
            icon: "https://extensions.gnome.org/extension-data/icons/demo.png".into(),
            rating: 3.47,
            rated: 17,
            url: "https://example.org/demo".into(),
            donation_urls: vec!["https://example.org/give".into(), "ftp://nope".into()],
            link: "/extension/7/demo/".into(),
            ..Extension::default()
        };
        let it = theme_item(&e);
        assert_eq!(it.id, "ego:demo@rexon.example");
        assert_eq!(it.category_id, CATEGORY_ID);
        assert_eq!(it.author, "demo_user");
        assert_eq!(it.rating, 69);
        assert_eq!(it.created, "2026-01-02T03:04:05");
        assert_eq!(it.updated, it.created, "updated = null — берётся дата создания");
        assert!(it.download_links.is_empty(), "ставит оболочка");
        assert_eq!(it.preview_urls, vec![e.screenshot.clone()]);
        let mut row = catalog_row(&it, &e, Some("51"));
        assert_eq!(row["egoRated"], 17);
        assert_eq!(row["egoDownloads"], 149_933);
        assert_eq!(row["egoUnsupported"], true);
        assert_eq!(row["egoUnsupportedText"], "Doesn't support GNOME 51");
        assert_eq!(row["egoPage"], "https://extensions.gnome.org/extension/7/demo/");
        assert_eq!(row["egoDonations"], serde_json::json!(["https://example.org/give"]));
        // The decimal separator follows the language (`format_decimal`).
        let expect = format!("★ {} (17) · ⬇ {}", crate::i18n::format_decimal(3.47, 1), crate::appearance_page::human_count(149_933));
        assert_eq!(row["siteStats"], expect.as_str());
        assert_eq!(catalog_row(&it, &e, None)["egoUnsupported"], false);

        let mut r = rel(5, STATUS_ACTIVE, &[(50, -1), (51, -1)]);
        r.version_name = "5.1.0".into();
        r.session_modes = vec!["unlock-dialog".into()];
        let releases = vec![rel(4, STATUS_ACTIVE, &[(49, -1)]), r];
        with_release(&mut row, &releases, Some(&v("51.1")));
        assert_eq!(row["egoReleaseForYou"], 5);
        assert_eq!(row["egoReleaseForYouText"], "Version for your GNOME: 5.1.0");
        assert_eq!(row["egoSupports"], "Supports GNOME 49–51");
        assert_eq!(row["egoSessionModes"], serde_json::json!(["unlock-dialog"]));
        assert_eq!(row["egoSessionModesText"], "Also works on the lock screen");
        assert_eq!(row["version"], "5.1.0");
        // There is no fitting release for 52 — a mark.
        let mut row2 = catalog_row(&it, &e, None);
        with_release(&mut row2, &releases, Some(&v("52")));
        assert_eq!(row2["egoUnsupportedText"], "Doesn't support GNOME 52");
        assert_eq!(row2["egoReleaseForYou"], 0);
    }

    /// Memory of storefront pictures: a screenshot, without it the icon; without a
    /// picture and without a uuid — not remembered; the address is the same as for
    /// the storefront tile.
    #[test]
    fn pictures_remember_showcase_tiles() {
        let shot = Extension {
            uuid: "a@x".into(),
            screenshot: "https://extensions.gnome.org/extension-data/screenshots/a.png".into(),
            icon: "https://extensions.gnome.org/extension-data/icons/a.png".into(),
            ..Extension::default()
        };
        let icon_only = Extension { uuid: "b@x".into(), icon: "https://extensions.gnome.org/extension-data/icons/b.png".into(), ..Extension::default() };
        let bare = Extension { uuid: "c@x".into(), ..Extension::default() };
        let nameless = Extension { screenshot: "https://extensions.gnome.org/s.png".into(), ..Extension::default() };
        let mut p = Pictures::default();
        p.remember(&[shot.clone(), icon_only.clone(), bare, nameless]);
        assert_eq!(p.get("a@x"), Some(theme_item(&shot).preview_url.as_str()));
        assert!(p.get("a@x").unwrap().contains("screenshots"));
        assert_eq!(p.get("b@x"), Some(theme_item(&icon_only).preview_url.as_str()));
        assert_eq!(p.get("c@x"), None);
        // A foreign site does not become a picture (as for the storefront tile).
        p.remember(&[Extension { uuid: "d@x".into(), screenshot: "https://evil.example/x.png".into(), ..Extension::default() }]);
        assert_eq!(p.get("d@x"), None);
        assert_eq!(p.by_uuid.len(), 2);
    }

    #[test]
    fn cache_ttls_and_backoff_after_5xx() {
        let gate = Mutex::new(Gate::new());
        let clock = std::cell::Cell::new(1_000u64);
        let now = || clock.get();
        let calls = std::cell::Cell::new(0);
        let mut ok = |_: &str| {
            calls.set(calls.get() + 1);
            Ok(br#"{"results":[]}"#.to_vec())
        };
        let list = listing_url("", "popular", 0, 20, None);
        let card = detail_url("demo@rexon.example").unwrap();
        assert_eq!(ttl_secs(&list), LIST_TTL_SECS);
        assert_eq!(ttl_secs(&listing_url("dock", "popular", 0, 20, None)), LIST_TTL_SECS);
        assert_eq!(ttl_secs(&card), DETAIL_TTL_SECS);
        assert_eq!(ttl_secs(&versions_url("demo@rexon.example", 1).unwrap()), DETAIL_TTL_SECS);
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &card, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 2, "повтор — из памяти");
        clock.set(1_000 + LIST_TTL_SECS * 1000);
        fetch_with(&gate, &list, false, &now, &mut ok).unwrap();
        fetch_with(&gate, &card, false, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 3, "листинг протух, карточка ещё жива");
        fetch_with(&gate, &card, true, &now, &mut ok).unwrap();
        assert_eq!(calls.get(), 4, "«Обновить» идёт в сеть");

        let mut down = |_: &str| Err(NetError::Status { code: 503, reason: "Service Unavailable".into() });
        let e = fetch_with(&gate, "u1", false, &now, &mut down).unwrap_err();
        assert_eq!(e.message(), site_down_message());
        let mut never = |_: &str| -> Result<Vec<u8>, NetError> { panic!("сеть во время отступа") };
        assert_eq!(
            fetch_with(&gate, "u2", false, &now, &mut never).unwrap_err(),
            FetchError::BackingOff { wait_secs: FAIL_BACKOFF_SECS }
        );
        // Memory works during the back-off.
        assert!(fetch_with(&gate, &card, false, &now, &mut never).is_ok());
        // "Refresh" tries right away.
        assert!(fetch_with(&gate, "u3", true, &now, &mut ok).is_ok());
        // A successful answer removes the back-off.
        assert!(fetch_with(&gate, "u4", false, &now, &mut ok).is_ok());

        let mut missing = |_: &str| Err(NetError::Status { code: 404, reason: "Not Found".into() });
        let e = fetch_with(&gate, "u5", false, &now, &mut missing).unwrap_err();
        assert!(e.is_not_found());
        assert!(fetch_with(&gate, "u6", false, &now, &mut ok).is_ok(), "404 отступа не включает");
    }

    #[test]
    fn collects_release_pages() {
        let mut asked = Vec::new();
        let rs = collect_releases("demo@rexon.example", |url| {
            asked.push(url.to_string());
            let n = if url.contains("page=1&") { 1 } else { 2 };
            let next = if n == 1 { "\"https://next\"" } else { "null" };
            Ok(format!(r#"{{"count":2,"next":{next},"results":[{{"version":{n},"status":3}}]}}"#).into_bytes())
        })
        .unwrap();
        assert_eq!(asked.len(), 2);
        assert_eq!(rs.iter().map(|r| r.version).collect::<Vec<_>>(), vec![1, 2]);
        let e = collect_releases("demo@rexon.example", |_| Err(FetchError::Parse("x".into()))).unwrap_err();
        assert_eq!(e, FetchError::Parse("x".into()));
        assert!(collect_releases("../x", |_| unreachable!()).is_err());
    }
}
