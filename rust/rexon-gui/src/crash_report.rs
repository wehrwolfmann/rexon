//! Crash report of the Rust window (`rexon-main-window`).
//!
//! The Python host had a report (`rexon/qt_main/app.py`: `sys.excepthook` →
//! a file in `~/.config/rexon/crashes` + a window), the Rust window had
//! nothing: on a panic the window simply disappeared, and the customer
//! learned neither what happened nor what to send to the developer. Here is
//! the same for panics.
//!
//! BOUNDARY. The directory, the file name, the header, rotation, the window
//! texts, the decision "one report per process / is it fatal" are the
//! core's (`rexon_core::qt_app`), the window process flag is
//! `rexon_core::cli`. Here there is only execution: the handler, collecting
//! system information, starting the window and the window itself.
//!
//! WHY THE WINDOW IS A SEPARATE PROCESS. A crashed process cannot draw:
//!   * a panic may happen in a worker thread, while Qt allows windows only
//!     in the GUI thread (in Python this was cured by re-throwing through a
//!     signal — here there is nobody to receive it: the event loop may
//!     already be dead);
//!   * a panic in a handler called by Qt hits the FFI boundary, and the
//!     process gets an `abort` RIGHT after the handler — there is no time
//!     for any event loop;
//!   * the cause of the panic may be Qt itself falling apart.
//!
//! So the handler writes a file and starts THE SAME binary with the flag
//! `--crash-dialog <file>`: a fresh process brings up its own Qt and shows
//! the window, and the crashed one quietly dies. `/proc/self/exe` is
//! started, not the path on disk: when the program is updated over a
//! running one, the file at the path is already different (or removed),
//! while the process image is the very one.
//!
//! The window process has two stages: a "guard" (without Qt) starts the
//! "window" and waits for a "shown" mark from it. No mark — Qt did not
//! start (`abort`), the QML did not load — the guard shows a system window
//! (kdialog, then zenity). No display at all — nothing is shown: the file
//! and a line in stderr remain.
//! The crashed process cannot wait — hence a separate guard. If the binary
//! itself did not start, the handler already tries a system window.
//!
//! Hook-up — one line, the first in the binary's `main`:
//! `rexon_gui::crash_report::install();` — it also runs the window process
//! (and then does not return).

use std::ffi::{c_char, CStr};
use std::os::unix::process::CommandExt;
use std::panic::PanicHookInfo;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

use cxx_qt_lib::{
    QByteArray, QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString,
    QUrl, QVariant,
};
use rexon_core::{cli, qt_app, ui_base};

/// Do not show the crash window (only the file and stderr). For runs
/// without a screen: tests, CI, scripts. Any non-empty value except `0`.
pub const NO_DIALOG_ENV: &str = "REXON_CRASH_NO_DIALOG";

/// Stage of the window process: without the variable — the guard, `window`
/// — the window itself. The variable also tells the window process from an
/// accidental manual run with the flag: such a run also goes through the
/// guard, and that is right.
const STAGE_ENV: &str = "REXON_CRASH_DIALOG_STAGE";
const STAGE_WINDOW: &str = "window";

/// Window exit code "the QML did not load" — time for the guard to call
/// the fallback.
const WINDOW_LOAD_FAILED: i32 = 3;

/// The line with which the window tells the guard "I am on screen" (on its
/// stdout; the guard holds the other end of the pipe). After it the
/// fallback windows are not called, however the window ends: the display
/// disappears on logout, Qt exits with an error — and without this mark the
/// guard would call kdialog where there is nothing to show any more (it
/// crashes with `abort`, caught by a live run).
const WINDOW_SHOWN_MARKER: &str = "crash-dialog-shown";

/// There has already been a report in this process (see
/// `qt_app::panic_decision`).
static REPORTED: AtomicBool = AtomicBool::new(false);

/// Install the panic handler. Call as the FIRST line of `main`.
///
/// If this process is the crash window (`--crash-dialog <file>`), the
/// function shows the window and ends the process without returning: an
/// ordinary start (the "one Rexon per session" lock, the tray, the main
/// window) is not needed and is harmful for it — the crashed instance may
/// still hold the lock.
pub fn install() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(req) = cli::crash_dialog_request(&args) {
        // Lines of the crash window ("nowhere to show", "did not come up") go to
        // the shared log: this process starts without a terminal, and otherwise
        // nobody would read its error stream.
        rexon_core::applog::init();
        let code = if std::env::var(STAGE_ENV).as_deref() == Ok(STAGE_WINDOW) {
            run_window(&req)
        } else {
            supervise(&req, &args)
        };
        std::process::exit(code);
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Standard printing to stderr — always and first: the terminal and the log
        // must see the panic even if the report is not written afterwards.
        previous(info);
        report_panic(info);
    }));
}

/// Body of the handler. Not a single `unwrap`: a panic inside the handler
/// is an immediate `abort` without a report.
fn report_panic(info: &PanicHookInfo) {
    let thread = std::thread::current();
    // TRANSLATORS: stands in the crash report for a thread that has no name.
    let unnamed = rexon_core::i18n::tr("<unnamed>");
    let thread_name = thread.name().unwrap_or(&unnamed);
    let decision = qt_app::panic_decision(
        thread.name() == Some("main"),
        REPORTED.swap(true, Ordering::SeqCst),
    );
    if !decision.save_log {
        return;
    }

    let message = info
        .payload_as_str()
        .map(str::to_string)
        // TRANSLATORS: stands in the crash report when the error has no message.
        .unwrap_or_else(|| rexon_core::i18n::tr("(panic without a message)"));
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
    // Forced capture: the customer does not have the RUST_BACKTRACE variable
    // set, and a report without a stack is almost useless.
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();

    let now = qt_app::Timestamp::now_local().unwrap_or_default();
    let header = live_header(now);
    let body = qt_app::panic_crash_text(&message, thread_name, location.as_deref(), &backtrace);
    let dir = crash_dir();

    let saved = qt_app::save_crash_log(&mut qt_app::AtomicCrashIo, &dir, &now, &header, &body);
    let path = saved.or_else(|| {
        // The reports directory is unavailable (no space, foreign permissions) —
        // the window still needs a report, and it goes through a temporary file.
        let p = qt_app::crash_transport_path(&runtime_dir(), std::process::id());
        qt_app::atomic_write(&p, &format!("{header}{body}")).ok().map(|_| p)
    });
    // One line to the shared log (`applog`): the panic, where, and where the
    // report went. The standard panic printing has already gone to the error
    // stream above.
    rexon_core::log_error!(
        "crash",
        "паника в потоке «{thread_name}»{}: {message}; отчёт: {}",
        location.as_deref().map(|l| format!(" ({l})")).unwrap_or_default(),
        path.as_deref().unwrap_or("не записан")
    );

    if !decision.show_dialog || dialog_disabled() {
        return;
    }
    if !can_show() {
        rexon_core::log_warn!("crash", "окно о крахе показать негде: нет дисплея");
        return;
    }
    let launched = path
        .as_deref()
        .is_some_and(|p| spawn_dialog_process(p, decision.fatal));
    if !launched {
        // Our own binary did not start (or there is nothing to show from the
        // file) — a system window right from here, without waiting.
        let report = format!("{header}{body}");
        let dialog = qt_app::crash_dialog_for_report(
            &report,
            path.as_deref().unwrap_or(""),
            &dir,
            decision.fatal,
        );
        for argv in qt_app::crash_fallback_argvs(&dialog) {
            if spawn_detached(Command::new(&argv[0]).args(&argv[1..]), decision.fatal) {
                break;
            }
        }
    }
}

fn can_show() -> bool {
    qt_app::crash_dialog_can_show(
        env_opt("WAYLAND_DISPLAY").as_deref(),
        env_opt("DISPLAY").as_deref(),
    )
}

fn dialog_disabled() -> bool {
    std::env::var_os(NO_DIALOG_ENV).is_some_and(|v| !v.is_empty() && v != "0")
}

/// Start the crash window process (the "guard" stage).
fn spawn_dialog_process(report_path: &str, fatal: bool) -> bool {
    let mut cmd = self_command();
    cmd.args(cli::crash_dialog_argv(report_path, fatal))
        .env_remove(STAGE_ENV)
        // The crash window speaks the same language as the crashed window. It does
        // not read the settings itself (their breakage may be the cause of the
        // crash), so the chosen language travels to it through the environment:
        // `LANGUAGE` is the first thing language selection listens to with an
        // empty setting (`i18n::language_candidates`).
        .env("LANGUAGE", rexon_core::i18n::current_language())
        // The "original LANGUAGE" mark would override the language chosen above:
        // the crash window must take exactly that one.
        .env_remove(rexon_core::i18n::ORIGINAL_LANGUAGE_ENV);
    spawn_detached(&mut cmd, fatal)
}

/// A command on our own image (see the module header: `/proc/self/exe`, not
/// the path).
fn self_command() -> Command {
    let mut cmd = Command::new("/proc/self/exe");
    if let Some(arg0) = std::env::args_os().next() {
        cmd.arg0(arg0);
    }
    cmd
}

/// Start without waiting, in our own process group: Ctrl+C in the terminal,
/// used to kill the crashed process, must not kill the window about it.
///
/// If the process survives (`!fatal`), a separate thread waits for the
/// child — otherwise a closed window would hang as a zombie until Rexon
/// exits.
fn spawn_detached(cmd: &mut Command, fatal: bool) -> bool {
    cmd.stdin(Stdio::null()).process_group(0);
    match cmd.spawn() {
        Ok(mut child) => {
            if !fatal {
                let _ = std::thread::Builder::new()
                    .name("crash-dialog-reaper".into())
                    .spawn(move || {
                        let _ = child.wait();
                    });
            }
            true
        }
        Err(_) => false,
    }
}

// ============================================================================
// System information for the header
// ============================================================================

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn crash_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    qt_app::crash_dir(env_opt("XDG_CONFIG_HOME").as_deref(), &home)
}

fn runtime_dir() -> String {
    env_opt("XDG_RUNTIME_DIR")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned())
}

extern "C" {
    /// Version of the loaded Qt. C linkage (declared in Qt inside
    /// `extern "C"`), returns a static string — safe to call even when Qt
    /// itself is falling apart.
    fn qVersion() -> *const c_char;
}

#[cfg(target_env = "gnu")]
extern "C" {
    fn gnu_get_libc_version() -> *const c_char;
}

fn c_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

fn libc_version() -> Option<(&'static str, String)> {
    #[cfg(target_env = "gnu")]
    {
        c_str(unsafe { gnu_get_libc_version() }).map(|v| ("glibc", v))
    }
    #[cfg(not(target_env = "gnu"))]
    {
        None
    }
}

fn read_trim(path: &str) -> String {
    std::fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Report header from the live system.
fn live_header(now: qt_app::Timestamp) -> String {
    let system = Some(read_trim("/proc/sys/kernel/ostype"))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Linux".to_string());
    let release = read_trim("/proc/sys/kernel/osrelease");
    let libc = libc_version();
    let os = qt_app::os_platform(
        &system,
        &release,
        std::env::consts::ARCH,
        libc.as_ref().map(|(n, v)| (*n, v.as_str())),
    );
    let entry = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let qt = c_str(unsafe { qVersion() });
    let (xcd, ds, st, lang) = (
        env_opt("XDG_CURRENT_DESKTOP"),
        env_opt("DESKTOP_SESSION"),
        env_opt("XDG_SESSION_TYPE"),
        env_opt("LANG"),
    );
    qt_app::rust_crash_header(&qt_app::RustCrashHeaderInput {
        now,
        rexon_version: env!("CARGO_PKG_VERSION"),
        entry: &entry,
        qt_version: qt.as_deref(),
        env: qt_app::CrashHeaderEnv {
            os_platform: &os,
            xdg_current_desktop: xcd.as_deref(),
            desktop_session: ds.as_deref(),
            xdg_session_type: st.as_deref(),
            lang: lang.as_deref(),
        },
    })
}

// ============================================================================
// Crash window process
// ============================================================================

/// Window content from the report file. If the file cannot be read, the
/// window is shown anyway: the user must learn that the program crashed.
fn load_dialog(req: &cli::CrashDialogRequest) -> qt_app::CrashDialog {
    let report = std::fs::read(&req.report_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_else(|e| {
            // TRANSLATORS: %1 is the report file path, %2 the system's error text.
            rexon_core::i18n::trf("(the report file can't be read: %1: %2)", &[&req.report_path, &e])
        });
    qt_app::crash_dialog_for_report(&report, &req.report_path, &crash_dir(), req.fatal)
}

/// The "guard" stage: start the window and wait; if it did not show — the
/// fallbacks.
fn supervise(req: &cli::CrashDialogRequest, args: &[String]) -> i32 {
    if !can_show() {
        rexon_core::log_warn!("crash", "окно о крахе показать негде (нет дисплея); отчёт: {}", req.report_path);
        return 1;
    }
    match run_window_stage(args) {
        Ok(()) => return 0,
        Err(why) => rexon_core::log_warn!("crash", "окно о крахе не поднялось ({why}) — пробую системное"),
    }
    let dialog = load_dialog(req);
    for argv in qt_app::crash_fallback_argvs(&dialog) {
        match Command::new(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).status() {
            Ok(s) if s.success() => return 0,
            Ok(_) | Err(_) => continue,
        }
    }
    rexon_core::log_error!("crash", "окно о крахе показать нечем; отчёт: {}", req.report_path);
    1
}

/// Start the "window" stage and wait. `Ok` means the window managed to show
/// (the [`WINDOW_SHOWN_MARKER`] mark), however it ended afterwards.
fn run_window_stage(args: &[String]) -> Result<(), String> {
    use std::io::BufRead;
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    // The command is temporary and dies at the end of the expression together
    // with the copy of the writing end: otherwise the read below would not get
    // the end of the pipe.
    let mut child = self_command()
        .args(args)
        .env(STAGE_ENV, STAGE_WINDOW)
        .stdin(Stdio::null())
        .stdout(writer)
        .spawn()
        .map_err(|e| e.to_string())?;
    // Only the first line: after showing, the window may start a file manager
    // ("Open folder"), which inherits the pipe — waiting for its end would mean
    // hanging until the user closes the file manager.
    let mut line = String::new();
    let _ = std::io::BufReader::new(reader).read_line(&mut line);
    let status = child.wait();
    if line.trim_end() == WINDOW_SHOWN_MARKER {
        return Ok(());
    }
    Err(match status {
        Ok(s) => s.to_string(),
        Err(e) => e.to_string(),
    })
}

/// The "window" stage: its own Qt, a QML window, code 0 — the user closed
/// it.
fn run_window(req: &cli::CrashDialogRequest) -> i32 {
    let dialog = load_dialog(req);
    // An empty QT_QPA_PLATFORM prevents Qt from choosing the wayland plugin —
    // the same core rule as for all entry points. There are no threads yet.
    if qt_app::should_unset_qt_platform(env_opt("QT_QPA_PLATFORM").as_deref()) {
        unsafe { std::env::remove_var("QT_QPA_PLATFORM") };
    }

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_organization_name(&QString::from(qt_app::ORGANIZATION_NAME));
    }
    if let Some(app) = app.as_mut() {
        // ⭐ THE NAME FOR THE SYSTEM IS A MACHINE NAME AND ONE PER PRODUCT. The
        // crash window is a separate process, and until now Qt called it by the
        // translated title of the MAIN window (`qt_main::WINDOW_TITLE`): one
        // product came to the system under two names at once — `Rexon` from the
        // main window and a human-readable string from here. The name has one
        // door — `rexon_core::tray::APP_ID`, where the analysis of why it must be
        // constant and untranslatable lives too.
        //
        // This process has no tray icon, but the application name goes past the
        // tray as well: crash entries in the system journal are labelled with it,
        // and the portal and system windows take it. The visible title of the
        // window itself does NOT change because of this: the user reads
        // `qt_app::crash_dialog_title()`, which comes into QML as the
        // `dialogTitle` property and has nothing to do with the application name.
        app.set_application_name(&QString::from(rexon_core::tray::APP_ID));
    }
    // Window icon in the X11 task bar — the same as the main window's.
    crate::tray::set_app_window_icon();
    QGuiApplication::set_desktop_file_name(&QString::from(qt_app::DESKTOP_FILE_NAME));
    // The crash window speaks Rexon's language — right to left too, and the
    // standard Qt labels (the edit menu of the report field) too.
    crate::install_qt_translations(&rexon_core::i18n::current_language());
    crate::fonts::install_rtl_fallbacks();

    let mut engine = QQmlApplicationEngine::new();
    let Some(mut engine) = engine.as_mut() else {
        return WINDOW_LOAD_FAILED;
    };

    // The window is in the product's standard skin. The default skin, not the
    // one chosen in the settings: the crash window has no right to depend on
    // reading the settings — their breakage may turn out to be the cause of
    // the crash.
    let colors = ui_base::skin_colors(ui_base::DEFAULT_SKIN);
    let color = |k: &str| colors.get(k).cloned().unwrap_or_default();
    let folder = dialog
        .informative
        .as_ref()
        .map(|_| {
            let dir = qt_app::crash_open_folder_argv(&req.report_path)
                .pop()
                .unwrap_or_default();
            QUrl::from_local_file(&QString::from(&dir)).to_qstring()
        })
        .unwrap_or_default();

    let mut props = QMap::<QMapPair_QString_QVariant>::default();
    let mut put = |k: &str, v: &str| {
        props.insert(QString::from(k), QVariant::from(&QString::from(v)));
    };
    put("dialogTitle", &dialog.title);
    put("mainText", &dialog.text);
    put("informative", dialog.informative.as_deref().unwrap_or(""));
    put("detailed", &dialog.detailed);
    put("folderUrl", &folder.to_string());
    // The "Report a problem" button exists only when the product has an issue
    // tracker address set (`qt_app::ISSUE_TRACKER_URL`): an empty address
    // means an empty property, and QML hides the button.
    let report = qt_app::crash_report_issue_button();
    let report_url = report
        .as_ref()
        .and_then(|(_, argv)| argv.last())
        .map(String::as_str)
        .unwrap_or("");
    put("reportLabel", report.as_ref().map(|(l, _)| l.as_str()).unwrap_or(""));
    put("reportUrl", report_url);
    put("copyLabel", &qt_app::crash_button_copy());
    put("openLabel", &qt_app::crash_button_open());
    put("closeLabel", &qt_app::crash_button_close());
    put("showDetailsLabel", &qt_app::crash_details_show());
    put("hideDetailsLabel", &qt_app::crash_details_hide());
    put("copiedLabel", &qt_app::crash_copied());
    put("bg", &color("bg_primary"));
    put("bgField", &color("bg_secondary"));
    put("bgCard", &color("bg_card"));
    put("bgCardHover", &color("bg_card_hover"));
    put("accent", &color("accent"));
    put("accentGreen", &color("accent_green"));
    put("accentGreenHover", &color("accent_green_hover"));
    put("textColor", &color("text_primary"));
    put("borderColor", &color("border"));
    engine.as_mut().set_initial_properties(&props);

    let loaded = std::sync::Arc::new(AtomicBool::new(false));
    let seen = loaded.clone();
    let _guard = engine.as_mut().on_object_created(move |_engine, object, _url| {
        if !object.is_null() {
            seen.store(true, Ordering::SeqCst);
        }
    });
    engine
        .as_mut()
        .load_data(&QByteArray::from(DIALOG_QML), &QUrl::from(DIALOG_QML_URL));
    if !loaded.load(Ordering::SeqCst) {
        rexon_core::log_error!("crash", "окно о крахе: QML не загрузился");
        return WINDOW_LOAD_FAILED;
    }
    println!("{WINDOW_SHOWN_MARKER}");
    match app.as_mut() {
        Some(app) => app.exec(),
        None => WINDOW_LOAD_FAILED,
    }
}

/// The address under which the engine knows the window markup (for error
/// messages; the window has no relative imports).
const DIALOG_QML_URL: &str = "qrc:/rexon/crash/CrashDialog.qml";

/// Markup of the crash window. As a string, not as a file of the
/// `org.rexon.gui` module: the window needs none of our types, and the
/// product's static QML module in the window process is an extra point of
/// failure. Not a single human-readable string is typed here — all of them
/// come from the core as properties.
///
/// The Controls style is Basic explicitly: the KDE system style has its own
/// button look, and setting their background has no effect there.
const DIALOG_QML: &str = r##"
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

ApplicationWindow {
    id: root
    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true

    required property string dialogTitle
    required property string mainText
    required property string informative
    required property string detailed
    required property string folderUrl
    required property string reportLabel
    required property string reportUrl
    required property string copyLabel
    required property string openLabel
    required property string closeLabel
    required property string showDetailsLabel
    required property string hideDetailsLabel
    required property string copiedLabel
    required property color bg
    required property color bgField
    required property color bgCard
    required property color bgCardHover
    required property color accent
    required property color accentGreen
    required property color accentGreenHover
    required property color textColor
    required property color borderColor

    property bool detailsOpen: false
    property bool copied: false

    visible: true
    title: dialogTitle
    color: bg
    width: 640
    minimumWidth: 480
    height: content.implicitHeight + 48
    minimumHeight: content.implicitHeight + 48


    component DialogButton: Button {
        id: control
        property bool primary: false
        implicitHeight: 42
        leftPadding: 18
        rightPadding: 18
        font.pixelSize: 15
        contentItem: Text {
            text: control.text
            font: control.font
            color: root.textColor
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 3
            color: control.primary
                   ? (control.hovered ? root.accentGreenHover : root.accentGreen)
                   : (control.hovered ? root.bgCardHover : root.bgCard)
            border.width: control.activeFocus ? 2 : 1
            border.color: control.activeFocus ? root.accent : root.borderColor
        }
    }

    ColumnLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 24
        spacing: 14
        // Esc закрывает окно. Обработчик клавиш, а не Shortcut: Shortcut
        // срабатывает только в АКТИВНОМ окне, а окно о крахе появляется из
        // фонового процесса и активным бывает не всегда; клавиша же доходит
        // сюда по цепочке фокуса от кнопки.
        Keys.onEscapePressed: Qt.quit()

        RowLayout {
            spacing: 14
            Layout.fillWidth: true
            Rectangle {
                width: 40
                height: 40
                radius: 20
                color: "#c0392b"
                Text {
                    anchors.centerIn: parent
                    text: "!"
                    color: "#ffffff"
                    font.pixelSize: 24
                    font.bold: true
                }
            }
            Text {
                text: root.dialogTitle
                color: root.textColor
                font.pixelSize: 20
                font.bold: true
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
        }

        Text {
            text: root.mainText
            color: root.textColor
            font.pixelSize: 16
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        TextEdit {
            visible: root.informative !== ""
            text: root.informative
            color: root.textColor
            opacity: 0.85
            font.pixelSize: 15
            readOnly: true
            selectByMouse: true
            wrapMode: TextEdit.Wrap
            Layout.fillWidth: true
        }

        DialogButton {
            text: root.detailsOpen ? root.hideDetailsLabel : root.showDetailsLabel
            onClicked: root.detailsOpen = !root.detailsOpen
        }

        ScrollView {
            visible: root.detailsOpen
            Layout.fillWidth: true
            Layout.preferredHeight: 280
            TextArea {
                id: details
                text: root.detailed
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.NoWrap
                color: root.textColor
                font.family: "monospace"
                font.pixelSize: 12
                background: Rectangle {
                    color: root.bgField
                    border.color: root.borderColor
                }
            }
        }

        Text {
            visible: root.copied
            text: root.copiedLabel
            color: root.accent
            font.pixelSize: 15
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            Item { Layout.fillWidth: true }
            DialogButton {
                visible: root.reportUrl !== ""
                text: root.reportLabel
                onClicked: Qt.openUrlExternally(root.reportUrl)
            }
            DialogButton {
                id: copyButton
                primary: true
                text: root.copyLabel
                onClicked: {
                    details.selectAll();
                    details.copy();
                    details.deselect();
                    root.copied = true;
                }
            }
            DialogButton {
                visible: root.folderUrl !== ""
                text: root.openLabel
                onClicked: Qt.openUrlExternally(root.folderUrl)
            }
            DialogButton {
                text: root.closeLabel
                onClicked: Qt.quit()
            }
        }
    }

    Component.onCompleted: copyButton.forceActiveFocus()
}
"##;

// ============================================================================
// Identity guards
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// ⭐ The crash window is a separate process of the product, and it must
    /// present itself to the system under THE SAME machine name as the main
    /// window and the tray bench (`rexon_core::tray::APP_ID`; the analysis is
    /// at the constant itself). A live defect: the translated title of the main
    /// window ended up here, and one product came to the system under two
    /// names at once.
    ///
    /// The guard looks at the SOURCE, not at the value: the value always agrees
    /// with itself, while it is exactly the call that can drift.
    #[test]
    fn имя_для_системы_берётся_из_общей_двери_а_не_из_заголовка() {
        let src = include_str!("crash_report.rs");
        let call = src
            .lines()
            .map(str::trim)
            .find(|l| l.starts_with("app.set_application_name("))
            .expect("ступень «окно» называет себя Qt");
        assert!(
            call.contains("rexon_core::tray::APP_ID"),
            "имя для системы взято не из общей двери: {call}"
        );
        assert!(
            !call.contains("WINDOW_TITLE"),
            "в имя для системы снова уехал заголовок окна: {call}"
        );
        // The name itself is a machine name: one word in Latin letters, not a
        // label for a human.
        assert!(rexon_core::tray::APP_ID.is_ascii());
        assert!(!rexon_core::tray::APP_ID.contains(' '));
    }

    /// And the human still reads a human-readable string: the visible window
    /// title is the core's string for the customer and does not depend on the
    /// machine name.
    #[test]
    fn видимый_заголовок_окна_падения_остался_человеческим() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let dir = "/home/u/.config/rexon/crashes";
        let dialog =
            qt_app::crash_dialog_for_report("ОТЧЁТ", &format!("{dir}/crash_1.log"), dir, true);
        assert_eq!(dialog.title, qt_app::crash_dialog_title());
        assert_eq!(dialog.title, "Rexon — An Error Occurred");
        assert_ne!(dialog.title, rexon_core::tray::APP_ID);
        assert!(
            dialog.title.contains(' '),
            "заголовок для человека — фраза, а не машинное имя: {}",
            dialog.title
        );
        // The window title is taken from this property, not from the application
        // name.
        assert!(DIALOG_QML.contains("title: dialogTitle"));
        // The writing direction (`Qt.application.layoutDirection`) may be taken —
        // the application name may not.
        assert!(!DIALOG_QML.contains("Qt.application.name"));
        assert!(!DIALOG_QML.contains("Qt.application.displayName"));
    }
}
