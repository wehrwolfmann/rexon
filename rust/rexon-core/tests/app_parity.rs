//! Golden-сверка `app.rs` (аварийный откат `--recover-kde`, ярлыки, хуки
//! входа).
//!
//! `golden/app_cases.json` — эталон. Когда-то он был снят read-only с живого
//! `rexon.app` (чистая `_strip_fork_applets_text` на синтетическом
//! appletsrc, детекторы токенов на пограничных строках, таблицы ключей
//! шрифтов, эффектов KWin и флагов одиночных вкладок). С 02.10.2026 Python в
//! Rexon — только архив: снимок заморожен и служит СОБСТВЕННЫМ эталоном
//! Rust, а не записью чужого поведения. Переснимать его нечем и незачем —
//! осознанная правка поведения правит и эталон.

mod i18n_pin;

use rexon_core::app;
use rexon_core::i18n;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/app_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn strs(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(s).collect()
}

#[test]
fn strip_fork_applets_cases() {
    let g = fx();
    for case in ["strip_full", "strip_none", "strip_preamble", "strip_empty"] {
        let c = &g[case];
        let (out, removed) = app::strip_fork_applets(s(&c["in"]));
        assert_eq!(out, s(&c["out"]), "текст после чистки [{case}]");
        assert_eq!(removed, strs(&c["removed"]), "удалённые plugin [{case}]");
    }
}

/// Без форк-апплетов текст обязан вернуться БАЙТ-В-БАЙТ исходным: файл
/// панели переписывается только когда есть что удалять.
#[test]
fn strip_is_identity_without_fork() {
    let g = fx();
    let src = s(&g["strip_none"]["in"]);
    let (out, removed) = app::strip_fork_applets(src);
    assert_eq!(out, src);
    assert!(removed.is_empty());
}

/// Секция форк-апплета выкидывается вместе с вложенными подсекциями, а
/// соседний стоковый systemtray и его extraItems остаются нетронутыми.
#[test]
fn strip_keeps_stock_applets() {
    let g = fx();
    let (out, removed) = app::strip_fork_applets(s(&g["strip_full"]["in"]));
    assert_eq!(removed.len(), 2, "должно уйти два форк-апплета: {removed:?}");
    assert!(!out.contains("SevenStart"), "остался форк-апплет");
    assert!(!out.contains("aeroshell"), "остался форк-апплет");
    assert!(
        !out.contains("favoritesPortedToKAstats"),
        "осиротевшая подсекция форк-апплета не удалена"
    );
    assert!(out.contains("org.kde.plasma.systemtray"), "снесён systemtray");
    assert!(
        out.contains("org.kde.plasma.battery"),
        "снесены extraItems системного лотка"
    );
    assert!(out.contains("org.kde.plasma.kickoff"), "снесён стоковый kickoff");
}

#[test]
fn fork_token_detection() {
    let g = fx();
    for (text, want) in g["fork_token"].as_object().unwrap() {
        assert_eq!(
            app::has_fork_token(text),
            want.as_bool().unwrap(),
            "строка {text:?}"
        );
    }
}

#[test]
fn kvantum_aero_detection() {
    let g = fx();
    for (theme, want) in g["kvantum_aero"].as_object().unwrap() {
        assert_eq!(
            app::is_aero_kvantum_theme(theme),
            want.as_bool().unwrap(),
            "тема {theme:?}"
        );
    }
}

#[test]
fn reset_tables() {
    let g = fx();
    assert_eq!(app::KDE_FONT_KEYS, strs(&g["font_keys"]).as_slice());
    assert_eq!(app::KWIN_FORK_EFFECTS, strs(&g["kwin_effects"]).as_slice());

    // argv строятся по этим же таблицам — сверяем форму на первом элементе.
    let fonts = app::kde_font_reset_argv("kwriteconfig6");
    assert_eq!(fonts.len(), app::KDE_FONT_KEYS.len());
    assert_eq!(
        fonts[0],
        vec![
            "kwriteconfig6",
            "--file",
            "kdeglobals",
            "--group",
            "General",
            "--key",
            "font",
            "--delete"
        ]
    );
    let effects = app::kwin_effect_reset_argv("kwriteconfig5");
    assert_eq!(effects.len(), app::KWIN_FORK_EFFECTS.len());
    assert_eq!(
        effects[0],
        vec![
            "kwriteconfig5",
            "--file",
            "kwinrc",
            "--group",
            "Plugins",
            "--key",
            "aeroglassblurEnabled",
            "--delete"
        ]
    );
    assert_eq!(
        app::kwin_reconfigure_argv("qdbus6"),
        vec!["qdbus6", "org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"]
    );
    assert_eq!(
        app::widget_style_breeze_argv("kwriteconfig6"),
        vec![
            "kwriteconfig6",
            "--file",
            "kdeglobals",
            "--group",
            "KDE",
            "--key",
            "widgetStyle",
            "Breeze"
        ]
    );
}

#[test]
fn single_tab_table_matches_original() {
    let g = fx();
    let want = g["single_tab_flags"].as_object().unwrap();
    assert_eq!(app::SINGLE_TAB_FLAGS.len(), want.len(), "число вкладок");
    for (flag, module) in app::SINGLE_TAB_FLAGS {
        assert_eq!(
            want.get(*flag).map(s),
            Some(*module),
            "модуль вкладки {flag}"
        );
    }
}

#[test]
fn cli_routing() {
    use app::Route;
    let args = |xs: &[&str]| -> Vec<String> {
        xs.iter().map(|s| (*s).to_string()).collect()
    };

    assert_eq!(app::route(&args(&[])), Route::MainWindow);
    assert_eq!(app::route(&args(&["--qt"])), Route::MainWindow);
    assert_eq!(app::route(&args(&["--apply-defaults"])), Route::ApplyDefaults);
    assert_eq!(app::route(&args(&["--sanitize-gtk"])), Route::SanitizeGtk);
    assert_eq!(app::route(&args(&["--session-splash"])), Route::SessionSplash);
    assert_eq!(app::route(&args(&["--recover-kde"])), Route::RecoverKde);
    assert_eq!(
        app::route(&args(&["--settings-qt"])),
        Route::SingleTab { module: "rexon.qt_settings.app" }
    );

    // Приоритет как в оригинале: служебные хуки раньше UI, вкладки раньше
    // заставки и аварийного отката.
    assert_eq!(
        app::route(&args(&["--recover-kde", "--apply-defaults"])),
        Route::ApplyDefaults
    );
    assert_eq!(
        app::route(&args(&["--recover-kde", "--compare-qt"])),
        Route::SingleTab { module: "rexon.qt_compare.app" }
    );
    // Несколько вкладок сразу — побеждает первая по таблице.
    assert_eq!(
        app::route(&args(&["--desktops-qt", "--appearance-qt"])),
        Route::SingleTab { module: "rexon.appearance_page" }
    );
    // Неизвестный флаг маршрут не меняет.
    assert_eq!(app::route(&args(&["--чего-то-нет"])), Route::MainWindow);
}

#[test]
fn main_window_args_drop_legacy_alias() {
    let args: Vec<String> = ["--qt", "--verbose", "путь/файл"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(app::main_window_args(&args), vec!["--verbose", "путь/файл"]);
}

// ============================================================================
// Ярлыки рабочего стола: `--page=<имя>`
// ============================================================================

#[test]
fn start_page_targets() {
    let one = |flag: &str| app::start_page(&[flag.to_string()]);
    assert_eq!(one("--page=install"), Some(("desktops", "")));
    assert_eq!(one("--page=backups"), Some(("settings", "")));
    assert_eq!(one("--page=compare"), Some(("desktops", "compare")));
    // Регистр и пробелы приезжают из чужих файлов — на них падать нельзя.
    assert_eq!(one("--page=Backups"), Some(("settings", "")));
    // Незнакомое имя и отсутствие флага одинаково означают «раздел по
    // умолчанию»: окно обязано открыться, а не отказать из-за опечатки.
    assert_eq!(one("--page=такого-нет"), None);
    assert_eq!(one("--tray"), None);
    assert_eq!(app::start_page(&[]), None);
}

/// Раздел из таблицы обязан существовать в составе окна, иначе ярлык ведёт
/// в никуда — а узнать об этом можно было бы только глазом.
#[test]
fn start_page_sections_exist() {
    for (page, section, _) in app::PAGE_TARGETS {
        assert!(
            rexon_core::qt_main::SECTIONS.contains(section),
            "--page={page} ведёт в раздел «{section}», которого нет в окне"
        );
    }
}

/// Сторож против рецидива: в ярлыке появился `--page=` без записи в
/// таблице — значит пункт меню снова обещает то, чего продукт не делает.
#[test]
fn ярлык_не_обещает_неизвестных_страниц() {
    for entry in [inst::LaunchEntry::Python, rust_entry()] {
        let desktop = inst::desktop_content_for(&entry);
        let mut found = 0;
        for tail in desktop.split(app::PAGE_FLAG).skip(1) {
            let name: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            found += 1;
            assert!(
                app::start_page(&[format!("{}{name}", app::PAGE_FLAG)]).is_some(),
                "ярлык зовёт --page={name}, а разбор такого имени не знает"
            );
        }
        assert_eq!(found, 3, "в ярлыке ожидались три быстрых действия --page=");
    }
}

// ============================================================================
// Вход ярлыка и автозапуска: Rust-окно, откат на Python
// ============================================================================

use rexon_core::installer as inst;

const RUST_BIN: &str = "/usr/lib/rexon/rexon-main-window";

fn rust_entry() -> inst::LaunchEntry {
    inst::LaunchEntry::MainWindow(RUST_BIN.to_string())
}

/// Группы `.desktop`: имя группы → пары ключ/значение.
fn groups(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.to_string(), Vec::new()));
        } else if let Some((k, v)) = line.split_once('=') {
            out.last_mut().expect("ключ до первой группы").1.push((k.into(), v.into()));
        }
    }
    out
}

fn key<'a>(g: &'a [(String, String)], k: &str) -> Option<&'a str> {
    g.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str())
}

fn argv(exec: &str) -> Vec<String> {
    exec.split_whitespace().map(str::to_string).collect()
}

#[test]
fn основной_вход_ярлыка_rust_окно() {
    let text = inst::desktop_content_for(&rust_entry());
    let g = groups(&text);
    assert_eq!(g[0].0, "Desktop Entry");
    assert_eq!(key(&g[0].1, "Exec"), Some(RUST_BIN));
    assert_eq!(key(&g[0].1, "TryExec"), Some(RUST_BIN));
    // X-ключи — в основной группе, а не в группе последнего действия.
    assert_eq!(key(&g[0].1, "X-GNOME-UsesNotifications"), Some("true"));
    for (name, keys) in &g[1..] {
        assert!(key(keys, "X-GNOME-UsesNotifications").is_none(), "X-ключ в [{name}]");
    }
}

#[test]
fn быстрые_действия_идут_на_rust_окно_и_понимаются_им() {
    let text = inst::desktop_content_for(&rust_entry());
    let g = groups(&text);
    let actions = key(&g[0].1, "Actions").unwrap();
    for id in ["InstallDE", "Backup", "Compare", "Recover"] {
        let (_, keys) = g
            .iter()
            .find(|(n, _)| *n == format!("Desktop Action {id}"))
            .unwrap_or_else(|| panic!("нет действия {id}"));
        assert!(actions.split(';').any(|a| a == id), "{id} не в Actions");
        let args = argv(key(keys, "Exec").unwrap());
        assert_eq!(args[0], RUST_BIN, "действие {id} ушло не на Rust-окно");
        // Rust-бинарь понимает флаг той же дверью: либо раздел окна, либо
        // маршрут без окна. Флаг, который не понимает никто, — обман.
        let rest = args[1..].to_vec();
        let understood = rexon_core::cli::start_section(&rest).is_some()
            || app::route(&rest) == app::Route::RecoverKde;
        assert!(understood, "Rust-окно не понимает {rest:?} из действия {id}");
    }
    assert!(
        text.contains(&format!("\nExec={RUST_BIN} {}\n", app::RECOVER_KDE_FLAG)),
        "аварийный откат обязан идти на Rust-окно"
    );
}

/// ⭐ 16.09.2026: пункта «Rexon (классическое окно)» в ярлыке БОЛЬШЕ НЕТ.
/// Классического окна на Python не везёт ни пакет дистрибутива, ни ручная
/// установка: команда `rexon` у обоих ведёт на то же Rust-окно, и пункт
/// открывал бы копию того, что открывает сам ярлык. Сторож держит это при
/// любом входе.
#[test]
fn пункта_классического_окна_в_ярлыке_нет() {
    for entry in [rust_entry(), inst::LaunchEntry::Python] {
        let text = inst::desktop_content_for(&entry);
        let g = groups(&text);
        let actions = key(&g[0].1, "Actions").unwrap();
        assert_eq!(actions, "InstallDE;Backup;Compare;Recover;", "{entry:?}");
        assert!(!actions.split(';').any(|a| a == "Classic"), "{entry:?}");
        assert!(g.iter().all(|(n, _)| n != "Desktop Action Classic"), "{entry:?}");
        assert!(!text.contains(inst::CLASSIC_WINDOW_ENTRY), "{entry:?}");
        assert!(
            !text.contains(rexon_core::single_instance::CLASSIC_FLAG),
            "{entry:?}"
        );
    }
}

#[test]
fn откат_по_переменной_уводит_на_python() {
    let cands = vec![RUST_BIN.to_string()];
    let all = |_: &str| true;
    for v in ["python", "Python", " PYTHON\n"] {
        assert_eq!(
            inst::choose_launch_entry(Some(v), &cands, all),
            inst::LaunchEntry::Python,
            "REXON_ENTRY={v:?}"
        );
    }
    for v in [None, Some(""), Some("rust"), Some("pythonic")] {
        assert_eq!(inst::choose_launch_entry(v, &cands, all), rust_entry(), "{v:?}");
    }
    assert_eq!(inst::ENTRY_ENV, "REXON_ENTRY");
}

#[test]
fn без_бинаря_ярлык_пишется_на_python_а_не_битым() {
    let cands = inst::main_window_candidates(Some("/usr/bin:/usr/local/bin"), "", &[]);
    let entry = inst::choose_launch_entry(None, &cands, |_| false);
    assert_eq!(entry, inst::LaunchEntry::Python);
    let text = inst::desktop_content_for(&entry);
    assert!(!text.contains("rexon-main-window"));
    let g = groups(&text);
    assert_eq!(key(&g[0].1, "Exec"), Some("rexon"));
    assert_eq!(key(&g[0].1, "TryExec"), Some("rexon"));
    // Первый ИСПОЛНЯЕМЫЙ кандидат, а не первый по списку.
    let e = inst::choose_launch_entry(None, &cands, |p| p == RUST_BIN);
    assert_eq!(e, rust_entry());
    // Путь с управляющим символом в `.desktop` не записать — пропускается.
    let bad = vec!["/x\ny/rexon-main-window".to_string()];
    assert_eq!(inst::choose_launch_entry(None, &bad, |_| true), inst::LaunchEntry::Python);
}

#[test]
fn кандидаты_бинаря_окна_по_порядку() {
    let c = inst::main_window_candidates(
        Some("/usr/local/bin:relative::/usr/bin/:/usr/local/bin"),
        "/home/u",
        &["/home/u/tree".to_string()],
    );
    assert_eq!(
        c,
        vec![
            "/usr/local/bin/rexon-main-window",
            "/usr/bin/rexon-main-window",
            inst::PACKAGE_MAIN_WINDOW,
            "/usr/local/lib/rexon/rexon-main-window",
            // Установка в домашнюю папку — та же раскладка.
            "/home/u/.local/lib/rexon/rexon-main-window",
            "/home/u/tree/rust/target/release/rexon-main-window",
        ]
    );
    // Установка в систему кладёт окно по раскладке пакета.
    assert_eq!(
        inst::main_window_candidates(None, "", &[]),
        vec![inst::PACKAGE_MAIN_WINDOW, "/usr/local/lib/rexon/rexon-main-window"]
    );
}

#[test]
fn автозапуск_по_тому_же_правилу_что_ярлык() {
    use rexon_core::tray::autostart_exec;
    let cands = vec![RUST_BIN.to_string()];
    for (env, exists) in [(None, true), (Some("python"), true), (None, false)] {
        let entry = inst::choose_launch_entry(env, &cands, |_| exists);
        let desktop = groups(&inst::desktop_content_for(&entry));
        let main = argv(key(&desktop[0].1, "Exec").unwrap());
        let auto = autostart_exec(&entry, Some("/usr/local/bin/rexon"))
            .expect("лаунчер задан — заводить есть чем");
        let auto = argv(&auto);
        assert_eq!(auto.last().map(String::as_str), Some("--tray"));
        // Лоток и есть хозяин — забирать замок ему не у кого.
        assert!(!auto.iter().any(|a| a == rexon_core::single_instance::CLASSIC_FLAG));
        // Один хозяин: программа автозапуска — та же, что у ярлыка (у
        // Python-входа ярлык зовёт `rexon` по PATH, лоток — найденный путь).
        match &entry {
            inst::LaunchEntry::MainWindow(p) => {
                assert_eq!(main[0], *p);
                assert_eq!(auto[0], *p);
            }
            inst::LaunchEntry::Python => {
                assert_eq!(main[0], "rexon");
                assert_eq!(auto[0], "/usr/local/bin/rexon");
            }
        }
    }
}

/// ⛔ Сторож хуков входа в сеанс: автозапуск дефолтов (`--apply-defaults`),
/// заставка (`.desktop` и юнит гибрида, `--session-splash`) и лоток пишутся
/// по ОДНОМУ выбору входа. При Rust-входе в них нет ни интерпретатора, ни
/// модуля `rexon`, и бинарь окна понимает флаг сам; при откате на Python —
/// прежние строки.
#[test]
fn хуки_входа_без_python_при_rust_входе() {
    use rexon_core::{appdefaults as ad, session_splash as ss, tray};
    const PY_MARKS: [&str; 4] = ["python", "-m rexon", "from rexon", "import rexon"];
    let cands = vec![RUST_BIN.to_string()];
    let launcher = Some("/usr/local/bin/rexon");
    let py = "/usr/bin/python3";
    for (env, exists) in [(None, true), (Some("python"), true), (None, false)] {
        let entry = inst::choose_launch_entry(env, &cands, |_| exists);
        let hooks = [
            (
                "дефолты",
                ad::autostart_desktop_content(
                    &ad::autostart_exec_for(&entry, launcher, Some("/opt/self")).unwrap(),
                ),
                "--apply-defaults",
            ),
            (
                "заставка, .desktop",
                ss::autostart_desktop_content(&ss::desktop_launcher_exec(&entry, launcher, py)),
                ss::SESSION_SPLASH_FLAG,
            ),
            (
                "заставка, юнит",
                ss::systemd_unit_content(&ss::unit_launcher_exec(&entry, launcher, py)),
                ss::SESSION_SPLASH_FLAG,
            ),
            (
                "лоток",
                tray::autostart_desktop_content(
                    &tray::autostart_exec(&entry, launcher).unwrap(),
                ),
                tray::START_HIDDEN_FLAG,
            ),
        ];
        for (name, text, flag) in hooks {
            let cmd = text
                .lines()
                .find_map(|l| l.strip_prefix("Exec=").or_else(|| l.strip_prefix("ExecStart=")))
                .unwrap_or_else(|| panic!("{name}: нет строки запуска\n{text}"));
            let args = argv(cmd);
            assert_eq!(args.last().map(String::as_str), Some(flag), "{name}: {cmd}");
            match &entry {
                inst::LaunchEntry::MainWindow(p) => {
                    assert_eq!(args[0], *p, "{name}");
                    for m in PY_MARKS {
                        assert!(!text.contains(m), "{name} при Rust-входе несёт «{m}»:\n{text}");
                    }
                    if flag != tray::START_HIDDEN_FLAG {
                        assert_ne!(
                            app::route(&args[1..]),
                            app::Route::MainWindow,
                            "{name}: Rust-окно не понимает {flag}"
                        );
                    }
                }
                inst::LaunchEntry::Python => assert_eq!(args[0], "/usr/local/bin/rexon", "{name}"),
            }
        }
    }
    assert_eq!(app::route(&["--apply-defaults".to_string()]), app::Route::ApplyDefaults);
    assert_eq!(app::route(&[ss::SESSION_SPLASH_FLAG.to_string()]), app::Route::SessionSplash);

    // ⛔Откат без лаунчера — НЕ строка через интерпретатор, а пусто: писать
    // нечего. Прежде отсюда выходило `python3 -m rexon.app --session-splash`
    // — команда, которую автозапуск и юнит выполняют без рабочего каталога,
    // а пакета `rexon` в `sys.path` системного интерпретатора нет: заставка
    // при входе молча не показывалась (починено 16.09.2026, блок H).
    let python = inst::LaunchEntry::Python;
    for got in [
        ss::desktop_launcher_exec(&python, None, py),
        ss::unit_launcher_exec(&python, None, py),
        ss::launcher_exec(None, py),
        ss::launcher_exec(Some("  "), py),
    ] {
        assert_eq!(got, "", "откат без лаунчера обязан молчать, а не звать python");
    }
    assert_eq!(tray::autostart_exec(&python, None), None);
    // У дефолтов есть своё последнее звено — САМ работающий бинарь Rexon:
    // он и обрабатывает `--apply-defaults`, посредник не нужен.
    assert_eq!(
        ad::autostart_exec_for(&python, None, Some("/opt/rexon/bin/rexon")).as_deref(),
        Some("/opt/rexon/bin/rexon")
    );
    assert_eq!(ad::autostart_exec_for(&python, Some(""), None), None);
    // Лаунчер найден — все три хука зовут ЕГО, и полным путём.
    assert_eq!(
        ss::desktop_launcher_exec(&python, launcher, py),
        "/usr/local/bin/rexon --session-splash"
    );
    assert_eq!(
        tray::autostart_exec(&python, launcher).as_deref(),
        Some("/usr/local/bin/rexon --tray")
    );
    assert_eq!(
        ad::autostart_exec_for(&python, launcher, None).as_deref(),
        Some("/usr/local/bin/rexon")
    );

    // Кавычки: у `.desktop` и у systemd правила разные.
    let odd = inst::LaunchEntry::MainWindow("/home/u/Rexon v5/rexon-main-window".into());
    assert_eq!(
        ss::desktop_launcher_exec(&odd, None, py),
        "\"/home/u/Rexon v5/rexon-main-window\" --session-splash"
    );
    assert_eq!(
        ss::unit_launcher_exec(&odd, None, py),
        "\"/home/u/Rexon v5/rexon-main-window\" --session-splash"
    );
    assert_eq!(
        ad::autostart_exec_for(&odd, None, None).as_deref(),
        Some("\"/home/u/Rexon v5/rexon-main-window\"")
    );
    let weird = inst::LaunchEntry::MainWindow("/a$b \"c\"\\d/rexon-main-window".into());
    assert_eq!(
        ss::unit_launcher_exec(&weird, None, py),
        "\"/a$$b \\\"c\\\"\\\\d/rexon-main-window\" --session-splash"
    );
    assert_eq!(ss::systemd_exec_arg("/a%b/x"), "/a%%b/x");
    assert_eq!(ss::systemd_exec_arg(RUST_BIN), RUST_BIN);
}

#[test]
fn путь_с_пробелом_берётся_в_кавычки() {
    assert_eq!(inst::desktop_exec_arg("/usr/bin/x"), "/usr/bin/x");
    assert_eq!(inst::desktop_exec_arg("/опт/Загрузки/x"), "/опт/Загрузки/x");
    assert_eq!(inst::desktop_exec_arg("/a b/x"), "\"/a b/x\"");
    assert_eq!(inst::desktop_exec_arg("/a$b/x"), "\"/a\\\\$b/x\"");
    assert_eq!(inst::desktop_exec_arg("/a%b/x"), "/a%%b/x");
    let e = inst::LaunchEntry::MainWindow("/a b/rexon-main-window".into());
    assert_eq!(e.exec(&["--tray"]), "\"/a b/rexon-main-window\" --tray");
    assert_eq!(e.try_exec(), "/a b/rexon-main-window");
}

/// Сгенерированные ярлыки проходят `desktop-file-validate` — на обоих входах
/// и на пути с пробелом и кириллицей. Каталог — во временном каталоге
/// сборки (под `rust/target`), не в `/tmp`. Нет утилиты — тест говорит об
/// этом вслух, а не зеленеет молча.
#[test]
fn ярлыки_проходят_desktop_file_validate() {
    let Ok(out) = std::process::Command::new("desktop-file-validate").arg("--help").output()
    else {
        eprintln!("НЕ ПРОВЕРЕНО: desktop-file-validate не установлен");
        return;
    };
    assert!(out.status.success());
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("desktop-validate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let entries = [
        inst::LaunchEntry::Python,
        rust_entry(),
        inst::LaunchEntry::MainWindow("/home/u/Загрузки/Rexon v5/rexon-main-window".into()),
    ];
    for (i, entry) in entries.iter().enumerate() {
        for (kind, text) in [
            ("rexon", inst::desktop_content_for(entry)),
            ("rexon-recover", inst::recover_desktop_content_for(entry)),
        ] {
            let path = dir.join(format!("{kind}-{i}.desktop"));
            std::fs::write(&path, &text).unwrap();
            let r = std::process::Command::new("desktop-file-validate").arg(&path).output().unwrap();
            let said = String::from_utf8_lossy(&r.stdout).to_string()
                + &String::from_utf8_lossy(&r.stderr);
            assert!(r.status.success() && said.trim().is_empty(), "{kind} {entry:?}: {said}");
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Ярлыки пакета печатает генератор ядра (`desktop-entry --package`), а
/// PKGBUILD ставит бинарь окна ровно туда, куда этот ярлык ведёт.
///
/// Прежде пакет вёз статичную копию `rexon.desktop`, и она отстала при
/// первой же правке генератора: ярлык пакета остался на Python-входе. Тест
/// краснеет, если (а) программа печати разойдётся с генератором, (б)
/// статичная копия вернётся в пакет, (в) путь установки окна в PKGBUILD
/// разойдётся с [`inst::PACKAGE_MAIN_WINDOW`].
#[test]
fn ярлыки_пакета_печатает_генератор_ядра() {
    let run = |args: &[&str]| {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_desktop-entry"))
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let package = inst::LaunchEntry::MainWindow(inst::PACKAGE_MAIN_WINDOW.to_string());
    assert_eq!(run(&["--package"]), inst::desktop_content_for(&package));
    assert_eq!(run(&["--package", "--recover"]), inst::recover_desktop_content_for(&package));
    assert_eq!(run(&["--python"]), inst::desktop_content_for(&inst::LaunchEntry::Python));
    let odd = "/home/u/Загрузки/Rexon v5/rexon-main-window";
    assert_eq!(
        run(&["--main-window", odd]),
        inst::desktop_content_for(&inst::LaunchEntry::MainWindow(odd.into()))
    );
    // Автозапуск печатается теми же функциями, которыми его пишет галочка в трее:
    // ярлык и автозапуск обязаны вести на один вход.
    let window_autostart = run(&["--main-window", odd, "--autostart"]);
    assert_eq!(
        window_autostart,
        rexon_core::tray::autostart_desktop_content(
            &rexon_core::tray::autostart_exec(&inst::LaunchEntry::MainWindow(odd.into()), None)
                .unwrap()
        )
    );
    assert!(
        window_autostart.contains(rexon_core::tray::START_HIDDEN_FLAG),
        "автозапуск Rust-окна без скрытого старта: {window_autostart}"
    );
    assert_eq!(
        run(&["--python", "--autostart"]),
        rexon_core::tray::autostart_desktop_content(
            &rexon_core::tray::autostart_exec(
                &inst::LaunchEntry::Python,
                Some(inst::PYTHON_ENTRY_CMD)
            )
            .unwrap()
        )
    );
    for bad in [
        &[][..],
        &["--package", "--python"],
        &["--main-window", "rel"],
        &["--x"],
        &["--package", "--recover", "--autostart"],
    ] {
        let st = std::process::Command::new(env!("CARGO_BIN_EXE_desktop-entry"))
            .args(bad)
            .output()
            .unwrap()
            .status;
        assert_eq!(st.code(), Some(2), "{bad:?}");
    }

    let aur = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/aur");
    let pkgbuild = std::fs::read_to_string(aur.join("PKGBUILD")).unwrap();
    for name in ["rexon.desktop", "rexon-recover.desktop"] {
        assert!(!aur.join(name).exists(), "статичная копия {name} вернулась в пакет");
    }
    assert!(pkgbuild.contains("desktop-entry --package"), "PKGBUILD не печатает ярлык ядром");
    assert!(
        pkgbuild.contains(&format!("\"$pkgdir{}\"", inst::PACKAGE_MAIN_WINDOW)),
        "PKGBUILD ставит окно не туда, куда ведёт ярлык ({})",
        inst::PACKAGE_MAIN_WINDOW
    );
}

/// Пакет везёт оба QML-плагина вкладки обоев, и редакция `Qcm.Material` в
/// нём та же, что выписана рядом с вендором.
///
/// Без обоих плагинов `qml_modules::qml_plugins_ready` говорит «нет», и
/// вендорные страницы не грузятся ни в одном окне. Разъехавшийся пин — самый
/// неприятный случай: QML новый, библиотека старая, и видно это только
/// глазами на живой странице.
#[test]
fn плагины_вкладки_обоев_едут_в_пакете() {
    use rexon_core::qml_modules as qm;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pkgbuild = std::fs::read_to_string(root.join("packaging/aur/PKGBUILD")).unwrap();

    let want = std::fs::read_to_string(
        root.join("data/qt_wallpaper_engine/qml_ui/waywallen/ui/QML_MATERIAL_PIN"),
    )
    .unwrap();
    let want = want.trim();
    assert!(
        pkgbuild.contains(&format!("_qm_pin={want}")),
        "пин Qcm.Material в PKGBUILD разошёлся с вендорным ({want})"
    );
    assert!(
        pkgbuild.contains("#commit=${_qm_pin}"),
        "источник QmlMaterial не закреплён коммитом"
    );
    // Каталог модулей и оба плагина — именами из ядра, не переписанными руками.
    assert!(
        pkgbuild.contains(&format!("$pkgdir{}", qm::SYSTEM_MODULE_DIRS[0])),
        "пакет кладёт QML-модули не туда, где ядро их ищет ({})",
        qm::SYSTEM_MODULE_DIRS[0]
    );
    for rel in [qm::QCM_MATERIAL_REL, qm::WAYWALLEN_CONTROL_REL] {
        let (dir, _) = rel.rsplit_once('/').expect("у плагина есть каталог модуля");
        assert!(pkgbuild.contains(dir), "в PKGBUILD не назван модуль {dir}");
    }
}

/// Массивы PKGBUILD, как их видит makepkg: файл исполняет сам bash с
/// заданным `REXON_BUNDLED_ENGINE` (`None` — переменной нет вовсе).
/// Возвращает `имя → значения` для depends, license, source, sha256sums,
/// noextract.
fn pkgbuild_arrays(
    bundled: Option<&str>,
) -> std::collections::HashMap<&'static str, Vec<String>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cmd = std::process::Command::new("bash");
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .arg("-c")
        .arg(
            r#"source "$1" || exit 1
for a in depends license source sha256sums noextract; do
  declare -n ref=$a
  for v in "${ref[@]}"; do printf '%s\t%s\n' "$a" "$v"; done
done"#,
        )
        .arg("pkgbuild")
        .arg(root.join("packaging/aur/PKGBUILD"));
    if let Some(v) = bundled {
        cmd.env("REXON_BUNDLED_ENGINE", v);
    }
    let out = cmd.output().expect("bash для чтения PKGBUILD");
    assert!(out.status.success(), "bash не прочёл PKGBUILD: {}", String::from_utf8_lossy(&out.stderr));
    let mut m: std::collections::HashMap<&'static str, Vec<String>> = Default::default();
    for line in String::from_utf8(out.stdout).unwrap().lines() {
        let (k, v) = line.split_once('\t').unwrap();
        let k = ["depends", "license", "source", "sha256sums", "noextract"]
            .into_iter()
            .find(|n| *n == k)
            .unwrap();
        m.entry(k).or_default().push(v.to_string());
    }
    m
}

/// Тег, с которого сняты протокол и вендорный интерфейс (без `v`).
fn upstream_tag() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tag = std::fs::read_to_string(root.join("data/qt_wallpaper_engine/proto/UPSTREAM_TAG")).unwrap();
    tag.split_whitespace().next().unwrap().trim_start_matches('v').to_string()
}

/// Движок обоев по умолчанию — отдельные пакеты дистрибутива (с 30.10.2026),
/// а не вшитый AppImage; так же описан и `.SRCINFO`.
///
/// Каждое условие — отдельная поломка: нижняя граница демона разошлась с
/// тегом, с которого сняты протокол и интерфейс, — пакет обещает работу с
/// демоном, которого ядро не понимает; AppImage или архив исходников OWE
/// просочились в вариант по умолчанию — снова то, что сопровождающие
/// дистрибутива не пустят; ядро ищет демон не там, куда его ставит пакет
/// `waywallen`.
#[test]
fn packaged_engine_comes_from_system_packages() {
    use rexon_core::we_client as wc;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pkgbuild = std::fs::read_to_string(root.join("packaging/aur/PKGBUILD")).unwrap();
    let tag = upstream_tag();
    assert!(
        pkgbuild.contains(&format!("\n_ww_min={tag}\n")),
        "_ww_min в PKGBUILD разошёлся с proto/UPSTREAM_TAG ({tag})"
    );
    assert!(pkgbuild.contains("\n_owe_min="), "нет нижней границы плагина");
    // Переменные — до depends: bash раскрывает массив в момент присваивания.
    let at = |needle: &str| pkgbuild.find(needle).unwrap_or_else(|| panic!("нет {needle}"));
    assert!(at("\n_ww_min=") < at("depends=(") && at("\n_owe_min=") < at("depends=("));
    assert!(at("REXON_BUNDLED_ENGINE") < at("depends=("), "переключатель варианта — до depends");

    // Без переменной и с `0` — один и тот же вариант по умолчанию.
    let unset = pkgbuild_arrays(None);
    assert_eq!(unset, pkgbuild_arrays(Some("0")), "REXON_BUNDLED_ENGINE=0 обязан совпасть с умолчанием");
    let deps = &unset["depends"];
    let owe_min = pkgbuild.split("\n_owe_min=").nth(1).unwrap().lines().next().unwrap();
    assert!(deps.contains(&format!("waywallen>={tag}")), "нет зависимости на пакет демона: {deps:?}");
    assert!(
        deps.contains(&format!("open-wallpaper-engine>={owe_min}")),
        "нет зависимости на пакет Open Wallpaper Engine: {deps:?}"
    );
    for gone in ["fuse3", "at-spi2-core"] {
        assert!(!deps.iter().any(|d| d == gone), "в варианте по умолчанию лишняя зависимость {gone}");
    }
    assert!(!unset["license"].iter().any(|l| l == "GPL-2.0-only"));
    assert!(
        !unset["source"].iter().any(|s| s.contains("AppImage") || s.contains("open-wallpaper-engine")),
        "в источники по умолчанию просочился вшитый движок: {:?}",
        unset["source"]
    );
    assert_eq!(unset["source"].len(), unset["sha256sums"].len());

    // .SRCINFO описывает ровно вариант по умолчанию.
    let srcinfo = std::fs::read_to_string(root.join("packaging/aur/.SRCINFO")).unwrap();
    let field = |name: &str| -> Vec<String> {
        srcinfo
            .lines()
            .filter_map(|l| l.trim().strip_prefix(&format!("{name} = ")).map(str::to_string))
            .collect()
    };
    assert_eq!(field("depends"), *deps, ".SRCINFO отстал от depends по умолчанию (makepkg --printsrcinfo)");
    assert_eq!(field("source"), unset["source"], ".SRCINFO отстал от source по умолчанию");
    assert_eq!(field("license"), unset["license"]);

    // Ядро ищет движок там, куда его ставят пакеты Arch.
    assert_eq!(wc::SYSTEM_DAEMON, "/usr/bin/waywallen");
    assert_eq!(
        wc::engine_layouts()[0].plugins_dir(),
        "/usr/share/waywallen/plugins",
        "open-wallpaper-engine ставит плагин в share/waywallen/plugins (install.lua)"
    );
}

/// Вариант `REXON_BUNDLED_ENGINE=1`: движок едет в пакете туда, где его ищет
/// ядро (`we_client::ENGINE_DIR`), с GPL-исходниками OWE и без зависимостей
/// на пакеты движка.
///
/// Каждое условие — отдельная поломка у человека: путь разошёлся — ядро не
/// видит движок и шлёт на экран «не установлен»; strip задел каталог
/// движка — от AppImage отрезан squashfs, движок мёртв; нет архива
/// исходников или GPL-2.0-only в license — раздача GPL-плагина без
/// исходника (GPL-2 §3(a)); остались depends на waywallen — пакет не
/// поставить там, ради чего вариант и есть; источник назван дважды —
/// makepkg отказывает.
#[test]
fn bundled_engine_variant_ships_where_core_looks() {
    use rexon_core::we_client as wc;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pkgbuild = std::fs::read_to_string(root.join("packaging/aur/PKGBUILD")).unwrap();
    let tag = upstream_tag();
    let b = pkgbuild_arrays(Some("1"));
    let deps = &b["depends"];
    for need in ["fuse3", "at-spi2-core"] {
        assert!(deps.iter().any(|d| d == need), "во вшитом варианте нет зависимости {need}");
    }
    assert!(
        !deps.iter().any(|d| d.starts_with("waywallen") || d.starts_with("open-wallpaper-engine")),
        "вшитый вариант требует пакеты движка: {deps:?}"
    );
    assert!(b["license"].iter().any(|l| l == "GPL-2.0-only"), "license=() молчит про GPL-2.0-only плагина");

    // Суммы на каждый источник, имена файлов не повторяются.
    assert_eq!(b["source"].len(), b["sha256sums"].len(), "source и sha256sums разной длины");
    let names: Vec<&str> = b["source"].iter().map(|s| s.split("::").next().unwrap()).collect();
    let uniq: std::collections::HashSet<&&str> = names.iter().collect();
    assert_eq!(uniq.len(), names.len(), "источник назван дважды: {names:?}");
    // Вариант по умолчанию — префикс вшитого: суммы и порядок не сдвинуты.
    let d = pkgbuild_arrays(None);
    assert_eq!(b["source"][..d["source"].len()], d["source"][..]);
    assert_eq!(b["sha256sums"][..d["sha256sums"].len()], d["sha256sums"][..]);
    // AppImage — ровно тег протокола (prepare() сверяет то же на сборке).
    let appimage = format!("waywallen-{tag}-x86_64.AppImage");
    assert!(
        b["source"].iter().any(|s| s.starts_with(&format!("{appimage}::"))),
        "вшитый AppImage не тег протокола {tag}: {:?}",
        b["source"]
    );
    assert!(b["noextract"].iter().any(|s| s.starts_with("org.waywallen.open-wallpaper-engine-")));

    // Раскладка в package() — та, что ищет ядро.
    assert!(
        pkgbuild.contains(&format!("local eng=\"$pkgdir{}\"", wc::ENGINE_DIR)),
        "пакет кладёт движок не туда, где его ищет ядро ({})",
        wc::ENGINE_DIR
    );
    assert!(
        pkgbuild.contains(&format!("\"$eng/{}\"", wc::ENGINE_APPIMAGE_NAME)),
        "имя AppImage в пакете разошлось с ядром"
    );
    assert!(
        pkgbuild.contains(&format!("\"$eng/{}/$_owe_id\"", wc::ENGINE_PLUGINS_SUBDIR)),
        "плагин OWE лежит не в <движок>/plugins — демон его не увидит"
    );
    assert!(pkgbuild.contains("\n_owe_id=org.waywallen.open-wallpaper-engine\n"));
    assert!(pkgbuild.contains("'!strip'"), "без !strip makepkg уничтожит AppImage");
    assert!(
        pkgbuild.contains(&format!("-path \"$pkgdir{}\"", wc::ENGINE_DIR)),
        "_strip_own_binaries не обходит каталог движка — strip отрежет squashfs AppImage"
    );
    assert!(pkgbuild.contains("\"$pkgdir/usr/share/rexon/sources/open-wallpaper-engine-${_owe_ver}-src.tar.xz\""));
    // Сторож тега — для обоих вариантов.
    assert!(pkgbuild.contains("\"$tag\" != \"v$_ww_min\""), "нет сторожа _ww_min");
    assert!(pkgbuild.contains("\"$tag\" != \"v$_ww_ver\""), "нет сторожа вшитого _ww_ver");

    // Ядро держит вшитую раскладку запасной, после системной.
    let layouts = wc::engine_layouts();
    let bundled = layouts
        .iter()
        .position(|l| l.kind == wc::EngineKind::Bundled && l.dir == wc::ENGINE_DIR)
        .expect("ядро не знает вшитой раскладки");
    let system = layouts.iter().position(|l| l.kind == wc::EngineKind::System).unwrap();
    assert!(system < bundled, "системный движок обязан идти первым");
    assert_eq!(layouts[bundled].daemon, wc::engine_appimage_path(wc::ENGINE_DIR));
}

/// Любое значение переключателя, кроме 0 и 1, сборку останавливает, а не
/// молча собирает вариант по умолчанию.
#[test]
fn bundled_engine_switch_rejects_garbage() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("bash")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("REXON_BUNDLED_ENGINE", "yes")
        .arg("-c")
        .arg(r#"source "$1" >/dev/null 2>&1; srcdir=/nonexistent; prepare"#)
        .arg("pkgbuild")
        .arg(root.join("packaging/aur/PKGBUILD"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "prepare() принял REXON_BUNDLED_ENGINE=yes");
    assert!(String::from_utf8_lossy(&out.stderr).contains("REXON_BUNDLED_ENGINE=yes"));
}

#[test]
fn paths() {
    assert_eq!(
        app::appletsrc_path("/home/u"),
        "/home/u/.config/plasma-org.kde.plasma.desktop-appletsrc"
    );
    assert_eq!(
        app::appletsrc_backup_path("/home/u/.config/x"),
        "/home/u/.config/x.rexon-bak"
    );
}

// ============================================================================
// --recover-kde: возврат обоев рабочих столов к стандартным
// ============================================================================

fn wp() -> Value {
    fx()["wallpaper_recover"].clone()
}

#[test]
fn desktop_containments_from_appletsrc() {
    let g = wp();
    // ⚠️ Расхождение с Python ОСОЗНАННОЕ (правило #16): оригинал не
    // сбрасывал «текущий контейнмент» на чужом заголовке секции, поэтому
    // апплет «Папки» внутри ПАНЕЛИ засчитывался панели — она попадала в
    // список «рабочих столов» (снятый эталон: 1, 2, 3 — где 2 это
    // plugin=org.kde.panel). Здесь панель отсеивается.
    let got = app::desktop_containment_ids(s(&g["appletsrc"]));
    assert_eq!(got, strs(&g["containments_fixed"]));
    assert_eq!(strs(&g["containments_python_buggy"]), vec!["1", "2", "3"]);
    assert!(!got.contains(&"2".to_string()), "панель не рабочий стол");

    // Обычные случаи с оригиналом совпадают байт-в-байт.
    assert_eq!(
        app::desktop_containment_ids(s(&g["appletsrc_simple"])),
        strs(&g["containments_simple"])
    );
    assert!(app::desktop_containment_ids(s(&g["appletsrc_no_desktop"])).is_empty());
    assert!(app::desktop_containment_ids("").is_empty());
    // Вложенный заголовок сам контейнментом не считается.
    assert!(app::desktop_containment_ids(
        "[Containments][1][Applets][2]\nplugin=org.kde.plasma.folder\n"
    )
    .is_empty());
    // Нецифровой номер не подходит (порт `\d+` в регулярном выражении).
    assert!(app::desktop_containment_ids(
        "[Containments][abc]\nplugin=org.kde.desktopcontainment\n"
    )
    .is_empty());
}

#[test]
fn wallpaper_reset_argv() {
    let g = wp();
    let kw = "/usr/bin/kwriteconfig6";
    assert_eq!(
        app::wallpaper_plugin_reset_argv(kw, "1"),
        strs(&g["wallpaper_plugin_argv"])
    );
    assert_eq!(
        app::wallpaper_image_argv(kw, "1", "/usr/share/wallpapers/Next/"),
        strs(&g["wallpaper_image_argv"])
    );
    let restart: Vec<Vec<&str>> = g["plasmashell_restart_argv"]
        .as_array()
        .unwrap()
        .iter()
        .map(strs)
        .collect();
    assert_eq!(app::plasmashell_restart_argv(), restart);
    assert_eq!(app::DEFAULT_WALLPAPER_DIRS, strs(&g["default_wallpaper_dirs"]));
    assert_eq!(app::FALLBACK_CONTAINMENT, s(&g["fallback_containment"]));
    assert_eq!(app::PYSIDE6_INSTALL_HINT, strs(&g["pyside6_hint"]));
}

// ============================================================================
// --recover-kde: слова, которые увидит человек
// ============================================================================
//
// ⭐ СТОРОЖ ТЕКСТОВ. Маршрут аварийного отката переехал на Rust целиком, и
// его строки живут ЗДЕСЬ. С 02.10.2026 эталон — сами строки Rust, записанные
// ниже дословно (`русский_откат_как_был`, `собранные_строки_…`); прежняя
// сверка фрагментов с замороженной редакцией `rexon/app.py` снята вместе с
// Python. Переименовали подпись у нас — сверка краснеет, пока эталон не
// поправлен осознанно, и расхождение видно сразу, а не на живой машине
// человека, у которого сломано оформление.

#[test]
fn отказ_на_чужом_рабочем_столе_теми_же_словами() {
    let _lang = i18n_pin::ru();
    assert!(app::is_plasma_desktop("KDE"));
    assert!(app::is_plasma_desktop("plasmawayland"));
    // Пустая переменная — «неизвестно», а не «чужой стол»: маршрут зовут и
    // из tty, когда сеанс уже не поднимается.
    assert!(app::is_plasma_desktop(""));
    assert!(!app::is_plasma_desktop("GNOME"));
    assert!(!app::is_plasma_desktop("XFCE"));

    let lines = app::not_plasma_lines("GNOME");
    assert_eq!(lines.len(), 3);
    assert!(lines[1].contains("GNOME"), "{}", lines[1]);
    assert_eq!(
        lines[0],
        "⚠️  Аварийное восстановление оформления пока умеет только KDE Plasma."
    );
    assert_eq!(lines[1], "    Текущий рабочий стол: GNOME. Ничего не изменено.");
}

#[test]
fn тема_берётся_из_резервной_копии_а_иначе_дефолтная() {
    assert_eq!(app::recover_theme_target(Some(" org.kde.oxygen.desktop \n")), "org.kde.oxygen.desktop");
    assert_eq!(app::recover_theme_target(Some("   ")), app::RECOVER_DEFAULT_TARGET);
    assert_eq!(app::recover_theme_target(None), app::RECOVER_DEFAULT_TARGET);
    assert_eq!(app::RECOVER_DEFAULT_TARGET, "org.kde.breezedark.desktop", "тема по умолчанию");
    assert_eq!(app::RECOVER_COLORSCHEME, "BreezeDark", "цветовая схема");
    assert_eq!(
        app::lookandfeel_apply_argv("X"),
        vec!["plasma-apply-lookandfeel", "-a", "X"]
    );
    assert_eq!(
        app::colorscheme_apply_argv(),
        vec!["plasma-apply-colorscheme", app::RECOVER_COLORSCHEME]
    );
}

#[test]
fn предупреждение_о_форках_совпадает_с_оригиналом() {
    let _lang = i18n_pin::ru();
    let forks = vec!["aeroshell-workspace-git".to_string()];
    let with_cmd = app::fork_warning_lines(&forks, true, "sudo pacman -Rns X");
    assert_eq!(with_cmd[1], "      • aeroshell-workspace-git");
    assert!(with_cmd[3].contains("sudo pacman -Rns X"), "{:?}", with_cmd);
    // Целей в репозиториях нет — команду НЕ печатаем, иначе человек
    // выполнит её руками и получит «не найдена цель».
    let without = app::fork_warning_lines(&forks, false, "Первая\nВторая");
    assert_eq!(without[2], "    Первая");
    assert_eq!(without[3], "    Вторая");
    assert!(
        !without.iter().any(|l| l.contains("удалите их")),
        "{without:?}"
    );

    // Слова заголовка и оговорок переписаны (см. JARGON_GONE), строй прежний:
    // заголовок, пакеты по одному, призыв, команда, оговорка.
    assert_eq!(with_cmd.len(), 5, "{with_cmd:?}");
    assert!(with_cmd[0].starts_with("⚠️  ") && with_cmd[0].ends_with(':'), "{}", with_cmd[0]);
    assert!(with_cmd[2].starts_with("    ") && with_cmd[2].ends_with("удалите их:"), "{}", with_cmd[2]);
    assert_eq!(with_cmd[3], "      sudo pacman -Rns X");
    assert!(with_cmd[4].contains("выйдите из сеанса и войдите снова"), "{}", with_cmd[4]);
}

#[test]
fn собранные_строки_читаются_как_у_оригинала() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        app::sddm_already_stock_line(Some("breeze")),
        "🖥️ Экран входа (SDDM) уже со стандартными настройками (тема «breeze»)."
    );
    assert_eq!(
        app::sddm_already_stock_line(None),
        "🖥️ Экран входа (SDDM) уже со стандартными настройками."
    );
    assert_eq!(
        app::sddm_removing_line(&["a.conf".into(), "b.conf".into()], Some("breeze")),
        "🖥️ Экран входа (SDDM): убираю сторонние настройки [a.conf, b.conf] → вернётся тема «breeze»."
    );
    assert!(
        app::sddm_removing_line(&["a.conf".into()], None).ends_with("вернётся вид по умолчанию дистрибутива."),
        "без имени темы — вид по умолчанию дистрибутива"
    );
    assert_eq!(
        app::applets_removed_line(&["x".into(), "y".into()], "/cfg/appletsrc.rexon-bak"),
        "🧱 С панели убраны сломанные виджеты от замен Plasma: 2 (x, y). Резервная копия: /cfg/appletsrc.rexon-bak"
    );
    assert_eq!(app::step_ok_line("готово"), "   ✅ готово");
    // Обрезка чужого stderr — ПО СИМВОЛАМ: срез посреди многобайтной буквы
    // в Rust паника, то есть маршрут падал бы на самой неудаче.
    let long = "я".repeat(500);
    let cut = app::apply_failed_line(&long);
    assert_eq!(cut.chars().count(), "❌ Ошибка: ".chars().count() + 200);
}

/// Обороты, от которых аварийный откат ушёл 01.10.2026: оригинал говорил
/// жаргоном («сток», «оверрайды», «logout/login», «родной дефолт»,
/// «форки», «апплеты», «бэкап», «reboot», «раскладка панели»), покупателю
/// непонятным, а аварийный откат как раз читает человек, у которого всё
/// сломалось. Ни одна русская строка отката не должна их вернуть. Новый
/// текст держат `собранные_строки_…` и `русский_откат_как_был`, строй —
/// `предупреждение_о_форках_…`.
const JARGON_GONE: &[(&str, &str)] = &[
    ("Обнаружены ПАКЕТЫ-ФОРКИ Plasma (подменяют ядро", "заголовок про пакеты-замены"),
    ("Чтобы вернуться на сток ПОЛНОСТЬЮ, удалите их:", "призыв к полному откату"),
    ("(затем logout/login). Сброс темы ниже", "оговорка про выход из сеанса"),
    ("Эффекты KWin от форк-тем сняты.", "снятие эффектов KWin"),
    ("Не удалось определить родной дефолт Plymouth", "стандартная заставка не определена"),
    ("применится после reboot", "смена загрузочной заставки"),
    ("уже на стоковом конфиге", "SDDM уже стандартный"),
    ("удаляю оверрайды [", "план сброса SDDM"),
    ("дистро-дефолту", "цель сброса SDDM без имени темы"),
    ("Найдены форки Plasma или их следы", "переход к чистке панели"),
    ("Из панели удалено битых апплетов форк-темы: ", "итог чистки панели"),
    ("lookandfeeltool не найден — раскладка панели не сброшена.", "нет lookandfeeltool"),
    ("Раскладка панели сброшена на стандартную (Kickoff).", "сброс панели"),
];

#[test]
fn жаргон_оригинала_заменён_осознанно() {
    let _lang = i18n_pin::ru();
    let lines = every_recover_line();
    assert!(lines.len() > 30, "сверять было нечего: {}", lines.len());
    for (old, what) in JARGON_GONE {
        assert!(
            !lines.iter().any(|l| l.contains(old)),
            "{what}: в русском тексте отката снова жаргон оригинала «{old}»"
        );
    }
    for jargon in ["сток", "оверрайд", "дефолт", "logout", "reboot", "форк", "апплет", "Бэкап"] {
        for l in &lines {
            assert!(!l.contains(jargon), "жаргон «{jargon}» в строке отката: {l}");
        }
    }
}

#[test]
fn стандартные_обои_выбираются_по_порядку() {
    let _lang = i18n_pin::ru();
    // Штатный кандидат есть — берём его и не смотрим дальше.
    let got = app::default_wallpaper_dir(
        &|p| p == "/usr/share/wallpapers/Next",
        &["Zebra".to_string()],
        &|_| true,
    );
    assert_eq!(got, "/usr/share/wallpapers/Next/", "слэш на конце обязателен");

    // Ни одного кандидата — берём ЛЮБОЙ установленный пакет обоев, но
    // только настоящий (с каталогом картинок внутри).
    let got = app::default_wallpaper_dir(
        &|_| false,
        &["Aaa".to_string(), "Zebra".to_string()],
        &|p| p == "/usr/share/wallpapers/Zebra/contents/images",
    );
    assert_eq!(got, "/usr/share/wallpapers/Zebra/");

    // Обоев в системе нет вовсе — имени не выдумываем.
    assert_eq!(app::default_wallpaper_dir(&|_| false, &[], &|_| false), "");
    assert_eq!(
        app::wallpaper_reset_line(2, ""),
        "🖼️  Обои сброшены на стандартные (на 2 рабочих столах)",
        "без картинки имя не называется"
    );
    assert!(app::wallpaper_reset_line(1, "/usr/share/wallpapers/Next/").ends_with(": Next"));
}

// ============================================================================
// Перевод аварийного отката
// ============================================================================
//
// Строки `--recover-kde` были зашиты по-русски; теперь исходный язык
// английский, русский — в `po/ru.po`. Сверки выше держат русский слово в
// слово, ниже — английский и откат без каталога.

fn cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// Все строки отката, которые может увидеть человек, — одним списком.
fn every_recover_line() -> Vec<String> {
    let mut all = app::not_plasma_lines("GNOME");
    all.extend(app::fork_warning_lines(&["aeroshell-plasma".into()], true, "sudo pacman -Rns x"));
    all.extend(app::fork_warning_lines(&["aeroshell-plasma".into()], false, "why"));
    all.extend([
        app::recovering_line("org.kde.breeze.desktop"),
        app::restored_line("org.kde.breeze.desktop"),
        app::apply_failed_line("boom"),
        app::FONTS_RESET_LINE.to_string(),
        app::KWIN_EFFECTS_RESET_LINE.to_string(),
        app::kvantum_breeze_line("SevenAero"),
        app::PLYMOUTH_TARGET_UNKNOWN_LINE.to_string(),
        app::plymouth_already_stock_line("bgrt"),
        app::plymouth_switch_line("aero", "bgrt"),
        app::plymouth_failed_line("x"),
        app::manual_command_line("sudo plymouth-set-default-theme -R bgrt"),
        app::sddm_already_stock_line(Some("breeze")),
        app::sddm_already_stock_line(None),
        app::sddm_removing_line(&["a.conf".into()], Some("breeze")),
        app::sddm_removing_line(&["a.conf".into()], None),
        app::sddm_failed_line("x"),
        app::PANEL_AGGRESSIVE_LINE.to_string(),
        app::applets_removed_line(&["x".into()], "/b"),
        app::applets_rewrite_failed_line("x"),
        app::LOOKANDFEELTOOL_MISSING_LINE.to_string(),
        app::PANEL_LAYOUT_RESET_LINE.to_string(),
        app::wallpaper_reset_line(1, ""),
        app::wallpaper_reset_line(2, "/usr/share/wallpapers/Next/"),
        app::LOOKANDFEEL_MISSING_LINE.to_string(),
        app::MSG_NOTHING_TO_LAUNCH.to_string(),
    ]);
    all
}

#[test]
fn english_recover_lines_have_no_russian() {
    let _lang = i18n_pin::en();
    for line in every_recover_line() {
        assert!(!cyrillic(&line), "русская буква в английском тексте: {line}");
    }
    assert_eq!(
        app::not_plasma_lines("GNOME"),
        vec![
            "⚠️  Emergency appearance recovery only supports KDE Plasma for now.",
            "    Current desktop: GNOME. Nothing was changed.",
            "    On this desktop, roll the appearance back with the desktop's own tools or \
             on the Appearance tab in Rexon.",
        ]
    );
    assert_eq!(app::recovering_line("X"), "🛟 Restoring the KDE theme: X");
    assert_eq!(app::restored_line("X"), "✅ Restored: X");
    assert_eq!(&**app::FONTS_RESET_LINE, "🔤 KDE fonts reset to the defaults.");
    assert_eq!(
        app::sddm_removing_line(&["a.conf".into(), "b.conf".into()], None),
        "🖥️ Login screen (SDDM): removing third-party settings [a.conf, b.conf] → it goes back \
         to the distribution default."
    );
    assert_eq!(
        app::plymouth_switch_line("aero", "bgrt"),
        "🐧 Boot splash: “aero” → “bgrt” (rebuilding the initramfs; takes effect after a reboot)."
    );
    assert_eq!(app::wallpaper_reset_line(1, ""), "🖼️  Wallpaper reset to the default one (1 desktop)");
    assert_eq!(
        app::wallpaper_reset_line(3, "/usr/share/wallpapers/Next/"),
        "🖼️  Wallpaper reset to the default one (3 desktops): Next"
    );
    assert_eq!(app::manual_command_line("cmd"), "   To do it by hand: cmd");
    // Обрезка чужого stderr по символам — и по-английски.
    let cut = app::apply_failed_line(&"я".repeat(500));
    assert_eq!(cut.chars().count(), "❌ Error: ".chars().count() + 200);
}

/// Аварийный маршрут зовут, когда система уже сломана: каталога может не
/// быть вовсе. Тогда строки — исходные английские, и ни одной паники.
#[test]
fn recover_lines_fall_back_to_english_without_catalog() {
    let _lang = i18n_pin::en();
    assert_eq!(i18n::set_language(Some("xx")), i18n::SOURCE_LANGUAGE, "каталога xx нет");
    let lines = every_recover_line();
    assert!(lines.iter().all(|l| !cyrillic(l)), "{lines:?}");
    assert_eq!(app::FONTS_RESET_LINE.msgid(), &**app::FONTS_RESET_LINE);
}

/// Константа-строка сравнивается и печатается как переведённый текст, на
/// том языке, что выбран сейчас, — а не на том, что был при первом
/// обращении.
#[test]
fn константа_отката_следует_за_языком() {
    let ru = {
        let _lang = i18n_pin::ru();
        let text: &str = app::PANEL_LAYOUT_RESET_LINE;
        assert_eq!(text, "🧱 Панель возвращена к стандартному виду (Kickoff).");
        assert_eq!(app::PANEL_LAYOUT_RESET_LINE.to_string(), text);
        assert!(text.to_string() == *app::PANEL_LAYOUT_RESET_LINE);
        text
    };
    let _lang = i18n_pin::en();
    let en: &str = app::PANEL_LAYOUT_RESET_LINE;
    assert_eq!(en, "🧱 Panel layout reset to the default one (Kickoff).");
    assert_ne!(en, ru);
    assert_eq!(
        app::MSG_NOTHING_TO_LAUNCH.to_string(),
        app::MSG_NOTHING_TO_LAUNCH.msgid(),
        "английский = исходная строка"
    );
}

/// Русский после переноса строк в каталог — как было до переноса (тексты
/// скопированы из прежнего `app.rs`, где были зашиты), кроме строк с
/// жаргоном, переписанных 01.10.2026 (см. [`JARGON_GONE`]): у них здесь новый
/// текст.
#[test]
fn русский_откат_как_был() {
    let _lang = i18n_pin::ru();
    let forks = vec!["aeroshell-plasma".to_string()];
    let before: Vec<(String, &str)> = vec![
        (app::not_plasma_lines("GNOME")[0].clone(), "⚠️  Аварийное восстановление оформления пока умеет только KDE Plasma."),
        (app::not_plasma_lines("GNOME")[1].clone(), "    Текущий рабочий стол: GNOME. Ничего не изменено."),
        (
            app::not_plasma_lines("GNOME")[2].clone(),
            "    Для него откат оформления делается средствами самого рабочего стола либо через \
             вкладку «Оформление» в Rexon.",
        ),
        (
            app::fork_warning_lines(&forks, true, "cmd")[0].clone(),
            "⚠️  Установлены ПАКЕТЫ-ЗАМЕНЫ частей Plasma (они подменяют саму Plasma, и сброс темы \
             их НЕ убирает):",
        ),
        (app::fork_warning_lines(&forks, true, "cmd")[2].clone(), "    Чтобы ПОЛНОСТЬЮ вернуть обычную Plasma, удалите их:"),
        (
            app::fork_warning_lines(&forks, true, "cmd")[4].clone(),
            "    (затем выйдите из сеанса и войдите снова). Сброс темы ниже меняет только внешний \
             вид — замены частей Plasma останутся.",
        ),
        (app::recovering_line("T"), "🛟 Восстанавливаю KDE-тему: T"),
        (app::restored_line("T"), "✅ Восстановлено: T"),
        (app::apply_failed_line("e"), "❌ Ошибка: e"),
        (app::FONTS_RESET_LINE.to_string(), "🔤 Шрифты KDE сброшены на стандартные."),
        (app::KWIN_EFFECTS_RESET_LINE.to_string(), "🪟 Эффекты KWin, оставленные заменами Plasma, сняты."),
        (app::kvantum_breeze_line("SevenAero"), "🎨 Тема Kvantum «SevenAero» (Aero) → стиль Breeze."),
        (app::PLYMOUTH_TARGET_UNKNOWN_LINE.to_string(), "⚠️ Не удалось узнать стандартную загрузочную заставку дистрибутива (Plymouth) — этот шаг пропущен."),
        (app::plymouth_already_stock_line("bgrt"), "🐧 Загрузочная заставка уже стандартная (bgrt)."),
        (
            app::plymouth_switch_line("aero", "bgrt"),
            "🐧 Загрузочная заставка: «aero» → «bgrt» (пересобирается initramfs, заработает после \
             перезагрузки).",
        ),
        (app::plymouth_failed_line("m"), "   ⚠️ Plymouth не сброшен: m"),
        (app::manual_command_line("c"), "   Вручную: c"),
        (app::sddm_failed_line("m"), "   ⚠️ SDDM не сброшен: m"),
        (app::PANEL_AGGRESSIVE_LINE.to_string(), "🔧 Найдены замены Plasma или их следы — убираю их остатки с панели."),
        (app::applets_rewrite_failed_line("e"), "⚠️ Не удалось переписать панель: e"),
        (app::LOOKANDFEELTOOL_MISSING_LINE.to_string(), "⚠️ Программа lookandfeeltool не найдена — панель не возвращена к стандартному виду."),
        (app::PANEL_LAYOUT_RESET_LINE.to_string(), "🧱 Панель возвращена к стандартному виду (Kickoff)."),
        (app::wallpaper_reset_line(1, "/usr/share/wallpapers/Next/"), "🖼️  Обои сброшены на стандартные (на 1 рабочем столе): Next"),
        (app::wallpaper_reset_line(5, "/usr/share/wallpapers/Next/"), "🖼️  Обои сброшены на стандартные (на 5 рабочих столах): Next"),
        (app::wallpaper_reset_line(22, ""), "🖼️  Обои сброшены на стандартные (на 22 рабочих столах)"),
        (
            app::LOOKANDFEEL_MISSING_LINE.to_string(),
            "❌ Ошибка: не удалось запустить plasma-apply-lookandfeel — KDE Plasma в системе не найдена. \
             Ничего не изменено.",
        ),
        (
            app::MSG_NOTHING_TO_LAUNCH.to_string(),
            "Не нашли, чем запускать Rexon: нет ни программы окна (из пакета, из установки в систему \
             или в домашнюю папку, из дерева разработчика), ни команды `rexon` в PATH. Запись \
             автозапуска не создана — она указывала бы в пустоту.",
        ),
    ];
    for (got, want) in before {
        assert_eq!(got, want);
    }
}
