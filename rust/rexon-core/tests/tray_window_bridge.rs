// Сторож моста «значок в лотке ↔ главное окно».
//
// ЧТО ИМЕННО ОХРАНЯЕТСЯ. Значок, флаг `--tray`, «крестик прячет окно» и
// «второй запуск поднимает окно первого» держались на трёх допущениях шима
// (`rust/rexon-gui/cpp/rexon_tray.cpp`), и все три были неверны для процесса
// с окном на QML:
//
//   1. главное окно — QWidget, его дают `QApplication::topLevelWidgets()`;
//   2. «объект приложения есть» = «QApplication есть» (а виджеты Qt без
//      QApplication не создаёт вовсе и гасит процесс);
//   3. окно трогают только из потока GUI, поэтому подъём окна слушателем
//      второго запуска невозможен в принципе.
//
// Проверять это прогоном нельзя: ядро собирается без Qt, а живой лоток и
// живое окно есть только на экране (там их проверяет стенд
// `rexon-tray-probe`). Здесь сторож читает ИСХОДНИК шима и кусается, если
// любое из трёх допущений вернётся — молча и незаметно, как оно и жило.
//
// ⚠️ ПРОВЕРКА НА УКУС у самого сторожа — сделана, а не обещана. Каждый раз
// правка вносилась в дерево, сторож обязан был покраснеть, после чего правка
// откатывалась:
//   • `topLevelWindows` → `topLevelWidgets` в шиме — покраснел;
//   • `use cxx_qt_lib_extras as _;` в rexon-gui/src/lib.rs закомментирован —
//     покраснел;
//   • `drop(engine)` перед выходом закомментирован — покраснел;
//   • `QApplication::new()` → `QGuiApplication::new()` в главном окне —
//     покраснел.
// Первая же попытка сторожа была ЛОЖНО зелёной: проверки стояли на
// `contains` по всему файлу, а закомментированная строка подстроку содержит
// — сторож ловил только удаление, но не отключение кода. Отсюда
// `has_live_line` ниже.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("не нашёл корень репозитория")
}

/// Есть ли в тексте ЖИВАЯ строка, начинающаяся с этого куска.
///
/// Построчно и с отсечкой комментария нарочно: `contains` по всему файлу
/// зеленел и на закомментированной строке — сторож с такой проверкой ловил
/// только удаление кода, а не его отключение, то есть самый частый способ
/// «на минутку выключу и забуду».
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

/// Окно на QML шим обязан искать среди окон, а не среди виджетов.
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
    // Всплывающие окна отсеиваются: без этого крестиком однажды спрячется
    // выпадающий список вместо главного окна.
    assert!(
        src.contains("Qt::Popup") && src.contains("transientParent"),
        "шим перестал отсеивать всплывающие окна при поиске главного"
    );
}

/// Виджетный путь обязан спрашивать НАСТОЯЩИЙ QApplication.
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

/// Двери окна обязаны переноситься в поток GUI сами.
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

/// QApplication на Rust берётся из cxx-qt-lib-extras — без этой зависимости
/// у бинаря главного окна значка быть не может.
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

/// Ссылка на крейт QApplication в корне rexon-gui — та строка, без которой
/// не линкуется ВЕСЬ пакет, включая чужие бинари.
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

/// Главное окно обязано строить QApplication и снимать сцену до выхода.
#[test]
fn главное_окно_строит_qapplication_и_снимает_сцену() {
    let path = repo_root().join("rust/rexon-gui/src/bin/main_window.rs");
    let src = std::fs::read_to_string(&path).expect("не прочитал main_window.rs");
    assert!(
        has_live_line(&src, "let mut app = QApplication::new();"),
        "главное окно снова строит QGuiApplication — значка в лотке у него \
         тогда нет вовсе (QMenu без QApplication не создаётся)"
    );
    // Роспуск обязан стоять ДО выхода из процесса, а не «где-нибудь в файле».
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

/// Раздел из меню значка переключается и у окна на QML.
#[test]
fn переход_к_разделу_умеет_полосу_на_qml() {
    let src = shim_source();
    assert!(
        src.contains("QQuickTabBar"),
        "переход к разделу снова умеет только виджетную полосу вкладок — в \
         окне на QML пункты подменю «Разделы» перестанут работать"
    );
}
