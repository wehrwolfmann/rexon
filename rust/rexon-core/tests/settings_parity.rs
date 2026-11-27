//! Golden check of the settings port against the original `rexon/settings.py`.
//!
//! Reference strings were captured from Python itself:
//!   json.dumps(d, indent=2, ensure_ascii=False)
//! (see the capture process in the migration history). Any format mismatch is
//! a port bug, not a test bug.

use rexon_core::settings::{defaults, load_settings, serialize_settings, detect_file_manager};
use serde_json::json;

/// Defaults serialise byte for byte like Python.
const GOLDEN_DEFAULTS: &str = "{\n  \"file_manager\": \"auto\",\n  \"theme\": \"auto\",\n  \"auto_backup\": true,\n  \"wizard_done\": false,\n  \"show_console_window\": true,\n  \"ui_skin\": \"steam_dark\"\n}";

/// File {theme:dark, new_key:42} over the defaults: in-place replacement +
/// appending the new key at the end, Cyrillic without escaping.
const GOLDEN_MERGED: &str = "{\n  \"file_manager\": \"auto\",\n  \"theme\": \"тёмная\",\n  \"auto_backup\": true,\n  \"wizard_done\": false,\n  \"show_console_window\": true,\n  \"ui_skin\": \"steam_dark\",\n  \"new_key\": 42\n}";

const GOLDEN_EMPTY: &str = "{}";

#[test]
fn defaults_serialize_byte_identical() {
    assert_eq!(serialize_settings(&defaults()), GOLDEN_DEFAULTS);
}

#[test]
fn empty_serialize_byte_identical() {
    assert_eq!(serialize_settings(&serde_json::Map::new()), GOLDEN_EMPTY);
}

#[test]
fn load_merges_like_python_update() {
    // Write a file with an override + a new key, read, serialise — must
    // match dict(_DEFAULTS).update(file).
    let path = std::env::temp_dir()
        .join(format!("rexon_settings_parity_{}.json", std::process::id()));
    std::fs::write(&path, r#"{"theme": "тёмная", "new_key": 42}"#).unwrap();
    let merged = load_settings(&path);
    let out = serialize_settings(&merged);
    let _ = std::fs::remove_file(&path);
    assert_eq!(out, GOLDEN_MERGED);
}

#[test]
fn load_missing_file_returns_defaults() {
    let missing = std::env::temp_dir().join("rexon_no_such_settings_xyz.json");
    assert_eq!(serialize_settings(&load_settings(&missing)), GOLDEN_DEFAULTS);
}

#[test]
fn detect_file_manager_first_present_wins() {
    // Only thunar and dolphin exist → take the first by priority (thunar).
    let present = |c: &str| matches!(c, "thunar" | "dolphin");
    assert_eq!(detect_file_manager(&present), "thunar");
    // Nothing found → fallback nautilus.
    assert_eq!(detect_file_manager(&|_| false), "nautilus");
    // Priority: nemo before nautilus.
    assert_eq!(detect_file_manager(&|c| c == "nemo" || c == "nautilus"), "nemo");
    let _ = json!({}); // serde_json::json! is available in the test
}
