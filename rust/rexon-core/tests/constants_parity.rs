//! Golden check of the constants port against the original `rexon/extensions/constants.py`.
//!
//! A byte golden file was captured from Python only for serializable data
//! (`RECOMMENDED_EXTENSIONS`); sets/dicts are unordered and are checked
//! by unit tests on their contents.

mod i18n_pin;

use rexon_core::constants;
use rexon_core::models::to_json_indent1;

/// `RECOMMENDED_EXTENSIONS` serialize byte for byte like Python
/// `json.dumps(..., ensure_ascii=False, indent=1)`: field order, a missing
/// `github_repo` omitted, Cyrillic without escaping.
#[test]
fn recommended_extensions_serialize_byte_identical() {
    let golden = include_str!("golden/recommended_extensions.json");
    let value = serde_json::to_value(constants::recommended_extensions()).unwrap();
    assert_eq!(to_json_indent1(&value), golden);
}

/// Versions.
#[test]
fn versions() {
    assert_eq!(constants::REXON_VERSION, (0, 1, 1));
    assert_eq!(constants::FORMAT_VERSION, 3);
}

/// Type classification matches the Python sets.
#[test]
fn type_classification() {
    assert!(constants::is_valid_type("theme-gtk"));
    assert!(constants::is_valid_type("plugin"));
    assert!(!constants::is_valid_type("theme-pack")); // v2 removed
    assert!(!constants::is_valid_type("nonsense"));

    assert!(constants::is_appearance("wallpaper-video-adaptive"));
    assert!(!constants::is_appearance("plugin"));

    assert!(constants::is_extension("desktop-tool"));
    assert!(!constants::is_extension("theme-gtk"));

    assert!(constants::is_scripted("plugin"));
    assert!(constants::is_scripted("desktop-tool"));
    assert!(!constants::is_scripted("theme-gtk"));

    // Desktop customization types are safe (only copying and
    // standard enable commands), hence in the «Оформление» category.
    assert!(constants::is_valid_type("gnome-shell-extension"));
    assert!(constants::is_appearance("cinnamon-applet"));
    assert!(!constants::is_scripted("kde-kwin-script"));

    // VALID_TYPES = the whole `de_customization::ROWS` matrix (34 decoration types,
    // since 01.11.2026 also Openbox/labwc frames) plus 2 script ones; there is no
    // separate type list any more.
    assert_eq!(constants::valid_types().len(), 36);
    assert_eq!(constants::category_appearance().len(), 34);
    assert_eq!(constants::CATEGORY_EXTENSIONS.len(), 2);
}

/// Install paths are expanded from the user's home; types without an entry in
/// INSTALL_PATHS (wallpaper-pack, script ones) — None.
///
/// ⚠️ THE LIVE ENVIRONMENT MUST NOT LEAK IN HERE. The check used to fake only
/// `HOME`, while [`constants::install_path`] looks at `XDG_CONFIG_HOME`
/// and `XDG_DATA_HOME` first (a fix, see its docstring). While those variables are
/// empty for whoever runs it, the value is derived from `HOME` and everything matches; once
/// they appear, the check turns red without anything being broken. Exactly the same class
/// was already caught in the bridge checks (`c7d5687`). So we take control of all
/// four and put the values WHERE the standard puts them by default:
/// the expectations in the body do not change because of this.
#[test]
fn install_paths() {
    let saved: Vec<(&str, Option<String>)> =
        ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"]
            .iter()
            .map(|k| (*k, std::env::var(k).ok()))
            .collect();
    std::env::set_var("HOME", "/home/tester");
    std::env::set_var("XDG_CONFIG_HOME", "/home/tester/.config");
    std::env::set_var("XDG_DATA_HOME", "/home/tester/.local/share");
    std::env::set_var("XDG_CACHE_HOME", "/home/tester/.cache");
    std::env::set_var("XDG_STATE_HOME", "/home/tester/.local/state");
    assert_eq!(
        constants::install_path("theme-gtk").unwrap().to_str().unwrap(),
        "/home/tester/.themes"
    );
    assert_eq!(
        constants::install_path("theme-kvantum").unwrap().to_str().unwrap(),
        "/home/tester/.config/Kvantum"
    );
    assert_eq!(
        constants::install_path("font-pack").unwrap().to_str().unwrap(),
        "/home/tester/.local/share/fonts"
    );
    // wallpaper-pack and script types are not in INSTALL_PATHS.
    assert!(constants::install_path("wallpaper-pack").is_none());
    assert!(constants::install_path("plugin").is_none());

    // A non-default XDG directory: the path must follow it, not HOME.
    std::env::set_var("XDG_CONFIG_HOME", "/run/cfg");
    std::env::set_var("XDG_DATA_HOME", "/run/data");
    assert_eq!(
        constants::install_path("theme-kvantum").unwrap().to_str().unwrap(),
        "/run/cfg/Kvantum"
    );
    assert_eq!(
        constants::install_path("font-pack").unwrap().to_str().unwrap(),
        "/run/data/fonts"
    );

    for (key, value) in saved {
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }
}

/// Labels, icons, KDE commands, gradients.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn labels_icons_commands() {
    // Captions are English `msgid`s; the Russian in `po/ru.po` is word for word the old one.
    {
        let _lang = i18n_pin::ru();
        assert_eq!(constants::type_label("theme-gtk").as_deref(), Some("Тема GTK"));
        assert_eq!(constants::type_label("plugin").as_deref(), Some("Плагин"));
    }
    {
        let _lang = i18n_pin::en();
        assert_eq!(constants::type_label("theme-gtk").as_deref(), Some("GTK Theme"));
        assert_eq!(constants::type_label("plugin").as_deref(), Some("Plugin"));
    }
    assert_eq!(constants::type_label("nope"), None);

    assert_eq!(constants::type_icon("theme-icons"), Some("📁"));

    assert_eq!(
        constants::kde_apply_command("theme-kde-global"),
        Some("plasma-apply-lookandfeel")
    );
    assert_eq!(constants::kde_apply_command("theme-gtk"), None);

    let grad = constants::type_gradient("theme-gtk").unwrap();
    assert_eq!(grad, [[0.55, 0.24, 0.78], [0.28, 0.10, 0.50]]);
    assert!(constants::type_gradient("wallpaper-pack").is_some());
    assert!(constants::type_gradient("nope").is_none());
}

/// Wallpaper formats and supported file extensions.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn wallpaper_and_file_formats() {
    {
        let _lang = i18n_pin::ru();
        assert_eq!(
            constants::wallpaper_format_label("scene").as_deref(),
            Some("Сцена (Parallax/Particles)")
        );
    }
    {
        let _lang = i18n_pin::en();
        assert_eq!(
            constants::wallpaper_format_label("video").as_deref(),
            Some("Video Wallpapers")
        );
    }
    assert_eq!(constants::wallpaper_format_icon("video"), Some("🎬"));
    assert_eq!(constants::WALLPAPER_FORMATS.len(), 7);

    assert!(constants::SUPPORTED_IMAGE_FORMATS.contains(&".png"));
    assert!(constants::SUPPORTED_VIDEO_FORMATS.contains(&".mp4"));
    assert!(constants::SUPPORTED_FONT_FORMATS.contains(&".pcf.gz"));
    assert!(constants::SUPPORTED_FLASH_FORMATS.contains(&".swf"));
}
