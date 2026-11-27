//! Golden check of the manifest v4 validator port against the original
//! `rexon/extensions/manifest_v4.py`.
//!
//! The golden `golden/manifest_v4_cases.json` was captured from Python itself: 28 input
//! manifests (valid ones + all error branches) were run through
//! `validate_v4_manifest`, `(ok, msg)` were recorded. The Rust port must return the same
//! `ok` and BYTE-FOR-BYTE the same error texts (including Python's `{x!r}` repr).

//! The second golden is `golden/ext_archive_cases.json`: for EVERY real
//! archive in `golden/` it records what the live Python reference
//! `_py_read_manifest_text` returned (a text or an exception type+message). The port must
//! give the same text (including a manifest not in UTF-8 → the same U+FFFD from
//! `errors="replace"`) and refuse for the same reasons.

use std::path::{Path, PathBuf};

use rexon_core::manifest_v4::{read_manifest_text, validate_v4_manifest, ParseError};
use serde_json::Value;

mod i18n_pin;

/// Every Python case → the same verdict and error text from the Rust port.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn validate_matches_python_all_cases() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let golden: Value = serde_json::from_str(include_str!("golden/manifest_v4_cases.json")).unwrap();
    let cases = golden.as_array().expect("массив кейсов");
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let meta = &case["meta"];
        let want_ok = case["ok"].as_bool().unwrap();
        let want_msg = case["msg"].as_str().unwrap();

        let (ok, msg) = validate_v4_manifest(meta);
        assert_eq!(ok, want_ok, "ok расходится в кейсе {name}");
        assert_eq!(msg, want_msg, "текст ошибки расходится в кейсе {name}");
    }
    // Sanity: the golden is not empty (otherwise the test checks nothing).
    assert_eq!(cases.len(), 28);
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Reading `manifest.json` from REAL archives matches Python — including
/// malformed inputs: a non-UTF-8 manifest, a truncated ZIP, an archive without a manifest.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn read_manifest_text_matches_python() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
    let golden: Value =
        serde_json::from_str(include_str!("golden/ext_archive_cases.json")).unwrap();
    let mut checked = 0;
    for (name, case) in golden.as_object().unwrap() {
        let want = &case["read_manifest_text"];
        let got = read_manifest_text(&golden_dir().join(name));
        match (want["error"].as_str(), &got) {
            (None, Ok(text)) => {
                assert_eq!(text, want["text"].as_str().unwrap(), "{name}: текст манифеста");
            }
            // Python's KeyError → NoManifest; in Python the text is wrapped in quotes
            // (str(KeyError) adds the repr), we compare the content without them.
            (Some("KeyError"), Err(ParseError::NoManifest(m))) => {
                let want_msg = want["msg"].as_str().unwrap().trim_matches('\'');
                assert_eq!(m, want_msg, "{name}: текст «нет манифеста»");
            }
            // We compare the text, not only the type: it is appended to the installer's
            // message and reaches the buyer's window verbatim.
            (Some("BadZipFile"), Err(ParseError::BadZip(m))) => {
                assert_eq!(m, want["msg"].as_str().unwrap(), "{name}: текст «битый архив»");
            }
            _ => panic!("{name}: Python дал {want}, порт — {got:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 7, "все архивы golden/ должны быть в эталоне");
}

/// A non-UTF-8 manifest: `errors="replace"` — cp1251 bytes become U+FFFD, and
/// such a manifest is NO longer parsed as JSON (just like in Python).
#[test]
fn cp1251_manifest_is_replaced_not_rejected() {
    let text = read_manifest_text(&golden_dir().join("manifest_cp1251.rxn")).unwrap();
    assert!(text.contains('\u{fffd}'), "ожидались U+FFFD, получено {text}");
    // The JSON itself is valid (the replacement landed inside a value string) —
    // so the broken encoding is caught not by the parser but by the user's eye in the UI.
    let meta: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(meta["format_version"], serde_json::json!(4));
    assert!(meta["title"].as_str().unwrap().contains('\u{fffd}'));
}

/// English interface: the v4 validator's refusals carry no Russian letters.
#[test]
fn english_validation_errors_have_no_russian() {
    let _lang = i18n_pin::en();
    let golden: Value = serde_json::from_str(include_str!("golden/manifest_v4_cases.json")).unwrap();
    for case in golden.as_array().unwrap() {
        let (_, msg) = validate_v4_manifest(&case["meta"]);
        assert!(
            !msg.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
            "{}: {msg}",
            case["name"]
        );
    }
    let (_, msg) = validate_v4_manifest(&serde_json::json!({"format_version": 3}));
    assert_eq!(msg, "format_version=3, expected 4");
}
