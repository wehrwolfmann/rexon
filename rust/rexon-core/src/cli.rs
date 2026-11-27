//! The single door of the Rexon entry point: a port of the `main()` body
//! from `rexon/app.py` (the route branches) plus what Python did BEFORE
//! starting Qt.
//!
//! DIVISION OF LABOUR. [`crate::app::route`] answers "what to do"; this
//! module "does it on the live machine and decides the exit code". The
//! binary (`rexon-gui/src/bin/main_window.rs`) knows no branch at all: it
//! calls [`run_headless`], and if that answered with a code it exits with
//! it, and if `None` it starts Qt on the section from [`start_section`].
//! The one-door rule: a new branch appears HERE, the binary is not edited.
//!
//! WHAT IT CLOSES. Before this module the Rust binary understood only
//! `--section=` and `--page=`, while the product's service flags
//! (`--apply-defaults`, `--sanitize-gtk`, `--session-splash`, five
//! standalone tab flags) silently went nowhere: Qt ignores an unknown
//! argument, and the binary opened the main window instead of the promised
//! work. Worse, it did not return the routes' EXIT CODES at all: the
//! `--apply-defaults` autostart hook reported zero when applying the
//! defaults failed, so the script saw success where nothing had been
//! applied. The original returned 0/1 (`os._exit(rc)` in `rexon.py`), and
//! this is exactly the "silent exit = success" substitution the project had
//! already caught in the window self-check guard.
//!
//! ⭐ `--recover-kde` IS CLOSED HERE TOO. The last route that depended on
//! the interpreter is now entirely Rust: argument parsing is in
//! [`crate::app`], the order of steps and the trips outside in
//! [`crate::system_exec::SystemExec::recover_kde`], the exit code here.
//! That was the last big gap of the entry point: while only Python could do
//! the appearance emergency rollback, the Rust binary could not become
//! `/usr/bin/rexon`; the "Restore desktop appearance" shortcut would have
//! refused the person exactly at the moment the appearance was already
//! broken.

use std::path::{Path, PathBuf};

use crate::app::Route;
use crate::i18n::{ntr, tr, trf};

// ============================================================================
// Standalone Qt tabs -> main window section
// ============================================================================
//
// In the original each of the five flags started a SEPARATE application
// with one window; that was the only way to open one tab while the tabs
// were different processes. In the Rust binary all six sections live in
// one window, and "one tab" is the same window opened on the right
// section, i.e. the already existing `--section=`. So the five modules
// boil down to a "flag -> section" table rather than five entry points: a
// second copy of the section list would silently drift from the first
// (rule #12).

/// Standalone tab module -> (window section, subsection inside it).
///
/// Keys are the values of [`crate::app::SINGLE_TAB_FLAGS`], values are
/// sections from [`crate::qt_main::SECTIONS`]; the two lists are compared
/// in `tests/cli_parity.rs`.
///
/// "DE comparison" is not a section of its own: since 08.07.2026 it lives
/// INSIDE "Desktops", so `qt_compare` has the same section as
/// `qt_desktops`, and the subsection tells them apart (exactly as with
/// `--page=compare`).
pub const SINGLE_TAB_SECTIONS: &[(&str, &str, &str)] = &[
    ("rexon.appearance_page", "appearance", ""),
    ("rexon.qt_extensions.app", "extensions", ""),
    ("rexon.qt_compare.app", "desktops", "compare"),
    ("rexon.qt_settings.app", "settings", ""),
    ("rexon.qt_desktops.app", "desktops", ""),
];

/// Section and subsection for a standalone tab module.
pub fn single_tab_section(module: &str) -> Option<(&'static str, &'static str)> {
    SINGLE_TAB_SECTIONS
        .iter()
        .find(|(m, _, _)| *m == module)
        .map(|(_, section, subpage)| (*section, *subpage))
}

/// The section the window must open on, judging by the whole command line.
///
/// Three sources in decreasing order of precision: `--section=<id>` (the
/// developer entry, the identifier as is), a product standalone tab flag
/// and `--page=<name>` from a desktop shortcut. If none applies, `None`,
/// and the window opens on the default section.
///
/// `--section=` comes first not by seniority but because it is the only
/// source whose identifier is given literally: the other two are
/// translations, and translating what has already been named precisely
/// means losing precision.
pub fn start_section(args: &[String]) -> Option<(String, String)> {
    // The window from the AeroThemePlasma breakage notification opens on
    // "Settings": the appearance check and its result are there (the check
    // itself starts right away).
    if args.iter().any(|a| a == crate::aeroshell::OPEN_FLAG) {
        return Some(("settings".to_string(), String::new()));
    }
    if let Some(id) = args.iter().find_map(|a| a.strip_prefix(SECTION_FLAG)) {
        // `--section=appearance/screeneffects` gives the subsection too (the key
        // of the "Appearance" sidebar section): this way the test bench opens the
        // right section without clicks.
        let (id, sub) = id.split_once('/').unwrap_or((id, ""));
        let id = id.trim();
        if !id.is_empty() {
            return Some((id.to_string(), sub.trim().to_string()));
        }
    }
    if let Route::SingleTab { module } = crate::app::route(args) {
        if let Some((section, subpage)) = single_tab_section(module) {
            return Some((section.to_string(), subpage.to_string()));
        }
    }
    crate::app::start_page(args).map(|(s, p)| (s.to_string(), p.to_string()))
}

/// Section flag: `--section=appearance` (with a subsection:
/// `--section=appearance/screeneffects`).
pub const SECTION_FLAG: &str = "--section=";

// ============================================================================
// The crash window as a separate process
// ============================================================================
//
// A crashed process does not draw the crash window: a panic happens in
// another thread and in a state where Qt is already unusable, and a panic
// across the Qt boundary is an `abort` right after the hook. So the hook
// (`rexon-gui::crash_report`) launches THE SAME binary with these flags,
// and a fresh process shows the window from the report file.

/// Flag of the crash window process: `--crash-dialog <report file>`.
pub const CRASH_DIALOG_FLAG: &str = "--crash-dialog";

/// Addition to [`CRASH_DIALOG_FLAG`]: the crashed process survived (a panic
/// in a worker thread), so the window says "keeps running" rather than
/// "has exited".
pub const CRASH_NONFATAL_FLAG: &str = "--crash-nonfatal";

/// A request for the crash window from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrashDialogRequest {
    pub report_path: String,
    pub fatal: bool,
}

/// Parse `--crash-dialog <file>` (or `--crash-dialog=<file>`). `None`
/// means this is not a crash window process. The flag without a file is
/// `None` too: there is nothing to show, and such a launch takes the usual
/// path.
pub fn crash_dialog_request(args: &[String]) -> Option<CrashDialogRequest> {
    let eq = format!("{CRASH_DIALOG_FLAG}=");
    let mut path = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == CRASH_DIALOG_FLAG {
            path = it.next().cloned();
            break;
        }
        if let Some(p) = a.strip_prefix(&eq) {
            path = Some(p.to_string());
            break;
        }
    }
    let path = path.filter(|p| !p.is_empty() && !p.starts_with("--"))?;
    Some(CrashDialogRequest {
        report_path: path,
        fatal: !args.iter().any(|a| a == CRASH_NONFATAL_FLAG),
    })
}

/// Arguments of the crash window process: the reverse side of
/// [`crash_dialog_request`], so parsing and building do not diverge.
pub fn crash_dialog_argv(report_path: &str, fatal: bool) -> Vec<String> {
    let mut argv = vec![CRASH_DIALOG_FLAG.to_string(), report_path.to_string()];
    if !fatal {
        argv.push(CRASH_NONFATAL_FLAG.to_string());
    }
    argv
}

// ============================================================================
// Routes without Qt
// ============================================================================

/// Run a route that does not need Qt. -> the process exit code.
///
/// `None`: the route needs a window, either the main window or a
/// standalone tab, which is now the same section of the same window.
///
/// `--recover-kde` does not open a window at all, and that is not a saving
/// but a condition for working: it is called when the appearance is
/// already broken, and Qt may not come up in such a session. The exit code
/// is the same as the original's: 0 means restored, 1 means the wrong
/// desktop or the theme did not apply.
///
/// `out` is the line sink (`println!` in the binary): the lines go in
/// exactly the order and with the words the original printed them.
pub fn run_headless(args: &[String], out: &mut dyn FnMut(&str)) -> Option<i32> {
    let route = crate::app::route(args);
    // The Plasma login guard (`--session-guard*`): the session script calls
    // it before KWin, ahead of all routes and without a window. It does not
    // intercept help and version: `--help` does nothing but explain.
    if !matches!(route, Route::Help | Route::Version) {
        if let Some(rc) = crate::session_guard::cli_route(args, out) {
            return Some(rc);
        }
        // The game mode guard: the gamescope-session-plus script calls it.
        if let Some(rc) = crate::gamemode_guard::cli_route(args, out) {
            return Some(rc);
        }
    }
    match route {
        Route::Help => {
            help_lines().iter().for_each(|l| out(l));
            Some(0)
        }
        Route::Version => {
            out(&version_text());
            Some(0)
        }
        Route::ApplyDefaults => Some(crate::appdefaults_exec::apply_defaults_route_live(out)),
        Route::SanitizeGtk => Some(sanitize_gtk_route(out)),
        Route::SessionSplash => Some(session_splash_route()),
        Route::RecoverKde => Some(recover_kde_route(out)),
        Route::AeroshellCheck { login } => {
            Some(crate::system_de_exec::aeroshell_check_route_live(login, out))
        }
        Route::UpdateCheck { login } => Some(crate::update_guard::check_route_live(login, out)),
        Route::ConkySet => Some(crate::conky_sets::cli_route(args, out)),
        Route::Doctor => Some(crate::login_rescue::doctor_live(out)),
        Route::DayNight => Some(crate::day_night::cli_route(args, out)),
        Route::SingleTab { .. } | Route::MainWindow => None,
    }
}

/// `rexon --version`: name, version and build commit, the same line the
/// log writes at startup (`applog::version_line`).
pub fn version_text() -> String {
    format!("Rexon {}", crate::applog::version_line())
}

/// `rexon --help`: what can be typed in the console. Command and flag names
/// are literal (they are typed by hand); only the explanation is
/// translated.
///
/// The help contains what is useful to a person. The hooks called by units
/// and login scripts (`--apply-defaults`, `--session-splash`,
/// `--update-check`, `--aeroshell-check`, `--session-guard`,
/// `--session-guard-notice`) are
/// deliberately left out: nobody runs them by hand.
pub fn help_lines() -> Vec<String> {
    let row = |cmd: &str, what: String| format!("  {cmd:<32}{what}");
    // Pages come from the same table that parses the flag: a new shortcut
    // page gets into the help by itself.
    let pages: Vec<&str> = crate::app::PAGE_TARGETS.iter().map(|(p, _, _)| *p).collect();
    let page_flags = format!("{}{}", crate::app::PAGE_FLAG, pages.join("|"));
    vec![
        // TRANSLATORS: first line of `rexon --help`. "rexon [COMMAND]" is typed literally, keep it.
        tr("Usage: rexon [COMMAND]"),
        String::new(),
        // TRANSLATORS: `rexon --help`, heading of the list of commands that open the Rexon window.
        tr("Open the window (needs a desktop session):"),
        // TRANSLATORS: `rexon --help`, explains running `rexon` without arguments.
        row("rexon", tr("open Rexon")),
        // TRANSLATORS: `rexon --help`, explains the --page=install|backups|compare option.
        row(&page_flags, tr("open Rexon on this page")),
        // TRANSLATORS: `rexon --help`, explains the --tray option.
        row(crate::tray::START_HIDDEN_FLAG, tr("start hidden, with only the tray icon")),
        String::new(),
        // TRANSLATORS: `rexon --help`, heading of the list of commands that work in a text console.
        tr("Work without a window (also from a text console):"),
        // TRANSLATORS: `rexon --help`, explains `rexon doctor`.
        row(crate::login_rescue::DOCTOR_WORD, tr("find out why the login screen doesn't start (changes nothing)")),
        // TRANSLATORS: `rexon --help`, explains the --recover-kde option.
        row(crate::app::RECOVER_KDE_FLAG, tr("roll back a broken Plasma look to the standard one")),
        // TRANSLATORS: `rexon --help`, explains the --session-guard-restore option.
        row(crate::session_guard::RESTORE_FLAG, tr("bring back the Plasma look that Rexon set aside after a crash")),
        // TRANSLATORS: `rexon --help`, explains the --gamemode-preflight option.
        row(crate::gamemode_guard::PREFLIGHT_FLAG, tr("check which video card game mode will show the picture on (changes nothing)")),
        // TRANSLATORS: `rexon --help`, explains the --sanitize-gtk option.
        row("--sanitize-gtk", tr("fix links to deleted themes in GTK settings")),
        // TRANSLATORS: `rexon --help`, explains the --conky-set option; NAME is typed literally.
        row(&format!("{} NAME", crate::conky_sets::CLI_FLAG), tr("apply a saved set of Conky widgets")),
        // TRANSLATORS: `rexon --help`, explains the --version option.
        row("--version", tr("show the Rexon version")),
        // TRANSLATORS: `rexon --help`, explains the --help option.
        row("--help", tr("show this help")),
    ]
}

/// Whether this process may bring up a window: there is a display
/// (`WAYLAND_DISPLAY`, `DISPLAY`) or a screenless Qt platform was chosen
/// explicitly (`QT_QPA_PLATFORM=offscreen` for the self-check). Without it
/// `QGuiApplication` dies with `abort` ("could not connect to display"), a
/// core dump instead of words. The decision is
/// [`crate::qt_app::qt_can_start`].
pub fn window_can_open(env: &dyn Fn(&str) -> Option<String>) -> bool {
    crate::qt_app::qt_can_start(
        env("WAYLAND_DISPLAY").as_deref(),
        env("DISPLAY").as_deref(),
        env("QT_QPA_PLATFORM").as_deref(),
    )
}

/// What to say when a window was requested where there is nowhere to show
/// it (SSH, a text console). Together with exit code 1, instead of `abort`.
pub fn no_display_lines() -> Vec<String> {
    vec![
        // TRANSLATORS: printed when `rexon` is run where no window can be shown (SSH, text console).
        tr("Rexon can't open its window here: there is no desktop session (WAYLAND_DISPLAY and DISPLAY are not set)."),
        // TRANSLATORS: second line of the "no desktop session" message. Commands are typed literally.
        tr("Open Rexon from your desktop. In a text console these work: rexon doctor, rexon --help."),
    ]
}

/// The `--sanitize-gtk` route, a port of the `rexon/app.py:640` branch.
///
/// Repair references to missing icon/cursor/colour themes in
/// `~/.config/gtk-{3,4}.0/settings.ini`, `kdeglobals` and `kcminputrc`.
/// Without it GTK applications (Firefox, Nautilus) loop looking for icons
/// of a theme no longer on disk and crash with SIGSEGV after a Global Theme
/// is applied.
///
/// The decisions are entirely [`crate::gtk_sanitizer`], the execution is
/// the installer's live sanitizer, the report text is
/// `gtk_sanitizer::verbose_report` (the same Python prints:
/// `print(report, end="")`, so the report is split into lines rather than
/// printed in one chunk with an extra newline).
pub fn sanitize_gtk_route(out: &mut dyn FnMut(&str)) -> i32 {
    let changes = crate::appearance_installer::live_gtk_sanitize(&home());
    for line in crate::gtk_sanitizer::verbose_report(&changes).lines() {
        out(line);
    }
    for line in gtk_note_lines(&crate::appearance_installer::live_gtk_notes(&home())) {
        out(&line);
    }
    0
}

/// The GTK sanitizer at window startup, a port of the "GTK config
/// auto-sanitizer before startup" block from `rexon/app.py`.
///
/// The same live sanitizer as the `--sanitize-gtk` route above, but silent
/// when there is nothing to repair: at startup a report is needed only
/// about what was done. Broken theme references in gtk-{3,4}.0 crash other
/// GTK applications, so we repair them before the first window.
pub fn startup_gtk_sanitize(out: &mut dyn FnMut(&str)) {
    let home = home();
    for line in startup_gtk_report(&crate::appearance_installer::live_gtk_sanitize(&home)) {
        out(&line);
    }
    for line in gtk_note_lines(&crate::appearance_installer::live_gtk_notes(&home)) {
        out(&line);
    }
}

/// Lines about references the sanitizer noticed but did NOT fix (a sound
/// theme that does not exist: the person's choice is not replaced). Empty
/// means no remarks.
fn gtk_note_lines(notes: &[String]) -> Vec<String> {
    notes.iter().map(|n| format!("⚠️ {n}")).collect()
}

/// Lines of the sanitizer's startup report; empty means there was nothing to repair.
fn startup_gtk_report(changes: &[String]) -> Vec<String> {
    if changes.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![ntr(
        "🛡️ GTK protection: fixed %n broken link:",
        "🛡️ GTK protection: fixed %n broken links:",
        changes.len() as u64,
    )];
    lines.extend(changes.iter().map(|c| format!("   • {c}")));
    lines
}

/// The first snapshot of the application protection baseline at window
/// startup.
///
/// The "Default application protection" section is on by default, but
/// until the first baseline Rexon remembers NOTHING and will not notice a
/// substitution. The customer must not have to press anything for this:
/// the baseline is taken by itself, invisibly, with no window and no
/// question.
///
/// ⚠️ IN A SEPARATE THREAD, not on the window startup path. Taking the
/// snapshot asks the system for the default application of every type:
/// that is `xdg-mime` and `gsettings`, about a dozen subprocesses.
/// ⭐Measured on the bench (private profile, xvfb, this machine): **573 ms**
/// on a clean profile, and that is still the fast case; on a slow machine
/// or with a sluggish `xdg-mime` it is more. On the startup path this would
/// give a hanging window without a single word, exactly the defect the
/// product had already caught while waiting for the wallpaper daemon. The
/// same kind of hook as the bridge announcement at window entry.
///
/// Invisible to the eye, but NOT to the log: whoever investigates later
/// must see that the baseline was taken automatically, how much was
/// remembered and how long it took.
pub fn startup_defaults_snapshot(mut out: Box<dyn FnMut(&str) + Send>) {
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = crate::system_de_exec::startup_defaults_snapshot_live();
        let elapsed = started.elapsed().as_millis();
        for line in startup_snapshot_report(&outcome, elapsed) {
            out(&line);
        }
    });
}

/// Log lines about the first baseline snapshot; empty means there was nothing to take.
fn startup_snapshot_report(
    outcome: &Option<Result<usize, String>>,
    elapsed_ms: u128,
) -> Vec<String> {
    match outcome {
        None => Vec::new(),
        Some(Ok(count)) => vec![trf(
            "🛡️ App protection: baseline taken on first launch (remembered: %1, in %2 ms)",
            &[count, &elapsed_ms],
        )],
        Some(Err(e)) => vec![trf(
            "⚠️ App protection: couldn't take the baseline on first launch: %1",
            &[e],
        )],
    }
}

/// Fields of the Rexon announcement to the bridge, a port of
/// `announce_self` (`rexon/rexon_bridge.py`): the version always, the
/// environment and its human name if known.
fn announce_fields(version: &str, current_de: &str, display: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("rexon_version".into(), version.into());
    if !current_de.is_empty() {
        fields.insert("current_de".into(), current_de.into());
    }
    if !display.is_empty() {
        fields.insert("current_de_display".into(), display.into());
    }
    fields
}

/// Announce Rexon to the bridge at window startup (`rexon_installed`,
/// version, environment): what the Python window does with `announce_self`.
///
/// Without the announcement the `rexon_installed` flag in `bridge.json` is
/// not set, and the very first check by an external application "is Rexon
/// installed at all?" answers "no". There are no expensive probes: only
/// the environment and the name from the core.
pub fn announce_to_bridge() -> std::io::Result<()> {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let de = crate::appdefaults::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default();
    let display = if de.is_empty() { String::new() } else { crate::screenshots::display_name(&de) };
    let fields = announce_fields(&crate::window_menu::version_string(), &de, &display);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let bridge = crate::rexon_bridge::RexonBridge::new_default();
    bridge.ensure_dirs()?;
    bridge.update_rexon_state(fields, &crate::engine_update::now_iso(now.as_secs() as i64))
}

/// The `--recover-kde` route, a port of the `rexon/app.py:682` branch and
/// its nineteen helpers (376 lines).
///
/// Appearance emergency rollback: restore the Global Theme from the backup,
/// remove font overrides and KWin effects from fork themes, return the boot
/// splash and the login screen to the distribution stock, the wallpapers to
/// the standard ones. Not a single line of Qt on the way: the route must
/// work in a session where Qt no longer comes up.
///
/// Decisions and texts are in [`crate::app`], the order of steps and the
/// trips outside in [`crate::system_exec`]; here only the live executor is
/// set up and the exit code returned.
pub fn recover_kde_route(out: &mut dyn FnMut(&str)) -> i32 {
    crate::system_exec::live().recover_kde(out)
}

/// The `--session-splash` route, a port of the `rexon/app.py:679` branch.
///
/// An autostart `.desktop` triggers it at login: play the chosen webm full
/// screen and pick the clip for the next login. Qt is not started here at
/// all: not a single line of Qt on the way.
///
/// The exit code is always 0, and that is NOT sloppiness: the core decides
/// so (`session_splash::Splash::main`): a splash failure must not get in
/// the way of logging in; it goes to the log as a warning.
pub fn session_splash_route() -> i32 {
    let home = home();
    let repo = repo_root();
    // The interpreter is not the original's `sys.executable`: a Rust process
    // has none. It is needed only for rolling back the autostart line to the
    // Python entry.
    let mut io = crate::splash_exec::LiveSplashIo::new(
        &home,
        crate::installer::PYTHON_FALLBACK_EXE,
    );
    let mut splash = crate::session_splash::Splash::new(&mut io, &home);
    splash.main(&repo) as i32
}

// ============================================================================
// Qt and QML messages to the Rexon log
// ============================================================================

/// Whether the Qt message logger was already installed (idempotent like the original).
static QT_LOGGER_INSTALLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Route Qt and QML messages into the Rexon log, a port of
/// `ui_base.install_qt_message_logger` (59 lines).
///
/// An error in a list delegate, a QML module not found, a TypeError in a
/// binding: all of this is diagnostics of the same level as the other
/// entries and must lie in the same file. In the Rust binary NOT A SINGLE
/// QML message got there, and an empty log reads as "QML is silent, no
/// errors".
///
/// ⛔ THERE IS NO MESSAGE HANDLER OF OUR OWN HERE. Qt prints not only from
/// the main thread (QQmlThread compiles QML and complains about it itself),
/// and handling a message in a foreign thread is the very trap because of
/// which the Python part's route is done through environment variables.
/// Qt's stock C++ handler prints; we only give it the format and the
/// destination.
///
/// Install BEFORE `QGuiApplication` is created: then the messages of Qt's
/// own initialisation are visible too. Returns `true` if descriptor 2 was
/// redirected into the log file.
pub fn install_qt_message_logger() -> bool {
    use std::sync::atomic::Ordering;
    if QT_LOGGER_INSTALLED.swap(true, Ordering::SeqCst) {
        return false;
    }
    // `setdefault`, not `set`: a format or destination set explicitly by the
    // person is their choice and must not be overridden.
    for (name, value) in crate::ui_base::qt_logging_env() {
        if std::env::var_os(name).is_none() {
            std::env::set_var(name, value);
        }
    }
    redirect_stderr_to_log()
}

/// Redirect descriptor 2 into the log file. `true` means redirected.
///
/// The "redirect or not" rule lives in the core
/// ([`crate::ui_base::should_redirect_stderr`]); here only the descriptor
/// is observed: a terminal, a file and a pipe are the launcher's explicit
/// choice, and we do not intercept them, otherwise the developer would stop
/// seeing messages where they expect them. We intercept a launch without a
/// console (the "Start" shortcut, a systemd unit), where stderr is a
/// journald socket or `/dev/null`.
fn redirect_stderr_to_log() -> bool {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(2, &mut st) } != 0 {
        return false;
    }
    let mode = st.st_mode;
    let is_tty = unsafe { libc::isatty(2) } == 1;
    if !crate::ui_base::should_redirect_stderr(
        is_tty,
        mode & libc::S_IFMT == libc::S_IFSOCK,
        mode & libc::S_IFMT == libc::S_IFCHR,
        is_tty,
    ) {
        return false;
    }

    // The file and rotation belong to the common log (`crate::applog`):
    // descriptor 2 goes to the same file, while the original error stream
    // (the systemd journal) keeps getting copies of the Rexon lines. Before,
    // there was a separate `dup2` into `~/.config/rexon/rexon.log` here,
    // without a growth limit and without a copy to systemd.
    crate::applog::init();
    crate::applog::redirect_stderr()
}

// ============================================================================
// Labels on Qt's OWN buttons in the Rexon language
// ============================================================================
//
// Buttons Qt draws itself ("OK", "Cancel", "Show Details…", "Apply", the
// edit menu in input fields) are translated only with the qtbase catalog
// loaded; without it English labels pop up in the middle of a Russian
// window. The catalog is taken from Qt itself rather than shipped by us.
//
// THE LANGUAGE IS THE REXON LANGUAGE, not the system's. Previously the
// catalog was chosen by `QLocale::system()` with a fallback to Russian:
// with Rexon in English on a Russian system the standard dialog said
// a Russian "Cancel" in the middle of an English window, and on a system without a
// Qt catalog (say, Japanese) the buttons turned Russian. Now the language
// is exactly the one the core chose for its own strings
// ([`crate::i18n::current_language`]); English (Qt's source language) has
// no catalog at all, and the fallback is English too: without a catalog
// Qt draws English labels itself.
//
// The core holds the DECISION (which files, in what order, for which
// language), the Qt layer the execution (`QTranslator`, `QLibraryInfo`):
// cxx-qt-lib 0.9 has wrappers for neither. The shim returns Qt's
// translations directory and loads exactly the files the core named.

/// Qt's own translation catalogs, in loading order.
///
/// `qtbase`: dialog buttons and the QtWidgets field edit menu;
/// `qtdeclarative`: Qt Quick strings, among them the QML field edit menu
/// (`TextArea`, `TextField`: contexts `CopyAction`, `SelectAllAction`…);
/// `qt`: the historical combined catalog of old Qt builds, not present
/// everywhere.
///
/// ⚠️ Caught 08.11.2026: in Qt 6.12 the QML edit menu in `qtdeclarative` is
/// translated into far from every language (German is there, Russian and
/// Chinese are not). The window shim plugs the holes: for these contexts it
/// takes the same labels from `qtbase` (context `QLineEdit`), see
/// `rexon_gui_factory.cpp`.
pub const QT_TRANSLATION_CATALOGS: &[&str] = &["qtbase", "qtdeclarative", "qt"];

/// The catalog without which Qt's standard labels stay English.
pub const QT_BASE_CATALOG: &str = "qtbase";

/// Separator between the catalog name and the language code (`qtbase_ru.qm`).
pub const QT_TRANSLATION_SEPARATOR: &str = "_";

/// File names (without `.qm`) of catalog `catalog` for the Rexon language
/// `language`, in decreasing preference: `pt_BR` -> `qtbase_pt_BR`,
/// `qtbase_pt`.
///
/// English is Qt's source language: for it the list is empty, no catalog.
/// `language` is a code from [`crate::i18n::current_language`] (`"ru"`,
/// `"pt_BR"`, `"en"`); an encoding and modifier (`.UTF-8`, `@euro`), if
/// they happen to arrive, are dropped.
pub fn qt_translation_candidates(catalog: &str, language: &str) -> Vec<String> {
    let code = language.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
    let base = code.split('_').next().unwrap_or("").to_string();
    if base.is_empty() || base == crate::i18n::SOURCE_LANGUAGE || code == "C" || code == "POSIX" {
        return Vec::new();
    }
    let mut out = vec![format!("{catalog}{QT_TRANSLATION_SEPARATOR}{code}")];
    if base != code {
        out.push(format!("{catalog}{QT_TRANSLATION_SEPARATOR}{base}"));
    }
    out
}

/// Which of Qt's own `.qm` files to load for the Rexon language
/// `language`: for each of [`QT_TRANSLATION_CATALOGS`], the first candidate
/// found in Qt's translations directory `dir`.
///
/// Empty means English: either Rexon is in English, or Qt has no catalog
/// for its language. We do NOT fall back to another language (previously
/// Russian): buttons in a foreign language in the middle of the window are
/// worse than the English ones Qt draws itself.
pub fn qt_translation_files(
    language: &str,
    dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    QT_TRANSLATION_CATALOGS
        .iter()
        .filter_map(|catalog| {
            qt_translation_candidates(catalog, language)
                .into_iter()
                .map(|name| dir.join(format!("{name}.qm")))
                .find(|path| exists(path))
        })
        .collect()
}

/// A log line when Rexon is not in English but `files` has no `qtbase`
/// catalog for its language (with the `zh_CN` -> `zh` fallback): Qt's
/// standard labels are then English, without an error and without
/// substituting the system language.
pub fn qt_translation_missing_note(language: &str, files: &[PathBuf]) -> Option<String> {
    if qt_translation_candidates(QT_BASE_CATALOG, language).is_empty() {
        return None;
    }
    let prefix = format!("{QT_BASE_CATALOG}{QT_TRANSLATION_SEPARATOR}");
    let has_base = files.iter().any(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix))
    });
    (!has_base).then(|| format!("каталога Qt для {language} нет, стандартные надписи по-английски"))
}

/// The environment variable by which the PLATFORM THEME chooses the
/// language of standard buttons (KDE Plasma: `KDEPlasmaPlatformTheme6`).
///
/// ⭐ Caught on the bench 30.09.2026: under `XDG_CURRENT_DESKTOP=KDE` the
/// "Yes/No" buttons of the standard QML dialog are drawn not by qtbase but
/// by the Plasma theme, which translates them with its KF6 catalog
/// (KLocalizedString) by the SYSTEM language. Without the qtbase catalog,
/// on a Russian system with Rexon in English, the dialog still said
/// "Yes/No" in Russian. KLocalizedString listens to `LANGUAGE`, so the window sets it
/// to the Rexon language before the application is created, and the Plasma
/// theme (like any other that listens to gettext) follows Rexon.
pub const QT_UI_LANGUAGE_ENV: &str = "LANGUAGE";

/// A trial run before a restart: the window binary with this variable
/// exits with zero on the first line of `main`. Together with
/// `LD_BIND_NOW=1` this checks "does the new copy load at all": all library
/// symbols are bound before `main`, and a mix of Qt versions shows up right
/// away.
pub const LINK_PROBE_ENV: &str = "REXON_LINK_PROBE";

/// The value of [`QT_UI_LANGUAGE_ENV`] for the Rexon language `language`
/// (a code from [`crate::i18n::current_language`]): the code itself without
/// the encoding; empty or `C` means English, the source language.
pub fn qt_ui_language_env(language: &str) -> String {
    let code = language.split(['.', '@']).next().unwrap_or("").trim().replace('-', "_");
    if code.is_empty() || code == "C" || code == "POSIX" {
        crate::i18n::SOURCE_LANGUAGE.to_string()
    } else {
        code
    }
}

// ============================================================================
// Window lines to the terminal
// ============================================================================

/// What to tell the terminal about the tray icon at window startup:
/// `icon_shown` means the icon is in place, `starts_hidden` means a
/// minimised start was requested.
pub fn tray_status_lines(icon_shown: bool, starts_hidden: bool) -> Vec<String> {
    let status = match (icon_shown, starts_hidden) {
        (true, false) => tr("🔔 Tray: icon added"),
        (true, true) => tr("🔔 Tray: icon added, starting minimized"),
        (false, false) => tr("🔔 Tray: no icon"),
        (false, true) => tr("🔔 Tray: no icon, starting minimized"),
    };
    let mut lines = vec![status];
    if starts_hidden && !icon_shown {
        // Asked to minimise, but there is no icon: the window is the only door
        // into the program and must not be hidden (rule #13).
        lines.push(tr("⚠️ Starting minimized was canceled: there's no tray icon"));
    }
    lines
}

/// How many players were stopped before exiting (printed only if there
/// were more than zero).
pub fn players_stopped_line(players: i64) -> String {
    trf("⏹️ Media players stopped before exit: %1", &[&players])
}

// ============================================================================
// Observations the pure core does not have
// ============================================================================

/// The home directory (the original's `os.path.expanduser("~")`).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

/// Root of the development tree: the core looks there for the built
/// `rexon-splash-overlay`.
///
/// The value is not made up but derived from the path of our own binary
/// (`…/rust/target/<profile>/rexon-main-window`); if it cannot be derived,
/// an empty string, and the core falls back to searching `PATH`, as it
/// knows how to.
///
/// Open to the tabs crate: from the same root the Rust window finds the
/// `rexon/` package (the vendor interface of the wallpaper tab); there is
/// no second copy of the derivation there.
pub fn repo_root() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return String::new();
    };
    repo_root_of(&exe)
}

/// The same derivation, but around SOMEONE ELSE'S file: the first directory
/// above it that contains `rust/Cargo.toml`. Empty means the file is not in
/// a tree.
///
/// ⭐ A separate door is not for looks. The Python entry has no binary of
/// its own: `current_exe` there is `/usr/bin/python3`, and the tree can
/// never be derived from it. The bridge (`rexon-py`) kept its own copy of
/// the rule for this, a second truth about what counts as a developer
/// tree; had they diverged, the decision "offer the first-run wizard or
/// not" would differ between the window and Python. The rule lives here,
/// and [`repo_root`] is the special case "file = own binary" (rule #12).
pub fn repo_root_of(path: &std::path::Path) -> String {
    for dir in path.ancestors() {
        if dir.join("rust").join("Cargo.toml").is_file() {
            return dir.to_string_lossy().into_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The root around SOMEONE ELSE'S file: what the door exists for.
    ///
    /// The tree is built for real: the rule checks that the `rust/Cargo.toml`
    /// file exists, and a fake observation cannot stand in for it.
    #[test]
    fn repo_root_of_foreign_path() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("rexon-repo-root-{nanos}"));
        let root = base.join("Загрузки").join("rexon_v5");
        std::fs::create_dir_all(root.join("rust")).expect("каталог rust создан");
        std::fs::write(root.join("rust").join("Cargo.toml"), "[workspace]\n")
            .expect("манифест дерева записан");
        std::fs::create_dir_all(root.join("rexon")).expect("каталог пакета создан");
        let module = root.join("rexon").join("app.py");
        std::fs::write(&module, "").expect("модуль записан");

        // The Python entry: no binary of its own (`current_exe` is the
        // interpreter), and the tree is derived from this very module.
        assert_eq!(repo_root_of(&module), root.to_string_lossy());
        // The tree's own directory is inside itself too.
        assert_eq!(repo_root_of(&root), root.to_string_lossy());

        // Outside the tree it is empty, and that is not sloppiness: this is how
        // an installed package sits on a customer's machine, and "what if it is a
        // tree" must not be invented.
        let outside = base.join("usr").join("lib").join("rexon").join("app.py");
        std::fs::create_dir_all(outside.parent().expect("родитель есть"))
            .expect("каталог установки создан");
        std::fs::write(&outside, "").expect("файл записан");
        assert_eq!(repo_root_of(&outside), "");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn crash_dialog_flag_forms_and_roundtrip() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            crash_dialog_request(&a(&["--crash-dialog", "/c/crash_1.log"])),
            Some(CrashDialogRequest { report_path: "/c/crash_1.log".into(), fatal: true })
        );
        assert_eq!(
            crash_dialog_request(&a(&["--crash-nonfatal", "--crash-dialog=/c/Отчёт 1.log"])),
            Some(CrashDialogRequest { report_path: "/c/Отчёт 1.log".into(), fatal: false })
        );
        // A flag without a file is not a window process: there is nothing to show.
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog"])), None);
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog", "--tray"])), None);
        assert_eq!(crash_dialog_request(&a(&["--crash-dialog="])), None);
        assert_eq!(crash_dialog_request(&a(&["--section=appearance"])), None);
        for fatal in [true, false] {
            let argv = crash_dialog_argv("/r/crash_2.log", fatal);
            assert_eq!(
                crash_dialog_request(&argv),
                Some(CrashDialogRequest { report_path: "/r/crash_2.log".into(), fatal })
            );
        }
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Nothing to repair: silent at startup; repaired: the number and the list.
    #[test]
    fn startup_gtk_report_speaks_only_about_repairs() {
        english();
        assert!(startup_gtk_report(&[]).is_empty());
        let lines = startup_gtk_report(&["gtk-3.0: icon theme".to_string()]);
        assert_eq!(lines[0], "🛡️ GTK protection: fixed 1 broken link:");
        assert_eq!(lines[1], "   • gtk-3.0: icon theme");
    }

    /// A remark about an unfixed reference goes on a separate line, without the word "fixed".
    #[test]
    fn gtk_note_lines_do_not_claim_a_fix() {
        assert!(gtk_note_lines(&[]).is_empty());
        let lines = gtk_note_lines(&["gtk-3.0/settings.ini: x".to_string()]);
        assert_eq!(lines, ["⚠️ gtk-3.0/settings.ini: x"]);
    }

    /// Announcement fields as in `announce_self`: the version always, the
    /// environment and its name only if known (empty ones are not written to
    /// the bridge).
    #[test]
    fn announce_fields_skip_unknown_desktop() {
        let full = announce_fields("0.0.1", "kde", "KDE Plasma");
        let keys: Vec<&str> = full.keys().map(String::as_str).collect();
        assert_eq!(keys, ["rexon_version", "current_de", "current_de_display"]);
        let bare = announce_fields("0.0.1", "", "");
        assert_eq!(bare.keys().map(String::as_str).collect::<Vec<_>>(), ["rexon_version"]);
    }

    #[test]
    fn each_single_tab_flag_leads_to_section() {
        for (flag, module) in crate::app::SINGLE_TAB_FLAGS {
            let (section, _sub) = single_tab_section(module)
                .unwrap_or_else(|| panic!("у флага {flag} нет раздела"));
            assert!(
                crate::qt_main::SECTIONS.contains(&section),
                "раздел {section} флага {flag} не из состава окна"
            );
        }
    }

    #[test]
    fn sections_have_no_extra_entries() {
        assert_eq!(SINGLE_TAB_SECTIONS.len(), crate::app::SINGLE_TAB_FLAGS.len());
        for (module, _, _) in SINGLE_TAB_SECTIONS {
            assert!(
                crate::app::SINGLE_TAB_FLAGS.iter().any(|(_, m)| m == module),
                "модуль {module} не соответствует ни одному флагу"
            );
        }
    }

    #[test]
    fn section_beats_tab_flag_and_launcher() {
        assert_eq!(
            start_section(&argv(&["--section=steam", "--settings-qt", "--page=install"])),
            Some(("steam".into(), String::new()))
        );
    }

    #[test]
    fn tab_flag_beats_launcher() {
        assert_eq!(
            start_section(&argv(&["--compare-qt", "--page=backups"])),
            Some(("desktops".into(), "compare".into()))
        );
    }

    #[test]
    fn launcher_translated_as_before() {
        assert_eq!(
            start_section(&argv(&["--page=backups"])),
            Some(("settings".into(), String::new()))
        );
        assert_eq!(start_section(&argv(&[])), None);
    }

    /// An empty `--section=` is not "a section with an empty name" but a typo:
    /// the window must open on the default section, not on nothing.
    #[test]
    fn empty_section_not_counted_as_section() {
        assert_eq!(start_section(&argv(&["--section="])), None);
        assert_eq!(
            start_section(&argv(&["--section=appearance/screeneffects"])),
            Some(("appearance".to_string(), "screeneffects".to_string()))
        );
        assert_eq!(
            start_section(&argv(&["--section=  ", "--page=install"])),
            Some(("desktops".into(), String::new()))
        );
    }

    #[test]
    fn window_routes_give_no_exit_code() {
        let mut said: Vec<String> = Vec::new();
        let mut out = |s: &str| said.push(s.to_string());
        assert_eq!(run_headless(&argv(&[]), &mut out), None);
        assert_eq!(run_headless(&argv(&["--settings-qt"]), &mut out), None);
        assert!(said.is_empty(), "маршруты окна ничего не печатают");
    }

    /// `rexon --help` and `--version` without a display: until 30.10.2026 both
    /// fell through to the main window, and Qt died with `abort` instead of
    /// printing help. Now they are windowless routes with code 0, and they beat
    /// any other flag (`--recover-kde --help` explains rather than rolls back).
    #[test]
    fn help_and_version_without_window_and_with_zero() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for args in [&["--help"][..], &["-h"], &["help"], &["--recover-kde", "--help"], &["--session-guard", "-h"]] {
            assert_eq!(crate::app::route(&argv(args)), Route::Help, "{args:?}");
            let mut said: Vec<String> = Vec::new();
            assert_eq!(run_headless(&argv(args), &mut |s| said.push(s.to_string())), Some(0), "{args:?}");
            assert_eq!(said, help_lines(), "{args:?}");
        }
        for args in [&["--version"][..], &["-V"], &["--apply-defaults", "--version"]] {
            assert_eq!(crate::app::route(&argv(args)), Route::Version, "{args:?}");
            let mut said: Vec<String> = Vec::new();
            assert_eq!(run_headless(&argv(args), &mut |s| said.push(s.to_string())), Some(0), "{args:?}");
            assert_eq!(said, vec![version_text()]);
        }
        assert!(version_text().starts_with("Rexon "), "{}", version_text());
        // The word `help` counts only in first position: a Conky set may be called that.
        assert_eq!(crate::app::route(&argv(&["--conky-set", "help"])), Route::ConkySet);
        // The help contains every console command worth knowing.
        let text = help_lines().join("\n");
        for cmd in ["doctor", "--recover-kde", "--session-guard-restore", "--sanitize-gtk", "--conky-set", "--version", "--help", "--page=", "--tray"] {
            assert!(text.contains(cmd), "в справке нет {cmd}:\n{text}");
        }
    }

    /// A window without a display: clear lines instead of Qt's `abort`. The
    /// self-check with `QT_QPA_PLATFORM=offscreen` needs no display.
    #[test]
    fn window_without_display_not_raised() {
        let env_of = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert!(!window_can_open(&env_of(&[])));
        assert!(!window_can_open(&env_of(&[("WAYLAND_DISPLAY", ""), ("DISPLAY", "")])));
        assert!(!window_can_open(&env_of(&[("QT_QPA_PLATFORM", "wayland;xcb")])));
        assert!(window_can_open(&env_of(&[("WAYLAND_DISPLAY", "wayland-0")])));
        assert!(window_can_open(&env_of(&[("DISPLAY", ":0")])));
        assert!(window_can_open(&env_of(&[("QT_QPA_PLATFORM", "offscreen")])));
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let lines = no_display_lines();
        assert!(lines.iter().any(|l| l.contains("rexon doctor")), "{lines:?}");
    }

    /// `--recover-kde` is a route with an exit code, not a window. The run
    /// itself is DELIBERATELY not done here: the route changes the live machine
    /// (pkexec, plasma-apply-*, a plasmashell restart). Its order of steps and
    /// its words are checked on fake input/output in
    /// `tests/system_exec_parity.rs`, where there is no live machine at all.
    #[test]
    fn recover_kde_parsed_as_route_not_window() {
        assert_eq!(
            crate::app::route(&argv(&["--recover-kde"])),
            Route::RecoverKde
        );
    }

    /// This build's Qt translations directory in miniature: Russian, German,
    /// Brazilian Portuguese, and English, which must not be loaded.
    const QT_DIR: &str = "/qt/translations";
    fn qt_has(files: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| {
            p.parent() == Some(Path::new(QT_DIR))
                && files.iter().any(|f| p.file_name().and_then(|n| n.to_str()) == Some(*f))
        }
    }
    const SHIPPED: &[&str] =
        &[
            "qtbase_ru.qm",
            "qt_ru.qm",
            "qtbase_de.qm",
            "qtdeclarative_de.qm",
            "qtbase_en.qm",
            "qt_en.qm",
            "qtbase_pt_BR.qm",
            "qtbase_zh.qm",
        ];

    fn qm(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| Path::new(QT_DIR).join(n)).collect()
    }

    #[test]
    fn qt_catalog_candidates_by_rexon_language() {
        assert_eq!(qt_translation_candidates("qtbase", "ru"), vec!["qtbase_ru"]);
        assert_eq!(qt_translation_candidates("qtbase", "pt_BR"), vec!["qtbase_pt_BR", "qtbase_pt"]);
        assert_eq!(qt_translation_candidates("qt", "de_DE.UTF-8"), vec!["qt_de_DE", "qt_de"]);
        for english in ["en", "en_GB", "", "C", "POSIX"] {
            assert!(qt_translation_candidates("qtbase", english).is_empty(), "{english:?}");
        }
    }

    /// Which of Qt's own `.qm` is chosen: Rexon decides the language, not the system.
    #[test]
    fn qt_files_chosen_by_rexon_language() {
        let exists = qt_has(SHIPPED);
        let dir = Path::new(QT_DIR);
        // Rexon in Russian: both catalogs.
        assert_eq!(qt_translation_files("ru", dir, &exists), qm(&["qtbase_ru.qm", "qt_ru.qm"]));
        // In German: only what exists (there is no `qt_de`).
        assert_eq!(
            qt_translation_files("de", dir, &exists),
            qm(&["qtbase_de.qm", "qtdeclarative_de.qm"])
        );
        // No catalog for the full code: fall back to the language without the region.
        assert_eq!(qt_translation_files("zh_CN", dir, &exists), qm(&["qtbase_zh.qm"]));
        assert_eq!(qt_translation_files("pt_BR", dir, &exists), qm(&["qtbase_pt_BR.qm"]));
        // In English: NOTHING, although `qtbase_en.qm` is on disk.
        assert!(qt_translation_files("en", dir, &exists).is_empty());
        // Qt lacks the language: fall back to English (empty), NOT to Russian.
        assert!(qt_translation_files("ja", dir, &exists).is_empty());
        assert!(qt_translation_files("uk", dir, &exists).is_empty());
    }

    /// The Rexon language exists, the Qt catalog does not: Qt labels are
    /// English, one line goes to the log; the system language plays no part.
    #[test]
    fn no_qt_catalog_for_rexon_language_one_log_line() {
        let exists = qt_has(SHIPPED);
        let dir = Path::new(QT_DIR);
        let ja = qt_translation_files("ja", dir, &exists);
        assert!(ja.is_empty());
        assert_eq!(
            qt_translation_missing_note("ja", &ja).as_deref(),
            Some("каталога Qt для ja нет, стандартные надписи по-английски")
        );
        // Only the combined `qt_*` without `qtbase_*` also counts as "none".
        assert!(qt_translation_missing_note("uk", &[dir.join("qt_uk.qm")]).is_some());
        // The catalog exists (including through the fallback): silent.
        for lang in ["ru", "de", "pt_BR", "zh_CN"] {
            let files = qt_translation_files(lang, dir, &exists);
            assert_eq!(qt_translation_missing_note(lang, &files), None, "{lang}");
        }
        // English is Qt's source language; it needs no catalog.
        assert_eq!(qt_translation_missing_note("en", &[]), None);
    }

    /// End-to-end check: environment ▸ Rexon language (as `i18n::activate`
    /// chooses it) ▸ Qt files. In this build only Russian has a Rexon catalog.
    fn qt_files_for(setting: Option<&str>, env: &[(&str, &str)]) -> Vec<PathBuf> {
        let env: Vec<(String, String)> =
            env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let lookup = |k: &str| env.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        let candidates = crate::i18n::language_candidates(setting, &lookup);
        let dirs = vec![PathBuf::from("/rexon/locale")];
        let rexon_has = |p: &Path| p == crate::i18n::catalog_file(Path::new("/rexon/locale"), "ru", "mo");
        let language = crate::i18n::resolve(&candidates, &dirs, &rexon_has)
            .map(|(lang, _)| lang)
            .unwrap_or_else(|| crate::i18n::SOURCE_LANGUAGE.to_string());
        qt_translation_files(&language, Path::new(QT_DIR), &qt_has(SHIPPED))
    }

    /// The platform theme (KDE) gets the Rexon language through `LANGUAGE`.
    #[test]
    fn platform_theme_language_equals_rexon_language() {
        assert_eq!(QT_UI_LANGUAGE_ENV, "LANGUAGE");
        assert_eq!(qt_ui_language_env("en"), "en");
        assert_eq!(qt_ui_language_env("ru"), "ru");
        assert_eq!(qt_ui_language_env("pt_BR"), "pt_BR");
        assert_eq!(qt_ui_language_env("de_DE.UTF-8"), "de_DE");
        assert_eq!(qt_ui_language_env(""), "en");
        assert_eq!(qt_ui_language_env("C"), "en");
    }

    #[test]
    fn english_rexon_on_russian_system_without_qt_catalog() {
        let ru_system = [("LANG", "ru_RU.UTF-8")];
        // A Russian system, Rexon in Russian: Russian Qt buttons.
        assert_eq!(qt_files_for(None, &ru_system), qm(&["qtbase_ru.qm", "qt_ru.qm"]));
        // `LANGUAGE=en` on a Russian system: English buttons.
        assert!(qt_files_for(None, &[("LANGUAGE", "en"), ("LANG", "ru_RU.UTF-8")]).is_empty());
        // The Rexon setting "English" on a Russian system: the same.
        assert!(qt_files_for(Some("en"), &ru_system).is_empty());
        // A language Rexon has no catalog for (a Japanese system): Rexon speaks
        // English, and so do the Qt buttons, not Russian.
        assert!(qt_files_for(None, &[("LANG", "ja_JP.UTF-8")]).is_empty());
        // German: Qt has a catalog, Rexon (in this build) does not. The buttons
        // follow Rexon; otherwise "Abbrechen" would stand in the middle of an
        // English window.
        assert!(qt_files_for(Some("de"), &ru_system).is_empty());
    }

    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn cyrillic(s: &str) -> bool {
        s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
    }

    #[test]
    fn english_terminal_lines_have_no_russian() {
        english();
        let mut all = Vec::new();
        all.extend(startup_gtk_report(&["gtk-3.0: icon theme".into(), "kdeglobals: x".into()]));
        all.extend(startup_snapshot_report(&Some(Ok(7)), 142));
        all.extend(startup_snapshot_report(&Some(Err("disk full".into())), 3));
        for icon in [true, false] {
            for hidden in [true, false] {
                all.extend(tray_status_lines(icon, hidden));
            }
        }
        all.push(players_stopped_line(2));
        for line in &all {
            assert!(!cyrillic(line), "русская буква в английском тексте: {line}");
        }
        assert_eq!(all[0], "🛡️ GTK protection: fixed 2 broken links:");
        assert_eq!(
            startup_gtk_report(&["gtk-3.0: icon theme".into()])[0],
            "🛡️ GTK protection: fixed 1 broken link:"
        );
        assert_eq!(
            startup_snapshot_report(&Some(Ok(7)), 142)[0],
            "🛡️ App protection: baseline taken on first launch (remembered: 7, in 142 ms)"
        );
        assert_eq!(
            tray_status_lines(false, true),
            vec![
                "🔔 Tray: no icon, starting minimized".to_string(),
                "⚠️ Starting minimized was canceled: there's no tray icon".to_string(),
            ]
        );
        assert_eq!(tray_status_lines(true, false), vec!["🔔 Tray: icon added".to_string()]);
        assert_eq!(players_stopped_line(2), "⏹️ Media players stopped before exit: 2");
    }

    /// The logger is idempotent: a second call does nothing (otherwise
    /// descriptor 2 would be redirected into the log over and over at every
    /// entry point).
    #[test]
    fn logger_installed_once() {
        install_qt_message_logger();
        assert!(!install_qt_message_logger());
    }

    /// Log of the first baseline snapshot: silent when there is nothing to
    /// take, and names the number remembered when it was taken.
    #[test]
    fn startup_snapshot_report_lines() {
        english();
        assert!(startup_snapshot_report(&None, 0).is_empty());
        let ok = startup_snapshot_report(&Some(Ok(7)), 142);
        assert_eq!(ok.len(), 1);
        assert!(ok[0].contains("remembered: 7"), "{}", ok[0]);
        assert!(ok[0].contains("142 ms"), "{}", ok[0]);
        let bad = startup_snapshot_report(&Some(Err("нет места".to_string())), 3);
        assert_eq!(bad.len(), 1);
        assert!(bad[0].contains("нет места"), "{}", bad[0]);
    }
}
