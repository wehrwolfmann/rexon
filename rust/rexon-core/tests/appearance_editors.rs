//! «Настроить…» у стиля приложений и «Изменить…» / «Создать на основе…» у
//! цветовой схемы — как в модулях KDE «Стиль приложений» и «Цвета».
//! Эталон — `golden/appearance_editors_cases.json`.

use rexon_core::color_scheme_edit as cse;
use rexon_core::themes;
use serde_json::Value;

mod i18n_pin;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/appearance_editors_cases.json")).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

#[test]
fn style_configure_button_follows_config_page() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let pages = themes::style_config_pages(&strings(&g["themerc"]));
    assert_eq!(pages.get("breeze").map(String::as_str), Some("kstyle_config/breezestyleconfig"));
    assert!(!pages.contains_key("windows"), "у стиля без ConfigPage настроек нет");
    for case in g["style_buttons"].as_array().unwrap() {
        let have = strings(&case["have"]);
        let kind = case["kind"].as_str().unwrap();
        let name = case["name"].as_str().unwrap();
        let b = themes::style_config_button(kind, name, &pages, &|p| have.iter().any(|h| h == p));
        let tag = format!("{kind}:{name} {have:?}");
        assert_eq!(b.visible, case["visible"].as_bool().unwrap(), "{tag}");
        assert_eq!(b.enabled, case["enabled"].as_bool().unwrap(), "{tag}");
        assert_eq!(b.argv, strings(&case["argv"]), "{tag}");
        if b.visible {
            assert_eq!(b.text, "Настроить…", "{tag}");
            // Нажать нельзя — причина обязана быть сказана.
            assert!(!b.tooltip.is_empty(), "{tag}: нет подсказки");
        }
    }
}

#[test]
fn style_themerc_dirs_follow_xdg() {
    let dirs = themes::style_themerc_dirs("/home/u", None, None);
    assert_eq!(dirs, ["/home/u/.local/share/kstyle/themes", "/usr/local/share/kstyle/themes", "/usr/share/kstyle/themes"]);
    let dirs = themes::style_themerc_dirs("/home/u", Some("/x"), Some("/a/:/b"));
    assert_eq!(dirs, ["/x/kstyle/themes", "/a/kstyle/themes", "/b/kstyle/themes"]);
}

#[test]
fn kde_editor_argv_matches_kcm_colors() {
    let g = golden();
    for case in g["editor_argv"].as_array().unwrap() {
        let got = cse::kde_editor_argv(
            case["name"].as_str().unwrap(),
            case["user"].as_bool().unwrap(),
            cse::EditMode::from_key(case["mode"].as_str().unwrap()),
            case["has"].as_bool().unwrap(),
        );
        let want = case["argv"].as_array().map(|_| strings(&case["argv"]));
        assert_eq!(got, want, "{case}");
    }
    assert!(cse::is_user_scheme("/home/u/.local/share/color-schemes/A.colors", "/home/u"));
    assert!(!cse::is_user_scheme("/usr/share/color-schemes/A.colors", "/home/u"));
}

#[test]
fn own_editor_reads_and_writes_the_main_roles() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let base = g["scheme_base"].as_str().unwrap();
    let roles = cse::read_roles(base);
    let want = g["scheme_roles"].as_array().unwrap();
    assert_eq!(roles.len(), want.len());
    for (r, w) in roles.iter().zip(want) {
        assert_eq!(r.key, w["key"].as_str().unwrap());
        assert_eq!(r.background, w["background"].as_str().unwrap(), "{}", r.key);
        assert_eq!(r.foreground, w["foreground"].as_str().unwrap(), "{}", r.key);
        assert!(!r.label.is_empty());
    }
    let mut edited = roles.clone();
    for r in &mut edited {
        match r.key.as_str() {
            "selection" => {
                r.background = "#ff0080".into();
                r.foreground = "#000000".into();
            }
            "titlebar" => {
                r.background = "#0a141e".into();
                r.foreground = "#fafafa".into();
            }
            _ => {}
        }
    }
    let built = cse::build_scheme(base, "My Pink", "MyPink", &edited);
    assert_eq!(built, g["scheme_built"].as_str().unwrap());
    assert_eq!(cse::suggested_name("Breeze Dark", cse::EditMode::NewFrom, false), "Breeze Dark (копия)");
    assert_eq!(cse::suggested_name("Mine", cse::EditMode::Edit, true), "Mine");
}

#[test]
fn own_editor_saves_a_new_scheme_and_refuses_clashes() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let tmp = std::env::temp_dir().join(format!("rexon-cse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let home = tmp.join("home");
    let sys = tmp.join("usr/share/color-schemes");
    std::fs::create_dir_all(&sys).unwrap();
    let base = sys.join("BreezeDark.colors");
    std::fs::write(&base, g["scheme_base"].as_str().unwrap()).unwrap();
    let sys_dirs = vec![sys.to_string_lossy().into_owned()];
    let roles = cse::read_roles(g["scheme_base"].as_str().unwrap());
    let home_s = home.to_string_lossy().into_owned();
    let base_s = base.to_string_lossy().into_owned();

    let r = cse::save_scheme(&home_s, &base_s, "Тёмная моя", &roles, None, &sys_dirs);
    assert!(r.ok, "{}", r.message);
    assert!(r.path.ends_with("/.local/share/color-schemes/Тёмнаямоя.colors"), "{}", r.path);
    let text = std::fs::read_to_string(&r.path).unwrap();
    assert!(text.contains("Name=Тёмная моя\n") && !text.contains("Name[ru]"));
    // Второй раз то же имя — отказ, не перезапись.
    let again = cse::save_scheme(&home_s, &base_s, "Тёмная моя", &roles, None, &sys_dirs);
    assert!(!again.ok && again.message.contains("уже есть"), "{}", again.message);
    // «Изменить…» своей схемы под тем же именем — можно.
    let own = cse::save_scheme(&home_s, &r.path, "Тёмная моя", &roles, Some("Тёмнаямоя"), &sys_dirs);
    assert!(own.ok, "{}", own.message);
    // Имя системной схемы — отказ: своя ушла бы в тень системной.
    let clash = cse::save_scheme(&home_s, &base_s, "Breeze Dark", &roles, None, &sys_dirs);
    assert!(!clash.ok);
    let empty = cse::save_scheme(&home_s, &base_s, "  ", &roles, None, &sys_dirs);
    assert!(!empty.ok);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn tile_decides_between_kde_editor_and_own_window() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let base = g["scheme_base"].as_str().unwrap();
    assert_eq!(cse::scheme_of_item("local:widgetstyle:Breeze", "/x"), None);
    assert_eq!(
        cse::scheme_of_item("local:colors:BreezeDark", "/usr/share/color-schemes/BreezeDark"),
        Some(("BreezeDark".into(), "/usr/share/color-schemes/BreezeDark.colors".into()))
    );
    let kde = cse::edit_open("local:colors:Mine", "/home/u/.local/share/color-schemes/Mine", "Mine", "/home/u",
                             cse::EditMode::Edit, true, base).unwrap();
    assert_eq!(kde.mode, "kde");
    assert_eq!(kde.argv, ["kcolorschemeeditor", "Mine", "--overwrite"]);
    // Редактора KDE нет: своё окно; своя схема правится поверх…
    let own = cse::edit_open("local:colors:Mine", "/home/u/.local/share/color-schemes/Mine", "Моя", "/home/u",
                             cse::EditMode::Edit, false, base).unwrap();
    assert_eq!((own.mode.as_str(), own.name.as_str(), own.overwrite.as_str()), ("own", "Моя", "Mine"));
    assert_eq!(own.roles.len(), 6);
    // …а системная и «Создать на основе…» — только новой схемой.
    let sys = cse::edit_open("local:colors:BreezeDark", "/usr/share/color-schemes/BreezeDark", "Breeze, тёмный вариант",
                             "/home/u", cse::EditMode::Edit, false, base).unwrap();
    assert_eq!((sys.name.as_str(), sys.overwrite.as_str()), ("Breeze, тёмный вариант (копия)", ""));
    let new = cse::edit_open("local:colors:Mine", "/home/u/.local/share/color-schemes/Mine", "Моя", "/home/u",
                             cse::EditMode::NewFrom, false, base).unwrap();
    assert_eq!(new.overwrite, "");
}
