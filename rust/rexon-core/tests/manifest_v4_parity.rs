//! Golden-сверка порта валидатора manifest v4 с оригиналом
//! `rexon/extensions/manifest_v4.py`.
//!
//! Эталон `golden/manifest_v4_cases.json` снят с самого Python: 28 входных
//! манифестов (валидные + все ветки ошибок) прогнаны через
//! `validate_v4_manifest`, записаны `(ok, msg)`. Rust-порт обязан вернуть те
//! же `ok` и БАЙТ-В-БАЙТ те же тексты ошибок (включая `{x!r}`-repr Python).

//! Второй эталон — `golden/ext_archive_cases.json`: для КАЖДОГО настоящего
//! архива из `golden/` записано, что вернул живой Python-эталон
//! `_py_read_manifest_text` (текст либо тип+сообщение исключения). Порт обязан
//! отдать тот же текст (в т.ч. манифест не в UTF-8 → те же U+FFFD от
//! `errors="replace"`) и отказать теми же причинами.

use std::path::{Path, PathBuf};

use rexon_core::manifest_v4::{read_manifest_text, validate_v4_manifest, ParseError};
use serde_json::Value;

mod i18n_pin;

/// Каждый Python-кейс → тот же вердикт и текст ошибки у Rust-порта.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn validate_matches_python_all_cases() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
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
    // Санити: эталон непустой (иначе тест ничего не проверяет).
    assert_eq!(cases.len(), 28);
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Чтение `manifest.json` из НАСТОЯЩИХ архивов совпадает с Python — включая
/// кривые входы: не-UTF-8 манифест, обрезанный ZIP, архив без манифеста.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn read_manifest_text_matches_python() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
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
            // KeyError у Python → NoManifest; текст в Python обёрнут кавычками
            // (str(KeyError) добавляет repr), сверяем содержимое без них.
            (Some("KeyError"), Err(ParseError::NoManifest(m))) => {
                let want_msg = want["msg"].as_str().unwrap().trim_matches('\'');
                assert_eq!(m, want_msg, "{name}: текст «нет манифеста»");
            }
            // Текст сверяем, а не только тип: он подклеивается к сообщению
            // установщика и доезжает до окна покупателя дословно.
            (Some("BadZipFile"), Err(ParseError::BadZip(m))) => {
                assert_eq!(m, want["msg"].as_str().unwrap(), "{name}: текст «битый архив»");
            }
            _ => panic!("{name}: Python дал {want}, порт — {got:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 7, "все архивы golden/ должны быть в эталоне");
}

/// Не-UTF-8 манифест: `errors="replace"` — байты cp1251 становятся U+FFFD, и
/// такой манифест уже НЕ разбирается как JSON (как и у Python).
#[test]
fn cp1251_manifest_is_replaced_not_rejected() {
    let text = read_manifest_text(&golden_dir().join("manifest_cp1251.rxn")).unwrap();
    assert!(text.contains('\u{fffd}'), "ожидались U+FFFD, получено {text}");
    // Сам JSON при этом валиден (замена попала внутрь строки-значения) —
    // поэтому битую кодировку ловит не парсер, а глаз пользователя в UI.
    let meta: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(meta["format_version"], serde_json::json!(4));
    assert!(meta["title"].as_str().unwrap().contains('\u{fffd}'));
}

/// Английский интерфейс: отказы валидатора v4 — без русских букв.
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
