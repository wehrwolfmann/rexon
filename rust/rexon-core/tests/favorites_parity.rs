//! Golden check of the favorites port against the original `rexon/appearance/favorites.py`.
//!
//! The golden `golden/favorites_store.json` was captured from Python: a store of two entries
//! (OCS + local), as `toggle_favorite` writes them, `json.dump(ensure_ascii=
//! False, indent=1)`. Any format mismatch is a port bug.
//!
//! `golden/favorites_real_names.json` is a full cycle on REAL installed
//! themes of this machine (color schemes `/usr/share/color-schemes`, icons
//! `/usr/share/icons`) plus an OCS item with Cyrillic/emoji/HTML: captured from live
//! `favorites.py` in a TEMPORARY HOME. It is the one that caught the `swap_remove` order
//! when REMOVING a star (see `favorites::toggle`). The golden is written WITHOUT
//! `sort_keys`: the key order of `download_links` (url/name/size) is load-bearing.

use rexon_core::favorites::{is_favorite, list_items, load_raw, save_raw, toggle};
use rexon_core::models::{to_json_indent1, ThemeItem};
use serde_json::{Map, Value};

/// The same two themes as in the python golden capture script.
fn ocs_item() -> ThemeItem {
    ThemeItem {
        id: "123456".into(),
        name: "Dracula".into(),
        author: "Team".into(),
        rating: 90,
        downloads: 5000,
        preview_url: "http://x/p.png".into(),
        preview_urls: vec!["http://x/1.png".into(), "http://x/2.png".into()],
        preview_local: "/cache/x.png".into(),
        ..Default::default()
    }
}

fn local_item() -> ThemeItem {
    ThemeItem {
        id: "local:gtk:MyTheme".into(),
        name: "Моя тема".into(),
        author: "🎨 Локальная".into(),
        preview_local: "/home/u/.themes/MyTheme/preview.png".into(),
        ..Default::default()
    }
}

/// A store of two added themes serializes byte-for-byte like Python.
/// Checks: insertion order, `preview_local` cleared on the OCS theme and
/// kept on the local one, Cyrillic/emoji unescaped, `indent=1`.
#[test]
fn store_serialize_byte_identical() {
    let golden = include_str!("golden/favorites_store.json");
    let mut store: Map<String, Value> = Map::new();
    assert!(toggle(&mut store, &ocs_item())); // → in favorites
    assert!(toggle(&mut store, &local_item()));
    assert_eq!(to_json_indent1(&Value::Object(store)), golden);
}

/// A repeated toggle removes the item and returns false.
#[test]
fn toggle_adds_then_removes() {
    let mut store: Map<String, Value> = Map::new();
    let item = ocs_item();
    assert!(toggle(&mut store, &item));
    assert!(is_favorite(&store, "123456"));
    assert!(!toggle(&mut store, &item)); // removed
    assert!(!is_favorite(&store, "123456"));
    assert!(store.is_empty());
}

/// An empty id — no change, always false, not "in favorites".
#[test]
fn empty_id_never_favorite() {
    let mut store: Map<String, Value> = Map::new();
    assert!(!toggle(&mut store, &ThemeItem::default()));
    assert!(store.is_empty());
    assert!(!is_favorite(&store, ""));
}

/// `list_items`: restores ThemeItem and sets `local_kind = "downloaded"`
/// on local ones; OCS ones keep `None`.
#[test]
fn list_items_sets_local_kind() {
    let mut store: Map<String, Value> = Map::new();
    toggle(&mut store, &ocs_item());
    toggle(&mut store, &local_item());
    let items = list_items(&store);
    assert_eq!(items.len(), 2);
    let ocs = items.iter().find(|i| i.id == "123456").unwrap();
    assert_eq!(ocs.local_kind, None);
    assert_eq!(ocs.preview_local, ""); // cleared on add
    let loc = items.iter().find(|i| i.id == "local:gtk:MyTheme").unwrap();
    assert_eq!(loc.local_kind.as_deref(), Some("downloaded"));
    assert_eq!(loc.preview_local, "/home/u/.themes/MyTheme/preview.png");
}

/// Unknown keys are dropped, missing ones take the default — like
/// `ThemeItem(**{k: v … if k in _FIELDS})`.
#[test]
fn list_items_drops_unknown_and_fills_defaults() {
    let mut store: Map<String, Value> = Map::new();
    store.insert(
        "42".into(),
        serde_json::json!({"id": "42", "unknown_field": 999, "rating": 60}),
    );
    let items = list_items(&store);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "42");
    assert_eq!(items[0].rating, 60);
    assert_eq!(items[0].name, ""); // default
}

/// A broken/missing file → an empty store (like the except in Python).
#[test]
fn load_bad_or_missing_file_is_empty() {
    let dir = std::env::temp_dir();
    let missing = dir.join("rexon_no_fav_xyz.json");
    assert!(load_raw(&missing).is_empty());

    let bad = dir.join(format!("rexon_bad_fav_{}.json", std::process::id()));
    std::fs::write(&bad, "not json at all").unwrap();
    assert!(load_raw(&bad).is_empty());
    let _ = std::fs::remove_file(&bad);

    // A JSON array (not an object) at the top level → empty too.
    let arr = dir.join(format!("rexon_arr_fav_{}.json", std::process::id()));
    std::fs::write(&arr, "[1, 2, 3]").unwrap();
    assert!(load_raw(&arr).is_empty());
    let _ = std::fs::remove_file(&arr);
}

/// save→load returns the same store (atomic write via .tmp + rename).
#[test]
fn save_then_load_roundtrip() {
    let path = std::env::temp_dir()
        .join(format!("rexon_fav_rt_{}.json", std::process::id()));
    let mut store: Map<String, Value> = Map::new();
    toggle(&mut store, &ocs_item());
    save_raw(&path, &store).unwrap();
    let loaded = load_raw(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded, store);
}

/// A full favorites cycle on REAL data of this machine: real color
/// schemes `/usr/share/color-schemes` and icon themes `/usr/share/icons` as
/// local items + an OCS item with Cyrillic, «guillemets», emoji and HTML in
/// the description.
///
/// The golden `golden/favorites_real_names.json` was captured from LIVE `favorites.py` in a
/// TEMPORARY HOME (the real `~/.config/rexon/favorites.json` was not created):
/// adding all items → raw file → `list_favorite_items` → removing
/// the first → the file again. The file bytes and the restored items are compared.
#[test]
fn real_installed_themes_roundtrip() {
    let g: Value = serde_json::from_str(include_str!("golden/favorites_real_names.json")).unwrap();
    let items: Vec<ThemeItem> = g["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| serde_json::from_value(v.clone()).unwrap())
        .collect();
    assert!(items.len() >= 5, "мало реальных элементов в эталоне");

    // 1. Adding in order → the same states and the same file bytes.
    let mut store: Map<String, Value> = Map::new();
    let states: Vec<bool> = items.iter().map(|it| toggle(&mut store, it)).collect();
    let want_states: Vec<bool> =
        g["toggle_states"].as_array().unwrap().iter().map(|v| v.as_bool().unwrap()).collect();
    assert_eq!(states, want_states, "состояния toggle");
    // Python writes json.dump(..., indent=1) WITHOUT a trailing newline.
    assert_eq!(
        to_json_indent1(&Value::Object(store.clone())),
        g["raw"].as_str().unwrap(),
        "favorites.json байт-в-байт"
    );
    let raw = g["raw"].as_str().unwrap();
    assert!(raw.chars().any(|c| c as u32 > 127), "эталон без не-ASCII — нечего проверять");
    assert!(!raw.contains("\\u"), "кириллица экранирована (ensure_ascii=False нарушен)");

    // 2. Restoring items: fields + the runtime `_local_kind` on local ones.
    let listed = list_items(&store);
    let want_listed = g["listed"].as_array().unwrap();
    assert_eq!(listed.len(), want_listed.len(), "число восстановленных");
    for (got, w) in listed.iter().zip(want_listed) {
        assert_eq!(got.id, w["id"].as_str().unwrap(), "id");
        assert_eq!(got.name, w["name"].as_str().unwrap(), "name {}", got.id);
        assert_eq!(got.author, w["author"].as_str().unwrap(), "author {}", got.id);
        assert_eq!(
            got.description_translated,
            w["description_translated"].as_str().unwrap(),
            "перевод описания {}",
            got.id
        );
        assert_eq!(
            got.preview_local,
            w["preview_local"].as_str().unwrap(),
            "preview_local {}",
            got.id
        );
        assert_eq!(got.local_kind.as_deref(), w["_local_kind"].as_str(), "_local_kind {}", got.id);
    }
    // Local themes keep the preview path, the OCS theme does not (the cache goes stale).
    assert!(listed[0].preview_local.is_empty(), "у OCS-темы preview_local не обнулён");
    assert!(listed[1..].iter().all(|i| !i.preview_local.is_empty()));

    // 3. Removing the first item → the same state and the same bytes.
    assert_eq!(
        toggle(&mut store, &items[0]),
        g["untoggle_state"].as_bool().unwrap(),
        "состояние снятия"
    );
    assert_eq!(
        to_json_indent1(&Value::Object(store)),
        g["raw_after_untoggle"].as_str().unwrap(),
        "favorites.json после снятия"
    );
}
