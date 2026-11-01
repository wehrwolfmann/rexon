//! Golden-сверка порта констант с оригиналом `rexon/extensions/constants.py`.
//!
//! Байтовый эталон снят с Python только для сериализуемых данных
//! (`RECOMMENDED_EXTENSIONS`); множества/словари неупорядочены и проверяются
//! юнит-тестами по содержимому.

mod i18n_pin;

use rexon_core::constants;
use rexon_core::models::to_json_indent1;

/// `RECOMMENDED_EXTENSIONS` сериализуются байт-в-байт как Python
/// `json.dumps(..., ensure_ascii=False, indent=1)`: порядок полей, пропуск
/// отсутствующего `github_repo`, кириллица без экранирования.
#[test]
fn recommended_extensions_serialize_byte_identical() {
    let golden = include_str!("golden/recommended_extensions.json");
    let value = serde_json::to_value(constants::recommended_extensions()).unwrap();
    assert_eq!(to_json_indent1(&value), golden);
}

/// Версии.
#[test]
fn versions() {
    assert_eq!(constants::REXON_VERSION, (0, 1, 0));
    assert_eq!(constants::FORMAT_VERSION, 3);
}

/// Классификация типов совпадает с Python-множествами.
#[test]
fn type_classification() {
    assert!(constants::is_valid_type("theme-gtk"));
    assert!(constants::is_valid_type("plugin"));
    assert!(!constants::is_valid_type("theme-pack")); // v2 убран
    assert!(!constants::is_valid_type("nonsense"));

    assert!(constants::is_appearance("wallpaper-video-adaptive"));
    assert!(!constants::is_appearance("plugin"));

    assert!(constants::is_extension("desktop-tool"));
    assert!(!constants::is_extension("theme-gtk"));

    assert!(constants::is_scripted("plugin"));
    assert!(constants::is_scripted("desktop-tool"));
    assert!(!constants::is_scripted("theme-gtk"));

    // Типы кастомизации окружений — безопасные (только копирование и
    // штатные команды включения), поэтому в категории «Оформление».
    assert!(constants::is_valid_type("gnome-shell-extension"));
    assert!(constants::is_appearance("cinnamon-applet"));
    assert!(!constants::is_scripted("kde-kwin-script"));

    // VALID_TYPES = вся матрица `de_customization::ROWS` (34 типа оформления,
    // с 01.11.2026 и рамки Openbox/labwc) плюс 2 скриптовых; отдельного
    // списка типов больше нет.
    assert_eq!(constants::valid_types().len(), 36);
    assert_eq!(constants::category_appearance().len(), 34);
    assert_eq!(constants::CATEGORY_EXTENSIONS.len(), 2);
}

/// Пути установки раскрываются от дома пользователя; типов без записи в
/// INSTALL_PATHS (wallpaper-pack, скриптовые) — None.
///
/// ⚠️ ЖИВОГО ОКРУЖЕНИЯ ЗДЕСЬ БЫТЬ НЕ ДОЛЖНО. Сверка подменяла только
/// `HOME`, а [`constants::install_path`] сперва смотрит `XDG_CONFIG_HOME`
/// и `XDG_DATA_HOME` (починка, см. её докстринг). Пока эти переменные у
/// запускающего пусты, значение считается от `HOME` и всё сходится; стоит
/// им появиться — и сверка краснеет, ничего не сломав. Ровно тот же класс
/// уже ловили на сверках моста (`c7d5687`). Поэтому берём под контроль всю
/// четвёрку и кладём значения ТУДА, куда их по умолчанию кладёт стандарт:
/// ожидания в теле от этого не меняются.
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
    // wallpaper-pack и скриптовые типы — не в INSTALL_PATHS.
    assert!(constants::install_path("wallpaper-pack").is_none());
    assert!(constants::install_path("plugin").is_none());

    // XDG-каталог не по умолчанию: путь обязан уехать за ним, а не за HOME.
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

/// Метки, иконки, KDE-команды, градиенты.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn labels_icons_commands() {
    // Подписи — английские `msgid`, русский — в `po/ru.po` дословно прежний.
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

/// Форматы обоев и поддерживаемые расширения файлов.
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
