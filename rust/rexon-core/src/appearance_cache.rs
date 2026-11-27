//! Appearance catalog cache — port of the PURE logic from `rexon/appearance/cache.py`.
//!
//! The original is a thread-safe SQLite cache `~/.cache/rexon/ocs_cache.db`
//! (tables `items`/`previews`/`lists`/`translations`). SQLite access itself
//! (opening the DB, `execute`, cursors, `commit`), `os.makedirs`, `os.path.isfile`
//! and `time.time()` are SIDE EFFECTS, they STAY in Python: the core is without Qt and without
//! `rusqlite`. What is ported here is the deterministic part (input→output):
//!   * SQL query texts — the constants [`CREATE_SCHEMA`], [`INSERT_ITEM`] and others
//!     (byte for byte the same strings that go to `sqlite3`);
//!   * the TTL constants [`LIST_TTL`]/[`TRANSLATION_TTL`] and the staleness predicates
//!     [`is_list_stale`]/[`is_translation_stale`] + the purge threshold
//!     [`translation_purge_threshold`];
//!   * serializing the `preview_urls` field (a JSON array, like `json.dumps`) —
//!     [`serialize_preview_urls`] — and parsing it [`parse_preview_urls`];
//!   * the CSV field `lists.ids` — [`join_ids`]/[`split_ids`];
//!   * the order and types of the `INSERT OR REPLACE INTO items` values
//!     ([`item_insert_values`]) and the parameters of the other UPSERTs;
//!   * normalizing an `items` table row into a [`ThemeItem`] ([`row_to_item`],
//!     port of `_row_to_item`).
//!
//! Observations (the current time `now`, column values from the DB) come in as
//! parameters. The check against Python is the golden `tests/appearance_cache_parity.rs`
//! (the reference was taken from the LIVE `cache.py` read-only).

use crate::models::ThemeItem;

// ── cache location on disk ─────────────────────────────────────────────────

/// The "Appearance" cache directory: the store catalog database, downloaded pictures,
/// rendered tiles of local themes.
///
/// For the live user's home directory (`$HOME`) this is
/// `$XDG_CACHE_HOME/rexon` if the variable is set, otherwise `~/.cache/rexon`.
/// For any other `home` (checks, test benches with a copy of the cache) it is always
/// `<home>/.cache/rexon`: the environment variable describes the cache of whoever
/// started the program, not of a substituted directory.
///
/// Until 13.10.2026 the variable was not read at all: launching the window with its own
/// `XDG_CACHE_HOME` (a test bench, a sandbox) read and WROTE the cache in the machine
/// owner's `~/.cache/rexon` — the store database, pictures and rendered tiles.
pub fn cache_dir(home: &str) -> String {
    let live_home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CACHE_HOME").unwrap_or_default();
    cache_dir_in(xdg_cache_for(home, &live_home, &xdg), home)
}

/// [`cache_dir`] without reading the environment: the cache root is `xdg_cache_home`, and without
/// it `<home>/.cache`.
pub fn cache_dir_in(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/{}", crate::utils::cache_home(xdg_cache_home, home), crate::utils::REXON_DIR_NAME)
}

/// Whether `$XDG_CACHE_HOME` (`xdg`) applies to the directory `home`: only when
/// it is the live user's home directory (`live_home`) and the variable
/// is set as a full path (the XDG specification says not to accept
/// a relative path).
pub fn xdg_cache_for<'x>(home: &str, live_home: &str, xdg: &'x str) -> Option<&'x str> {
    let same = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
    (!live_home.is_empty() && same(home, live_home) && xdg.starts_with('/')).then_some(xdg)
}

/// The catalog SQLite database file.
pub fn db_path(home: &str) -> String {
    format!("{}/ocs_cache.db", cache_dir(home))
}

/// The directory of downloaded previews.
pub fn preview_dir(home: &str) -> String {
    format!("{}/ocs_previews", cache_dir(home))
}

// ── TTL constants and invalidation predicates ───────────────────────────────

/// Shelf life of the search list cache — 1 hour (`LIST_TTL = 3600`).
pub const LIST_TTL: f64 = 3600.0;

/// Shelf life of a cached translation — 30 days (`TRANSLATION_TTL`).
pub const TRANSLATION_TTL: f64 = 30.0 * 24.0 * 3600.0;

/// Whether a list cache is stale — port of `time.time() - row["cached_at"] > LIST_TTL`
/// from `get_list`. A strict `>`: exactly at the boundary the record is still fresh.
pub fn is_list_stale(cached_at: f64, now: f64) -> bool {
    (now - cached_at) > LIST_TTL
}

/// Whether full details are stale — port of `not at or (time.time() - at) > max_age`
/// from `get_item_detail`. No mark (`NULL` or `0`) — the records came from
/// a list response, they do not count as full details.
pub fn is_detail_stale(detail_at: Option<f64>, now: f64, max_age: f64) -> bool {
    match detail_at {
        Some(at) if at != 0.0 => (now - at) > max_age,
        _ => true,
    }
}

/// A record's detail response mark.
pub const SELECT_DETAIL_AT: &str = "SELECT detail_at FROM items WHERE id=?";

/// Whether a cached translation is stale — port of
/// `time.time() - (row["cached_at"] or 0) > TRANSLATION_TTL` from
/// `get_translation`. `cached_at = None` (NULL in the DB) → treated as `0`
/// (the Python idiom `... or 0`).
pub fn is_translation_stale(cached_at: Option<f64>, now: f64) -> bool {
    (now - cached_at.unwrap_or(0.0)) > TRANSLATION_TTL
}

/// The threshold for `purge_stale_translations`: records with
/// `cached_at < threshold` are deleted, where `threshold = time.time() - TRANSLATION_TTL`.
pub fn translation_purge_threshold(now: f64) -> f64 {
    now - TRANSLATION_TTL
}

// ── SQL texts (byte for byte as in cache.py) ───────────────────────────────

/// DDL creating the tables (`_init_schema` → `executescript`).
pub const CREATE_SCHEMA: &str = concat!(
    "\n",
    "                CREATE TABLE IF NOT EXISTS items (\n",
    "                    id TEXT PRIMARY KEY,\n",
    "                    category_id INTEGER,\n",
    "                    name TEXT, author TEXT,\n",
    "                    description TEXT, description_ru TEXT,\n",
    "                    rating INTEGER, downloads INTEGER,\n",
    "                    preview_url TEXT, download_url TEXT,\n",
    "                    download_name TEXT, download_size INTEGER,\n",
    "                    version TEXT, license TEXT,\n",
    "                    created TEXT, updated TEXT,\n",
    "                    cached_at REAL\n",
    "                );\n",
    "                CREATE TABLE IF NOT EXISTS previews (\n",
    "                    url TEXT PRIMARY KEY,\n",
    "                    local_path TEXT,\n",
    "                    cached_at REAL\n",
    "                );\n",
    "                CREATE TABLE IF NOT EXISTS lists (\n",
    "                    key TEXT PRIMARY KEY,\n",
    "                    ids TEXT,\n",
    "                    total INTEGER,\n",
    "                    cached_at REAL\n",
    "                );\n",
    "                CREATE TABLE IF NOT EXISTS translations (\n",
    "                    src_hash TEXT PRIMARY KEY,\n",
    "                    translated TEXT,\n",
    "                    cached_at REAL\n",
    "                );\n",
    "                ",
);

/// Migration: add the `preview_urls` column (gallery) if it is missing.
pub const ADD_PREVIEW_URLS_COLUMN: &str = "ALTER TABLE items ADD COLUMN preview_urls TEXT";

/// Checking which columns the `items` table has (for the migration).
pub const PRAGMA_TABLE_INFO_ITEMS: &str = "PRAGMA table_info(items)";

/// UPSERT of one item (`_insert_item`) — 18 placeholders.
pub const INSERT_ITEM: &str = concat!(
    "INSERT OR REPLACE INTO items\n",
    "               (id, category_id, name, author, description, description_ru,\n",
    "                rating, downloads, preview_url, preview_urls, download_url,\n",
    "                download_name, download_size, version, license, created,\n",
    "                updated, cached_at)\n",
    "               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
);

/// Select one item by `id` (`get_item`/`get_list`).
pub const SELECT_ITEM_BY_ID: &str = "SELECT * FROM items WHERE id=?";

/// A light selection of `(id, name, preview_url)` of all items (`all_items_min`).
pub const SELECT_ITEMS_MIN: &str = "SELECT id, name, preview_url FROM items";

/// Select a list cache by key (`get_list`).
pub const SELECT_LIST: &str = "SELECT ids, total, cached_at FROM lists WHERE key=?";

/// UPSERT of a list cache (`put_list`).
pub const INSERT_LIST: &str =
    "INSERT OR REPLACE INTO lists (key, ids, total, cached_at) VALUES (?,?,?,?)";

/// Drop the whole list cache (`invalidate_lists`).
pub const DELETE_LISTS: &str = "DELETE FROM lists";

/// Select the local preview path by URL (`get_preview`).
pub const SELECT_PREVIEW: &str = "SELECT local_path FROM previews WHERE url=?";

/// Select all `(url, local_path)` previews (`all_previews_min`).
pub const SELECT_PREVIEWS_MIN: &str = "SELECT url, local_path FROM previews";

/// UPSERT of a preview (`put_preview`).
pub const INSERT_PREVIEW: &str =
    "INSERT OR REPLACE INTO previews (url, local_path, cached_at) VALUES (?,?,?)";

/// Select a translation by `src_hash` (`get_translation`).
pub const SELECT_TRANSLATION: &str =
    "SELECT translated, cached_at FROM translations WHERE src_hash=?";

/// Delete one stale translation record (`get_translation`, lazy purge).
pub const DELETE_TRANSLATION: &str = "DELETE FROM translations WHERE src_hash=?";

/// UPSERT of a translation (`put_translation`).
pub const INSERT_TRANSLATION: &str =
    "INSERT OR REPLACE INTO translations (src_hash, translated, cached_at) VALUES (?,?,?)";

/// Delete all stale translations (`purge_stale_translations`).
pub const DELETE_STALE_TRANSLATIONS: &str = "DELETE FROM translations WHERE cached_at < ?";

// ── serializing/parsing the preview_urls field (a JSON array) ──────────────

/// Serialize `preview_urls` as `json.dumps(list or [])` in `_insert_item`:
/// `ensure_ascii=True` (non-ASCII → `\uXXXX`, astral → a surrogate pair),
/// the element separator `", "`. An empty list → `"[]"`.
pub fn serialize_preview_urls(urls: &[String]) -> String {
    if urls.is_empty() {
        return "[]".to_string();
    }
    let mut out = String::from("[");
    for (i, u) in urls.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        py_json_string(u, &mut out);
    }
    out.push(']');
    out
}

/// Encode a string as a Python `json.dumps` string literal
/// (`ensure_ascii=True`) into `out`, including the quotes.
fn py_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            // Printable ASCII (0x20..=0x7e) — as is.
            c if ('\u{20}'..='\u{7e}').contains(&c) => out.push(c),
            // Everything else (control <0x20, 0x7f and any non-ASCII) → \uXXXX,
            // beyond the BMP — a surrogate pair, as in CPython.
            c => {
                let cp = c as u32;
                if cp <= 0xFFFF {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    let v = cp - 0x1_0000;
                    let hi = 0xD800 + (v >> 10);
                    let lo = 0xDC00 + (v & 0x3FF);
                    out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// Parse the stored `preview_urls` — port of the expression from `_row_to_item`:
/// `json.loads(raw or "[]")`; a non-list or a parse error → `[]`; then
/// `[str(u) for u in previews if u]` (falsy elements are filtered out, the rest
/// are converted to a string like Python `str`). `raw = None`/`""` → `"[]"`.
pub fn parse_preview_urls(raw: Option<&str>) -> Vec<String> {
    let text = match raw {
        Some(t) if !t.is_empty() => t,
        _ => "[]",
    };
    let parsed: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(arr) = parsed.as_array() else {
        return Vec::new();
    };
    arr.iter().filter(|v| json_truthy(v)).map(python_str).collect()
}

/// Python truthiness of a JSON value (`if u`): null/false/0/""/[]/{} → false.
fn json_truthy(v: &serde_json::Value) -> bool {
    use serde_json::Value;
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The equivalent of Python `str(u)` for a JSON value (in `preview_urls` they are practically
/// always strings; numbers/booleans are supported for exact filtering parity).
fn python_str(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Number(n) => n.to_string(),
        // Nested containers do not occur in preview_urls; we return the JSON form.
        other => other.to_string(),
    }
}

// ── the CSV field lists.ids ────────────────────────────────────────────────

/// Build `lists.ids` — port of `",".join(it.id for it in items)` (`put_list`).
pub fn join_ids(ids: &[String]) -> String {
    ids.join(",")
}

/// Parse `lists.ids` — port of `[i for i in (row["ids"] or "").split(",")
/// if i]` (`get_list`): empty segments are dropped, `None`/`""` → `[]`.
pub fn split_ids(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// ── UPSERT values (order + placeholder types) ──────────────────────────────

/// A query parameter value — as sqlite3 binds a Python object: `TEXT`,
/// `INTEGER`, `REAL` or `NULL`. Documents the order/types of placeholders;
/// the binding itself is a side effect in Python.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

/// The 18 values of `INSERT OR REPLACE INTO items` in column order (`_insert_item`).
/// `now` = `time.time()` (an observation, comes as a parameter) → the `cached_at` column.
/// `description_ru` is taken from `description_translated`, `preview_urls`
/// is serialized by [`serialize_preview_urls`].
pub fn item_insert_values(item: &ThemeItem, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(item.id.clone()),
        CacheValue::Int(item.category_id),
        CacheValue::Text(item.name.clone()),
        CacheValue::Text(item.author.clone()),
        CacheValue::Text(item.description.clone()),
        CacheValue::Text(item.description_translated.clone()),
        CacheValue::Int(item.rating),
        CacheValue::Int(item.downloads),
        CacheValue::Text(item.preview_url.clone()),
        CacheValue::Text(serialize_preview_urls(&item.preview_urls)),
        CacheValue::Text(item.download_url.clone()),
        CacheValue::Text(item.download_name.clone()),
        CacheValue::Int(item.download_size),
        CacheValue::Text(item.version.clone()),
        CacheValue::Text(item.license.clone()),
        CacheValue::Text(item.created.clone()),
        CacheValue::Text(item.updated.clone()),
        CacheValue::Real(now),
    ]
}

/// Parameters of `INSERT OR REPLACE INTO lists` (`put_list`): key, ids, total, now.
pub fn list_insert_values(key: &str, ids: &str, total: i64, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(key.to_string()),
        CacheValue::Text(ids.to_string()),
        CacheValue::Int(total),
        CacheValue::Real(now),
    ]
}

/// Parameters of `INSERT OR REPLACE INTO previews` (`put_preview`).
pub fn preview_insert_values(url: &str, local_path: &str, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(url.to_string()),
        CacheValue::Text(local_path.to_string()),
        CacheValue::Real(now),
    ]
}

/// Parameters of `INSERT OR REPLACE INTO translations` (`put_translation`).
pub fn translation_insert_values(src_hash: &str, translated: &str, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(src_hash.to_string()),
        CacheValue::Text(translated.to_string()),
        CacheValue::Real(now),
    ]
}

// ── normalizing an items table row into ThemeItem ──────────────────────────

/// Column values of an `items` table row (observations from the DB). `Option::None`
/// = SQL NULL; Python's `sqlite3.Row` returns them as `None`, and `_row_to_item`
/// normalizes them with the `... or <default>` idiom.
#[derive(Debug, Clone, Default)]
pub struct ItemRow {
    /// PRIMARY KEY — never NULL, hence not an `Option`.
    pub id: String,
    pub category_id: Option<i64>,
    pub name: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub description_ru: Option<String>,
    pub rating: Option<i64>,
    pub downloads: Option<i64>,
    pub preview_url: Option<String>,
    pub preview_urls: Option<String>,
    pub download_url: Option<String>,
    pub download_name: Option<String>,
    pub download_size: Option<i64>,
    pub version: Option<String>,
    pub license: Option<String>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

/// Build a [`ThemeItem`] from an `items` table row — port of `_row_to_item`.
/// NULL fields → defaults (`or ""`/`or 0`), `preview_urls` → [`parse_preview_urls`],
/// `description_ru` → `description_translated`. Other `ThemeItem` fields
/// (`preview_local`, `installed`, `install_path`, `download_links`) — defaults.
pub fn row_to_item(row: &ItemRow) -> ThemeItem {
    ThemeItem {
        id: row.id.clone(),
        category_id: row.category_id.unwrap_or(0),
        name: row.name.clone().unwrap_or_default(),
        author: row.author.clone().unwrap_or_default(),
        description: row.description.clone().unwrap_or_default(),
        description_translated: row.description_ru.clone().unwrap_or_default(),
        rating: row.rating.unwrap_or(0),
        downloads: row.downloads.unwrap_or(0),
        preview_url: row.preview_url.clone().unwrap_or_default(),
        preview_urls: parse_preview_urls(row.preview_urls.as_deref()),
        download_url: row.download_url.clone().unwrap_or_default(),
        download_name: row.download_name.clone().unwrap_or_default(),
        download_size: row.download_size.unwrap_or(0),
        version: row.version.clone().unwrap_or_default(),
        license: row.license.clone().unwrap_or_default(),
        created: row.created.clone().unwrap_or_default(),
        updated: row.updated.clone().unwrap_or_default(),
        ..ThemeItem::default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  The LIVE catalog cache: the same database as Python's
// ═══════════════════════════════════════════════════════════════════════════
//
// WHY IT IS HERE. Everything above is rules without side effects, and until 08.09.2026
// they had no executor at all: SQLite was opened by `rexon/appearance/cache.py`,
// i.e. the theme catalog worked ONLY in a process with Python. Because of that the
// "Appearance" tab under the Rust window host opened empty: the store client
// (`crate::ocs::OcsClient`) had not a single live implementation of [`crate::ocs::OcsCacheIo`].
//
// The live cache sits next to the rules, not in the Qt layer, exactly per rule #12:
// SQL texts, TTLs and row parsing are here, and there must be no second copy of them.
// `rusqlite` is already in the core tree (pulled in by `steam_token`), so this adds
// no new dependencies.
//
// ⚠️ THE DATABASE IS SHARED WITH PYTHON, and that is a condition, not a coincidence: both tabs
// (the PySide6 widget one and the QML one) go to `~/.cache/rexon/ocs_cache.db`. So
// the schema here is created with THE SAME strings and migrated with the same two
// columns; if they diverged, Python would stop reading what we wrote.

/// Migration: the `detail_at` column (when the record came from a DETAIL
/// response). The pair of [`ADD_PREVIEW_URLS_COLUMN`] — the live database has both.
pub const ADD_DETAIL_AT_COLUMN: &str = "ALTER TABLE items ADD COLUMN detail_at REAL";

/// UPSERT of an item that PRESERVES `detail_at` (port of `_insert_item`).
///
/// Separate from [`INSERT_ITEM`], and that is not decoration: `INSERT OR REPLACE`
/// writes the whole row, so a LIST response would erase the "full
/// details were there" mark, and installation would get an item without file links.
/// The 19th parameter is the same `id` (for the subquery).
pub const INSERT_ITEM_KEEP_DETAIL: &str = concat!(
    "INSERT OR REPLACE INTO items\n",
    "               (id, category_id, name, author, description, description_ru,\n",
    "                rating, downloads, preview_url, preview_urls, download_url,\n",
    "                download_name, download_size, version, license, created,\n",
    "                updated, cached_at, detail_at)\n",
    "               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,\n",
    "                       (SELECT detail_at FROM items WHERE id=?))",
);

/// Mark a record as coming from a detail response (port of `put_item`).
pub const UPDATE_DETAIL_AT: &str = "UPDATE items SET detail_at=? WHERE id=?";

/// Epoch seconds as a fractional number — the original's `time.time()`.
pub fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// The live SQLite cache of the appearance catalog — the executor of this module's rules.
///
/// It is opened anew for EVERY job (like `OCSCache()` in the Python worker):
/// a sqlite connection does not cross threads, and the tab's jobs each run in
/// their own. If the database did not open (no rights, the disk is full), the cache is simply empty: this is
/// a slowdown, not a failure, and the tab must keep working.
pub struct LiveCache {
    conn: Option<rusqlite::Connection>,
    preview_dir: String,
}

impl LiveCache {
    /// Open the database and bring the schema up to date (port of `__init__`).
    pub fn open(home: &str) -> Self {
        let dir = cache_dir(home);
        let previews = preview_dir(home);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::create_dir_all(&previews);
        let conn = rusqlite::Connection::open(db_path(home)).ok();
        let mut cache = LiveCache { conn, preview_dir: previews };
        cache.init_schema();
        cache
    }

    /// The downloaded previews directory (the same one [`crate::ocs::OcsClient::new`] expects).
    pub fn preview_dir(&self) -> &str {
        &self.preview_dir
    }

    fn init_schema(&mut self) {
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        if conn.execute_batch(CREATE_SCHEMA).is_err() {
            // Could not create the schema — this database cannot be used further:
            // silent errors on every query only burn time.
            self.conn = None;
            return;
        }
        let mut columns: Vec<String> = Vec::new();
        if let Ok(mut stmt) = conn.prepare(PRAGMA_TABLE_INFO_ITEMS) {
            if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(1)) {
                columns = rows.filter_map(Result::ok).collect();
            }
        }
        if !columns.iter().any(|c| c == "preview_urls") {
            let _ = conn.execute(ADD_PREVIEW_URLS_COLUMN, []);
        }
        if !columns.iter().any(|c| c == "detail_at") {
            let _ = conn.execute(ADD_DETAIL_AT_COLUMN, []);
        }
    }

    /// An `items` table row → [`ItemRow`]. Column names, not numbers:
    /// `SELECT *` returns them in schema order, and migrations changed it.
    fn read_item_row(row: &rusqlite::Row<'_>) -> ItemRow {
        ItemRow {
            id: row.get("id").unwrap_or_default(),
            category_id: row.get("category_id").ok(),
            name: row.get("name").ok(),
            author: row.get("author").ok(),
            description: row.get("description").ok(),
            description_ru: row.get("description_ru").ok(),
            rating: row.get("rating").ok(),
            downloads: row.get("downloads").ok(),
            preview_url: row.get("preview_url").ok(),
            preview_urls: row.get("preview_urls").ok(),
            download_url: row.get("download_url").ok(),
            download_name: row.get("download_name").ok(),
            download_size: row.get("download_size").ok(),
            version: row.get("version").ok(),
            license: row.get("license").ok(),
            created: row.get("created").ok(),
            updated: row.get("updated").ok(),
        }
    }

    /// One item by number — port of `get_item`.
    pub fn get_item(&mut self, content_id: &str) -> Option<ThemeItem> {
        let conn = self.conn.as_ref()?;
        let mut stmt = conn.prepare(SELECT_ITEM_BY_ID).ok()?;
        stmt.query_row([content_id], |r| Ok(row_to_item(&Self::read_item_row(r))))
            .ok()
    }

    /// Fresh FULL details or `None` (then — to the network) — port of
    /// `get_item_detail`. Freshness — by `detail_at` ([`is_detail_stale`]).
    pub fn get_item_detail(&mut self, content_id: &str, max_age: f64) -> Option<ThemeItem> {
        let at = {
            let conn = self.conn.as_ref()?;
            conn.query_row(SELECT_DETAIL_AT, [content_id], |r| r.get::<_, Option<f64>>(0))
                .ok()?
        };
        if is_detail_stale(at, now_epoch(), max_age) {
            return None;
        }
        self.get_item(content_id)
    }

    /// Store an item, marking it as coming from a detail response —
    /// port of `put_item`.
    pub fn put_item(&mut self, item: &ThemeItem) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        if insert_item_keep_detail(conn, item, now).is_err() {
            return;
        }
        let _ = conn.execute(
            UPDATE_DETAIL_AT,
            rusqlite::params![now, item.id.as_str()],
        );
    }

    /// A search page from the cache — port of `get_list` (stale = miss).
    pub fn get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)> {
        let conn = self.conn.as_ref()?;
        let mut stmt = conn.prepare(SELECT_LIST).ok()?;
        let (ids_raw, total, cached_at) = stmt
            .query_row([key], |r| {
                Ok((
                    r.get::<_, Option<String>>(0).unwrap_or_default(),
                    r.get::<_, Option<i64>>(1).unwrap_or_default().unwrap_or(0),
                    r.get::<_, Option<f64>>(2).unwrap_or_default().unwrap_or(0.0),
                ))
            })
            .ok()?;
        if is_list_stale(cached_at, now_epoch()) {
            return None;
        }
        drop(stmt);
        let ids = split_ids(ids_raw.as_deref());
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(item) = self.get_item(&id) {
                items.push(item);
            }
        }
        Some((items, total))
    }

    /// Remember a search page — port of `put_list` (one commit per page).
    pub fn put_list(&mut self, key: &str, items: &[ThemeItem], total: i64) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let Ok(tx) = conn.transaction() else {
            return;
        };
        for item in items {
            if insert_item_keep_detail(&tx, item, now).is_err() {
                return;
            }
        }
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let vals = list_insert_values(key, &join_ids(&ids), total, now);
        if tx.execute(INSERT_LIST, rusqlite::params_from_iter(bind(&vals))).is_err() {
            return;
        }
        let _ = tx.commit();
    }

    /// Path of a downloaded preview — port of `get_preview`.
    ///
    /// A record whose file vanished from disk is a miss: otherwise the tile would show
    /// an empty space instead of a picture that can be downloaded again.
    pub fn get_preview(&mut self, url: &str) -> Option<String> {
        let conn = self.conn.as_ref()?;
        let mut stmt = conn.prepare(SELECT_PREVIEW).ok()?;
        let path: String = stmt
            .query_row([url], |r| r.get::<_, Option<String>>(0))
            .ok()?
            .unwrap_or_default();
        if !path.is_empty() && std::path::Path::new(&path).is_file() {
            Some(path)
        } else {
            None
        }
    }

    /// Mark a preview as downloaded — port of `put_preview`.
    pub fn put_preview(&mut self, url: &str, local_path: &str) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let vals = preview_insert_values(url, local_path, now);
        let _ = conn.execute(INSERT_PREVIEW, rusqlite::params_from_iter(bind(&vals)));
    }

    /// `(name, preview link)` of all cached themes — port of
    /// `all_items_min`; raw material for backfilling previews of local themes
    /// ([`crate::installed_scan::backfill_preview`]).
    pub fn all_items_min(&mut self) -> Vec<(String, String)> {
        let Some(conn) = self.conn.as_ref() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(SELECT_ITEMS_MIN) else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        });
        match rows {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `(preview link, category number)` of all cached themes: from
    /// them the installed scan knows which section a picture belongs to
    /// (`installed_scan::LiveScan::with_store_records`).
    pub fn all_preview_categories(&mut self) -> Vec<(String, i64)> {
        let Some(conn) = self.conn.as_ref() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare("SELECT preview_url, category_id FROM items") else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(1)?.unwrap_or_default(),
            ))
        });
        match rows {
            Ok(rows) => rows.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// `url → file` of all previews whose file is still on disk — port of
    /// `all_previews_min`.
    pub fn all_previews_min(&mut self) -> Vec<(String, String)> {
        let Some(conn) = self.conn.as_ref() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(SELECT_PREVIEWS_MIN) else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
            ))
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(Result::ok)
            .filter(|(_, path)| !path.is_empty() && std::path::Path::new(path).is_file())
            .collect()
    }

    /// A fresh translation from the cache or `None` — port of `get_translation`
    /// (a stale record is deleted lazily, as in the original).
    pub fn get_translation(&mut self, src_hash: &str) -> Option<String> {
        let now = now_epoch();
        let conn = self.conn.as_ref()?;
        let mut stmt = conn.prepare(SELECT_TRANSLATION).ok()?;
        let (text, cached_at) = stmt
            .query_row([src_hash], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, Option<f64>>(1)?,
                ))
            })
            .ok()?;
        drop(stmt);
        if is_translation_stale(cached_at, now) {
            if let Some(conn) = self.conn.as_mut() {
                let _ = conn.execute(DELETE_TRANSLATION, [src_hash]);
            }
            return None;
        }
        Some(text)
    }

    /// Write a translation to the cache — port of `put_translation`. The `src_hash` key
    /// is computed by [`crate::translator::translation_cache_key`]: the engine version
    /// is already in it, and records of the previous engine will not fall under this key.
    pub fn put_translation(&mut self, src_hash: &str, translated: &str) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let vals = translation_insert_values(src_hash, translated, now);
        let _ = conn.execute(INSERT_TRANSLATION, rusqlite::params_from_iter(bind(&vals)));
    }

    /// A one-off purge of stale translations — port of `purge_stale_translations`.
    /// Returns the number of deleted records.
    pub fn purge_stale_translations(&mut self) -> usize {
        let threshold = translation_purge_threshold(now_epoch());
        let Some(conn) = self.conn.as_mut() else {
            return 0;
        };
        conn.execute(DELETE_STALE_TRANSLATIONS, rusqlite::params![threshold])
            .unwrap_or(0)
    }
}

/// UPSERT of an item preserving `detail_at` (shared by a single insert and
/// a batch under one transaction).
fn insert_item_keep_detail(
    conn: &rusqlite::Connection,
    item: &ThemeItem,
    now: f64,
) -> rusqlite::Result<usize> {
    let mut vals = bind(&item_insert_values(item, now));
    // The subquery's 19th parameter is the same item number.
    vals.push(rusqlite::types::Value::Text(item.id.clone()));
    conn.execute(INSERT_ITEM_KEEP_DETAIL, rusqlite::params_from_iter(vals))
}

/// [`CacheValue`] → a sqlite parameter value. A translator, not a second set of
/// rules: the order and types of parameters are decided by the functions above.
fn bind(values: &[CacheValue]) -> Vec<rusqlite::types::Value> {
    values
        .iter()
        .map(|v| match v {
            CacheValue::Null => rusqlite::types::Value::Null,
            CacheValue::Int(i) => rusqlite::types::Value::Integer(*i),
            CacheValue::Real(f) => rusqlite::types::Value::Real(*f),
            CacheValue::Text(s) => rusqlite::types::Value::Text(s.clone()),
        })
        .collect()
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// The live user's cache follows `XDG_CACHE_HOME`; a substituted home
    /// directory (checks, a test bench with a copy of the cache) does not see the variable.
    #[test]
    fn cache_dir_follows_xdg_only_for_the_live_home() {
        assert_eq!(cache_dir_in(None, "/home/u"), "/home/u/.cache/rexon");
        assert_eq!(cache_dir_in(Some("/stand/cache"), "/home/u"), "/stand/cache/rexon");
        assert_eq!(cache_dir_in(Some(""), "/home/u/"), "/home/u/.cache/rexon");
        assert_eq!(xdg_cache_for("/home/u", "/home/u", "/stand/cache"), Some("/stand/cache"));
        assert_eq!(xdg_cache_for("/home/u/", "/home/u", "/stand/cache"), Some("/stand/cache"));
        assert_eq!(xdg_cache_for("/tmp/fake-home", "/home/u", "/stand/cache"), None);
        assert_eq!(xdg_cache_for("/home/u", "/home/u", ""), None);
        assert_eq!(xdg_cache_for("/home/u", "/home/u", "relative/cache"), None);
        assert_eq!(xdg_cache_for("/home/u", "", "/stand/cache"), None);
        // All "Appearance" cache paths grow from one root.
        let home = "/tmp/rexon-cache-dir-probe";
        let root = cache_dir(home);
        assert_eq!(root, format!("{home}/.cache/rexon"));
        for path in [
            db_path(home),
            preview_dir(home),
            crate::appearance_page::placeholder_dir(home),
            crate::appearance_page::aur_preview_dir(home),
            crate::installed_scan::sidecar_preview_path(home, "colors", "Nord"),
            crate::aur_install::log_path(home, "pkg"),
        ] {
            assert!(path.starts_with(&format!("{root}/")), "{path}");
        }
    }

    /// Its own "home" per check: the live cache writes real files, and it
    /// must not touch the machine's cache.
    struct TempHome(String);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!("rexon-cache-{tag}-{nanos}"));
            std::fs::create_dir_all(&path).expect("временный дом");
            TempHome(path.to_string_lossy().into_owned())
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn item(id: &str, name: &str) -> ThemeItem {
        ThemeItem {
            id: id.to_string(),
            name: name.to_string(),
            preview_url: format!("https://example.invalid/{id}.png"),
            preview_urls: vec![format!("https://example.invalid/{id}-2.png")],
            download_url: format!("https://example.invalid/{id}.tar.gz"),
            category_id: 104,
            ..ThemeItem::default()
        }
    }

    /// A written search page is read back with THE SAME contents. The check
    /// goes the whole way: schema, migrations, item UPSERTs, the CSV field `ids` and
    /// building a `ThemeItem` from a table row.
    #[test]
    fn a_written_page_reads_back_the_same_way() {
        let home = TempHome::new("list");
        let mut cache = LiveCache::open(&home.0);
        let items = vec![item("101", "Первая"), item("202", "Вторая")];
        cache.put_list("ключ", &items, 42);

        // ⚠️ ANOTHER instance reads: the cache is set up per job, and a record
        // visible only to its own connection would be useless.
        let mut second = LiveCache::open(&home.0);
        let (back, total) = second.get_list("ключ").expect("страница обязана найтись");
        assert_eq!(total, 42);
        let names: Vec<String> = back.iter().map(|it| it.name.clone()).collect();
        assert_eq!(names, vec!["Первая".to_string(), "Вторая".to_string()]);
        assert_eq!(back[0].preview_urls, items[0].preview_urls);
        assert_eq!(back[1].category_id, 104);
    }

    /// A stale page is a MISS, not an outdated answer. The staleness measure
    /// is one for both tab versions (`is_list_stale`), and here we check that the
    /// live cache really asks it.
    #[test]
    fn a_stale_page_is_a_miss() {
        let home = TempHome::new("stale");
        let mut cache = LiveCache::open(&home.0);
        cache.put_list("ключ", &[item("1", "Тема")], 1);
        assert!(cache.get_list("ключ").is_some(), "свежая страница обязана найтись");

        // Age the record right in the database: a live check cannot wait an hour.
        let old = now_epoch() - LIST_TTL - 1.0;
        {
            let conn = rusqlite::Connection::open(db_path(&home.0)).expect("база");
            conn.execute("UPDATE lists SET cached_at=?", rusqlite::params![old])
                .expect("состарить");
        }
        let mut second = LiveCache::open(&home.0);
        assert!(
            second.get_list("ключ").is_none(),
            "страница старше часа обязана быть промахом"
        );
    }

    /// The "full details were there" mark (`detail_at`) survives a LIST response.
    ///
    /// ⚠️ This is why a separate `INSERT_ITEM_KEEP_DETAIL` exists: a plain
    /// `INSERT OR REPLACE` writes the whole row, the mark would be erased at
    /// every storefront refresh, and installation would get an item without file
    /// links.
    #[test]
    fn a_list_answer_does_not_erase_the_detail_mark() {
        let home = TempHome::new("detail");
        let mut cache = LiveCache::open(&home.0);
        let full = item("777", "Полная");
        cache.put_item(&full);

        let mark_after_detail = detail_at(&home.0, "777");
        assert!(
            mark_after_detail.is_some(),
            "детальный ответ обязан ставить отметку"
        );

        // The same item arrives again, but now as a storefront page.
        cache.put_list("ключ", &[item("777", "Полная")], 1);
        assert_eq!(
            detail_at(&home.0, "777"),
            mark_after_detail,
            "ответ-список не смеет стирать отметку детального ответа"
        );
    }

    /// A preview record whose file vanished from disk is a miss. Otherwise the tile
    /// would show an empty space instead of a picture that can be downloaded
    /// again.
    #[test]
    fn a_preview_whose_file_is_gone_is_a_miss() {
        let home = TempHome::new("preview");
        let mut cache = LiveCache::open(&home.0);
        let file = format!("{}/картинка.png", preview_dir(&home.0));
        std::fs::write(&file, b"png").expect("файл превью");
        cache.put_preview("https://example.invalid/p.png", &file);
        assert_eq!(
            cache.get_preview("https://example.invalid/p.png").as_deref(),
            Some(file.as_str())
        );

        std::fs::remove_file(&file).expect("убрать файл");
        assert!(
            cache.get_preview("https://example.invalid/p.png").is_none(),
            "запись без файла обязана быть промахом"
        );
    }

    /// The `detail_at` mark of an item row (only for the checks above).
    /// Full details live in the cache for the display period, a record from a list response
    /// does not count as full details, stale ones are a miss.
    #[test]
    fn detail_is_served_from_cache_only_while_fresh() {
        let home = TempHome::new("detail-ttl");
        let mut cache = LiveCache::open(&home.0);
        let ttl = crate::ocs::DETAIL_TTL_SECS as f64;

        // Only from a list — no mark, to the network.
        cache.put_list("ключ", &[item("5", "Из списка")], 1);
        assert!(cache.get_item_detail("5", ttl).is_none());
        // No record at all.
        assert!(cache.get_item_detail("нет", ttl).is_none());

        // A detail response — fresh, served from the cache.
        cache.put_item(&item("6", "Полная"));
        let mut second = LiveCache::open(&home.0);
        let hit = second.get_item_detail("6", ttl).expect("свежие детали");
        assert_eq!(hit.name, "Полная");

        // Age the mark by a day and a second — a miss.
        {
            let conn = rusqlite::Connection::open(db_path(&home.0)).expect("база");
            conn.execute(
                "UPDATE items SET detail_at=? WHERE id='6'",
                rusqlite::params![now_epoch() - ttl - 1.0],
            )
            .expect("состарить");
        }
        let mut third = LiveCache::open(&home.0);
        assert!(third.get_item_detail("6", ttl).is_none(), "старше суток — в сеть");
    }

    /// The full translation cycle: written → read by ANOTHER instance →
    /// aged → the purge took it away. The whole write path is checked
    /// (`put_translation` + `translation_insert_values` + the schema), not just
    /// the staleness predicate.
    #[test]
    fn a_translation_survives_a_round_trip_and_dies_by_ttl() {
        let home = TempHome::new("translation");
        let mut cache = LiveCache::open(&home.0);
        let key = crate::translator::translation_cache_key("en", "ru", "Dark theme");
        cache.put_translation(&key, "Тёмная тема");

        // Another instance reads: a record visible only to its own connection
        // would be useless — a translation outlives the application run.
        let mut second = LiveCache::open(&home.0);
        assert_eq!(
            second.get_translation(&key).as_deref(),
            Some("Тёмная тема"),
            "свежий перевод обязан найтись"
        );
        // There is no foreign key in the cache — otherwise a translation would replace neighboring text.
        assert!(second.get_translation("нет такого").is_none());
        assert_eq!(second.purge_stale_translations(), 0, "свежему переводу чистка не грозит");

        // Age it by a month and a second right in the database: a live check cannot wait 30
        // days.
        {
            let conn = rusqlite::Connection::open(db_path(&home.0)).expect("база");
            conn.execute(
                "UPDATE translations SET cached_at=?",
                rusqlite::params![now_epoch() - TRANSLATION_TTL - 1.0],
            )
            .expect("состарить");
        }
        let mut third = LiveCache::open(&home.0);
        assert!(third.get_translation(&key).is_none(), "перевод старше TTL — промах");
        // The lazy purge on read already removed the row, the one-off one finds
        // nothing: both roads lead to an empty table.
        assert_eq!(third.purge_stale_translations(), 0, "ленивая чистка уже удалила запись");
        assert_eq!(rows_in_translations(&home.0), 0, "протухшая строка не смеет копиться");

        // A record under the same key is written again (UPSERT, not a duplicate).
        third.put_translation(&key, "Тёмное оформление");
        third.put_translation(&key, "Тёмная тема");
        assert_eq!(rows_in_translations(&home.0), 1, "UPSERT, а не второй ряд");
        assert_eq!(third.get_translation(&key).as_deref(), Some("Тёмная тема"));
    }

    /// How many rows the translations table has (only for the check above).
    fn rows_in_translations(home: &str) -> i64 {
        let conn = rusqlite::Connection::open(db_path(home)).expect("база");
        conn.query_row("SELECT COUNT(*) FROM translations", [], |r| r.get(0))
            .expect("счёт строк")
    }

    #[test]
    fn detail_staleness_matches_the_original() {
        let now = 1_000_000.0;
        assert!(is_detail_stale(None, now, 10.0));
        assert!(is_detail_stale(Some(0.0), now, 10.0), "`not at` оригинала");
        assert!(!is_detail_stale(Some(now - 10.0), now, 10.0), "ровно срок — ещё свежо");
        assert!(is_detail_stale(Some(now - 10.5), now, 10.0));
    }

    fn detail_at(home: &str, id: &str) -> Option<f64> {
        let conn = rusqlite::Connection::open(db_path(home)).ok()?;
        conn.query_row("SELECT detail_at FROM items WHERE id=?", [id], |r| {
            r.get::<_, Option<f64>>(0)
        })
        .ok()
        .flatten()
    }
}
