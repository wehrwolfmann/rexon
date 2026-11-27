// rexon-main-window — the Rexon main window whose host is in Rust.
//
// WHY THIS BINARY EXISTS AT ALL. Before it the dependency pointed the wrong
// way: the process was brought up by Python (`rexon.py` → `rexon/app.py` →
// `rexon/qt_main/app.py`), which also created the `QApplication` and the
// `QMainWindow`, while Rust connected to it as a library through
// `ctypes.CDLL`. There was no main window binary in Rust — `src/bin/` held
// only helper ones (the video wallpaper host, the web wallpaper host, the
// socket probe, the model bench). While the process host is Python, moving
// off it is impossible in principle.
//
// Here the host is Rust: `QGuiApplication`, `QQmlApplicationEngine` and a
// window entirely in QML (`qml/MainWindow.qml`). There is not a single
// widget in the process.
//
// ⭐ THIS IS THE PRODUCT ENTRY POINT. The desktop shortcut (with all its
// quick actions: `--page=`, `--recover-kde`) and the tray autostart lead
// here — the entry is chosen by one core function
// (`installer::launch_entry_live`, commit db4e33e). The Python window
// (`rexon.py` → `rexon/app.py` → `rexon/qt_main/**`) stays next to it as a
// separate shortcut menu item "Rexon (classic window)" and as a fallback:
// `REXON_ENTRY=python` or the absence of this binary — and the shortcut and
// autostart are written for the Python entry. So everything the Python host
// does at start-up this window must do too: otherwise switching the
// shortcut would silently take away part of the product from the customer.
//
// Launch:
//     cargo build-app && rust/target/release/rexon-main-window
// Open the right section at once (identifiers are in `rexon_core::qt_main`):
//     rust/target/release/rexon-main-window --section=appearance
// Also understood are the name from the desktop shortcut
// (`--page=install|backups|compare`) and the five single-tab flags of the
// product (`--appearance-qt`, `--extensions-qt`, `--compare-qt`,
// `--settings-qt`, `--desktops-qt`) — with the same translation as the
// product (`rexon_core::cli::start_section`).
// Helper routes without a window are `--apply-defaults`, `--sanitize-gtk`,
// `--session-splash`: the core handles them and returns an EXIT CODE, which
// becomes the process exit code.
// A run without a window (for scripts):
//     QT_QPA_PLATFORM=offscreen rust/target/release/rexon-main-window --selftest
// Exit code in `--selftest` mode: 0 — all checks passed, 1 — at least one
// failed, 2 — the window did not report in the allotted time.

use std::os::raw::{c_char, c_int};

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString, QUrl,
    QVariant,
};
// ⭐ QApplication, not QGuiApplication, and this is the only reason the QML
// window got a tray icon: QSystemTrayIcon, QMenu and QAction are widgets,
// and Qt does not create them at all without a QApplication. cxx-qt-lib
// itself does not have it either in 0.9 or in 0.10, but the neighbouring
// crate of the same release does — the analysis is in
// `rexon-gui/Cargo.toml`.
use cxx_qt_lib_extras::QApplication;

// The reference to the crate library is mandatory, not decorative: without
// it rustc does not pull in the rlib, and with it the static cxx-qt
// archives (models, moc, the QML plugin with resources) — the binary
// builds, but the QML is empty.
use rexon_gui::main_window::qobject::MainWindowModel;

use rexon_core::main_window_tour as tour;
use rexon_core::{applog, cli, exit_guard, i18n, single_instance};
use rexon_core::{log_error, log_info, log_warn};

/// Flag of a run without eyes.
const SELFTEST_ARG: &str = "--selftest";

/// Self-test watchdog. If the QML did not load or did not report, the run
/// must FINISH WITH AN ERROR, not with a quiet zero: a script will take
/// silence for success.
const SELFTEST_TIMEOUT_SECS: u64 = 30;

// Two Qt services that cxx-qt-lib 0.9 does not have at all; the shim is in
// `cpp/rexon_gui_factory.cpp`, along with the analysis of why they are in
// C++. C linkage instead of a cxx bridge: only strings and numbers cross
// the boundary, a bridge would be a superfluous link.
extern "C" {
    fn rexon_drain_thread_pool(ms: c_int) -> c_int;
    fn rexon_stop_media_players() -> c_int;
    fn rexon_qt_runtime_version() -> *const c_char;
    fn rexon_qt_build_version() -> *const c_char;
}

/// How long the process may be exiting after the end of the event loop
/// before the watchdog terminates it forcibly. The background task drain is
/// limited to 8 s (`exit_guard::DEFAULT_BUDGET_MS`), the rest is seconds; a
/// hung exit held the "one Rexon per session" lock, and a restart ran into
/// it.
const EXIT_WATCHDOG_SECS: u64 = 20;

/// A BENCH, not the product: "hang" on exit for this many seconds (checking
/// the exit watchdog and the new copy waiting for the lock).
const STAND_EXIT_HANG_ENV: &str = "REXON_STAND_EXIT_HANG_SECS";

/// Delay before a restart in the chain bench, seconds.
const STAND_RESTART_DELAY_SECS: u64 = 8;

/// The exit step the process is at right now — for the watchdog's line.
static EXIT_STEP: std::sync::Mutex<&'static str> = std::sync::Mutex::new("");

fn exit_step(step: &'static str) {
    if let Ok(mut s) = EXIT_STEP.lock() {
        *s = step;
    }
    rexon_core::log_debug!("exit", "шаг выхода: {step}");
}

/// Exit watchdog: if we did not leave within [`EXIT_WATCHDOG_SECS`] — a
/// line into the log (the step and the live threads of the process) and
/// `_exit` with the same code. Without the watchdog a hung exit left a
/// windowless ghost process holding the lock.
fn spawn_exit_watchdog(code: i32) {
    let _ = std::thread::Builder::new().name("rexon-exit-watch".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(EXIT_WATCHDOG_SECS));
        let step = EXIT_STEP.lock().map(|s| *s).unwrap_or("?");
        log_error!(
            "exit",
            "выход завис на шаге «{step}» дольше {EXIT_WATCHDOG_SECS} с — процесс завершается принудительно; живые потоки: {}",
            live_threads()
        );
        unsafe { libc::_exit(code) };
    });
}

/// Names of the process's live threads (`/proc/self/task/*/comm`) with
/// repeats.
fn live_threads() -> String {
    let mut names: Vec<String> = std::fs::read_dir("/proc/self/task")
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
                .map(|n| n.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut out: Vec<String> = Vec::new();
    for name in names {
        match out.last_mut() {
            Some(last) if last.starts_with(&format!("{name}×")) || *last == name => {
                let n = last.rsplit_once('×').and_then(|(_, n)| n.parse::<u32>().ok()).unwrap_or(1);
                *last = format!("{name}×{}", n + 1);
            }
            _ => out.push(name),
        }
    }
    out.join(", ")
}

fn c_text(p: *const c_char) -> String {
    if p.is_null() {
        return "?".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Launch lines: who, from where, with which Qt and in which language.
fn log_startup(args: &[String], language: &str, restart_of: Option<u32>) {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    log_info!(
        "start",
        "Rexon {} запущен: pid {}, родитель {}, {exe} {}",
        applog::version_line(),
        std::process::id(),
        unsafe { libc::getppid() },
        args.join(" ")
    );
    let (run, build) = unsafe { (c_text(rexon_qt_runtime_version()), c_text(rexon_qt_build_version())) };
    if run == build {
        log_info!("start", "Qt {run}");
    } else {
        log_warn!("start", "Qt рантайма {run}, а собран Rexon с Qt {build}");
    }
    log_info!("start", "окружение: {}", applog::desktop_env_line());
    if let Some(prev) = restart_of {
        log_info!("restart", "шаг 3: это новая копия, запущенная перезапуском процесса {prev}");
    }
    let saved = i18n::saved_setting();
    log_info!(
        "lang",
        "выбор «{saved}» → язык {language} (как в системе: {}); каталог {} (и для Rust, и для QML)",
        i18n::language_for_setting(None),
        i18n::mo_path().map_or_else(|| "нет".to_string(), |p| p.display().to_string())
    );
}

/// How many tasks of Qt's global pool the exit drain did not wait for.
/// Separate from the total count — because exactly this number goes into
/// the log: there are zero tasks of our own, yet the process still does not
/// leave, so the pool holds it.
static POOL_LEFT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn main() {
    // Trial launch before a restart (`settings_actions::probe_new_copy`):
    // getting here means the loader bound all the symbols, nothing more is
    // needed. Strictly the FIRST line: no lock, no log, no Qt.
    if std::env::var_os(cli::LINK_PROBE_ENV).is_some() {
        std::process::exit(0);
    }

    // The panic hook and the crash report window — as the FIRST line: the
    // windowless routes and the single instance lock go below, and
    // `--crash-dialog` shows the report itself and ends the process there.
    rexon_gui::crash_report::install();

    let selftest = std::env::args().any(|a| a == SELFTEST_ARG);
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Tour mode (`--tour`, `rexon_core::main_window_tour`): the window is
    // invisible, walks through the sections and stores by itself, takes
    // screenshots and writes a report. Like the self-test, it does not touch
    // the profile: no lock, no tray, no Rexon log — the process log goes into
    // the tour folder.
    let tour_cfg = match tour::parse_args(&args, || tour::default_out_dir(&|k| std::env::var(k).ok())) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("обход: {e}");
            std::process::exit(tour::EXIT_BAD_ARGS);
        }
    };
    if let Some(cfg) = &tour_cfg {
        if let Err(e) = prepare_tour_dir(cfg) {
            eprintln!("обход: папка {} не готова: {e}", cfg.out_dir.display());
            std::process::exit(tour::EXIT_BAD_ARGS);
        }
    }
    // Quiet mode — the self-test or the tour: everything that touches the live
    // session and the user's profile is skipped.
    let quiet = selftest || tour_cfg.is_some();
    // Set up the tour BEFORE loading QML: the executor asks for the first
    // command already in the window's `Component.onCompleted`, that is, inside
    // `engine.load`.
    if let Some(cfg) = &tour_cfg {
        rexon_gui::main_window::tour_install(cfg.clone());
    }

    // The log (`~/.local/state/rexon/rexon.log`) — before everything else, so
    // that both the language and the lock get into it. The self-test does not
    // write into the profile.
    if !quiet {
        applog::init();
        applog::install_fatal_signal_logger();
        // SIGTERM/SIGINT/SIGHUP — the line "terminated by signal X from process N"
        // and then the previous behaviour (exit).
        applog::install_exit_signal_logger();
    }
    // The "I am a restart" flag is read and REMOVED: the children of this
    // process (and its own future restart) must not inherit it.
    let restart_of: Option<u32> = std::env::var(single_instance::RESTART_OF_ENV)
        .ok()
        .and_then(|v| v.trim().parse().ok());
    // Benches (not the product): the restart chain and the "hung exit". Both
    // variables are removed so that they do not travel to the children on
    // their own.
    let stand_restarts: u32 = std::env::var(rexon_gui::settings_actions::STAND_RESTARTS_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let stand_exit_hang: u64 = std::env::var(STAND_EXIT_HANG_ENV)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    unsafe {
        std::env::remove_var(single_instance::RESTART_OF_ENV);
        std::env::remove_var("REXON_RESTART_WATCH_TICKS");
        std::env::remove_var(rexon_gui::settings_actions::STAND_RESTARTS_ENV);
        std::env::remove_var(STAND_EXIT_HANG_ENV);
    }

    // The interface language — BEFORE the first line for the user: the helper
    // routes below speak too. The decision (setting ▸ locale ▸ English) and
    // finding the catalogue belong to the core (`rexon_core::i18n`).
    let language = rexon_core::i18n::init();
    if !quiet {
        log_startup(&args, &language, restart_of);
    }

    // ── Routes that do not need a window ──────────────────────────────
    //
    // One door (`rexon_core::cli::run_headless`): the binary knows not a single
    // branch, the core answers with an exit code — and it MUST become the
    // process exit code. A script would take a silent zero on a failed
    // autostart hook for success; the original returned 0/1 (`os._exit(rc)`).
    //
    // Warning: BEFORE the "one Rexon per session" lock: helper routes do not
    // open a window, and there is no reason for them to run into someone
    // else's lock — otherwise the login splash would not play for someone who
    // already has Rexon open.
    if !quiet {
        if let Some(rc) = cli::run_headless(&args, &mut |line| println!("{line}")) {
            std::process::exit(rc);
        }
    }

    // ── A window with nowhere to show it ──────────────────────────────
    //
    // From here on there is only the window. Without a display (SSH, a text
    // console) `QApplication` ends in `abort` — a core dump and not a word to
    // the user. Say it in human terms and exit with 1 — before the lock: a
    // knock from the console would raise the window on someone else's screen.
    // The decision belongs to the core (`cli::window_can_open`).
    if !cli::window_can_open(&|k| std::env::var(k).ok()) {
        log_error!("start", "окно показать негде: нет дисплея ({})", applog::desktop_env_line());
        for line in cli::no_display_lines() {
            eprintln!("{line}");
        }
        std::process::exit(1);
    }

    // AUR helpers (paru/yay) of this window get a "sudo" — the session
    // privilege shim (`rexon_core::privilege::aur`): their pacman calls go into
    // the window's single password. The helper routes above and the self-test
    // do not need it.
    if !quiet {
        rexon_core::privilege::aur::enable();
    }

    // The section to open the window on: `--section=` (the developer entry), a
    // product single-tab flag or the name from the desktop shortcut. The
    // translation belongs entirely to the core (`cli::start_section`), there is
    // no table of our own here: with one the shortcut, the flags and the binary
    // would silently drift apart.
    //
    // The subsection ("DE comparison" inside "Desktops", `--page=compare` from
    // the shortcut) is computed by the core with the same call, and it goes to
    // QML as the initial property `startSubpage` — as for the Python window
    // (`show_compare`).
    let (start_section, start_subpage) = cli::start_section(&args).unwrap_or_default();

    // Broken theme links in gtk-{3,4}.0 crash other GTK applications — we fix
    // them before the first window, as the Python window start does. The live
    // sanitizer is the same as for the `--sanitize-gtk` route (`cli`), there is
    // no second copy. The self-test does not touch the profile.
    if !quiet {
        cli::startup_gtk_sanitize(&mut |line| {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "gtk", line, false);
        });
        // A truncated `~/.local/share/icons/hicolor/index.theme` overrides the
        // system one and hides the icons of the whole desktop (defect of
        // 09.11.2026) — rename it to `.bak-rexon-<date>`; a complete file is not
        // touched. One file read, so before the first window.
        if let Some(line) = rexon_core::icon_hygiene::heal_live() {
            println!("{line}");
        }
        // Several notification daemons for one bus name (desktops were also
        // installed bypassing Rexon) — the choice by desktop; if already in order,
        // it does nothing.
        std::thread::spawn(rexon_core::notifications_slot::sync_live);
        // Second copies of ordinary Conky widgets put by a previous installation
        // (each widget was shown twice) — only our own and untouched ones.
        std::thread::spawn(rexon_core::ext_exec::heal_conky_duplicates_live);
    }

    // ── One Rexon per session ─────────────────────────────────────────
    //
    // The lock and the listener are shared with the Python product
    // (`rexon_core::single_instance`, where both defects are analysed): the
    // address is the same, so launching the Rust binary next to a running
    // Rexon will NOT raise a second window and a second tray icon, but will
    // knock on the first one.
    //
    // Warning: the self-test does not take the lock ON PURPOSE. `--selftest`
    // checks the window, not a second instance of the product: running into
    // someone else's lock, it would silently exit with zero — and a run that
    // never built a window would count as green. This is exactly the
    // "quiet exit = success" substitution the project already caught at the
    // self-test watchdog below.
    if !quiet {
        let runtime_dir = single_instance::runtime_dir_from_env();
        let lock = single_instance::lock_path(&runtime_dir);
        let mut held = single_instance::acquire_lock(&lock);
        if held {
            log_info!("instance", "замок одного экземпляра взят: {lock}");
        } else {
            log_info!(
                "instance",
                "замок {lock} занят (держит процесс {})",
                single_instance::lock_holder_pid(&lock).map_or_else(|| "?".to_string(), |p| p.to_string())
            );
        }
        // Restart: the lock is held by the departing previous copy. Leaving now
        // would be exactly the live case of 08.10.2026 (the new one knocked on the
        // departing one and exited, then the old one left too). We wait for it to
        // leave ourselves.
        if let (false, Some(prev)) = (held, restart_of) {
            log_info!(
                "restart",
                "шаг 4: жду ухода прежнего процесса {prev} (до {} с)",
                single_instance::RESTART_LOCK_WAIT.as_secs()
            );
            match single_instance::wait_for_previous(&lock, prev, single_instance::RESTART_LOCK_WAIT) {
                single_instance::PreviousWait::Acquired(waited) => {
                    log_info!("restart", "шаг 5: прежний ушёл, замок взят через {} мс", waited.as_millis());
                    held = true;
                }
                single_instance::PreviousWait::PreviousStillRunning => log_error!(
                    "restart",
                    "прежний процесс {prev} не ушёл за {} с — стучусь в него, чтобы окно хотя бы было",
                    single_instance::RESTART_LOCK_WAIT.as_secs()
                ),
                single_instance::PreviousWait::OtherHolder(pid) => log_warn!(
                    "restart",
                    "прежний процесс {prev} ушёл, но замок держит другой ({}) — стучусь в него",
                    pid.map_or_else(|| "?".to_string(), |p| p.to_string())
                ),
            }
        }
        if !held {
            // A hidden launch (`--tray`) does not knock: the knock would pull the
            // already open window onto the screen. The decision is shared with the
            // Python entry.
            let knocks =
                single_instance::on_busy_lock(&args) == single_instance::OnBusyLock::Knock;
            let sock = single_instance::socket_path(&runtime_dir);
            // The section of the shortcut's quick action (`--page=backups` etc.)
            // travels in the knock: the running window will open it, not just raise
            // itself. What to knock is decided by the core (`knock_target`), as for
            // the Python entry.
            let target = single_instance::knock_target(&args);
            let target = target.as_ref().map(|(s, p)| (s.as_str(), p.as_str()));
            let outcome = knocks.then(|| single_instance::knock(&sock, target));
            // The real reason for the silence goes to the log; the core's text goes to
            // the user.
            match &outcome {
                Some(single_instance::KnockOutcome::NoAnswer(reason)) => {
                    log_warn!("instance", "стук «подними окно» без ответа: {reason}")
                }
                Some(other) => log_info!("instance", "стук «подними окно»: {other:?}"),
                None => log_info!("instance", "скрытый запуск при занятом замке — выхожу молча"),
            }
            // This launch will have no window of its own, and Qt sends "startup
            // finished" only when a window is shown: without this line the MATE panel
            // kept "Starting Rexon" and the busy cursor until the timeout
            // (`single_instance::complete_startup_notification`).
            match single_instance::complete_startup_notification() {
                Some((id, Ok(()))) => log_info!("instance", "уведомление о запуске завершено: {id}"),
                Some((id, Err(e))) => log_warn!("instance", "уведомление о запуске {id} не завершено: {e}"),
                None => {}
            }
            // A restart that never became Rexon, and nobody raised the window: the
            // user pressed "Restart" and was left with nothing — tell them with a
            // notification, since we have no window.
            if restart_of.is_some() && outcome != Some(single_instance::KnockOutcome::Raised) {
                log_error!("restart", "перезапуск не удался: окна нет ни у новой, ни у прежней копии");
                // TRANSLATORS: Title of a desktop notification when Rexon failed to come back after a restart.
                let summary = i18n::tr("Rexon didn't restart");
                let body = i18n::trf(
                    // TRANSLATORS: Body of a desktop notification; %1 is the path of the Rexon log file.
                    "The previous copy of Rexon didn't close in time. Open Rexon from the application menu; details are in the log: %1",
                    &[&applog::log_path().to_string_lossy()],
                );
                // The "Open Rexon" button — the same as the menu shortcut the text talks
                // about (without buttons the text remains the only way).
                let actions = vec![("open".to_string(), i18n::tr("Open Rexon"))];
                if rexon_core::utils::notify_ask_live(&summary, &body, &actions, "", 600).as_deref() == Some("open") {
                    if let Err(e) = rexon_core::installer::open_rexon_live(&[]) {
                        log_warn!("restart", "Rexon из уведомления не открылся: {e}");
                    }
                }
            }
            // Silence is not allowed here (rule #13): the user clicked the shortcut and
            // must learn where the program went. What to say is decided by the core.
            for line in single_instance::said_on_busy_lock(&lock, outcome.as_ref()) {
                println!("{line}");
                applog::write_opts(applog::Level::Info, "lock", &line, false);
            }
            std::process::exit(0);
        }
    }

    // Without this a run from a script looks mute: when stderr is not a
    // terminal, Qt on Arch/CachyOS sends messages to journald, and the
    // self-test lines from QML do not get into the output.
    //
    // The self-test has only this variable, WITHOUT a format and without
    // redirecting descriptor 2 into the log: the run reads the lines from QML
    // with its own parser, and changing their form under it would mean fixing
    // one thing while breaking another.
    // Standard buttons under KDE Plasma are translated by the platform theme
    // by `LANGUAGE`, not by qtbase: without this line, with an English Rexon on
    // a Russian system, the dialog said "Да/Нет". The value is the core's
    // decision (`cli::qt_ui_language_env`), set before the application is
    // created, while there are no other threads.
    // Warning: through the core, not `set_var`: the original `LANGUAGE` is
    // remembered (`i18n::original_locale_env`), and "as in the system" is
    // computed from it, not from the substituted one (live case of 08.10.2026,
    // "Wie im System (Deutsch)").
    i18n::override_ui_language_env(&cli::qt_ui_language_env(&language));
    if !quiet {
        log_info!("lang", "переменные языка: {}", i18n::locale_env_report());
    }

    if quiet {
        unsafe { std::env::set_var("QT_FORCE_STDERR_LOGGING", "1") };
    } else {
        // Qt and QML messages go to the Rexon log. The decision belongs entirely to
        // the core (`ui_base::qt_logging_env` + `should_redirect_stderr`); without
        // this NOT A SINGLE QML message got in, and an empty log read as "no
        // errors".
        cli::install_qt_message_logger();
    }

    // MANDATORY and not obvious: the static QML plugin with resources and type
    // registration is linked as a separate archive, and without an explicit
    // reference the linker throws it away — QML then fails with "qrc:/… No
    // such file or directory" before any import error.
    cxx_qt::init_qml_module!("org.rexon.gui");

    let mut app = QApplication::new();
    // Our own wallpaper removes the live Wallpaper Engine wallpaper (the
    // core's decision — `wallpaper_exec::yields_we_wallpapers`). The socket to
    // the daemon lives in Qt, so the remover is installed by the window, not
    // by the core.
    rexon_core::wallpaper_exec::set_we_stopper(rexon_gui::we_socket::stop_we_wallpapers_hook);
    // Cinnamon with layer-shell (Muffin 6.8+): live wallpapers as a desktop
    // layer, as on wlr compositors, not as a shell extension. The fact — before
    // the first decision about the wallpaper path.
    rexon_gui::wallpaper_host::cinnamon_layer::init_fact();
    // Presenting ourselves to the system — with the same strings as the other
    // entry points of the product (`rexon_core::qt_app`), not a second copy
    // here.
    //
    // Each setter is a separate block, as in the neighbouring bench:
    // `Pin<&mut …>` is not copied, and two calls in a row in one block do not
    // compile.
    if let Some(app) = app.as_mut() {
        app.set_organization_name(&QString::from(rexon_core::qt_app::ORGANIZATION_NAME));
    }
    if let Some(app) = app.as_mut() {
        // Warning: the application name is a MACHINE name, not the window title:
        // Qt gives it to the bus as the `Id` property of the tray icon, and the
        // shell remembers the icon settings made by the customer by `Id`. The
        // analysis and the guard are in `rexon_core::tray::APP_ID`.
        app.set_application_name(&QString::from(rexon_core::tray::APP_ID));
    }
    // `setDesktopFileName` is static in Qt (it is about the whole process, not
    // about the application object), and cxx-qt-lib wraps it the same way.
    QGuiApplication::set_desktop_file_name(&QString::from(
        rexon_core::qt_app::DESKTOP_FILE_NAME,
    ));
    // The shortcut name is enough for Wayland, but the X11 task bar (MATE,
    // XFCE, Cinnamon) takes the picture from the window's `_NET_WM_ICON` — the
    // icon is set before the first window and applies to all windows of the
    // process (`rexon_gui::tray::set_app_window_icon`).
    if !rexon_gui::tray::set_app_window_icon() && !quiet {
        log_warn!("start", "значок окна не найден ни в теме, ни файлом — панель задач покажет общий");
    }

    // Labels on Qt's OWN buttons ("OK", "Cancel", "Show Details…", the edit
    // menu in text fields) — in the Rexon language, not the system's: Qt draws
    // them itself, and no translation of ours reaches them.
    rexon_gui::install_qt_translations(&language);
    install_app_translation(&language);
    // Arabic, Persian and Hebrew — with their own fonts, not DejaVu, which
    // fontconfig gives first (the families are the core's,
    // `qt_app::RTL_FALLBACK_FONTS`).
    rexon_gui::fonts::install_rtl_fallbacks();

    // The wallpaper tab: the package directory, the `waywallen.ui` types (with
    // the `App` singleton, which also installs the translator of the vendored
    // pages), the filter model, the daemon launcher and warming up Qt Protobuf.
    // Strictly AFTER the application and BEFORE the first QML load — the same
    // order as for the Python host, and the same door (`rexon_gui::we_env`),
    // there is no second copy here.
    rexon_gui::we_env::prepare_process();
    // The KDE wallpaper plugin the user already has — in step with this
    // Rexon's bundle, on any desktop (defect of 12.11.2026: Rexon updated the
    // engine and the plugin in the bundle, but the old one stayed in ~/.local —
    // on logging into Plasma it hung plasmashell). An ordinary start only
    // compares the hashes, in the background.
    if !quiet {
        rexon_gui::we_socket::refresh_installed_kde_plugin("старт Rexon");
    }

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        // Import paths of the wallpaper tab's plugins, the disk thumbnail cache
        // and the engine entry for changing the language of the vendored pages.
        // The pointer is to the engine itself: `QQmlApplicationEngine` inherits
        // `QQmlEngine` as its first (and only) base, their address is shared.
        unsafe {
            let ptr = engine.as_mut().get_unchecked_mut() as *mut QQmlApplicationEngine;
            rexon_gui::we_env::prepare_engine(ptr.cast());
        }
        // The self-test flag goes to QML as an initial property of the root, not
        // as a context variable: context properties are not wrapped in cxx-qt-lib
        // 0.9, while `setInitialProperties` exists and works BEFORE the first
        // binding.
        let mut props = QMap::<QMapPair_QString_QVariant>::default();
        props.insert(QString::from("selftest"), QVariant::from(&selftest));
        // Tour: the executor in QML is enabled by this property, the window gets
        // the screenshot size (the core, `main_window_tour::WINDOW_SIZE`).
        props.insert(QString::from("tourMode"), QVariant::from(&tour_cfg.is_some()));
        if tour_cfg.is_some() {
            let (w, h) = tour::WINDOW_SIZE;
            props.insert(QString::from("width"), QVariant::from(&w));
            props.insert(QString::from("height"), QVariant::from(&h));
        }
        // The window is loaded HIDDEN and shown at the end of start-up
        // (`tray::first_show_main_window`, with the measurement of 02.11.2026
        // there): with `visible: true` from the markup it came onto the screen in
        // the middle of loading and stood transparent while the GUI thread finished
        // the start-up. The self-test does not hide the window — its run does not
        // touch this path.
        if !quiet {
            props.insert(QString::from("visible"), QVariant::from(&false));
        }
        props.insert(
            QString::from("startSection"),
            QVariant::from(&QString::from(&start_section)),
        );
        props.insert(
            QString::from("startSubpage"),
            QVariant::from(&QString::from(&start_subpage)),
        );
        props.insert(
            QString::from("weVendorQmlDir"),
            QVariant::from(&QString::from(&rexon_gui::we_env::vendor_qml_dir())),
        );
        engine.as_mut().set_initial_properties(&props);
        // The path inside the resource repeats the file path in the crate: cxx-qt
        // puts the .qml by relative path, not into the module root.
        // The engine address — before `load`: that one takes the pinned reference
        // for itself.
        let engine_ptr = unsafe { engine.as_mut().get_unchecked_mut() as *mut QQmlApplicationEngine };
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/MainWindow.qml"));
        if !quiet {
            // SAFETY: the engine is alive (the owner is `engine` until the end of
            // `main`) and loaded; the GUI thread.
            let adopted = unsafe { rexon_gui::tray::adopt_main_window(engine_ptr.cast()) };
            if !adopted {
                log_warn!("start", "корень QML не окно — показывать нечего");
            }
        }
    }

    // ── The system tray icon ──────────────────────────────────────────
    //
    // The sections for the submenu, the picture file and the `--tray` flag are
    // taken from the core by the door itself
    // (`rexon_gui::tray::install_for_main_window`) — not a single decision and
    // not a single label here. Installed AFTER loading QML: the icon at once
    // takes the main window under its close watcher, and before loading there
    // is no window yet.
    //
    // The self-test does NOT install the icon: it runs through the sections and
    // exits, and an extra tray icon in the live session is litter on the
    // user's panel.
    // Whether to show the window at the end of start-up: it is hidden only
    // with a live icon and a `--tray` request (without an icon the window is
    // the only door, rule #13).
    let mut show_window = !quiet;
    if !quiet {
        let tray = rexon_gui::tray::install_for_main_window(&args);
        if tray && rexon_core::tray::starts_hidden(&args) {
            show_window = false;
        }
        // They asked to start minimized, but there is no icon — the window is the
        // only door into the program, and it must not be hidden (rule #13). The
        // icon's door does not hide the window in this case; it still has to be
        // said — the core adds the line.
        for line in cli::tray_status_lines(tray, rexon_core::tray::starts_hidden(&args)) {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "tray", &line, false);
        }
        // Autostart entries with an old program path (live case of 08.10.2026:
        // `/usr/local/bin/rexon`) are silently skipped by the system — we fix them
        // on every launch, quietly and with a log entry. With a live icon this is
        // done by its thread (before refreshing the compositor lines, otherwise
        // there would be two writers of one file), without an icon by a separate
        // thread.
        if !tray {
            std::thread::spawn(rexon_gui::tray::repair_autostart_entries);
        }
        // Session look watcher: a theme changed anywhere (in Rexon, in MATE's
        // "Appearance" window, in the settings of any environment) gets its own
        // icons and cursor. Lives as long as Rexon does, with or without the icon.
        rexon_core::theme_watch::spawn_once();
    }

    // Announce Rexon to the bridge (`rexon_installed`, the version, the
    // environment) — once per launch, like `announce_self` of the Python
    // window. In a separate thread: writing a file must not delay the first
    // paint. The bridge is not critical — a failure goes as a line into the
    // log.
    if !quiet {
        std::thread::spawn(|| {
            if let Err(e) = cli::announce_to_bridge() {
                log_warn!("bridge", "объявление не записано: {e}");
            }
        });
    }

    // Razer Axon wallpapers — prepare in advance, in the background and with a
    // low priority, so that "Apply" works at once. With a delay: do not get in
    // the way of start-up. If it is turned off in the settings or there is no
    // Axon, the thread exits at once.
    if !quiet {
        rexon_core::axon_prepare::spawn_background(rexon_core::axon_prepare::STARTUP_DELAY);
    }

    // The reference of default application protection — if it has never been
    // taken yet. The section is on by default, but until the first reference
    // Rexon remembers nothing and will not notice a substitution, and the
    // customer is not obliged to press "Remember now". The decision and the
    // action are entirely in the core (`cli::startup_defaults_snapshot`), here
    // there is one line; it also starts the thread — taking it polls the
    // system with subprocesses and would delay the first paint in the start-up
    // path.
    //
    // Warning: the self-test does not take the reference — precisely the
    // reference, not "does not touch the profile": it creates settings.json and
    // the snapshot directory even without us, that is its long-standing
    // behaviour. A comment must not promise more than the code does: the next
    // person will rely on that promise.
    if !quiet {
        cli::startup_defaults_snapshot(Box::new(|line: &str| {
            println!("{line}");
            applog::write_opts(applog::Level::Info, "defaults", line, false);
        }));
    }

    // The "raise the window" listener comes up AFTER loading QML: before that
    // there is nothing to raise yet, and the socket would already answer "ok"
    // to emptiness.
    if !quiet {
        let runtime_dir = single_instance::runtime_dir_from_env();
        let sock = single_instance::socket_path(&runtime_dir);
        // A long `XDG_RUNTIME_DIR`: the address went to the fallback directory —
        // this needs to be known both on success (where to look for the socket)
        // and even more so on failure.
        if let Some(note) = single_instance::socket_address_note(&runtime_dir) {
            log_info!("instance", "{note}");
        }
        // The holder is Rust: on "yield" from the classic window (`--classic`) the
        // window leaves, on "show section" it opens it and raises itself.
        match single_instance::ActivationListener::spawn_for(
            &sock,
            single_instance::Holder::Rust,
            on_activation,
        ) {
            Ok(()) => log_info!("instance", "слушатель «подними окно» встал: {sock}"),
            // Did not come up — no big deal for this launch, but a second launch will
            // now tell the user that Rexon does not respond, and the reason needs to
            // be known.
            Err(e) => log_warn!("instance", "слушатель «подними окно» не встал ({sock}): {e}"),
        }
    }

    if !quiet && stand_restarts > 0 {
        rexon_gui::settings_actions::STAND_RESTARTS_LEFT
            .store(stand_restarts, std::sync::atomic::Ordering::SeqCst);
        log_info!("restart", "стенд: перезапуск через {STAND_RESTART_DELAY_SECS} с (осталось {stand_restarts})");
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(STAND_RESTART_DELAY_SECS));
            rexon_gui::settings_actions::stand_restart_now();
        });
    }

    if let Some(cfg) = tour_cfg {
        let limit = cfg.timeout_secs;
        println!("обход: {} → {}", cfg.target_label(), cfg.out_dir.display());
        // The overall timeout: a tour without network or with a hung page must
        // end, and the report must say what it got stuck on.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(limit));
            let reason = format!("общий тайм-аут {limit} с");
            let code = rexon_gui::main_window::tour_abort(&reason).unwrap_or(tour::EXIT_TIMEOUT);
            println!("обход: {reason}");
            unsafe { libc::_exit(code) };
        });
    }

    if selftest {
        eprintln!(
            "хозяин окна на Rust: {}",
            std::any::type_name::<MainWindowModel>()
        );
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(SELFTEST_TIMEOUT_SECS));
            eprintln!(
                "окно не отчиталось за {SELFTEST_TIMEOUT_SECS} с — самопроверка провалена"
            );
            std::process::exit(2);
        });
    }

    // Qt's global thread pool is counted by the exit drain, but our background
    // task counter cannot see it: the pool's worker threads are started by
    // C++. The hook is installed BEFORE the event loop — after `exec()` it is
    // too late to register.
    exit_guard::register_waiter(|ms| {
        let left = unsafe { rexon_drain_thread_pool(ms.min(c_int::MAX as u64) as c_int) };
        let left = left.max(0) as usize;
        POOL_LEFT.store(left, std::sync::atomic::Ordering::SeqCst);
        left
    });

    // The first showing of the window is the last thing before the event loop:
    // after that the GUI thread paints a frame at once, and a transparent
    // window never appears on screen.
    if !quiet {
        // Main thread watchdog: if the window does not respond for longer than 2 s
        // — a line into the log with the current action and the stack (live case
        // of 15.11.2026: a hang, "Close" from the desktop, SIGKILL — and an empty
        // log). Not in the self-test and not in the tour (`quiet`).
        rexon_gui::action_journal::install_main_watchdog();
        rexon_gui::tray::first_show_main_window(show_window);
    }

    // The event loop's exit code is the self-test's verdict, and it must
    // become the process exit code. Simply returning from main is not
    // allowed: then a failure would look like success.
    let code = match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    };
    let exit_started = std::time::Instant::now();
    if !quiet {
        log_info!(
            "exit",
            "цикл событий завершён (код {code}); причина: {}",
            rexon_gui::tray::quit_reason()
        );
    }
    spawn_exit_watchdog(code);
    exit_step("остановка проигрывателей");

    // ── Players — stop BEFORE any teardown ────────────────────────────
    //
    // Warning: dismantling the engine (below) does NOT stop the players first:
    // `QQmlApplicationEngine` destroys the window, `~QQuickWindow` waits for
    // the render thread, and the live preview's decoding thread keeps working
    // all that time. Caught live on 28.09.2026 (core dumps 1328272 and 1332260,
    // the "Steam" section with a playing splash, two crashes in a row): in the
    // middle of the teardown the VP9 decoder on an NVIDIA GPU (NVDEC) stops
    // passing `cuCtxPushCurrent`, and FFmpeg crashes while printing that error
    // — SIGSEGV in `av_log` from `ff_cuda_check` (it reads uninitialized
    // pointers to the error name). On the bench — 1 exit in 20 via "Quit" from
    // the tray. Hence the players first: stop them and remove the source,
    // which synchronously waits for the FFmpeg threads — and only then
    // everything else. Qt's destructor order promises nothing here.
    let players = unsafe { rexon_stop_media_players() };
    if players > 0 {
        let line = cli::players_stopped_line(i64::from(players));
        println!("{line}");
        if !quiet {
            applog::write_opts(applog::Level::Info, "exit", &line, false);
        }
    }
    exit_step("сток фоновых задач");
    if stand_exit_hang > 0 {
        log_warn!("exit", "стенд: зависаю на выходе на {stand_exit_hang} с");
        std::thread::sleep(std::time::Duration::from_secs(stand_exit_hang));
    }

    // ── Background task drain ─────────────────────────────────────────
    //
    // Exactly here, not at the end of `main`: `exec()` has already returned
    // (the event loop decided to end), but neither `app` nor `engine` has been
    // torn down yet — this is the only point where the honest sequence "ask to
    // stop → wait → tear down" is feasible. Everything later happens after the
    // Qt objects are destroyed.
    //
    // Without this `std::process::exit` cut a background task off wherever it
    // happened to be — including in the middle of writing a theme file — and
    // the user would see "Rexon quit unexpectedly" where they simply closed the
    // window.
    let left = exit_guard::stop_background_workers(exit_guard::DEFAULT_BUDGET_MS, true);
    if let Some(msg) = exit_guard::drain_report(
        left,
        POOL_LEFT.load(std::sync::atomic::Ordering::SeqCst),
    ) {
        // Silence is not allowed (rule #13): the user sees a process stuck on exit
        // as "the program does not close", and the reason must stay in the log.
        log_warn!("exit", "{msg}");
    }

    // Warning: TEAR DOWN THE SCENE BEFORE EXITING, and in exactly this order.
    // Leaving the event loop does not tear down the built window: the QML
    // engine keeps holding the root object, and with it the live players of
    // the sections — and their decoding threads (`QFFmpeg::Stream`) keep
    // working. `exit` then runs the termination handlers and unloads libraries
    // under a working thread, and Rexon crashes right on exit. Caught live:
    // "Quit" from the icon menu after opening the "Steam" section with a video
    // splash — SIGSEGV in `av_log`; before that the scene render thread
    // (`QSGRenderThread`) crashed with the same class. Dismantling the engine
    // destroys the root object, it destroys the players, and they destroy
    // their threads.
    //
    // The background task drain above does NOT replace this: it counts our
    // tasks, while the scene and multimedia threads are started by Qt itself.
    exit_step("роспуск движка QML");
    drop(engine);
    exit_step("роспуск приложения");
    drop(app);
    exit_step("завершение процесса");

    if !quiet {
        log_info!("exit", "выход за {} мс, код {code}", exit_started.elapsed().as_millis());
    }
    std::process::exit(code);
}

/// Tour folder: create it, remove the screenshots and the report of the
/// previous tour (other files are not touched), the process error stream
/// into its log, the Qt variables of an invisible window. Strictly before
/// the first thread and before Qt.
fn prepare_tour_dir(cfg: &tour::TourConfig) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    std::fs::create_dir_all(&cfg.out_dir)?;
    for entry in std::fs::read_dir(&cfg.out_dir)?.flatten() {
        if tour::is_tour_artifact(&entry.file_name().to_string_lossy()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let journal = std::fs::File::create(cfg.out_dir.join(tour::JOURNAL_FILE))?;
    if unsafe { libc::dup2(journal.as_raw_fd(), 2) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    for (key, value) in tour::qt_env() {
        // SAFETY: there are no other threads yet.
        unsafe { std::env::set_var(key, value) };
    }
    Ok(())
}

/// Translating QML strings (`qsTranslate("rexon", …)`) — the same language
/// and THE SAME `.mo` catalogue that the core already chose for the Rust
/// strings: the window translator asks the core
/// (`rexon_gui::install_app_translator`); Rexon has no `.qm` of its own.
///
/// English (the source) has no catalogue, and that is not an error: the
/// translator is not installed then.
fn install_app_translation(language: &str) {
    if !rexon_gui::install_app_translator() {
        log_warn!("lang", "переводчик строк QML не встал (нет приложения Qt) — подписи QML останутся английскими");
    } else if language != rexon_core::i18n::SOURCE_LANGUAGE && rexon_core::i18n::mo_path().is_none() {
        log_warn!("lang", "для языка «{language}» нет rexon.mo — подписи останутся английскими");
    }
}

/// Raise this instance's window at the request of a second launch.
///
/// Called FROM THE LISTENER THREAD (`ActivationListener::spawn`), while Qt
/// allows touching a window only from the GUI thread — so raising it from
/// here is not allowed under any circumstances, and "I'll do what I can"
/// here would mean a product crash out of nowhere.
///
/// ⭐ THE DOOR IS OPEN. Previously a `false` stub stood here: the QML window
/// had no path to the window itself — there are no widgets in the process,
/// and `cxx-qt-lib` exposes neither the root object of
/// `QQmlApplicationEngine` nor the `QWindow`. The path was found at the
/// tray shim: it looks for the window among `topLevelWindows()` (the
/// QQuickWindow is there) and itself moves the work into the GUI thread
/// with a queued call waiting for the answer. So calling from here IS
/// allowed, and the answer is honest: `false` means "there really is no
/// window", not "cannot do it".
fn raise_window() -> bool {
    rexon_gui::tray::raise_main_window()
}

/// Handler of the activation listener (the decision is
/// `single_instance::listener_action`).
///
/// Called from the listener thread; the tray doors move the work into the
/// GUI thread themselves. On `HandOver` the exit is only SCHEDULED and
/// `true` is returned: the listener will send the "leaving" answer after the
/// return, and exiting before the answer would cut the classic window's
/// connection.
fn on_activation(action: single_instance::ListenerAction) -> bool {
    match action {
        single_instance::ListenerAction::Raise(section) => {
            if let Some((id, subpage)) = &section {
                if !rexon_gui::tray::open_section(id, subpage) {
                    log_warn!("instance", "раздел «{id}» по просьбе второго запуска не открылся");
                }
            }
            let raised = raise_window();
            log_info!("instance", "второй запуск: поднять окно (раздел {section:?}) → {raised}");
            raised
        }
        single_instance::ListenerAction::HandOver => {
            rexon_gui::tray::request_quit("передача хозяина классическому окну");
            true
        }
    }
}
