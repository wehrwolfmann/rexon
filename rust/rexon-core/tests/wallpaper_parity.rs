//! Golden-сверка порта `wallpaper.rs` с оригиналом `rexon/wallpaper.py`.
//!
//! Эталон снят прогоном той же формулы нормализации переопределения свойств
//! и того же разбора размеров кадра, что в оригинале, по набору пограничных
//! значений (пусто, `{}`, голое значение, уже завёрнутое, кириллические
//! ключи, не-объект, битый JSON, мусор в размерах). ⛔ Обои не запускались.
//!
//! `golden/wallpaper_cases.json` — снятые значения.

use rexon_core::wallpaper as wp;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/wallpaper_cases.json")).unwrap()
}

#[test]
fn legacy_suffixes_match_original() {
    let g = fx();
    let want: Vec<&str> = g["legacy_suffixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(wp::LEGACY_FILE_SUFFIXES.to_vec(), want);
}

#[test]
fn parent_dir_only_for_legacy_files() {
    // Файл со «старым» расширением — подниматься к каталогу.
    assert!(wp::use_parent_dir("/обои/index.html", true));
    assert!(wp::use_parent_dir("/обои/scene.json", true));
    assert!(wp::use_parent_dir("/обои/видео.mp4", true));
    assert!(wp::use_parent_dir("/обои/app.EXE", true), "регистр не важен");
    // Файл с другим расширением — нет.
    assert!(!wp::use_parent_dir("/обои/preview.png", true));
    // Каталог — никогда, даже если имя оканчивается похоже.
    assert!(!wp::use_parent_dir("/обои/scene.json", false));
    assert!(!wp::use_parent_dir("/обои", false));
}

#[test]
fn user_props_override_matches_original() {
    let g = fx();
    for (raw, want) in g["user_props"].as_object().unwrap() {
        let got = wp::parse_user_props_override(raw);
        match want {
            // Оригинал возвращал None и на пустом значении, и на валидном
            // JSON, который не объект.
            Value::Null => assert_eq!(
                got,
                wp::UserPropsOverride::None,
                "вход {raw:?} должен давать отсутствие переопределения"
            ),
            // Метка генератора для нераспарсенного значения.
            Value::String(s) if s == "<битый>" => assert_eq!(
                got,
                wp::UserPropsOverride::Broken,
                "вход {raw:?} должен читаться как битый"
            ),
            Value::Object(expected) => match got {
                wp::UserPropsOverride::Props(props) => {
                    assert_eq!(&props, expected, "вход {raw:?}")
                }
                other => panic!("вход {raw:?}: ожидались свойства, получено {other:?}"),
            },
            other => panic!("неожиданный эталон {other} для {raw:?}"),
        }
    }
}

/// Голое значение заворачивается, уже завёрнутое не трогается — иначе
/// сохранённые настройки конфигуратора не доедут до рендера.
#[test]
fn user_props_wrapping_is_idempotent() {
    let once = wp::parse_user_props_override(r#"{"скорость": 0.5}"#);
    let wp::UserPropsOverride::Props(props) = once else {
        panic!("ожидались свойства");
    };
    let text = serde_json::to_string(&Value::Object(props.clone())).unwrap();
    let twice = wp::parse_user_props_override(&text);
    assert_eq!(twice, wp::UserPropsOverride::Props(props), "повторный проход");
}

/// Починка при порте: нечисловое значение переменной окружения роняло
/// запуск обоев `ValueError`; здесь оно откатывается к умолчанию.
#[test]
fn mmap_size_survives_garbage() {
    let g = fx();
    let want = g["mmap_size"].as_object().unwrap();

    // Кейсы, где оригинал отработал нормально, — совпадают.
    assert_eq!(wp::mmap_size(Some(""), Some("")), (1920, 1080));
    assert_eq!(want["''/''"], serde_json::json!([1920, 1080]));
    assert_eq!(wp::mmap_size(Some("2560"), Some("1440")), (2560, 1440));
    assert_eq!(want["'2560'/'1440'"], serde_json::json!([2560, 1440]));
    assert_eq!(wp::mmap_size(Some("1920"), Some("")), (1920, 1080));
    assert_eq!(wp::mmap_size(Some(" 800 "), Some("600")), (800, 600));
    assert_eq!(wp::mmap_size(None, None), (1920, 1080));

    // А вот здесь оригинал падал — эталон это и зафиксировал.
    assert_eq!(
        want["'abc'/'1080'"].as_str(),
        Some("<ValueError>"),
        "эталон обязан фиксировать падение оригинала"
    );
    assert_eq!(
        wp::mmap_size(Some("abc"), Some("1080")),
        (1920, 1080),
        "мусор должен откатываться к умолчанию, а не ронять запуск"
    );
    assert_eq!(wp::mmap_size(Some("-5"), Some("1e3")), (-5, 1080));
}

// ── видео-хост: решения, снятые с `rexon/video_wallpaper.py` ───────────────
//
// Эталон — код оригинала (`get_qml_file`, `setup_renderer`, `_as_bool`,
// `_as_float01`, отказ по типу в `_build_ctx_from_dir`). ⛔ Обои не
// запускались, Qt не стартовал: всё здесь — чистые вход → выход.

use serde_json::{json, Map};

#[test]
fn video_qml_file_prefers_offscreen() {
    // Офскрин перекрывает всё: при mmap-выводе окна на экране нет вовсе.
    assert_eq!(wp::video_qml_file(true, true), wp::QML_OFFSCREEN);
    assert_eq!(wp::video_qml_file(true, false), wp::QML_OFFSCREEN);
    assert_eq!(wp::video_qml_file(false, true), wp::QML_WAYLAND);
    assert_eq!(wp::video_qml_file(false, false), wp::QML_X11);
    // ⚠️ Файлов с такими именами в дереве больше нет: Python-хост обоев
    // снят, а Rust-хост рисует одним `WallpaperVideo.qml` с тремя режимами.
    // Константы остались ТОКЕНАМИ решения — их переводит в режим показа
    // `rexon-gui::wallpaper_host::placement`. Значения закреплены здесь,
    // чтобы тихая правка одной из сторон не разъехалась с другой.
    assert_eq!(wp::QML_OFFSCREEN, "VideoWallpaperOffscreen.qml");
    assert_eq!(wp::QML_WAYLAND, "VideoWallpaper.qml");
    assert_eq!(wp::QML_X11, "VideoWallpaperX11.qml");
}

#[test]
fn user_prop_bool_matches_original() {
    // Порт `_as_bool`: bool как есть, число по нулю, строка по списку.
    assert!(wp::as_bool(Some(&json!(true)), false));
    assert!(!wp::as_bool(Some(&json!(false)), true));
    assert!(wp::as_bool(Some(&json!(1)), false));
    assert!(!wp::as_bool(Some(&json!(0)), true));
    assert!(wp::as_bool(Some(&json!(0.5)), false));
    for yes in ["1", "true", "TRUE", " yes ", "on"] {
        assert!(wp::as_bool(Some(&json!(yes)), false), "{yes:?}");
    }
    // Строка вне списка даёт False, а НЕ умолчание — так было в оригинале.
    for no in ["false", "0", "off", "нет", ""] {
        assert!(!wp::as_bool(Some(&json!(no)), true), "{no:?}");
    }
    // Нет свойства / null → умолчание.
    assert!(wp::as_bool(None, true));
    assert!(!wp::as_bool(None, false));
    assert!(wp::as_bool(Some(&Value::Null), true));
}

#[test]
fn user_prop_volume_matches_original() {
    // WE хранит громкость в 0..100, QML ждёт 0..1.
    assert_eq!(wp::as_volume01(Some(&json!(50)), 0.5), 0.5);
    assert_eq!(wp::as_volume01(Some(&json!(100)), 0.5), 1.0);
    assert_eq!(wp::as_volume01(Some(&json!(0.25)), 0.5), 0.25);
    assert_eq!(wp::as_volume01(Some(&json!("75")), 0.5), 0.75);
    // Больше сотни зажимается, отрицательное — тоже.
    assert_eq!(wp::as_volume01(Some(&json!(500)), 0.5), 1.0);
    assert_eq!(wp::as_volume01(Some(&json!(-3)), 0.5), 0.0);
    // Неразобранное → умолчание.
    assert_eq!(wp::as_volume01(None, 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&Value::Null), 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&json!("громко")), 0.42), 0.42);
    assert_eq!(wp::as_volume01(Some(&json!([1, 2])), 0.42), 0.42);
}

#[test]
fn video_context_properties_match_setup_renderer() {
    // Пустые свойства → все умолчания оригинала.
    let empty = Map::new();
    let props = wp::video_context_properties("/видео/клип.mp4", false, &empty, 1920, 1080);
    let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "videoUrl",
            "isWindowed",
            "audioEnabled",
            "audioVolume",
            "loopVideo",
            "renderWidth",
            "renderHeight",
        ],
        "имена и порядок свойств контекста"
    );
    assert_eq!(
        props[0].value,
        wp::QmlValue::LocalFileUrl("/видео/клип.mp4".into())
    );
    assert_eq!(props[1].value, wp::QmlValue::Bool(false));
    assert_eq!(props[2].value, wp::QmlValue::Bool(wp::DEFAULT_AUDIO_ENABLED));
    assert_eq!(props[3].value, wp::QmlValue::Real(wp::DEFAULT_VOLUME));
    assert_eq!(props[4].value, wp::QmlValue::Bool(wp::DEFAULT_LOOP));
    assert_eq!(props[5].value, wp::QmlValue::Int(1920));
    assert_eq!(props[6].value, wp::QmlValue::Int(1080));

    // Формат `defaults_for_js` — `{ключ: {"value": …}}`.
    let wrapped: Map<String, Value> = serde_json::from_value(json!({
        "audioenabled": {"value": "false"},
        "volume": {"value": 20},
        "loopvideo": {"value": 0},
    }))
    .unwrap();
    let props = wp::video_context_properties("/x.mp4", true, &wrapped, 800, 600);
    assert_eq!(props[1].value, wp::QmlValue::Bool(true), "isWindowed");
    assert_eq!(props[2].value, wp::QmlValue::Bool(false), "audioEnabled");
    assert_eq!(props[3].value, wp::QmlValue::Real(0.2), "audioVolume");
    assert_eq!(props[4].value, wp::QmlValue::Bool(false), "loopVideo");

    // Плоские значения (из конфигуратора) читаются так же.
    let flat: Map<String, Value> = serde_json::from_value(json!({
        "audioenabled": false,
        "volume": 100,
    }))
    .unwrap();
    let props = wp::video_context_properties("/x.mp4", false, &flat, 1, 1);
    assert_eq!(props[2].value, wp::QmlValue::Bool(false));
    assert_eq!(props[3].value, wp::QmlValue::Real(1.0));
}

#[test]
fn video_host_takes_only_video() {
    assert_eq!(wp::video_host_reject_reason("video", "/w/project.json"), None);
    let scene = wp::video_host_reject_reason("scene", "/w/project.json").unwrap();
    assert_eq!(
        scene,
        "Обои типа 'scene' рисует движок waywallen, а не этот хост: /w/project.json"
    );
    // Тип не определился вовсе — вопросительный знак, как печатал оригинал.
    let unknown = wp::video_host_reject_reason("", "/w/project.json").unwrap();
    assert!(unknown.starts_with("Обои типа '?' рисует"), "{unknown}");
}

/// Список видео-расширений ОДИН на весь продукт: у видео-хоста своего быть
/// не должно (именно так `.ogv` знал только он).
#[test]
fn video_entry_exts_include_ogv() {
    let exts = rexon_core::user_props::VIDEO_ENTRY_EXTS;
    for want in [
        ".mp4", ".webm", ".mkv", ".avi", ".mov", ".m4v", ".ogv", ".wmv", ".mpg", ".mpeg", ".gif",
    ] {
        assert!(exts.contains(&want), "нет расширения {want}");
    }
    assert_eq!(exts.len(), 11);
}

// ── веб-обои ───────────────────────────────────────────────────────────────
//
// Эталон поведения — удалённый `rexon/web_wallpaper.py` (виден в истории git,
// коммит 2091eac^). Сверялось по нему: правило пустого значения свойства,
// состав заглушек WE, обёртка applyUserProperties и флаг Chromium.

/// ⚠️ Правило, которое легко потерять: свойство БЕЗ значения уезжает в
/// страницу пустым объектом `{}`, а не пропускается. От этого зависит целая
/// ветка кода самих обоев — увидев `{}`, обоина грузит свой запасной файл.
/// Пропусти его — и обои с невыбранной картинкой останутся пустыми.
#[test]
fn web_defaults_keep_empty_property_as_empty_object() {
    // Ограничители r##…##: внутри есть цвет с решёткой, и обычное r#"
    // закрылось бы само на нём.
    let props: serde_json::Map<String, Value> = serde_json::from_str(
        r##"{
            "schemecolor": {"type": "color", "value": "#ff00ff"},
            "картинка":    {"type": "file",  "value": ""},
            "без_значения":{"type": "file"},
            "нуль":        {"type": "file",  "value": null},
            "голое":       7
        }"##,
    )
    .unwrap();
    let out = wp::web_defaults_for_js(&props);

    assert_eq!(out["schemecolor"]["value"], Value::from("#ff00ff"));
    // Голое значение из конфигуратора заворачивается так же, как из
    // project.json: дальше их не различить.
    assert_eq!(out["голое"]["value"], Value::from(7));
    for empty in ["картинка", "без_значения", "нуль"] {
        assert!(out.contains_key(empty), "свойство {empty} пропало целиком");
        assert_eq!(
            out[empty],
            serde_json::json!({}),
            "{empty} обязано быть пустым объектом, а не отсутствовать"
        );
    }
}

#[test]
fn web_defaults_json_is_valid_and_empty_stays_empty() {
    assert_eq!(wp::web_defaults_json(&serde_json::Map::new()), "{}");
    let props: serde_json::Map<String, Value> =
        serde_json::from_str(r#"{"a": {"value": "б"}}"#).unwrap();
    let json = wp::web_defaults_json(&props);
    // Строка уходит в страницу как литерал JS — она обязана разбираться.
    let back: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(back["a"]["value"], Value::from("б"));
}

/// Заглушки WE — не украшение: обоина Мастерской проверяет наличие функции
/// СИНХРОННО при разборе HTML, и без неё её скрипт обрывается целиком.
#[test]
fn we_polyfill_declares_every_global_the_wallpapers_probe() {
    for name in [
        "wallpaperRegisterAudioListener",
        "wallpaperRegisterMediaStatusListener",
        "wallpaperRegisterMediaPlaybackListener",
        "wallpaperRegisterMediaPropertiesListener",
        "wallpaperRegisterMediaThumbnailListener",
        "wallpaperRegisterMediaTimelineListener",
        "wallpaperRequestRandomFileForProperty",
        "wallpaperPropertyListener",
        "wallpaperPluginListener",
    ] {
        assert!(wp::WE_POLYFILL_JS.contains(name), "нет заглушки {name}");
    }
    // Слушателя звука заглушка обязана СОХРАНЯТЬ, а не глотать: обоина
    // перерисовывает холст именно из него.
    assert!(wp::WE_POLYFILL_JS.contains("_rexonAudioListeners.push"));
    // Повторное внедрение безвредно — сценарий ставится и на подкадры.
    assert!(wp::WE_POLYFILL_JS.contains("__rexonWePolyfillInstalled"));
}

#[test]
fn we_ticker_fires_once_with_full_spectrum() {
    assert!(wp::WE_TICKER_JS.contains("__rexonInitialTickFired"));
    assert!(wp::WE_TICKER_JS.contains("Float32Array(128)"));
    // Тик отложен: applyUserProperties обязано успеть примениться раньше.
    assert!(wp::WE_TICKER_JS.contains("50"));
}

#[test]
fn web_apply_defaults_js_survives_wallpaper_without_listener() {
    let js = wp::web_apply_defaults_js("{\"a\":{\"value\":1}}");
    // Без try исключение в подмешанном сценарии оборвало бы и тик звука.
    assert!(js.contains("try"));
    assert!(js.contains("applyUserProperties"));
    assert!(js.contains("{\"a\":{\"value\":1}}"));
}

/// ⛔ `--disable-gpu` тут смертелен: с ним Chromium не сводит холст в граф
/// сцены Qt, и офскрин-кадр содержал бы только DOM.
#[test]
fn web_offscreen_flags_keep_gpu() {
    assert_eq!(wp::WEB_OFFSCREEN_CHROMIUM_FLAGS, "--in-process-gpu");
    assert!(!wp::WEB_OFFSCREEN_CHROMIUM_FLAGS.contains("--disable-gpu"));
}

/// Ссылку нельзя заворачивать в `QUrl::fromLocalFile` — получится
/// «file:///https:/example.com», то есть молча пустая страница.
#[test]
fn web_target_url_distinguishes_link_from_path() {
    use rexon_core::wallpaper::QmlValue;
    let by_name = |props: Vec<wp::QmlContextProperty>, name: &str| {
        props.iter().find(|p| p.name == name).unwrap().value.clone()
    };

    let link = wp::web_context_properties("https://example.com", false, 1920, 1080);
    assert_eq!(
        by_name(link, "targetUrl"),
        QmlValue::Url("https://example.com".into())
    );
    let file_link = wp::web_context_properties("file:///w/index.html", false, 1, 1);
    assert_eq!(
        by_name(file_link, "targetUrl"),
        QmlValue::Url("file:///w/index.html".into())
    );
    let local = wp::web_context_properties("/w/index.html", true, 640, 480);
    assert_eq!(
        by_name(local.clone(), "targetUrl"),
        QmlValue::LocalFileUrl("/w/index.html".into())
    );
    assert_eq!(by_name(local.clone(), "isWindowed"), QmlValue::Bool(true));
    assert_eq!(by_name(local.clone(), "renderWidth"), QmlValue::Int(640));
    assert_eq!(by_name(local, "renderHeight"), QmlValue::Int(480));
}

#[test]
fn web_host_takes_only_web_type() {
    assert!(wp::web_host_reject_reason("web", "/w/project.json").is_none());
    let video = wp::web_host_reject_reason("video", "/w/project.json").unwrap();
    assert!(video.contains("'video'"), "{video}");
    assert!(video.contains("/w/project.json"), "{video}");
    let unknown = wp::web_host_reject_reason("", "/w/project.json").unwrap();
    assert!(unknown.contains("'?'"), "{unknown}");
}

/// Решение «это ссылка» одно на продукт: у веб-хоста своей копии быть не
/// должно. Отличие от `wallpaper_engine::is_url` ровно одно и намеренное —
/// `file://` тоже ссылка, заворачивать его повторно нельзя.
#[test]
fn web_url_check_extends_engine_rule_by_file_scheme() {
    for link in ["http://a", "https://a", "file:///a"] {
        assert!(wp::is_web_url(link), "{link}");
    }
    for path in ["/a/index.html", "a.html", ""] {
        assert!(!wp::is_web_url(path), "{path}");
    }
    // Ровно то же, что решает движок, плюс file://.
    assert_eq!(
        wp::is_web_url("http://a"),
        rexon_core::wallpaper_engine::is_url("http://a")
    );
}

// ── доустановка видеозависимостей ──────────────────────────────────────────
//
// ⛔ Ни одна команда ниже не выполнялась: проверка только СТРОИТ argv.

/// Своей таблицы установки у движка обоев больше нет — отвечает общий движок
/// пакетов. Прежняя копия знала три семейства из одиннадцати и успела с ним
/// разойтись, поэтому сверка идёт по ВСЕМ значениям перечисления, а не по
/// списку из трёх.
#[test]
fn install_deps_argv_asks_the_package_engine() {
    use rexon_core::system::PkgFamily;
    use rexon_core::wallpaper_engine as we;

    for family in [
        "arch", "debian", "fedora", "suse", "gentoo", "alpine", "void", "nix",
        "solus", "clear", "alt", "unknown",
    ] {
        let fam = PkgFamily::from_value(family).unwrap();
        for session in ["wayland", "x11"] {
            let pkgs = we::install_deps_packages(family, session);
            for avail in [false, true] {
                assert_eq!(
                    we::install_deps_argv(family, &pkgs, avail),
                    rexon_core::system_plan::install_argv(fam, &pkgs, avail),
                    "{family}/{session}: движок обоев ответил своё (helper={avail})"
                );
            }
        }
    }
}

/// Ровно те две команды, которые прежняя таблица собирала НЕВЕРНО. Записаны
/// строками, а не выведены из движка: иначе эталон поехал бы вслед за
/// правкой таблицы.
///
/// * pacman без `--needed` переустанавливал уже стоящий mpv — лишняя закачка,
///   повторный прогон хуков и подмена локально собранного пакета репозиторным.
/// * `apt` сам объявляет свой интерфейс нестабильным для скриптов, а без
///   `--no-install-recommends` за mpv приезжает хвост рекомендованных.
#[test]
fn install_deps_argv_fixes_the_two_wrong_commands() {
    use rexon_core::wallpaper_engine as we;

    let arch = we::install_deps_packages("arch", "wayland");
    assert_eq!(arch, vec!["mpv".to_string(), "mpvpaper".to_string()]);
    assert_eq!(
        we::install_deps_argv("arch", &arch, false),
        Some(vec![
            "pkexec".into(), "pacman".into(), "-S".into(), "--needed".into(),
            "--noconfirm".into(), "mpv".into(), "mpvpaper".into(),
        ])
    );
    let debian = we::install_deps_packages("debian", "wayland");
    assert_eq!(
        we::install_deps_argv("debian", &debian, false),
        Some(vec![
            "pkexec".into(), "apt-get".into(), "install".into(), "-y".into(),
            "--no-install-recommends".into(), "mpv".into(),
        ])
    );
}

/// Восемь семейств, которым прежняя таблица отвечала «не знаю» при
/// полностью известном менеджере: кнопка «Установить зависимости» на них
/// просто не работала. Тот же класс, что и почин openSUSE в
/// `youtube_wallpaper` (правило #12).
#[test]
fn install_deps_argv_answers_the_eight_forgotten_families() {
    use rexon_core::wallpaper_engine as we;

    for family in ["suse", "gentoo", "alpine", "void", "nix", "solus", "clear", "alt"] {
        let pkgs = we::install_deps_packages(family, "wayland");
        let argv = we::install_deps_argv(family, &pkgs, false)
            .unwrap_or_else(|| panic!("{family}: движок промолчал"));
        assert!(argv.contains(&"mpv".to_string())
                || argv.contains(&"nixpkgs.mpv".to_string()),
                "{family}: {argv:?}");
    }
}

/// Честное «не знаю» осталось на месте: неопознанное семейство и пустой
/// список пакетов командой не выражаются, вызывающий говорит человеку
/// доустановить руками.
#[test]
fn install_deps_argv_stays_silent_when_there_is_nothing_to_say() {
    use rexon_core::wallpaper_engine as we;

    let pkgs = we::install_deps_packages("arch", "wayland");
    for family in ["unknown", "windows", "", "ARCH"] {
        assert_eq!(we::install_deps_argv(family, &pkgs, false), None, "{family}");
        assert_eq!(we::install_deps_argv(family, &pkgs, true), None, "{family}");
    }
    assert_eq!(we::install_deps_argv("arch", &[], false), None);
    assert_eq!(we::install_deps_argv("arch", &[], true), None);
}
