//! Сверка цветов схемы KDE, деления на светлые и тёмные и тона стилей Plasma
//! (`rexon_core::local_preview::scheme_look` и соседи) с замером на самой KDE.
//!
//! `golden/scheme_look_cases.json` снят программой на `KColorScheme`
//! (KF6ColorScheme 6.31): палитра схемы, цвет и текст заголовка, яркость фона
//! окна (`qGray`) и признак «тёмная» (меньше 192).
//!   * `synthetic` — краевые случаи: пустой файл, `#hex`, четыре части,
//!     пробелы, часть вне 0..=255, две части, повтор ключа, заголовок из
//!     `[Colors:Header]` / `[WM]` / выделения, граница 191/192;
//!   * `real` — 51 настоящая схема; тексты берутся из
//!     `local_preview_cases.json` (`parse_colors_real`) по имени файла;
//!   * `plasma_styles` — стили Plasma машины, где снимали: у стиля без файла
//!     `colors` тон «берёт цвета схемы».

use rexon_core::local_preview as lp;
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/scheme_look_cases.json")).unwrap()
}

fn rgb(v: &Value) -> (u8, u8, u8) {
    let a = v.as_array().unwrap();
    (a[0].as_u64().unwrap() as u8, a[1].as_u64().unwrap() as u8, a[2].as_u64().unwrap() as u8)
}

fn check(label: &str, text: &str, want: &Value) {
    let got = lp::scheme_look(text);
    let fields = [
        ("window", got.window),
        ("window_text", got.window_text),
        ("view", got.view),
        ("view_text", got.view_text),
        ("selection", got.selection),
        ("selection_text", got.selection_text),
        ("button", got.button),
        ("button_text", got.button_text),
        ("title", got.title),
        ("title_text", got.title_text),
    ];
    for (name, value) in fields {
        assert_eq!(value, rgb(&want[name]), "{label}: {name}");
    }
    assert_eq!(lp::q_gray(got.window) as u64, want["gray"].as_u64().unwrap(), "{label}: gray");
    let dark = want["dark"].as_bool().unwrap();
    assert_eq!(lp::is_dark(&got), dark, "{label}: dark");
    let tone = if dark { lp::Tone::Dark } else { lp::Tone::Light };
    assert_eq!(lp::tone_of_text(text), tone, "{label}: tone");
}

#[test]
fn synthetic_cases_match_kde() {
    let g = golden();
    let cases = g["synthetic"].as_array().unwrap();
    assert!(cases.len() >= 15);
    for case in cases {
        check(case["label"].as_str().unwrap(), case["text"].as_str().unwrap(), case);
    }
}

#[test]
fn real_schemes_match_kde() {
    let g = golden();
    let texts: Value = serde_json::from_str(include_str!("golden/local_preview_cases.json")).unwrap();
    let texts = texts["parse_colors_real"].as_array().unwrap();
    let real = g["real"].as_array().unwrap();
    assert_eq!(real.len(), texts.len());
    let (mut dark, mut light) = (0, 0);
    for want in real {
        let file = want["file"].as_str().unwrap();
        let text = texts.iter().find(|t| t["file"] == want["file"]).unwrap()["text"].as_str().unwrap();
        check(file, text, want);
        if want["dark"].as_bool().unwrap() {
            dark += 1;
        } else {
            light += 1;
        }
    }
    assert!(dark >= 10 && light >= 10, "в эталоне должны быть и светлые, и тёмные: {light}/{dark}");
}

#[test]
fn plasma_style_tones_match_kde() {
    let g = golden();
    let root = std::env::temp_dir().join(format!("rexon-scheme-look-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut seen_follows = false;
    for style in g["plasma_styles"].as_array().unwrap() {
        let name = style["name"].as_str().unwrap();
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("metadata.json"), "{}").unwrap();
        if let Some(colors) = style["colors"].as_str() {
            std::fs::write(dir.join("colors"), colors).unwrap();
        }
        let want = style["tone"].as_str().unwrap();
        seen_follows |= want == "follows";
        assert_eq!(lp::plasma_style_tone(&dir).key(), want, "стиль {name}");
        let id = format!("local:plasma:{name}");
        let path = dir.to_string_lossy();
        assert_eq!(lp::item_tone(&id, &path).map(|t| t.key()), Some(want), "стиль {name}");
        // Подпись «Поддерживает цветовые схемы» — ровно у стилей без своих
        // цветов, и не тогда, когда отобраны только они.
        assert_eq!(lp::follows_scheme_badge(&id, &path, lp::TONE_ALL).is_some(), want == "follows", "{name}");
        assert!(lp::follows_scheme_badge(&id, &path, lp::TONE_FOLLOWS).is_none(), "{name}");
        for filter in [lp::TONE_LIGHT, lp::TONE_DARK, lp::TONE_FOLLOWS] {
            assert_eq!(lp::tone_passes(&id, &path, filter), filter == want, "{name} при {filter}");
        }
        assert!(lp::tone_passes(&id, &path, lp::TONE_ALL));
    }
    assert!(seen_follows, "в эталоне нет стиля без своих цветов");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scheme_files_filter_by_tone() {
    let g = golden();
    let root = std::env::temp_dir().join(format!("rexon-scheme-filter-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for case in g["synthetic"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        std::fs::write(root.join(format!("{label}.colors")), case["text"].as_str().unwrap()).unwrap();
        // Скан отдаёт путь схемы без окончания `.colors`.
        let path = root.join(label).to_string_lossy().into_owned();
        let id = format!("local:colors:{label}");
        let dark = case["dark"].as_bool().unwrap();
        assert_eq!(lp::tone_passes(&id, &path, lp::TONE_DARK), dark, "{label}");
        assert_eq!(lp::tone_passes(&id, &path, lp::TONE_LIGHT), !dark, "{label}");
        assert!(!lp::tone_passes(&id, &path, lp::TONE_FOLLOWS), "{label}");
        assert!(lp::follows_scheme_badge(&id, &path, lp::TONE_ALL).is_none());
    }
    // Тема другого вида тона не имеет: при выбранном фильтре её не видно, при
    // «Все» — видно.
    assert!(lp::tone_passes("local:gtk:Adwaita", "/nowhere", lp::TONE_ALL));
    assert!(!lp::tone_passes("local:gtk:Adwaita", "/nowhere", lp::TONE_DARK));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn filter_bar_and_chips() {
    assert!(lp::tone_bar_applies("colors", true));
    assert!(lp::tone_bar_applies("plasma", true));
    assert!(!lp::tone_bar_applies("colors", false), "в магазине тон неизвестен");
    assert!(!lp::tone_bar_applies("gtk", true));
    let keys = |k: &str| lp::tone_chips(k).iter().map(|c| c.key).collect::<Vec<_>>();
    assert_eq!(keys("colors"), ["all", "light", "dark"]);
    assert_eq!(keys("plasma"), ["all", "light", "dark", "follows"]);
    assert!(keys("icons").is_empty());
    assert_eq!(lp::tone_filter("colors", "follows"), "all", "у схем такого фильтра нет");
    assert_eq!(lp::tone_filter("plasma", "follows"), "follows");
    assert_eq!(lp::tone_filter("colors", "мусор"), "all");
}
