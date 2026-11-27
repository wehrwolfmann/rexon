//! Golden check of the theme-apply commands against the original `rexon/extensions/api.py`.
//!
//! The golden `golden/api_theme_commands.json` was captured from Python itself: `safe_run`
//! was monkeypatched on a real `ExtensionAPI`, `apply_gtk_theme`/
//! `apply_icon_theme` were called for a set of DEs, and the passed argv was recorded (or `null` if
//! no command is built). The Rust port must return the same argv (or `None`).

//! The second golden is `golden/ext_api_copy_cases.json`: the copy target of a look
//! directory (`copy_theme_files`/`copy_icon_files`), captured from the reference
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
    // Sanity: gsettings DEs, xfce and "no command" are covered (7 DEs × 2 kinds).
    assert_eq!(cases.len(), 14);
}

/// The copy target matches Python, trailing slashes included. An empty name
/// (`src_dir == "/"` or `""`) is `None`: the original got the target directory
/// ITSELF there and went on to delete it (wiping all of the user's themes).
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
