//! Golden-сверка порта `rexon/appearance/cache.py` (модуль
//! `rexon_core::appearance_cache`).
//!
//! Эталон снят с ЖИВОГО `cache.py` read-only (одноразовым
//! генератором), БЕЗ инстанцирования `OCSCache` (SQLite/ФС
//! не трогаются):
//!   * `sql_literals` — все SQL-строки, извлечённые AST-обходом исходника;
//!   * `list_ttl`/`translation_ttl` — константы модуля;
//!   * `ttl_list`/`ttl_translation` — предикаты `get_list`/`get_translation`;
//!   * `purge_threshold` — порог `purge_stale_translations`;
//!   * `preview_urls_serialize` — реальный `json.dumps(list or [])`;
//!   * `preview_urls_parse`/`row_to_item` — реальные `_row_to_item`-выражения
//!     (последнее — вызовом самого `_row_to_item(dict)`);
//!   * `ids_join`/`ids_split` — CSV-поле `lists.ids`;
//!   * `item_insert_values` — порядок/типы плейсхолдеров `_insert_item`.
//!
//! ⚠️ Сам sqlite3, `os.makedirs`, `os.path.isfile`, `time.time()` — side effect,
//! НЕ портированы; проверяется ЧИСТАЯ логика.

use rexon_core::appearance_cache as ac;
use rexon_core::appearance_cache::CacheValue;
use rexon_core::models::ThemeItem;
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/appearance_cache_cases.json")).unwrap()
}

/// Все SQL-строки Rust-констант совпадают ПО МНОЖЕСТВУ с литералами из
/// живого исходника cache.py (байт-в-байт, включая отступы DDL).
#[test]
fn sql_literals_match_python() {
    let g = golden();
    let mut want: Vec<String> = g["sql_literals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    want.sort();

    let mut got: Vec<String> = vec![
        ac::CREATE_SCHEMA,
        ac::ADD_PREVIEW_URLS_COLUMN,
        ac::PRAGMA_TABLE_INFO_ITEMS,
        ac::INSERT_ITEM,
        ac::SELECT_ITEM_BY_ID,
        ac::SELECT_ITEMS_MIN,
        ac::SELECT_LIST,
        ac::INSERT_LIST,
        ac::DELETE_LISTS,
        ac::SELECT_PREVIEW,
        ac::SELECT_PREVIEWS_MIN,
        ac::INSERT_PREVIEW,
        ac::SELECT_TRANSLATION,
        ac::DELETE_TRANSLATION,
        ac::INSERT_TRANSLATION,
        ac::DELETE_STALE_TRANSLATIONS,
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    got.sort();

    assert_eq!(got, want, "набор SQL-строк должен совпадать с cache.py");
}

/// TTL-константы совпадают с Python (в секундах).
#[test]
fn ttl_constants_match_python() {
    let g = golden();
    assert_eq!(ac::LIST_TTL, g["list_ttl"].as_f64().unwrap());
    assert_eq!(ac::TRANSLATION_TTL, g["translation_ttl"].as_f64().unwrap());
}

/// Предикат протухания списков — `(now - cached_at) > LIST_TTL`.
#[test]
fn list_staleness_matches_python() {
    let g = golden();
    for c in g["ttl_list"].as_array().unwrap() {
        let cached_at = c["cached_at"].as_f64().unwrap();
        let now = c["now"].as_f64().unwrap();
        let want = c["stale"].as_bool().unwrap();
        assert_eq!(ac::is_list_stale(cached_at, now), want, "ttl_list {c:?}");
    }
}

/// Предикат протухания переводов — `(now - (cached_at or 0)) > TRANSLATION_TTL`.
#[test]
fn translation_staleness_matches_python() {
    let g = golden();
    for c in g["ttl_translation"].as_array().unwrap() {
        let cached_at = c["cached_at"].as_f64(); // None → NULL
        let now = c["now"].as_f64().unwrap();
        let want = c["stale"].as_bool().unwrap();
        assert_eq!(
            ac::is_translation_stale(cached_at, now),
            want,
            "ttl_translation {c:?}"
        );
    }
}

/// Порог чистки протухших переводов — `now - TRANSLATION_TTL`.
#[test]
fn purge_threshold_matches_python() {
    let g = golden();
    for c in g["purge_threshold"].as_array().unwrap() {
        let now = c["now"].as_f64().unwrap();
        let want = c["threshold"].as_f64().unwrap();
        assert_eq!(ac::translation_purge_threshold(now), want, "purge {c:?}");
    }
}

/// Сериализация preview_urls байт-в-байт как `json.dumps(list or [])`.
#[test]
fn preview_urls_serialize_matches_python() {
    let g = golden();
    for c in g["preview_urls_serialize"].as_array().unwrap() {
        let input: Vec<String> = c["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let want = c["output"].as_str().unwrap();
        assert_eq!(ac::serialize_preview_urls(&input), want, "serialize {input:?}");
    }
}

/// Разбор preview_urls совпадает с выражением из `_row_to_item`.
#[test]
fn preview_urls_parse_matches_python() {
    let g = golden();
    for c in g["preview_urls_parse"].as_array().unwrap() {
        let raw = c["raw"].as_str(); // None → NULL
        let want: Vec<String> = c["output"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ac::parse_preview_urls(raw), want, "parse {raw:?}");
    }
}

/// CSV `lists.ids`: join и split совпадают с Python.
#[test]
fn ids_csv_matches_python() {
    let g = golden();
    for c in g["ids_join"].as_array().unwrap() {
        let ids: Vec<String> = c["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ac::join_ids(&ids), c["output"].as_str().unwrap(), "join {ids:?}");
    }
    for c in g["ids_split"].as_array().unwrap() {
        let raw = c["raw"].as_str();
        let want: Vec<String> = c["output"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ac::split_ids(raw), want, "split {raw:?}");
    }
}

/// Значения `INSERT OR REPLACE INTO items`: порядок + типы + сериализация.
#[test]
fn item_insert_values_match_python() {
    let g = golden();
    let iv = &g["item_insert_values"];
    let now = iv["now"].as_f64().unwrap();
    let item: ThemeItem = serde_json::from_value(iv["item"].clone()).unwrap();

    let want: Vec<CacheValue> = iv["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(cache_value_from_golden)
        .collect();

    assert_eq!(ac::item_insert_values(&item, now), want);
}

/// row_to_item совпадает с реальным `_row_to_item` (нормализация NULL + разбор
/// preview_urls); сравнение через `asdict`-эквивалент (serde Value).
#[test]
fn row_to_item_matches_python() {
    let g = golden();
    for c in g["row_to_item"].as_array().unwrap() {
        let row = item_row_from_golden(&c["row"]);
        let got = serde_json::to_value(ac::row_to_item(&row)).unwrap();
        assert_eq!(got, c["expected"], "row_to_item {:?}", c["row"]);
    }
}

// ── хелперы разбора golden ──────────────────────────────────────────────────

/// `["text"|"int"|"real"|"null", value]` → [`CacheValue`].
fn cache_value_from_golden(v: &Value) -> CacheValue {
    let arr = v.as_array().unwrap();
    match arr[0].as_str().unwrap() {
        "text" => CacheValue::Text(arr[1].as_str().unwrap().to_string()),
        "int" => CacheValue::Int(arr[1].as_i64().unwrap()),
        "real" => CacheValue::Real(arr[1].as_f64().unwrap()),
        "null" => CacheValue::Null,
        other => panic!("неизвестный тег {other}"),
    }
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v[key].as_str().map(str::to_string)
}

fn opt_i64(v: &Value, key: &str) -> Option<i64> {
    v[key].as_i64()
}

fn item_row_from_golden(v: &Value) -> ac::ItemRow {
    ac::ItemRow {
        id: v["id"].as_str().unwrap().to_string(),
        category_id: opt_i64(v, "category_id"),
        name: opt_str(v, "name"),
        author: opt_str(v, "author"),
        description: opt_str(v, "description"),
        description_ru: opt_str(v, "description_ru"),
        rating: opt_i64(v, "rating"),
        downloads: opt_i64(v, "downloads"),
        preview_url: opt_str(v, "preview_url"),
        preview_urls: opt_str(v, "preview_urls"),
        download_url: opt_str(v, "download_url"),
        download_name: opt_str(v, "download_name"),
        download_size: opt_i64(v, "download_size"),
        version: opt_str(v, "version"),
        license: opt_str(v, "license"),
        created: opt_str(v, "created"),
        updated: opt_str(v, "updated"),
    }
}

/// Расположение кеша каталога на диске (`CACHE_DIR`/`DB_PATH`/`PREVIEW_DIR`):
/// эталон — значения ЖИВОГО `cache.py` на этой машине.
#[test]
fn cache_paths_match_python() {
    let g: Value =
        serde_json::from_str(include_str!("golden/appearance_extra_cases.json")).unwrap();
    let p = &g["cache_paths"];
    let home = p["home"].as_str().unwrap();
    assert_eq!(ac::cache_dir(home), p["cache_dir"].as_str().unwrap(), "CACHE_DIR");
    assert_eq!(ac::db_path(home), p["db_path"].as_str().unwrap(), "DB_PATH");
    assert_eq!(ac::preview_dir(home), p["preview_dir"].as_str().unwrap(), "PREVIEW_DIR");
    // Смена HOME не должна ничего «прибивать» к домашке разработчика.
    assert_eq!(ac::db_path("/tmp/x"), "/tmp/x/.cache/rexon/ocs_cache.db");
}
