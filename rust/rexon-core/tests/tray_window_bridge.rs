// Guard of the «tray icon ↔ main window» bridge.
//
// WHAT EXACTLY IS GUARDED. The icon, the `--tray` flag, «the close button
// hides the window» and «a second launch raises the first one's window»
// rested on three assumptions of the shim
// (`rust/rexon-gui/cpp/rexon_tray.cpp`), and all three were wrong for a
// process with a QML window:
//
//   1. the main window is a QWidget, given by `QApplication::topLevelWidgets()`;
//   2. «an application object exists» = «a QApplication exists» (and Qt does
//      not create widgets at all without a QApplication and kills the process);
//   3. the window is touched only from the GUI thread, so raising the window
//      from the second-launch listener is impossible in principle.
//
// This cannot be checked by a run: the core builds without Qt, and a live
// tray and a live window exist only on screen (there the `rexon-tray-probe`
// stand checks them). Here the guard reads the shim SOURCE and bites if any
// of the three assumptions returns — silently and unnoticed, as it lived.
//
// ⚠️ The BITE TEST of the guard itself was done, not promised. Each time a
// change was made in the tree, the guard had to go red, after which the
// change was reverted:
//   • `topLevelWindows` → `topLevelWidgets` in the shim — went red;
//   • `use cxx_qt_lib_extras as _;` in rexon-gui/src/lib.rs commented out —
//     went red;
//   • `drop(engine)` before exit commented out — went red;
//   • `QApplication::new()` → `QGuiApplication::new()` in the main window —
//     went red.
// The guard's very first attempt was FALSELY green: the checks used
// `contains` over the whole file, and a commented-out line contains the
// substring — the guard caught only removal, not disabling of code. Hence
// `has_live_line` below.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("не нашёл корень репозитория")
}

/// Whether the text has a LIVE line starting with this piece.
///
/// Line by line and with the comment cut off on purpose: `contains` over the
/// whole file went green on a commented-out line too — a guard with such a
/// check caught only code removal, not its disabling, i.e. the most common
/// way of «I'll turn it off for a minute and forget».
fn has_live_line(text: &str, needle: &str) -> bool {
    text.lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with("//"))
        .any(|line| line.starts_with(needle))
}

fn shim_source() -> String {
    let path = repo_root().join("rust/rexon-gui/cpp/rexon_tray.cpp");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("не прочитал шим значка ({}): {e}", path.display()))
}

/// The shim must look for a QML window among windows, not among widgets.
#[test]
fn шим_ищет_окно_среди_окон_а_не_только_виджетов() {
    let src = shim_source();
    assert!(
        src.contains("QGuiApplication::topLevelWindows()")
            && has_live_line(&src, "const auto windows = QGuiApplication::topLevelWindows();"),
        "шим больше не ищет главное окно среди QWindow — у процесса с окном \
         на QML виджетов нет ни одного, и значок снова останется без окна"
    );
    assert!(
        src.contains("QPointer<QWindow> g_qwindow"),
        "шим не держит найденное окно на QML"
    );
    // Popup windows are filtered out: without this, the close button would one
    // day hide a drop-down list instead of the main window.
    assert!(
        src.contains("Qt::Popup") && src.contains("transientParent"),
        "шим перестал отсеивать всплывающие окна при поиске главного"
    );
}

/// The widget path must ask the REAL QApplication.
#[test]
fn шим_не_создаёт_виджеты_без_qapplication() {
    let src = shim_source();
    assert!(
        src.contains("qobject_cast<QApplication *>(QCoreApplication::instance())"),
        "шим перестал проверять, что в процессе именно QApplication: QMenu \
         без него не создаётся, и попытка гасит процесс целиком"
    );
    assert!(
        !src.contains("if (QApplication::instance() == nullptr)"),
        "вернулась проверка «объект приложения есть»: она проходит и в \
         процессе с QGuiApplication, после чего значок роняет программу"
    );
}

/// The window's doors must move into the GUI thread by themselves.
#[test]
fn двери_окна_переносятся_в_поток_gui() {
    let src = shim_source();
    assert!(
        has_live_line(&src, "bool on_gui_thread("),
        "из шима ушёл перенос работы в поток GUI — без него слушатель второго \
         запуска поднять окно не может, и подъём снова станет заглушкой"
    );
    assert!(
        src.contains("Qt::BlockingQueuedConnection"),
        "перенос перестал ждать ответа: второй запуск начнёт врать человеку \
         «поднял окно» при пустом экране"
    );
    for door in [
        "bool rexon_tray_guard_window()\n{\n    return on_gui_thread(guard_window_here);",
        "bool rexon_tray_raise_window()\n{\n    return on_gui_thread(raise_window_here);",
        "bool rexon_tray_hide_window()\n{\n    return on_gui_thread(hide_window_here);",
    ] {
        assert!(
            src.contains(door),
            "дверь окна зовётся напрямую, мимо потока GUI:\n{door}"
        );
    }
}

/// QApplication in Rust comes from cxx-qt-lib-extras — without this dependency
/// the main window binary cannot have an icon.
#[test]
fn крейт_gui_объявляет_qapplication_на_rust() {
    let path = repo_root().join("rust/rexon-gui/Cargo.toml");
    let manifest = std::fs::read_to_string(&path).expect("не прочитал манифест rexon-gui");
    assert!(
        manifest.contains("cxx-qt-lib-extras"),
        "из rexon-gui ушла cxx-qt-lib-extras: QApplication на Rust больше \
         взять негде, а без неё значок в лотке не создаётся вовсе"
    );
}

/// The reference to the QApplication crate in the rexon-gui root — the line
/// without which the WHOLE package fails to link, including other binaries.
#[test]
fn крейт_gui_ссылается_на_qapplication_из_кода() {
    let path = repo_root().join("rust/rexon-gui/src/lib.rs");
    let lib = std::fs::read_to_string(&path).expect("не прочитал src/lib.rs крейта rexon-gui");
    assert!(
        has_live_line(&lib, "use cxx_qt_lib_extras as _;"),
        "из rexon-gui/src/lib.rs ушла ссылка на cxx-qt-lib-extras. Одной \
         зависимости в манифесте НЕ достаточно: статический архив крейта \
         линкуется только к артефактам, чей Rust-код на него ссылается, и \
         без этой строки падает линковка всех бинарей пакета \
         («undefined symbol: cxx_qt_init_crate_cxx_qt_lib_extras»), причём \
         на `cargo build --lib` поломки не видно"
    );
}

/// The main window must build a QApplication and tear down the scene before exit.
#[test]
fn главное_окно_строит_qapplication_и_снимает_сцену() {
    let path = repo_root().join("rust/rexon-gui/src/bin/main_window.rs");
    let src = std::fs::read_to_string(&path).expect("не прочитал main_window.rs");
    assert!(
        has_live_line(&src, "let mut app = QApplication::new();"),
        "главное окно снова строит QGuiApplication — значка в лотке у него \
         тогда нет вовсе (QMenu без QApplication не создаётся)"
    );
    // The teardown must come BEFORE the process exits, not «somewhere in the file».
    let lines: Vec<&str> = src.lines().map(str::trim_start).collect();
    let live = |needle: &str| {
        lines
            .iter()
            .position(|line| !line.starts_with("//") && line.starts_with(needle))
    };
    let exit = live("std::process::exit(code)")
        .expect("в main_window.rs нет выхода с кодом цикла событий");
    assert!(
        live("drop(engine)").is_some_and(|d| d < exit),
        "перед выходом из процесса больше не распускается движок QML: пока \
         сцена жива, её потоки отрисовки и мультимедиа работают, и выход \
         роняет Rexon на самом выходе (пойман SIGSEGV в av_log)"
    );
    assert!(
        has_live_line(&src, "let tray = rexon_gui::tray::install_for_main_window"),
        "главное окно перестало ставить значок в лоток"
    );
    assert!(
        has_live_line(&src, "rexon_gui::tray::raise_main_window()"),
        "подъём окна для второго запуска снова стал заглушкой"
    );
}

/// A section from the icon menu switches in the QML window too.
#[test]
fn переход_к_разделу_умеет_полосу_на_qml() {
    let src = shim_source();
    assert!(
        src.contains("QQuickTabBar"),
        "переход к разделу снова умеет только виджетную полосу вкладок — в \
         окне на QML пункты подменю «Разделы» перестанут работать"
    );
}
