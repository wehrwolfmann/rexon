//! OCS API client of the "Appearance" catalog — a port of
//! `rexon/appearance/ocs_client.py` IN FULL: both response parsing and the
//! network layer.
//!
//! Parsing: OCS XML responses → models (`parse_list`/`parse_content`/
//! `parse_comments`), choosing the best download link for Linux
//! (`link_score`/`pick_best_link`), URL normalization (`safe_url`, a port of
//! `urllib.parse.urlsplit`+`quote`+`urlunsplit`).
//!
//! Network: [`http_get`]/[`http_download`] (port of `_http_get`/`_http_download`)
//! and [`OcsClient`] (port of the `OCSClient` class). The call itself is hidden
//! behind the [`NetIo`] trait (`net.rs`), the SQLite cache behind [`OcsCacheIo`],
//! so the "cache → network → stale cache" logic is tested on RECORDED responses,
//! not on the live api.opendesktop.org. The former decision "HTTP stays in
//! Python" is cancelled: the goal is to remove Python from everywhere except the
//! interface.
//!
//! Parity with Python: references were captured from `ocs_client.py` itself
//! (`_parse_list`/`_flatten_comments`/`_safe_url`/`_link_score`/`_pick_best_link`
//! on a REAL api.opendesktop.org response captured with curl; the network
//! branches — with `urlopen`/cache/file operations monkeypatched), and Rust must
//! return the same models, the same strings and the same order. The golden tests
//! are `tests/ocs_parity.rs` (parsing) and `tests/net_parity.rs` (network
//! branches).

use crate::models::ThemeItem;
use crate::net::{self, LogLevel, NetError, NetIo};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use serde_json::{json, Value};

/// Base URL of the OCS API (matches `API_BASE`).
pub const API_BASE: &str = "https://api.opendesktop.org/ocs/v1";
/// User-Agent of requests (`USER_AGENT`).
pub const USER_AGENT: &str = "Rexon/5.0";
/// Request timeout, seconds (`TIMEOUT`).
pub const TIMEOUT_SECS: u64 = 25;

/// Timeout for downloading a theme archive, seconds (`_http_download`, the
/// default value of the `timeout` argument).
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// Size ceiling of a theme archive, bytes (the default `max_size`, 600 MiB).
pub const DOWNLOAD_MAX_SIZE: i64 = 600 * 1024 * 1024;

/// Timeout for downloading a preview, seconds (`get_preview_image`).
pub const PREVIEW_TIMEOUT_SECS: u64 = 20;

/// Size ceiling of a preview, bytes (20 MiB).
pub const PREVIEW_MAX_SIZE: i64 = 20 * 1024 * 1024;

/// How long a product's FULL details live in the cache for DISPLAY, seconds (a
/// day, the original's `DETAIL_TTL`).
///
/// A pling detail response took up to 20 s, and for that long a person looked at
/// an empty panel after opening a product from an already shown grid; a theme's
/// description does not change within a day. Installation does NOT use this
/// period: it needs fresh archive links and calls [`OcsClient::get_item`].
pub const DETAIL_TTL_SECS: u64 = 24 * 3600;

/// Chunk size for streaming download (`resp.read(65536)`).
pub const DOWNLOAD_CHUNK: usize = 65536;

/// Allowed sort modes — the keys and values of `SORT_MAP` (in OCS they
/// coincide; the mapping is only needed to cut off garbage from the UI combo
/// box).
pub const SORT_MODES: &[&str] = &["new", "down", "high", "alpha"];

/// Archive extensions the installer can unpack (`_ARCHIVE_EXTS`).
/// The score OCS gives a product WITHOUT A SINGLE vote (`<score>50</score>`).
///
/// The API does not return vote counts at all: neither in the short list
/// (`content/data?...`, `details="summary"`) nor in the full card
/// (`content/data/<id>`, `details="full"`) — there is only `score` 0–100
/// (measured on api.kde-look.org on 16.11.2026: all new items are exactly 50,
/// products with votes are 58, 83, 88…). The store's starting score is the
/// middle of the scale, and showing it as "★ 2.5" would pass the absence of
/// votes off as a mediocre review. So exactly 50 reads as "no ratings". The cost
/// of this assumption: a product whose votes averaged out at exactly 50 will
/// also show without a rating.
pub const UNRATED_SCORE: i64 = 50;

/// The order matters: `_link_score` takes the FIRST match (`.tar.gz` before `.tar`).
pub const ARCHIVE_EXTS: &[&str] = &[
    ".tar.xz", ".tar.zst", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.lz",
    ".tar", ".zip", ".7z",
];

/// Ready KPackage packages: a zip inside, the unpacker takes them by contents.
/// That is how widgets (`.plasmoid`), KWin scripts (`.kwinscript`) and other
/// Plasma packages (`.kpackage`) are published — exactly what Discover installs.
/// Until 05.10.2026 they were unknown, and for the "HTML Widget Host" product,
/// with equal scores, the README.md that came first was chosen.
pub const KPACKAGE_EXTS: &[&str] = &[".plasmoid", ".kpackage", ".kwinscript"];

/// Distribution packages and installers: they are installed by their own
/// manager for one system rather than laid out into a theme directory.
/// `.pkg.tar.zst` is the same tar by extension, and until 05.10.2026 it got
/// archive points: for the "Wallpaper Engine" plugin a pacman package was chosen
/// instead of the ready KPackage `com.github.catsout.wallpaperEngineKde.tar.gz`
/// lying next to it.
pub const DISTRO_PACKAGE_EXTS: &[&str] = &[
    ".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar", ".deb", ".rpm",
    ".appimage", ".snap", ".flatpak", ".flatpakref", ".apk", ".exe", ".msi",
    ".dmg", ".pkg",
];

/// Text attached to a product (description, license, change log) — nothing to install.
pub const DOC_EXTS: &[&str] = &[".md", ".txt", ".pdf", ".html", ".htm", ".rst"];

/// Penalty for a distribution package/installer (see [`DISTRO_PACKAGE_EXTS`]):
/// below any archive and ready package, but above bare text.
const DISTRO_PACKAGE_PENALTY: i64 = 150;

/// Penalty for text (README and the like): the lowest score of all, so that it
/// does not beat anything that can be installed at all.
const DOC_PENALTY: i64 = 1000;

/// Bonus for a ready KPackage package for a kind installed by KPackage (widgets,
/// wallpaper plugins, KWin scripts and effects…): such a package is exactly what
/// Discover installs, it is taken before other archives.
const KPACKAGE_KIND_BONUS: i64 = 30;

/// File name words with which authors mark an ADD-ON to a product's main
/// archive: icons, wallpapers, fonts "as a bonus" (`Extra 1.4.tar.xz`,
/// `extra-Icons.zip`, `Extra's.tar.xz`). Whole words of the name are compared
/// (the separator is any character other than a letter or digit).
///
/// Never: defect of 17.10.2026: for the McOS products (GTK 135, Cinnamon 133)
/// the files come alphabetically, "Extra 1.4.tar.xz" first, all archives have
/// equal scores, and the add-ons archive was installed instead of the theme.
pub const SECONDARY_FILE_WORDS: &[&str] = &["extra", "extras", "addon", "addons", "bonus"];

/// Penalty for an add-on ([`SECONDARY_FILE_WORDS`]): below any main archive, but
/// above a non-archive — if the product has no other archives, it is taken.
const SECONDARY_FILE_PENALTY: i64 = 30;

/// Bonus for a file whose name (without extension, case-insensitive, only
/// letters and digits) matches the product title — and a smaller one if the
/// title is contained in the name. Smaller than any of the class differences
/// above: it is only a choice among equivalent archives (`Rage-Cursor` for the
/// product "Rage cursor", not the alphabetically first `Rage-Blue-Cursor`).
const TITLE_EXACT_BONUS: i64 = 8;
const TITLE_CONTAINED_BONUS: i64 = 5;

/// Markers of files NOT for Linux (Windows/macOS builds) — `_NON_LINUX_MARKERS`.
pub const NON_LINUX_MARKERS: &[&str] = &[
    "windows", "win64", "win32", "-win", "win-", "_win", ".ani", ".cur",
    ".exe", ".msi", ".dmg", "macos", "-mac", "mac-", "osx",
];

/// Result of parsing one `<content>` — a `ThemeItem` plus the total number of
/// comments. In Python `comment_count` is a dynamic attribute on the dataclass
/// (`item.comment_count = num("comments")`) that is not in `dataclasses.fields`;
/// here it is moved next to it so that `ThemeItem` stays a byte-for-byte port of
/// the model.
#[derive(Debug, Clone, PartialEq)]
pub struct OcsContent {
    pub item: ThemeItem,
    pub comment_count: i64,
}

/// One comment (flat, with a nesting depth) — port of the dictionary from
/// `_flatten_comments`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcsComment {
    pub author: String,
    pub date: String,
    pub subject: String,
    pub text: String,
    pub score: String,
    pub depth: i64,
}

// ── XML DOM (an analogue of xml.etree.ElementTree for the needed operations) ─

/// A minimal tree node: the tag name, the direct text and the direct children.
///
/// `text` — the concatenation of the node's direct text; for leaf tags (and it is
/// exactly on them that `findtext` is called) this coincides with
/// `ElementTree.text`.
struct Node {
    tag: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn new(tag: &str) -> Self {
        Node { tag: tag.to_string(), text: String::new(), children: Vec::new() }
    }

    /// The first direct child with the given tag (`Element.find`).
    fn child(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// All direct children with the given tag (`Element.findall`).
    fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.tag == tag)
    }

    /// Text of the first child (`Element.findtext`): `None` — no tag, `Some("")` —
    /// the tag exists but is empty (as in ET).
    fn findtext(&self, tag: &str) -> Option<&str> {
        self.child(tag).map(|c| c.text.as_str())
    }
}

/// Build a DOM from XML. `Err(())` — a parsing error (the analogue of
/// `ET.ParseError`, to which Python answers with an empty result).
fn parse_dom(xml: &str) -> Result<Node, ()> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node::new("__root__")];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                stack.push(Node::new(&tag));
            }
            Ok(Event::End(_)) => {
                // the root (__root__) is not closed by a tag — we guard with len>1.
                if stack.len() > 1 {
                    let node = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                stack.last_mut().unwrap().children.push(Node::new(&tag));
            }
            Ok(Event::Text(e)) => {
                // In quick-xml 0.39 entities come as separate GeneralRef events,
                // so Text is already raw literal text (without &..;).
                let t = e.decode().map_err(|_| ())?;
                stack.last_mut().unwrap().text.push_str(&t);
            }
            Ok(Event::GeneralRef(e)) => {
                // A numeric reference (&#NN; / &#xNN;) or a predefined entity
                // (lt/gt/amp/quot/apos) — ET resolves exactly these; an unknown named
                // entity is a ParseError in ET → Err for us.
                let ch = match e.resolve_char_ref().map_err(|_| ())? {
                    Some(c) => c,
                    None => match e.decode().map_err(|_| ())?.as_ref() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => return Err(()),
                    },
                };
                stack.last_mut().unwrap().text.push(ch);
            }
            Ok(Event::CData(e)) => {
                let s = String::from_utf8_lossy(e.as_ref()).into_owned();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(stack.pop().unwrap())
}

/// `(findtext(tag) or default).strip()` — an exact port of the `txt` helper.
fn txt(node: &Node, tag: &str, default: &str) -> String {
    let s = match node.findtext(tag) {
        Some(t) if !t.is_empty() => t,
        _ => default,
    };
    s.trim().to_string()
}

/// `int(txt(tag,"0") or "0")` in `try/except ValueError -> 0` — port of `num`.
fn num(node: &Node, tag: &str) -> i64 {
    let s = txt(node, tag, "0");
    let s = if s.is_empty() { "0" } else { s.as_str() };
    s.parse::<i64>().unwrap_or(0)
}

// ── Choosing the best download link ────────────────────────────────────────

/// Fitness score of a link for installing a Linux theme — port of `_link_score`,
/// without knowledge of the product kind (see [`link_score_for`]).
pub fn link_score(link: &Value) -> i64 {
    link_score_for(link, false)
}

/// Fitness score of a link; `kpackage_kind` — a product of a kind installed by
/// KPackage (then a ready `.plasmoid`/`.kpackage`/`.kwinscript` gets a bonus).
///
/// Scale: archive +100 (tar another +10), ready KPackage +110, non-archive
/// −100; a Windows/macOS marker −200; a distribution package/installer −150;
/// text (README, `.md`, `.txt`) −1000 — below everything else.
pub fn link_score_for(link: &Value, kpackage_kind: bool) -> i64 {
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    // hay = (name or url or "").lower()
    let hay = if !name.is_empty() {
        name
    } else if !url.is_empty() {
        url
    } else {
        ""
    }
    .to_lowercase();

    let mut score = 0i64;
    if KPACKAGE_EXTS.iter().any(|e| hay.ends_with(e)) {
        score += 110;
        if kpackage_kind {
            score += KPACKAGE_KIND_BONUS;
        }
    } else {
        match ARCHIVE_EXTS.iter().find(|e| hay.ends_with(**e)) {
            Some(ext) => {
                score += 100;
                if ext.starts_with(".tar") {
                    score += 10;
                }
            }
            None => score -= 100,
        }
    }
    if NON_LINUX_MARKERS.iter().any(|m| hay.contains(m)) {
        score -= 200;
    }
    if DISTRO_PACKAGE_EXTS.iter().any(|e| hay.ends_with(e)) {
        score -= DISTRO_PACKAGE_PENALTY;
    }
    let file = hay.rsplit('/').next().unwrap_or("");
    if DOC_EXTS.iter().any(|e| hay.ends_with(e)) || file.starts_with("readme") {
        score -= DOC_PENALTY;
    }
    if file.split(|c: char| !c.is_alphanumeric()).any(|w| SECONDARY_FILE_WORDS.contains(&w)) {
        score -= SECONDARY_FILE_PENALTY;
    }
    score
}

/// Only letters and digits, in lower case: "Rage cursor" and "Rage-Cursor" are
/// compared as one.
fn alnum_lower(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Bonus for a file name matching the product title ([`TITLE_EXACT_BONUS`],
/// [`TITLE_CONTAINED_BONUS`]); a short title (fewer than three characters) is not
/// compared.
pub fn title_bonus(link: &Value, title: &str) -> i64 {
    let want = alnum_lower(title);
    if want.chars().count() < 3 {
        return 0;
    }
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    let file = if name.is_empty() { url } else { name }.rsplit('/').next().unwrap_or("").to_lowercase();
    let stem = KPACKAGE_EXTS
        .iter()
        .chain(ARCHIVE_EXTS)
        .find_map(|e| file.strip_suffix(e))
        .unwrap_or(file.as_str());
    // A timestamp the store appends to a re-uploaded file with the same
    // name (`Nilium-20200426034630.tar.gz` next to `Nilium.tar.gz`) does
    // not change the name: which of the two is newer cannot be told from the
    // name, and on a tie the response order stays.
    let stem = match stem.rsplit_once('-') {
        Some((base, tail)) if tail.len() == 14 && tail.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => stem,
    };
    let have = alnum_lower(stem);
    if have == want {
        TITLE_EXACT_BONUS
    } else if have.contains(&want) {
        TITLE_CONTAINED_BONUS
    } else {
        0
    }
}

/// Index of the most suitable link; `-1` if the list is empty — port of
/// `_pick_best_link`, without knowledge of the product kind (see
/// [`pick_best_link_for`]).
pub fn pick_best_link(links: &[Value]) -> isize {
    pick_best_link_for(links, false)
}

/// Index of the most suitable link taking the product kind into account. On a
/// tie of scores the order is preserved (the first best), like
/// `max(range(len), key=…)`.
pub fn pick_best_link_for(links: &[Value], kpackage_kind: bool) -> isize {
    pick_best_link_titled(links, kpackage_kind, "")
}

/// [`pick_best_link_for`] with the product title: among equivalent archives the
/// one whose name matches the title wins ([`title_bonus`]). The order of files
/// in the store's response is alphabetical, not "main first", so order alone is
/// not enough for the choice. On a full tie — the first one.
pub fn pick_best_link_titled(links: &[Value], kpackage_kind: bool, title: &str) -> isize {
    let score = |l: &Value| link_score_for(l, kpackage_kind) + title_bonus(l, title);
    let mut best: Option<(usize, i64)> = None;
    for (i, l) in links.iter().enumerate() {
        let s = score(l);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((i, s));
        }
    }
    best.map_or(-1, |(i, _)| i as isize)
}

/// Number of the file [`OcsClient::download`] will take from a product without
/// an explicit choice; `-1` — there is no file list.
pub fn auto_file_index(item: &ThemeItem) -> isize {
    let kpackage = item.download_links.len() > 1 && store_type_is_kpackage(item.category_id);
    pick_best_link_titled(&item.download_links, kpackage, &item.name)
}

/// File name for the person and for a manual choice: the `name` field, and
/// without it the last piece of the address.
fn link_file_name(link: &Value) -> String {
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    if !name.is_empty() {
        return name.to_string();
    }
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    url.rsplit('/').next().unwrap_or("").to_string()
}

/// Number of the product file to install: the one chosen by hand (`wanted` — a
/// name from [`file_choices`]), and if the name is not among the files (empty,
/// the list was updated) — the automatic one ([`auto_file_index`]).
pub fn file_index_for(item: &ThemeItem, wanted: &str) -> isize {
    if !wanted.is_empty() {
        if let Some(i) = item.download_links.iter().position(|l| link_file_name(l) == wanted) {
            return i as isize;
        }
    }
    auto_file_index(item)
}

/// Name of file `idx` of the product; `None` — no such number or the file has no
/// name.
pub fn file_name_at(item: &ThemeItem, idx: isize) -> Option<String> {
    let name = item.download_links.get(usize::try_from(idx).ok()?)?.get("name").and_then(Value::as_str)?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Name of the file [`OcsClient::download`] will take from a product without an
/// explicit choice; `None` — there is no file list or the file has no name.
pub fn chosen_file_name(item: &ThemeItem) -> Option<String> {
    file_name_at(item, auto_file_index(item))
}

/// Which product file is taken at installation — a line for the installation
/// log when the product has several files (otherwise `None`: there was nothing
/// to choose from).
pub fn chosen_file_note(item: &ThemeItem) -> Option<String> {
    file_note_at(item, auto_file_index(item))
}

/// [`chosen_file_note`] for file `idx` — chosen by hand or automatically.
pub fn file_note_at(item: &ThemeItem, idx: isize) -> Option<String> {
    let total = item.download_links.len();
    if total < 2 {
        return None;
    }
    let name = file_name_at(item, idx)?;
    Some(crate::i18n::trf(
        // TRANSLATORS: %1 is a file name from the store, %2 is how many files the item has.
        "File: “%1” (the item has %2 files)",
        &[&name, &total.to_string()],
    ))
}

/// A product file in the choice list of the details panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChoice {
    /// The file name — also the choice key ([`file_index_for`]).
    pub name: String,
    /// Size in KiB, as the store reports it (`downloadsize`).
    pub size_kib: i64,
    /// Upload day `YYYY-MM-DD` — only from the timestamp the store appends to the
    /// name of a re-uploaded file (`-20230519162903`). A file has no date of its own
    /// in the store's response, so most often this is empty.
    pub date: String,
    /// This file is taken automatically ([`auto_file_index`]) — the same one the
    /// tile button installs.
    pub chosen: bool,
}

/// The day from the store's timestamp in the file name
/// (`Name-20230519162903.tar.gz` → `2023-05-19`); empty — there is no
/// timestamp.
fn store_stamp_day(file: &str) -> String {
    let lower = file.to_lowercase();
    let stem = KPACKAGE_EXTS
        .iter()
        .chain(ARCHIVE_EXTS)
        .find_map(|e| lower.strip_suffix(e))
        .unwrap_or(lower.as_str());
    match stem.rsplit_once('-') {
        Some((_, t)) if t.len() == 14 && t.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{}-{}-{}", &t[..4], &t[4..6], &t[6..8])
        }
        _ => String::new(),
    }
}

/// Product file options for a manual choice — all in response order, with the
/// automatic one marked. One file or none — empty: there is nothing to choose
/// from, and the panel has no choice block.
pub fn file_choices(item: &ThemeItem) -> Vec<FileChoice> {
    if item.download_links.len() < 2 {
        return Vec::new();
    }
    let auto = auto_file_index(item);
    item.download_links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let name = link_file_name(l);
            FileChoice {
                date: store_stamp_day(&name),
                size_kib: l.get("size").and_then(Value::as_i64).unwrap_or(0),
                chosen: i as isize == auto,
                name,
            }
        })
        .collect()
}

/// Whether a store product with this `typeid` is installed by KPackage — decided
/// by the catalog section (subcategories lead to the umbrella) and the
/// environment matrix, as for the installer. No install directory is needed
/// here, so the home directory is empty.
pub fn store_type_is_kpackage(type_id: i64) -> bool {
    crate::categories::category_by_id(std::path::Path::new(""), type_id)
        .is_some_and(|c| crate::appearance_installer::is_kpackage_kind(&c.apply_kind))
}

// ── Parsing content ────────────────────────────────────────────────────────

/// Store tags with which an author specifies the license → its short name.
///
/// The `license` field in the OCS response is almost always empty (it is absent
/// from the summary altogether), while the author chooses the license on the
/// site, and it comes as a TAG in `tags`. The table was compiled from the live
/// response of 04.10.2026 (5348 products in 54 categories, among them cursors
/// 107, global themes 722, KWin effects 719): exactly these tags occurred there.
/// A version the tag does not name is not invented: "cc-by" stays "CC-BY",
/// "bsd-license" — "BSD".
///
/// Warning: bare "bsd", "x11", "apache", "cc" are NOT in the table: in the
/// response they mark the system, the windowing system and the server
/// ("linux,bsd", "x11,wayland"), not the license.
pub const LICENSE_TAGS: &[(&str, &str)] = &[
    ("gplv3", "GPL-3.0"),
    ("gplv3-later", "GPL-3.0-or-later"),
    ("gplv2", "GPL-2.0"),
    ("gplv2-later", "GPL-2.0-or-later"),
    ("gpl", "GPL"),
    ("agplv3", "AGPL-3.0"),
    ("lgplv3", "LGPL-3.0"),
    ("lgplv2", "LGPL-2.0"),
    ("lgplv2.1", "LGPL-2.1"),
    ("lgpl", "LGPL"),
    ("mit-license", "MIT"),
    ("mit", "MIT"),
    ("bsd-license", "BSD"),
    ("freebsd", "BSD-2-Clause"),
    ("apache-license", "Apache-2.0"),
    ("x11-license", "X11"),
    ("mpl", "MPL"),
    ("mpl-2.0", "MPL-2.0"),
    ("artistic-2", "Artistic-2.0"),
    ("cc0", "CC0-1.0"),
    ("cc-by", "CC-BY"),
    ("cc-by-sa", "CC-BY-SA"),
    ("cc-by-nd", "CC-BY-ND"),
    ("cc-by-nc", "CC-BY-NC"),
    ("cc-by-nc-sa", "CC-BY-NC-SA"),
    ("cc-by-nc-nd", "CC-BY-NC-ND"),
];

/// License from the product's tags: all recognized ones, without duplicates, in
/// tag order, comma-separated. Nothing recognized — empty (the card will show
/// "—").
pub fn license_from_tags(tags: &str) -> String {
    let mut found: Vec<&str> = Vec::new();
    for tag in tags.split(',') {
        let tag = tag.trim().to_ascii_lowercase();
        if let Some((_, spdx)) = LICENSE_TAGS.iter().find(|(t, _)| *t == tag) {
            if !found.contains(spdx) {
                found.push(spdx);
            }
        }
    }
    found.join(", ")
}

/// Parsing one `<content>` into `OcsContent` — port of `_parse_content`.
/// `None` if `id` is empty.
fn parse_content(content: &Node) -> Option<OcsContent> {
    let cid = txt(content, "id", "");
    if cid.is_empty() {
        return None;
    }

    // Preview — the large picture (previewpic1), then the small one.
    let mut preview = String::new();
    for k in ["previewpic1", "smallpreviewpic1", "previewpic2"] {
        let v = txt(content, k, "");
        if !v.is_empty() {
            preview = v;
            break;
        }
    }

    // ALL screenshots previewpic1..12 (gaps in numbering are skipped, duplicates too).
    let mut previews: Vec<String> = Vec::new();
    for i in 1..=12 {
        let v = txt(content, &format!("previewpic{i}"), "");
        if !v.is_empty() && !previews.contains(&v) {
            previews.push(v);
        }
    }

    // Downloadable files (downloadlink1..N, stopping at the first empty one).
    let mut links: Vec<Value> = Vec::new();
    for i in 1..20 {
        let link = txt(content, &format!("downloadlink{i}"), "");
        if link.is_empty() {
            break;
        }
        let name = txt(content, &format!("downloadname{i}"), "");
        let size = num(content, &format!("downloadsize{i}"));
        // key order url/name/size = as in the Python dict (preserve_order).
        links.push(json!({ "url": link, "name": name, "size": size }));
    }

    // The best link for Linux (not necessarily the first).
    let type_id = num(content, "typeid");
    let (dl_url, dl_name, dl_size) = {
        let kpackage = links.len() > 1 && store_type_is_kpackage(type_id);
        let best = pick_best_link_titled(&links, kpackage, &txt(content, "name", ""));
        if best >= 0 {
            let l = &links[best as usize];
            (
                l.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
                l.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                l.get("size").and_then(Value::as_i64).unwrap_or(0),
            )
        } else {
            (String::new(), String::new(), 0)
        }
    };

    let item = ThemeItem {
        id: cid,
        name: txt(content, "name", ""),
        description: txt(content, "description", ""),
        author: txt(content, "personid", ""),
        license: {
            let own = txt(content, "license", "");
            if own.is_empty() {
                license_from_tags(&txt(content, "tags", ""))
            } else {
                own
            }
        },
        version: txt(content, "version", ""),
        rating: num(content, "score"),
        downloads: num(content, "downloads"),
        preview_url: preview,
        preview_urls: previews,
        download_url: dl_url,
        download_name: dl_name,
        download_size: dl_size,
        category_id: type_id,
        created: txt(content, "created", ""),
        updated: txt(content, "changed", ""),
        download_links: links,
        ..ThemeItem::default()
    };
    Some(OcsContent { item, comment_count: num(content, "comments") })
}

/// Diagnostics of an unsuccessful OCS response parse — for log parity of the
/// calling layer (Python writes log.error/log.warning in these branches; the
/// behavioral result does not depend on the diagnostics).
#[derive(Debug, Clone, PartialEq)]
pub enum OcsDiag {
    /// Invalid XML (the analogue of `ET.ParseError`).
    Xml,
    /// The store answered with a refusal: `meta/status != ok` or `statuscode` is not
    /// 100. `code` may be empty, `message` is the refusal text from the store itself.
    Status { code: String, message: String },
}

/// Why the store did not give the page — as opposed to "no products".
///
/// On 20.10.2026 the store's API answered any catalog request with HTTP 200 and
/// a refusal "999 unknown request", and the section said "nothing found". A
/// refusal, a connection failure and an unreadable response are NOT an empty
/// result: the person is told that the store is not answering, and the cache is
/// not overwritten by such a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreFailure {
    /// Could not reach it: the network error text.
    Network(String),
    /// The store answered, but with a refusal (`status failed` / `statuscode` ≠ 100).
    Refused { code: String, message: String },
    /// A response came, but it cannot be read.
    Malformed,
}

/// A refusal in `<meta>`: `status != ok` or a code other than 100 (OCS
/// success). An empty code with `status ok` is not a refusal — old mirrors
/// answer like that.
fn meta_refusal(meta: &Node) -> Option<OcsDiag> {
    let code = txt(meta, "statuscode", "");
    if txt(meta, "status", "") != "ok" || !(code.is_empty() || code == "100") {
        return Some(OcsDiag::Status { code, message: txt(meta, "message", "") });
    }
    None
}

/// A refusal in JSON form (`{"status":"failed","statuscode":999,…}`): that is how
/// the API answers with `format=json` and on a number of paths regardless of the
/// format. We do not ask for a successful JSON catalog — it is "unreadable" too.
fn json_refusal(text: &str) -> Option<OcsDiag> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let status = v.get("status").and_then(Value::as_str).unwrap_or("");
    let code = match v.get("statuscode") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.trim().to_string(),
        _ => String::new(),
    };
    if status == "ok" && (code.is_empty() || code == "100") {
        return Some(OcsDiag::Xml);
    }
    let message = v.get("message").and_then(Value::as_str).unwrap_or("").trim().to_string();
    Some(OcsDiag::Status { code, message })
}

/// Parsing a list response (`/content/data`) — port of `_parse_list`.
/// Returns `(content, totalitems)`. An XML error or `status != ok` → `([], 0)`
/// (with not-ok Python does not read totalitems either → 0).
pub fn parse_list(xml: &[u8]) -> (Vec<OcsContent>, i64) {
    let (items, total, _diag) = parse_list_checked(xml);
    (items, total)
}

/// `parse_list` + diagnostics of failure (which branch Python would have logged).
pub fn parse_list_checked(xml: &[u8]) -> (Vec<OcsContent>, i64, Option<OcsDiag>) {
    let Ok(text) = std::str::from_utf8(xml) else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };
    if text.trim_start().starts_with('{') {
        return (Vec::new(), 0, Some(json_refusal(text).unwrap_or(OcsDiag::Xml)));
    }
    let Ok(dom) = parse_dom(text) else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };
    let Some(ocs) = dom.child("ocs") else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };

    let meta = ocs.child("meta");
    if let Some(refusal) = meta.and_then(meta_refusal) {
        return (Vec::new(), 0, Some(refusal));
    }
    let total = meta.map(|m| num(m, "totalitems")).unwrap_or(0);

    let Some(data) = ocs.child("data") else {
        return (Vec::new(), total, None);
    };
    let items = data.children_named("content").filter_map(parse_content).collect();
    (items, total, None)
}

/// The detail response was not parsed (broken XML / no `<data>` / no
/// `<content>`) — Python goes to the SQLite cache in these branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedDetail;

/// Parsing a detail response (`/content/data/{id}`) — port of the parsing branch
/// of `OCSClient.get_item` (the meta status is NOT checked there, unlike the
/// list).
///
/// * `Err(MalformedDetail)` — "cannot be parsed": the caller goes to the cache.
/// * `Ok(None)` — parsed, but `id` is empty: Python returns None WITHOUT the cache.
/// * `Ok(Some(_))` — the item.
pub fn parse_item_detail(xml: &[u8]) -> Result<Option<OcsContent>, MalformedDetail> {
    let text = std::str::from_utf8(xml).map_err(|_| MalformedDetail)?;
    let dom = parse_dom(text).map_err(|_| MalformedDetail)?;
    let ocs = dom.child("ocs").ok_or(MalformedDetail)?;
    let data = ocs.child("data").ok_or(MalformedDetail)?;
    let content = data.child("content").ok_or(MalformedDetail)?;
    Ok(parse_content(content))
}

/// Parsing a comments response (`/comments/data/...`) into a flat list — port of
/// the `_flatten_comments` unfolding over the `<data>` node.
///
/// Status checks and the handling of code 101 from the network method
/// `comments()` are NOT included here (that is the network layer). An XML error
/// or a missing `<data>` → an empty list.
pub fn parse_comments(xml: &[u8]) -> Vec<OcsComment> {
    let Ok(text) = std::str::from_utf8(xml) else {
        return Vec::new();
    };
    let Ok(dom) = parse_dom(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(data) = dom.child("ocs").and_then(|o| o.child("data")) {
        flatten_comments(data, 0, &mut out);
    }
    out
}

/// Parsing a comments response WITH a meta status check — port of the parsing
/// branch of `OCSClient.comments` (after `_http_get`).
///
/// Returns `(result, diagnostics)`:
/// * `(None, Some(_))` — the endpoint is unavailable: broken XML or
///   `status != ok` with a code ≠ 101 (Python returns None + log.warning);
/// * `(Some([]), None)` — code 101 "no comments" or no `<data>`;
/// * `(Some(flat), None)` — a flat list with depth nesting.
pub fn parse_comments_checked(xml: &[u8]) -> (Option<Vec<OcsComment>>, Option<OcsDiag>) {
    let Ok(text) = std::str::from_utf8(xml) else {
        return (None, Some(OcsDiag::Xml));
    };
    let Ok(dom) = parse_dom(text) else {
        return (None, Some(OcsDiag::Xml));
    };
    let Some(ocs) = dom.child("ocs") else {
        return (None, Some(OcsDiag::Xml));
    };
    if let Some(refusal) = ocs.child("meta").and_then(meta_refusal) {
        // 101 = an empty result: the content has 0 comments.
        if matches!(&refusal, OcsDiag::Status { code, .. } if code == "101") {
            return (Some(Vec::new()), None);
        }
        return (None, Some(refusal));
    }
    let mut out = Vec::new();
    if let Some(data) = ocs.child("data") {
        flatten_comments(data, 0, &mut out);
    }
    (Some(out), None)
}

/// Recursive collection of `<comment>` (+ nested `<children>`) — port of
/// `_flatten_comments`.
fn flatten_comments(parent: &Node, depth: i64, out: &mut Vec<OcsComment>) {
    for c in parent.children_named("comment") {
        out.push(OcsComment {
            author: txt(c, "user", ""),
            date: txt(c, "date", ""),
            subject: txt(c, "subject", ""),
            text: txt(c, "text", ""),
            score: txt(c, "score", ""),
            depth,
        });
        if let Some(children) = c.child("children") {
            flatten_comments(children, depth + 1, out);
        }
    }
}

// ── URL normalization (safe_url) ───────────────────────────────────────────

// Schemes for which urlunsplit restores `//netloc` (a subset of urllib's
// `uses_netloc` covering real OCS links).
const USES_NETLOC: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "telnet", "imap", "wais", "file",
    "mms", "https", "shttp", "snews", "prospero", "rtsp", "rtspu", "rsync",
    "svn", "svn+ssh", "sftp", "nfs", "git", "git+ssh", "ws", "wss",
];

/// Percent-encode like `urllib.parse.quote(s, safe)`: always safe = ALPHA/DIGIT/
/// `_.-~`, plus the given `safe` characters; other bytes (including non-ASCII)
/// → `%XX` (uppercase, by UTF-8 bytes).
fn quote(s: &str, safe: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let is_safe = b.is_ascii_alphanumeric()
            || matches!(b, b'_' | b'.' | b'-' | b'~')
            || (b < 0x80 && safe.as_bytes().contains(&b));
        if is_safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The network part of a link — port of `urllib.parse.urlparse(url).netloc`
/// (together with the login and port if they are present: the comparison is
/// always exact). A public wrapper over the shared parsing — a second link
/// parser must not be created in the core (rule #12).
pub fn url_netloc(url: &str) -> String {
    urlsplit(url).1
}

/// Port of `urllib.parse.urlsplit` (without port validation): `(scheme, netloc,
/// path, query, fragment)`. The order of splitting is as in CPython: scheme →
/// `//netloc` → `#fragment` → `?query`.
pub fn urlsplit(url: &str) -> (String, String, String, String, String) {
    let mut rest = url;
    let mut scheme = String::new();
    if let Some(i) = rest.find(':') {
        if i > 0 {
            let cand = &rest[..i];
            // scheme_chars: ALPHANUM + '+' '-' '.'
            if cand
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
            {
                scheme = cand.to_ascii_lowercase();
                rest = &rest[i + 1..];
            }
        }
    }

    let mut netloc = String::new();
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = after[..end].to_string();
        rest = &after[end..];
    }

    let mut fragment = String::new();
    if let Some(i) = rest.find('#') {
        fragment = rest[i + 1..].to_string();
        rest = &rest[..i];
    }

    let mut query = String::new();
    if let Some(i) = rest.find('?') {
        query = rest[i + 1..].to_string();
        rest = &rest[..i];
    }

    (scheme, netloc, rest.to_string(), query, fragment)
}

/// Port of `urllib.parse.urlunsplit`.
fn urlunsplit(scheme: &str, netloc: &str, path: &str, query: &str, fragment: &str) -> String {
    let mut url = path.to_string();
    if !netloc.is_empty()
        || (!scheme.is_empty() && USES_NETLOC.contains(&scheme) && !url.starts_with("//"))
    {
        if !url.is_empty() && !url.starts_with('/') {
            url = format!("/{url}");
        }
        url = format!("//{netloc}{url}");
    }
    if !scheme.is_empty() {
        url = format!("{scheme}:{url}");
    }
    if !query.is_empty() {
        url = format!("{url}?{query}");
    }
    if !fragment.is_empty() {
        url = format!("{url}#{fragment}");
    }
    url
}

/// Percent-encode unsafe characters in an OCS URL — port of `_safe_url`. Already
/// encoded `%XX` are kept (`%` in safe → no double encoding), the fragment is not
/// touched (as in the original).
pub fn safe_url(url: &str) -> String {
    let (scheme, netloc, path, query, fragment) = urlsplit(url);
    let path = quote(&path, "/%:@!$&'()*+,;=~-._");
    let query = quote(&query, "/%:@!$&'()*+,;=?~-._");
    urlunsplit(&scheme, &netloc, &path, &query, &fragment)
}

// ── Full-size preview (the detail view gallery) ────────────────────────────

/// previewpic URL → URL of the full-resolution original — port of
/// `full_preview_url`.
///
/// Removes the FIRST occurrence of the pling CDN resizer segment
/// `/cache/<W>x<H><tail>/img/` → `/img/` (Python: `_CACHE_SEG_RE.sub(…, count=1)`
/// with the regex `/cache/\d+x\d+[^/]*/img/`). A URL without such a segment is
/// returned as is — the transformation is safe for non-pling links (a no-op).
///
/// The regex is reproduced by hand (without the regex crate): `[^/]*` is a
/// superset of `\d+`, so the greedy `\d+` needs no backtracking, and a linear
/// parse is enough: after `\d+ x \d+` go to the next `/` and check that `/img/`
/// follows.
pub fn full_preview_url(url: &str) -> String {
    const CACHE: &str = "/cache/";
    let b = url.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = url[from..].find(CACHE) {
        let start = from + rel;
        let mut p = start + CACHE.len();
        // \d+
        let d1 = p;
        while p < b.len() && b[p].is_ascii_digit() {
            p += 1;
        }
        // 'x' between the groups of digits
        if p > d1 && p < b.len() && b[p] == b'x' {
            p += 1;
            let d2 = p;
            while p < b.len() && b[p].is_ascii_digit() {
                p += 1;
            }
            if p > d2 {
                // [^/]* — up to the next '/' (the segment cannot contain it)
                let mut q = p;
                while q < b.len() && b[q] != b'/' {
                    q += 1;
                }
                if url[q..].starts_with("/img/") {
                    // count=1: replace this occurrence and leave.
                    return format!("{}/img/{}", &url[..start], &url[q + "/img/".len()..]);
                }
            }
        }
        // This `/cache/` did not fit — the regex would shift the start by one character.
        from = start + 1;
    }
    url.to_string()
}

// ── Network layer: deterministic parts (cache keys, URLs, file names) ───────

/// `SORT_MAP.get(sort, "new")` — the OCS sort mode from the UI combo box value.
/// An unknown value (including an empty one) → `"new"`, like `dict.get` with a
/// default.
pub fn sort_mode(sort: &str) -> &'static str {
    SORT_MODES.iter().copied().find(|m| *m == sort).unwrap_or("new")
}

/// Cache key of a catalog page — port of `OCSClient.list_key`
/// (`f"cat{id}:p{page}:s{sortmode}:q{search.strip().lower()}"`).
///
/// `lower()` in Python is full Unicode (`«Тёмная» → «тёмная»`), so here it is
/// `to_lowercase()` (not `to_ascii_lowercase`). `strip()` trims Unicode
/// whitespace — `trim()` does the same.
pub fn list_key(category_id: &str, page: i64, sort: &str, search: &str) -> String {
    let sortmode = sort_mode(sort);
    let q = search.trim().to_lowercase();
    format!("cat{category_id}:p{page}:s{sortmode}:q{q}")
}

/// `urllib.parse.quote_plus`: like [`quote`] with an empty `safe`, but space →
/// `+`. Used by `urlencode` for each parameter value.
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b == b' ' {
            out.push('+');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// URL of a catalog request — port of the assembly in `OCSClient.search`
/// (`urlencode` over the parameters in the order of the dict declaration;
/// `search` is added ONLY if it is non-empty after `strip()` — and already
/// trimmed).
///
/// `category_id` as a string: aggregate categories go as a list "716,488"
/// (Python substitutes `str(category_id)`, where the id may also be a string
/// from `store_cats`).
pub fn search_url(category_id: &str, page: i64, pagesize: i64, sortmode: &str, search: &str) -> String {
    let mut q = format!(
        "categories={}&page={}&pagesize={}&sortmode={}",
        quote_plus(category_id),
        quote_plus(&page.to_string()),
        quote_plus(&pagesize.to_string()),
        quote_plus(sortmode),
    );
    let s = search.trim();
    if !s.is_empty() {
        q.push_str("&search=");
        q.push_str(&quote_plus(s));
    }
    format!("{API_BASE}/content/data?{q}")
}

/// URL of a detail request — port of `f"{API_BASE}/content/data/{content_id}"`.
/// There is no escaping in Python (the id is digits from the API itself);
/// normalizing unsafe characters is done by [`safe_url`] right before the
/// request.
pub fn item_url(content_id: &str) -> String {
    format!("{API_BASE}/content/data/{content_id}")
}

/// URL of comments — port of `OCSClient.comments`
/// (`/comments/data/1/{id}/0?page=…&pagesize=…`; type=1 — content, second_id=0).
pub fn comments_url(content_id: &str, page: i64, pagesize: i64) -> String {
    format!(
        "{API_BASE}/comments/data/1/{content_id}/0?page={}&pagesize={}",
        quote_plus(&page.to_string()),
        quote_plus(&pagesize.to_string()),
    )
}

/// Name of a preview file in the cache — port of the computation in
/// `OCSClient.get_preview_image`: `sha1(url).hexdigest() + ext`, where `ext` is
/// the extension from the URL PATH (`urlparse(url).path`, i.e. without
/// `?query`/`#fragment`); empty or longer than 5 characters → `.jpg`
/// (protection against "extensions" like `.verylongext`).
pub fn preview_cache_name(url: &str) -> String {
    let (_, _, path, _, _) = urlsplit(url);
    let ext = splitext(&path);
    // len(ext) > 5 — Python counts CHARACTERS, not bytes (important for Cyrillic).
    let ext = if ext.is_empty() || ext.chars().count() > 5 { ".jpg" } else { &ext };
    format!("{}{}", crate::translator::sha1_hex(url.as_bytes()), ext)
}

/// Extension like `os.path.splitext(p)[1]`: the last dot in the base name, and
/// leading dots of the base name do NOT count (`.hidden` → `""`).
fn splitext(path: &str) -> String {
    let base_at = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let base = &path[base_at..];
    // CPython: skip the leading dots, then look for the last dot in the rest.
    let lead = base.len() - base.trim_start_matches('.').len();
    match base[lead..].rfind('.') {
        Some(i) => base[lead + i..].to_string(),
        None => String::new(),
    }
}

// ── Network layer: HTTP requests and orchestration with the cache ──────────
//
// Port of `_http_get`/`_http_download` and the `OCSClient` class. The call
// itself is behind the [`NetIo`] trait (see `net.rs`), the cache behind
// [`OcsCacheIo`]; here are only the decisions: which URL, what counts as
// success, when to go to the cache, when to delete a partially downloaded file.

/// The catalog cache — the public API of `OCSCache` (`appearance/cache.py`) to
/// the extent `OCSClient` uses it.
///
/// The implementation stays outside: today it is a live SQLite in
/// `~/.cache/rexon/ocs_cache.db`, whose pure part is already ported into
/// [`crate::appearance_cache`] (SQL texts, TTL invalidation of lists,
/// serialization of fields). Invalidation is not duplicated here:
/// `cache_get_list` must return `None` for a stale (older than an hour) page, and
/// `cache_get_preview` for an entry whose file on disk has disappeared (port of
/// the `os.path.isfile` check in `get_preview`).
pub trait OcsCacheIo {
    /// `OCSCache.get_list(key)` — `None` if there is no entry or it is stale.
    fn cache_get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)>;
    /// `OCSCache.put_list(key, items, total)`.
    fn cache_put_list(&mut self, key: &str, items: &[ThemeItem], total: i64);
    /// `OCSCache.get_item(content_id)`.
    fn cache_get_item(&mut self, content_id: &str) -> Option<ThemeItem>;
    /// `OCSCache.put_item(item)`.
    fn cache_put_item(&mut self, item: &ThemeItem);
    /// `OCSCache.get_item_detail(content_id, max_age)` — the FULL details, if the
    /// detail response came no earlier than `max_age_secs` ago; an entry from a list
    /// response does not count as full details.
    fn cache_get_item_detail(&mut self, content_id: &str, max_age_secs: u64) -> Option<ThemeItem>;
    /// `OCSCache.get_preview(url)` — the path to the downloaded preview file.
    fn cache_get_preview(&mut self, url: &str) -> Option<String>;
    /// `OCSCache.put_preview(url, local_path)`.
    fn cache_put_preview(&mut self, url: &str, local_path: &str);
}

/// The client's executor: network + cache.
pub trait OcsIo: NetIo + OcsCacheIo {}
impl<T: NetIo + OcsCacheIo> OcsIo for T {}

/// `os.path.dirname(path)` — everything up to the last `/` (without one — an
/// empty string).
fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        // "/a" → "/", as in Python (the slice does not eat the only slash).
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "",
    }
}

/// GET with failure logging — port of `_http_get`.
///
/// ALL network failures are caught (the original's `URLError`/`HTTPError`/
/// `OSError` — i.e. both a code outside 2xx and a refused connection), the result
/// is `None` and a warning in the log with the same text as in Python. The URL
/// goes through [`safe_url`]: OCS links bring raw spaces in file names.
///
/// Warning — a divergence from the original (rule #12): a temporary failure IS
/// RETRIED — see [`crate::net::with_retry`]. In Python a single connection break
/// meant an empty catalog page, although the very next attempt would have
/// returned an answer.
pub fn http_get<IO: NetIo + ?Sized>(io: &mut IO, url: &str, timeout_secs: u64) -> Option<Vec<u8>> {
    http_get_result(io, url, timeout_secs).ok()
}

/// [`http_get`], but with the text of the final error — so that a connection
/// failure can be told apart from an empty response upstairs.
pub fn http_get_result<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    let safe = safe_url(url);
    let what = format!("OCS GET {url}");
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.get(&safe, USER_AGENT, timeout_secs)
    });
    match res {
        Ok(body) => Ok(body),
        Err(e) => {
            io.log(LogLevel::Warning, &format!("OCS GET failed: {url} — {e}"));
            Err(e.to_string())
        }
    }
}

/// The shared tail of a download failure — port of the `except` branch of
/// `_http_download`: a message in the log and deleting the partially downloaded
/// file.
fn download_failed<IO: NetIo + ?Sized>(io: &mut IO, url: &str, dest: &str, e: &NetError) -> bool {
    io.log(LogLevel::Error, &format!("OCS download failed: {url} — {e}"));
    if io.exists(dest) {
        io.remove(dest);
    }
    false
}

/// Download `url` into `dest` — port of `_http_download`.
///
/// `on_progress` — whether there is a progress callback (in Python this is
/// `on_progress is not None`): with `false` [`NetIo::progress`] is not called at
/// all. `max_size` = 0 removes the ceiling (in Python this is a falsy argument
/// value).
///
/// The ceiling is checked twice, as in the original: before downloading by
/// `Content-Length` (then the file is not even created) and by the bytes
/// actually received — the server may lie in the header or not send it at all.
///
/// Warning — a fix to the original (rule #16): a `dest` without a directory (a
/// bare file name) got into `os.makedirs("")` in Python → `FileNotFoundError` →
/// the download "failed" out of nowhere. Here an empty directory is simply not
/// created.
///
/// Warning — a divergence from the original (rule #12): a temporary failure IS
/// RETRIED, and the download is repeated IN FULL (see
/// [`crate::net::with_retry`]). A break in the middle of an archive meant "the
/// theme did not install" in Python, although the second attempt usually gets
/// through.
pub fn http_download<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    dest: &str,
    on_progress: bool,
    timeout_secs: u64,
    max_size: i64,
) -> bool {
    let safe = safe_url(url);
    let what = format!("OCS скачивание {url}");
    // A retry takes the download IN FULL: the response is opened anew, the
    // file is created anew (`create` overwrites it) and the progress goes
    // from zero. Appending the second attempt to the end of the first one's
    // stub is not allowed — it would be a broken archive that the unpacker
    // rejects only after "Downloaded".
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        download_once(io, &safe, dest, on_progress, timeout_secs, max_size)
    });
    match res {
        Ok(ok) => ok,
        Err(e) => download_failed(io, url, dest, &e),
    }
}

/// One download attempt: `Ok(true)` — the file is in place, `Ok(false)` — a
/// refusal WITHOUT a network failure (over the size limit, no point in
/// repeating), `Err` — a network or write failure, whose retry is decided by
/// [`net::with_retry`].
fn download_once<IO: NetIo + ?Sized>(
    io: &mut IO,
    safe_url: &str,
    dest: &str,
    on_progress: bool,
    timeout_secs: u64,
    max_size: i64,
) -> Result<bool, NetError> {
    let total = io.open(safe_url, USER_AGENT, timeout_secs)?;
    if max_size > 0 && total > max_size {
        io.log(LogLevel::Error, &format!("Архив слишком большой: {total}"));
        io.close();
        return Ok(false);
    }
    let mut done: i64 = 0;
    let dir = dirname(dest).to_string();
    if !dir.is_empty() {
        io.makedirs(&dir);
    }
    if let Err(e) = io.create(dest) {
        io.close();
        return Err(e);
    }
    loop {
        let chunk = match io.read(DOWNLOAD_CHUNK) {
            Ok(c) => c,
            Err(e) => {
                io.close_file();
                io.close();
                return Err(e);
            }
        };
        if chunk.is_empty() {
            break;
        }
        done += chunk.len() as i64;
        if max_size > 0 && done > max_size {
            // The original closes the file here, erases it and leaves WITHOUT a
            // log entry — the size was already checked above, this is a
            // duplicate safeguard.
            io.close_file();
            io.remove(dest);
            io.close();
            return Ok(false);
        }
        if let Err(e) = io.write(&chunk) {
            io.close_file();
            io.close();
            return Err(e);
        }
        if on_progress {
            io.progress(done, total);
        }
    }
    io.close_file();
    io.close();
    Ok(true)
}

/// An item from the cache: `comment_count` does not go through SQLite (there is
/// no column), in this case the original leaves the attribute unset, and the
/// interface reads it via `getattr(item, "comment_count", None)`. Zero is exactly
/// what the interface sees today through the PyO3 bridge
/// (`d.get("_comment_count", 0)`).
fn from_cache(item: ThemeItem) -> OcsContent {
    OcsContent { item, comment_count: 0 }
}

/// The synchronous catalog client — port of the `OCSClient` class.
///
/// Call from a worker thread: the methods block on the network exactly like the
/// original, which lives in the QThread workers of the "Appearance" tab. The
/// exception is [`cached_search`](Self::cached_search): it does not go to the
/// network and is safe from the interface thread.
pub struct OcsClient<IO: OcsIo> {
    io: IO,
    preview_dir: String,
}

impl<IO: OcsIo> OcsClient<IO> {
    /// `preview_dir` — the preview cache directory (`PREVIEW_DIR`, see
    /// [`crate::appearance_cache::preview_dir`]).
    pub fn new(io: IO, preview_dir: &str) -> Self {
        OcsClient { io, preview_dir: preview_dir.to_string() }
    }

    /// Take the executor back (for parity tests — for the final checks).
    pub fn into_io(self) -> IO {
        self.io
    }

    /// Content search — port of `OCSClient.search`; returns `(items, totalitems)`.
    ///
    /// `category_id` as a string: umbrella categories go as a list "716,488". The
    /// order of the original's decisions is preserved: a fresh cache (if
    /// `use_cache`) → network → on a network failure the STALE cache → empty. A
    /// store refusal and a network failure collapse into an empty page — whoever
    /// cares about the reason calls [`search_checked`](Self::search_checked).
    ///
    /// Warning — a fix to the original (rule #16): Python filled an empty `typeid`
    /// with the line `it.category_id = it.category_id or category_id`, and for an
    /// umbrella category the string "716,488" got into the model's integer field
    /// (followed by int-to-str comparisons in the interface). Here the string is
    /// first parsed into a number, and an unparsable one (that very umbrella) leaves
    /// 0.
    pub fn search(
        &mut self,
        category_id: &str,
        page: i64,
        pagesize: i64,
        sort: &str,
        search: &str,
        use_cache: bool,
    ) -> (Vec<OcsContent>, i64) {
        self.search_checked(category_id, page, pagesize, sort, search, use_cache)
            .unwrap_or_default()
    }

    /// [`search`](Self::search) with the reason of the failure: `Err` — the store did
    /// not give the page (a connection failure, a refusal, an unreadable response)
    /// and there is nothing in the cache to substitute it with; `Ok` with an empty
    /// list — there really are no products.
    ///
    /// Warning — a divergence from the original: Python also put a refusal response
    /// into the cache (as an empty page) — and after a store failure the section
    /// stood empty for an hour, while a good page in the cache was overwritten. Here
    /// only a successful response goes into the cache.
    pub fn search_checked(
        &mut self,
        category_id: &str,
        page: i64,
        pagesize: i64,
        sort: &str,
        search: &str,
        use_cache: bool,
    ) -> Result<(Vec<OcsContent>, i64), StoreFailure> {
        let sortmode = sort_mode(sort);
        let key = list_key(category_id, page, sort, search);
        if use_cache {
            if let Some((items, total)) = self.io.cache_get_list(&key) {
                return Ok((items.into_iter().map(from_cache).collect(), total));
            }
        }

        let url = search_url(category_id, page, pagesize, sortmode, search);
        let raw = match http_get_result(&mut self.io, &url, TIMEOUT_SECS) {
            Ok(raw) => raw,
            // the network went down — we try the stale cache as a fallback
            Err(e) => return self.cached_or(&key, StoreFailure::Network(e)),
        };
        let (mut items, total, diag) = parse_list_checked(&raw);
        match diag {
            Some(OcsDiag::Xml) => {
                self.io
                    .log(LogLevel::Error, "OCS XML parse error: некорректный XML-ответ");
                return self.cached_or(&key, StoreFailure::Malformed);
            }
            Some(OcsDiag::Status { code, message }) => {
                self.io.log(LogLevel::Warning, &format!("OCS status not ok: {code}"));
                return self.cached_or(&key, StoreFailure::Refused { code, message });
            }
            None => {}
        }
        let fallback_cat = category_id.parse::<i64>().unwrap_or(0);
        for it in &mut items {
            if it.item.category_id == 0 {
                it.item.category_id = fallback_cat;
            }
        }
        let plain: Vec<ThemeItem> = items.iter().map(|c| c.item.clone()).collect();
        self.io.cache_put_list(&key, &plain, total);
        Ok((items, total))
    }

    /// The store did not give the page: the page from the cache if it is
    /// there, otherwise the reason of the failure.
    fn cached_or(
        &mut self,
        key: &str,
        why: StoreFailure,
    ) -> Result<(Vec<OcsContent>, i64), StoreFailure> {
        match self.io.cache_get_list(key) {
            Some((items, total)) => Ok((items.into_iter().map(from_cache).collect(), total)),
            None => Err(why),
        }
    }

    /// Cache only, WITHOUT the network — port of `OCSClient.cached_search` (instant
    /// rendering from the interface thread).
    pub fn cached_search(
        &mut self,
        category_id: &str,
        page: i64,
        sort: &str,
        search: &str,
    ) -> Option<(Vec<OcsContent>, i64)> {
        let key = list_key(category_id, page, sort, search);
        self.io
            .cache_get_list(&key)
            .map(|(items, total)| (items.into_iter().map(from_cache).collect(), total))
    }

    /// Details of one item — port of `OCSClient.get_item`.
    ///
    /// A network failure OR an unparsable response → the item from the cache. A
    /// parsed item is put into the cache; an empty `id` returns `None` without
    /// touching the cache (as in the original).
    pub fn get_item(&mut self, content_id: &str) -> Option<OcsContent> {
        let url = item_url(content_id);
        let Some(raw) = http_get(&mut self.io, &url, TIMEOUT_SECS) else {
            return self.io.cache_get_item(content_id).map(from_cache);
        };
        match parse_item_detail(&raw) {
            // cannot be parsed (broken XML / no <data>/<content>) → cache
            Err(MalformedDetail) => self.io.cache_get_item(content_id).map(from_cache),
            Ok(None) => None,
            Ok(Some(mut content)) => {
                // Warning: `<downloads>` from `content/data/<id>` is NOT the same
                // number as in the list response for the same product: live responses
                // of 08.11.2026 — Candy icons 3 118 105 in the list and 759 in the
                // details, Neon Icons 14 537 and 13 166, Neon-Dreamscape 6061 and 0.
                // The tile, the "by downloads" sort and the store site count by the
                // list; the details do not explain their number. So that the tile and
                // the details panel show the same thing (and the details do not
                // overwrite the tile's counter in the cache), we keep the number from
                // the list if it is already known.
                if let Some(known) = self.io.cache_get_item(&content.item.id) {
                    if known.downloads > 0 {
                        content.item.downloads = known.downloads;
                    }
                }
                self.io.cache_put_item(&content.item);
                Some(content)
            }
        }
    }

    /// Details for DISPLAY — port of `OCSClient.get_item(content_id, max_age)`.
    ///
    /// Fresh full details (no older than `max_age_secs`) are returned from the cache
    /// without the network; otherwise — the ordinary [`get_item`](Self::get_item).
    /// The details panel calls this with [`DETAIL_TTL_SECS`]; installation does not.
    pub fn get_item_cached(&mut self, content_id: &str, max_age_secs: u64) -> Option<OcsContent> {
        if let Some(hit) = self.io.cache_get_item_detail(content_id, max_age_secs) {
            return Some(from_cache(hit));
        }
        self.get_item(content_id)
    }

    /// Comments on content — port of `OCSClient.comments`.
    ///
    /// `Some([])` — there are no comments (including code 101); `None` — the
    /// endpoint is unavailable or the response cannot be parsed (the interface
    /// quietly hides the section).
    pub fn comments(
        &mut self,
        content_id: &str,
        page: i64,
        pagesize: i64,
    ) -> Option<Vec<OcsComment>> {
        let url = comments_url(content_id, page, pagesize);
        let raw = http_get(&mut self.io, &url, TIMEOUT_SECS)?;
        let (out, diag) = parse_comments_checked(&raw);
        match diag {
            Some(OcsDiag::Xml) => self.io.log(
                LogLevel::Warning,
                "OCS comments XML parse error: некорректный XML-ответ",
            ),
            Some(OcsDiag::Status { code, .. }) => self
                .io
                .log(LogLevel::Warning, &format!("OCS comments status not ok: {code}")),
            None => {}
        }
        out
    }

    /// Download a preview into the cache, return the local path — port of
    /// `OCSClient.get_preview_image`.
    pub fn get_preview_image(&mut self, url: &str) -> Option<String> {
        if url.is_empty() {
            return None;
        }
        // `if local:` — an empty string also means a cache miss in Python.
        if let Some(local) = self.io.cache_get_preview(url).filter(|p| !p.is_empty()) {
            return Some(local);
        }
        let dest = format!("{}/{}", self.preview_dir, preview_cache_name(url));
        if http_download(
            &mut self.io,
            url,
            &dest,
            false,
            PREVIEW_TIMEOUT_SECS,
            PREVIEW_MAX_SIZE,
        ) {
            self.io.cache_put_preview(url, &dest);
            return Some(dest);
        }
        None
    }

    /// Download a theme archive into `dest` — port of `OCSClient.download`.
    ///
    /// `link_index` out of range (`-1` by default) → automatic choice of the best
    /// link for Linux ([`auto_file_index`]); a file chosen by hand in the details
    /// panel comes as a number from [`file_index_for`]. `on_progress` — whether
    /// there is a progress callback.
    pub fn download(
        &mut self,
        item: &ThemeItem,
        dest: &str,
        link_index: isize,
        on_progress: bool,
    ) -> bool {
        let url = if !item.download_links.is_empty() {
            let mut idx = link_index;
            if !(idx >= 0 && (idx as usize) < item.download_links.len()) {
                idx = auto_file_index(item);
            }
            item.download_links[idx as usize]
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        } else {
            item.download_url.clone()
        };
        if url.is_empty() {
            return false;
        }
        http_download(
            &mut self.io,
            &url,
            dest,
            on_progress,
            DOWNLOAD_TIMEOUT_SECS,
            DOWNLOAD_MAX_SIZE,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  The live executor of the store client
// ═══════════════════════════════════════════════════════════════════════════
//
// WHY. The header of [`OcsCacheIo`] promises that the implementation "stays
// outside", and until 08.09.2026 there was exactly one outside — in Python
// (`rexon/appearance/ocs_client.py` on top of `appearance/cache.py`). So the
// appearance catalog worked only in a process with an interpreter, and the
// "Appearance" tab under the Rust window owner opened as an empty storefront.
//
// Here the promise is fulfilled: the network is taken from
// [`crate::net::UreqNet`], the cache from
// [`crate::appearance_cache::LiveCache`], and not a single new rule appears
// along the way — both halves already existed, only the assembly was missing.

/// The live [`OcsIo`] executor: the real network plus the real SQLite cache.
///
/// Created FOR ONE job (like `OCSClient()` of a Python worker): it holds an
/// sqlite connection inside, and that does not cross threads.
pub struct LiveOcsIo {
    net: crate::net::UreqNet,
    cache: crate::appearance_cache::LiveCache,
}

impl LiveOcsIo {
    /// The default network plus the cache in `~/.cache/rexon`.
    pub fn new(home: &str) -> Self {
        LiveOcsIo {
            net: crate::net::UreqNet::new(),
            cache: crate::appearance_cache::LiveCache::open(home),
        }
    }

    /// The same, but with an already configured transport: a cancellable one
    /// ([`crate::net::UreqNet::with_cancel`]) or one counting download progress
    /// ([`crate::net::UreqNet::with_progress`]).
    pub fn with_net(net: crate::net::UreqNet, home: &str) -> Self {
        LiveOcsIo { net, cache: crate::appearance_cache::LiveCache::open(home) }
    }

    /// The preview cache directory — the second argument of [`OcsClient::new`].
    pub fn preview_dir(&self) -> String {
        self.cache.preview_dir().to_string()
    }

    /// The cache directly: disk scans take raw material for previews of local
    /// themes from it, and translation cleanup takes its own table.
    pub fn cache_mut(&mut self) -> &mut crate::appearance_cache::LiveCache {
        &mut self.cache
    }
}

impl NetIo for LiveOcsIo {
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError> {
        self.net.get(url, user_agent, timeout_secs)
    }
    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError> {
        self.net.open(url, user_agent, timeout_secs)
    }
    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        self.net.read(n)
    }
    fn close(&mut self) {
        self.net.close();
    }
    fn makedirs(&mut self, path: &str) {
        self.net.makedirs(path);
    }
    fn create(&mut self, path: &str) -> Result<(), NetError> {
        self.net.create(path)
    }
    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        self.net.write(chunk)
    }
    fn close_file(&mut self) {
        self.net.close_file();
    }
    fn exists(&mut self, path: &str) -> bool {
        self.net.exists(path)
    }
    fn remove(&mut self, path: &str) {
        self.net.remove(path);
    }
    fn progress(&mut self, done: i64, total: i64) {
        self.net.progress(done, total);
    }
    fn log(&mut self, level: LogLevel, message: &str) {
        self.net.log(level, message);
    }
    fn sleep(&mut self, ms: u64) {
        self.net.sleep(ms);
    }
    fn content_type(&mut self) -> String {
        self.net.content_type()
    }
}

impl OcsCacheIo for LiveOcsIo {
    fn cache_get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)> {
        self.cache.get_list(key)
    }
    fn cache_put_list(&mut self, key: &str, items: &[ThemeItem], total: i64) {
        self.cache.put_list(key, items, total);
    }
    fn cache_get_item(&mut self, content_id: &str) -> Option<ThemeItem> {
        self.cache.get_item(content_id)
    }
    fn cache_put_item(&mut self, item: &ThemeItem) {
        self.cache.put_item(item);
    }
    fn cache_get_item_detail(&mut self, content_id: &str, max_age_secs: u64) -> Option<ThemeItem> {
        self.cache.get_item_detail(content_id, max_age_secs as f64)
    }
    fn cache_get_preview(&mut self, url: &str) -> Option<String> {
        self.cache.get_preview(url)
    }
    fn cache_put_preview(&mut self, url: &str, local_path: &str) {
        self.cache.put_preview(url, local_path);
    }
}

#[cfg(test)]
mod license_tag_tests {
    use super::license_from_tags;

    #[test]
    fn license_comes_from_tags() {
        // Live tags of 04.10.2026: the cursors "Aemeath" and the effect "ba-click-fx".
        assert_eq!(
            license_from_tags("animatedcursor,efi,cursor,cursor-theme,gplv3"),
            "GPL-3.0"
        );
        assert_eq!(
            license_from_tags("original-product,unix,gplv3,linux,kwin,mouse-trail"),
            "GPL-3.0"
        );
        assert_eq!(license_from_tags("GPLv2-later, CC-BY-SA"), "GPL-2.0-or-later, CC-BY-SA");
        assert_eq!(license_from_tags("mit-license,mit"), "MIT");
    }

    #[test]
    fn unknown_tags_give_nothing() {
        assert_eq!(license_from_tags(""), "");
        assert_eq!(license_from_tags("linux,bsd,x11,apache,cc"), "");
    }
}

#[cfg(test)]
mod file_choice_tests {
    use super::{file_choices, file_index_for, file_note_at, pick_best_link_titled, Value};
    use crate::models::ThemeItem;
    use serde_json::json;

    fn item_with(names: &[&str], title: &str) -> ThemeItem {
        ThemeItem {
            id: "7".into(),
            name: title.into(),
            download_links: names
                .iter()
                .enumerate()
                .map(|(i, n)| json!({ "url": format!("https://f/{i}/{n}"), "name": n, "size": 100 * (i as i64 + 1) }))
                .collect(),
            ..ThemeItem::default()
        }
    }

    /// Options for a manual choice: all files in response order, exactly the
    /// one taken automatically is marked (the same rule as for installation
    /// from the tile), with the size and the upload date from the name.
    #[test]
    fn choices_list_all_files_and_mark_the_automatic_one() {
        let item = item_with(
            &["Extra 1.4.tar.xz", "McOS-MJV-Gnome-3.30-20230519162903.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"],
            "McOS-MJV-Gnome-3.30",
        );
        let got = file_choices(&item);
        let names: Vec<&str> = got.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Extra 1.4.tar.xz", "McOS-MJV-Gnome-3.30-20230519162903.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"]
        );
        let chosen: Vec<bool> = got.iter().map(|c| c.chosen).collect();
        assert_eq!(chosen, [false, true, false], "автоматически — тот же, что у плитки");
        assert_eq!(got.iter().filter(|c| c.chosen).count(), 1);
        assert_eq!(got[2].size_kib, 300);
        // The date comes only from the store's timestamp in the name; without
        // it — empty, not invented.
        assert_eq!(got[1].date, "2023-05-19");
        assert_eq!(got[0].date, "");
        assert_eq!(got[2].date, "");
    }

    /// One file (or none) — there is nothing to choose from, there is no block.
    #[test]
    fn single_file_has_no_choice() {
        assert!(file_choices(&item_with(&["Only.tar.xz"], "Only")).is_empty());
        assert!(file_choices(&item_with(&[], "x")).is_empty());
    }

    /// A file chosen by hand is found by name; an unknown or empty name gives
    /// the automatic choice, not a refusal and not the first file.
    #[test]
    fn explicit_file_name_overrides_the_automatic_choice() {
        let item = item_with(&["Extra 1.4.tar.xz", "Theme.tar.xz"], "Theme");
        assert_eq!(file_index_for(&item, ""), 1);
        assert_eq!(file_index_for(&item, "Extra 1.4.tar.xz"), 0);
        assert_eq!(file_index_for(&item, "Gone.tar.xz"), 1);
        // The log line names the file that is actually taken.
        let note = file_note_at(&item, 0).unwrap();
        assert!(note.contains("Extra 1.4.tar.xz"), "{note}");
        assert!(file_note_at(&item_with(&["A.zip"], ""), 0).is_none());
    }

    fn links(names: &[&str]) -> Vec<Value> {
        names.iter().map(|n| json!({ "url": format!("https://f/{n}"), "name": n, "size": 1 })).collect()
    }

    fn chosen<'a>(names: &[&'a str], title: &str) -> &'a str {
        let i = pick_best_link_titled(&links(names), false, title);
        names[usize::try_from(i).unwrap()]
    }

    /// Live store products of 19.10.2026: files come alphabetically, and the
    /// add-ons archive ("Extra …") came first with equal scores.
    #[test]
    fn extras_archive_is_not_the_theme() {
        assert_eq!(
            chosen(
                &["Extra 1.4.tar.xz", "McOS-MJV-Cinnamon-Edition-2.0.tar.gz", "McOS-Mint-Cinnamon-Edition-master.tar.xz"],
                "McOS-Mint-Cinnamon-Edition",
            ),
            "McOS-Mint-Cinnamon-Edition-master.tar.xz"
        );
        assert_eq!(
            chosen(
                &["Extra 1.4.tar.xz", "McOS-MJV-Dark-mode-Gnome-3.30-1.1.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"],
                "McOS-MJV-Gnome-3.30",
            ),
            "McOS-MJV-Gnome-3.30-1.1.tar.xz"
        );
        assert_eq!(chosen(&["Extra's.tar.xz", "U8.DARK.tar.xz", "U8.tar.xz"], "UNITY8 - RE-DESIGNED"), "U8.DARK.tar.xz");
        assert_eq!(chosen(&["extra-Icons.zip", "Theme.zip"], ""), "Theme.zip");
        // There are no other archives — the add-on is taken too (better than nothing).
        assert_eq!(chosen(&["Extra 1.4.tar.xz", "preview.png"], ""), "Extra 1.4.tar.xz");
        // A word, not part of a word: "Extravaganza" is not an add-on.
        assert_eq!(chosen(&["Extravaganza.tar.xz", "B.tar.xz"], ""), "Extravaganza.tar.xz");
    }

    /// Among equivalent archives — the one whose name matches the product
    /// title; an exact match comes before containment.
    #[test]
    fn file_named_like_the_item_wins_ties() {
        assert_eq!(chosen(&["Rage-Blue-Cursor.tar.gz", "Rage-Cursor.tar.gz"], "Rage cursor"), "Rage-Cursor.tar.gz");
        assert_eq!(
            chosen(&["Redmond-firefox.tar.gz", "Redmond97-icons.tar.gz", "Redmond97.tar.gz"], "Redmond97"),
            "Redmond97.tar.gz"
        );
        // A name match does not override the file class: a Windows build and a
        // distribution package stay below an archive.
        assert_eq!(chosen(&["Rage-Cursor-windows.zip", "Other.tar.gz"], "Rage cursor"), "Other.tar.gz");
        assert_eq!(chosen(&["Rage-Cursor.deb", "Other.tar.gz"], "Rage cursor"), "Other.tar.gz");
        // The store's timestamp does not belong to the name: of two uploads of
        // the same file the first in response order stays.
        assert_eq!(
            chosen(&["Nilium-20200426034630.tar.gz", "Nilium.tar.gz"], "Nilium"),
            "Nilium-20200426034630.tar.gz"
        );
        assert_eq!(
            chosen(&["BeautyFolders-20230519162903.tar.gz", "BeautyLine-20230918134713.tar.gz"], "BeautyLine"),
            "BeautyLine-20230918134713.tar.gz"
        );
        // Without a title — the former order: the first of equals.
        assert_eq!(chosen(&["A.tar.xz", "B.tar.xz"], ""), "A.tar.xz");
        assert_eq!(pick_best_link_titled(&[], false, "x"), -1);
    }
}
