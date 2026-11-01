//! Кеш каталога оформления — порт ЧИСТОЙ логики из `rexon/appearance/cache.py`.
//!
//! Оригинал — потокобезопасный SQLite-кеш `~/.cache/rexon/ocs_cache.db`
//! (таблицы `items`/`previews`/`lists`/`translations`). Сам SQLite-доступ
//! (открытие БД, `execute`, курсоры, `commit`), `os.makedirs`, `os.path.isfile`
//! и `time.time()` — SIDE EFFECT, он ОСТАЁТСЯ на Python: ядро без Qt и без
//! `rusqlite`. Сюда портирована детерминированная часть (вход→выход):
//!   * тексты SQL-запросов — константы [`CREATE_SCHEMA`], [`INSERT_ITEM`] и др.
//!     (байт-в-байт те же строки, что уходят в `sqlite3`);
//!   * TTL-константы [`LIST_TTL`]/[`TRANSLATION_TTL`] и предикаты протухания
//!     [`is_list_stale`]/[`is_translation_stale`] + порог чистки
//!     [`translation_purge_threshold`];
//!   * сериализация поля `preview_urls` (JSON-массив, как `json.dumps`) —
//!     [`serialize_preview_urls`] — и его разбор [`parse_preview_urls`];
//!   * CSV-поле `lists.ids` — [`join_ids`]/[`split_ids`];
//!   * порядок и типы значений `INSERT OR REPLACE INTO items`
//!     ([`item_insert_values`]) и параметры остальных UPSERT'ов;
//!   * нормализация строки таблицы `items` в [`ThemeItem`] ([`row_to_item`],
//!     порт `_row_to_item`).
//!
//! Наблюдения (текущее время `now`, значения колонок из БД) приходят
//! параметрами. Сверка с Python — golden `tests/appearance_cache_parity.rs`
//! (эталон снят с ЖИВОГО `cache.py` read-only).

use crate::models::ThemeItem;

// ── расположение кеша на диске ─────────────────────────────────────────────

/// Каталог кеша «Оформления»: база каталога магазина, скачанные картинки,
/// нарисованные плитки местных тем.
///
/// Для домашнего каталога живого пользователя (`$HOME`) это
/// `$XDG_CACHE_HOME/rexon`, если переменная задана, иначе `~/.cache/rexon`.
/// Для любого другого `home` (сверки, стенды с копией кеша) — всегда
/// `<home>/.cache/rexon`: переменная окружения описывает кеш того, кто
/// запустил программу, а не подставного каталога.
///
/// До 13.10.2026 переменная не читалась вовсе: запуск окна со своим
/// `XDG_CACHE_HOME` (стенд, песочница) читал и ПИСАЛ кеш в `~/.cache/rexon`
/// хозяина машины — базу магазина, картинки и нарисованные плитки.
pub fn cache_dir(home: &str) -> String {
    let live_home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CACHE_HOME").unwrap_or_default();
    cache_dir_in(xdg_cache_for(home, &live_home, &xdg), home)
}

/// [`cache_dir`] без чтения окружения: корень кеша — `xdg_cache_home`, а без
/// него `<home>/.cache`.
pub fn cache_dir_in(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/{}", crate::utils::cache_home(xdg_cache_home, home), crate::utils::REXON_DIR_NAME)
}

/// Относится ли `$XDG_CACHE_HOME` (`xdg`) к каталогу `home`: только когда
/// это домашний каталог живого пользователя (`live_home`) и переменная
/// задана полным путём (относительный путь спецификация XDG велит не
/// принимать).
pub fn xdg_cache_for<'x>(home: &str, live_home: &str, xdg: &'x str) -> Option<&'x str> {
    let same = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
    (!live_home.is_empty() && same(home, live_home) && xdg.starts_with('/')).then_some(xdg)
}

/// Файл SQLite-базы каталога.
pub fn db_path(home: &str) -> String {
    format!("{}/ocs_cache.db", cache_dir(home))
}

/// Каталог скачанных превью.
pub fn preview_dir(home: &str) -> String {
    format!("{}/ocs_previews", cache_dir(home))
}

// ── TTL-константы и предикаты инвалидации ──────────────────────────────────

/// Срок годности кеша списков поиска — 1 час (`LIST_TTL = 3600`).
pub const LIST_TTL: f64 = 3600.0;

/// Срок годности кешированного перевода — 30 дней (`TRANSLATION_TTL`).
pub const TRANSLATION_TTL: f64 = 30.0 * 24.0 * 3600.0;

/// Протух ли кеш списка — порт `time.time() - row["cached_at"] > LIST_TTL`
/// из `get_list`. Строгое `>`: ровно на границе запись ещё свежая.
pub fn is_list_stale(cached_at: f64, now: f64) -> bool {
    (now - cached_at) > LIST_TTL
}

/// Протухли ли полные детали — порт `not at or (time.time() - at) > max_age`
/// из `get_item_detail`. Нет отметки (`NULL` либо `0`) — записи пришли из
/// ответа-списка, полными деталями они не считаются.
pub fn is_detail_stale(detail_at: Option<f64>, now: f64, max_age: f64) -> bool {
    match detail_at {
        Some(at) if at != 0.0 => (now - at) > max_age,
        _ => true,
    }
}

/// Отметка детального ответа у записи.
pub const SELECT_DETAIL_AT: &str = "SELECT detail_at FROM items WHERE id=?";

/// Протух ли кешированный перевод — порт
/// `time.time() - (row["cached_at"] or 0) > TRANSLATION_TTL` из
/// `get_translation`. `cached_at = None` (NULL в БД) → трактуется как `0`
/// (Python-идиома `... or 0`).
pub fn is_translation_stale(cached_at: Option<f64>, now: f64) -> bool {
    (now - cached_at.unwrap_or(0.0)) > TRANSLATION_TTL
}

/// Порог для `purge_stale_translations`: удаляются записи с
/// `cached_at < threshold`, где `threshold = time.time() - TRANSLATION_TTL`.
pub fn translation_purge_threshold(now: f64) -> f64 {
    now - TRANSLATION_TTL
}

// ── тексты SQL (байт-в-байт как в cache.py) ────────────────────────────────

/// DDL создания таблиц (`_init_schema` → `executescript`).
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

/// Миграция: добавить колонку `preview_urls` (галерея), если её нет.
pub const ADD_PREVIEW_URLS_COLUMN: &str = "ALTER TABLE items ADD COLUMN preview_urls TEXT";

/// Проверка наличия колонок таблицы `items` (для миграции).
pub const PRAGMA_TABLE_INFO_ITEMS: &str = "PRAGMA table_info(items)";

/// UPSERT одного элемента (`_insert_item`) — 18 плейсхолдеров.
pub const INSERT_ITEM: &str = concat!(
    "INSERT OR REPLACE INTO items\n",
    "               (id, category_id, name, author, description, description_ru,\n",
    "                rating, downloads, preview_url, preview_urls, download_url,\n",
    "                download_name, download_size, version, license, created,\n",
    "                updated, cached_at)\n",
    "               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
);

/// Выбрать один элемент по `id` (`get_item`/`get_list`).
pub const SELECT_ITEM_BY_ID: &str = "SELECT * FROM items WHERE id=?";

/// Лёгкая выборка `(id, name, preview_url)` всех элементов (`all_items_min`).
pub const SELECT_ITEMS_MIN: &str = "SELECT id, name, preview_url FROM items";

/// Выбрать кеш списка по ключу (`get_list`).
pub const SELECT_LIST: &str = "SELECT ids, total, cached_at FROM lists WHERE key=?";

/// UPSERT кеша списка (`put_list`).
pub const INSERT_LIST: &str =
    "INSERT OR REPLACE INTO lists (key, ids, total, cached_at) VALUES (?,?,?,?)";

/// Сбросить весь кеш списков (`invalidate_lists`).
pub const DELETE_LISTS: &str = "DELETE FROM lists";

/// Выбрать локальный путь превью по URL (`get_preview`).
pub const SELECT_PREVIEW: &str = "SELECT local_path FROM previews WHERE url=?";

/// Выбрать все `(url, local_path)` превью (`all_previews_min`).
pub const SELECT_PREVIEWS_MIN: &str = "SELECT url, local_path FROM previews";

/// UPSERT превью (`put_preview`).
pub const INSERT_PREVIEW: &str =
    "INSERT OR REPLACE INTO previews (url, local_path, cached_at) VALUES (?,?,?)";

/// Выбрать перевод по `src_hash` (`get_translation`).
pub const SELECT_TRANSLATION: &str =
    "SELECT translated, cached_at FROM translations WHERE src_hash=?";

/// Удалить одну протухшую запись перевода (`get_translation`, ленивая чистка).
pub const DELETE_TRANSLATION: &str = "DELETE FROM translations WHERE src_hash=?";

/// UPSERT перевода (`put_translation`).
pub const INSERT_TRANSLATION: &str =
    "INSERT OR REPLACE INTO translations (src_hash, translated, cached_at) VALUES (?,?,?)";

/// Удалить все протухшие переводы (`purge_stale_translations`).
pub const DELETE_STALE_TRANSLATIONS: &str = "DELETE FROM translations WHERE cached_at < ?";

// ── сериализация/разбор поля preview_urls (JSON-массив) ────────────────────

/// Сериализовать `preview_urls` как `json.dumps(list or [])` в `_insert_item`:
/// `ensure_ascii=True` (не-ASCII → `\uXXXX`, astral → суррогатная пара),
/// разделитель элементов `", "`. Пустой список → `"[]"`.
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

/// Закодировать строку как строковый литерал Python `json.dumps`
/// (`ensure_ascii=True`) в `out`, включая кавычки.
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
            // Печатаемый ASCII (0x20..=0x7e) — как есть.
            c if ('\u{20}'..='\u{7e}').contains(&c) => out.push(c),
            // Всё прочее (управляющие <0x20, 0x7f и любой не-ASCII) → \uXXXX,
            // за пределами BMP — суррогатная пара, как в CPython.
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

/// Разобрать сохранённое `preview_urls` — порт выражения из `_row_to_item`:
/// `json.loads(raw or "[]")`; не-список или ошибка разбора → `[]`; затем
/// `[str(u) for u in previews if u]` (falsy-элементы отфильтрованы, прочие
/// приведены к строке как Python `str`). `raw = None`/`""` → `"[]"`.
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

/// Python-истинность JSON-значения (`if u`): null/false/0/""/[]/{} → false.
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

/// Аналог Python `str(u)` для JSON-значения (в `preview_urls` практически
/// всегда строки; числа/булевы поддержаны для точного паритета фильтрации).
fn python_str(v: &serde_json::Value) -> String {
    use serde_json::Value;
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Number(n) => n.to_string(),
        // Вложенные контейнеры в preview_urls не встречаются; отдаём JSON-вид.
        other => other.to_string(),
    }
}

// ── CSV-поле lists.ids ─────────────────────────────────────────────────────

/// Собрать `lists.ids` — порт `",".join(it.id for it in items)` (`put_list`).
pub fn join_ids(ids: &[String]) -> String {
    ids.join(",")
}

/// Разобрать `lists.ids` — порт `[i for i in (row["ids"] or "").split(",")
/// if i]` (`get_list`): пустые сегменты отбрасываются, `None`/`""` → `[]`.
pub fn split_ids(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// ── значения UPSERT (порядок + типы плейсхолдеров) ─────────────────────────

/// Значение параметра запроса — как sqlite3 связывает Python-объект: `TEXT`,
/// `INTEGER`, `REAL` или `NULL`. Документирует порядок/типы плейсхолдеров;
/// само связывание — side effect на Python.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

/// 18 значений `INSERT OR REPLACE INTO items` в порядке колонок (`_insert_item`).
/// `now` = `time.time()` (наблюдение, приходит параметром) → колонка `cached_at`.
/// `description_ru` берётся из `description_translated`, `preview_urls`
/// сериализуется [`serialize_preview_urls`].
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

/// Параметры `INSERT OR REPLACE INTO lists` (`put_list`): key, ids, total, now.
pub fn list_insert_values(key: &str, ids: &str, total: i64, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(key.to_string()),
        CacheValue::Text(ids.to_string()),
        CacheValue::Int(total),
        CacheValue::Real(now),
    ]
}

/// Параметры `INSERT OR REPLACE INTO previews` (`put_preview`).
pub fn preview_insert_values(url: &str, local_path: &str, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(url.to_string()),
        CacheValue::Text(local_path.to_string()),
        CacheValue::Real(now),
    ]
}

/// Параметры `INSERT OR REPLACE INTO translations` (`put_translation`).
pub fn translation_insert_values(src_hash: &str, translated: &str, now: f64) -> Vec<CacheValue> {
    vec![
        CacheValue::Text(src_hash.to_string()),
        CacheValue::Text(translated.to_string()),
        CacheValue::Real(now),
    ]
}

// ── нормализация строки таблицы items в ThemeItem ──────────────────────────

/// Значения колонок строки таблицы `items` (наблюдения из БД). `Option::None`
/// = SQL NULL; Python `sqlite3.Row` отдаёт их как `None`, а `_row_to_item`
/// нормализует идиомой `... or <default>`.
#[derive(Debug, Clone, Default)]
pub struct ItemRow {
    /// PRIMARY KEY — не бывает NULL, поэтому не `Option`.
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

/// Построить [`ThemeItem`] из строки таблицы `items` — порт `_row_to_item`.
/// NULL-поля → дефолты (`or ""`/`or 0`), `preview_urls` → [`parse_preview_urls`],
/// `description_ru` → `description_translated`. Прочие поля `ThemeItem`
/// (`preview_local`, `installed`, `install_path`, `download_links`) — дефолты.
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
//  ЖИВОЙ кеш каталога: та же база, что у Python
// ═══════════════════════════════════════════════════════════════════════════
//
// ЗАЧЕМ ОН ЗДЕСЬ. Всё выше — правила без побочных эффектов, и до 08.09.2026
// исполнителя у них не было вовсе: SQLite открывал `rexon/appearance/cache.py`,
// то есть каталог тем работал ТОЛЬКО в процессе с Python. Из-за этого вкладка
// «Оформление» под Rust-хозяином окна открывалась пустой: у клиента магазина
// (`crate::ocs::OcsClient`) не было ни одной живой реализации [`crate::ocs::OcsCacheIo`].
//
// Живой кеш лежит рядом с правилами, а не в Qt-слое, ровно по правилу #12:
// тексты SQL, TTL и разбор строк — здесь, и второй их копии быть не должно.
// `rusqlite` в дереве ядра уже есть (её тянет `steam_token`), новых
// зависимостей это не добавляет.
//
// ⚠️ БАЗА ОБЩАЯ С PYTHON, и это условие, а не совпадение: обе вкладки
// (виджетная на PySide6 и QML) ходят в `~/.cache/rexon/ocs_cache.db`. Поэтому
// схема здесь заводится ТЕМИ ЖЕ строками и мигрируется теми же двумя
// колонками; разойдись они — Python перестал бы читать написанное нами.

/// Миграция: колонка `detail_at` (когда запись приходила из ДЕТАЛЬНОГО
/// ответа). Пара к [`ADD_PREVIEW_URLS_COLUMN`] — в живой базе есть обе.
pub const ADD_DETAIL_AT_COLUMN: &str = "ALTER TABLE items ADD COLUMN detail_at REAL";

/// UPSERT элемента, СОХРАНЯЮЩИЙ `detail_at` (порт `_insert_item`).
///
/// Отдельно от [`INSERT_ITEM`], и это не украшение: `INSERT OR REPLACE`
/// пишет строку целиком, поэтому ответ-СПИСОК стирал бы отметку «полные
/// детали были», и установка получала бы товар без ссылок на файлы.
/// 19-й параметр — тот же `id` (для подзапроса).
pub const INSERT_ITEM_KEEP_DETAIL: &str = concat!(
    "INSERT OR REPLACE INTO items\n",
    "               (id, category_id, name, author, description, description_ru,\n",
    "                rating, downloads, preview_url, preview_urls, download_url,\n",
    "                download_name, download_size, version, license, created,\n",
    "                updated, cached_at, detail_at)\n",
    "               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,\n",
    "                       (SELECT detail_at FROM items WHERE id=?))",
);

/// Отметить запись пришедшей из детального ответа (порт `put_item`).
pub const UPDATE_DETAIL_AT: &str = "UPDATE items SET detail_at=? WHERE id=?";

/// Секунды эпохи дробным числом — `time.time()` оригинала.
pub fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Живой SQLite-кеш каталога оформления — исполнитель правил этого модуля.
///
/// Открывается на КАЖДУЮ работу заново (как `OCSCache()` у Python-воркера):
/// соединение sqlite не пересекает потоки, а работы вкладки идут каждая в
/// своём. База не открылась (нет прав, диск полон) — кеш просто пуст: это
/// замедление, а не отказ, и вкладка обязана продолжать работать.
pub struct LiveCache {
    conn: Option<rusqlite::Connection>,
    preview_dir: String,
}

impl LiveCache {
    /// Открыть базу и довести схему до сегодняшней (порт `__init__`).
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

    /// Каталог скачанных превью (его же ждёт [`crate::ocs::OcsClient::new`]).
    pub fn preview_dir(&self) -> &str {
        &self.preview_dir
    }

    fn init_schema(&mut self) {
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        if conn.execute_batch(CREATE_SCHEMA).is_err() {
            // Схему завести не вышло — дальше работать с этой базой нельзя:
            // молчаливые ошибки на каждом запросе только жгут время.
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

    /// Строка таблицы `items` → [`ItemRow`]. Имена колонок, а не номера:
    /// `SELECT *` отдаёт их в порядке схемы, а он менялся миграциями.
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

    /// Один элемент по номеру — порт `get_item`.
    pub fn get_item(&mut self, content_id: &str) -> Option<ThemeItem> {
        let conn = self.conn.as_ref()?;
        let mut stmt = conn.prepare(SELECT_ITEM_BY_ID).ok()?;
        stmt.query_row([content_id], |r| Ok(row_to_item(&Self::read_item_row(r))))
            .ok()
    }

    /// Свежие ПОЛНЫЕ детали либо `None` (тогда — в сеть) — порт
    /// `get_item_detail`. Свежесть — по `detail_at` ([`is_detail_stale`]).
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

    /// Положить элемент, отметив его пришедшим из детального ответа —
    /// порт `put_item`.
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

    /// Страница поиска из кеша — порт `get_list` (протухшая = промах).
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

    /// Запомнить страницу поиска — порт `put_list` (один коммит на страницу).
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

    /// Путь скачанного превью — порт `get_preview`.
    ///
    /// Запись, чей файл с диска исчез, промахивается: иначе плитка показывала
    /// бы пустое место вместо картинки, которую можно скачать заново.
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

    /// Отметить скачанное превью — порт `put_preview`.
    pub fn put_preview(&mut self, url: &str, local_path: &str) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let vals = preview_insert_values(url, local_path, now);
        let _ = conn.execute(INSERT_PREVIEW, rusqlite::params_from_iter(bind(&vals)));
    }

    /// `(имя, ссылка на превью)` всех закешированных тем — порт
    /// `all_items_min`; сырьё бэкофилла превью локальных тем
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

    /// `(ссылка на превью, номер категории)` всех закешированных тем: по
    /// ним скан установленного знает, к какому разделу относится картинка
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

    /// `url → файл` всех превью, чей файл ещё лежит на диске — порт
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

    /// Свежий перевод из кеша либо `None` — порт `get_translation`
    /// (протухшая запись удаляется лениво, как у оригинала).
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

    /// Записать перевод в кеш — порт `put_translation`. Ключ `src_hash`
    /// считает [`crate::translator::translation_cache_key`]: версия движка
    /// уже в нём, и записи прежнего движка не попадут под этот ключ.
    pub fn put_translation(&mut self, src_hash: &str, translated: &str) {
        let now = now_epoch();
        let Some(conn) = self.conn.as_mut() else {
            return;
        };
        let vals = translation_insert_values(src_hash, translated, now);
        let _ = conn.execute(INSERT_TRANSLATION, rusqlite::params_from_iter(bind(&vals)));
    }

    /// Разовая чистка протухших переводов — порт `purge_stale_translations`.
    /// Отдаёт число удалённых записей.
    pub fn purge_stale_translations(&mut self) -> usize {
        let threshold = translation_purge_threshold(now_epoch());
        let Some(conn) = self.conn.as_mut() else {
            return 0;
        };
        conn.execute(DELETE_STALE_TRANSLATIONS, rusqlite::params![threshold])
            .unwrap_or(0)
    }
}

/// UPSERT элемента с сохранением `detail_at` (общий для одиночной вставки и
/// для пачки под одной транзакцией).
fn insert_item_keep_detail(
    conn: &rusqlite::Connection,
    item: &ThemeItem,
    now: f64,
) -> rusqlite::Result<usize> {
    let mut vals = bind(&item_insert_values(item, now));
    // 19-й параметр подзапроса — тот же номер товара.
    vals.push(rusqlite::types::Value::Text(item.id.clone()));
    conn.execute(INSERT_ITEM_KEEP_DETAIL, rusqlite::params_from_iter(vals))
}

/// [`CacheValue`] → значение параметра sqlite. Переводчик, а не второй набор
/// правил: порядок и типы параметров решают функции выше.
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

    /// Кеш живого пользователя идёт за `XDG_CACHE_HOME`; подставной домашний
    /// каталог (сверки, стенд с копией кеша) переменную не видит.
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
        // Все пути кеша «Оформления» растут из одного корня.
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

    /// Свой «дом» на одну сверку: живой кеш пишет настоящие файлы, и трогать
    /// кеш машины он не должен.
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

    /// Записанная страница поиска читается обратно ТЕМ ЖЕ составом. Сверка
    /// проходит весь путь: схему, миграции, UPSERT товаров, CSV-поле `ids` и
    /// сборку `ThemeItem` из строки таблицы.
    #[test]
    fn a_written_page_reads_back_the_same_way() {
        let home = TempHome::new("list");
        let mut cache = LiveCache::open(&home.0);
        let items = vec![item("101", "Первая"), item("202", "Вторая")];
        cache.put_list("ключ", &items, 42);

        // ⚠️ Читает ДРУГОЙ экземпляр: кеш заводится на одну работу, и запись,
        // видимая только своему соединению, была бы бесполезна.
        let mut second = LiveCache::open(&home.0);
        let (back, total) = second.get_list("ключ").expect("страница обязана найтись");
        assert_eq!(total, 42);
        let names: Vec<String> = back.iter().map(|it| it.name.clone()).collect();
        assert_eq!(names, vec!["Первая".to_string(), "Вторая".to_string()]);
        assert_eq!(back[0].preview_urls, items[0].preview_urls);
        assert_eq!(back[1].category_id, 104);
    }

    /// Протухшая страница — ПРОМАХ, а не устаревший ответ. Мерило протухания
    /// одно на обе версии вкладки (`is_list_stale`), и здесь проверяется, что
    /// живой кеш его действительно спрашивает.
    #[test]
    fn a_stale_page_is_a_miss() {
        let home = TempHome::new("stale");
        let mut cache = LiveCache::open(&home.0);
        cache.put_list("ключ", &[item("1", "Тема")], 1);
        assert!(cache.get_list("ключ").is_some(), "свежая страница обязана найтись");

        // Состарить запись прямо в базе: живой сверке ждать час нельзя.
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

    /// Отметка «полные детали были» (`detail_at`) переживает ответ-СПИСОК.
    ///
    /// ⚠️ Ради этого и заведён отдельный `INSERT_ITEM_KEEP_DETAIL`: обычный
    /// `INSERT OR REPLACE` пишет строку целиком, отметка стиралась бы при
    /// каждом обновлении витрины, и установка получала бы товар без ссылок на
    /// файлы.
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

        // Тот же товар приезжает ещё раз, но уже страницей витрины.
        cache.put_list("ключ", &[item("777", "Полная")], 1);
        assert_eq!(
            detail_at(&home.0, "777"),
            mark_after_detail,
            "ответ-список не смеет стирать отметку детального ответа"
        );
    }

    /// Запись превью, чей файл с диска исчез, — промах. Иначе плитка
    /// показывала бы пустое место вместо картинки, которую можно скачать
    /// заново.
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

    /// Отметка `detail_at` строки товара (только для сверок выше).
    /// Полные детали живут в кеше срок показа, запись из ответа-списка
    /// полными деталями не считается, протухшие — промах.
    #[test]
    fn detail_is_served_from_cache_only_while_fresh() {
        let home = TempHome::new("detail-ttl");
        let mut cache = LiveCache::open(&home.0);
        let ttl = crate::ocs::DETAIL_TTL_SECS as f64;

        // Только из списка — отметки нет, в сеть.
        cache.put_list("ключ", &[item("5", "Из списка")], 1);
        assert!(cache.get_item_detail("5", ttl).is_none());
        // Нет записи вовсе.
        assert!(cache.get_item_detail("нет", ttl).is_none());

        // Детальный ответ — свежий, отдаётся из кеша.
        cache.put_item(&item("6", "Полная"));
        let mut second = LiveCache::open(&home.0);
        let hit = second.get_item_detail("6", ttl).expect("свежие детали");
        assert_eq!(hit.name, "Полная");

        // Состарить отметку на сутки и секунду — промах.
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

    /// Полный круг перевода: записали → прочитали ДРУГИМ экземпляром →
    /// состарили → чистка его унесла. Проверяется весь путь записи
    /// (`put_translation` + `translation_insert_values` + схема), а не только
    /// предикат протухания.
    #[test]
    fn a_translation_survives_a_round_trip_and_dies_by_ttl() {
        let home = TempHome::new("translation");
        let mut cache = LiveCache::open(&home.0);
        let key = crate::translator::translation_cache_key("en", "ru", "Dark theme");
        cache.put_translation(&key, "Тёмная тема");

        // Читает другой экземпляр: запись, видимая только своему соединению,
        // была бы бесполезна — перевод переживает работу приложения.
        let mut second = LiveCache::open(&home.0);
        assert_eq!(
            second.get_translation(&key).as_deref(),
            Some("Тёмная тема"),
            "свежий перевод обязан найтись"
        );
        // Чужого ключа в кеше нет — иначе перевод подменял бы соседний текст.
        assert!(second.get_translation("нет такого").is_none());
        assert_eq!(second.purge_stale_translations(), 0, "свежему переводу чистка не грозит");

        // Состарить на месяц и секунду прямо в базе: живой сверке не ждать 30
        // дней.
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
        // Ленивая чистка при чтении уже убрала строку, разовая — ничего не
        // находит: обе дороги ведут к пустой таблице.
        assert_eq!(third.purge_stale_translations(), 0, "ленивая чистка уже удалила запись");
        assert_eq!(rows_in_translations(&home.0), 0, "протухшая строка не смеет копиться");

        // Запись под тем же ключом ложится заново (UPSERT, не дубль).
        third.put_translation(&key, "Тёмное оформление");
        third.put_translation(&key, "Тёмная тема");
        assert_eq!(rows_in_translations(&home.0), 1, "UPSERT, а не второй ряд");
        assert_eq!(third.get_translation(&key).as_deref(), Some("Тёмная тема"));
    }

    /// Сколько строк в таблице переводов (только для сверки выше).
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
