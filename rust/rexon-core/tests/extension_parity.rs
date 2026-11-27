//! Golden check of the `Extension` port against the original `rexon/extensions/extension.py`.
//!
//! Two goldens were captured from Python itself:
//!   * `golden/extension_get_info_cases.json` — for a set of manifests (plugin/
//!     wallpaper-pack/font-pack, with real files in the directory) the output of
//!     `get_info()` via `json.dumps(..., ensure_ascii=False, indent=1)` was recorded;
//!     the absolute directory prefix is normalized to `DIR`. Among the cases is
//!     the REAL manifest of an extension installed on the machine
//!     (`midnight-office-shift` from store.kde.org) and a `font-pack` with the compound
//!     extension `.pcf.gz` (there `os.path.splitext` gave `.gz` and the font
//!     got lost — fixed on both sides).
//!   * `golden/extension_errors.json` — `ValueError` texts for invalid
//!     manifests (missing required field / unknown type).
//!
//! The Rust port must return BYTE-FOR-BYTE the same `get_info()` serialization (after
//! the same directory normalization) and the same error texts.


mod i18n_pin;

use std::path::Path;

use rexon_core::extension::{Extension, ExtensionError};
use rexon_core::models::to_json_indent1;
use serde_json::{Map, Value};

/// Create a temporary directory and lay out `manifest.json` and the listed files.
fn make_ext_dir(manifest: &Value, files: &[String]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rexon_ext_test_{}_{}",
        std::process::id(),
        // uniqueness across cases within the process
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    for rel in files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, b"").unwrap();
    }
    dir
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `enabled_state` from the golden (object `{id: bool}`) into a `Map`.
fn enabled_map(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn get_info_matches_python_all_cases() {
    // The Python golden remembers Russian type labels — the check runs on the Russian
    // catalog (`po/ru.po`); in the core the labels are English `msgid`s.
    let _lang = i18n_pin::ru();
    let golden: Value =
        serde_json::from_str(include_str!("golden/extension_get_info_cases.json")).unwrap();
    let cases = golden.as_array().expect("массив кейсов");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let manifest = &case["manifest"];
        let files: Vec<String> = case["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let enabled = enabled_map(&case["enabled_state"]);

        let dir = make_ext_dir(manifest, &files);
        let ext = Extension::from_manifest(manifest.clone(), dir.clone(), &enabled)
            .unwrap_or_else(|e| panic!("кейс {name}: манифест не загрузился: {e}"));

        // Our get_info → serialization → directory normalized to DIR.
        let got_raw = to_json_indent1(&Value::Object(ext.get_info()));
        let got = got_raw.replace(&dir.to_string_lossy().into_owned(), "DIR");

        // Golden: the same object from the golden → the same serialization (key
        // order kept thanks to preserve_order).
        let want = to_json_indent1(&case["info"]);

        assert_eq!(got, want, "get_info расходится в кейсе {name}");
        std::fs::remove_dir_all(&dir).ok();
    }
    // 5 synthetic + a real installed extension from this machine
    // (store.kde.org, wallpaper-pack) + a font-pack with the compound `.pcf.gz`.
    assert_eq!(cases.len(), 7);
}

#[test]
fn errors_match_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let golden: Value =
        serde_json::from_str(include_str!("golden/extension_errors.json")).unwrap();
    let cases = golden.as_array().expect("массив кейсов");
    let empty = Map::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let manifest = case["manifest"].clone();
        let want_msg = case["error"]["msg"].as_str().unwrap();

        let err = Extension::from_manifest(manifest, "/tmp/x", &empty)
            .expect_err(&format!("кейс {name}: ожидалась ошибка"));
        assert_eq!(err.to_string(), want_msg, "текст ошибки расходится: {name}");
    }
    assert_eq!(cases.len(), 4);
}

/// `load_from_dir` for a missing manifest gives `ManifestNotFound` with
/// the directory path (like Python's `FileNotFoundError` `f"...не найден в {dir}"`).
#[test]
fn load_from_dir_missing_manifest() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let dir = std::env::temp_dir().join(format!("rexon_ext_nomanifest_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let empty = Map::new();
    let err = Extension::load_from_dir(dir.clone(), &empty).unwrap_err();
    assert!(matches!(err, ExtensionError::ManifestNotFound(_)));
    assert_eq!(
        err.to_string(),
        format!("manifest.json не найден в {}", dir.display())
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// `load_from_dir` reads the manifest from disk and validates it the same way as from memory.
#[test]
fn load_from_dir_roundtrip() {
    let manifest = serde_json::json!({"id": "rt", "name": "RT", "type": "plugin"});
    let files: [String; 0] = [];
    let dir = make_ext_dir(&manifest, &files);
    let empty = Map::new();
    let ext = Extension::load_from_dir(dir.clone(), &empty).unwrap();
    assert_eq!(ext.ext_id(), "rt");
    assert_eq!(ext.ext_type(), "plugin");
    assert!(ext.enabled);
    assert!(!Path::new(&dir).join("preview.png").exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// English interface: manifest errors and default labels carry no
/// Russian letters; Russian stays as before (the checks above run on `po/ru.po`).
#[test]
fn english_errors_and_defaults_have_no_russian() {
    let _lang = i18n_pin::en();
    let empty = Map::new();
    let err = Extension::from_manifest(serde_json::json!({"id": "x", "name": "X", "type": "nope"}), "/tmp/x", &empty)
        .expect_err("неизвестный тип");
    let text = err.to_string();
    assert!(text.starts_with("manifest.json: unknown type 'nope'. Allowed: "), "{text}");
    let err = Extension::from_manifest(serde_json::json!({"name": "X", "type": "plugin"}), "/tmp/x", &empty)
        .expect_err("нет id");
    assert_eq!(err.to_string(), "manifest.json: the required field 'id' is missing");
    assert_eq!(
        ExtensionError::ManifestNotFound("/d".into()).to_string(),
        "manifest.json not found in /d"
    );
    let ext = Extension::from_manifest(serde_json::json!({"id": "x", "name": "X", "type": "plugin"}), "/tmp/x", &empty).unwrap();
    let info = ext.get_info();
    assert_eq!(info["author"], "Unknown");
}

/// In Russian the default labels are as they were.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn russian_defaults_keep_old_texts() {
    let _lang = i18n_pin::ru();
    let empty = Map::new();
    let ext = Extension::from_manifest(serde_json::json!({"id": "x", "name": "X", "type": "plugin"}), "/tmp/x", &empty).unwrap();
    let info = ext.get_info();
    assert_eq!(info["author"], "Неизвестно");
}
