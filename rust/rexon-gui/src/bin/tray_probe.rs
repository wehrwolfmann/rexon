// rexon-tray-probe — контрольный стенд значка в лотке у окна на QML.
//
// ЗАЧЕМ ОН ЕСТЬ. Значок в лотке, флаг `--tray`, «крестик прячет окно» и
// «второй запуск поднимает окно первого» держались на одном допущении шима
// (`cpp/rexon_tray.cpp`): главное окно — это QWidget, а в процессе живёт
// QApplication. У бинаря главного окна на Rust не верно ни то, ни другое:
// приложение — QGuiApplication, окно — QQuickWindow, виджетов нет ни одного.
// Стенд поднимает ровно ту связку, что нужна продукту (QApplication из
// cxx-qt-lib-extras + QQmlApplicationEngine + qml/MainWindow.qml), ставит
// значок и проверяет все четыре двери НА ЖИВОМ ЭКРАНЕ.
//
// ПОЧЕМУ ОТДЕЛЬНЫЙ БИНАРЬ, А НЕ РЕЖИМ ГЛАВНОГО ОКНА. Продукт значок уже
// ставит сам (`main_window.rs`), и повторять это здесь незачем. Стенд нужен
// ради одного, чего продукту делать нельзя: собрать процесс СО СЛОМАННЫМ
// механизмом (QGuiApplication вместо QApplication) и убедиться, что значка
// тогда нет, а программа при этом не падает. Тащить эту развилку в
// продуктовую точку входа значило бы держать в ней ветку «а теперь соберись
// неправильно».
//
// Запуск глазами (окно + значок, живёт до выхода из меню значка):
//     rust/target/release/rexon-tray-probe
// Сразу в лоток, без окна:
//     rust/target/release/rexon-tray-probe --tray
// Прогон без рук (окно на экране, ответ числами, код возврата — приговор):
//     rust/target/release/rexon-tray-probe --check
//
// ⭐ УКУС. Стенд, который зеленеет и со сломанным механизмом, ничего не
// проверяет. `REXON_TRAY_PROBE_BITE=guiapp` строит QGuiApplication — ровно
// то, что было до этой работы. Прогон обязан на этом ПОКРАСНЕТЬ (значка нет)
// и обязан НЕ УПАСТЬ: попытка создать QMenu без QApplication гасит процесс
// целиком, и защита от этого — часть работы.

use cxx_qt_lib::{QQmlApplicationEngine, QString, QUrl};
use cxx_qt_lib_extras::QApplication;

// Ссылка на библиотеку крейта обязательна, а не декоративна: без неё rustc
// не подтягивает rlib, а вместе с ним статические архивы cxx-qt (модели,
// moc, QML-плагин с ресурсами) — бинарь собирается, но QML пуст.
#[allow(unused_imports)]
use rexon_gui::main_window::qobject::MainWindowModel;

use rexon_core::single_instance;
use rexon_core::tray as core_tray;

/// Сколько ждать, пока QML построит окно, прежде чем дёргать двери.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(2500);

/// Раздел, на который прогон переключает окно из «меню значка». Второй, а не
/// первый: первый уже открыт, и «переключили» на нём неотличимо от «ничего
/// не сделали».
const CHECK_SECTION: i32 = 2;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let bite = std::env::var("REXON_TRAY_PROBE_BITE").unwrap_or_default();

    // Замок и сокет второго запуска — в подставном каталоге, если он задан
    // через XDG_RUNTIME_DIR. Стенд не имеет права отбирать их у живого
    // Rexon: у продукта они те же самые.
    let runtime_dir = single_instance::runtime_dir_from_env();
    let sock = single_instance::socket_path(&runtime_dir);

    cxx_qt::init_qml_module!("org.rexon.gui");

    // ⭐ Вот она, вся развязка блокера: QApplication на Rust. Под укусом —
    // QGuiApplication, как было; значок тогда встать не должен.
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
        // То же машинное имя, что у продукта: из него Qt делает свойство `Id`
        // значка на шине, и стенд обязан мерить ровно ту строку, которая
        // уедет к покупателю (`rexon_core::tray::APP_ID`).
        app.set_application_name(&QString::from(core_tray::APP_ID));
    }
    cxx_qt_lib::QGuiApplication::set_desktop_file_name(&QString::from(
        rexon_core::qt_app::DESKTOP_FILE_NAME,
    ));

    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/MainWindow.qml"));
    }

    // Значок: разделы, файл картинки и `--tray` дверь берёт из ядра сама.
    let installed = rexon_gui::tray::install_for_main_window(&args);
    println!(
        "ЗНАЧОК: {}",
        if installed { "поставлен" } else { "нет" }
    );
    if core_tray::starts_hidden(&args) && !installed {
        // Просили свернуться, но значка нет — окно единственная дверь в
        // программу, прятать его нельзя (правило #13, защита от дурака).
        println!("⚠️ запуск свёрнутым отменён: значка в лотке нет");
    }

    // Слушатель «подними окно» второго запуска. Встаёт ПОСЛЕ загрузки QML:
    // до неё поднимать нечего, а сокет уже отвечал бы «ok» на пустоту.
    if let Err(e) =
        single_instance::ActivationListener::spawn(&sock, rexon_gui::tray::raise_main_window)
    {
        eprintln!("слушатель «подними окно» не встал ({sock}): {e}");
    }
    println!("СОКЕТ: {sock}");

    if check {
        std::thread::spawn(move || run_check(installed));
    }

    // Код возврата цикла событий — это приговор прогона, и он обязан стать
    // кодом возврата процесса.
    let code = if let Some(app) = app.as_mut() {
        app.as_mut().map(|a| a.exec()).unwrap_or(2)
    } else if let Some(gui) = gui_app.as_mut() {
        gui.as_mut().map(|a| a.exec()).unwrap_or(2)
    } else {
        2
    };
    let verdict = VERDICT.load(std::sync::atomic::Ordering::SeqCst);

    // ⚠️ СНЯТЬ СЦЕНУ ДО ВЫХОДА, и именно в этом порядке. Выход из цикла
    // событий не разбирает построенное окно: движок QML продолжает держать
    // корневой объект, а вместе с ним живые проигрыватели вкладок — и их
    // потоки декодирования (QFFmpeg::Stream) продолжают работать. Если
    // после этого позвать `std::process::exit`, обработчики завершения
    // разбирают libc под работающим потоком, и Rexon падает на самом
    // выходе: пойман живьём на «Выходе» из меню значка после открытия
    // раздела «Steam» с видеозаставкой (SIGSEGV в av_log). Роспуск движка
    // сносит корневой объект, тот — проигрыватели, те — свои потоки, и
    // только после этого выходить безопасно.
    drop(engine);
    drop(app);
    drop(gui_app);

    std::process::exit(if check { verdict.max(code) } else { code });
}

/// Прогон дверей значка ИЗ ЧУЖОГО ПОТОКА — как их зовёт слушатель второго
/// запуска. Печатает каждую проверку и выходит из процесса приговором.
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
    // Второй запуск: стучимся в СВОЙ же сокет ровно так, как это делает
    // второй экземпляр продукта. Проверяется вся цепочка целиком, а не
    // «функция вернула true».
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

    // Гасимся ЧЕРЕЗ Qt, а не `std::process::exit` из этого потока: выход из
    // чужого потока прокручивает обработчики завершения одновременно с живым
    // циклом событий, и прогон оставлял после себя дамп памяти — приговор
    // печатался верный, а процесс всё равно падал. Приговор уносит с собой
    // главный поток, когда цикл событий вернёт управление.
    rexon_gui::tray::quit();
}

/// Приговор прогона: 0 — прошёл, 1 — провалился.
static VERDICT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
