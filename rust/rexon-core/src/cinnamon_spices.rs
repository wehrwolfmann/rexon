//! The Cinnamon Spices store — the core of the `spices` source for the five Cinnamon
//! sections of the "Appearance" tab (applets, desklets, extensions, themes,
//! Nemo actions).
//!
//! How the site works (measured 17.10.2026, `https://cinnamon-spices.linuxmint.com`):
//! * **Index** — five static JSON files (`/json/<type>s.json`), a dictionary
//!   "key → record". The key is the uuid, for themes — the theme directory name (for some themes
//!   it differs from `name`: `Adara-Dark` ↔ "Adara Dark"). Installing and
//!   enabling must go by the KEY. The index has no Cinnamon version — it is only in
//!   `metadata.json` inside the archive.
//! * **Archive** — `/files/<type>s/<key>.zip`, inside it the directory `<key>/` (for
//!   actions there is also `<key>.nemo_action` next to it).
//! * **Load.** There are no terms of use, the site is behind a WAF. The index
//!   is fetched no more than once a day with a conditional request (`If-None-Match` /
//!   `If-Modified-Since` → 304), without a cache-busting time parameter;
//!   pictures — lazily; the archive — only on a button press.
//!
//! Here are only decisions: parsing the index, the storefront, the cache, compatibility with
//! the Cinnamon version, building `.mo` from `.po` and the install PLAN. Executing
//! the plan (downloading, unpacking, copying, gsettings) is a separate step.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::i18n::{tr, trf};
use crate::models::ThemeItem;

/// The site address.
pub const HOME_URL: &str = "https://cinnamon-spices.linuxmint.com";
/// The source key in the "Store:" switcher.
pub const SOURCE_KEY: &str = "spices";
/// The prefix of the card id and the installed log record:
/// `spices:<type>:<key>`.
pub const ID_PREFIX: &str = "spices:";
/// `provider_id` of the installed log.
pub const PROVIDER_ID: &str = "cinnamon-spices";
/// Index lifetime: no more than once a day per type.
pub const CACHE_TTL_SECS: i64 = 24 * 3600;
/// After a failed attempt to get the index (network, 5xx, garbage) — do not knock
/// again before this period (except for the "Refresh" button): otherwise every
/// opening of the section without network would hit the site anew.
pub const FAIL_BACKOFF_SECS: i64 = 15 * 60;
/// Cache file format version: if it grew, an old file is read as stale.
pub const CACHE_VERSION: u32 = 1;
/// Index request timeout, seconds (for connecting and for each read).
pub const INDEX_TIMEOUT_SECS: u64 = 15;
/// We do not read an index larger than this: the biggest one (applets) weighs ~0.7 MB.
pub const MAX_INDEX_BYTES: u64 = 16 << 20;

/// The request `User-Agent` — the same as the KDE Store client's.
pub fn user_agent() -> &'static str {
    crate::ocs::USER_AGENT
}

// ═══════════════════════════════════════════════════════════════════════
//  Spice types
// ═══════════════════════════════════════════════════════════════════════

/// A spice type. Install directories and the enabling method are NOT duplicated here:
/// they are in the `de_customization` matrix (the row by [`SpiceType::kind`]), the section
/// number — in `categories::DE_SECTIONS`.
#[derive(Debug, PartialEq, Eq)]
pub struct SpiceType {
    /// `applet`, `desklet`, `extension`, `theme`, `action`.
    pub key: &'static str,
    /// The "Appearance" section key.
    pub section: &'static str,
    /// The index and directory name on the site: `/json/<json>.json`, `/files/<json>/`.
    pub json: &'static str,
    /// The main code file without which Cinnamon does not load the spice.
    pub main_js: Option<&'static str>,
    /// The type for the DBus call `org.Cinnamon.ReloadXlet` (xlets only).
    pub xlet: Option<&'static str>,
}

/// Five types — one row per section.
pub const TYPES: &[SpiceType] = &[
    SpiceType { key: "applet", section: "cinnamonapplets", json: "applets", main_js: Some("applet.js"), xlet: Some("APPLET") },
    SpiceType { key: "desklet", section: "cinnamondesklets", json: "desklets", main_js: Some("desklet.js"), xlet: Some("DESKLET") },
    SpiceType { key: "extension", section: "cinnamonext", json: "extensions", main_js: Some("extension.js"), xlet: Some("EXTENSION") },
    SpiceType { key: "theme", section: "cinnamontheme", json: "themes", main_js: None, xlet: None },
    SpiceType { key: "action", section: "nemoactions", json: "actions", main_js: None, xlet: None },
];

impl SpiceType {
    /// The section's appearance type (`cinnamon-applet`, `theme-cinnamon`…) —
    /// from `categories::category_kind`.
    pub fn kind(&self) -> &'static str {
        crate::categories::category_kind(self.section).unwrap_or("")
    }

    /// The section number (909, 915–918) — from `categories::DE_SECTIONS`.
    pub fn cat_id(&self) -> i64 {
        crate::categories::DE_SECTIONS
            .iter()
            .find(|(_, k)| *k == self.section)
            .map(|(id, _)| *id)
            .unwrap_or(0)
    }

    /// The customization matrix row of this type.
    pub fn row(&self) -> Option<&'static crate::de_customization::Row> {
        crate::de_customization::row(self.kind())
    }

    pub fn is_theme(&self) -> bool {
        self.key == "theme"
    }

    pub fn is_action(&self) -> bool {
        self.key == "action"
    }
}

/// The type by the "Appearance" section key.
pub fn type_for_section(section: &str) -> Option<&'static SpiceType> {
    TYPES.iter().find(|t| t.section == section)
}

/// The type by its own key (`applet`…).
pub fn type_for_key(key: &str) -> Option<&'static SpiceType> {
    TYPES.iter().find(|t| t.key == key)
}

/// The index address. Without a time parameter: it makes every request
/// unique and breaks the server cache.
pub fn index_url(t: &SpiceType) -> String {
    format!("{HOME_URL}/json/{}.json", t.json)
}

/// The spice's page on the site ("Open page"). `spices-id` for themes is
/// a string, for the others — a number.
pub fn page_url(t: &SpiceType, spices_id: &Value) -> String {
    let id = match spices_id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    format!("{HOME_URL}/{}/view/{id}", t.json)
}

// ═══════════════════════════════════════════════════════════════════════
//  Index
// ═══════════════════════════════════════════════════════════════════════

/// An index record.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spice {
    /// The index dictionary key: the uuid, for themes — the directory name.
    pub key: String,
    /// The number (for themes — a string) for the link to the site page.
    pub spices_id: Value,
    pub name: String,
    pub description: String,
    /// `name_<lang>` / `description_<lang>`; themes have no such field.
    pub translations: Map<String, Value>,
    /// The number of "likes" on the site (not a 0–100 scale).
    pub score: i64,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds; this is the version for update checks.
    pub last_edited: i64,
    pub last_commit: String,
    pub last_commit_subject: String,
    /// The archive path from the site root.
    pub file: String,
    /// Archive size, bytes.
    pub file_size: i64,
    /// The icon path from the site root (empty for themes).
    pub icon: String,
    /// The screenshot path from the site root (empty for actions).
    pub screenshot: String,
    /// The author's nickname; the site's `none`/`unknown` are already empty here.
    pub author: String,
}

/// Whether a key is fit to be a directory name: `[A-Za-z0-9@._+-]+`, not `.` and not `..`.
/// Measured 17.10.2026: all 572 keys of the site satisfy this rule.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key != "."
        && key != ".."
        && key.len() <= 255
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
}

/// A path from the site root that is safe to append to [`HOME_URL`]:
/// starts with `/` (otherwise `HOME_URL` + `@evil/…` would lead to someone else's
/// address), without `..`, `//`, `?`, `#` and spaces.
fn site_path_ok(p: &str) -> bool {
    p.starts_with('/')
        && !p.starts_with("//")
        && !p.contains("..")
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-/%".contains(&b))
}

fn str_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn site_path_field(v: &Value, key: &str) -> String {
    let p = str_field(v, key);
    if site_path_ok(&p) { p } else { String::new() }
}

/// Parse the index. Tolerant of an empty `{}`, of missing fields and of records
/// of a foreign shape; records with an unfit key or archive path are dropped
/// (the key becomes a directory name, the path — a download address). The order is
/// as in the file.
pub fn parse_index(body: &str) -> Result<Vec<Spice>, String> {
    let root: Value = serde_json::from_str(body).map_err(|e| format!("index not parsed: {e}"))?;
    let obj = root.as_object().ok_or_else(|| "index is not an object".to_string())?;
    let mut out = Vec::with_capacity(obj.len());
    for (key, v) in obj {
        if !v.is_object() || !valid_key(key) {
            continue;
        }
        let file = site_path_field(v, "file");
        if file.is_empty() {
            continue;
        }
        let author = str_field(v, "author_user");
        let author = if matches!(author.as_str(), "none" | "unknown") { String::new() } else { author };
        out.push(Spice {
            key: key.clone(),
            spices_id: v.get("spices-id").cloned().unwrap_or(Value::Null),
            name: str_field(v, "name"),
            description: str_field(v, "description"),
            translations: v.get("translations").and_then(Value::as_object).cloned().unwrap_or_default(),
            score: int_field(v, "score"),
            created: int_field(v, "created"),
            last_edited: int_field(v, "last_edited"),
            last_commit: str_field(v, "last_commit"),
            last_commit_subject: str_field(v, "last_commit_subject"),
            file,
            file_size: int_field(v, "file_size"),
            icon: site_path_field(v, "icon"),
            screenshot: site_path_field(v, "screenshot"),
            author,
        });
    }
    Ok(out)
}

/// The interface language without encoding and modifier: `ru_RU.UTF-8@x` → `ru_RU`.
fn bare_lang(lang: &str) -> &str {
    let end = lang.find(['.', '@']).unwrap_or(lang.len());
    &lang[..end]
}

fn translated(s: &Spice, field: &str, lang_long: &str) -> Option<String> {
    let long = bare_lang(lang_long);
    let short = long.split('_').next().unwrap_or(long);
    let mut keys = vec![format!("{field}_{long}")];
    if short != long {
        keys.push(format!("{field}_{short}"));
    }
    keys.iter()
        .filter(|_| !long.is_empty())
        .find_map(|k| s.translations.get(k).and_then(Value::as_str).filter(|t| !t.trim().is_empty()))
        .map(str::to_string)
}

/// Name and description in the interface language: first `<field>_<lang_COUNTRY>`
/// (`name_zh_TW`), then `<field>_<lang>` (`name_ru`), otherwise the original.
pub fn localized(s: &Spice, lang_long: &str) -> (String, String) {
    (
        translated(s, "name", lang_long).unwrap_or_else(|| s.name.clone()),
        translated(s, "description", lang_long).unwrap_or_else(|| s.description.clone()),
    )
}

/// A storefront page: search (a case-insensitive substring in the name,
/// the description — original and translated — the key and the author), sorting,
/// slicing. `(page, how many matched in total)`.
///
/// Rexon sortings → site data: `new` — `last_edited` descending,
/// `high` and `down` — `score` descending (the site has no download counts),
/// `alpha` — the displayed name. An unknown one — like `new`. Equal ones — in index
/// order.
pub fn filter_sort_page(
    list: &[Spice],
    query: &str,
    sort: &str,
    page: usize,
    size: usize,
    lang: &str,
) -> (Vec<Spice>, usize) {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(&Spice, String)> = list
        .iter()
        .map(|s| (s, localized(s, lang)))
        .filter(|(s, (name, desc))| {
            q.is_empty()
                || [name.as_str(), desc.as_str(), &s.name, &s.description, &s.key, &s.author]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&q))
        })
        .map(|(s, (name, _))| (s, name))
        .collect();
    match sort {
        "high" | "down" => hits.sort_by(|a, b| b.0.score.cmp(&a.0.score)),
        "alpha" => hits.sort_by_cached_key(|(_, name)| name.to_lowercase()),
        _ => hits.sort_by(|a, b| b.0.last_edited.cmp(&a.0.last_edited)),
    }
    let total = hits.len();
    let size = size.max(1);
    let page = hits.into_iter().skip(page.saturating_mul(size)).take(size).map(|(s, _)| s.clone()).collect();
    (page, total)
}

/// A `YYYY.MM.DD` date from unix seconds — this is how the site and Cinnamon show
/// a spice's version.
pub fn version_label(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    crate::engine_update::now_iso(unix)[..10].replace('-', ".")
}

fn iso(unix: i64) -> String {
    if unix <= 0 { String::new() } else { crate::engine_update::now_iso(unix) }
}

fn site_url(path: &str) -> String {
    if path.is_empty() { String::new() } else { format!("{HOME_URL}{path}") }
}

/// Card id: `spices:<type>:<key>`.
pub fn item_id(t: &SpiceType, key: &str) -> String {
    format!("{ID_PREFIX}{}:{key}", t.key)
}

/// Parse a card id back: `(type, key)`.
pub fn parse_item_id(id: &str) -> Option<(&'static SpiceType, &str)> {
    let rest = id.strip_prefix(ID_PREFIX)?;
    let (tk, key) = rest.split_once(':')?;
    let t = type_for_key(tk)?;
    valid_key(key).then_some((t, key))
}

/// The license for the card: the site has no such field, the spices code lives in
/// the Cinnamon Spices repositories under the GPL.
pub fn license_label() -> String {
    tr("GPL (Cinnamon Spices repository)")
}

/// The card's small picture: for themes — the site thumbnail (about 7 KB rather than
/// a 700 KB screenshot), for actions — the icon (no screenshots), for the others —
/// the screenshot.
pub fn thumb_url(t: &SpiceType, s: &Spice) -> String {
    if t.is_theme() {
        return format!("{HOME_URL}/uploads/themes/thumbs/{}.png", s.key);
    }
    if t.is_action() || s.screenshot.is_empty() {
        return site_url(&s.icon);
    }
    site_url(&s.screenshot)
}

/// A storefront card. `rating` and `downloads` are zero: the site's "likes" are not on
/// a 0–100 scale (they are shown as a separate "★ N" line), there are no download
/// counts.
pub fn theme_item(t: &SpiceType, s: &Spice, lang: &str) -> ThemeItem {
    let (name, description) = localized(s, lang);
    // The card gallery is the full-size screenshot alone; the icon only when
    // there is no screenshot (actions). Previously the list was «screenshot,
    // icon», and the shared gallery rule (`appearance_page::gallery_urls`
    // drops the first picture — the KDE Store «face» of an item) left the
    // card and the full-screen view with the icon (27.11.2026).
    let gallery: Vec<String> = [&s.screenshot, &s.icon]
        .into_iter()
        .map(|p| site_url(p))
        .find(|u| !u.is_empty())
        .into_iter()
        .collect();
    ThemeItem {
        id: item_id(t, &s.key),
        name,
        description,
        author: s.author.clone(),
        license: license_label(),
        version: version_label(s.last_edited),
        rating: 0,
        downloads: 0,
        preview_url: thumb_url(t, s),
        preview_urls: gallery,
        download_url: site_url(&s.file),
        download_name: s.file.rsplit('/').next().unwrap_or("").to_string(),
        download_size: (s.file_size.max(0) + 1023) / 1024,
        category_id: t.cat_id(),
        created: iso(s.created),
        updated: iso(s.last_edited),
        source_name: if localized(s, lang).0 != s.name { s.name.clone() } else { String::new() },
        ..Default::default()
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn fixture(json: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/data/cinnamon_spices/{json}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn load(t: &SpiceType) -> Vec<Spice> {
        parse_index(&fixture(t.json)).unwrap()
    }

    #[test]
    fn types_line_up_with_sections_and_matrix() {
        let want = [
            ("applet", 915, "cinnamon-applet", ".local/share/cinnamon/applets"),
            ("desklet", 916, "cinnamon-desklet", ".local/share/cinnamon/desklets"),
            ("extension", 917, "cinnamon-extension", ".local/share/cinnamon/extensions"),
            ("theme", 909, "theme-cinnamon", ".themes"),
            ("action", 918, "nemo-action", ".local/share/nemo/actions"),
        ];
        assert_eq!(TYPES.len(), want.len());
        for (key, id, kind, dest) in want {
            let t = type_for_key(key).unwrap();
            assert_eq!(t.cat_id(), id, "{key}");
            assert_eq!(t.kind(), kind, "{key}");
            assert_eq!(t.row().unwrap().dest, dest, "{key}");
            assert_eq!(type_for_section(t.section), Some(t));
        }
        assert_eq!(type_for_section("gnomeext"), None);
    }

    #[test]
    fn urls() {
        let a = type_for_key("applet").unwrap();
        let th = type_for_key("theme").unwrap();
        assert_eq!(index_url(a), "https://cinnamon-spices.linuxmint.com/json/applets.json");
        assert!(!index_url(a).contains('?'));
        assert_eq!(page_url(a, &Value::from(17)), "https://cinnamon-spices.linuxmint.com/applets/view/17");
        assert_eq!(page_url(th, &Value::from("Adapta")), "https://cinnamon-spices.linuxmint.com/themes/view/Adapta");
    }

    #[test]
    fn all_five_indexes_parse() {
        for t in TYPES {
            let list = load(t);
            assert_eq!(list.len(), 6, "{}", t.key);
            for s in &list {
                assert!(valid_key(&s.key));
                assert_eq!(s.file, format!("/files/{}/{}.zip", t.json, s.key), "{}", s.key);
                assert!(s.last_edited > 1_500_000_000 || t.is_theme(), "{}", s.key);
                assert!(s.file_size > 0);
                if t.is_theme() {
                    assert!(s.icon.is_empty() && s.translations.is_empty());
                    assert!(s.spices_id.is_string());
                } else {
                    assert_eq!(s.icon, format!("/files/{}/{}.png", t.json, s.key));
                    assert!(s.spices_id.is_number());
                }
                if t.is_action() {
                    assert!(s.screenshot.is_empty());
                }
            }
        }
        let applets = load(type_for_key("applet").unwrap());
        let weather = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        assert_eq!(weather.author, "", "author_user = none is hidden");
        assert_eq!(weather.score, 270);
        assert_eq!(weather.screenshot, "/git/applets/weather@mockturtl/screenshot.png");
        let empty = applets.iter().find(|s| s.key == "1440@jvlianodorneles").unwrap();
        assert!(empty.translations.is_empty());
        assert_eq!(empty.author, "jvlianodorneles");
        // The order is as in the file.
        assert_eq!(applets[0].key, "1440@jvlianodorneles");
    }

    #[test]
    fn theme_key_differs_from_name() {
        let themes = load(type_for_key("theme").unwrap());
        let adara = themes.iter().find(|s| s.key == "Adara-Dark").unwrap();
        assert_eq!(adara.name, "Adara Dark");
        let th = type_for_key("theme").unwrap();
        let item = theme_item(th, adara, "ru");
        assert_eq!(item.id, "spices:theme:Adara-Dark");
        assert_eq!(item.preview_url, "https://cinnamon-spices.linuxmint.com/uploads/themes/thumbs/Adara-Dark.png");
        assert_eq!(item.download_url, "https://cinnamon-spices.linuxmint.com/files/themes/Adara-Dark.zip");
        assert_eq!(parse_item_id(&item.id), Some((th, "Adara-Dark")));
    }

    #[test]
    fn tolerant_parse_and_hostile_entries() {
        assert_eq!(parse_index("{}").unwrap(), vec![]);
        assert!(parse_index("[]").is_err());
        assert!(parse_index("<html>").is_err());
        let body = r#"{
            "ok@x": {"file": "/files/applets/ok@x.zip"},
            "../evil": {"file": "/files/applets/evil.zip"},
            "host@x": {"file": "@evil.example/x.zip"},
            "dots@x": {"file": "/files/../../x.zip"},
            "num@x": {"file": "/files/applets/num@x.zip", "score": "12", "last_edited": 1.7e9, "icon": "//evil/x.png"},
            "notobj": 5
        }"#;
        let list = parse_index(body).unwrap();
        let keys: Vec<&str> = list.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["ok@x", "num@x"]);
        assert_eq!(list[0].name, "");
        assert_eq!(list[1].score, 12);
        assert_eq!(list[1].last_edited, 1_700_000_000);
        assert_eq!(list[1].icon, "", "an address on a foreign host is dropped");
    }

    #[test]
    fn valid_key_rule() {
        for k in ["weather@mockturtl", "Adara-Dark", "a+b_c.d", "AlbumArt3.0@claudiux"] {
            assert!(valid_key(k), "{k}");
        }
        for k in ["", ".", "..", "a/b", "a b", "ключ", "a\\b", "a:b"] { // i18n-exempt: a non-ASCII key must be refused
            assert!(!valid_key(k), "{k}");
        }
    }

    #[test]
    fn localized_prefers_long_then_short() {
        let mut s = Spice { name: "Weather".into(), description: "D".into(), ..Default::default() };
        s.translations.insert("name_zh".into(), "zh".into());
        s.translations.insert("name_zh_TW".into(), "zh_TW".into());
        s.translations.insert("name_ru".into(), "Погода".into()); // i18n-exempt: test data, a site translation
        s.translations.insert("description_ru".into(), "  ".into());
        assert_eq!(localized(&s, "zh_TW").0, "zh_TW");
        assert_eq!(localized(&s, "zh_CN").0, "zh");
        assert_eq!(localized(&s, "ru_RU.UTF-8"), ("Погода".to_string(), "D".to_string()), "an empty translation is not taken"); // i18n-exempt: test data, a site translation
        assert_eq!(localized(&s, "ru").0, "Погода"); // i18n-exempt: test data, a site translation
        assert_eq!(localized(&s, "de").0, "Weather");
        assert_eq!(localized(&s, "").0, "Weather");

        // A real record: zh_CN and zh_TW side by side.
        let applets = load(type_for_key("applet").unwrap());
        let a = applets.iter().find(|s| s.key == "AutostartPrograms@spacy01").unwrap();
        let tw = a.translations["name_zh_TW"].as_str().unwrap();
        let cn = a.translations["name_zh_CN"].as_str().unwrap();
        assert_eq!(localized(a, "zh_TW").0, tw);
        assert_eq!(localized(a, "zh_CN").0, cn);
        let w = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        assert_eq!(localized(w, "ru").0, w.translations["name_ru"].as_str().unwrap());
    }

    #[test]
    fn sorting_and_search() {
        let applets = load(type_for_key("applet").unwrap());
        let keys = |v: &[Spice]| v.iter().map(|s| s.key.clone()).collect::<Vec<_>>();

        let (top, total) = filter_sort_page(&applets, "", "high", 0, 2, "en");
        assert_eq!(total, 6);
        assert_eq!(top[0].key, "weather@mockturtl");
        assert!(top[0].score >= top[1].score);
        assert_eq!(keys(&filter_sort_page(&applets, "", "down", 0, 10, "en").0), keys(&filter_sort_page(&applets, "", "high", 0, 10, "en").0));

        let (new, _) = filter_sort_page(&applets, "", "new", 0, 10, "en");
        assert!(new.windows(2).all(|w| w[0].last_edited >= w[1].last_edited));
        assert_eq!(keys(&filter_sort_page(&applets, "", "???", 0, 10, "en").0), keys(&new));

        let (alpha, _) = filter_sort_page(&applets, "", "alpha", 0, 10, "en");
        let names: Vec<String> = alpha.iter().map(|s| s.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // Search: by the original name, by the translation, by the key, by the author.
        assert_eq!(keys(&filter_sort_page(&applets, "WEATHER", "new", 0, 10, "ru").0), ["weather@mockturtl"]);
        let w = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        let ru = localized(w, "ru").0;
        assert_eq!(keys(&filter_sort_page(&applets, &ru, "new", 0, 10, "ru").0), ["weather@mockturtl"]);
        assert_eq!(keys(&filter_sort_page(&applets, "mockturtl", "new", 0, 10, "en").0), ["weather@mockturtl"]);
        assert_eq!(filter_sort_page(&applets, "jvlianodorneles", "new", 0, 10, "en").1, 1);
        assert_eq!(filter_sort_page(&applets, "no-such-thing", "new", 0, 10, "en").1, 0);

        // Page slicing.
        let (p1, t) = filter_sort_page(&applets, "", "new", 1, 4, "en");
        assert_eq!((p1.len(), t), (2, 6));
        assert!(filter_sort_page(&applets, "", "new", 9, 4, "en").0.is_empty());
    }

    #[test]
    fn theme_item_field_by_field() {
        let a = type_for_key("applet").unwrap();
        let w = load(a).into_iter().find(|s| s.key == "weather@mockturtl").unwrap();
        let item = theme_item(a, &w, "ru");
        let (name, desc) = localized(&w, "ru");
        let want = ThemeItem {
            id: "spices:applet:weather@mockturtl".into(),
            name: name.clone(),
            description: desc,
            author: String::new(),
            license: license_label(),
            version: version_label(w.last_edited),
            rating: 0,
            downloads: 0,
            preview_url: "https://cinnamon-spices.linuxmint.com/git/applets/weather@mockturtl/screenshot.png".into(),
            preview_urls: vec![
                "https://cinnamon-spices.linuxmint.com/git/applets/weather@mockturtl/screenshot.png".into(),
            ],
            download_url: "https://cinnamon-spices.linuxmint.com/files/applets/weather@mockturtl.zip".into(),
            download_name: "weather@mockturtl.zip".into(),
            download_size: (1_519_150 + 1023) / 1024,
            category_id: 915,
            created: crate::engine_update::now_iso(w.created),
            updated: crate::engine_update::now_iso(w.last_edited),
            source_name: "Weather".into(),
            ..Default::default()
        };
        assert_eq!(item, want);
        assert_ne!(name, "Weather");
        // The card and the full-screen view get the screenshot, not the icon.
        assert_eq!(
            crate::appearance_page::gallery_urls(&item.preview_urls, &item.preview_url),
            vec!["https://cinnamon-spices.linuxmint.com/git/applets/weather@mockturtl/screenshot.png".to_string()]
        );
        assert_eq!(item.version.len(), 10);
        assert_eq!(version_label(1_784_985_259), "2026.07.25");
        assert_eq!(version_label(0), "");

        // An action: the picture is the icon, there is no screenshot.
        let act = type_for_key("action").unwrap();
        let b = load(act).into_iter().find(|s| s.key == "backup-this-file@h3nr1ke").unwrap();
        let item = theme_item(act, &b, "en");
        assert_eq!(item.preview_url, "https://cinnamon-spices.linuxmint.com/files/actions/backup-this-file@h3nr1ke.png");
        assert_eq!(item.preview_urls, vec![item.preview_url.clone()]);
        assert_eq!(item.category_id, 918);
        assert_eq!(item.source_name, "");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Compatibility with the Cinnamon version
// ═══════════════════════════════════════════════════════════════════════

/// A `major.minor[.patch]` version; a missing part is 0. `6.6.9` →
/// `(6, 6, 9)`, `6.6` → `(6, 6, 0)`. A tail after the numbers (`6.6.9-1.1`,
/// `6.6.9+mint`) is dropped.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let core = s.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("");
    let mut parts = core.split('.').filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next().unwrap_or(Some(0)).unwrap_or(0);
    Some((major, minor, patch))
}

/// The installed Cinnamon version: `cinnamon --version` ("Cinnamon 6.6.9",
/// the second word), otherwise the package version (pacman, dpkg, rpm). `run` —
/// running a command, returns stdout on success.
pub fn running_cinnamon_version(run: &dyn Fn(&[&str]) -> Option<String>) -> Option<(u32, u32, u32)> {
    if let Some(v) = run(&["cinnamon", "--version"]).and_then(|o| o.split_whitespace().nth(1).and_then(parse_version)) {
        return Some(v);
    }
    let probes: &[(&[&str], usize)] = &[
        (&["pacman", "-Q", "cinnamon"], 1),
        (&["dpkg-query", "-W", "-f=${Version}", "cinnamon"], 0),
        (&["rpm", "-q", "--qf", "%{VERSION}", "cinnamon"], 0),
    ];
    for (argv, word) in probes {
        if let Some(v) = run(argv).and_then(|o| o.split_whitespace().nth(*word).and_then(parse_version)) {
            return Some(v);
        }
    }
    None
}

/// [`running_cinnamon_version`] on the live machine.
pub fn live_cinnamon_version() -> Option<(u32, u32, u32)> {
    running_cinnamon_version(&|argv: &[&str]| {
        let out = std::process::Command::new(argv[0]).args(&argv[1..]).stdin(std::process::Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

/// The leading digits of a string as a number (`"6abc"` → 6, `"x"` → none) — this is how
/// Cinnamon reads version parts from `metadata.json`.
fn leading_int(s: &str) -> Option<u32> {
    let s = s.trim_start();
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `cinnamon-version` is a list of MINIMUM versions: compatible if at least
/// one `major.minor` entry is not newer than the current one. An empty list — incompatible
/// (that is how both the shell itself and its settings window read it); an unparsable
/// entry does not match.
pub fn version_check(required: &[String], current: (u32, u32, u32)) -> bool {
    required.iter().any(|r| {
        let mut it = r.split('.');
        let (Some(maj), Some(min)) = (it.next().and_then(leading_int), it.next().and_then(leading_int)) else {
            return false;
        };
        current.0 > maj || (current.0 == maj && current.1 >= min)
    })
}

/// The `cinnamon-version` list from `metadata.json`: `None` — no field.
/// Numbers and a single string are accepted too (they occur in old
/// spices).
pub fn cinnamon_versions(meta: &Value) -> Option<Vec<String>> {
    let v = meta.get("cinnamon-version")?;
    let one = |x: &Value| match x {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    Some(match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        other => one(other).into_iter().collect(),
    })
}

fn version_dir(name: &str) -> Option<(u32, u32, u32)> {
    // ^[1-9][0-9]*\.[0-9]+(\.[0-9]+)?$
    let parts: Vec<&str> = name.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts[0].starts_with('0')
    {
        return None;
    }
    parse_version(name)
}

/// The subdirectory of a multiversion spice that Cinnamon will take: among
/// names like `3.8` / `4.0.1` — the largest one not newer than the current version
/// (`3.8` = `3.8.0`). `None` — the code lies in the root.
///
/// Parts are compared as NUMBERS. The 6.6.9 shell itself compares them
/// as strings when choosing (`"10" < "8"`), the settings window — as numbers; they diverge
/// only on parts of 10 and above, which the catalog does not have today.
pub fn pick_version_subdir(subdirs: &[String], current: (u32, u32, u32)) -> Option<String> {
    subdirs
        .iter()
        .filter_map(|n| version_dir(n).map(|v| (v, n)))
        .filter(|(v, _)| *v <= current)
        // Equal ones (`6.6` and `6.6.0`) — the more detailed name, as in the settings window.
        .max_by_key(|(v, n)| (*v, n.len()))
        .map(|(_, n)| n.clone())
}

#[cfg(test)]
mod version_tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_versions() {
        assert_eq!(parse_version("6.6.9"), Some((6, 6, 9)));
        assert_eq!(parse_version("6.6"), Some((6, 6, 0)));
        assert_eq!(parse_version("6.6.9-1.1"), Some((6, 6, 9)));
        assert_eq!(parse_version(" 5.8.4+wilma\n"), Some((5, 8, 4)));
        assert_eq!(parse_version("6"), None);
        assert_eq!(parse_version("Cinnamon"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn running_version_probes_in_order() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = |argv: &[&str]| {
            calls.borrow_mut().push(argv[0].to_string());
            match argv[0] {
                "cinnamon" => Some("Cinnamon 6.6.9\n".to_string()),
                _ => None,
            }
        };
        assert_eq!(running_cinnamon_version(&run), Some((6, 6, 9)));
        assert_eq!(calls.borrow().as_slice(), ["cinnamon"]);

        let pkg = |argv: &[&str]| match argv[0] {
            "pacman" => Some("cinnamon 6.6.9-1.1\n".to_string()),
            _ => None,
        };
        assert_eq!(running_cinnamon_version(&pkg), Some((6, 6, 9)));
        let deb = |argv: &[&str]| match argv[0] {
            "dpkg-query" => Some("6.4.13+zara".to_string()),
            _ => None,
        };
        assert_eq!(running_cinnamon_version(&deb), Some((6, 4, 13)));
        assert_eq!(running_cinnamon_version(&|_: &[&str]| None), None);
    }

    /// A table of cases with the behavior of the 6.6.9 shell.
    #[test]
    fn version_check_table() {
        let cur = (6, 6, 9);
        let weather = s(&["2.2", "2.4", "2.6", "2.8", "3.0", "3.2", "3.4", "3.6", "3.8", "4.0", "4.2", "4.4", "4.6", "4.8"]);
        assert!(version_check(&weather, cur), "the last 4.8 ≤ 6.6");
        assert!(!version_check(&s(&["6.8"]), cur));
        assert!(version_check(&s(&["6.6"]), cur));
        assert!(version_check(&s(&["6.6"]), (6, 6, 0)));
        assert!(!version_check(&s(&["6.6"]), (6, 4, 13)));
        assert!(version_check(&s(&["5.8", "7.0"]), cur), "one matching version is enough");
        assert!(version_check(&s(&["6.6.12"]), cur), "the third part is not compared");
        assert!(!version_check(&s(&[]), cur), "an empty list is incompatible");
        assert!(!version_check(&s(&["abc", "6"]), cur));
        assert!(version_check(&s(&["4.x2", "6.0beta"]), cur), "like parseInt: leading digits");
    }

    #[test]
    fn versions_from_metadata() {
        let m: Value = serde_json::from_str(r#"{"cinnamon-version": ["3.6", 4.0, "6.6"]}"#).unwrap();
        assert_eq!(cinnamon_versions(&m), Some(s(&["3.6", "4.0", "6.6"])));
        let one: Value = serde_json::from_str(r#"{"cinnamon-version": "6.0"}"#).unwrap();
        assert_eq!(cinnamon_versions(&one), Some(s(&["6.0"])));
        assert_eq!(cinnamon_versions(&serde_json::json!({})), None);
    }

    #[test]
    fn subdir_table() {
        let cur = (6, 6, 9);
        // weather@mockturtl on 6.6.9 takes 3.8.
        assert_eq!(pick_version_subdir(&s(&["3.0", "po", "icons", "3.8", "arrow-icons"]), cur).as_deref(), Some("3.8"));
        assert_eq!(pick_version_subdir(&s(&["po", "icons"]), cur), None);
        assert_eq!(pick_version_subdir(&s(&["6.8", "7.0"]), cur), None, "all newer — the root");
        assert_eq!(pick_version_subdir(&s(&["6.6", "6.6.9", "6.6.10"]), cur).as_deref(), Some("6.6.9"));
        assert_eq!(pick_version_subdir(&s(&["6.6", "6.6.0"]), (6, 6, 0)).as_deref(), Some("6.6.0"));
        assert_eq!(pick_version_subdir(&s(&["6.6.0", "6.6"]), (6, 6, 0)).as_deref(), Some("6.6.0"));
        assert_eq!(pick_version_subdir(&s(&["5.4", "6.10", "6.2"]), cur).as_deref(), Some("6.2"), "6.10 > 6.6 numerically");
        assert_eq!(pick_version_subdir(&s(&["4.0", "4.10"]), (4, 10, 0)).as_deref(), Some("4.10"));
        for bad in ["06.0", "6", "6.", ".6", "6.0.1.2", "6.a", "v6.0"] {
            assert_eq!(pick_version_subdir(&s(&[bad]), cur), None, "{bad}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  .po → .mo
// ═══════════════════════════════════════════════════════════════════════
//
// Spice translations arrive as `po/<lang>.po` sources, while gettext reads
// only a compiled `.mo`. We build it ourselves, without an external `msgfmt` (it may
// not be on the system). The file layout follows the format description in the
// GNU gettext manual ("The Format of GNU MO Files"): a header, two tables
// (length, offset), a hash table, NUL-terminated strings. The result matches
// `msgfmt` 1.0 byte for byte (checked in tests on real spice .po files).

/// One catalog entry: the key (`context\x04msgid[\0msgid_plural]`) and
/// the translation (forms separated by `\0`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PoEntry {
    key: Vec<u8>,
    value: Vec<u8>,
}

/// A quoted string: `"…"` with C escaping.
fn po_unquote(s: &str, line_no: usize) -> Result<Vec<u8>, String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|_| s.len() >= 2)
        .ok_or_else(|| format!("line {line_no}: a quoted string expected"))?;
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            return Err(format!("line {line_no}: a quote inside a string"));
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let e = *b.get(i).ok_or_else(|| format!("line {line_no}: truncated escape"))?;
        i += 1;
        match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' | b'?' => out.push(e),
            b'0'..=b'7' => {
                let mut v = u32::from(e - b'0');
                for _ in 0..2 {
                    match b.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(v as u8);
            }
            b'x' => {
                let start = i;
                let mut v: u32 = 0;
                while let Some(d) = b.get(i).and_then(|d| (*d as char).to_digit(16)) {
                    v = (v << 4 | d) & 0xff;
                    i += 1;
                }
                if i == start {
                    return Err(format!("line {line_no}: \\x without digits"));
                }
                out.push(v as u8);
            }
            other => return Err(format!("line {line_no}: unknown escape \\{}", other as char)),
        }
    }
    Ok(out)
}

#[derive(Default)]
struct PoDraft {
    ctx: Option<Vec<u8>>,
    id: Option<Vec<u8>>,
    plural: Option<Vec<u8>>,
    strs: Vec<(usize, Vec<u8>)>,
    fuzzy: bool,
    started_at: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum PoField {
    Ctx,
    Id,
    Plural,
    Str(usize),
}

/// Parse a `.po` into the entries that go into the `.mo`: without obsolete ones
/// (`#~`), without fuzzy ones (`#, fuzzy`, except the header) and without untranslated ones
/// (an empty first form). The order is as in the file.
fn parse_po(po: &str) -> Result<Vec<PoEntry>, String> {
    let mut out = Vec::new();
    let mut d = PoDraft::default();
    let mut field: Option<PoField> = None;
    let mut fuzzy_pending = false;

    fn finish(d: &mut PoDraft, out: &mut Vec<PoEntry>) -> Result<(), String> {
        let draft = std::mem::take(d);
        let Some(id) = draft.id else {
            if draft.ctx.is_some() || !draft.strs.is_empty() {
                return Err(format!("line {}: entry without msgid", draft.started_at));
            }
            return Ok(());
        };
        if draft.strs.is_empty() {
            return Err(format!("line {}: entry without msgstr", draft.started_at));
        }
        let mut forms = draft.strs;
        forms.sort_by_key(|(n, _)| *n);
        if forms.iter().enumerate().any(|(i, (n, _))| i != *n) {
            return Err(format!("line {}: msgstr[] forms out of order", draft.started_at));
        }
        if draft.plural.is_none() && forms.len() != 1 {
            return Err(format!("line {}: msgstr[] without msgid_plural", draft.started_at));
        }
        let is_header = id.is_empty() && draft.ctx.is_none();
        if (draft.fuzzy && !is_header) || forms[0].1.is_empty() {
            return Ok(());
        }
        let mut key = Vec::new();
        if let Some(c) = &draft.ctx {
            key.extend_from_slice(c);
            key.push(4);
        }
        key.extend_from_slice(&id);
        if let Some(p) = &draft.plural {
            key.push(0);
            key.extend_from_slice(p);
        }
        let mut value = Vec::new();
        for (i, (_, f)) in forms.iter().enumerate() {
            if i > 0 {
                value.push(0);
            }
            value.extend_from_slice(f);
        }
        if is_header {
            // As in msgfmt: the template creation date does not go into the compiled catalog
            // (otherwise one and the same build would give different files).
            let text = String::from_utf8_lossy(&value).into_owned();
            let kept: String = text.split_inclusive('\n').filter(|l| !l.starts_with("POT-Creation-Date:")).collect();
            value = kept.into_bytes();
        }
        out.push(PoEntry { key, value });
        Ok(())
    }

    for (n, raw) in po.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if line.starts_with("#~") || line.starts_with("#|") {
                continue;
            }
            if d.id.is_some() && !d.strs.is_empty() {
                finish(&mut d, &mut out)?;
                field = None;
            }
            if let Some(flags) = line.strip_prefix("#,") {
                if flags.split(',').any(|f| f.trim() == "fuzzy") {
                    fuzzy_pending = true;
                }
            }
            continue;
        }
        if line.starts_with('"') {
            let bytes = po_unquote(line, line_no)?;
            let target = match field {
                Some(PoField::Ctx) => d.ctx.as_mut(),
                Some(PoField::Id) => d.id.as_mut(),
                Some(PoField::Plural) => d.plural.as_mut(),
                Some(PoField::Str(i)) => d.strs.iter_mut().find(|(k, _)| *k == i).map(|(_, v)| v),
                None => None,
            }
            .ok_or_else(|| format!("line {line_no}: continuation without a keyword"))?;
            target.extend_from_slice(&bytes);
            continue;
        }
        let (kw, rest) = line.split_once(|c: char| c.is_whitespace() || c == '"').map_or((line, ""), |(k, _)| (k, &line[k.len()..]));
        let value = po_unquote(rest, line_no)?;
        let new_entry = matches!(kw, "msgctxt" | "msgid") && (!d.strs.is_empty() || (kw == "msgctxt" && d.id.is_some()));
        if new_entry {
            finish(&mut d, &mut out)?;
        }
        if d.ctx.is_none() && d.id.is_none() && d.strs.is_empty() {
            d.started_at = line_no;
            d.fuzzy = std::mem::take(&mut fuzzy_pending);
        }
        match kw {
            "msgctxt" => {
                if d.ctx.is_some() || d.id.is_some() {
                    return Err(format!("line {line_no}: extra msgctxt"));
                }
                d.ctx = Some(value);
                field = Some(PoField::Ctx);
            }
            "msgid" => {
                if d.id.is_some() {
                    return Err(format!("line {line_no}: a second msgid in a row"));
                }
                d.id = Some(value);
                field = Some(PoField::Id);
            }
            "msgid_plural" => {
                if d.id.is_none() || d.plural.is_some() || !d.strs.is_empty() {
                    return Err(format!("line {line_no}: msgid_plural out of place"));
                }
                d.plural = Some(value);
                field = Some(PoField::Plural);
            }
            _ => {
                let idx = if kw == "msgstr" {
                    if d.plural.is_some() {
                        return Err(format!("line {line_no}: a plural form needs msgstr[N]"));
                    }
                    0
                } else if let Some(num) = kw.strip_prefix("msgstr[").and_then(|r| r.strip_suffix(']')) {
                    if d.plural.is_none() {
                        return Err(format!("line {line_no}: msgstr[N] without msgid_plural"));
                    }
                    num.parse::<usize>().map_err(|_| format!("line {line_no}: form number \"{num}\""))?
                } else {
                    return Err(format!("line {line_no}: unknown keyword \"{kw}\""));
                };
                if d.id.is_none() || d.strs.iter().any(|(k, _)| *k == idx) {
                    return Err(format!("line {line_no}: msgstr out of place"));
                }
                d.strs.push((idx, value));
                field = Some(PoField::Str(idx));
            }
        }
    }
    finish(&mut d, &mut out)?;
    Ok(out)
}

/// A string hash for the `.mo` table (from the GNU gettext format description).
fn mo_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in s {
        h = (h << 4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}

fn next_prime(mut n: u32) -> u32 {
    let is_prime = |n: u32| n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0);
    n |= 1;
    while !is_prime(n) {
        n += 2;
    }
    n
}

/// Build a `.mo` (byte order — little-endian) from `.po` text.
/// An error — the file is corrupted (a repeated msgid, a garbage line, broken
/// escaping): such a language is skipped, the others are installed.
pub fn po_to_mo(po: &str) -> Result<Vec<u8>, String> {
    let mut entries = parse_po(po)?;
    // The lookup key — up to the first NUL (without msgid_plural).
    let lookup = |e: &PoEntry| -> Vec<u8> { e.key.split(|b| *b == 0).next().unwrap_or(&[]).to_vec() };
    entries.sort_by_key(lookup);
    if let Some(w) = entries.windows(2).find(|w| lookup(&w[0]) == lookup(&w[1])) {
        return Err(format!("duplicate entry \"{}\"", String::from_utf8_lossy(&lookup(&w[0]))));
    }
    let n = entries.len() as u32;
    let hash_size = if n == 0 { 0 } else { next_prime(n * 4 / 3).max(3) };
    let orig_at: u32 = 28;
    let trans_at = orig_at + n * 8;
    let hash_at = trans_at + n * 8;
    let mut strings_at = hash_at + hash_size * 4;

    let mut orig_table = Vec::new();
    let mut trans_table = Vec::new();
    let mut blob = Vec::new();
    for e in &entries {
        orig_table.push((e.key.len() as u32, strings_at));
        strings_at += e.key.len() as u32 + 1;
    }
    for e in &entries {
        trans_table.push((e.value.len() as u32, strings_at));
        strings_at += e.value.len() as u32 + 1;
    }
    for e in &entries {
        blob.extend_from_slice(&e.key);
        blob.push(0);
    }
    for e in &entries {
        blob.extend_from_slice(&e.value);
        blob.push(0);
    }
    let mut hash = vec![0u32; hash_size as usize];
    if hash_size > 2 {
        for (i, e) in entries.iter().enumerate() {
            let h = mo_hash(&lookup(e));
            let mut idx = h % hash_size;
            let incr = 1 + h % (hash_size - 2);
            while hash[idx as usize] != 0 {
                idx = if idx >= hash_size - incr { idx - (hash_size - incr) } else { idx + incr };
            }
            hash[idx as usize] = i as u32 + 1;
        }
    }

    let mut out = Vec::with_capacity(strings_at as usize);
    for w in [0x9504_12de_u32, 0, n, orig_at, trans_at, hash_size, hash_at] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    for (len, off) in orig_table.iter().chain(trans_table.iter()) {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for h in hash {
        out.extend_from_slice(&h.to_le_bytes());
    }
    out.extend_from_slice(&blob);
    Ok(out)
}

/// The language code from a `.po` name: everything before the first dot (`pt_BR.po` → `pt_BR`).
/// `None` — the name is unfit for a directory (`../x.po`, empty, not `.po`).
pub fn po_language(file_name: &str) -> Option<&str> {
    if !file_name.ends_with(".po") {
        return None;
    }
    let lang = file_name.split('.').next().unwrap_or("");
    let ok = !lang.is_empty() && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@-".contains(&b));
    ok.then_some(lang)
}

#[cfg(test)]
mod po_tests {
    use super::*;
    use crate::i18n::Catalog;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/po/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    const FIXTURES: &[&str] = &["powerman-cs.po", "1440-da.po", "backup-uk.po", "albumart-tr_TR.po"];

    // i18n-exempt-begin: test data, a catalog with non-ASCII translations
    const EDGE: &str = r#"
# шапка без пометок
msgid ""
msgstr ""
"Project-Id-Version: x\n"
"POT-Creation-Date: 2025-01-01 00:00+0000\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n>=2 && n<=4 ? 1 : 2);\n"

msgid "allempty"
msgid_plural "allempties"
msgstr[0] ""
msgstr[1] ""
msgstr[2] ""

msgid "partempty"
msgid_plural "partempties"
msgstr[0] "a"
msgstr[1] ""
msgstr[2] "c"

msgid "firstempty"
msgid_plural "firstempties"
msgstr[0] ""
msgstr[1] "b"
msgstr[2] "c"

msgctxt "ctx"
msgid ""
msgstr "ctx-empty-id"

msgctxt "menu"
msgid "Open"
msgstr "Открыть (меню)"

msgid "Open"
msgstr "Открыть"

#, fuzzy, javascript-format
msgid "fuzzy one"
msgstr "неточно"

msgid "esc"
msgstr "a\tb\\c\"d\101\x42"

msgid ""
"multi "
"line"
msgstr ""
"много"
"строк"

#~ msgid "obsolete"
#~ msgstr "old"
"#;
    // i18n-exempt-end

    #[test]
    fn roundtrip_through_our_reader() {
        let cat = Catalog::parse_mo(&po_to_mo(EDGE).unwrap()).unwrap();
        // i18n-exempt-begin: test data, translations from the catalog above
        assert_eq!(cat.get(None, "Open"), Some("Открыть"));
        assert_eq!(cat.get(Some("menu"), "Open"), Some("Открыть (меню)"));
        assert_eq!(cat.get(Some("ctx"), ""), Some("ctx-empty-id"));
        assert_eq!(cat.get(None, "esc"), Some("a\tb\\c\"dAB"));
        assert_eq!(cat.get(None, "multi line"), Some("многострок"));
        // i18n-exempt-end
        assert_eq!(cat.get(None, "fuzzy one"), None, "a fuzzy entry is not taken");
        assert_eq!(cat.get(None, "obsolete"), None);
        assert_eq!(cat.get(None, "allempty"), None);
        assert_eq!(cat.get(None, "firstempty"), None, "the first form is empty — not translated");
        assert_eq!(cat.plural_lookup("partempty", 1).as_deref(), Some("a"));
        assert_eq!(cat.plural_lookup("partempty", 7).as_deref(), Some("c"));
        assert_eq!(cat.nplurals(), 3);

        for name in FIXTURES {
            let po = fixture(name);
            let cat = Catalog::parse_mo(&po_to_mo(&po).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            // Every non-empty non-fuzzy entry was found.
            let entries = parse_po(&po).unwrap();
            assert!(entries.len() > 1, "{name}");
            for e in entries.iter().filter(|e| !e.key.is_empty()) {
                let key = String::from_utf8(e.key.clone()).unwrap();
                let (ctx, id) = match key.split_once('\u{4}') {
                    Some((c, i)) => (Some(c), i),
                    None => (None, key.as_str()),
                };
                let id = id.split('\0').next().unwrap();
                let first = String::from_utf8(e.value.split(|b| *b == 0).next().unwrap().to_vec()).unwrap();
                assert_eq!(cat.get(ctx, id), Some(first.as_str()), "{name}: {id}");
            }
        }
        let cs = Catalog::parse_mo(&po_to_mo(&fixture("powerman-cs.po")).unwrap()).unwrap();
        assert_eq!(cs.plural_lookup("hour", 1).as_deref(), Some("hodina"));
        assert_eq!(cs.plural_lookup("hour", 3).as_deref(), Some("hodiny"));
        assert_eq!(cs.plural_lookup("hour", 5).as_deref(), Some("hodin"));
        assert_eq!(cs.get(Some("time of battery remaining"), "%d:%02d"), Some("%d:%02d"));
        // Fuzzy ones are dropped: tr_TR has four, the header is kept.
        let tr_po = fixture("albumart-tr_TR.po");
        let kept = parse_po(&tr_po).unwrap();
        assert!(kept.iter().any(|e| e.key.is_empty()));
        let headers = kept.iter().find(|e| e.key.is_empty()).unwrap();
        assert!(!String::from_utf8_lossy(&headers.value).contains("POT-Creation-Date"));
    }

    /// A byte-for-byte check against `msgfmt` (if it is on the system).
    #[test]
    fn byte_identical_with_msgfmt() {
        let Ok(probe) = std::process::Command::new("msgfmt").arg("--version").output() else {
            eprintln!("no msgfmt — comparison NOT CHECKED");
            return;
        };
        if !probe.status.success() {
            eprintln!("msgfmt does not start — comparison NOT CHECKED");
            return;
        }
        let dir = std::env::temp_dir().join(format!("rexon-spices-po-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cases: Vec<(String, String)> = FIXTURES.iter().map(|n| (n.to_string(), fixture(n))).collect();
        cases.push(("edge.po".into(), EDGE.into()));
        for (name, po) in cases {
            let src = dir.join(&name);
            let dst = dir.join(format!("{name}.mo"));
            std::fs::write(&src, &po).unwrap();
            let st = std::process::Command::new("msgfmt").arg("-o").arg(&dst).arg(&src).status().unwrap();
            assert!(st.success(), "{name}: msgfmt refused");
            let want = std::fs::read(&dst).unwrap();
            let got = po_to_mo(&po).unwrap();
            assert_eq!(got, want, "{name}: .mo differs from msgfmt");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A manual run: every `.po` under `REXON_SPICES_PO_DIR` (for example
    /// unpacked spice archives) against `msgfmt`, byte for byte.
    #[test]
    #[ignore]
    fn msgfmt_on_directory() {
        let Ok(root) = std::env::var("REXON_SPICES_PO_DIR") else { return };
        let out = std::env::temp_dir().join(format!("rexon-spices-po-dir-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let (mut same, mut total) = (0, 0);
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().and_then(|x| x.to_str()) != Some("po") {
                    continue;
                }
                total += 1;
                let dst = out.join("x.mo");
                let ok = std::process::Command::new("msgfmt").arg("-o").arg(&dst).arg(&p).status().unwrap().success();
                let ours = po_to_mo(&std::fs::read_to_string(&p).unwrap());
                match (ok, ours) {
                    (true, Ok(m)) if m == std::fs::read(&dst).unwrap() => same += 1,
                    (false, Err(_)) => same += 1,
                    (ok, ours) => eprintln!("DIFFERS {}: msgfmt={ok} ours={:?}", p.display(), ours.map(|m| m.len())),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&out);
        eprintln!("matched {same} of {total}");
        assert_eq!(same, total);
    }

    #[test]
    fn broken_po_is_an_error() {
        for bad in [
            "msgid \"a\"\nmsgstr \"b\"\nmsgid \"a\"\nmsgstr \"c\"\n",
            "msgid \"a\"\nmsgstr \"b\\q\"\n",
            "msgid \"a\nmsgstr \"b\"\n",
            "garbage line\n",
            "msgid \"a\"\n",
            "msgid \"a\"\nmsgstr[0] \"b\"\n",
            "msgid \"a\"\nmsgid_plural \"as\"\nmsgstr \"b\"\n",
            "\"orphan\"\n",
        ] {
            assert!(po_to_mo(bad).is_err(), "{bad:?}");
        }
        assert!(po_to_mo("").unwrap().len() == 28, "an empty catalog is one header");
    }

    #[test]
    fn language_from_file_name() {
        assert_eq!(po_language("pt_BR.po"), Some("pt_BR"));
        assert_eq!(po_language("tr_TR.po"), Some("tr_TR"));
        assert_eq!(po_language("sr@latin.po"), Some("sr@latin"));
        assert_eq!(po_language("ru.utf8.po"), Some("ru"));
        assert_eq!(po_language("weather@mockturtl.pot"), None);
        assert_eq!(po_language(".po"), None);
        assert_eq!(po_language("..po"), None);
        assert_eq!(po_language("a b.po"), None);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Index cache
// ═══════════════════════════════════════════════════════════════════════

/// The cache file of one index: the body as it arrived plus what the conditional
/// request needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CachedIndex {
    pub version: u32,
    /// When the index was last confirmed by the server (200 or 304),
    /// unix seconds. 0 — never.
    pub fetched: i64,
    /// When the last attempt to get the index failed (0 — never).
    pub failed: i64,
    pub etag: String,
    pub last_modified: String,
    /// The index body (the site's JSON) as is.
    pub body: String,
}

/// The cache path: `$XDG_CACHE_HOME/rexon/cinnamon-spices/<json>.json`.
pub fn cache_path(xdg_cache_home: Option<&str>, home: &str, t: &SpiceType) -> String {
    format!("{}/rexon/cinnamon-spices/{}.json", crate::utils::cache_home(xdg_cache_home, home), t.json)
}

/// [`cache_path`] of this process.
pub fn live_cache_path(t: &SpiceType) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home, t)
}

/// Read the cache. A foreign version, a broken file and a body that does not
/// parse — as if there is no cache (i.e. it is stale).
pub fn load_cache(path: &Path) -> Option<(CachedIndex, Vec<Spice>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let c: CachedIndex = serde_json::from_str(&text).ok()?;
    if c.version != CACHE_VERSION {
        return None;
    }
    let list = if c.body.is_empty() { None } else { parse_index(&c.body).ok() };
    match list {
        Some(list) => Some((c, list)),
        // No body (only a failure mark) — the validators are no good.
        None => Some((CachedIndex { etag: String::new(), last_modified: String::new(), body: String::new(), fetched: 0, ..c }, Vec::new())),
    }
}

/// Write the cache (via a temporary file — there are no half-written ones).
pub fn save_cache(path: &Path, c: &CachedIndex) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(c).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// The answer to a conditional request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondReply {
    /// 304: we have the latest version.
    NotModified,
    /// 200 with a body and new validators (empty — the server did not give them).
    Body { body: Vec<u8>, etag: String, last_modified: String },
}

/// The conditional GET transport. The shared [`crate::net::NetIo`] has neither response
/// headers nor per-call request headers, and without them `ETag`/`304` cannot be
/// done — so here is our own narrow trait: in checks it answers with
/// recorded responses, in production — [`UreqConditional`].
pub trait ConditionalNet {
    /// `GET url` with `If-None-Match`/`If-Modified-Since` (empty ones are not
    /// sent). A response outside 2xx and not 304 — `NetError::Status`.
    fn conditional_get(
        &mut self,
        url: &str,
        user_agent: &str,
        etag: &str,
        last_modified: &str,
        timeout_secs: u64,
    ) -> Result<CondReply, crate::net::NetError>;
}

/// How the index refresh ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshSource {
    /// The cache is fresh, the network was not used.
    Cache,
    /// The server answered 304 — the cache was extended.
    NotModified,
    /// A new index arrived.
    Downloaded,
    /// The network/server let us down — the previous cache is shown (possibly empty).
    Stale,
}

/// The result of [`refresh`].
#[derive(Debug, Clone, PartialEq)]
pub struct Refresh {
    pub spices: Vec<Spice>,
    pub source: RefreshSource,
    /// When the list was last confirmed by the server (0 — never).
    pub fetched: i64,
    /// What to tell the person; empty — all is well.
    pub note: String,
}

/// No network / the server did not answer.
pub fn offline_message(have_saved: bool) -> String {
    if have_saved {
        tr("Couldn't reach the Cinnamon Spices website — the saved list is shown.")
    } else {
        tr("Couldn't reach the Cinnamon Spices website. Check your internet connection and try again.")
    }
}

/// The index for display. A fresh cache (younger than a day) — without network. Otherwise (or with
/// `force` — the "Refresh" button) — a conditional request with the cache validators:
/// 304 extends the cache, 200 replaces it. A network failure, a server error response
/// or garbage instead of JSON — the previous cache with a mark, and no retry
/// before [`FAIL_BACKOFF_SECS`] (except with `force`).
pub fn refresh(net: &mut dyn ConditionalNet, path: &Path, t: &SpiceType, force: bool, now: i64) -> Refresh {
    let cached = load_cache(path);
    let (meta, saved) = cached.unwrap_or_else(|| (CachedIndex { version: CACHE_VERSION, ..Default::default() }, Vec::new()));
    let have_body = !meta.body.is_empty();
    let fresh = have_body && meta.fetched > 0 && now >= meta.fetched && now - meta.fetched < CACHE_TTL_SECS;
    let backing_off = meta.failed > 0 && now >= meta.failed && now - meta.failed < FAIL_BACKOFF_SECS;
    if !force && (fresh || backing_off) {
        let note = if fresh { String::new() } else { offline_message(have_body) };
        let source = if fresh { RefreshSource::Cache } else { RefreshSource::Stale };
        return Refresh { spices: saved, source, fetched: meta.fetched, note };
    }

    let (etag, lm) = if have_body { (meta.etag.as_str(), meta.last_modified.as_str()) } else { ("", "") };
    let reply = net.conditional_get(&index_url(t), user_agent(), etag, lm, INDEX_TIMEOUT_SECS);
    let fail = |meta: CachedIndex, saved: Vec<Spice>| {
        let have = !meta.body.is_empty();
        let fetched = meta.fetched;
        let _ = save_cache(path, &CachedIndex { failed: now, ..meta });
        Refresh { spices: saved, source: RefreshSource::Stale, fetched, note: offline_message(have) }
    };
    match reply {
        Ok(CondReply::NotModified) if have_body => {
            let _ = save_cache(path, &CachedIndex { fetched: now, failed: 0, ..meta });
            Refresh { spices: saved, source: RefreshSource::NotModified, fetched: now, note: String::new() }
        }
        Ok(CondReply::Body { body, etag, last_modified }) => {
            let text = String::from_utf8_lossy(&body).into_owned();
            match parse_index(&text) {
                Ok(list) => {
                    let c = CachedIndex { version: CACHE_VERSION, fetched: now, failed: 0, etag, last_modified, body: text };
                    let _ = save_cache(path, &c);
                    Refresh { spices: list, source: RefreshSource::Downloaded, fetched: now, note: String::new() }
                }
                // Garbage (an error page instead of JSON) does not overwrite the cache.
                Err(_) => fail(meta, saved),
            }
        }
        // 304 without a cache (we sent no validators) — a strange answer.
        Ok(CondReply::NotModified) | Err(_) => fail(meta, saved),
    }
}

/// The production conditional request transport: `ureq` with the same settings as
/// the shared network layer (`crate::net::UreqNet`): the system root store
/// via native-tls, a timeout on connecting and on every read, up to 10
/// redirects. Without `Accept-Encoding` — the server sends as is.
pub struct UreqConditional {
    agent: Option<(u64, ureq::Agent)>,
    /// A body larger than this is not read (after decompression).
    limit: u64,
    /// Ask for a compressed response (`Accept-Encoding: gzip`) and decompress it:
    /// GitHub's theme repository tree weighs ~9 MB, compressed — ~1.2 MB.
    gzip: bool,
}

impl Default for UreqConditional {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqConditional {
    pub fn new() -> Self {
        UreqConditional { agent: None, limit: MAX_INDEX_BYTES, gzip: false }
    }

    /// A compressed response and its own size ceiling (for [`THEME_TREE_URL`]).
    pub fn gzip_up_to(limit: u64) -> Self {
        UreqConditional { agent: None, limit, gzip: true }
    }

    fn agent(&mut self, timeout_secs: u64) -> ureq::Agent {
        if let Some((t, a)) = &self.agent {
            if *t == timeout_secs {
                return a.clone();
            }
        }
        let t = std::time::Duration::from_secs(timeout_secs);
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(t))
            .timeout_send_request(Some(t))
            .timeout_recv_response(Some(t))
            .timeout_recv_body(Some(t))
            .max_redirects(10)
            .http_status_as_error(false)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        let agent = ureq::Agent::new_with_config(config);
        self.agent = Some((timeout_secs, agent.clone()));
        agent
    }
}

impl ConditionalNet for UreqConditional {
    fn conditional_get(
        &mut self,
        url: &str,
        user_agent: &str,
        etag: &str,
        last_modified: &str,
        timeout_secs: u64,
    ) -> Result<CondReply, crate::net::NetError> {
        use crate::net::NetError;
        use std::io::Read;
        let mut req = self.agent(timeout_secs).get(url).header("User-Agent", user_agent);
        if !etag.is_empty() {
            req = req.header("If-None-Match", etag);
        }
        if !last_modified.is_empty() {
            req = req.header("If-Modified-Since", last_modified);
        }
        if self.gzip {
            req = req.header("Accept-Encoding", "gzip");
        }
        let resp = req.call().map_err(|e| NetError::Transport { detail: format!("<urlopen error {e}>") })?;
        let code = resp.status().as_u16();
        if code == 304 {
            return Ok(CondReply::NotModified);
        }
        if !(200..300).contains(&code) {
            return Err(NetError::Status { code, reason: resp.status().canonical_reason().unwrap_or("").to_string() });
        }
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
        let (etag, last_modified) = (header("etag"), header("last-modified"));
        let gzipped = self.gzip && header("content-encoding").eq_ignore_ascii_case("gzip");
        let limit = self.limit;
        let mut body = Vec::new();
        let reader = resp.into_body().into_reader();
        let read = if gzipped {
            flate2::read::GzDecoder::new(reader.take(limit)).take(limit).read_to_end(&mut body)
        } else {
            reader.take(limit).read_to_end(&mut body)
        };
        read.map_err(|e| NetError::Transport { detail: e.to_string() })?;
        Ok(CondReply::Body { body, etag, last_modified })
    }
}

/// [`refresh`] on the live machine (network, this process's cache).
pub fn live_refresh(t: &SpiceType, force: bool) -> Refresh {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    refresh(&mut UreqConditional::new(), Path::new(&live_cache_path(t)), t, force, now)
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::net::NetError;

    /// Recorded responses in order; the requests are collected for the check.
    struct Replay {
        answers: Vec<Result<CondReply, NetError>>,
        asked: Vec<(String, String, String, String)>,
    }

    impl ConditionalNet for Replay {
        fn conditional_get(&mut self, url: &str, ua: &str, etag: &str, lm: &str, _t: u64) -> Result<CondReply, NetError> {
            self.asked.push((url.into(), ua.into(), etag.into(), lm.into()));
            assert!(!self.answers.is_empty(), "an extra request to the site");
            self.answers.remove(0)
        }
    }

    fn replay(answers: Vec<Result<CondReply, NetError>>) -> Replay {
        Replay { answers, asked: Vec::new() }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-spices-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("rexon/cinnamon-spices/applets.json")
    }

    fn body() -> Vec<u8> {
        std::fs::read(format!("{}/tests/data/cinnamon_spices/applets.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn ok200(etag: &str) -> Result<CondReply, NetError> {
        Ok(CondReply::Body { body: body(), etag: etag.into(), last_modified: "Fri, 16 Oct 2026 22:15:06 GMT".into() })
    }

    const T0: i64 = 1_792_000_000;

    fn applets() -> &'static SpiceType {
        type_for_key("applet").unwrap()
    }

    #[test]
    fn path_layout() {
        assert_eq!(cache_path(None, "/home/u", applets()), "/home/u/.cache/rexon/cinnamon-spices/applets.json");
        assert_eq!(cache_path(Some("/c"), "/home/u", type_for_key("theme").unwrap()), "/c/rexon/cinnamon-spices/themes.json");
    }

    #[test]
    fn first_fetch_then_day_of_silence_then_304() {
        let p = tmp("flow");
        let mut net = replay(vec![ok200("W/\"ddfd-1\"")]);
        let r = refresh(&mut net, &p, applets(), false, T0);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Downloaded, 6, T0));
        assert!(r.note.is_empty());
        assert_eq!(net.asked[0], ("https://cinnamon-spices.linuxmint.com/json/applets.json".into(), "Rexon/5.0".into(), String::new(), String::new()));

        // Younger than a day — not a single request.
        let mut quiet = replay(vec![]);
        let r = refresh(&mut quiet, &p, applets(), false, T0 + CACHE_TTL_SECS - 1);
        assert_eq!((r.source, r.spices.len()), (RefreshSource::Cache, 6));
        assert!(quiet.asked.is_empty());

        // A day passed — a conditional request with validators; 304 extends.
        let mut net = replay(vec![Ok(CondReply::NotModified)]);
        let r = refresh(&mut net, &p, applets(), false, T0 + CACHE_TTL_SECS);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::NotModified, 6, T0 + CACHE_TTL_SECS));
        assert_eq!(net.asked[0].2, "W/\"ddfd-1\"");
        assert_eq!(net.asked[0].3, "Fri, 16 Oct 2026 22:15:06 GMT");
        assert!(!net.asked[0].0.contains('?'), "without the time parameter");
        let (c, _) = load_cache(&p).unwrap();
        assert_eq!(c.fetched, T0 + CACHE_TTL_SECS);

        // "Refresh" — to the network even with a fresh cache; a new body replaces it.
        let mut net = replay(vec![ok200("W/\"ddfd-2\"")]);
        let r = refresh(&mut net, &p, applets(), true, T0 + CACHE_TTL_SECS + 5);
        assert_eq!(r.source, RefreshSource::Downloaded);
        assert_eq!(load_cache(&p).unwrap().0.etag, "W/\"ddfd-2\"");
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn server_error_and_broken_connection_keep_old_cache_and_back_off() {
        for (name, err) in [
            ("500", NetError::Status { code: 500, reason: "Internal Server Error".into() }),
            ("eof", NetError::Transport { detail: "<urlopen error connection closed>".into() }),
        ] {
            let p = tmp(name);
            refresh(&mut replay(vec![ok200("e1")]), &p, applets(), false, T0);
            let later = T0 + CACHE_TTL_SECS + 10;
            let mut net = replay(vec![Err(err)]);
            let r = refresh(&mut net, &p, applets(), false, later);
            assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Stale, 6, T0), "{name}");
            assert_eq!(r.note, offline_message(true));
            // Right away again — we do not knock (the pause after a failure).
            let mut quiet = replay(vec![]);
            let r = refresh(&mut quiet, &p, applets(), false, later + 60);
            assert_eq!((r.source, r.spices.len()), (RefreshSource::Stale, 6));
            assert!(quiet.asked.is_empty());
            // "Refresh" tries right away; after the pause — too.
            let mut net = replay(vec![Ok(CondReply::NotModified)]);
            assert_eq!(refresh(&mut net, &p, applets(), true, later + 61).source, RefreshSource::NotModified);
            let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
        }
    }

    #[test]
    fn garbage_body_does_not_replace_cache() {
        let p = tmp("garbage");
        refresh(&mut replay(vec![ok200("e1")]), &p, applets(), false, T0);
        let later = T0 + CACHE_TTL_SECS;
        let html = Ok(CondReply::Body { body: b"<html>Access Denied - Sucuri Website Firewall</html>".to_vec(), etag: "x".into(), last_modified: String::new() });
        let r = refresh(&mut replay(vec![html]), &p, applets(), false, later);
        assert_eq!((r.source, r.spices.len()), (RefreshSource::Stale, 6));
        let (c, list) = load_cache(&p).unwrap();
        assert_eq!((c.etag.as_str(), list.len(), c.failed), ("e1", 6, later));
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn no_cache_and_no_network() {
        let p = tmp("offline");
        let mut net = replay(vec![Err(NetError::Transport { detail: "dns".into() })]);
        let r = refresh(&mut net, &p, applets(), false, T0);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Stale, 0, 0));
        assert_eq!(r.note, offline_message(false));
        // A failure mark does not prevent a clean download later, without validators.
        let mut net = replay(vec![ok200("e")]);
        let r = refresh(&mut net, &p, applets(), false, T0 + FAIL_BACKOFF_SECS);
        assert_eq!(r.source, RefreshSource::Downloaded);
        assert_eq!((net.asked[0].2.as_str(), net.asked[0].3.as_str()), ("", ""));
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn broken_cache_file_counts_as_expired() {
        for junk in ["not json", r#"{"version": 99, "fetched": 1, "body": "{}"}"#, r#"{"version": 1, "fetched": 1792000000, "etag": "e", "body": "[broken"}"#] {
            let p = tmp("junk");
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, junk).unwrap();
            let mut net = replay(vec![ok200("e2")]);
            let r = refresh(&mut net, &p, applets(), false, T0 + 1);
            assert_eq!(r.source, RefreshSource::Downloaded, "{junk}");
            assert_eq!(net.asked[0].2, "", "validators of a broken cache are not sent: {junk}");
            let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
        }
    }

    /// The production transport against a local server: request headers and parsing
    /// 200 / 304 / 500.
    #[test]
    fn live_transport_sends_validators_and_reads_them_back() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let answers = [
                "HTTP/1.1 200 OK\r\nETag: W/\"a1\"\r\nLast-Modified: Fri, 16 Oct 2026 22:15:06 GMT\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ];
            let mut heads = Vec::new();
            for a in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 4096];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(&buf[..n]);
                }
                heads.push(String::from_utf8_lossy(&got).to_lowercase());
                s.write_all(a.as_bytes()).unwrap();
            }
            heads
        });
        let url = format!("http://127.0.0.1:{port}/json/applets.json");
        let mut net = UreqConditional::new();
        let first = net.conditional_get(&url, "Rexon/5.0", "", "", 5).unwrap();
        assert_eq!(first, CondReply::Body { body: b"{}".to_vec(), etag: "W/\"a1\"".into(), last_modified: "Fri, 16 Oct 2026 22:15:06 GMT".into() });
        assert_eq!(net.conditional_get(&url, "Rexon/5.0", "W/\"a1\"", "Fri, 16 Oct 2026 22:15:06 GMT", 5).unwrap(), CondReply::NotModified);
        assert!(matches!(net.conditional_get(&url, "Rexon/5.0", "", "", 5), Err(NetError::Status { code: 500, .. })));
        let heads = server.join().unwrap();
        assert!(heads[0].contains("user-agent: rexon/5.0"));
        assert!(!heads[0].contains("if-none-match") && !heads[0].contains("if-modified-since"));
        assert!(!heads[0].contains("accept-encoding"), "compression is not requested");
        assert!(heads[1].contains("if-none-match: w/\"a1\""));
        assert!(heads[1].contains("if-modified-since: fri, 16 oct 2026 22:15:06 gmt"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Archive check and install plan
// ═══════════════════════════════════════════════════════════════════════

use crate::installed_scan::FsSnapshot;

/// Why a spice will not install. The text for the person — [`SpiceRefusal::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpiceRefusal {
    /// The key from the index is unfit for a directory name.
    InvalidKey,
    /// An archive entry leads outside the unpack directory (`../`, an absolute path).
    UnsafePath { entry: String },
    /// There is no `<key>/` directory inside.
    NotZipLayout,
    /// No `metadata.json` (xlets and actions).
    NoMetadata,
    /// `metadata.json` does not parse as a JSON object.
    BadMetadata,
    /// The `uuid` in `metadata.json` does not match the directory: Cinnamon does not load such a spice.
    UuidMismatch { found: String },
    /// A required `metadata.json` field is missing (`name`, `description`).
    MissingProperty { name: String },
    /// None of the `cinnamon-version` versions fits.
    Incompatible { need: Vec<String>, current: (u32, u32, u32) },
    /// The extension has no `cinnamon-version`: Cinnamon does not load such a thing.
    MissingCinnamonVersion,
    /// No main file (`applet.js`…) in the directory Cinnamon will take.
    NoMainJs { file: String },
    /// The theme has no `cinnamon/cinnamon.css`.
    NoCinnamonCss,
    /// The action has no `<uuid>.nemo_action`.
    NoActionFile,
}

/// "Cinnamon N or newer is needed, but here it is M" — the refusal when installing
/// an incompatible spice (Wehrwolfmann's decision of 17.10.2026: the storefront
/// shows all of them, the check happens at install time).
pub fn incompatible_message(name: &str, need: &[String], current: (u32, u32, u32)) -> String {
    let lowest = need
        .iter()
        .filter_map(|v| parse_version(v).map(|p| (p, v)))
        .min_by_key(|(p, _)| *p)
        .map(|(_, v)| v.clone());
    let cur = format!("{}.{}.{}", current.0, current.1, current.2);
    match lowest {
        Some(v) if parse_version(&v).is_some_and(|p| p > current) => trf(
            "%1 needs Cinnamon %2 or newer, and this computer has Cinnamon %3. Update Cinnamon or choose another item.",
            &[&name, &v, &cur],
        ),
        _ => trf(
            "%1 doesn't support Cinnamon %2 (the version on this computer), so Cinnamon wouldn't load it.",
            &[&name, &cur],
        ),
    }
}

impl SpiceRefusal {
    /// The explanation for the person; `name` — the spice name on the card.
    pub fn message(&self, name: &str) -> String {
        match self {
            SpiceRefusal::InvalidKey => trf("%1 has a name that can't be used as a folder name, so it can't be installed.", &[&name]),
            SpiceRefusal::UnsafePath { entry } => {
                trf("The archive of %1 contains an unsafe path (%2), so it wasn't installed.", &[&name, entry])
            }
            SpiceRefusal::NotZipLayout => trf("The archive of %1 isn't laid out the way Cinnamon expects, so it wasn't installed.", &[&name]),
            SpiceRefusal::NoMetadata => trf("%1 is damaged: its description file metadata.json is missing.", &[&name]),
            SpiceRefusal::BadMetadata => trf("%1 is damaged: its description file metadata.json can't be read.", &[&name]),
            SpiceRefusal::UuidMismatch { found } => trf(
                "%1 is damaged: its description names it «%2», so Cinnamon wouldn't load it.",
                &[&name, found],
            ),
            SpiceRefusal::MissingProperty { name: field } => trf(
                "%1 is damaged: its description file has no «%2» field, so Cinnamon wouldn't load it.",
                &[&name, field],
            ),
            SpiceRefusal::Incompatible { need, current } => incompatible_message(name, need, *current),
            SpiceRefusal::MissingCinnamonVersion => trf(
                "%1 doesn't say which Cinnamon versions it supports, and Cinnamon doesn't load such extensions.",
                &[&name],
            ),
            SpiceRefusal::NoMainJs { file } => trf("%1 is damaged: the file %2 is missing.", &[&name, file]),
            SpiceRefusal::NoCinnamonCss => trf("%1 isn't a Cinnamon theme: it has no cinnamon/cinnamon.css.", &[&name]),
            SpiceRefusal::NoActionFile => trf("%1 is damaged: the Nemo action file is missing.", &[&name]),
        }
    }
}

/// All archive entries stay inside the unpack directory. Check BEFORE
/// unpacking (the same check as the shared appearance unpacker's).
pub fn check_entry_names(names: &[String]) -> Result<(), SpiceRefusal> {
    const PROBE: &str = "/rexon-spice-extract";
    match names.iter().find(|n| n.contains('\\') || !crate::appearance_installer::safe_extract_entry(PROBE, n)) {
        Some(bad) => Err(SpiceRefusal::UnsafePath { entry: bad.clone() }),
        None => Ok(()),
    }
}

/// What was found in the archive — for the plan.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// `metadata.json` (for a theme without it — an empty object).
    pub meta: Value,
    /// The version subdirectory Cinnamon will take (`3.8`), or the root.
    pub version_subdir: Option<String>,
    /// Names of `po/<lang>.po` in the spice root, alphabetically (empty for themes).
    pub po_files: Vec<String>,
}

fn snap_join(a: &str, b: &str) -> String {
    format!("{}/{b}", a.trim_end_matches('/'))
}

/// Check an unpacked archive (`root` — the unpack directory, with
/// `<key>/` in it). `current` — the Cinnamon version on the machine; `None` (Cinnamon not
/// found) — the version is not checked, the main file is searched for in the root and in
/// the version subdirectories.
pub fn check_layout(
    fs: &FsSnapshot,
    root: &str,
    t: &SpiceType,
    key: &str,
    current: Option<(u32, u32, u32)>,
) -> Result<Layout, SpiceRefusal> {
    if !valid_key(key) {
        return Err(SpiceRefusal::InvalidKey);
    }
    let dir = snap_join(root, key);
    if !fs.is_dir(&dir) {
        return Err(SpiceRefusal::NotZipLayout);
    }
    let meta_path = snap_join(&dir, "metadata.json");
    let meta: Value = match fs.read_text(&meta_path) {
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(v) if v.is_object() => v,
            _ => return Err(SpiceRefusal::BadMetadata),
        },
        None if t.is_theme() => Value::Object(Map::new()),
        None => return Err(SpiceRefusal::NoMetadata),
    };

    if t.is_theme() {
        if !fs.is_file(&snap_join(&dir, "cinnamon/cinnamon.css")) {
            return Err(SpiceRefusal::NoCinnamonCss);
        }
        return Ok(Layout { meta, version_subdir: None, po_files: Vec::new() });
    }

    let uuid = meta.get("uuid").and_then(Value::as_str).unwrap_or("");
    if uuid != key {
        return Err(SpiceRefusal::UuidMismatch { found: uuid.to_string() });
    }
    if t.is_action() && !fs.is_file(&snap_join(root, &format!("{key}.nemo_action"))) {
        return Err(SpiceRefusal::NoActionFile);
    }

    let mut version_subdir = None;
    if let Some(main) = t.main_js {
        for field in ["name", "description"] {
            if meta.get(field).is_none() {
                return Err(SpiceRefusal::MissingProperty { name: field.to_string() });
            }
        }
        match (cinnamon_versions(&meta), current) {
            (None, _) if t.key == "extension" => return Err(SpiceRefusal::MissingCinnamonVersion),
            (Some(need), Some(cur)) if !version_check(&need, cur) => {
                return Err(SpiceRefusal::Incompatible { need, current: cur });
            }
            _ => {}
        }
        let subdirs: Vec<String> = fs.scandir(&dir).iter().filter(|n| fs.is_dir(&snap_join(&dir, n))).cloned().collect();
        let candidates: Vec<Option<String>> = match current {
            Some(cur) => vec![pick_version_subdir(&subdirs, cur)],
            // The version is unknown: any version subdirectory or the root will do.
            None => {
                let mut c: Vec<Option<String>> = subdirs.iter().filter(|n| version_dir(n).is_some()).cloned().map(Some).collect();
                c.sort_by_key(|n| std::cmp::Reverse(n.as_deref().and_then(version_dir)));
                c.push(None);
                c
            }
        };
        let found = candidates.iter().find(|sub| {
            let code = sub.as_deref().map_or(dir.clone(), |s| snap_join(&dir, s));
            fs.is_file(&snap_join(&code, main))
        });
        match found {
            Some(sub) => version_subdir = sub.clone(),
            None => {
                let shown = candidates[0].as_deref().map_or(main.to_string(), |s| format!("{s}/{main}"));
                return Err(SpiceRefusal::NoMainJs { file: shown });
            }
        }
    }

    let po_dir = snap_join(&dir, "po");
    let mut po_files: Vec<String> = fs
        .scandir(&po_dir)
        .iter()
        .filter(|n| po_language(n).is_some() && fs.is_file(&snap_join(&po_dir, n)))
        .cloned()
        .collect();
    po_files.sort();
    Ok(Layout { meta, version_subdir, po_files })
}

/// Where the person lives: everything is installed under `home` (as Cinnamon looks for it:
/// `~/.local/share/cinnamon/…`, `~/.themes`, `~/.local/share/nemo/actions`,
/// translations — `~/.local/share/locale`). Cinnamon keeps spice settings
/// in the XDG config directory (`$XDG_CONFIG_HOME`, usually `~/.config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiceEnv {
    pub home: String,
    /// The XDG config directory: `$XDG_CONFIG_HOME` or `~/.config`.
    pub config: String,
}

impl SpiceEnv {
    /// Under the home directory `home`, settings — in `~/.config`.
    pub fn at_home(home: &str) -> Self {
        let home = home.trim_end_matches('/').to_string();
        SpiceEnv { config: format!("{home}/.config"), home }
    }

    /// Under `home`: the live config directory if this is the process's home
    /// directory, otherwise `home/.config`.
    pub fn for_home(home: &str) -> Self {
        let live = Self::live();
        if live.home == home.trim_end_matches('/') {
            live
        } else {
            Self::at_home(home)
        }
    }

    /// The live machine: `$HOME` and `$XDG_CONFIG_HOME` (empty — as if not set).
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let mut env = Self::at_home(&home);
        if let Some(c) = std::env::var("XDG_CONFIG_HOME").ok().filter(|c| c.starts_with('/')) {
            env.config = c.trim_end_matches('/').to_string();
        }
        env
    }
}

/// The install circumstances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstallContext {
    /// The spice is already installed (the directory exists) — this is an update.
    pub is_update: bool,
    /// The spice is currently enabled (it is in the enabled list).
    pub enabled: bool,
    /// The Cinnamon version on the machine.
    pub cinnamon: Option<(u32, u32, u32)>,
}

/// An install plan step — one side effect for the executor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SpiceStep {
    /// `mkdir -p`.
    MakeDirs { path: String },
    /// Delete a directory if it exists.
    RemoveTree { path: String },
    /// Delete a file if it exists.
    RemoveFile { path: String },
    /// Copy a tree (file permissions as in the archive).
    CopyTree { from: String, to: String },
    /// Copy a file.
    CopyFile { from: String, to: String },
    /// Build a `.mo` from a `.po` ([`po_to_mo`]); a failure is a line in the log and
    /// the language is skipped, the installation continues.
    CompilePo { po: String, mo: String },
    /// Append `"last-edited": ts` to `metadata.json` (keep the order of the other
    /// keys, indent 4). `create` — the file may not exist (a theme):
    /// then create it from this single field.
    WriteLastEdited { meta: String, ts: i64, create: bool },
    /// Add a string to a gsettings list if it is not there.
    GsettingsListAdd { schema: String, key: String, item: String },
    /// Remove all of the spice's entries from a gsettings list ([`list_without`]):
    /// `field` — the number of the `:`-separated field of an entry (3 for applets, 0 for desklets),
    /// `None` — the whole entry (extensions, Nemo actions).
    GsettingsListDrop { schema: String, key: String, item: String, field: Option<u32> },
    /// Reset a gsettings key to the default if it currently equals `value`
    /// (the shell theme being removed is selected now).
    GsettingsResetIf { schema: String, key: String, value: String },
    /// Delete a directory only if it is empty (translation language directories).
    RemoveDirIfEmpty { path: String },
    /// Forget the installed log record.
    Forget { content_id: String },
    /// `org.Cinnamon.ReloadXlet(uuid, type)` — an enabled xlet picks up
    /// the new code without restarting the shell.
    ReloadXlet { uuid: String, xlet: String },
    /// A record in the installed log.
    Record { content_id: String, provider: String, web_url: String },
}

/// The type's install directory (`~/.themes`, `~/.local/share/cinnamon/applets`…).
pub fn install_root(env: &SpiceEnv, t: &SpiceType) -> String {
    let dest = t.row().map_or("", |r| r.dest);
    snap_join(&env.home, dest)
}

/// The translations directory: `~/.local/share/locale/<lang>/LC_MESSAGES`.
pub fn locale_dir(env: &SpiceEnv, lang: &str) -> String {
    format!("{}/.local/share/locale/{lang}/LC_MESSAGES", env.home.trim_end_matches('/'))
}

/// The install plan of an unpacked archive `extracted` (with `<key>/` in it, for
/// actions also `<key>.nemo_action`). A refusal comes before the first side
/// effect.
pub fn plan_install(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    fs: &FsSnapshot,
    extracted: &str,
    ctx: &InstallContext,
) -> Result<Vec<SpiceStep>, SpiceRefusal> {
    let key = s.key.as_str();
    let layout = check_layout(fs, extracted, t, key, ctx.cinnamon)?;
    let root = install_root(env, t);
    let dest = snap_join(&root, key);
    let src = snap_join(extracted, key);
    let mut steps = vec![SpiceStep::MakeDirs { path: root.clone() }, SpiceStep::RemoveTree { path: dest.clone() }];
    let action_file = format!("{key}.nemo_action");
    if t.is_action() {
        steps.push(SpiceStep::RemoveFile { path: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::CopyTree { from: src.clone(), to: dest.clone() });
    if t.is_action() {
        steps.push(SpiceStep::CopyFile { from: snap_join(extracted, &action_file), to: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::WriteLastEdited { meta: snap_join(&dest, "metadata.json"), ts: s.last_edited, create: t.is_theme() });
    for po in &layout.po_files {
        let lang = po_language(po).unwrap_or_default();
        steps.push(SpiceStep::CompilePo {
            po: snap_join(&snap_join(&src, "po"), po),
            mo: snap_join(&locale_dir(env, lang), &format!("{key}.mo")),
        });
    }
    if t.is_action() && !ctx.is_update {
        // A fresh action is put in DISABLED: Nemo shows in the menu everything
        // that is not in this list, and the person enables it themselves.
        let (schema, list_key) = crate::de_apply::NEMO_DISABLED;
        steps.push(SpiceStep::GsettingsListAdd { schema: schema.into(), key: list_key.into(), item: action_file.clone() });
    }
    if let (Some(xlet), true) = (t.xlet, ctx.enabled) {
        steps.push(SpiceStep::ReloadXlet { uuid: key.to_string(), xlet: xlet.to_string() });
    }
    steps.push(SpiceStep::Record { content_id: item_id(t, key), provider: PROVIDER_ID.into(), web_url: page_url(t, &s.spices_id) });
    Ok(steps)
}

/// A tree snapshot from the list of zip entries (`dir/` — directories) with the texts
/// of the needed files (`metadata.json`, `.nemo_action`); other files are
/// empty. Parent directories appear by themselves.
pub fn snapshot_from_entries(root: &str, entries: &[String], texts: &Map<String, Value>) -> FsSnapshot {
    let root = root.trim_end_matches('/').to_string();
    let mut dirs: Map<String, Value> = Map::new();
    let mut files: Map<String, Value> = Map::new();
    fn add_child(dirs: &mut Map<String, Value>, parent: &str, name: &str) {
        let list = dirs.entry(parent.to_string()).or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(a) = list {
            if !a.iter().any(|v| v.as_str() == Some(name)) {
                a.push(Value::from(name));
            }
        }
    }
    dirs.insert(root.clone(), Value::Array(Vec::new()));
    for e in entries {
        let is_dir = e.ends_with('/');
        let parts: Vec<&str> = e.trim_end_matches('/').split('/').filter(|p| !p.is_empty()).collect();
        let mut cur = root.clone();
        for (i, p) in parts.iter().enumerate() {
            add_child(&mut dirs, &cur, p);
            let next = format!("{cur}/{p}");
            if i + 1 < parts.len() || is_dir {
                dirs.entry(next.clone()).or_insert_with(|| Value::Array(Vec::new()));
            } else {
                let text = texts.get(e.as_str()).and_then(Value::as_str).unwrap_or("");
                files.insert(next.clone(), Value::from(text));
            }
            cur = next;
        }
    }
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

/// A snapshot of a real directory (an unpacked archive): all directories and
/// files, texts — only for `metadata.json` and `*.nemo_action` (no more than
/// 1 MB). Links are not followed and count as files.
pub fn live_snapshot(root: &Path) -> FsSnapshot {
    fn walk(dir: &Path, dirs: &mut Map<String, Value>, files: &mut Map<String, Value>) {
        let mut names = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
                names.push(Value::from(name.clone()));
                if md.is_dir() {
                    walk(&path, dirs, files);
                } else {
                    let wants = name == "metadata.json" || name.ends_with(".nemo_action");
                    let text = if wants && md.is_file() && md.len() <= 1 << 20 {
                        std::fs::read_to_string(&path).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    files.insert(path.to_string_lossy().into_owned(), Value::from(text));
                }
            }
        }
        dirs.insert(dir.to_string_lossy().into_owned(), Value::Array(names));
    }
    let mut dirs = Map::new();
    let mut files = Map::new();
    walk(root, &mut dirs, &mut files);
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

// ── Enabling an applet (for the tab; Wehrwolfmann's decision of 17.10.2026) ──

/// Add an applet at the START of the right zone of the first panel, as the
/// Cinnamon settings window does: entries of the same panel and zone with order ≥ 0
/// are shifted by one, the new one is `panel1:right:0:<uuid>:<number>`.
/// Returns the new list and the new value of `next-applet-id`.
///
/// The instance number is taken at least one greater than the numbers already taken:
/// for stock entries the counter can be 0 while numbers are taken, and then two
/// applets would get one number and shared settings.
pub fn applet_enable_front(entries: &[String], uuid: &str, next_id: i64) -> (Vec<String>, i64) {
    let mut max_used = -1i64;
    let mut out: Vec<String> = entries
        .iter()
        .map(|e| {
            let mut parts: Vec<String> = e.split(':').map(str::to_string).collect();
            if let Some(id) = parts.get(4).and_then(|p| p.parse::<i64>().ok()) {
                max_used = max_used.max(id);
            }
            if parts.len() >= 4 && parts[0] == "panel1" && parts[1] == "right" {
                if let Ok(pos) = parts[2].parse::<i64>() {
                    if pos >= 0 {
                        parts[2] = (pos + 1).to_string();
                    }
                }
            }
            parts.join(":")
        })
        .collect();
    let id = next_id.max(max_used + 1).max(0);
    out.push(format!("panel1:right:0:{uuid}:{id}"));
    (out, id + 1)
}

// ═══════════════════════════════════════════════════════════════════════
//  Updates and uninstalling
// ═══════════════════════════════════════════════════════════════════════

/// The installed version mark — the `last-edited` field in `metadata.json`
/// (installation appends it, both Rexon's and the Cinnamon settings window's). A number
/// or a string with a number; no field — the version is unknown.
pub fn installed_last_edited(meta: &Value) -> Option<i64> {
    match meta.get("last-edited")? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Whether there is an update: the installed mark is older than the edit in the index.
/// No mark (the spice was put in by hand) — we do not offer an update, just like
/// the Cinnamon settings window: there is nothing to compare with.
pub fn has_update(installed: Option<i64>, index_last_edited: i64) -> bool {
    installed.is_some_and(|i| i < index_last_edited)
}

/// The entries of the enabled list without instances of the spice `item`. `field` — the number of
/// the `:`-separated field of an entry (an applet `panel1:right:0:<uuid>:<number>` — 3,
/// a desklet `<uuid>:<number>:<x>:<y>` — 0), `None` — the whole entry. A leading
/// `!` (a disabled instance) is ignored in the comparison. The comparison is
/// exact: `a@b` does not touch `xa@b`.
pub fn list_without(entries: &[String], item: &str, field: Option<u32>) -> Vec<String> {
    entries
        .iter()
        .filter(|e| {
            let part = match field {
                Some(i) => e.split(':').nth(i as usize).unwrap_or(""),
                None => e.as_str(),
            };
            part.trim_start_matches('!') != item
        })
        .cloned()
        .collect()
}

/// The translations directory of all languages: `~/.local/share/locale`.
pub fn locale_root(env: &SpiceEnv) -> String {
    format!("{}/.local/share/locale", env.home.trim_end_matches('/'))
}

/// The uninstall plan of the spice `key`. `fs` — a snapshot of `~/.local/share/locale` (it
/// is used to find the spice's `.mo` in all languages). First the spice is disabled
/// (the entry leaves the enabled list, a selected theme is reset to
/// the default), then its files are removed: the directory, for an action also
/// `.nemo_action`, translations (emptied language directories too), the settings
/// `~/.config/cinnamon/spices/<uuid>` and the old `~/.cinnamon/configs/<uuid>`;
/// at the end — the installed log record.
///
/// A Nemo action being "disabled" is its line in `disabled-actions`; after
/// the file is removed it is not needed and is removed too.
pub fn plan_uninstall(env: &SpiceEnv, t: &SpiceType, key: &str, fs: &FsSnapshot) -> Result<Vec<SpiceStep>, SpiceRefusal> {
    use crate::de_customization::{Activate, EntryFmt};
    if !valid_key(key) {
        return Err(SpiceRefusal::InvalidKey);
    }
    let root = install_root(env, t);
    let mut steps = Vec::new();
    match t.row().map(|r| &r.activate) {
        Some(Activate::GsettingsList { schema, key: list, entry, .. }) => {
            let field = match entry {
                EntryFmt::CinnamonApplet => Some(3),
                EntryFmt::CinnamonDesklet => Some(0),
                EntryFmt::Uuid => None,
            };
            steps.push(SpiceStep::GsettingsListDrop { schema: (*schema).into(), key: (*list).into(), item: key.into(), field });
        }
        Some(Activate::Gsettings { schema, key: k }) => {
            steps.push(SpiceStep::GsettingsResetIf { schema: (*schema).into(), key: (*k).into(), value: key.into() });
        }
        _ => {}
    }
    let action_file = format!("{key}.nemo_action");
    if t.is_action() {
        steps.push(SpiceStep::RemoveFile { path: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::RemoveTree { path: snap_join(&root, key) });
    if t.is_action() {
        let (schema, list) = crate::de_apply::NEMO_DISABLED;
        steps.push(SpiceStep::GsettingsListDrop { schema: schema.into(), key: list.into(), item: action_file, field: None });
    }
    if !t.is_theme() {
        let locales = locale_root(env);
        let mut langs: Vec<String> = fs.scandir(&locales).to_vec();
        langs.sort();
        for lang in langs.iter().filter(|l| valid_key(l)) {
            let dir = locale_dir(env, lang);
            let mo = snap_join(&dir, &format!("{key}.mo"));
            if fs.is_file(&mo) {
                steps.push(SpiceStep::RemoveFile { path: mo });
                steps.push(SpiceStep::RemoveDirIfEmpty { path: dir });
                steps.push(SpiceStep::RemoveDirIfEmpty { path: snap_join(&locales, lang) });
            }
        }
        steps.push(SpiceStep::RemoveTree { path: format!("{}/cinnamon/spices/{key}", env.config.trim_end_matches('/')) });
        steps.push(SpiceStep::RemoveTree { path: format!("{}/.cinnamon/configs/{key}", env.home.trim_end_matches('/')) });
    }
    steps.push(SpiceStep::Forget { content_id: item_id(t, key) });
    Ok(steps)
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    struct Tree {
        snap: FsSnapshot,
        names: Vec<String>,
    }

    /// The trees of six real archives (taken 17.10.2026): the names of zip
    /// entries, their permissions and the texts of `metadata.json` / `.nemo_action`.
    fn tree(archive: &str) -> Tree {
        let all: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
        )
        .unwrap();
        let t = &all[archive];
        let names: Vec<String> = t["entries"].as_array().unwrap().iter().map(|e| e[0].as_str().unwrap().to_string()).collect();
        let snap = snapshot_from_entries(X, &names, t["texts"].as_object().unwrap());
        Tree { snap, names }
    }

    const X: &str = "/tmp/x";
    const CUR: Option<(u32, u32, u32)> = Some((6, 6, 9));

    fn env() -> SpiceEnv {
        SpiceEnv::at_home("/home/u")
    }

    fn spice(key: &str, id: Value) -> Spice {
        Spice { key: key.into(), spices_id: id, last_edited: 1_780_583_449, ..Default::default() }
    }

    fn ctx(is_update: bool, enabled: bool) -> InstallContext {
        InstallContext { is_update, enabled, cinnamon: CUR }
    }

    fn po_steps(steps: &[SpiceStep]) -> Vec<(String, String)> {
        steps
            .iter()
            .filter_map(|s| match s {
                SpiceStep::CompilePo { po, mo } => Some((po.clone(), mo.clone())),
                _ => None,
            })
            .collect()
    }

    fn no_po(steps: &[SpiceStep]) -> Vec<SpiceStep> {
        steps.iter().filter(|s| !matches!(s, SpiceStep::CompilePo { .. })).cloned().collect()
    }

    fn all_archives() -> [&'static str; 7] {
        [
            "1440@jvlianodorneles",
            "weather@mockturtl",
            "AlbumArt3.0@claudiux",
            "ArrangementOfButtonsOnWindowTitleBar@claudiux",
            "Adara-Dark",
            "backup-this-file@h3nr1ke",
            "powerman@dr.drummie",
        ]
    }

    #[test]
    fn real_archives_have_safe_names() {
        for a in all_archives() {
            check_entry_names(&tree(a).names).unwrap_or_else(|e| panic!("{a}: {e:?}"));
        }
    }

    #[test]
    fn simple_applet() {
        let a = type_for_key("applet").unwrap();
        let tr = tree("1440@jvlianodorneles");
        let s = spice("1440@jvlianodorneles", Value::from(427));
        let steps = plan_install(&env(), a, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        let d = "/home/u/.local/share/cinnamon/applets";
        assert_eq!(
            no_po(&steps),
            vec![
                SpiceStep::MakeDirs { path: d.into() },
                SpiceStep::RemoveTree { path: format!("{d}/1440@jvlianodorneles") },
                SpiceStep::CopyTree { from: "/tmp/x/1440@jvlianodorneles".into(), to: format!("{d}/1440@jvlianodorneles") },
                SpiceStep::WriteLastEdited { meta: format!("{d}/1440@jvlianodorneles/metadata.json"), ts: 1_780_583_449, create: false },
                SpiceStep::Record {
                    content_id: "spices:applet:1440@jvlianodorneles".into(),
                    provider: "cinnamon-spices".into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/applets/view/427".into(),
                },
            ]
        );
        let po = po_steps(&steps);
        let langs: Vec<&str> = tr.names.iter().filter_map(|n| n.strip_prefix("1440@jvlianodorneles/po/")).filter_map(po_language).collect();
        assert_eq!(po.len(), langs.len());
        assert!(po.len() >= 3);
        assert!(po.contains(&(
            "/tmp/x/1440@jvlianodorneles/po/da.po".into(),
            "/home/u/.local/share/locale/da/LC_MESSAGES/1440@jvlianodorneles.mo".into()
        )));
        assert!(po.iter().all(|(p, _)| !p.ends_with(".pot")));
        // An applet without cinnamon-version is compatible; no version at all — too.
        let unknown = InstallContext { cinnamon: None, ..ctx(false, false) };
        assert!(plan_install(&env(), a, &s, &tr.snap, X, &unknown).is_ok());
    }

    #[test]
    fn multiversion_applet_picks_3_8_and_reloads_when_enabled() {
        let a = type_for_key("applet").unwrap();
        let tr = tree("weather@mockturtl");
        let lay = check_layout(&tr.snap, X, a, "weather@mockturtl", CUR).unwrap();
        assert_eq!(lay.version_subdir.as_deref(), Some("3.8"));
        assert!(lay.po_files.iter().any(|p| p == "ru.po"));
        assert!(lay.po_files.windows(2).all(|w| w[0] < w[1]));
        // On an old Cinnamon — 3.0; below 3.0 a root without applet.js will not do.
        assert_eq!(check_layout(&tr.snap, X, a, "weather@mockturtl", Some((3, 6, 0))).unwrap().version_subdir.as_deref(), Some("3.0"));
        assert_eq!(
            check_layout(&tr.snap, X, a, "weather@mockturtl", Some((2, 8, 0))),
            Err(SpiceRefusal::NoMainJs { file: "applet.js".into() })
        );
        // The version is unknown — any subdirectory will do.
        assert_eq!(check_layout(&tr.snap, X, a, "weather@mockturtl", None).unwrap().version_subdir.as_deref(), Some("3.8"));

        let s = spice("weather@mockturtl", Value::from(17));
        let steps = plan_install(&env(), a, &s, &tr.snap, X, &ctx(true, true)).unwrap();
        assert_eq!(
            steps.iter().rev().nth(1),
            Some(&SpiceStep::ReloadXlet { uuid: "weather@mockturtl".into(), xlet: "APPLET".into() })
        );
        assert!(!steps.iter().any(|s| matches!(s, SpiceStep::GsettingsListAdd { .. })));
        let not_enabled = plan_install(&env(), a, &s, &tr.snap, X, &ctx(true, false)).unwrap();
        assert!(!not_enabled.iter().any(|s| matches!(s, SpiceStep::ReloadXlet { .. })));
    }

    #[test]
    fn multiversion_flag_without_subdirs() {
        // powerman: "multiversion": true, but there are no subdirectories; versions 6.4/6.6.
        let a = type_for_key("applet").unwrap();
        let tr = tree("powerman@dr.drummie");
        let lay = check_layout(&tr.snap, X, a, "powerman@dr.drummie", CUR).unwrap();
        assert_eq!(lay.version_subdir, None);
        match check_layout(&tr.snap, X, a, "powerman@dr.drummie", Some((6, 2, 0))) {
            Err(SpiceRefusal::Incompatible { need, current }) => {
                assert_eq!(need, vec!["6.4".to_string(), "6.6".to_string()]);
                assert_eq!(current, (6, 2, 0));
                let msg = SpiceRefusal::Incompatible { need, current }.message("PowerMan");
                assert!(msg.contains("6.4") && msg.contains("6.2.0"), "{msg}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn desklet() {
        let d = type_for_key("desklet").unwrap();
        let tr = tree("AlbumArt3.0@claudiux");
        let s = spice("AlbumArt3.0@claudiux", Value::from(73));
        let steps = plan_install(&env(), d, &s, &tr.snap, X, &ctx(false, true)).unwrap();
        let root = "/home/u/.local/share/cinnamon/desklets";
        assert_eq!(steps[0], SpiceStep::MakeDirs { path: root.into() });
        assert_eq!(steps[2], SpiceStep::CopyTree { from: "/tmp/x/AlbumArt3.0@claudiux".into(), to: format!("{root}/AlbumArt3.0@claudiux") });
        assert!(steps.contains(&SpiceStep::ReloadXlet { uuid: "AlbumArt3.0@claudiux".into(), xlet: "DESKLET".into() }));
        assert!(!po_steps(&steps).is_empty());
        // Without desklet.js — a refusal.
        let mut names = tr.names.clone();
        names.retain(|n| n != "AlbumArt3.0@claudiux/desklet.js");
        let all: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let snap = snapshot_from_entries(X, &names, all["AlbumArt3.0@claudiux"]["texts"].as_object().unwrap());
        assert_eq!(plan_install(&env(), d, &s, &snap, X, &ctx(false, false)), Err(SpiceRefusal::NoMainJs { file: "desklet.js".into() }));
    }

    #[test]
    fn extension_and_its_mandatory_version() {
        let e = type_for_key("extension").unwrap();
        let key = "ArrangementOfButtonsOnWindowTitleBar@claudiux";
        let tr = tree(key);
        let s = spice(key, Value::from(107));
        let steps = plan_install(&env(), e, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(steps[1], SpiceStep::RemoveTree { path: format!("/home/u/.local/share/cinnamon/extensions/{key}") });
        assert_eq!(steps.last().unwrap(), &SpiceStep::Record {
            content_id: format!("spices:extension:{key}"),
            provider: PROVIDER_ID.into(),
            web_url: "https://cinnamon-spices.linuxmint.com/extensions/view/107".into(),
        });
        // On Cinnamon 3.4 (before 3.6) — incompatible.
        assert!(matches!(check_layout(&tr.snap, X, e, key, Some((3, 4, 0))), Err(SpiceRefusal::Incompatible { .. })));
        // An extension without cinnamon-version — a refusal; an applet without it — allowed.
        let meta = format!("{{\"uuid\": \"{key}\", \"name\": \"n\", \"description\": \"d\"}}");
        let mut texts = Map::new();
        texts.insert(format!("{key}/metadata.json"), Value::from(meta));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, e, key, CUR), Err(SpiceRefusal::MissingCinnamonVersion));
    }

    #[test]
    fn theme_goes_to_dot_themes_by_key() {
        let th = type_for_key("theme").unwrap();
        let tr = tree("Adara-Dark");
        let s = spice("Adara-Dark", Value::from("Adara-Dark"));
        let steps = plan_install(&env(), th, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(
            steps,
            vec![
                SpiceStep::MakeDirs { path: "/home/u/.themes".into() },
                SpiceStep::RemoveTree { path: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::CopyTree { from: "/tmp/x/Adara-Dark".into(), to: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::WriteLastEdited { meta: "/home/u/.themes/Adara-Dark/metadata.json".into(), ts: 1_780_583_449, create: true },
                SpiceStep::Record {
                    content_id: "spices:theme:Adara-Dark".into(),
                    provider: PROVIDER_ID.into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/themes/view/Adara-Dark".into(),
                },
            ]
        );
        // Without cinnamon/cinnamon.css — not a Cinnamon theme.
        let mut names = tr.names.clone();
        names.retain(|n| n != "Adara-Dark/cinnamon/cinnamon.css");
        let snap = snapshot_from_entries(X, &names, &Map::new());
        assert_eq!(check_layout(&snap, X, th, "Adara-Dark", CUR), Err(SpiceRefusal::NoCinnamonCss));
    }

    #[test]
    fn nemo_action_two_roots_and_disabled_on_first_install() {
        let act = type_for_key("action").unwrap();
        let key = "backup-this-file@h3nr1ke";
        let tr = tree(key);
        let s = spice(key, Value::from(37));
        let d = "/home/u/.local/share/nemo/actions";
        let fresh = plan_install(&env(), act, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(
            no_po(&fresh),
            vec![
                SpiceStep::MakeDirs { path: d.into() },
                SpiceStep::RemoveTree { path: format!("{d}/{key}") },
                SpiceStep::RemoveFile { path: format!("{d}/{key}.nemo_action") },
                SpiceStep::CopyTree { from: format!("/tmp/x/{key}"), to: format!("{d}/{key}") },
                SpiceStep::CopyFile { from: format!("/tmp/x/{key}.nemo_action"), to: format!("{d}/{key}.nemo_action") },
                SpiceStep::WriteLastEdited { meta: format!("{d}/{key}/metadata.json"), ts: 1_780_583_449, create: false },
                SpiceStep::GsettingsListAdd {
                    schema: "org.nemo.plugins".into(),
                    key: "disabled-actions".into(),
                    item: format!("{key}.nemo_action"),
                },
                SpiceStep::Record {
                    content_id: format!("spices:action:{key}"),
                    provider: PROVIDER_ID.into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/actions/view/37".into(),
                },
            ]
        );
        assert!(po_steps(&fresh).iter().any(|(_, mo)| mo == &format!("/home/u/.local/share/locale/pt_BR/LC_MESSAGES/{key}.mo")));
        // An update: we do not touch the disabled list; being enabled does not matter.
        let update = plan_install(&env(), act, &s, &tr.snap, X, &ctx(true, true)).unwrap();
        assert!(!update.iter().any(|s| matches!(s, SpiceStep::GsettingsListAdd { .. } | SpiceStep::ReloadXlet { .. })));
        // Without .nemo_action — a refusal.
        let names: Vec<String> = tr.names.iter().filter(|n| !n.ends_with(".nemo_action")).cloned().collect();
        let all: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let snap = snapshot_from_entries(X, &names, all[key]["texts"].as_object().unwrap());
        assert_eq!(check_layout(&snap, X, act, key, CUR), Err(SpiceRefusal::NoActionFile));
    }

    #[test]
    fn refusals() {
        let a = type_for_key("applet").unwrap();
        // ../ and absolute paths.
        for bad in ["../evil", "x/../../evil", "/etc/passwd", "a\\..\\b"] {
            assert_eq!(check_entry_names(&[bad.to_string()]), Err(SpiceRefusal::UnsafePath { entry: bad.into() }), "{bad}");
        }
        assert!(check_entry_names(&["a/./b".into(), "a/b/../c".into()]).is_ok());

        // The directory in the archive is not named after the key.
        let tr = tree("1440@jvlianodorneles");
        assert_eq!(check_layout(&tr.snap, X, a, "other@x", CUR), Err(SpiceRefusal::NotZipLayout));
        assert_eq!(check_layout(&tr.snap, X, a, "../x", CUR), Err(SpiceRefusal::InvalidKey));

        // uuid in metadata.json ≠ the directory.
        let mut texts = Map::new();
        texts.insert("1440@jvlianodorneles/metadata.json".into(), Value::from(r#"{"uuid": "1440@someone", "name": "n", "description": "d"}"#));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::UuidMismatch { found: "1440@someone".into() }));
        // No metadata.json, broken, without name.
        let snap = snapshot_from_entries(X, &tr.names, &Map::new());
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::BadMetadata), "empty text is not JSON");
        let names: Vec<String> = tr.names.iter().filter(|n| !n.ends_with("metadata.json")).cloned().collect();
        let snap = snapshot_from_entries(X, &names, &Map::new());
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::NoMetadata));
        texts.insert("1440@jvlianodorneles/metadata.json".into(), Value::from(r#"{"uuid": "1440@jvlianodorneles", "description": "d"}"#));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::MissingProperty { name: "name".into() }));
        // An incompatible version — a clear text.
        texts.insert(
            "1440@jvlianodorneles/metadata.json".into(),
            Value::from(r#"{"uuid": "1440@jvlianodorneles", "name": "n", "description": "d", "cinnamon-version": ["6.8"]}"#),
        );
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        let err = check_layout(&snap, X, a, "1440@jvlianodorneles", CUR).unwrap_err();
        assert_eq!(err, SpiceRefusal::Incompatible { need: vec!["6.8".into()], current: (6, 6, 9) });
        let msg = err.message("1440");
        assert!(msg.contains("6.8") && msg.contains("6.6.9") && msg.contains("1440"), "{msg}");
        // Every refusal gives a non-empty text with the name.
        for r in [
            SpiceRefusal::InvalidKey,
            SpiceRefusal::UnsafePath { entry: "../x".into() },
            SpiceRefusal::NotZipLayout,
            SpiceRefusal::NoMetadata,
            SpiceRefusal::BadMetadata,
            SpiceRefusal::UuidMismatch { found: "f".into() },
            SpiceRefusal::MissingProperty { name: "name".into() },
            SpiceRefusal::Incompatible { need: vec![], current: (6, 6, 9) },
            SpiceRefusal::MissingCinnamonVersion,
            SpiceRefusal::NoMainJs { file: "applet.js".into() },
            SpiceRefusal::NoCinnamonCss,
            SpiceRefusal::NoActionFile,
        ] {
            assert!(r.message("Name").contains("Name"), "{r:?}");
        }
    }

    #[test]
    fn live_snapshot_matches_entry_snapshot() {
        let dir = std::env::temp_dir().join(format!("rexon-spices-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("u@x/3.8/po")).unwrap();
        std::fs::write(dir.join("u@x/metadata.json"), r#"{"uuid":"u@x","name":"n","description":"d"}"#).unwrap();
        std::fs::write(dir.join("u@x/3.8/applet.js"), "x").unwrap();
        std::fs::write(dir.join("u@x/3.8/po/ru.po"), "").unwrap();
        let snap = live_snapshot(&dir);
        let root = dir.to_string_lossy().into_owned();
        let lay = check_layout(&snap, &root, type_for_key("applet").unwrap(), "u@x", CUR).unwrap();
        assert_eq!(lay.version_subdir.as_deref(), Some("3.8"));
        assert!(lay.po_files.is_empty(), "po is looked for only in the spice root");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn applet_goes_to_the_front_of_the_right_box() {
        let entries: Vec<String> = [
            "panel1:left:0:menu@cinnamon.org",
            "panel1:right:0:systray@cinnamon.org:3",
            "panel1:right:1:calendar@cinnamon.org:7",
            "panel2:right:0:other@x:2",
            "panel1:center:0:clock@x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (out, next) = applet_enable_front(&entries, "weather@mockturtl", 0);
        assert_eq!(
            out,
            vec![
                "panel1:left:0:menu@cinnamon.org",
                "panel1:right:1:systray@cinnamon.org:3",
                "panel1:right:2:calendar@cinnamon.org:7",
                "panel2:right:0:other@x:2",
                "panel1:center:0:clock@x",
                "panel1:right:0:weather@mockturtl:8",
            ]
        );
        assert_eq!(next, 9, "counter 0 with 7 taken — take 8");
        let (out, next) = applet_enable_front(&[], "a@b", 12);
        assert_eq!((out, next), (vec!["panel1:right:0:a@b:12".to_string()], 13));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Executor: downloading, unpacking, plan steps
// ═══════════════════════════════════════════════════════════════════════

/// The spice archive ceiling: the biggest on the site (measured 17.10.2026) is
/// an extension of about 83 MB.
pub const MAX_ARCHIVE_BYTES: i64 = 120 << 20;
/// The archive download timeout, seconds (for connecting and each read).
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// The executor's non-file side effects: gsettings settings and calling
/// the shell over DBus. In production — [`LiveSys`], in tests — recording to memory.
pub trait SpiceSys {
    /// A key's string list; `None` — could not read (no schema).
    fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>>;
    fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String>;
    /// A string key; `None` — could not read.
    fn string_get(&mut self, schema: &str, key: &str) -> Option<String>;
    fn reset(&mut self, schema: &str, key: &str) -> Result<(), String>;
    /// `org.Cinnamon.ReloadXlet(uuid, type)`.
    fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String>;
}

/// The real `gsettings` and `gdbus`.
pub struct LiveSys;

fn run_out(argv: &[&str], timeout: f64) -> Result<String, String> {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    match crate::splash_exec::run_capture(&argv, timeout) {
        Some(o) if o.rc == 0 => Ok(o.stdout),
        Some(o) => Err(o.stderr.trim().to_string()),
        None => Err(trf("couldn't run %1", &[&argv[0]])),
    }
}

impl SpiceSys for LiveSys {
    fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>> {
        run_out(&["gsettings", "get", schema, key], 10.0).ok().map(|o| crate::de_apply::parse_gsettings_list(&o))
    }
    fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String> {
        let v = crate::wallpaper_engine::gvariant_string_array(items);
        run_out(&["gsettings", "set", schema, key, &v], 10.0).map(|_| ())
    }
    fn string_get(&mut self, schema: &str, key: &str) -> Option<String> {
        run_out(&["gsettings", "get", schema, key], 10.0).ok().map(|o| crate::de_apply::parse_gsettings_string(&o).unwrap_or_default())
    }
    fn reset(&mut self, schema: &str, key: &str) -> Result<(), String> {
        run_out(&["gsettings", "reset", schema, key], 10.0).map(|_| ())
    }
    fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String> {
        run_out(
            &["gdbus", "call", "--session", "--dest", "org.Cinnamon", "--object-path", "/org/Cinnamon", "--method", "org.Cinnamon.ReloadXlet", uuid, xlet],
            10.0,
        )
        .map(|_| ())
    }
}

/// An executor without gsettings and DBus — for purely file-based plans.
struct NoSys;

impl SpiceSys for NoSys {
    fn list_get(&mut self, _: &str, _: &str) -> Option<Vec<String>> {
        None
    }
    fn list_set(&mut self, _: &str, _: &str, _: &[String]) -> Result<(), String> {
        Ok(())
    }
    fn string_get(&mut self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn reset(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn reload_xlet(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// The result of executing a plan: what did not get installed but did not cancel the installation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExecReport {
    /// Languages whose `.po` did not build.
    pub bad_po: Vec<String>,
    /// Could not reload the enabled xlet (the shell is not running…).
    pub reload_failed: bool,
}

/// A path under one of the roots (`home`, the config directory, the work directory):
/// the executor deletes and writes nothing outside them.
fn under(path: &str, roots: &[&str]) -> bool {
    !path.contains("/../")
        && !path.ends_with("/..")
        && roots.iter().any(|r| {
            let r = r.trim_end_matches('/');
            !r.is_empty() && path.starts_with(&format!("{r}/"))
        })
}

fn remove_tree(path: &str) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Copy a tree with file permissions. Links inside the archive are not
/// carried over: the spice does not need them, and a link pointing out of the directory is
/// a loophole.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let md = std::fs::symlink_metadata(e.path())?;
        let dest = to.join(e.file_name());
        if md.is_dir() {
            copy_tree(&e.path(), &dest)?;
            std::fs::set_permissions(&dest, md.permissions())?;
        } else if md.is_file() {
            std::fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

/// `metadata.json` with the `last-edited` field (other keys in the previous order,
/// indent 4, as in the Cinnamon settings window).
pub fn with_last_edited(meta_text: Option<&str>, ts: i64) -> Result<String, String> {
    let mut obj: Map<String, Value> = match meta_text {
        Some(t) => match serde_json::from_str::<Value>(t) {
            Ok(Value::Object(m)) => m,
            _ => return Err("metadata.json".to_string()),
        },
        None => Map::new(),
    };
    obj.insert("last-edited".to_string(), Value::from(ts));
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    Value::Object(obj).serialize(&mut ser).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Execute the plan. `roots` — where writing and deleting are allowed; `journal` —
/// the `Record`/`Forget` steps. The first failure of a file step stops
/// execution (`Err` — the reason text); a failed translation and xlet reload
/// are report notes.
pub fn execute(
    steps: &[SpiceStep],
    roots: &[&str],
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> Result<ExecReport, String> {
    let mut report = ExecReport::default();
    let guard = |p: &str| -> Result<(), String> {
        if under(p, roots) {
            Ok(())
        } else {
            Err(trf("refused to touch %1: it's outside the home folder", &[&p]))
        }
    };
    let io = |p: &str, e: std::io::Error| format!("{p}: {e}");
    for step in steps {
        match step {
            SpiceStep::MakeDirs { path } => {
                guard(path)?;
                std::fs::create_dir_all(path).map_err(|e| io(path, e))?;
            }
            SpiceStep::RemoveTree { path } => {
                guard(path)?;
                remove_tree(path).map_err(|e| io(path, e))?;
            }
            SpiceStep::RemoveFile { path } => {
                guard(path)?;
                match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(path, e)),
                    _ => {}
                }
            }
            SpiceStep::RemoveDirIfEmpty { path } => {
                guard(path)?;
                let _ = std::fs::remove_dir(path); // not empty — stays
            }
            SpiceStep::CopyTree { from, to } => {
                guard(to)?;
                copy_tree(Path::new(from), Path::new(to)).map_err(|e| io(to, e))?;
            }
            SpiceStep::CopyFile { from, to } => {
                guard(to)?;
                std::fs::copy(from, to).map_err(|e| io(to, e))?;
            }
            SpiceStep::CompilePo { po, mo } => {
                guard(mo)?;
                let lang = Path::new(po).file_name().and_then(|n| n.to_str()).and_then(po_language).unwrap_or("?").to_string();
                let built = std::fs::read(po).map_err(|e| e.to_string()).and_then(|b| po_to_mo(&String::from_utf8_lossy(&b)));
                let written = built.and_then(|bytes| {
                    if let Some(dir) = Path::new(mo).parent() {
                        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(mo, bytes).map_err(|e| e.to_string())
                });
                if let Err(e) = written {
                    log(&format!("translation {lang} not built: {e}"));
                    report.bad_po.push(lang);
                }
            }
            SpiceStep::WriteLastEdited { meta, ts, create } => {
                guard(meta)?;
                let text = std::fs::read_to_string(meta).ok();
                if text.is_none() && !create {
                    return Err(trf("%1 is missing", &[meta]));
                }
                let out = with_last_edited(text.as_deref(), *ts).map_err(|e| trf("%1 can't be read", &[&e]))?;
                std::fs::write(meta, out).map_err(|e| io(meta, e))?;
            }
            SpiceStep::GsettingsListAdd { schema, key, item } => {
                let mut list = sys.list_get(schema, key).unwrap_or_default();
                if !list.iter().any(|e| e == item) {
                    list.push(item.clone());
                    sys.list_set(schema, key, &list).map_err(|e| format!("{schema} {key}: {e}"))?;
                }
            }
            SpiceStep::GsettingsListDrop { schema, key, item, field } => {
                // No schema (Cinnamon/Nemo are not installed) — nothing to disable either.
                if let Some(list) = sys.list_get(schema, key) {
                    let kept = list_without(&list, item, *field);
                    if kept.len() != list.len() {
                        sys.list_set(schema, key, &kept).map_err(|e| format!("{schema} {key}: {e}"))?;
                    }
                }
            }
            SpiceStep::GsettingsResetIf { schema, key, value } => {
                if sys.string_get(schema, key).as_deref() == Some(value.as_str()) {
                    sys.reset(schema, key).map_err(|e| format!("{schema} {key}: {e}"))?;
                }
            }
            SpiceStep::ReloadXlet { uuid, xlet } => {
                if let Err(e) = sys.reload_xlet(uuid, xlet) {
                    log(&format!("ReloadXlet {uuid}: {e}"));
                    report.reload_failed = true;
                }
            }
            SpiceStep::Record { .. } | SpiceStep::Forget { .. } => journal(step)?,
        }
    }
    Ok(report)
}

/// The spice state on disk: `None` — not installed; `Some(mark)` — installed,
/// the `last-edited` mark from `metadata.json` (absent for one put in by hand).
/// A Nemo action is installed if its `.nemo_action` exists.
pub fn installed_state(env: &SpiceEnv, t: &SpiceType, key: &str) -> Option<Option<i64>> {
    if !valid_key(key) {
        return None;
    }
    let root = install_root(env, t);
    let dir = snap_join(&root, key);
    let present = if t.is_action() {
        Path::new(&snap_join(&root, &format!("{key}.nemo_action"))).is_file()
    } else {
        Path::new(&dir).is_dir()
    };
    if !present {
        return None;
    }
    let meta = std::fs::read_to_string(snap_join(&dir, "metadata.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    Some(meta.as_ref().and_then(installed_last_edited))
}

/// Whether an xlet is enabled (it is in its enabled list). Themes and actions — no:
/// there is nothing to reload for them after an update.
pub fn is_enabled(sys: &mut dyn SpiceSys, t: &SpiceType, key: &str) -> bool {
    use crate::de_customization::{Activate, EntryFmt};
    let Some(Activate::GsettingsList { schema, key: list, entry, .. }) = t.row().map(|r| &r.activate) else {
        return false;
    };
    let field = match entry {
        EntryFmt::CinnamonApplet => Some(3),
        EntryFmt::CinnamonDesklet => Some(0),
        EntryFmt::Uuid => None,
    };
    sys.list_get(schema, list).is_some_and(|l| list_without(&l, key, field).len() != l.len())
}

/// Names of zip entries; `None` — this is not a zip.
fn zip_names(archive: &Path) -> Option<Vec<String>> {
    let mut zf = zip::ZipArchive::new(std::fs::File::open(archive).ok()?).ok()?;
    (0..zf.len()).map(|i| zf.by_index(i).ok().map(|f| f.name().to_string())).collect()
}

/// What the plan installed (for the log): the spice directory and, for an action, the file.
fn planned_paths(steps: &[SpiceStep]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|s| match s {
            SpiceStep::CopyTree { to, .. } | SpiceStep::CopyFile { to, .. } => Some(to.clone()),
            _ => None,
        })
        .collect()
}

/// The install result sentence.
pub fn installed_message(t: &SpiceType, name: &str, is_update: bool, report: &ExecReport, reloaded: bool) -> String {
    let mut text = if is_update {
        trf("%1 is updated.", &[&name])
    } else if t.is_theme() {
        trf("%1 is installed. Press “Apply” on its card to use it.", &[&name])
    } else {
        trf("%1 is installed. It isn't turned on yet: press “Enable” on its card.", &[&name])
    };
    if reloaded {
        text.push(' ');
        text.push_str(&if report.reload_failed {
            tr("Cinnamon will pick up the new version after you sign out and back in.")
        } else {
            tr("Cinnamon has already picked up the new version.")
        });
    }
    if !report.bad_po.is_empty() {
        text.push(' ');
        text.push_str(&trf("Translations for these languages weren't installed: %1.", &[&report.bad_po.join(", ")]));
    }
    text
}

/// Install an already downloaded archive `archive`: checking entries, unpacking into
/// `work`, checking the tree, the plan, execution. `journal` — the installed log
/// record (`Record`, with the paths the plan installed). The `work` directory
/// is left to the caller.
#[allow(clippy::too_many_arguments)]
pub fn install_archive(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    name: &str,
    archive: &Path,
    work: &Path,
    cinnamon: Option<(u32, u32, u32)>,
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep, &[String]) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> (bool, String) {
    install_archive_in(env, t, s, name, archive, work, cinnamon, None, sys, journal, log)
}

/// [`install_archive`] from the GTK or window frames section (`section`, see
/// [`PART_SECTIONS`]): the theme is installed only if the archive itself has what
/// this section will apply ([`fits_section`]). The section storefront is already
/// filtered by the repository tree, but it is the site archive that gets installed — and it is
/// the archive that is checked, not our forecast.
#[allow(clippy::too_many_arguments)]
pub fn install_archive_in(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    name: &str,
    archive: &Path,
    work: &Path,
    cinnamon: Option<(u32, u32, u32)>,
    section: Option<&str>,
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep, &[String]) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> (bool, String) {
    let Some(names) = zip_names(archive) else {
        return (false, SpiceRefusal::NotZipLayout.message(name));
    };
    if let Err(r) = check_entry_names(&names) {
        return (false, r.message(name));
    }
    let out = work.join("x");
    let out_s = out.to_string_lossy().into_owned();
    if let Err(e) = crate::appearance_installer::live_safe_extract(&archive.to_string_lossy(), &out_s) {
        return (false, trf("Couldn't unpack the archive of %1: %2", &[&name, &e]));
    }
    let snap = live_snapshot(&out);
    if let Some(sec) = section.filter(|_| t.is_theme()) {
        let dir = snap_join(&out_s, &s.key);
        let has = |rel: &str| {
            let p = snap_join(&dir, rel);
            snap.is_file(&p) || snap.is_dir(&p)
        };
        if !fits_section(sec, &has) {
            return (false, part_missing_message(name, sec));
        }
    }
    let is_update = installed_state(env, t, &s.key).is_some();
    let enabled = is_update && is_enabled(sys, t, &s.key);
    let ctx = InstallContext { is_update, enabled, cinnamon };
    let steps = match plan_install(env, t, s, &snap, &out_s, &ctx) {
        Ok(st) => st,
        Err(r) => return (false, r.message(name)),
    };
    let paths = planned_paths(&steps);
    let work_s = work.to_string_lossy().into_owned();
    let roots = [env.home.as_str(), work_s.as_str()];
    log(&trf("Installing %1…", &[&name]));
    match execute(&steps, &roots, sys, &mut |st| journal(st, &paths), log) {
        Ok(report) => (true, installed_message(t, name, is_update, &report, enabled)),
        Err(e) => (false, trf("Couldn't install %1: %2", &[&name, &e])),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Editing the installed log: the shared install log is written,
/// `ocs_installed.json` is its mirror (step 5b of merging the installers).
fn journal_edit(env: &SpiceEnv, edit: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let mut data = crate::install_journal::store_log_view(&path);
    edit(&mut data);
    crate::install_journal::put_store_log(&path, &data).map_err(|e| e.to_string())
}

/// Record the spice in the installed log (`provider_id` —
/// [`PROVIDER_ID`], the address — the site page).
pub fn journal_record(env: &SpiceEnv, item: &ThemeItem, t: &SpiceType, web_url: &str, paths: &[String]) -> Result<(), String> {
    let entry = crate::ocs_store::InstalledItem {
        content_id: item.id.clone(),
        name: item.name.clone(),
        rexon_type: crate::appearance_installer::kind_to_rexon_type(t.section).to_string(),
        author: item.author.clone(),
        description: item.description.clone(),
        version: item.version.clone(),
        license: item.license.clone(),
        preview_url: item.preview_url.clone(),
        web_url: web_url.to_string(),
        provider_id: PROVIDER_ID.to_string(),
        category_id: t.cat_id(),
        ..Default::default()
    };
    journal_edit(env, |data| {
        crate::ocs_store::record_installed(data, &entry, paths, &crate::engine_update::now_iso(now_secs()));
    })
}

/// The work directory under Rexon's cache (not in `/tmp`: that is memory, and an archive
/// can be close to a hundred megabytes).
fn make_work_dir() -> Result<std::path::PathBuf, String> {
    let cache = live_cache_path(&TYPES[0]);
    let base = Path::new(&cache).parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    for n in 0..100u32 {
        let p = base.join(format!("work-{}-{}-{n}", std::process::id(), now_secs()));
        if std::fs::create_dir(&p).is_ok() {
            return Ok(p);
        }
    }
    Err(trf("couldn't create a temporary folder in %1", &[&base.display()]))
}

/// Install (or update) the spice of a storefront card: the index record — from
/// the cache, the archive — from the site (only on a button press), then [`install_archive`].
pub fn live_install(item: &ThemeItem, mut log: Box<dyn FnMut(&str) + Send>, progress: Option<Box<dyn FnMut(i64, i64) + Send>>) -> (bool, String) {
    let Some((t, key)) = parse_item_id(&item.id) else {
        return (false, trf("%1 isn't from Cinnamon Spices.", &[&item.name]));
    };
    let name = if item.name.is_empty() { key.to_string() } else { item.name.clone() };
    let Some(s) = load_cache(Path::new(&live_cache_path(t))).and_then(|(_, list)| list.into_iter().find(|s| s.key == key)) else {
        return (false, trf("Couldn't find %1 in the Cinnamon Spices catalog. Refresh the section and try again.", &[&name]));
    };
    let env = SpiceEnv::live();
    let work = match make_work_dir() {
        Ok(w) => w,
        Err(e) => return (false, e),
    };
    let archive = work.join("spice.zip");
    let mut net = crate::net::UreqNet::new();
    let with_progress = progress.is_some();
    if let Some(cb) = progress {
        net = net.with_progress(cb);
    }
    let url = site_url(&s.file);
    let result = if !crate::ocs::http_download(&mut net, &url, &archive.to_string_lossy(), with_progress, DOWNLOAD_TIMEOUT_SECS, MAX_ARCHIVE_BYTES) {
        (false, trf("Couldn't download %1 from Cinnamon Spices. Check your internet connection and try again.", &[&name]))
    } else {
        let web_url = page_url(t, &s.spices_id);
        let mut journal = |step: &SpiceStep, paths: &[String]| match step {
            SpiceStep::Record { .. } => journal_record(&env, item, t, &web_url, paths),
            _ => Ok(()),
        };
        let mut say = |l: &str| log(l);
        let section = part_section_of(item);
        install_archive_in(&env, t, &s, &name, &archive, &work, live_cinnamon_version(), section, &mut LiveSys, &mut journal, &mut say)
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Remove a spice by the [`plan_uninstall`] plan through `sys`.
pub fn uninstall_with(env: &SpiceEnv, t: &SpiceType, key: &str, name: &str, sys: &mut dyn SpiceSys, log: &mut dyn FnMut(&str)) -> (bool, String) {
    let locales = live_snapshot(Path::new(&locale_root(env)));
    let plan = match plan_uninstall(env, t, key, &locales) {
        Ok(s) => s,
        Err(r) => return (false, r.message(name)),
    };
    let log_path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let steps = with_journal_removal(&plan, &log_path, &item_id(t, key));
    let roots = [env.home.as_str(), env.config.as_str()];
    let mut journal = |step: &SpiceStep| match step {
        SpiceStep::Forget { content_id } => journal_edit(env, |d| {
            crate::ocs_store::forget_installed(d, content_id);
        }),
        _ => Ok(()),
    };
    let outcome = execute(&steps, &roots, sys, &mut journal, log);
    match outcome {
        Ok(_) => (true, trf("%1 is removed.", &[&name])),
        Err(e) => (false, trf("Couldn't remove %1: %2", &[&name, &e])),
    }
}

/// The removal plan `plan` of a spice, in which the files recorded by the purchase
/// `content_id` in the shared install log (next to the store log
/// `log_path`) are removed by the log — only its own (step 3d of merging the installers).
/// Disabling (gsettings lists, theme reset) and cleaning up translations and settings are
/// plan steps as before: they are not written to the log. If the purchase is not in the log, the
/// plan stays as is.
fn with_journal_removal(plan: &[SpiceStep], log_path: &str, content_id: &str) -> Vec<SpiceStep> {
    use crate::ext_manager::ExtStep;
    use crate::install_journal as journal;
    let view = journal::store_view(log_path);
    let Some(entry) = view.items.get(&journal::key_of(journal::Source::Store, content_id)) else {
        return plan.to_vec();
    };
    let r = journal::removal_plan(entry, &journal::probe_live);
    let mut ours: Vec<SpiceStep> = r
        .steps
        .into_iter()
        .filter_map(|st| match st {
            ExtStep::RemoveTree { path, .. } => Some(SpiceStep::RemoveTree { path }),
            ExtStep::RemoveFile { path } => Some(SpiceStep::RemoveFile { path }),
            _ => None,
        })
        .collect();
    ours.extend(r.prune.into_iter().map(|path| SpiceStep::RemoveDirIfEmpty { path }));
    let path_of = |st: &SpiceStep| match st {
        SpiceStep::RemoveTree { path } | SpiceStep::RemoveFile { path } => Some(path.clone()),
        _ => None,
    };
    journal::merge_into_plan(plan, &path_of, entry, ours)
}

/// "Remove" of a spice from the installed log (`spices:<type>:<key>`).
pub fn live_uninstall(content_id: &str, mut log: Box<dyn FnMut(&str) + Send>) -> (bool, String) {
    let Some((t, key)) = parse_item_id(content_id) else {
        return (false, trf("%1 isn't from Cinnamon Spices.", &[&content_id]));
    };
    let env = SpiceEnv::live();
    let path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let data = crate::install_journal::store_log_view(&path);
    let name = data.get(content_id).and_then(|r| r.get("name")).and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(key).to_string();
    uninstall_with(&env, t, key, &name, &mut LiveSys, &mut |l| log(l))
}

/// After "Remove" on a disk tile (`theme_removal::commit` removed the directory
/// or the `.nemo_action`) — clean up whatever else the spice left: translations,
/// settings, for an action — its directory. Only files under `env.home` and
/// the config directory; gsettings lists are not touched (an enabled one cannot be
/// removed, `commit` edits the log itself). Themes and paths not from spice
/// directories — does nothing.
pub fn cleanup_after_removal(env: &SpiceEnv, path: &str) {
    let p = path.trim_end_matches('/');
    let Some((parent, name)) = p.rsplit_once('/') else { return };
    for t in TYPES.iter().filter(|t| !t.is_theme()) {
        if install_root(env, t) != parent {
            continue;
        }
        let key = if t.is_action() {
            match name.strip_suffix(".nemo_action") {
                Some(k) => k,
                None => continue,
            }
        } else {
            name
        };
        let locales = live_snapshot(Path::new(&locale_root(env)));
        let Ok(steps) = plan_uninstall(env, t, key, &locales) else { continue };
        let files: Vec<SpiceStep> = steps
            .into_iter()
            .filter(|s| matches!(s, SpiceStep::RemoveTree { .. } | SpiceStep::RemoveFile { .. } | SpiceStep::RemoveDirIfEmpty { .. }))
            .collect();
        let roots = [env.home.as_str(), env.config.as_str()];
        if let Err(e) = execute(&files, &roots, &mut NoSys, &mut |_| Ok(()), &mut |l| crate::log_warn!("theme", "{l}")) {
            crate::log_warn!("theme", "spice {key} leftovers not removed: {e}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  The storefront card: lines, button, warning
// ═══════════════════════════════════════════════════════════════════════

/// Calls that Cinnamon executes while stopping until the answer: a spice with
/// them can hang the shell (the same rule as in the Cinnamon settings
/// window). This does not forbid installation — it warns.
pub const BLOCKING_CALLS: [&str; 4] = ["spawn_sync", "spawn_command_line_sync", "GTop", "get_file_contents_utf8_sync"];

/// Whether the `.js` of an installed spice has blocking calls
/// ([`BLOCKING_CALLS`]). No more than 4 MB per file and 2000 files are read;
/// links are not followed.
pub fn scan_for_danger(dir: &Path) -> bool {
    fn walk(dir: &Path, budget: &mut u32) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else { return false };
        for e in rd.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let path = e.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.is_dir() {
                if walk(&path, budget) {
                    return true;
                }
            } else if md.is_file() && md.len() <= 4 << 20 && path.extension().is_some_and(|x| x == "js") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if BLOCKING_CALLS.iter().any(|c| text.contains(c)) {
                        return true;
                    }
                }
            }
        }
        false
    }
    let mut budget = 2000;
    walk(dir, &mut budget)
}

/// The installed spice directory for the [`scan_for_danger`] check; themes
/// have no code — `None`.
pub fn code_dir(env: &SpiceEnv, t: &SpiceType, key: &str) -> Option<String> {
    (!t.is_theme() && valid_key(key)).then(|| snap_join(&install_root(env, t), key))
}

/// The warning line on the card.
pub fn danger_line() -> String {
    tr("⚠ May freeze Cinnamon for a moment")
}

/// The warning tooltip.
pub fn danger_tooltip() -> String {
    tr("This item runs commands and waits for them to finish. While it waits, the whole Cinnamon desktop stops responding — usually for a moment, but a slow command can freeze it for longer.")
}

/// The card's stat line after the tile's star icon: "N · updated
/// YYYY.MM.DD" (N — the site's "likes"; the site has no download counts).
pub fn stats_line(score: i64, last_edited: i64) -> String {
    let date = version_label(last_edited);
    let n = format!("\u{a0}{}", score.max(0));
    if date.is_empty() {
        n
    } else {
        format!("{n} · {}", trf("updated %1", &[&date]))
    }
}

/// The stat line tooltip.
pub fn stats_tooltip() -> String {
    tr("How many people liked it on the Cinnamon Spices website, and when it was last changed")
}

/// "Likes" in the details: "⭐ N".
pub fn detail_stars(score: i64) -> String {
    format!("⭐ {}", score.max(0))
}

/// The card button: installed and newer on the site — "Update", otherwise as for
/// any store item (spices have no "Apply" on the storefront card:
/// the installed tile above and its button enable them).
pub fn card_button(installed: bool, has_update: bool) -> crate::appearance_page::CardButton {
    card_button_for(installed, has_update, false)
}

/// [`card_button`] with "Apply" for an installed one (`can_apply`): this is how a Spices
/// theme behaves in the GTK and window frames sections ([`PART_SECTIONS`]) — an installed
/// grid with its own button may not be there (a theme without GTK 3/4 is not visible in
/// "GTK themes"), and the card itself enables it.
pub fn card_button_for(installed: bool, has_update: bool, can_apply: bool) -> crate::appearance_page::CardButton {
    if installed && has_update {
        return crate::appearance_page::CardButton { text: tr("⬆ Update"), state: "false", enabled: true };
    }
    crate::appearance_page::card_install_button(installed, false, can_apply)
}

/// The main details button — the same states as [`card_button`].
pub fn detail_button(installed: bool, has_update: bool) -> crate::appearance_page::InstallButton {
    detail_button_for(installed, has_update, false)
}

/// [`detail_button`] with "Apply" for an installed one (see [`card_button_for`]).
pub fn detail_button_for(installed: bool, has_update: bool, can_apply: bool) -> crate::appearance_page::InstallButton {
    if installed && has_update {
        return crate::appearance_page::InstallButton {
            text: tr("⬆ Update"),
            tooltip: tr("A newer version is on the Cinnamon Spices website. It's installed over this one; its settings are kept."),
            state: "false",
        };
    }
    crate::appearance_page::install_button(installed, false, can_apply)
}

#[cfg(test)]
mod uninstall_tests {
    use super::*;

    fn env() -> SpiceEnv {
        SpiceEnv { home: "/home/u".into(), config: "/home/u/.cfg".into() }
    }

    fn none() -> FsSnapshot {
        FsSnapshot::from_json(&serde_json::json!({}))
    }

    /// A snapshot of the translations directory: `ru` and `pt_BR` with the spice's `.mo`, `de` — without.
    fn locales(key: &str) -> FsSnapshot {
        let entries = vec![
            format!("ru/LC_MESSAGES/{key}.mo"),
            format!("pt_BR/LC_MESSAGES/{key}.mo"),
            "de/LC_MESSAGES/other@x.mo".to_string(),
        ];
        snapshot_from_entries("/home/u/.local/share/locale", &entries, &Map::new())
    }

    #[test]
    fn update_is_older_mark_only() {
        assert!(has_update(Some(100), 200));
        assert!(!has_update(Some(200), 200));
        assert!(!has_update(Some(300), 200));
        assert!(!has_update(None, 200), "without a mark there is nothing to compare with");
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": 1_700_000_000})), Some(1_700_000_000));
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": " 42 "})), Some(42));
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": 4.2e1})), Some(42));
        assert_eq!(installed_last_edited(&serde_json::json!({"uuid": "a@b"})), None);
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": "yesterday"})), None);
    }

    #[test]
    fn list_without_matches_exact_field() {
        let applets: Vec<String> = [
            "panel1:left:0:menu@cinnamon.org",
            "panel1:right:0:weather@mockturtl:8",
            "panel1:right:1:!weather@mockturtl:9",
            "panel1:right:2:xweather@mockturtl:10",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            list_without(&applets, "weather@mockturtl", Some(3)),
            vec!["panel1:left:0:menu@cinnamon.org".to_string(), "panel1:right:2:xweather@mockturtl:10".to_string()]
        );
        // A stock entry without an instance number is found too.
        assert_eq!(list_without(&applets, "menu@cinnamon.org", Some(3)).len(), 3);
        let desklets = vec!["a@b:0:100:100".to_string(), "xa@b:1:10:10".to_string()];
        assert_eq!(list_without(&desklets, "a@b", Some(0)), vec!["xa@b:1:10:10".to_string()]);
        let exts = vec!["e@x".to_string(), "e@xy".to_string()];
        assert_eq!(list_without(&exts, "e@x", None), vec!["e@xy".to_string()]);
    }

    #[test]
    fn applet_disabled_then_files_translations_settings_journal() {
        let key = "weather@mockturtl";
        let steps = plan_uninstall(&env(), type_for_key("applet").unwrap(), key, &locales(key)).unwrap();
        let l = "/home/u/.local/share/locale";
        assert_eq!(
            steps,
            vec![
                SpiceStep::GsettingsListDrop { schema: "org.cinnamon".into(), key: "enabled-applets".into(), item: key.into(), field: Some(3) },
                SpiceStep::RemoveTree { path: format!("/home/u/.local/share/cinnamon/applets/{key}") },
                SpiceStep::RemoveFile { path: format!("{l}/pt_BR/LC_MESSAGES/{key}.mo") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/pt_BR/LC_MESSAGES") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/pt_BR") },
                SpiceStep::RemoveFile { path: format!("{l}/ru/LC_MESSAGES/{key}.mo") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/ru/LC_MESSAGES") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/ru") },
                SpiceStep::RemoveTree { path: format!("/home/u/.cfg/cinnamon/spices/{key}") },
                SpiceStep::RemoveTree { path: format!("/home/u/.cinnamon/configs/{key}") },
                SpiceStep::Forget { content_id: format!("spices:applet:{key}") },
            ]
        );
    }

    #[test]
    fn desklet_and_extension_use_their_lists() {
        let empty = none();
        let d = plan_uninstall(&env(), type_for_key("desklet").unwrap(), "AlbumArt3.0@claudiux", &empty).unwrap();
        assert_eq!(
            d[0],
            SpiceStep::GsettingsListDrop {
                schema: "org.cinnamon".into(),
                key: "enabled-desklets".into(),
                item: "AlbumArt3.0@claudiux".into(),
                field: Some(0)
            }
        );
        assert_eq!(d[1], SpiceStep::RemoveTree { path: "/home/u/.local/share/cinnamon/desklets/AlbumArt3.0@claudiux".into() });
        let e = plan_uninstall(&env(), type_for_key("extension").unwrap(), "x@y", &empty).unwrap();
        assert_eq!(e[0], SpiceStep::GsettingsListDrop { schema: "org.cinnamon".into(), key: "enabled-extensions".into(), item: "x@y".into(), field: None });
        assert!(!e.iter().any(|s| matches!(s, SpiceStep::RemoveFile { .. })), "no translations — nothing to remove");
    }

    #[test]
    fn theme_resets_only_if_current_and_keeps_others() {
        let steps = plan_uninstall(&env(), type_for_key("theme").unwrap(), "Adara-Dark", &locales("Adara-Dark")).unwrap();
        assert_eq!(
            steps,
            vec![
                SpiceStep::GsettingsResetIf { schema: "org.cinnamon.theme".into(), key: "name".into(), value: "Adara-Dark".into() },
                SpiceStep::RemoveTree { path: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::Forget { content_id: "spices:theme:Adara-Dark".into() },
            ]
        );
    }

    #[test]
    fn action_removes_both_roots_and_its_disabled_line() {
        let key = "backup-this-file@h3nr1ke";
        let steps = plan_uninstall(&env(), type_for_key("action").unwrap(), key, &none()).unwrap();
        let d = "/home/u/.local/share/nemo/actions";
        assert_eq!(steps[0], SpiceStep::RemoveFile { path: format!("{d}/{key}.nemo_action") }, "the menu item goes first");
        assert_eq!(steps[1], SpiceStep::RemoveTree { path: format!("{d}/{key}") });
        assert_eq!(
            steps[2],
            SpiceStep::GsettingsListDrop { schema: "org.nemo.plugins".into(), key: "disabled-actions".into(), item: format!("{key}.nemo_action"), field: None }
        );
        assert_eq!(steps.last(), Some(&SpiceStep::Forget { content_id: format!("spices:action:{key}") }));
    }

    #[test]
    fn bad_key_refused_before_any_step() {
        for bad in ["", "..", "../x", "a/b"] {
            assert_eq!(plan_uninstall(&env(), type_for_key("applet").unwrap(), bad, &none()), Err(SpiceRefusal::InvalidKey));
        }
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// gsettings in memory: `(schema, key)` → a list or a string.
    #[derive(Default)]
    struct MemSys {
        lists: HashMap<(String, String), Vec<String>>,
        strings: HashMap<(String, String), String>,
        resets: Vec<(String, String)>,
        reloads: Vec<(String, String)>,
    }

    impl SpiceSys for MemSys {
        fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>> {
            Some(self.lists.get(&(schema.into(), key.into())).cloned().unwrap_or_default())
        }
        fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String> {
            self.lists.insert((schema.into(), key.into()), items.to_vec());
            Ok(())
        }
        fn string_get(&mut self, schema: &str, key: &str) -> Option<String> {
            self.strings.get(&(schema.into(), key.into())).cloned()
        }
        fn reset(&mut self, schema: &str, key: &str) -> Result<(), String> {
            self.strings.remove(&(schema.into(), key.into()));
            self.resets.push((schema.into(), key.into()));
            Ok(())
        }
        fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String> {
            self.reloads.push((uuid.into(), xlet.into()));
            Ok(())
        }
    }

    /// A temporary HOME (the files are small).
    struct Home {
        root: std::path::PathBuf,
        env: SpiceEnv,
    }

    impl Home {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("rexon-spices-exec-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).unwrap();
            let home = root.join("home").to_string_lossy().into_owned();
            Home { env: SpiceEnv::at_home(&home), root }
        }
        fn work(&self, n: u32) -> std::path::PathBuf {
            let w = self.root.join(format!("work{n}"));
            std::fs::create_dir_all(&w).unwrap();
            w
        }
        /// The tree under HOME: "path permissions" alphabetically (like `find -printf '%P %m'`).
        fn tree(&self) -> Vec<String> {
            fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
                use std::os::unix::fs::PermissionsExt;
                for e in std::fs::read_dir(dir).unwrap().flatten() {
                    let md = std::fs::symlink_metadata(e.path()).unwrap();
                    let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().into_owned();
                    out.push(format!("{rel} {:o}", md.permissions().mode() & 0o777));
                    if md.is_dir() {
                        walk(base, &e.path(), out);
                    }
                }
            }
            let mut out = Vec::new();
            let base = Path::new(&self.env.home);
            walk(base, base, &mut out);
            out.sort();
            out
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn trees() -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }

    /// Build a zip from a fixture: names, permissions, the texts of `metadata.json` and
    /// `.nemo_action`; the `da.po` of applet 1440 is a real translation.
    fn make_zip(archive: &str, dest: &Path) {
        let all = trees();
        let t = &all[archive];
        let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
        for e in t["entries"].as_array().unwrap() {
            let name = e[0].as_str().unwrap();
            let mode = e[1].as_u64().unwrap() as u32;
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(mode);
            if name.ends_with('/') {
                zw.add_directory(name, opts).unwrap();
            } else {
                zw.start_file(name, opts).unwrap();
                let body = if name == "1440@jvlianodorneles/po/da.po" {
                    std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/po/1440-da.po", env!("CARGO_MANIFEST_DIR"))).unwrap()
                } else {
                    t["texts"].get(name).and_then(Value::as_str).unwrap_or("").to_string()
                };
                zw.write_all(body.as_bytes()).unwrap();
            }
        }
        zw.finish().unwrap();
    }

    fn spice(key: &str, id: Value, ts: i64) -> Spice {
        Spice { key: key.into(), spices_id: id, last_edited: ts, name: key.into(), ..Default::default() }
    }

    type Journal = Vec<(String, Vec<String>)>;

    fn install(h: &Home, n: u32, archive: &str, kind: &str, s: &Spice, sys: &mut MemSys, journal: &mut Journal) -> (bool, String) {
        let work = h.work(n);
        let zip = work.join("a.zip");
        make_zip(archive, &zip);
        let t = type_for_key(kind).unwrap();
        let mut j = |st: &SpiceStep, paths: &[String]| {
            if let SpiceStep::Record { content_id, .. } = st {
                journal.push((content_id.clone(), paths.to_vec()));
            }
            Ok::<(), String>(())
        };
        install_archive(&h.env, t, s, &s.name, &zip, &work, Some((6, 6, 9)), sys, &mut j, &mut |_| {})
    }

    #[test]
    fn applet_install_tree_metadata_translations_update_and_removal() {
        let h = Home::new("applet");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "1440@jvlianodorneles";
        let (ok, msg) = install(&h, 0, key, "applet", &spice(key, Value::from(427), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        // The tree: every archive entry — with its permissions.
        let tree = h.tree();
        let all = trees();
        for e in all[key]["entries"].as_array().unwrap() {
            let name = e[0].as_str().unwrap().trim_end_matches('/');
            let want = format!(".local/share/cinnamon/applets/{name} {:o}", e[1].as_u64().unwrap());
            assert!(tree.contains(&want), "missing {want}: {tree:#?}");
        }
        assert!(tree.iter().any(|l| l.starts_with(&format!(".local/share/locale/da/LC_MESSAGES/{key}.mo "))), "{tree:#?}");
        // last-edited is appended at the end, previous keys in the previous order, indent 4.
        let dir = format!(".local/share/cinnamon/applets/{key}");
        let meta = std::fs::read_to_string(format!("{}/{dir}/metadata.json", h.env.home)).unwrap();
        let v: Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(installed_last_edited(&v), Some(1_700_000_000));
        let orig: Value = serde_json::from_str(all[key]["texts"][format!("{key}/metadata.json")].as_str().unwrap()).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        let orig_keys: Vec<&String> = orig.as_object().unwrap().keys().collect();
        assert_eq!(&keys[..orig_keys.len()], &orig_keys[..]);
        assert_eq!(keys.last().map(|k| k.as_str()), Some("last-edited"));
        assert!(meta.contains("\n    \""), "indent 4: {meta}");
        assert_eq!(journal, vec![(format!("spices:applet:{key}"), vec![format!("{}/{dir}", h.env.home)])]);
        assert_eq!(installed_state(&h.env, type_for_key("applet").unwrap(), key), Some(Some(1_700_000_000)));
        assert!(sys.reloads.is_empty(), "a fresh applet is not enabled — nothing to reload");

        // Updating an enabled one: the settings are intact, ReloadXlet was called.
        let cfg = format!("{}/cinnamon/spices/{key}", h.env.config);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(format!("{cfg}/1.json"), "{}").unwrap();
        sys.lists.insert(("org.cinnamon".into(), "enabled-applets".into()), vec![format!("panel1:right:0:{key}:3")]);
        let (ok, msg) = install(&h, 1, key, "applet", &spice(key, Value::from(427), 1_800_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        assert_eq!(sys.reloads, vec![(key.to_string(), "APPLET".to_string())]);
        assert!(Path::new(&format!("{cfg}/1.json")).is_file(), "settings survived the update");
        assert_eq!(installed_state(&h.env, type_for_key("applet").unwrap(), key), Some(Some(1_800_000_000)));

        // Removal: disabled, the directory, translation and settings removed.
        let (ok, msg) = uninstall_with(&h.env, type_for_key("applet").unwrap(), key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(sys.lists[&("org.cinnamon".to_string(), "enabled-applets".to_string())].is_empty());
        let left = h.tree();
        assert!(!left.iter().any(|l| l.contains(key)), "{left:#?}");
        assert!(!left.iter().any(|l| l.starts_with(".local/share/locale/da")), "an empty language directory is removed: {left:#?}");
    }

    #[test]
    fn action_two_roots_disabled_then_removed() {
        let h = Home::new("action");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "backup-this-file@h3nr1ke";
        let (ok, msg) = install(&h, 0, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let d = ".local/share/nemo/actions";
        let tree = h.tree();
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}.nemo_action "))), "{tree:#?}");
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}/metadata.json "))), "{tree:#?}");
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}/")) && l.ends_with(".sh 755")), "the script is executable: {tree:#?}");
        let disabled = ("org.nemo.plugins".to_string(), "disabled-actions".to_string());
        assert_eq!(sys.lists[&disabled], vec![format!("{key}.nemo_action")], "a fresh action is disabled");
        assert_eq!(journal[0].1.len(), 2, "the journal remembers the directory and the file: {journal:?}");
        // The person enabled it; an update does not touch the list.
        sys.lists.insert(disabled.clone(), Vec::new());
        let (ok, _) = install(&h, 1, key, "action", &spice(key, Value::from(37), 1_800_000_000), &mut sys, &mut journal);
        assert!(ok);
        assert!(sys.lists[&disabled].is_empty());
        let (ok, msg) = uninstall_with(&h.env, type_for_key("action").unwrap(), key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(!h.tree().iter().any(|l| l.contains(key)), "{:#?}", h.tree());
    }

    /// Step 3d: the purchase is recorded in the store log — the directory and the action file
    /// are removed by the shared install log, disabling and cleaning up translations and
    /// settings — by the plan, as before; the shared log is checked and the record is gone.
    /// In place of the spice directory there is a link of the person's: the link itself is removed,
    /// what it points to stays intact (as before).
    #[test]
    fn uninstall_goes_by_install_journal_when_recorded() {
        let h = Home::new("by-journal");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "backup-this-file@h3nr1ke";
        let t = type_for_key("action").unwrap();
        let (ok, msg) = install(&h, 0, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let record = |paths: &[String]| {
            let log = crate::ocs_store::install_log_path_xdg(Some(&h.env.config), &h.env.home);
            std::fs::create_dir_all(Path::new(&log).parent().unwrap()).unwrap();
            let card = serde_json::json!({ item_id(t, key): {"name": key, "rexon_type": "nemo-actions", "paths": paths} });
            std::fs::write(&log, card.to_string()).unwrap();
            log
        };
        let log = record(&journal[0].1);
        let rexon = Path::new(&log).parent().unwrap().to_path_buf();
        let cfg = format!("{}/cinnamon/spices/{key}", h.env.config);
        std::fs::create_dir_all(&cfg).unwrap();
        let (ok, msg) = uninstall_with(&h.env, t, key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(!h.tree().iter().any(|l| l.contains(key)), "{:#?}", h.tree());
        assert!(crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref()).is_empty());
        let general = crate::install_journal::parse(std::fs::read_to_string(crate::install_journal::journal_path(&rexon)).ok().as_deref())
            .expect("shared journal checked");
        assert!(general.items.is_empty(), "{general:#?}");

        // Something foreign in place of the directory: the person put a link to their own thing there.
        // The store log does not store path kinds, the check looks at the disk.
        let (ok, msg) = install(&h, 1, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let dir = format!("{}/{key}", install_root(&h.env, t));
        record(&journal[1].1);
        std::fs::remove_dir_all(&dir).unwrap();
        let mine = h.root.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::os::unix::fs::symlink(&mine, &dir).unwrap();
        // The check sees the link: the log now has a link — and the link itself
        // is removed, not what it points to.
        let (ok, msg) = uninstall_with(&h.env, t, key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(mine.is_dir(), "the link target is removed");
        assert!(std::fs::symlink_metadata(&dir).is_err());
    }

    #[test]
    fn theme_desklet_extension_multiversion_install_where_cinnamon_looks() {
        let h = Home::new("misc");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        for (n, (archive, kind, dir)) in [
            ("Adara-Dark", "theme", ".themes/Adara-Dark"),
            ("AlbumArt3.0@claudiux", "desklet", ".local/share/cinnamon/desklets/AlbumArt3.0@claudiux"),
            ("ArrangementOfButtonsOnWindowTitleBar@claudiux", "extension", ".local/share/cinnamon/extensions/ArrangementOfButtonsOnWindowTitleBar@claudiux"),
            ("weather@mockturtl", "applet", ".local/share/cinnamon/applets/weather@mockturtl"),
        ]
        .into_iter()
        .enumerate()
        {
            let (ok, msg) = install(&h, n as u32, archive, kind, &spice(archive, Value::from(1), 1_700_000_000), &mut sys, &mut journal);
            assert!(ok, "{archive}: {msg}");
            let meta: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/{dir}/metadata.json", h.env.home)).unwrap()).unwrap();
            assert_eq!(installed_last_edited(&meta), Some(1_700_000_000), "{archive}");
        }
        // The desklet scripts' permissions were preserved (0755 in the archive).
        let tree = h.tree();
        assert!(tree.iter().any(|l| l.contains("AlbumArt3.0@claudiux/scripts/") && l.ends_with(" 755")), "{tree:#?}");
        // The theme is selected — removal resets it to the default.
        sys.strings.insert(("org.cinnamon.theme".into(), "name".into()), "Adara-Dark".into());
        let (ok, _) = uninstall_with(&h.env, type_for_key("theme").unwrap(), "Adara-Dark", "Adara Dark", &mut sys, &mut |_| {});
        assert!(ok);
        assert_eq!(sys.resets, vec![("org.cinnamon.theme".to_string(), "name".to_string())]);
        assert!(!Path::new(&format!("{}/.themes/Adara-Dark", h.env.home)).exists());
    }

    #[test]
    fn refusals_leave_home_untouched() {
        let h = Home::new("refuse");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        // The index key does not match the directory in the archive.
        let (ok, msg) = install(&h, 0, "1440@jvlianodorneles", "applet", &spice("other@x", Value::from(1), 1), &mut sys, &mut journal);
        assert!(!ok && msg.contains("other@x"), "{msg}");
        // Not a zip (a WAF page instead of the archive).
        let work = h.work(1);
        std::fs::write(work.join("a.zip"), "<html>WAF</html>").unwrap();
        let (ok, _) = install_archive(
            &h.env,
            type_for_key("applet").unwrap(),
            &spice("a@b", Value::Null, 1),
            "a@b",
            &work.join("a.zip"),
            &work,
            None,
            &mut sys,
            &mut |_, _| Ok(()),
            &mut |_| {},
        );
        assert!(!ok);
        assert!(h.tree().is_empty(), "{:#?}", h.tree());
        assert!(journal.is_empty());
    }

    #[test]
    fn executor_refuses_paths_outside_roots() {
        let mut sys = MemSys::default();
        for p in ["/etc/x", "/home/u/../etc", "/home/u/a/../../etc", "/home/uu/x", "/home/u"] {
            let steps = [SpiceStep::RemoveTree { path: p.into() }];
            let err = execute(&steps, &["/home/u"], &mut sys, &mut |_| Ok(()), &mut |_| {}).unwrap_err();
            assert!(err.contains(p), "{p}: {err}");
        }
    }

    #[test]
    fn last_edited_keeps_order_and_creates_for_themes() {
        let out = with_last_edited(Some(r#"{"uuid":"a@b","name":"Ä","last-edited":1,"z":2}"#), 5).unwrap();
        assert_eq!(out, "{\n    \"uuid\": \"a@b\",\n    \"name\": \"Ä\",\n    \"last-edited\": 5,\n    \"z\": 2\n}");
        assert_eq!(with_last_edited(None, 7).unwrap(), "{\n    \"last-edited\": 7\n}");
        assert!(with_last_edited(Some("[1]"), 7).is_err());
    }

    #[test]
    fn cleanup_after_tile_removal_takes_translations_settings_and_action_dir() {
        let h = Home::new("cleanup");
        let home = h.env.home.clone();
        let key = "a@b";
        for p in [
            format!("{home}/.local/share/locale/ru/LC_MESSAGES/{key}.mo"),
            format!("{home}/.local/share/locale/ru/LC_MESSAGES/keep.mo"),
            format!("{}/cinnamon/spices/{key}/1.json", h.env.config),
            format!("{home}/.local/share/nemo/actions/{key}/run.sh"),
        ] {
            std::fs::create_dir_all(Path::new(&p).parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap();
        }
        // The tile was removed: the `.nemo_action` is already gone.
        cleanup_after_removal(&h.env, &format!("{home}/.local/share/nemo/actions/{key}.nemo_action"));
        let tree = h.tree();
        assert!(!tree.iter().any(|l| l.contains(key)), "{tree:#?}");
        assert!(tree.iter().any(|l| l.contains("keep.mo")), "a foreign translation stays: {tree:#?}");
        // A path not from the spices directory — nothing.
        cleanup_after_removal(&h.env, &format!("{home}/.local/share/locale"));
        assert!(h.tree().iter().any(|l| l.contains("keep.mo")));
    }
}

#[cfg(test)]
mod card_tests {
    use super::*;

    #[test]
    fn danger_scan_finds_blocking_calls_in_js_only() {
        let dir = std::env::temp_dir().join(format!("rexon-spices-danger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("3.8")).unwrap();
        std::fs::write(dir.join("applet.js"), "const x = 1;").unwrap();
        std::fs::write(dir.join("README.md"), "GLib.spawn_command_line_sync").unwrap();
        assert!(!scan_for_danger(&dir), "README is not code");
        std::fs::write(dir.join("3.8/applet.js"), "let [ok, out] = GLib.spawn_command_line_sync('ls');").unwrap();
        assert!(scan_for_danger(&dir), "a version subdirectory is checked too");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!scan_for_danger(Path::new("/nonexistent/rexon")));
        let env = SpiceEnv::at_home("/home/u");
        assert_eq!(code_dir(&env, type_for_key("theme").unwrap(), "Adara-Dark"), None, "a theme has no code");
        assert_eq!(
            code_dir(&env, type_for_key("applet").unwrap(), "a@b").as_deref(),
            Some("/home/u/.local/share/cinnamon/applets/a@b")
        );
        assert_eq!(code_dir(&env, type_for_key("applet").unwrap(), "../x"), None);
    }

    #[test]
    fn card_lines_and_buttons() {
        let line = stats_line(42, 1_780_583_449);
        assert!(line.contains("42") && line.contains("2026.06.04") && !line.contains('★'), "{line}");
        assert_eq!(stats_line(-3, 0), "\u{a0}0");
        assert_ne!(card_button(true, true).text, crate::appearance_page::card_install_button(true, false, false).text);
        assert_eq!(card_button(true, false).text, crate::appearance_page::card_install_button(true, false, false).text);
        assert_eq!(card_button(false, true).text, crate::appearance_page::card_install_button(false, false, false).text, "not installed — Install, not Update");
        assert_ne!(detail_button(true, true).text, crate::appearance_page::install_button(true, false, false).text);
        assert!(!detail_button(true, true).tooltip.is_empty());
        assert_eq!(detail_stars(7), "⭐ 7");
        assert!(!danger_line().is_empty() && !danger_tooltip().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Spices themes on other desktops: the GTK and window frames sections
// ═══════════════════════════════════════════════════════════════════════
//
// A Spices theme is a Cinnamon shell theme (`cinnamon/cinnamon.css`), but
// almost half of them carry in the same directory an application theme too
// (`gtk-3.0`, `gtk-4.0`, `gtk-2.0`), and window frames (`metacity-1` — Marco and
// Metacity, `xfwm4`). Measurement of the theme repository tree on 24.10.2026: of 108 themes
// 50 have a GTK 3/4 stylesheet, 45 have Metacity frames (one more has the directory
// but no theme file in it), 16 have xfwm4 frames; 56 are Cinnamon only.
//
// Outside Cinnamon such a theme is shown in the sections "GTK themes", "Window frames
// (XFCE)", "Window frames (MATE)" and "GNOME Shell theme" (four themes also have
// `gnome-shell`) — and only one that has this section's part: showing a theme without it would mean installing something that will not
// apply here. The theme contents are not listed in the site index; they come from the
// theme repository tree on GitHub — ONE request for all themes, once a day,
// conditional (`ETag` → 304). It is still the site archive that gets installed, and at install time
// it is the archive itself that is checked ([`install_archive_in`]).

/// Sections where Spices themes are shown by their contents: "Appearance"
/// section keys (the table is in `categories`).
pub const PART_SECTIONS: &[&str] = &["gtk", "xfwm4", "marco", "gnomeshell"];

/// The theme repository tree (all paths in one response). Inside
/// `<key>/files/<key>/…` is exactly what the site puts into the archive `<key>/…`.
pub const THEME_TREE_URL: &str =
    "https://api.github.com/repos/linuxmint/cinnamon-spices-themes/git/trees/master?recursive=1";
/// The tree ceiling after decompression (currently ~9 MB).
pub const MAX_TREE_BYTES: u64 = 64 << 20;
/// The version of the theme contents cache file.
pub const PARTS_CACHE_VERSION: u32 = 1;

/// Whether this is a section where Spices themes are shown by contents.
pub fn is_part_section(section: &str) -> bool {
    PART_SECTIONS.contains(&section)
}

/// The card's section if it is in the GTK or window frames section
/// ([`PART_SECTIONS`]) — by the section number in `category_id` (set by
/// [`section_theme_item`]).
pub fn part_section_of(item: &ThemeItem) -> Option<&'static str> {
    if !parse_item_id(&item.id).is_some_and(|(t, _)| t.is_theme()) {
        return None;
    }
    let key = crate::categories::category_by_id(Path::new(""), item.category_id)?.key;
    PART_SECTIONS.iter().copied().find(|s| *s == key)
}

/// The section part's subdirectory — from the matrix (`Payload::ThemeSubdir`), not
/// as a second copy: `xfwm4` for XFCE, `metacity-1` for MATE, `gnome-shell` for GNOME.
fn part_subdir(section: &str) -> Option<&'static str> {
    let kind = crate::categories::category_kind(section)?;
    match crate::de_customization::row(kind)?.payload {
        crate::de_customization::Payload::ThemeSubdir { sub } => Some(sub),
        _ => None,
    }
}

/// Files without which the part will not be read: xfwm4 reads `themerc`, Marco and
/// Metacity — `metacity-theme-<N>.xml` (Marco knows formats 1–3), the GNOME
/// shell — `gnome-shell.css`.
fn part_markers(sub: &str) -> &'static [&'static str] {
    match sub {
        "xfwm4" => &["themerc"],
        "metacity-1" => &["metacity-theme-3.xml", "metacity-theme-2.xml", "metacity-theme-1.xml"],
        "gnome-shell" => &["gnome-shell.css"],
        _ => &[],
    }
}

/// Whether the theme has what the section `section` will apply. `has(path)` — whether there is
/// a file or directory at the path from the theme root (`gtk-3.0/gtk.css`).
///
/// An application theme — the same rule as for the "GTK themes" grid
/// (`installed_scan::GTK_PARTS` with `gtk.css`): what the storefront showed, the grid
/// will see after installation. Frames and the GNOME shell — the section's subdirectory from
/// the matrix with a theme file inside: an empty `metacity-1` (it happens, see the measurement above)
/// will not be read by Marco.
pub fn fits_section(section: &str, has: &dyn Fn(&str) -> bool) -> bool {
    if section == "gtk" {
        return crate::installed_scan::GTK_PARTS.iter().any(|p| has(&format!("{p}/gtk.css")));
    }
    if !is_part_section(section) {
        return false;
    }
    part_subdir(section).is_some_and(|sub| part_markers(sub).iter().any(|m| has(&format!("{sub}/{m}"))))
}

/// The theme contents for the card: which parts it has. The names are technology
/// names, untranslated.
pub fn parts_summary(has: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    let any = |paths: &[&str]| paths.iter().any(|p| has(p));
    let mut out = Vec::new();
    if has("cinnamon/cinnamon.css") {
        out.push("Cinnamon");
    }
    if has("gtk-2.0/gtkrc") {
        out.push("GTK 2");
    }
    if has("gtk-3.0/gtk.css") {
        out.push("GTK 3");
    }
    if has("gtk-4.0/gtk.css") {
        out.push("GTK 4");
    }
    if any(&["metacity-1/metacity-theme-3.xml", "metacity-1/metacity-theme-2.xml", "metacity-1/metacity-theme-1.xml"]) {
        out.push("Metacity/Marco");
    }
    if has("xfwm4/themerc") {
        out.push("xfwm4");
    }
    if has("openbox-3/themerc") {
        out.push("Openbox");
    }
    if has("gnome-shell/gnome-shell.css") {
        out.push("GNOME Shell");
    }
    if has("plank") {
        out.push("Plank");
    }
    out
}

/// The contents of all themes: theme key → paths from the theme root up to two levels deep
/// (`gtk-3.0`, `gtk-3.0/gtk.css`), alphabetically.
pub type ThemeParts = std::collections::BTreeMap<String, Vec<String>>;

/// `has` for [`fits_section`] by the contents of one theme.
pub fn has_in(paths: &[String]) -> impl Fn(&str) -> bool + '_ {
    move |p: &str| paths.binary_search_by(|x| x.as_str().cmp(p)).is_ok()
}

/// Parse the theme repository tree ([`THEME_TREE_URL`]). Only
/// paths `<key>/files/<key>/…` with a valid key are taken; a truncated tree
/// (`truncated`) is an error: half the contents would silently hide themes.
pub fn parse_theme_tree(body: &str) -> Result<ThemeParts, String> {
    #[derive(Deserialize)]
    struct Entry {
        path: String,
    }
    #[derive(Deserialize)]
    struct Tree {
        #[serde(default)]
        truncated: bool,
        tree: Vec<Entry>,
    }
    let tree: Tree = serde_json::from_str(body).map_err(|e| format!("theme tree not parsed: {e}"))?;
    if tree.truncated {
        return Err("theme tree arrived truncated".to_string());
    }
    let mut out = ThemeParts::new();
    for e in &tree.tree {
        let mut it = e.path.splitn(4, '/');
        let (Some(k), Some("files"), Some(k2), Some(rest)) = (it.next(), it.next(), it.next(), it.next()) else {
            continue;
        };
        if k != k2 || !valid_key(k) || rest.is_empty() || rest.split('/').count() > 2 {
            continue;
        }
        out.entry(k.to_string()).or_default().push(rest.to_string());
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    Ok(out)
}

/// The theme contents cache file: the contents themselves (not the tree — it is dozens of times
/// larger) and the conditional request validators.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartsCache {
    pub version: u32,
    pub fetched: i64,
    pub failed: i64,
    pub etag: String,
    pub last_modified: String,
    pub parts: ThemeParts,
}

/// The contents cache path: `$XDG_CACHE_HOME/rexon/cinnamon-spices/theme-parts.json`.
pub fn parts_cache_path(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon/cinnamon-spices/theme-parts.json", crate::utils::cache_home(xdg_cache_home, home))
}

/// The result of [`refresh_parts`].
#[derive(Debug, Clone, PartialEq)]
pub struct PartsRefresh {
    pub parts: ThemeParts,
    pub source: RefreshSource,
    /// What to tell the person; empty — all is well.
    pub note: String,
}

/// Could not find out the theme contents.
pub fn parts_offline_message(have_saved: bool) -> String {
    if have_saved {
        tr("Couldn't check which Cinnamon Spices themes fit this section — the saved list is shown.")
    } else {
        tr("Couldn't find out which Cinnamon Spices themes fit this section (the list of theme contents comes from GitHub). Check your internet connection and try again later.")
    }
}

/// The theme contents for the storefront — in the same way as the index ([`refresh`]):
/// a fresh cache (younger than a day) without network; otherwise (or with `force`) a conditional
/// request; 304 extends, 200 replaces; a failure — the previous cache with a mark and
/// no retry before [`FAIL_BACKOFF_SECS`].
pub fn refresh_parts(net: &mut dyn ConditionalNet, path: &Path, force: bool, now: i64) -> PartsRefresh {
    let saved: PartsCache = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<PartsCache>(&t).ok())
        .filter(|c| c.version == PARTS_CACHE_VERSION)
        .unwrap_or(PartsCache { version: PARTS_CACHE_VERSION, ..Default::default() });
    let have = !saved.parts.is_empty();
    let fresh = have && saved.fetched > 0 && now >= saved.fetched && now - saved.fetched < CACHE_TTL_SECS;
    let backing_off = saved.failed > 0 && now >= saved.failed && now - saved.failed < FAIL_BACKOFF_SECS;
    let write = |c: &PartsCache| -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    };
    if !force && (fresh || backing_off) {
        let (source, note) = if fresh { (RefreshSource::Cache, String::new()) } else { (RefreshSource::Stale, parts_offline_message(have)) };
        return PartsRefresh { parts: saved.parts, source, note };
    }
    let (etag, lm) = if have { (saved.etag.as_str(), saved.last_modified.as_str()) } else { ("", "") };
    let reply = net.conditional_get(THEME_TREE_URL, user_agent(), etag, lm, INDEX_TIMEOUT_SECS);
    let fail = |saved: PartsCache| {
        let have = !saved.parts.is_empty();
        let _ = write(&PartsCache { failed: now, ..saved.clone() });
        PartsRefresh { parts: saved.parts, source: RefreshSource::Stale, note: parts_offline_message(have) }
    };
    match reply {
        Ok(CondReply::NotModified) if have => {
            let _ = write(&PartsCache { fetched: now, failed: 0, ..saved.clone() });
            PartsRefresh { parts: saved.parts, source: RefreshSource::NotModified, note: String::new() }
        }
        Ok(CondReply::Body { body, etag, last_modified }) => match parse_theme_tree(&String::from_utf8_lossy(&body)) {
            Ok(parts) if !parts.is_empty() => {
                let c = PartsCache { version: PARTS_CACHE_VERSION, fetched: now, failed: 0, etag, last_modified, parts };
                let _ = write(&c);
                PartsRefresh { parts: c.parts, source: RefreshSource::Downloaded, note: String::new() }
            }
            // Garbage, a truncated or empty tree do not overwrite the cache.
            _ => fail(saved),
        },
        Ok(CondReply::NotModified) | Err(_) => fail(saved),
    }
}

/// [`refresh_parts`] on the live machine: a compressed GitHub response, this
/// process's cache.
pub fn live_refresh_parts(force: bool) -> PartsRefresh {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let path = parts_cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home);
    refresh_parts(&mut UreqConditional::gzip_up_to(MAX_TREE_BYTES), Path::new(&path), force, now_secs())
}

/// Index themes the section `section` will apply: by the contents `parts`.
/// Themes whose contents are unknown (not in the tree) are not shown — showing
/// and failing to apply is worse than not showing.
pub fn themes_for_section(list: &[Spice], parts: &ThemeParts, section: &str) -> Vec<Spice> {
    list.iter()
        .filter(|s| parts.get(&s.key).is_some_and(|p| fits_section(section, &has_in(p))))
        .cloned()
        .collect()
}

/// Desktops where window frames are drawn by the GTK theme (Mutter and its descendants:
/// GNOME, Budgie, Pantheon) — there a theme from "GTK themes" changes the frames too.
fn borders_follow_gtk(de: &str) -> bool {
    use crate::de_customization::{de_from_id, De};
    matches!(de_from_id(de), Some(De::Gnome | De::Budgie | De::Pantheon))
}

/// The card's honest caveat: what of the theme will apply here. In Cinnamon
/// nothing is caveated (there the theme is enabled entirely in its own section).
pub fn part_note(section: &str, de: &str) -> Option<String> {
    use crate::de_customization::{de_from_id, De};
    if de_from_id(de) == Some(De::Cinnamon) || !is_part_section(section) {
        return None;
    }
    Some(if section == "gnomeshell" {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme's look for the GNOME Shell will be applied.")
    } else if section != "gtk" {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme's window borders will be applied.")
    } else if borders_follow_gtk(de) {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme for applications and window borders will be applied.")
    } else {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme for applications (GTK) will be applied.")
    })
}

/// An install refusal: the archive lacks the section's part (the storefront was wrong —
/// the repository and the site archive diverged).
pub fn part_missing_message(name: &str, section: &str) -> String {
    let part = match section {
        "gtk" => "GTK 3/GTK 4",
        "xfwm4" => "xfwm4",
        "gnomeshell" => "GNOME Shell",
        _ => "Metacity/Marco",
    };
    trf("%1 has no %2 part, so there's nothing to apply here — it wasn't installed.", &[&name, &part])
}

/// The card of a Spices theme in the section `section` ([`PART_SECTIONS`]) of the desktop
/// `de`: the section number in `category_id` — "Apply" will find its path by it
/// (a GTK theme, xfwm4 frames, Marco frames, the GNOME shell — the shared
/// apply mechanism), in
/// the description at the top — the [`part_note`] caveat and the theme contents.
pub fn section_theme_item(s: &Spice, lang: &str, section: &str, de: &str, parts: &[String]) -> ThemeItem {
    let t = type_for_key("theme").expect("the theme type is in the table");
    let mut it = theme_item(t, s, lang);
    if let Some(c) = crate::categories::categories(Path::new("")).into_iter().find(|c| c.key == section) {
        it.category_id = c.id;
    }
    let summary = parts_summary(&has_in(parts));
    let mut head: Vec<String> = part_note(section, de).into_iter().collect();
    if !summary.is_empty() {
        head.push(trf("Included in the theme: %1.", &[&summary.join(", ")]));
    }
    if !head.is_empty() {
        let body = std::mem::take(&mut it.description);
        it.description = if body.trim().is_empty() { head.join("\n") } else { format!("{}\n\n{body}", head.join("\n")) };
    }
    it
}

/// A click on a Spices theme card in the GTK or frames section: the theme is installed and
/// not newer on the site — apply it (the theme directory name); `None` — install
/// (or update). Decided by the disk and the saved index.
pub fn part_apply_name(env: &SpiceEnv, item: &ThemeItem, index: &[Spice]) -> Option<String> {
    part_section_of(item)?;
    let (t, key) = parse_item_id(&item.id)?;
    let mark = installed_state(env, t, key)?;
    let newer = index.iter().find(|s| s.key == key).is_some_and(|s| has_update(mark, s.last_edited));
    (!newer).then(|| key.to_string())
}

/// [`part_apply_name`] on the live machine.
pub fn live_part_apply_name(item: &ThemeItem) -> Option<String> {
    part_section_of(item)?;
    let t = type_for_key("theme")?;
    let index = load_cache(Path::new(&live_cache_path(t))).map(|(_, l)| l).unwrap_or_default();
    part_apply_name(&SpiceEnv::live(), item, &index)
}

/// The "Cinnamon Spices" chip tooltip in the GTK or window frames section.
pub fn parts_chip_tooltip(section: &str) -> String {
    if section == "gtk" {
        tr("Themes from the Cinnamon Spices website that include a theme for applications (GTK 3 or GTK 4)")
    } else if section == "gnomeshell" {
        tr("Themes from the Cinnamon Spices website that include a GNOME Shell theme")
    } else {
        tr("Themes from the Cinnamon Spices website that include window borders for this desktop")
    }
}

#[cfg(test)]
mod part_tests {
    use super::*;
    use crate::net::NetError;
    use std::io::Write;

    /// A trimmed real GitHub response (the theme repository tree, 24.10.2026):
    /// Winterbloom — Cinnamon + GTK 3; Orchidea — Cinnamon + Metacity;
    /// Aune — GTK 3/4, xfwm4 and `metacity-1` without a theme file; Adara-Dark —
    /// Cinnamon only; CBlack — GTK 2/3, Metacity, xfwm4. Plus foreign paths:
    /// `.github`, a directory with another key inside, a key with a space.
    fn tree_body() -> String {
        std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/theme_tree.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn parts() -> ThemeParts {
        parse_theme_tree(&tree_body()).unwrap()
    }

    fn spice(key: &str, ts: i64) -> Spice {
        Spice { key: key.into(), name: key.into(), description: "Desc".into(), last_edited: ts, file: format!("/files/themes/{key}.zip"), ..Default::default() }
    }

    fn keys(list: &[Spice]) -> Vec<&str> {
        list.iter().map(|s| s.key.as_str()).collect()
    }

    #[test]
    fn tree_gives_parts_of_each_theme_and_nothing_else() {
        let p = parts();
        assert_eq!(p.keys().map(String::as_str).collect::<Vec<_>>(), ["Adara-Dark", "Aune", "CBlack", "Orchidea", "Winterbloom"]);
        let w = &p["Winterbloom"];
        assert!(w.contains(&"gtk-3.0".to_string()) && w.contains(&"gtk-3.0/gtk.css".to_string()), "{w:?}");
        assert!(w.windows(2).all(|x| x[0] < x[1]), "alphabetical, no duplicates");
        assert!(!p["CBlack"].iter().any(|x| x.split('/').count() > 2), "nothing deeper than two levels");
        // A truncated tree is an error, not half the themes; garbage — too.
        let cut = tree_body().replacen("\"truncated\": false", "\"truncated\": true", 1);
        assert!(cut.contains("\"truncated\": true"));
        assert!(parse_theme_tree(&cut).is_err());
        assert!(parse_theme_tree("<html>rate limit</html>").is_err());
    }

    #[test]
    fn section_shows_only_themes_with_its_part() {
        let p = parts();
        let index: Vec<Spice> = ["Adara-Dark", "Aune", "CBlack", "Orchidea", "Winterbloom", "NotInTree"].iter().map(|k| spice(k, 1)).collect();
        assert_eq!(keys(&themes_for_section(&index, &p, "gtk")), ["Aune", "CBlack", "Winterbloom"]);
        assert_eq!(keys(&themes_for_section(&index, &p, "xfwm4")), ["Aune", "CBlack"]);
        // Aune has a metacity-1 directory but no theme file in it: Marco will
        // not read it — it is not in "Window frames (MATE)".
        assert_eq!(keys(&themes_for_section(&index, &p, "marco")), ["CBlack", "Orchidea"]);
        // Foreign sections and the Cinnamon section itself — not by contents.
        assert!(themes_for_section(&index, &p, "cinnamontheme").is_empty());
        assert!(themes_for_section(&index, &p, "icons").is_empty());
        // None of the five has a GNOME shell; a theme with one does.
        assert!(themes_for_section(&index, &p, "gnomeshell").is_empty());
        let shell = vec!["gnome-shell".to_string(), "gnome-shell/gnome-shell.css".to_string()];
        assert!(fits_section("gnomeshell", &has_in(&shell)) && !fits_section("gtk", &has_in(&shell)));
        assert!(!fits_section("gnomeshell", &has_in(&shell[..1])), "an empty directory is not a shell theme");
        // Part subdirectories — from the matrix, not as a second copy.
        assert_eq!(part_subdir("xfwm4"), Some("xfwm4"));
        assert_eq!(part_subdir("marco"), Some("metacity-1"));
        assert_eq!(part_subdir("gnomeshell"), Some("gnome-shell"));
    }

    #[test]
    fn parts_summary_names_what_is_inside() {
        let p = parts();
        assert_eq!(parts_summary(&has_in(&p["CBlack"])), ["Cinnamon", "GTK 2", "GTK 3", "Metacity/Marco", "xfwm4"]);
        assert_eq!(parts_summary(&has_in(&p["Adara-Dark"])), ["Cinnamon"]);
        assert_eq!(parts_summary(&has_in(&p["Aune"])), ["Cinnamon", "GTK 3", "GTK 4", "xfwm4"]);
    }

    #[test]
    fn note_tells_honestly_what_applies_here() {
        assert_eq!(part_note("gtk", "cinnamon"), None, "no note needed in Cinnamon");
        assert_eq!(part_note("cinnamontheme", "mate"), None);
        let apps = part_note("gtk", "kde").unwrap();
        let apps_borders = part_note("gtk", "gnome").unwrap();
        let borders = part_note("marco", "mate").unwrap();
        assert_ne!(apps, apps_borders);
        assert_ne!(apps, borders);
        assert_eq!(part_note("gtk", "budgie").as_deref(), Some(apps_borders.as_str()), "Budgie: borders follow the GTK theme");
        assert_eq!(part_note("gtk", "xfce").as_deref(), Some(apps.as_str()), "XFCE has its own borders section");
        assert_eq!(part_note("xfwm4", "xfce").as_deref(), Some(borders.as_str()));
        let shell = part_note("gnomeshell", "gnome").unwrap();
        assert!(shell != borders && shell != apps_borders);
    }

    #[test]
    fn section_card_goes_to_its_section_and_says_what_applies() {
        let p = parts();
        let s = spice("CBlack", 1_700_000_000);
        for (section, id) in [("gtk", 135), ("xfwm4", 906), ("marco", 907)] {
            let it = section_theme_item(&s, "ru_RU", section, "mate", &p["CBlack"]);
            assert_eq!(it.category_id, id, "{section}");
            assert_eq!(it.id, "spices:theme:CBlack", "{section}: the same journal entry and the same removal");
            assert_eq!(part_section_of(&it), Some(section));
            let note = part_note(section, "mate").unwrap();
            assert!(it.description.starts_with(&note), "{}", it.description);
            assert!(it.description.contains("GTK 3, Metacity/Marco") && it.description.ends_with("\n\nDesc"), "{}", it.description);
        }
        // In Cinnamon there is no caveat, the contents stay.
        let it = section_theme_item(&s, "ru_RU", "gtk", "cinnamon", &p["CBlack"]);
        assert!(it.description.contains("Cinnamon, GTK 2") && !it.description.contains(&part_note("gtk", "kde").unwrap()));
        // The Cinnamon section card does not go by contents.
        let t = type_for_key("theme").unwrap();
        assert_eq!(part_section_of(&theme_item(t, &s, "ru_RU")), None);
        assert_eq!(part_section_of(&theme_item(type_for_key("applet").unwrap(), &spice("a@b", 1), "ru_RU")), None);
    }

    #[test]
    fn buttons_offer_apply_only_where_the_card_applies() {
        use crate::appearance_page::{card_install_button, install_button};
        assert_eq!(card_button_for(true, false, true).text, card_install_button(true, false, true).text, "installed — Apply");
        assert_eq!(card_button_for(true, false, false).text, card_button(true, false).text);
        assert_eq!(card_button_for(true, true, true).text, card_button(true, true).text, "newer on the site — Update");
        assert_eq!(card_button_for(false, false, true).text, card_install_button(false, false, false).text);
        assert_eq!(detail_button_for(true, false, true).text, install_button(true, false, true).text);
        assert_eq!(detail_button_for(true, true, true).text, detail_button(true, true).text);
    }

    /// Recorded responses in order.
    struct Replay(Vec<Result<CondReply, NetError>>, Vec<(String, String)>);

    impl ConditionalNet for Replay {
        fn conditional_get(&mut self, url: &str, _ua: &str, etag: &str, _lm: &str, _t: u64) -> Result<CondReply, NetError> {
            self.1.push((url.into(), etag.into()));
            assert!(!self.0.is_empty(), "an extra request");
            self.0.remove(0)
        }
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-spices-parts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("theme-parts.json")
    }

    fn body200(etag: &str) -> Result<CondReply, NetError> {
        Ok(CondReply::Body { body: tree_body().into_bytes(), etag: etag.into(), last_modified: String::new() })
    }

    const T0: i64 = 1_792_000_000;

    #[test]
    fn parts_cache_day_304_failure_and_garbage() {
        assert_eq!(parts_cache_path(None, "/home/u"), "/home/u/.cache/rexon/cinnamon-spices/theme-parts.json");
        let p = tmp("flow");
        let mut net = Replay(vec![body200("W/\"a\"")], Vec::new());
        let r = refresh_parts(&mut net, &p, false, T0);
        assert_eq!((r.source, r.parts.len(), r.note.as_str()), (RefreshSource::Downloaded, 5, ""));
        assert_eq!(net.1, [(THEME_TREE_URL.to_string(), String::new())]);
        let saved = std::fs::read_to_string(&p).unwrap();
        assert!(saved.len() < 20_000 && !saved.contains("\"tree\""), "the cache holds the contents, not the tree");
        // A day — without network.
        let mut quiet = Replay(Vec::new(), Vec::new());
        assert_eq!(refresh_parts(&mut quiet, &p, false, T0 + 3600).source, RefreshSource::Cache);
        // A day passed — a conditional request with ETag, 304 extends.
        let mut net = Replay(vec![Ok(CondReply::NotModified)], Vec::new());
        let r = refresh_parts(&mut net, &p, false, T0 + CACHE_TTL_SECS + 1);
        assert_eq!((r.source, r.parts.len()), (RefreshSource::NotModified, 5));
        assert_eq!(net.1[0].1, "W/\"a\"");
        // "Refresh": the answer is garbage (a rate limit page) and a truncated tree —
        // the previous contents stay, with a mark; no immediate retry.
        let cut = tree_body().replacen("\"truncated\": false", "\"truncated\": true", 1).into_bytes();
        for answer in [
            Ok(CondReply::Body { body: b"{\"message\":\"API rate limit exceeded\"}".to_vec(), etag: String::new(), last_modified: String::new() }),
            Ok(CondReply::Body { body: cut, etag: String::new(), last_modified: String::new() }),
            Err(NetError::Status { code: 403, reason: "rate limit exceeded".into() }),
        ] {
            let mut net = Replay(vec![answer], Vec::new());
            let r = refresh_parts(&mut net, &p, true, T0 + 2 * CACHE_TTL_SECS);
            assert_eq!((r.source, r.parts.len()), (RefreshSource::Stale, 5));
            assert_eq!(r.note, parts_offline_message(true));
        }
        let mut quiet = Replay(Vec::new(), Vec::new());
        let r = refresh_parts(&mut quiet, &p, false, T0 + 2 * CACHE_TTL_SECS + 60);
        assert_eq!((r.source, r.parts.len()), (RefreshSource::Stale, 5), "after a failure do not knock right away");
        // No cache at all and no network — empty and an explanation.
        let q = tmp("offline");
        let mut net = Replay(vec![Err(NetError::Transport { detail: "dns".into() })], Vec::new());
        let r = refresh_parts(&mut net, &q, false, T0);
        assert!(r.parts.is_empty() && r.note == parts_offline_message(false));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        let _ = std::fs::remove_dir_all(q.parent().unwrap());
    }

    /// A temporary HOME.
    struct Home(std::path::PathBuf, SpiceEnv);

    impl Home {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("rexon-spices-partinst-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).unwrap();
            let env = SpiceEnv::at_home(&root.join("home").to_string_lossy());
            Home(root, env)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct NoGsettings;

    impl SpiceSys for NoGsettings {
        fn list_get(&mut self, _: &str, _: &str) -> Option<Vec<String>> {
            Some(Vec::new())
        }
        fn list_set(&mut self, _: &str, _: &str, _: &[String]) -> Result<(), String> {
            panic!("a list theme does not touch gsettings")
        }
        fn string_get(&mut self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn reset(&mut self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn reload_xlet(&mut self, _: &str, _: &str) -> Result<(), String> {
            panic!("a theme is not an xlet")
        }
    }

    /// A theme archive: `<key>/` with Cinnamon and the listed files.
    fn theme_zip(dest: &Path, key: &str, extra: &[&str]) {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for f in ["cinnamon/cinnamon.css"].iter().chain(extra) {
            zw.start_file(format!("{key}/{f}"), opts).unwrap();
            zw.write_all(b"/* */").unwrap();
        }
        zw.finish().unwrap();
    }

    fn install_in(h: &Home, n: u32, key: &str, extra: &[&str], section: Option<&str>) -> (bool, String) {
        let work = h.0.join(format!("work{n}"));
        std::fs::create_dir_all(&work).unwrap();
        let zip = work.join("a.zip");
        theme_zip(&zip, key, extra);
        let t = type_for_key("theme").unwrap();
        let mut journal = |_: &SpiceStep, _: &[String]| Ok::<(), String>(());
        install_archive_in(&h.1, t, &spice(key, 1_700_000_000), key, &zip, &work, None, section, &mut NoGsettings, &mut journal, &mut |_| {})
    }

    #[test]
    fn install_from_gtk_or_borders_checks_the_archive_itself() {
        let h = Home::new("inst");
        let themes = format!("{}/.themes", h.1.home);
        // A theme with GTK 3 from "GTK themes" — installed into ~/.themes whole.
        let (ok, msg) = install_in(&h, 0, "Winter", &["gtk-3.0/gtk.css", "index.theme"], Some("gtk"));
        assert!(ok, "{msg}");
        assert!(Path::new(&format!("{themes}/Winter/gtk-3.0/gtk.css")).is_file());
        assert!(Path::new(&format!("{themes}/Winter/cinnamon/cinnamon.css")).is_file(), "the theme is installed whole");
        // The archive diverged from the tree: no GTK — a refusal, nothing on disk.
        let (ok, msg) = install_in(&h, 1, "OnlyShell", &["metacity-1/metacity-theme-3.xml"], Some("gtk"));
        assert!(!ok && msg == part_missing_message("OnlyShell", "gtk"), "{msg}");
        assert!(!Path::new(&format!("{themes}/OnlyShell")).exists());
        // The same theme installs in "Window frames (MATE)", but not in XFCE.
        assert!(install_in(&h, 2, "OnlyShell", &["metacity-1/metacity-theme-3.xml"], Some("marco")).0);
        let (ok, msg) = install_in(&h, 3, "Borderless", &["metacity-1/thumbnail.png", "gtk-2.0/gtkrc"], Some("xfwm4"));
        assert!(!ok && msg == part_missing_message("Borderless", "xfwm4"), "{msg}");
        // From the Cinnamon section the contents are not checked — as before.
        assert!(install_in(&h, 4, "Borderless", &[], None).0);
    }

    /// A live request to GitHub with a compressed response (network; run by hand:
    /// `--ignored live_tree_from_github`). The second request is conditional, 304.
    #[test]
    #[ignore]
    fn live_tree_from_github() {
        let p = tmp("live");
        let mut net = UreqConditional::gzip_up_to(MAX_TREE_BYTES);
        let r = refresh_parts(&mut net, &p, true, T0);
        assert_eq!(r.source, RefreshSource::Downloaded, "{}", r.note);
        let gtk = r.parts.values().filter(|v| fits_section("gtk", &has_in(v))).count();
        let marco = r.parts.values().filter(|v| fits_section("marco", &has_in(v))).count();
        let xfwm = r.parts.values().filter(|v| fits_section("xfwm4", &has_in(v))).count();
        eprintln!("themes {}, GTK {gtk}, Marco {marco}, xfwm4 {xfwm}", r.parts.len());
        assert!(r.parts.len() > 50 && gtk > 20 && marco > 20 && xfwm > 5);
        let again = refresh_parts(&mut net, &p, true, T0 + 10);
        assert_eq!(again.source, RefreshSource::NotModified);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn press_applies_installed_theme_and_updates_stale_one() {
        let h = Home::new("press");
        let s = spice("Winter", 1_700_000_000);
        let card = section_theme_item(&s, "ru_RU", "gtk", "kde", &["gtk-3.0".into(), "gtk-3.0/gtk.css".into()]);
        assert_eq!(part_apply_name(&h.1, &card, &[s.clone()]), None, "not installed — install");
        assert!(install_in(&h, 0, "Winter", &["gtk-3.0/gtk.css"], Some("gtk")).0);
        assert_eq!(part_apply_name(&h.1, &card, &[s.clone()]).as_deref(), Some("Winter"), "installed — apply");
        let newer = Spice { last_edited: s.last_edited + 10, ..s.clone() };
        assert_eq!(part_apply_name(&h.1, &card, &[newer]), None, "newer on the site — update");
        // The Cinnamon section card is not applied this way: there the installed tile
        // enables it.
        let cin = theme_item(type_for_key("theme").unwrap(), &s, "ru_RU");
        assert_eq!(part_apply_name(&h.1, &cin, &[s]), None);
    }
}
