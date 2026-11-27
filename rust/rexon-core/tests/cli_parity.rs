//! Entry point guard: the Rust entry understands EVERYTHING the product understands.
//!
//! WHY. Qt silently ignores an unknown argument, so a missing flag
//! neither crashes nor complains — it just opens the normal window instead of
//! the promised work. A defect of this kind cannot be seen by eye: with
//! the flag and without it the screen looks the same. So it must be caught by
//! a guard that reads BOTH sources.
//!
//! WHAT WE GUARD. The expectations are the golden file `tests/golden/cli_cases.json`
//! (`include_str!`, i.e. rebuilt on any edit of the golden file);
//! the checks turn red on their own:
//!
//!   1. every service flag of the golden file is either handled in
//!      [`rexon_core::cli::run_headless`] with an exit code, or routed
//!      to a window section — not one of them can silently fall into the main window;
//!   2. Qt's own translation catalogs and the separator are as in the golden file, otherwise
//!      English Qt buttons will again appear in the middle of a Russian window (language and
//!      fallback — the Rexon language, fallback to English);
//!   3. the budget and the slice of the background task drain are as in the golden file.
//!
//! ORIGIN OF THE GOLDEN FILE. The values were captured from the former Python entry point
//! (`rexon/app.py`, `rexon/qt_main/app.py`, `rexon/utils.py`); until 02.10.2026
//! the check read their frozen sources. By the owner's decision of 02.10.2026
//! Python in Rexon is only an archive, and the golden file became Rust's own contract:
//! an edit on our side turns the check red until the golden file is updated deliberately.
//!
//! No run here launches or writes anything: the routes
//! are checked on argument parsing, not on execution.

use rexon_core::{app, cli, exit_guard};

use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/cli_cases.json")).unwrap()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .expect("массив строк")
        .iter()
        .map(|x| x.as_str().expect("строка").to_string())
        .collect()
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Run a route "dry": the output is collected, nothing is launched.
fn dry(flag: &str) -> (Option<i32>, Vec<String>) {
    let mut said: Vec<String> = Vec::new();
    let rc = {
        let mut out = |s: &str| said.push(s.to_string());
        cli::run_headless(&argv(&[flag]), &mut out)
    };
    (rc, said)
}

// ============================================================================
// 1. No branch of the route silently falls into the main window
// ============================================================================

/// Service route flags — the ones parsed BEFORE Qt. The list comes from the
/// golden file: every flag must lead to ITS OWN route, so a flag renamed
/// on our side turns the guard red rather than quietly dropping out of the check.
fn headless_flags() -> Vec<String> {
    strings(&golden()["headless_flags"])
}

#[test]
fn headless_flags_lead_to_their_own_routes() {
    let flags = headless_flags();
    assert!(!flags.is_empty(), "в эталоне нет служебных флагов");
    let want = [
        ("--apply-defaults", app::Route::ApplyDefaults),
        ("--sanitize-gtk", app::Route::SanitizeGtk),
        ("--session-splash", app::Route::SessionSplash),
    ];
    let got: Vec<&str> = want.iter().map(|(f, _)| *f).collect();
    assert_eq!(flags, got, "служебные флаги разошлись с эталоном");
    for (flag, route) in want {
        assert_eq!(app::route(&argv(&[flag])), route, "флаг {flag}");
    }
    let recover = golden()["recover_flag"].as_str().unwrap().to_string();
    assert_eq!(app::RECOVER_KDE_FLAG, recover, "флаг аварийного отката");
}

/// ⭐ THE MAIN CHECK. A service flag MUST have an exit code: Python
/// returned 0/1 (`return 0 if ok else 1`), the Rust binary before the fix returned
/// nothing at all — the autostart hook reported zero on failure.
#[test]
fn у_каждого_служебного_флага_есть_код_возврата() {
    for flag in headless_flags() {
        let route = app::route(&argv(&[&flag]));
        assert!(
            !matches!(route, app::Route::MainWindow),
            "флаг {flag} разбирается как главное окно"
        );
    }
    // ⛔ THE RUN ITSELF IS NOT DONE HERE FOR ANY OF THEM: all four have
    // side effects on a live machine. `--apply-defaults` applies the
    // application defaults, `--session-splash` launches mpv,
    // `--sanitize-gtk` edits GTK configs, and `--recover-kde` changes the owner's theme,
    // login screen and wallpaper. The order of steps and the words of each route
    // are checked on FAKE I/O in the checks of its executor
    // (`system_exec_parity.rs`, `appdefaults_exec_parity.rs`), where there is no live
    // machine at all.
    assert_eq!(
        app::route(&argv(&["--recover-kde"])),
        app::Route::RecoverKde,
        "--recover-kde обязан оставаться маршрутом с кодом возврата"
    );
    // Only WINDOW routes are safe to run: they have no side effects,
    // and they give no exit code — the binary raises the window.
    let (rc, said) = dry("--settings-qt");
    assert_eq!(rc, None, "флаг вкладки открывает окно, а не выходит кодом");
    assert!(said.is_empty(), "и ничего не печатает: {said:?}");
}

/// The five flags of the product's single tabs are routed to window sections — not
/// one of them opens the default section.
#[test]
fn каждый_флаг_одиночной_вкладки_открывает_свой_раздел() {
    let ours: Vec<String> = app::SINGLE_TAB_FLAGS.iter().map(|(f, _)| f.to_string()).collect();
    assert_eq!(
        ours,
        strings(&golden()["single_tab_flags"]),
        "флаги одиночных вкладок разошлись с эталоном — потерялся или завёлся лишний"
    );
    for (flag, _module) in app::SINGLE_TAB_FLAGS {
        let got = cli::start_section(&argv(&[flag]));
        let (section, _sub) = got
            .as_ref()
            .unwrap_or_else(|| panic!("флаг {flag} не открывает никакого раздела"));
        assert!(
            rexon_core::qt_main::SECTIONS.contains(&section.as_str()),
            "раздел {section} флага {flag} не из состава окна"
        );
    }
}

/// Names from the desktop shortcut are still translated — by the same
/// translation as the product's, not by a second copy of the table.
#[test]
fn имена_из_ярлыка_переводятся_как_у_продукта() {
    for (page, section, subpage) in app::PAGE_TARGETS {
        assert_eq!(
            cli::start_section(&argv(&[&format!("{}{page}", app::PAGE_FLAG)])),
            Some((section.to_string(), subpage.to_string())),
            "ярлык --page={page}"
        );
    }
}

// ============================================================================
// 2. Russian labels on Qt's own buttons
// ============================================================================

/// Qt's own translation catalogs — as in the golden file, in the same order, plus
/// a DELIBERATELY added `qtdeclarative`: the golden file lived on QtWidgets, while the edit menu
/// of QML fields (`TextArea`, `TextField`) is translated by Qt Quick with its own
/// catalog (08.11.2026: without it the menu was English with Rexon in Russian).
#[test]
fn qt_translation_catalogs_match_the_golden() {
    let ours: Vec<String> = cli::QT_TRANSLATION_CATALOGS.iter().map(|c| c.to_string()).collect();
    let quick = "qtdeclarative";
    let without_quick: Vec<String> = ours.iter().filter(|c| *c != quick).cloned().collect();
    assert_eq!(without_quick, strings(&golden()["qt_translation_catalogs"]));
    assert_eq!(ours.iter().filter(|c| *c == quick).count(), 1, "каталог Qt Quick обязан грузиться");
    assert_eq!(ours.first().map(String::as_str), Some("qtbase"), "qtbase — первым");
}

/// The separator — as in the golden file. The fallback language is DELIBERATELY not the old one:
/// the Python original loaded the catalog by the SYSTEM language and fell back to
/// `qtbase_ru` (its whole interface was Russian). In Rexon with translations the language of
/// Qt buttons is the language of Rexon itself, and the fallback is English, i.e. no
/// catalog at all.
#[test]
fn separator_matches_the_golden_and_fallback_is_english() {
    assert_eq!(
        cli::QT_TRANSLATION_SEPARATOR,
        golden()["qt_translation_separator"].as_str().unwrap(),
        "разделитель имени каталога разошёлся с эталоном"
    );
    let only_ru = |p: &std::path::Path| p.ends_with("qtbase_ru.qm") || p.ends_with("qt_ru.qm");
    let dir = std::path::Path::new("/qt");
    assert!(
        cli::qt_translation_files("ja", dir, &only_ru).is_empty(),
        "языка у Qt нет — откат обязан быть на английский, а не на русский"
    );
    assert!(cli::qt_translation_files("en", dir, &|_| true).is_empty());
    assert_eq!(cli::qt_translation_files("ru", dir, &only_ru).len(), 2);
}

/// The translator must be a Qt object, not a Python one: a Python one
/// is called from the QML loading thread and deadlocks the threads through the GIL.
/// The Rust entry has no Python one by construction — we guard that the shim is not
/// replaced by handling in Rust.
#[test]
fn перевод_кнопок_идёт_каталогом_а_не_колбэком() {
    const SHIM_H: &str = include_str!("../../rexon-gui/cpp/rexon_gui_factory.h");
    const SHIM_CPP: &str = include_str!("../../rexon-gui/cpp/rexon_gui_factory.cpp");
    assert!(
        SHIM_H.contains("rexon_install_qt_translations"),
        "шим переводов Qt потерялся из заголовка"
    );
    assert!(
        SHIM_CPP.contains("installTranslator"),
        "шим больше не включает каталог переводов"
    );
    assert!(
        SHIM_CPP.contains("QLibraryInfo::path"),
        "каталог обязан браться у самой Qt, а не носиться своим"
    );
    // Edit menu of QML fields: where `qtdeclarative` has no translation (Qt 6.12:
    // ru, zh_CN), the labels are taken from `qtbase` (the `QLineEdit` context).
    for needle in ["RexonQuickEditMenuTranslator", "\"SelectAllAction\"", "\"QLineEdit\""] {
        assert!(SHIM_CPP.contains(needle), "в шиме нет {needle}: меню правки QML снова английское");
    }
}

/// EVERY process with a Qt window installs the Qt catalogs — the crash window also has
/// a report field with its edit menu.
#[test]
fn каталоги_qt_ставит_каждое_окно() {
    const MAIN: &str = include_str!("../../rexon-gui/src/bin/main_window.rs");
    const CRASH: &str = include_str!("../../rexon-gui/src/crash_report.rs");
    assert!(MAIN.contains("rexon_gui::install_qt_translations("), "главное окно");
    assert!(CRASH.contains("crate::install_qt_translations("), "окно о крахе");
}

// ============================================================================
// 3. Background task drain
// ============================================================================

#[test]
fn stop_budget_and_slice_match_the_golden() {
    let g = golden();
    assert_eq!(
        exit_guard::DEFAULT_BUDGET_MS,
        g["stop_budget_ms"].as_u64().unwrap(),
        "бюджет стока разошёлся с эталоном"
    );
    assert_eq!(
        exit_guard::WAIT_SLICE_MS,
        g["stop_wait_slice_ms"].as_u64().unwrap(),
        "ломоть ожидания разошёлся с эталоном"
    );
}

/// The order is mandatory: first ASK (network requests are cut on the spot),
/// then wait. Waiting without asking means sitting out the whole network
/// timeout, and that is exactly the original's "closing hung for up to 30 seconds".
#[test]
fn сток_просит_остановиться_прежде_чем_ждать() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let step = Arc::new(AtomicUsize::new(0));
    let asked = Arc::new(AtomicUsize::new(0));
    let waited = Arc::new(AtomicUsize::new(0));

    let (s, a) = (step.clone(), asked.clone());
    exit_guard::register_stopper(move || {
        a.store(s.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
    });
    let (s, w) = (step.clone(), waited.clone());
    exit_guard::register_waiter(move |_ms| {
        w.store(s.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        0
    });

    exit_guard::stop_background_workers(10, false);
    assert_eq!(asked.load(Ordering::SeqCst), 1, "просьбу дёрнули не первой");
    assert_eq!(waited.load(Ordering::SeqCst), 2, "пул досчитали не последним");
}
