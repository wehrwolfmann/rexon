//! Golden-сверка команд применения темы с оригиналом `rexon/extensions/api.py`.
//!
//! Эталон `golden/api_theme_commands.json` снят с самого Python: `safe_run`
//! замонипатчен на реальном `ExtensionAPI`, вызваны `apply_gtk_theme`/
//! `apply_icon_theme` для набора DE, записан переданный argv (или `null`, если
//! команда не строится). Rust-порт обязан вернуть тот же argv (или `None`).

//! Второй эталон — `golden/ext_api_copy_cases.json`: цель копирования каталога
//! оформления (`copy_theme_files`/`copy_icon_files`), снятая с эталонного
//! `ExtensionAPI._py_copy_target`.

use rexon_core::ext_api::{copy_target, theme_apply_command, ThemeKind};
use serde_json::Value;

#[test]
fn theme_commands_match_python() {
    let golden: Value =
        serde_json::from_str(include_str!("golden/api_theme_commands.json")).unwrap();
    let cases = golden.as_array().expect("массив кейсов");
    for case in cases {
        let de = case["de"].as_str().unwrap();
        let kind = match case["kind"].as_str().unwrap() {
            "gtk" => ThemeKind::Gtk,
            "icon" => ThemeKind::Icon,
            other => panic!("неизвестный kind: {other}"),
        };
        let name = case["name"].as_str().unwrap();

        let got = theme_apply_command(de, kind, name);
        let want: Option<Vec<String>> = match &case["argv"] {
            Value::Null => None,
            Value::Array(a) => Some(
                a.iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect(),
            ),
            other => panic!("argv не массив/null: {other}"),
        };
        assert_eq!(got, want, "argv расходится: de={de}, kind={:?}", kind);
    }
    // Санити: покрыты gsettings-DE, xfce и «нет команды» (7 DE × 2 вида).
    assert_eq!(cases.len(), 14);
}

/// Цель копирования совпадает с Python, включая хвостовые слэши. Пустое имя
/// (`src_dir == "/"` или `""`) — `None`: у оригинала там получался САМ
/// каталог-приёмник и он шёл его сносить (снос всех тем пользователя).
#[test]
fn copy_target_matches_python() {
    let golden: Value =
        serde_json::from_str(include_str!("golden/ext_api_copy_cases.json")).unwrap();
    let cases = golden.as_array().expect("массив кейсов");
    let mut refused = 0;
    for case in cases {
        let base = case["base_dir"].as_str().unwrap();
        let src = case["src_dir"].as_str().unwrap();
        let want = case["target"].as_str().map(str::to_string);
        if want.is_none() {
            refused += 1;
        }
        assert_eq!(copy_target(base, src), want, "цель расходится: base={base}, src={src}");
    }
    assert_eq!(cases.len(), 8);
    assert_eq!(refused, 2, "кейсы мусорного src_dir обязаны быть в эталоне");
}
