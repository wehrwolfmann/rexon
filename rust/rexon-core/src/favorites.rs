//! Favorite appearance themes — port of `rexon/appearance/favorites.py`.
//!
//! Storage is `~/.config/rexon/favorites.json` in the format
//! `{id: {ThemeItem fields}}` (not just ids, so cards can be shown without
//! another OCS request). The key is `ThemeItem.id` (the OCS content_id). It is
//! written as `json.dump(ensure_ascii=False, indent=1)` → checked byte for byte
//! (golden test `favorites_parity.rs`).
//!
//! The core functions are pure (they work on the store/path passed in) — as in
//! `settings.rs`; thin wrappers take the real path via `default_path()`.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::models::{to_json_indent1, ThemeItem};

/// Path to the favorites store: `~/.config/rexon/favorites.json`.
///
/// Warning — fix: the path used to be built from bare `$HOME`, bypassing
/// `XDG_CONFIG_HOME`. The interface meanwhile read the XDG-aware path
/// (`rexon/appearance/favorites.py` builds it via `expand_user`), and with a
/// non-standard `XDG_CONFIG_HOME` the favorites SPLIT IN TWO: the tab showed
/// one file, the core wrote another, and stars vanished out of nowhere. Such an
/// environment is not exotic: Flatpak, the Steam runtime and simply a carefully
/// configured profile all set it. The path is now computed the same way as all
/// other user paths — through the single point
/// [`crate::utils::rexon_config_dir`].
pub fn default_path() -> PathBuf {
    crate::utils::rexon_config_dir().join("favorites.json")
}

/// Load the raw store `{id: {…}}`. Any read/parse error, or a non-object at the
/// top level → an empty store (like `except (OSError, JSONDecodeError)` and the
/// `isinstance(data, dict)` check in Python). Values are kept as is — Python
/// does not normalize them on load.
pub fn load_raw(path: &Path) -> Map<String, Value> {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ => Map::new(),
        },
        Err(_) => Map::new(),
    }
}

/// Save the store: mkdir the parent, then the shared door
/// [`crate::utils::atomic_write`] with `indent=1`. A directory/write error is
/// returned (Python logs a warning and silently carries on).
pub fn save_raw(path: &Path, store: &Map<String, Value>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = to_json_indent1(&Value::Object(store.clone()));
    crate::utils::atomic_write(&path.to_string_lossy(), &data)
}

/// Whether an item is a favorite: a non-empty id present in the store.
pub fn is_favorite(store: &Map<String, Value>, item_id: &str) -> bool {
    !item_id.is_empty() && store.contains_key(item_id)
}

/// Add/remove an item in the store (in place). Returns the new state (`true` =
/// now a favorite). An empty id — no change, `false`.
///
/// For OCS themes `preview_local` (a cache path — it may go stale) is cleared;
/// the preview is re-fetched from `preview_url`. For LOCAL themes (id `local:…`)
/// `preview_local` is a stable path to a file on disk and `preview_url` is empty
/// → we KEEP it, otherwise a local theme would sit in "Favorites" without a
/// preview.
pub fn toggle(store: &mut Map<String, Value>, item: &ThemeItem) -> bool {
    if item.id.is_empty() {
        return false;
    }
    if store.contains_key(&item.id) {
        // Warning: it is `shift_remove`, NOT `remove`: with the `preserve_order`
        // feature enabled the serde map is an IndexMap, and `remove` == `swap_remove`,
        // i.e. the LAST element moves into the removed one's place. Python's
        // `del data[id]` keeps the order of the rest, and the key order of
        // favorites.json is the order of cards in "⭐ Favorites": with `swap_remove`,
        // after unstarring one theme the last favorite jumped into its place (and the
        // file diverged from Python byte for byte).
        store.shift_remove(&item.id);
        return false;
    }
    let mut entry = serde_json::to_value(item).expect("ThemeItem сериализуется");
    if !item.id.starts_with("local:") {
        if let Value::Object(ref mut obj) = entry {
            obj.insert("preview_local".to_string(), Value::from(""));
        }
    }
    store.insert(item.id.clone(), entry);
    true
}

/// All favorites as `ThemeItem` (for the grid of the "Favorites" category).
///
/// serde drops unknown keys and missing ones take their default — the analogue
/// of `ThemeItem(**{k: v … if k in _FIELDS})`. An entry that cannot be parsed
/// into a `ThemeItem` is skipped. For local themes (id `local:…`) we restore the
/// runtime attribute `local_kind = "downloaded"` (it is not serialized) so the
/// card is shown as local rather than as an OCS product.
pub fn list_items(store: &Map<String, Value>) -> Vec<ThemeItem> {
    let mut out = Vec::new();
    for value in store.values() {
        let Ok(mut item) = serde_json::from_value::<ThemeItem>(value.clone()) else {
            continue;
        };
        if item.id.starts_with("local:") {
            item.local_kind = Some("downloaded".to_string());
        }
        out.push(item);
    }
    out
}
