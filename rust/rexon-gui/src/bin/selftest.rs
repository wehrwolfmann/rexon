// Стенды самопроверки крейта rexon-gui: живые QObject'ы и QML без окна
// продукта и без Python. Один бинарь на все сценарии — каждый выбирается
// своим аргументом, разбор сценария живёт в модуле крейта.
//
// Выдача Discover (модель и поздние колбэки на удалённую модель):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --remote-selftest
// Запросы Discover (поиск, страницы, фильтры, сортировки):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --discover-selftest
// Страница «Обои» (список, семь запросов-действий и модель библиотеки):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --wallpaper-selftest
// 64-битное целое в QML против мостов:
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe
// Живые модели в настоящем QML (qml/LiveModelsProbe.qml) — сетка Discover
// (modelReset/rowsInserted при подаче и догрузке страниц, перепривязка
// карточки), редактор свойств обоев и гашение плашки состояния:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --remote-model-signals-selftest
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --user-property-selftest
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --toast-selftest
// Крупный просмотр картинки (настоящие щелчки и клавиши; нужен QML-модуль
// QtTest из qt6-declarative):
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --image-viewer-selftest
// Вкладка Steam, «Steam-фоны»: фильтр «все / статичные / анимированные»,
// отметка на миниатюре и в просмотре, строка «анимированные не показаны» с
// кнопкой «Повторить». Steam подставной (свой HOME), сеть заглушена:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --steam-backgrounds-selftest
// Вкладка Steam, «Витрины профиля»: помощник загрузки — возврат к набору,
// «Скопировать название», отметки шагов, проверка профиля при глухой сети:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --steam-showcase-wizard-selftest
// Все безоконные сценарии подряд гоняет `cargo test -p rexon-gui`
// (tests/selftest_scenarios.rs).
//
// Отдельный сценарий — стенд слоя обоев (нужна ЖИВАЯ сессия Wayland, под
// offscreen смысла не имеет):
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --layer-shell-selftest
// Он запускает этот же бинарь с --layer-shell-probe под WAYLAND_DEBUG=1 и
// выносит приговор по протокольному следу. Разбор — src/layer_shell_selftest.rs.
// Проверка стенда на укус (стенд, который зеленеет всегда, ничего не
// проверяет): `--layer-shell-bite` (все виды подряд) либо
// `--layer-shell-bite=<вид>` — прогон с испорченным слоем ОБЯЗАН покраснеть.

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString, QUrl,
    QVariant,
};

// Ссылка на библиотеку крейта обязательна, а не декоративна: без неё rustc
// не подтягивает rlib, а вместе с ним и статические архивы cxx-qt (модели,
// moc, QML-плагин с ресурсами) — бинарь собирается, но QML пуст.
use rexon_gui::layer_shell_selftest::{BITE_ARG, BITE_ENV, PROBE_ARG, SELFTEST_ARG};

/// Сценарии `qml/LiveModelsProbe.qml`: ключ командной строки → значение
/// свойства `scenario` корня документа.
const LIVE_MODEL_SCENARIOS: &[(&str, &str)] = &[
    ("--remote-model-signals-selftest", "remote"),
    ("--user-property-selftest", "userprop"),
    ("--toast-selftest", "toast"),
    ("--image-viewer-selftest", "viewer"),
    ("--store-files-selftest", "storefiles"),
    (STEAM_BG_ARG, "steambg"),
    (STEAM_WIZARD_ARG, "steamwiz"),
];

/// Сценарий помощника загрузки витрин на том же подставном Steam.
const STEAM_WIZARD_ARG: &str = "--steam-showcase-wizard-selftest";

/// Сценарий сетки фонов вкладки Steam на подставном Steam.
const STEAM_BG_ARG: &str = "--steam-backgrounds-selftest";

fn main() {
    let has = |arg: &str| std::env::args().any(|a| a == arg);
    // Стенд выдачи Discover: живой QObject без окна и без QML. Прогоняет
    // все случаи голдена и отдельно — поздние колбэки на удалённую модель.
    // Разбор — src/remote_list_model.rs.
    if has(rexon_gui::remote_list_model::selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::remote_list_model::selftest::run());
    }
    // Стенд запросов Discover: живые объекты-владельцы против голдена.
    // Разбор — src/discover_queries.rs.
    if has(rexon_gui::discover_queries::selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::discover_queries::selftest::run());
    }
    // Стенд страницы «Обои»: восемь живых запросов (список + семь действий)
    // и модель библиотеки против голдена. Разбор — src/wallpaper_page_selftest.rs.
    if has(rexon_gui::wallpaper_page_selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::wallpaper_page_selftest::run());
    }
    // Проба «64-битное целое в QML»: живой движок QML против мостов.
    // Разбор — src/qml_int64_probe.rs. Проба сама зовёт `std::process::exit`.
    if has(rexon_gui::qml_int64_probe::ARG) {
        rexon_gui::qml_int64_probe::run();
    }
    // Живые модели в настоящем QML: то, чего безоконные стенды выше не
    // видят (структурные сигналы модели и перепривязка живых делегатов).
    if has(STEAM_BG_ARG) || has(STEAM_WIZARD_ARG) {
        if let Err(why) = prepare_fake_steam() {
            eprintln!("подставной Steam не собран: {why}");
            std::process::exit(2);
        }
    }
    if let Some((_, scenario)) = LIVE_MODEL_SCENARIOS.iter().find(|(arg, _)| has(arg)) {
        std::process::exit(run_live_models_probe(scenario));
    }
    // Стенд слоя обоев: разбор до создания QGuiApplication, чтобы
    // родительская половина стенда вообще не трогала Qt (ей нужен только
    // процесс-ребёнок и его протокольный след).
    if has(SELFTEST_ARG) {
        std::process::exit(rexon_gui::layer_shell_selftest::run());
    }
    if let Some(arg) = std::env::args().find(|a| a.starts_with(BITE_ARG)) {
        let kind = arg.split_once('=').map(|(_, v)| v.to_string()).unwrap_or_default();
        std::process::exit(rexon_gui::layer_shell_selftest::run_bite(&kind));
    }
    if has(PROBE_ARG) {
        std::process::exit(run_layer_shell_probe());
    }
    let live: Vec<&str> = LIVE_MODEL_SCENARIOS.iter().map(|(arg, _)| *arg).collect();
    eprintln!(
        "не указан сценарий: {} | {} | {} | {} | {} | {} | {}[=вид]",
        rexon_gui::remote_list_model::selftest::ARG,
        rexon_gui::discover_queries::selftest::ARG,
        rexon_gui::wallpaper_page_selftest::ARG,
        rexon_gui::qml_int64_probe::ARG,
        live.join(" | "),
        SELFTEST_ARG,
        BITE_ARG,
    );
    std::process::exit(2);
}

/// Подставной Steam для сценария сетки фонов: каталог Steam с одной учётной
/// записью и дисковый кэш списка из пяти фонов (два анимированных — один
/// роликом от Steam, второй по адресу самого фона).
///
/// ⚠️ Только в СВОЁМ домашнем каталоге: сценарий пишет в `$HOME`, и на
/// настоящем он лёг бы поверх живого Steam и кэша продукта. Бегун
/// (`tests/selftest_scenarios.rs`) даёт каталог `rexon-selftest-…`; без него
/// — отказ.
///
/// Сеть заглушена прокси на закрытый порт: вкладка честно пойдёт за списком
/// и миниатюрами, получит мгновенный отказ и оставит список из кэша — а
/// прогон не зависит ни от связи, ни от ответа Steam. Входа в Steam у
/// подставного каталога нет, и это тоже часть сценария: строка «анимированные
/// не показаны» обязана назвать причину.
fn prepare_fake_steam() -> Result<(), String> {
    use rexon_core::qt_steam as qs;
    use rexon_core::steam_profile_items::ProfileBackgroundItem;

    let home = std::env::var("HOME").unwrap_or_default();
    let leaf = std::path::Path::new(&home)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !leaf.starts_with("rexon-selftest-") {
        return Err(format!(
            "HOME=«{home}» — не каталог прогона (ждём rexon-selftest-…); в настоящий дом сценарий не пишет"
        ));
    }
    for name in ["http_proxy", "https_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        std::env::set_var(name, "http://127.0.0.1:9");
    }
    std::env::remove_var("XDG_CACHE_HOME");
    std::env::remove_var("XDG_CONFIG_HOME");

    const ACCOUNT: &str = "40000001";
    let root = format!("{home}/.local/share/Steam");
    let account_cfg = format!("{root}/userdata/{ACCOUNT}/config");
    std::fs::create_dir_all(format!("{root}/config")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&account_cfg).map_err(|e| e.to_string())?;
    std::fs::write(format!("{account_cfg}/localconfig.vdf"), "\"UserLocalConfigStore\"\n{\n}\n")
        .map_err(|e| e.to_string())?;
    let sid = qs::steamid64_from_accountid(ACCOUNT).ok_or("номер учётной записи не разобран")?;

    let item = |id: &str, name: &str, game: &str, url: &str, animated: bool| ProfileBackgroundItem {
        id: id.to_string(),
        name: name.to_string(),
        game: game.to_string(),
        url: format!("http://127.0.0.1:9/{url}"),
        preview: format!("http://127.0.0.1:9/preview-{id}.jpg"),
        animated,
        equipped: false,
    };
    let items = [
        item("1", "Night", "Portal 2", "night.jpg", false),
        item("2", "Rain", "Portal 2", "rain.webm", true),
        item("3", "Day", "Half-Life", "day.jpg", false),
        item("4", "Snow", "Half-Life", "snow.webm", false),
        item("5", "Sunset", "Half-Life", "sunset.jpg", false),
    ];
    let cache = qs::inventory_cache_path(None, &home, &sid);
    if let Some(dir) = std::path::Path::new(&cache).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&cache, qs::inventory_cache_json(&sid, &items, 0)).map_err(|e| e.to_string())?;

    // Набор для помощника загрузки витрин: две части, ни одна не загружена.
    // Самих картинок нет — помощнику для шагов они не нужны.
    use rexon_core::steam_showcase as sc;
    let set = sc::output_root(&sc::pictures_dir(None, None, &home)).join("probe-set");
    std::fs::create_dir_all(&set).map_err(|e| e.to_string())?;
    let part = |file: &str, role: &str, width: u32, height: u32| sc::WizardPart {
        file_name: file.to_string(),
        role: role.to_string(),
        width,
        height,
        uploaded: false,
    };
    sc::save_wizard_state(
        &set,
        &sc::WizardState {
            kind: sc::ShowcaseKind::Artwork.key().to_string(),
            parts: vec![part("1-main.png", "main", 506, 900), part("2-side.png", "side", 100, 830)],
            placed: false,
            verified: false,
        },
    )
}

/// Сценарий живых моделей: `qml/LiveModelsProbe.qml` с выбранным
/// `scenario`. Приговор выносит сам QML (`Qt.exit(0|1)`), код цикла событий
/// становится кодом процесса; 2 — документ не отчитался вовсе.
fn run_live_models_probe(scenario: &str) -> i32 {
    // Без этого прогон из скрипта немой: когда stderr не терминал, Qt на
    // Arch/CachyOS уводит console.log в journald.
    std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
    // Окну нужен цикл отрисовки (делегаты ListView/GridView создаются в
    // нём), но на живой стол ему незачем: платформу, если её не задали,
    // ставим offscreen сами.
    if std::env::var_os("QT_QPA_PLATFORM").is_none() {
        std::env::set_var("QT_QPA_PLATFORM", "offscreen");
    }
    cxx_qt::init_qml_module!("org.rexon.gui");

    // Страховка на случай, когда QML не загрузился и некому сказать
    // Qt.exit: молчание обязано стать провалом, а не тихим нулём. У самого
    // документа свой сторож на 20 с — этот длиннее.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(40));
        eprintln!("сценарий живых моделей не отчитался за 40 с — самопроверка провалена");
        std::process::exit(2);
    });

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        let mut props = QMap::<QMapPair_QString_QVariant>::default();
        props.insert(QString::from("scenario"), QVariant::from(&QString::from(scenario)));
        // Строки блока выбора файла — настоящий ответ ядра, а не слепок в
        // QML: стенд сверяет разметку с тем, что ядро отдаёт на самом деле.
        props.insert(
            QString::from("storeFilesJson"),
            QVariant::from(&QString::from(&store_files_fixture(true))),
        );
        props.insert(
            QString::from("storeFilesSingleJson"),
            QVariant::from(&QString::from(&store_files_fixture(false))),
        );
        engine.as_mut().set_initial_properties(&props);
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/LiveModelsProbe.qml"));
    }
    match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    }
}

/// Товар магазина для сценария «storefiles» (живой случай McOS 17.10.2026:
/// архив дополнений идёт первым по алфавиту, ставиться должна тема). `two` —
/// два файла; иначе один, и блока выбора быть не должно.
fn store_files_fixture(two: bool) -> String {
    use serde_json::json;
    let mut links = vec![json!({
        "url": "https://f/McOS-MJV-Gnome-3.30-1.1.tar.xz",
        "name": "McOS-MJV-Gnome-3.30-1.1.tar.xz",
        "size": 1419,
    })];
    if two {
        links.insert(0, json!({ "url": "https://f/Extra 1.4.tar.xz", "name": "Extra 1.4.tar.xz", "size": 13 }));
    }
    let item = rexon_core::models::ThemeItem {
        id: "1".into(),
        name: "McOS-MJV-Gnome-3.30".into(),
        download_links: links,
        ..Default::default()
    };
    rexon_core::appearance_page::store_file_choices_json(&item)
}

/// Подопытное окно стенда слоя обоев: QML-окно, которому слой ставит Rust.
///
/// Окно живёт на ЖИВОМ рабочем столе, поэтому у прогона два независимых
/// ограничителя: этот сторож и убийца процесса на стороне родителя. Даже
/// если QML зависнет насмерть, окно исчезнет — вместе с процессом.
fn run_layer_shell_probe() -> i32 {
    cxx_qt::init_qml_module!("org.rexon.gui");

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(15));
        eprintln!("подопытное окно не отчиталось за 15 с — снимаю его с экрана");
        std::process::exit(3);
    });

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_name(&QString::from("Rexon (проверка слоя обоев)"));
    }

    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        // Вид укуса едет в QML начальным свойством корня, а не переменной
        // контекста: контекстные свойства в cxx-qt-lib 0.9 не обёрнуты, а
        // setInitialProperties есть и работает ДО первой привязки.
        let bite = std::env::var(BITE_ENV).unwrap_or_default();
        let mut props = QMap::<QMapPair_QString_QVariant>::default();
        props.insert(QString::from("bite"), QVariant::from(&QString::from(&bite)));
        engine.as_mut().set_initial_properties(&props);
        engine.load(&QUrl::from(
            "qrc:/qt/qml/org/rexon/gui/qml/LayerShellProbe.qml",
        ));
    }

    match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    }
}
