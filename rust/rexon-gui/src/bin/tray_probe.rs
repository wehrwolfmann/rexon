// rexon-tray-probe — the control test bench for the tray icon of the QML
// window.
//
// WHY IT EXISTS. The tray icon, the `--tray` flag, "the close button hides
// the window" and "a second launch raises the first one's window" relied on
// one assumption of the shim (`cpp/rexon_tray.cpp`): the main window is a
// QWidget, and a QApplication lives in the process. For the Rust main
// window binary neither is true: the application is a QGuiApplication, the
// window is a QQuickWindow, and there is not a single widget. The bench
// brings up exactly the combination the product needs (QApplication from
// cxx-qt-lib-extras + QQmlApplicationEngine + qml/MainWindow.qml), sets up
// the icon and checks all four doors ON A LIVE SCREEN.
//
// WHY A SEPARATE BINARY AND NOT A MODE OF THE MAIN WINDOW. The product
// already sets up the icon by itself (`main_window.rs`), and there is no
// reason to repeat that here. The bench is needed for one thing the product
// must not do: build a process with a BROKEN mechanism (QGuiApplication
// instead of QApplication) and make sure there is then no icon, while the
// program does not crash. Dragging that branch into the product entry
// point would mean keeping a branch "and now build yourself wrongly" in it.
//
// Run with eyes (window + icon, lives until quitting from the icon menu):
//     rust/target/release/rexon-tray-probe
// Straight into the tray, without a window:
//     rust/target/release/rexon-tray-probe --tray
// Hands-free run (window on screen, answer in numbers, the exit code is the
// verdict):
//     rust/target/release/rexon-tray-probe --check
//
// THE BITE. A bench that goes green with a broken mechanism checks nothing.
// `REXON_TRAY_PROBE_BITE=guiapp` builds a QGuiApplication — exactly what
// there was before this work. The run must go RED on it (no icon) and must
// NOT CRASH: an attempt to create a QMenu without a QApplication kills the
// whole process, and protection against that is part of the work.

use cxx_qt_lib::{QQmlApplicationEngine, QString, QUrl};
use cxx_qt_lib_extras::QApplication;

// The link to the crate library is mandatory, not decorative: without it
// rustc does not pull in the rlib, and with it the cxx-qt static archives
// (models, moc, the QML plugin with resources) — the binary builds, but the
// QML is empty.
#[allow(unused_imports)]
use rexon_gui::main_window::qobject::MainWindowModel;

use rexon_core::single_instance;
use rexon_core::tray as core_tray;

/// How long to wait for QML to build the window before poking the doors.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(2500);

/// The section the run switches the window to from the "icon menu". The
/// second, not the first: the first is already open, and "switched" on it
/// is indistinguishable from "did nothing".
const CHECK_SECTION: i32 = 2;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let bite = std::env::var("REXON_TRAY_PROBE_BITE").unwrap_or_default();

    // The second-launch lock and socket are in a fake directory if it is set
    // through XDG_RUNTIME_DIR. The bench has no right to take them away from a
    // live Rexon: the product uses exactly the same ones.
    let runtime_dir = single_instance::runtime_dir_from_env();
    let sock = single_instance::socket_path(&runtime_dir);

    cxx_qt::init_qml_module!("org.rexon.gui");

    // Here it is, the whole resolution of the blocker: a QApplication in Rust.
    // Under the bite — a QGuiApplication, as before; then the icon must not
    // come up.
    let mut gui_app = None;
    let mut app = None;
    if bite == "guiapp" {
        eprintln!("УКУС: приложение — QGuiApplication, значка быть не должно");
        gui_app = Some(cxx_qt_lib::QGuiApplication::new());
    } else {
        app = Some(QApplication::new());
    }
    if let Some(app) = app.as_mut().and_then(|a| a.as_mut()) {
        app.set_organization_name(&QString::from(rexon_core::qt_app::ORGANIZATION_NAME));
    }
    if let Some(app) = app.as_mut().and_then(|a| a.as_mut()) {
        // The same machine name as the product's: Qt makes the icon's `Id` property
        // on the bus from it, and the bench must measure exactly the string that
        // will go to the customer (`rexon_core::tray::APP_ID`).
        app.set_application_name(&QString::from(core_tray::APP_ID));
    }
    cxx_qt_lib::QGuiApplication::set_desktop_file_name(&QString::from(
        rexon_core::qt_app::DESKTOP_FILE_NAME,
    ));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/MainWindow.qml"));
    }

    // Icon: the door takes the sections, the picture file and `--tray` from the
    // core by itself.
    let installed = rexon_gui::tray::install_for_main_window(&args);
    println!(
        "ЗНАЧОК: {}",
        if installed { "поставлен" } else { "нет" }
    );
    if core_tray::starts_hidden(&args) && !installed {
        // Asked to minimize, but there is no icon — the window is the only door
        // into the program and must not be hidden (rule #13, foolproofing).
        println!("⚠️ запуск свёрнутым отменён: значка в лотке нет");
    }

    // The second launch's "raise the window" listener. It comes up AFTER the
    // QML is loaded: before that there is nothing to raise, and the socket
    // would already be answering "ok" to emptiness.
    if let Err(e) =
        single_instance::ActivationListener::spawn(&sock, rexon_gui::tray::raise_main_window)
    {
        eprintln!("слушатель «подними окно» не встал ({sock}): {e}");
    }
    println!("СОКЕТ: {sock}");

    if check {
        std::thread::spawn(move || run_check(installed));
    }

    // The event loop's exit code is the run's verdict, and it must become the
    // process exit code.
    let code = if let Some(app) = app.as_mut() {
        app.as_mut().map(|a| a.exec()).unwrap_or(2)
    } else if let Some(gui) = gui_app.as_mut() {
        gui.as_mut().map(|a| a.exec()).unwrap_or(2)
    } else {
        2
    };
    let verdict = VERDICT.load(std::sync::atomic::Ordering::SeqCst);

    // Warning: TEAR DOWN THE SCENE BEFORE EXITING, and exactly in this order.
    // Leaving the event loop does not dismantle the built window: the QML
    // engine keeps holding the root object, and with it the tabs' live players
    // — and their decoding threads (QFFmpeg::Stream) keep running. If
    // `std::process::exit` is called after that, the exit handlers tear down
    // libc under a running thread, and Rexon crashes right at exit: caught
    // live on "Quit" from the icon menu after opening the "Steam" section with
    // the video splash (SIGSEGV in av_log). Dropping the engine destroys the
    // root object, which destroys the players, which stop their threads, and
    // only after that is exiting safe.
    drop(engine);
    drop(app);
    drop(gui_app);

    std::process::exit(if check { verdict.max(code) } else { code });
}

/// Run of the icon doors FROM ANOTHER THREAD — the way the second-launch
/// listener calls them. Prints every check and exits the process with the
/// verdict.
fn run_check(installed: bool) {
    std::thread::sleep(SETTLE);

    let mut failures: Vec<String> = Vec::new();
    let say = |name: &str, ok: bool, failures: &mut Vec<String>| {
        println!("{} {name}", if ok { "✅" } else { "❌" });
        if !ok {
            failures.push(name.to_string());
        }
    };

    say("значок стоит в лотке", installed, &mut failures);
    say(
        "окно поднято из чужого потока",
        rexon_gui::tray::raise_main_window(),
        &mut failures,
    );
    say(
        "переход к разделу из меню значка",
        rexon_gui::tray::select_section(CHECK_SECTION),
        &mut failures,
    );
    // Second launch: we knock on OUR OWN socket exactly the way a second
    // instance of the product does. The whole chain is checked, not "the
    // function returned true".
    let sock = single_instance::socket_path(&single_instance::runtime_dir_from_env());
    say(
        "второй запуск поднял окно через сокет",
        single_instance::activate_running_instance(&sock),
        &mut failures,
    );

    if failures.is_empty() {
        println!("ПРОГОН: все проверки прошли");
    } else {
        println!("ПРОГОН ПРОВАЛЕН: {}", failures.join(", "));
        VERDICT.store(1, std::sync::atomic::Ordering::SeqCst);
    }

    // We exit THROUGH Qt, not with `std::process::exit` from this thread:
    // exiting from another thread runs the exit handlers concurrently with the
    // live event loop, and the run left a core dump behind — the verdict
    // printed was correct, but the process still crashed. The verdict is
    // carried out by the main thread when the event loop returns control.
    rexon_gui::tray::quit();
}

/// Run verdict: 0 — passed, 1 — failed.
static VERDICT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
