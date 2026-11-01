//! Golden-сверка порта `utils.rs` с оригиналом `rexon/utils.py`.
//!
//! `utils` — ЛИСТ, от которого зависит почти весь остальной код (пути
//! пользователя, поиск бинарей, D-Bus), поэтому эталон снят на РЕАЛЬНОЙ
//! машине разработки (CachyOS/KDE, Plasma 6.7): реальные бинари из PATH,
//! реальный ответ живой сессионной шины, реальные пути с кириллицей
//! (`~/Загрузки/…` — штатный рабочий каталог) и с пробелами.
//!
//! Эталон снимался с Python-реализации ДО её удаления, а не через мост —
//! иначе сверка сравнивала бы ядро с самим собой.
//!
//! Два места, где ядро НАМЕРЕННО отличается от оригинала, и это проверяется
//! отдельными тестами, а не замазывается:
//!   • `$XDG_*_HOME` с хвостовым `/` приводится к виду без него (в Python
//!     значение отдавалось как есть, и склейка давала `//`);
//!   • право на запуск проверяется по биту `x`, а не вызовом `access(2)`
//!     (у ядра нет libc); на бинарях дистрибутива это то же самое.

mod i18n_pin;

use rexon_core::utils;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/utils_cases.json")).unwrap()
}

/// PATH, при котором снимался эталон: со «своим» PATH тест нашёл бы тот же
/// бинарь в другом каталоге и развалился бы на пустом месте.
fn golden_path(g: &Value) -> String {
    g["path_env"].as_str().unwrap().to_string()
}

fn opt_str(v: &Value) -> Option<&str> {
    v.as_str()
}

// ============================================================================
// Журналирование и переключатель бэкенда
// ============================================================================

#[test]
fn logging_settings_match_original() {
    let g = fx();
    assert_eq!(utils::LOGGER_NAME, g["logger_name"].as_str().unwrap());
    assert_eq!(utils::LOG_FORMAT, g["log_format"].as_str().unwrap());
    assert_eq!(utils::LOG_DATE_FORMAT, g["log_datefmt"].as_str().unwrap());
    assert_eq!(i64::from(utils::LEVEL_DEBUG), g["file_level"].as_i64().unwrap());
    assert_eq!(i64::from(utils::LEVEL_DEBUG), g["root_level"].as_i64().unwrap());
    assert_eq!(
        i64::from(utils::LEVEL_WARNING),
        g["stream_level"].as_i64().unwrap()
    );

    // Место журнала с 08.10.2026 расходится с Python-эталоном НАМЕРЕННО:
    // журнал — состояние, а не настройка (`$XDG_STATE_HOME`), и Python-окно
    // уже в архиве. Сверяем с новым правилом, а не с эталоном.
    assert_eq!(utils::log_dir("/home/u"), "/home/u/.local/state/rexon");
    assert_eq!(utils::log_file("/home/u"), "/home/u/.local/state/rexon/rexon.log");
    assert_eq!(utils::log_dir_xdg(Some("/x/state/"), "/home/u"), "/x/state/rexon");
    assert_eq!(utils::log_dir_xdg(Some(""), "/home/u"), "/home/u/.local/state/rexon");
}

#[test]
fn run_defaults_match_original() {
    let g = fx();
    let timeout = g["default_timeout"].as_u64().unwrap();
    assert_eq!(utils::DEFAULT_TIMEOUT_SECS, timeout);

    // Захват вывода включает текстовый режим; без него — нет.
    let quick = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let with_capture = utils::run_defaults(&quick(&["pacman", "-Q", "mate"]), true, false);
    assert!(with_capture.text_mode);
    assert_eq!(with_capture.timeout_secs, Some(timeout));

    let plain = utils::run_defaults(&quick(&["gsettings", "get", "a", "b"]), false, false);
    assert!(!plain.text_mode);
    assert_eq!(plain.timeout_secs, Some(timeout));

    // Свой таймаут вызывающего не перебивается.
    let explicit = utils::run_defaults(&quick(&["pacman", "-Q"]), true, true);
    assert!(explicit.text_mode);
    assert_eq!(explicit.timeout_secs, None);
}

/// Классы времени работы: 30 секунд на опрос, но не на установку и не на
/// сборку. Дефект 2026-09-01: установка стола MATE звала `yay -S` с девятью
/// пакетами и получала «таймаут (30 сек)» — все девять «не установились».
#[test]
fn run_duration_by_meaning_of_command() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    use rexon_core::utils::RunDuration::*;

    // Опрос — быстрый, даже у помощника AUR.
    assert_eq!(utils::run_duration(&a(&["pacman", "-Q", "mate"])), Quick);
    assert_eq!(utils::run_duration(&a(&["pacman", "-Qi", "mate-tweak"])), Quick);
    assert_eq!(utils::run_duration(&a(&["pacman", "-Ss", "yaru"])), Quick);
    assert_eq!(utils::run_duration(&a(&["systemctl", "is-enabled", "sddm"])), Quick);
    assert_eq!(utils::run_duration(&a(&["gsettings", "get", "org.mate.interface", "gtk-theme"])), Quick);
    assert_eq!(utils::run_duration(&a(&["yay", "-Q", "mate-tweak"])), Quick);

    // Скачивание из репозиториев.
    assert_eq!(utils::run_duration(&a(&["pacman", "-S", "--noconfirm", "mate"])), Download);
    assert_eq!(utils::run_duration(&a(&["pacman", "-Syu"])), Download);
    assert_eq!(utils::run_duration(&a(&["pkexec", "pacman", "-S", "mate"])), Download);
    assert_eq!(utils::run_duration(&a(&["apt-get", "install", "-y", "mate-desktop-environment"])), Download);
    assert_eq!(utils::run_duration(&a(&["flatpak", "install", "-y", "org.kde.kate"])), Download);
    assert_eq!(utils::run_duration(&a(&["git", "clone", "--depth", "1", "https://example/x"])), Download);
    // Наши привилегированные обёртки: за именем скрипта — установка стола.
    assert_eq!(utils::run_duration(&a(&["pkexec", "bash", "/tmp/rexon_install_ab12.sh"])), Download);
    assert_eq!(utils::run_duration(&a(&["pkexec", "/usr/local/lib/rexon-de-helper", "mate", "arch", "/tmp/x.log"])), Download);

    // Сборка из исходников.
    assert_eq!(
        utils::run_duration(&a(&[
            "yay", "-S", "--needed", "--noconfirm", "mate-tweak", "brisk-menu"
        ])),
        Build
    );
    assert_eq!(utils::run_duration(&a(&["paru", "-S", "vala-panel-appmenu"])), Build);
    assert_eq!(utils::run_duration(&a(&["makepkg", "-si"])), Build);
    assert_eq!(utils::run_duration(&a(&["emerge", "mate-base/mate"])), Build);

    // Таймауты классов различаются и растут в нужную сторону.
    assert_eq!(utils::default_timeout_secs(&a(&["pacman", "-Q"])), utils::QUICK_TIMEOUT_SECS);
    assert_eq!(
        utils::default_timeout_secs(&a(&["pacman", "-S", "mate"])),
        utils::DOWNLOAD_TIMEOUT_SECS
    );
    assert_eq!(
        utils::default_timeout_secs(&a(&["yay", "-S", "mate-tweak"])),
        utils::BUILD_TIMEOUT_SECS
    );
    assert!(utils::QUICK_TIMEOUT_SECS < utils::DOWNLOAD_TIMEOUT_SECS);
    assert!(utils::DOWNLOAD_TIMEOUT_SECS < utils::BUILD_TIMEOUT_SECS);
}

#[test]
fn messages() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        utils::msg_timeout("pacman", "30"),
        "Команда pacman: таймаут (30 сек)"
    );
    assert_eq!(
        utils::msg_run_failed("['ls']", "нет доступа"),
        "Ошибка выполнения команды ['ls']: нет доступа"
    );
    assert_eq!(utils::msg_no_sound_player(""), "Нечем проиграть звук: в системе нет проигрывателя");
    assert_eq!(
        utils::msg_missing_tool(&["qdbus6", "qdbus-qt6"], ""),
        "Не найден инструмент qdbus6 (пробовали: qdbus6, qdbus-qt6)"
    );
    assert_eq!(utils::msg_missing_tool(&["tar"], ""), "Не найден инструмент tar");
    let run = utils::run(&utils::RunSpec::default(), None);
    assert_eq!(run.error, "пустая команда");
}

fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Ошибки запуска команд, «нечем играть звук», «нет инструмента» и «нет
/// D-Bus» — на английском без единой русской буквы.
#[test]
fn english_tool_messages_have_no_russian() {
    let _lang = i18n_pin::en();
    assert_eq!(utils::msg_timeout("pacman", "30"), "Command pacman: timed out (30 s)");
    assert_eq!(
        utils::msg_run_failed("['ls']", "Permission denied"),
        "Couldn't run the command ['ls']: Permission denied"
    );
    assert_eq!(
        utils::msg_no_sound_player(""),
        "Can't play the sound: no audio player is installed"
    );
    assert_eq!(
        utils::msg_no_sound_player(&utils::tool_package_hint("pw-play", "arch")),
        "Can't play the sound: no audio player is installed. Install the “pipewire” package"
    );
    assert_eq!(utils::msg_missing_tool(&["tar"], ""), "Tool tar not found");
    assert_eq!(
        utils::msg_missing_tool(&["qdbus6", "qdbus-qt6"], "Install the “x” package"),
        "Tool qdbus6 not found (tried: qdbus6, qdbus-qt6). Install the “x” package"
    );
    let dbus = utils::msg_no_dbus_tool(&["qdbus6", "qdbus"]);
    assert!(dbus.starts_with("Neither dbus-send nor qdbus (qdbus6, qdbus) is installed"), "{dbus}");
    let run = utils::run(&utils::RunSpec::default(), None);
    assert_eq!(run.error, "empty command");
    for (binary, _) in utils::TOOL_PACKAGES {
        for family in ["arch", "debian", "fedora", "suse"] {
            let (names, _) = utils::tool_candidates(binary);
            let hint = utils::tool_package_hint(names[0], family);
            let text = utils::msg_missing_tool(&names, &hint);
            assert!(!has_cyrillic(&text), "{binary}/{family}: {text}");
        }
    }
    for text in [
        utils::msg_timeout("x", "1"),
        utils::msg_run_failed("x", "y"),
        utils::msg_no_sound_player(""),
        utils::msg_no_sound_player("hint"),
        dbus,
    ] {
        assert!(!has_cyrillic(&text), "{text}");
    }
}

// ============================================================================
// Пути пользователя
// ============================================================================

#[test]
fn expand_user_matches_original() {
    let g = fx();
    let home = g["home"].as_str().unwrap();
    for row in g["expand_user"].as_array().unwrap() {
        let cfg = opt_str(&row["xdg_config_home"]);
        let dat = opt_str(&row["xdg_data_home"]);
        // Хвостовой `/` в переменной — единственное намеренное расхождение:
        // сравниваем без него (склеенные пути совпадают точно).
        let sloppy = cfg.is_some_and(|v| v.ends_with('/')) || dat.is_some_and(|v| v.ends_with('/'));
        let cmp = |got: &str, want: &str, what: &str| {
            if sloppy {
                assert_eq!(
                    got.trim_end_matches('/'),
                    want.trim_end_matches('/'),
                    "{what} при XDG {cfg:?}/{dat:?}"
                );
            } else {
                assert_eq!(got, want, "{what} при XDG {cfg:?}/{dat:?}");
            }
        };

        cmp(
            &utils::config_home(cfg, home),
            row["config_home"].as_str().unwrap(),
            "config_home",
        );
        cmp(
            &utils::data_home(dat, home),
            row["data_home"].as_str().unwrap(),
            "data_home",
        );
        for (path, want) in row["paths"].as_object().unwrap() {
            cmp(
                &utils::expand_user_xdg(path, home, cfg, dat),
                want.as_str().unwrap(),
                &format!("expand_user({path:?})"),
            );
        }
    }
}

#[test]
fn expand_user_prefix_boundary_fixed() {
    // Баг оригинала (правило #16 — правим в порте): `startswith("~/.config")`
    // резал пути-соседи посередине, и чтение/запись уходили в ЧУЖОЙ каталог.
    // Здесь закреплено правильное поведение при нестандартном XDG, где
    // ошибка была видна ярче всего.
    let h = "/home/user";
    let cfg = Some("/tmp/cfg");
    let dat = Some("/tmp/dat");
    for (path, want) in [
        ("~/.config-old/x", "/home/user/.config-old/x"),
        ("~/.configfoo", "/home/user/.configfoo"),
        ("~/.local/share-bak/i", "/home/user/.local/share-bak/i"),
        ("~/.local/shareX", "/home/user/.local/shareX"),
        // а сами XDG-ветки по-прежнему уезжают куда велено
        ("~/.config/rexon", "/tmp/cfg/rexon"),
        ("~/.local/share/icons", "/tmp/dat/icons"),
    ] {
        assert_eq!(utils::expand_user_xdg(path, h, cfg, dat), want, "путь {path}");
    }
}

#[test]
fn expand_user_checked_refuses_what_core_cannot_do() {
    // `~имя` требует pwd, которого у ядра нет; неизвестный `$HOME` — тем
    // более. В обоих случаях живой Python обязан раскрыть путь сам.
    assert_eq!(utils::expand_user_checked("~root/x", Some("/home/u"), None, None), None);
    assert_eq!(utils::expand_user_checked("~", None, None, None), None);
    assert_eq!(utils::expand_user_checked("~/x", Some(""), None, None), None);
    // `~~/x` тоже отдаём Python: оригинал возвращает путь как есть только
    // потому, что пользователя `~` не существует, — это ответ pwd, а не наш.
    assert_eq!(utils::expand_user_checked("~~/x", Some("/home/u"), None, None), None);
    // Всё остальное ядро берёт на себя.
    assert_eq!(
        utils::expand_user_checked("~/.config/rexon", Some("/home/u"), None, None),
        Some("/home/u/.config/rexon".into())
    );
    assert_eq!(
        utils::expand_user_checked("/abs/Загрузки/x", Some("/home/u"), None, None),
        Some("/abs/Загрузки/x".into())
    );
    assert_eq!(
        utils::expand_user_checked("", Some("/home/u"), None, None),
        Some(String::new())
    );
}

#[test]
fn config_home_never_returns_empty() {
    // Патология, но проверяем: пустая строка на выходе означала бы, что
    // os.path.join собрал ОТНОСИТЕЛЬНЫЙ путь, и конфиг уехал бы в cwd.
    assert_eq!(utils::config_home(Some("/"), "/home/u"), "/");
    assert_eq!(utils::config_home(Some("///"), "/home/u"), "/");
    assert_eq!(utils::expand_user_xdg("~/.config/rexon", "/home/u", Some("/"), None), "/rexon");
    assert_eq!(utils::expand_user_xdg("~", "/", None, None), "/");
    assert_eq!(utils::expand_user_xdg("~/x", "/", None, None), "/x");
}

// ============================================================================
// Поиск бинарей — на реальных командах этой машины
// ============================================================================
//
// ⚠️ ЭТИМ СВЕРКАМ ЖИВАЯ МАШИНА НУЖНА ПО СУЩЕСТВУ, и подставным корнем их
// не лечат. Вопрос, на который они отвечают, — «ведёт ли наш `which` себя
// как `shutil.which` на НАСТОЯЩИХ файлах»: бит `x`, символические ссылки,
// каталог вместо файла, порядок каталогов PATH. Посеянное дерево ответило
// бы только про то, что мы сами посеяли. Поэтому вместо изоляции они
// помечены `#[cfg_attr(feature = "isolated-checks", ignore = "…")]` — под
// `cargo test-core-isolated` libtest назовёт их отдельной строкой
// «ignored» с причиной, и их краснота на чужой машине не спутается с
// настоящей поломкой. Разбор приёма — в `rexon-core/Cargo.toml`.

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: разрешает настоящие бинари в PATH эталона")]
fn which_matches_shutil_which() {
    let g = fx();
    let path = golden_path(&g);
    for (cmd, want) in g["which"].as_object().unwrap() {
        assert_eq!(
            utils::which(cmd, Some(&path)).as_deref(),
            want.as_str(),
            "which({cmd:?})"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: разрешает настоящие бинари, в т.ч. /bin/sh")]
fn which_honours_path_env_quirks() {
    let g = fx();
    for case in g["which_path_env"].as_array().unwrap() {
        let cmd = case["cmd"].as_str().unwrap();
        let path = case["path"].as_str().unwrap();
        assert_eq!(
            utils::which(cmd, Some(path)).as_deref(),
            case["want"].as_str(),
            "which({cmd:?}) при PATH={path:?}"
        );
    }
    // PATH нет вовсе → тот же список, что берёт shutil.which из CS_PATH.
    assert_eq!(utils::CS_PATH_FALLBACK, "/bin:/usr/bin");
    assert!(utils::which("sh", None).is_some(), "sh обязан находиться в /bin");
}

#[test]
fn tool_tables_match_original() {
    let g = fx();
    let sets = g["tool_sets"].as_object().unwrap();
    assert_eq!(utils::TOOL_SETS.len(), sets.len(), "число наборов синонимов");
    for (tool, pair) in sets {
        let want_names: Vec<&str> = pair[0].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let want_dirs: Vec<&str> = pair[1].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let (names, dirs) = utils::tool_candidates(tool);
        assert_eq!(names, want_names, "кандидаты {tool}");
        assert_eq!(dirs, want_dirs, "каталоги {tool}");
        assert!(utils::tool_has_variants(tool), "{tool} должен считаться подменяемым");
    }
    let subs = g["tool_subcommands"].as_object().unwrap();
    assert_eq!(utils::TOOL_SUBCOMMANDS.len(), subs.len());
    for (tool, prefix) in subs {
        let want: Vec<&str> = prefix.as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(utils::tool_subcommand(tool).unwrap(), want.as_slice(), "префикс {tool}");
        assert!(utils::tool_has_variants(tool));
    }
    // Неизвестное имя — сам себе кандидат и подмене не подлежит.
    let (names, dirs) = utils::tool_candidates("nosuchtool42");
    assert_eq!(names, vec!["nosuchtool42"]);
    assert!(dirs.is_empty());
    assert!(!utils::tool_has_variants("nosuchtool42"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: ищет настоящие бинари в PATH эталона")]
fn resolve_tool_matches_original() {
    let g = fx();
    let path = golden_path(&g);
    for (tool, want) in g["resolve_tool"].as_object().unwrap() {
        assert_eq!(
            utils::resolve_tool(tool, &[], Some(&path)).as_deref(),
            want.as_str(),
            "resolve_tool({tool:?})"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает argv вокруг настоящего бинаря")]
fn tool_argv_matches_original() {
    let g = fx();
    let path = golden_path(&g);
    let want_argv = |v: &Value| -> Option<Vec<String>> {
        v.as_array()
            .map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect())
    };
    for (tool, want) in g["tool_argv"].as_object().unwrap() {
        assert_eq!(
            utils::tool_argv(tool, &[], Some(&path)),
            want_argv(want),
            "tool_argv({tool:?})"
        );
    }

    // С аргументами (включая нестроковые и кириллицу в пути).
    let args = |xs: &[&str]| xs.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    let a = &g["tool_argv_args"];
    assert_eq!(
        utils::tool_argv("convert", &args(&["-resize", "100", "a.png"]), Some(&path)),
        want_argv(&a["convert"])
    );
    assert_eq!(
        utils::tool_argv(
            "identify",
            &args(&["-format", "%w %h", "/tmp/Загрузки/х.png"]),
            Some(&path)
        ),
        want_argv(&a["identify"])
    );
    assert_eq!(
        utils::tool_argv("nosuchtool42", &args(&["-v"]), Some(&path)),
        want_argv(&a["nosuchtool42"])
    );

    // tool_cmd: нет инструмента → argv[0] остаётся каноническим именем.
    let c = &g["tool_cmd"];
    assert_eq!(
        utils::tool_cmd(
            "kwriteconfig6",
            &args(&["--file", "kdeglobals", "--key", "X", "1"]),
            Some(&path)
        ),
        want_argv(&c["kwriteconfig6"]).unwrap()
    );
    assert_eq!(
        utils::tool_cmd("nosuchtool42", &args(&["-v"]), Some(&path)),
        want_argv(&c["nosuchtool42"]).unwrap()
    );
    assert_eq!(
        utils::tool_cmd("identify", &args(&["-format", "%w"]), Some(&path)),
        want_argv(&c["identify"]).unwrap()
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: подставляет настоящий путь бинаря")]
fn resolved_argv_matches_original() {
    let g = fx();
    let path = golden_path(&g);
    for (key, want) in g["resolved_argv"].as_object().unwrap() {
        let want: Vec<String> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        // Ключ эталона — argv[0]; сам argv восстанавливаем из ожидаемого,
        // кроме случаев подмены, которые перечислены явно ниже.
        let cmd: Vec<String> = match key.as_str() {
            "convert" => vec!["convert".into(), "a.png".into(), "b.png".into()],
            "identify" => vec![
                "identify".into(),
                "-format".into(),
                "%w".into(),
                "/tmp/х.png".into(),
            ],
            "ls" => vec!["ls".into(), "-la".into()],
            "kwriteconfig6" => vec!["kwriteconfig6".into(), "--file".into(), "kdeglobals".into()],
            "7z" => vec!["7z".into(), "x".into(), "-o/tmp/Загрузки".into()],
            "empty" => vec![],
            "nosuchtool42" => vec!["nosuchtool42".into(), "-v".into()],
            other => panic!("неописанный случай эталона: {other}"),
        };
        assert_eq!(utils::resolved_argv(&cmd, Some(&path)), want, "случай {key}");
    }
}

/// Каталоги вне PATH — на живом дереве: именно так `qdbus6` ищется на
/// Kubuntu (бинарь в `/usr/lib/qt6/bin/`, куда PATH не смотрит).
#[test]
fn lookup_tool_finds_binary_outside_path_by_glob() {
    let root = std::env::temp_dir().join("rexon-utils-parity-Загрузки");
    let _ = fs::remove_dir_all(&root);
    let bin = root.join("lib").join("x86_64-linux-gnu").join("qt6").join("bin");
    fs::create_dir_all(&bin).unwrap();
    // Не-исполняемый однофамилец в каталоге, который сортируется раньше:
    // проверка, что берётся именно исполняемый файл.
    let other = root.join("lib").join("aaa").join("qt6").join("bin");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("qdbus6"), b"").unwrap();
    fs::set_permissions(other.join("qdbus6"), fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(bin.join("qdbus6"), b"#!/bin/sh\n").unwrap();
    fs::set_permissions(bin.join("qdbus6"), fs::Permissions::from_mode(0o755)).unwrap();

    let pattern = format!("{}/lib/*/qt6/bin", root.display());
    let found = utils::lookup_tool(&["qdbus6", "qdbus-qt6"], &[&pattern], Some(""));
    assert_eq!(found.as_deref(), Some(bin.join("qdbus6").to_str().unwrap()));

    // Точный каталог без шаблона — тоже путь оригинала (`/usr/sbin`).
    let exact = bin.to_str().unwrap().to_string();
    assert_eq!(
        utils::lookup_tool(&["qdbus6"], &[&exact], Some("")).as_deref(),
        Some(bin.join("qdbus6").to_str().unwrap())
    );
    // Ничего не подходит — и каталог не существует: честный None.
    assert_eq!(
        utils::lookup_tool(&["нетакого"], &[&pattern, "/нет/такого/каталога"], Some("")),
        None
    );
    let _ = fs::remove_dir_all(&root);
}

// ============================================================================
// Имена пакетов
// ============================================================================

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn tool_package_names_match_original() {
    // Эталон — русские подсказки оригинала, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    let mut checked = 0usize;
    for (key, want) in g["tool_package_name"].as_object().unwrap() {
        let (binary, family) = key.rsplit_once('|').unwrap();
        assert_eq!(
            utils::tool_package_name(binary, family),
            want.as_str().unwrap(),
            "tool_package_name({binary:?}, {family:?})"
        );
        checked += 1;
    }
    assert!(checked > 100, "проверено слишком мало пар: {checked}");
    for (key, want) in g["tool_package_hint"].as_object().unwrap() {
        let (binary, family) = key.rsplit_once('|').unwrap();
        assert_eq!(
            utils::tool_package_hint(binary, family),
            want.as_str().unwrap(),
            "tool_package_hint({binary:?}, {family:?})"
        );
    }
}

/// Подсказка «что поставить» — на языке интерфейса; пометка «только в AUR»
/// переводится вместе с ней.
#[test]
fn english_package_hints_have_no_russian() {
    let _lang = i18n_pin::en();
    assert_eq!(utils::tool_package_hint("git", "arch"), "Install the “git” package");
    assert_eq!(utils::tool_package_name("win2xcur", "arch"), "win2xcur (from the AUR)");
    assert_eq!(
        utils::tool_package_hint("win2xcur", "arch"),
        "Install the “win2xcur (from the AUR)” package"
    );
    assert_eq!(utils::tool_package_hint("нет-такого", "arch"), "");
    for (binary, table) in utils::TOOL_PACKAGES {
        for (family, _) in *table {
            let name = utils::tool_package_name(binary, family);
            let hint = utils::tool_package_hint(binary, family);
            for text in [&name, &hint] {
                assert!(
                    !text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
                    "{binary}/{family}: {text}"
                );
            }
        }
    }
}

/// Выбор проигрывателя звука: по тому, ЧТО установлено и ЧТО оно умеет.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn sound_player_selection() {
    // Эталон — русские подсказки оригинала, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    let sp = &g["sound_players"];
    for c in sp["cases"].as_array().unwrap() {
        let available: Vec<String> = c["available"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let file = c["file"].as_str().unwrap();
        let want: Option<Vec<String>> = c["argv"].as_array().map(|a| {
            a.iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        });
        assert_eq!(
            utils::sound_play_argv(&available, file),
            want,
            "проигрыватель для {file} при {available:?}"
        );
    }
    for c in sp["event"].as_array().unwrap() {
        let available: Vec<String> = c["available"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let want: Option<Vec<String>> = c["argv"].as_array().map(|a| {
            a.iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect()
        });
        assert_eq!(
            utils::sound_play_event_argv(&available, c["event"].as_str().unwrap()),
            want
        );
    }
    for (path, want) in sp["suggestion"].as_object().unwrap() {
        assert_eq!(utils::sound_player_suggestion(path), want.as_str().unwrap());
    }
    let m = &sp["msg"];
    assert_eq!(
        utils::msg_no_sound_player(&utils::tool_package_hint("pw-play", "arch")),
        m["with_hint"].as_str().unwrap()
    );
    assert_eq!(
        utils::msg_no_sound_player(""),
        m["no_hint"].as_str().unwrap()
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn missing_tool_message_matches_original() {
    // Эталон — русские подсказки оригинала, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    for (key, want) in g["msg_missing_tool"].as_object().unwrap() {
        let (tool, family) = key.rsplit_once('|').unwrap();
        let (names, _) = utils::tool_candidates(tool);
        let hint = utils::tool_package_hint(names[0], family);
        assert_eq!(
            utils::msg_missing_tool(&names, &hint),
            want.as_str().unwrap(),
            "сообщение о {tool} на {family}"
        );
    }
}

// ============================================================================
// D-Bus — на живом ответе сессионной шины этой машины
// ============================================================================

#[test]
fn dbus_send_arg_matches_original() {
    let g = fx();
    let a = &g["dbus_send_arg"];
    let s = |k: &str| a[k].as_str().unwrap();
    assert_eq!(utils::DbusArg::Bool(true).to_dbus_send(), s("True"));
    assert_eq!(utils::DbusArg::Bool(false).to_dbus_send(), s("False"));
    assert_eq!(utils::DbusArg::Int(0).to_dbus_send(), s("0"));
    assert_eq!(utils::DbusArg::Int(42).to_dbus_send(), s("42"));
    assert_eq!(utils::DbusArg::Int(-7).to_dbus_send(), s("-7"));
    assert_eq!(
        utils::DbusArg::Str("org.kde.KWin".into()).to_dbus_send(),
        s("str")
    );
    assert_eq!(utils::DbusArg::Str("Загрузки".into()).to_dbus_send(), s("str_cyr"));
    assert_eq!(utils::DbusArg::Str(String::new()).to_dbus_send(), s("empty"));
    // float в Python уходит строкой (isinstance(1.5, int) — False).
    assert_eq!(utils::DbusArg::Str("1.5".into()).to_dbus_send(), s("float"));
}

#[test]
fn parse_dbus_reply_matches_original() {
    let g = fx();
    let cases = g["parse_dbus_reply"].as_object().unwrap();
    // Живые ответы шины (GetId, GetServerInformation, variant) должны быть
    // непустыми — иначе тест «проходит» ни на чём.
    for name in ["get_id", "server_info", "variant_uint32"] {
        assert!(
            !cases[name]["raw"].as_str().unwrap().is_empty(),
            "эталон {name} снят с мёртвой шины — перезаписать на живой машине"
        );
    }
    for (name, case) in cases {
        let raw = case["raw"].as_str().unwrap();
        assert_eq!(
            utils::parse_dbus_send_reply(raw),
            case["want"].as_str().unwrap(),
            "разбор ответа {name}"
        );
    }
}

#[test]
fn dbus_call_argv_matches_original() {
    let g = fx();
    for kind_key in ["dbus-send", "qdbus"] {
        let c = &g["dbus_call_argv"][kind_key];
        let args: Vec<utils::DbusArg> = c["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| match v {
                Value::Bool(b) => utils::DbusArg::Bool(*b),
                Value::Number(n) => utils::DbusArg::Int(n.as_i64().unwrap()),
                Value::String(s) => utils::DbusArg::Str(s.clone()),
                other => panic!("неожидаемый аргумент эталона: {other}"),
            })
            .collect();
        let want: Vec<String> = c["want"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            utils::dbus_call_argv(
                c["kind"].as_str().unwrap(),
                c["exe"].as_str().unwrap(),
                c["dest"].as_str().unwrap(),
                c["path"].as_str().unwrap(),
                c["method"].as_str().unwrap(),
                &args,
            ),
            want,
            "argv для {kind_key}"
        );
    }

    // Служба/объект/метод KWin и plasmashell — те же, что в оригинале.
    assert_eq!(
        utils::KWIN_RECONFIGURE,
        ("org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure")
    );
    assert_eq!(
        utils::PLASMA_EVALUATE_SCRIPT,
        (
            "org.kde.plasmashell",
            "/PlasmaShell",
            "org.kde.PlasmaShell.evaluateScript"
        )
    );
}

#[test]
fn notify_argv_matches_original() {
    let g = fx();
    let want = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        utils::notify_dbus_argv(
            "/usr/bin/dbus-send",
            "Тема применена",
            "Breeze Dark",
            "rexon",
            5000
        ),
        want(&g["notify_dbus_argv"])
    );
    assert_eq!(
        utils::notify_send_argv("/usr/bin/notify-send", "Тема применена", "Breeze Dark", "rexon"),
        want(&g["notify_send_argv"])
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: выбирает настоящий dbus-инструмент")]
fn dbus_tool_preference_matches_original() {
    // Эталон — русский текст оригинала, сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    let names: Vec<&str> = g["qdbus_names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    // Имена qdbus берутся из того же набора синонимов, что и у оригинала.
    let (qdbus, _) = utils::tool_candidates("qdbus6");
    assert_eq!(qdbus, names);
    assert_eq!(
        utils::msg_no_dbus_tool(&names),
        g["msg_no_dbus_tool"].as_str().unwrap()
    );

    // Порядок предпочтения: сперва dbus-send (есть везде), потом qdbus.
    assert_eq!(
        utils::DBUS_TOOL_PREFERENCE,
        &[("dbus-send", "dbus-send"), ("qdbus6", "qdbus")]
    );
    // На живой машине выбор тот же, что сделал оригинал.
    let live = g["dbus_tool_live"].as_array().unwrap();
    let path = golden_path(&g);
    let chosen = utils::DBUS_TOOL_PREFERENCE
        .iter()
        .find_map(|(tool, kind)| utils::resolve_tool(tool, &[], Some(&path)).map(|exe| (*kind, exe)));
    match chosen {
        Some((kind, exe)) => {
            assert_eq!(kind, live[0].as_str().unwrap());
            assert_eq!(exe, live[1].as_str().unwrap());
        }
        None => assert!(live[0].is_null(), "оригинал нашёл бинарь, а ядро — нет"),
    }
}

// ============================================================================
// Исполнитель внешних команд
// ============================================================================
//
// ⚠️ ПОЧЕМУ ПРОВЕРЯЕТСЯ ПЛАН, А НЕ ТОЛЬКО ЗАПУСК. Половину наших команд
// выполнить в сверке нельзя в принципе: за `pkexec` стоит установка стола и
// запрос пароля у живого человека. Поэтому мерило привилегированных путей —
// ТЕКСТОВЫЙ паритет собираемой команды (`run_plan`: argv после подмены имён,
// таймаут по классу, добавки к окружению), а сам `run` проверяется на
// безобидных `/bin/sh` и `/bin/echo`, где виден каждый исход.

fn spec(argv: &[&str]) -> utils::RunSpec {
    utils::RunSpec {
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn run_plan_takes_timeout_from_command_class() {
    // Опрос отвечает сразу, установка зависит от сети, сборка — от машины.
    // Единого дефолта на все три быть не может: тридцатью секундами `pacman
    // -Q` мерили сборку девяти пакетов из AUR, и установка стола падала
    // целиком.
    let cases: &[(&[&str], u64, utils::RunDuration)] = &[
        (&["pacman", "-Q", "kvantum"], utils::QUICK_TIMEOUT_SECS, utils::RunDuration::Quick),
        (&["pacman", "-S", "--noconfirm", "mate"], utils::DOWNLOAD_TIMEOUT_SECS, utils::RunDuration::Download),
        (&["yay", "-S", "--noconfirm", "mate-tweak"], utils::BUILD_TIMEOUT_SECS, utils::RunDuration::Build),
        (&["pkexec", "pacman", "-S", "--noconfirm", "mate"], utils::DOWNLOAD_TIMEOUT_SECS, utils::RunDuration::Download),
        (&["pkexec", "bash", "/tmp/rexon_install_ab12.sh"], utils::DOWNLOAD_TIMEOUT_SECS, utils::RunDuration::Download),
        (&["gsettings", "get", "org.gnome.desktop.interface", "gtk-theme"], utils::QUICK_TIMEOUT_SECS, utils::RunDuration::Quick),
        (&[], utils::QUICK_TIMEOUT_SECS, utils::RunDuration::Quick),
    ];
    for (argv, want_secs, want_class) in cases {
        let plan = utils::run_plan(&spec(argv), Some(utils::CS_PATH_FALLBACK));
        assert_eq!(plan.timeout_secs, *want_secs, "таймаут {argv:?}");
        assert_eq!(plan.duration, *want_class, "класс {argv:?}");
    }
    // Свой таймаут вызывающего класс не пересиливает.
    let mut s = spec(&["yay", "-S", "mate"]);
    s.timeout_secs = Some(7);
    assert_eq!(utils::run_plan(&s, None).timeout_secs, 7);
}

#[test]
fn run_plan_mutes_locale_only_when_output_is_parsed() {
    // ⚠️ На русской локали пакетные менеджеры печатают переведённые поля
    // («Зависит от» вместо «Depends On»), и разбор в ядре не находит ничего.
    // Прежде эту пару подставлял по месту сам Python, глядя на флаг плана, —
    // решение ядра исполняли две разные строки в двух файлах.
    let mut s = spec(&["pacman", "-Qi", "plasma-desktop"]);
    s.lang_c = true;
    let plan = utils::run_plan(&s, Some(utils::CS_PATH_FALLBACK));
    assert_eq!(
        plan.env,
        vec![
            ("LANG".to_string(), "C".to_string()),
            ("LC_ALL".to_string(), "C".to_string())
        ]
    );
    // Без флага окружение не трогаем вовсе: команде с выводом для человека
    // русская локаль нужна.
    let plan = utils::run_plan(&spec(&["pacman", "-Qi", "plasma-desktop"]), None);
    assert!(plan.env.is_empty());

    // Явные добавки идут ПОСЛЕ глушения локали — вызывающий может её перебить.
    let mut s = spec(&["env"]);
    s.lang_c = true;
    s.env = vec![("LC_ALL".into(), "ru_RU.UTF-8".into())];
    let plan = utils::run_plan(&s, None);
    assert_eq!(plan.env.last().unwrap().1, "ru_RU.UTF-8");
}

#[test]
fn run_plan_always_closes_stdin() {
    // Ни одна наша команда не читает клавиатуру. Вопрос вроде «Enter a
    // selection (default=all):» на унаследованном stdin висел бы до конца
    // таймаута, а таймаут сборки — четыре часа.
    for argv in [&["pacman", "-S", "mate"][..], &["/bin/echo", "x"][..], &[][..]] {
        assert!(utils::run_plan(&spec(argv), None).stdin_null, "{argv:?}");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: подставляет настоящий путь бинаря")]
fn run_plan_substitutes_tool_synonyms() {
    // Подмена имени бинаря достаётся и argv-планам, собранным ядром: план
    // называет канонический `kwriteconfig6`, а на Kubuntu 24.04 существует
    // только `kwriteconfig5`.
    let g = fx();
    let path = golden_path(&g);
    let plan = utils::run_plan(&spec(&["convert", "a.png", "b.png"]), Some(&path));
    let want = utils::resolved_argv(
        &["convert", "a.png", "b.png"].map(String::from),
        Some(&path),
    );
    assert_eq!(plan.argv, want);
    // Имя без синонимов проходит насквозь.
    let plan = utils::run_plan(&spec(&["pkexec", "pacman", "-S", "mate"]), Some(&path));
    assert_eq!(plan.argv, vec!["pkexec", "pacman", "-S", "mate"]);
}

#[test]
fn decode_text_matches_python_text_mode() {
    // `text=True` переводит концы строк по-универсальному и заменяет негодные
    // байты — без этого строки живого хода сборки различались бы.
    assert_eq!(utils::decode_text(b"a\r\nb\rc\nd"), "a\nb\nc\nd");
    assert_eq!(utils::decode_text("тема\n".as_bytes()), "тема\n");
    assert_eq!(utils::decode_text(&[0xff, b'a']), "\u{fffd}a");
    assert_eq!(utils::decode_text(b""), "");
}

/// Разбираемый вывод: ребёнок видит C-локаль и НЕ видит `LANGUAGE`, которую
/// окно Rexon ставит своим языком (`cli::qt_ui_language_env`).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящий /bin/sh")]
fn parsed_output_has_no_ui_language() {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "printf '%s|%s|%s' \"${LANGUAGE-нет}\" \"$LC_ALL\" \"$LANG\""])
        .env("LANGUAGE", "de")
        .env("LC_ALL", "de_DE.UTF-8");
    let out = utils::parsed_output_locale(&mut cmd).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "нет|C|C");
    assert_eq!(utils::PARSED_OUTPUT_ENV_REMOVE, &["LANGUAGE"]);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящие /bin/sh и /bin/echo")]
fn run_reports_every_outcome_apart() {
    // Эталон — русские тексты оригинала: язык закреплён (без замка сверка
    // краснела или зеленела в зависимости от соседних потоков).
    let _lang = i18n_pin::ru();
    // Исход обязан различать «не смогли» и «не дождались»: человеку это
    // РАЗНЫЕ новости.
    let mut s = spec(&["/bin/echo", "тема", "применена"]);
    s.output = utils::RunOutput::Capture;
    let r = utils::run(&s, None);
    assert_eq!(r.outcome, utils::RunOutcome::Ok);
    assert_eq!(r.returncode, Some(0));
    assert_eq!(utils::decode_text(&r.stdout), "тема применена\n");

    let mut s = spec(&["/bin/sh", "-c", "echo беда >&2; exit 3"]);
    s.output = utils::RunOutput::Capture;
    let r = utils::run(&s, None);
    assert_eq!(r.outcome, utils::RunOutcome::Failed);
    assert_eq!(r.returncode, Some(3));
    assert_eq!(utils::decode_text(&r.stderr), "беда\n");

    let mut s = spec(&["/bin/sh", "-c", "sleep 30"]);
    s.output = utils::RunOutput::Capture;
    s.timeout_secs = Some(1);
    let r = utils::run(&s, None);
    assert_eq!(r.outcome, utils::RunOutcome::Timeout);
    assert_eq!(r.returncode, None);
    assert_eq!(r.timeout_secs, 1);

    let r = utils::run(&spec(&["/nosuchbinary42"]), None);
    assert_eq!(r.outcome, utils::RunOutcome::Error);
    assert!(r.error.starts_with("Ошибка выполнения команды"), "{}", r.error);

    let r = utils::run(&spec(&[]), None);
    assert_eq!(r.outcome, utils::RunOutcome::Error);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящий /bin/sh")]
fn run_delivers_muted_locale_to_the_child() {
    // Мерило не «мы положили переменную в план», а «ребёнок её увидел».
    let mut s = spec(&["/bin/sh", "-c", "echo $LC_ALL/$LANG"]);
    s.output = utils::RunOutput::Capture;
    s.lang_c = true;
    assert_eq!(utils::decode_text(&utils::run(&s, None).stdout), "C/C\n");

    let mut s = spec(&["/bin/sh", "-c", "echo $REXON_TEST_VAR"]);
    s.output = utils::RunOutput::Capture;
    s.env = vec![("REXON_TEST_VAR".into(), "Загрузки".into())];
    assert_eq!(
        utils::decode_text(&utils::run(&s, None).stdout),
        "Загрузки\n"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящий /bin/sh")]
fn run_writes_output_to_file_for_live_progress() {
    // Так идёт долгая установка: вывод в файл, живой ход дела читают из него
    // отдельно. stderr сливается туда же — иначе половина строк сборки
    // (`==> Making package`) не дошла бы до человека.
    let path = std::env::temp_dir().join("rexon_run_plan_check.log");
    let mut s = spec(&["/bin/sh", "-c", "echo из-stdout; echo из-stderr >&2"]);
    s.output = utils::RunOutput::File(path.to_string_lossy().into_owned());
    s.stderr_to_stdout = true;
    let r = utils::run(&s, None);
    assert_eq!(r.outcome, utils::RunOutcome::Ok);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("из-stdout"), "{text}");
    assert!(text.contains("из-stderr"), "{text}");
    let _ = fs::remove_file(&path);

    // stderr ПЕРВЫМ, stdout следом: при двух отдельных открытиях файла
    // stdout писал со своей позиции 0 и затирал строку stderr — так из
    // журнала помощника AUR пропадало «sudo: a terminal is required…».
    let mut s = spec(&["/bin/sh", "-c", "echo sudo-error-line >&2; echo out"]);
    s.output = utils::RunOutput::File(path.to_string_lossy().into_owned());
    s.stderr_to_stdout = true;
    utils::run(&s, None);
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text, "sudo-error-line\nout\n");
    let _ = fs::remove_file(&path);

    // Некуда писать — честная ошибка, а не паника.
    let mut s = spec(&["/bin/echo", "x"]);
    s.output = utils::RunOutput::File("/nosuchdir42/log".into());
    assert_eq!(utils::run(&s, None).outcome, utils::RunOutcome::Error);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: запускает настоящий /bin/sh")]
fn run_merges_stderr_into_stdout_when_asked() {
    let mut s = spec(&["/bin/sh", "-c", "echo раз; echo два >&2"]);
    s.output = utils::RunOutput::Capture;
    s.stderr_to_stdout = true;
    let r = utils::run(&s, None);
    let text = utils::decode_text(&r.stdout);
    assert!(text.contains("раз") && text.contains("два"), "{text}");
    assert!(r.stderr.is_empty());
}
