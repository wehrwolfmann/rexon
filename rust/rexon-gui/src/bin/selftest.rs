// Self-test benches of the rexon-gui crate: live QObjects and QML without
// the product window and without Python. One binary for all scenarios —
// each is chosen by its own argument, and the scenario's analysis lives in
// the crate's module.
//
// Discover results (the model and late callbacks to a removed model):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --remote-selftest
// Discover queries (search, pages, filters, sorting):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --discover-selftest
// The "Wallpapers" page (the list, seven action queries and the library model):
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --wallpaper-selftest
// Removing the live wallpaper against a fake waywallen daemon:
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --we-stop-selftest
// A 64-bit integer in QML against the bridges:
//     QT_QPA_PLATFORM=offscreen cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe
// Live models in real QML (qml/LiveModelsProbe.qml) — the Discover grid
// (modelReset/rowsInserted on delivering and loading more pages, card
// rebinding), the wallpaper property editor and the status banner fading:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --remote-model-signals-selftest
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --user-property-selftest
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --toast-selftest
// Large picture view (real clicks and keys; needs the QtTest QML module from
// qt6-declarative):
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --image-viewer-selftest
// The Steam tab, "Steam backgrounds": the "all / static / animated" filter,
// the badge on the thumbnail and in the viewer, the "animated ones are not
// shown" line with the "Retry" button. Steam is fake (own HOME), the
// network is stubbed:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --steam-backgrounds-selftest
// The Steam tab, "Profile showcases": the upload helper — returning to a
// set, "Copy name", step marks, checking the profile with a dead network:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --steam-showcase-wizard-selftest
// All windowless scenarios in a row are run by `cargo test -p rexon-gui`
// (tests/selftest_scenarios.rs).
//
// A separate scenario is the wallpaper layer test bench (it needs a LIVE
// Wayland session; under offscreen it makes no sense):
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --layer-shell-selftest
// It starts this same binary with --layer-shell-probe under WAYLAND_DEBUG=1
// and gives the verdict from the protocol trace. Analysis —
// src/layer_shell_selftest.rs. Checking the bench with a bite (a bench that
// is always green checks nothing): `--layer-shell-bite` (all kinds in a
// row) or `--layer-shell-bite=<kind>` — a run with a spoiled layer MUST go
// red.
//
// The idle screensaver overlay bench (OVERLAY layer, exclusive keyboard, one
// window per screen, leaving on a key / a pointer move). ⛔ Only inside a
// nested compositor — the stand brings one up in an isolated environment and
// runs the bench there:
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --overlay-stand=labwc
//     cargo run -p rexon-gui --bin rexon-gui-selftest -- --overlay-stand=kwin --overlay-bite
// Analysis and the machine-readable verdict — `layer_shell_selftest::overlay`.

use cxx_qt_lib::{
    QGuiApplication, QMap, QMapPair_QString_QVariant, QQmlApplicationEngine, QString, QUrl,
    QVariant,
};

// The link to the crate library is mandatory, not decorative: without it
// rustc does not pull in the rlib, and with it the cxx-qt static archives
// (models, moc, the QML plugin with resources) — the binary builds, but the
// QML is empty.
use rexon_gui::layer_shell_selftest::{BITE_ARG, BITE_ENV, PROBE_ARG, SELFTEST_ARG};

/// `qml/LiveModelsProbe.qml` scenarios: command line option → value of the
/// document root's `scenario` property.
const LIVE_MODEL_SCENARIOS: &[(&str, &str)] = &[
    ("--remote-model-signals-selftest", "remote"),
    ("--user-property-selftest", "userprop"),
    ("--toast-selftest", "toast"),
    ("--image-viewer-selftest", "viewer"),
    ("--store-files-selftest", "storefiles"),
    (STEAM_BG_ARG, "steambg"),
    (STEAM_WIZARD_ARG, "steamwiz"),
];

/// Scenario of the showcase upload helper on the same fake Steam.
const STEAM_WIZARD_ARG: &str = "--steam-showcase-wizard-selftest";

/// Scenario of the Steam tab's background grid on a fake Steam.
const STEAM_BG_ARG: &str = "--steam-backgrounds-selftest";

fn main() {
    let has = |arg: &str| std::env::args().any(|a| a == arg);
    // Discover results bench: a live QObject without a window and without QML.
    // Runs all golden cases and, separately, late callbacks to a removed model.
    // Analysis — src/remote_list_model.rs.
    if has(rexon_gui::remote_list_model::selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::remote_list_model::selftest::run());
    }
    // Discover queries bench: live owner objects against the golden file.
    // Analysis — src/discover_queries.rs.
    if has(rexon_gui::discover_queries::selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::discover_queries::selftest::run());
    }
    // "Wallpapers" page bench: eight live queries (the list + seven actions)
    // and the library model against the golden file. Analysis —
    // src/wallpaper_page_selftest.rs.
    if has(rexon_gui::wallpaper_page_selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::wallpaper_page_selftest::run());
    }
    // Removing the live wallpaper against a fake daemon: the WebSocket
    // handshake race in a worker thread. Analysis — src/we_stop_selftest.rs.
    if has(rexon_gui::we_stop_selftest::ARG) {
        std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
        std::process::exit(rexon_gui::we_stop_selftest::run());
    }
    // The "64-bit integer in QML" probe: a live QML engine against the
    // bridges. Analysis — src/qml_int64_probe.rs. The probe calls
    // `std::process::exit` itself.
    if has(rexon_gui::qml_int64_probe::ARG) {
        rexon_gui::qml_int64_probe::run();
    }
    // Live models in real QML: what the windowless benches above cannot see
    // (structural model signals and rebinding of live delegates).
    if has(STEAM_BG_ARG) || has(STEAM_WIZARD_ARG) {
        if let Err(why) = prepare_fake_steam() {
            eprintln!("подставной Steam не собран: {why}");
            std::process::exit(2);
        }
    }
    if let Some((_, scenario)) = LIVE_MODEL_SCENARIOS.iter().find(|(arg, _)| has(arg)) {
        std::process::exit(run_live_models_probe(scenario));
    }
    // Wallpaper layer bench: parsing happens before QGuiApplication is
    // created, so that the parent half of the bench does not touch Qt at all
    // (it needs only the child process and its protocol trace).
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
    // Idle screensaver overlay bench: same two-process scheme; the stand
    // variant brings up its own nested compositor first.
    {
        use rexon_gui::layer_shell_selftest::overlay as ov;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let value = |prefix: &str| {
            args.iter().find(|a| a.as_str() == prefix || a.starts_with(&format!("{prefix}="))).map(|a| {
                a.split_once('=').map(|(_, v)| v.to_string()).unwrap_or_default()
            })
        };
        if let Some(name) = value(ov::STAND_ARG) {
            let rest: Vec<String> = args.iter().filter(|a| !a.starts_with(ov::STAND_ARG)).cloned().collect();
            std::process::exit(ov::run_stand(&name, &rest));
        }
        if let Some(spec) = value(ov::SELFTEST_ARG) {
            std::process::exit(ov::run(&spec));
        }
        if let Some(kind) = value(ov::BITE_ARG) {
            std::process::exit(ov::run_bite(&kind));
        }
        if has(ov::PROBE_ARG) {
            std::process::exit(run_overlay_probe());
        }
    }
    let live: Vec<&str> = LIVE_MODEL_SCENARIOS.iter().map(|(arg, _)| *arg).collect();
    eprintln!(
        "не указан сценарий: {} | {} | {} | {} | {} | {} | {} | {}[=вид] | \
         --overlay-selftest[=key|motion|none] | --overlay-bite[=вид] | \
         --overlay-stand=labwc|sway|kwin [аргументы проверки]",
        rexon_gui::remote_list_model::selftest::ARG,
        rexon_gui::we_stop_selftest::ARG,
        rexon_gui::discover_queries::selftest::ARG,
        rexon_gui::wallpaper_page_selftest::ARG,
        rexon_gui::qml_int64_probe::ARG,
        live.join(" | "),
        SELFTEST_ARG,
        BITE_ARG,
    );
    std::process::exit(2);
}

/// Fake Steam for the background grid scenario: a Steam directory with one
/// account and a disk cache of a list of five backgrounds (two animated —
/// one with a clip from Steam, the other by the background's own address).
///
/// Warning: only in OUR OWN home directory: the scenario writes to `$HOME`,
/// and on the real one it would land on top of the live Steam and the
/// product cache. The runner (`tests/selftest_scenarios.rs`) provides a
/// `rexon-selftest-…` directory; without it — a refusal.
///
/// The network is stubbed with a proxy on a closed port: the tab will
/// honestly go for the list and thumbnails, get an instant refusal and keep
/// the list from the cache — and the run depends neither on connectivity
/// nor on Steam's answer. The fake directory has no Steam login, and that is
/// part of the scenario too: the "animated ones are not shown" line must
/// name the reason.
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

    // A set for the showcase upload helper: two parts, none uploaded. There are
    // no pictures themselves — the helper does not need them for the steps.
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

/// Live models scenario: `qml/LiveModelsProbe.qml` with the chosen
/// `scenario`. The verdict is given by the QML itself (`Qt.exit(0|1)`), the
/// event loop code becomes the process code; 2 means the document did not
/// report at all.
fn run_live_models_probe(scenario: &str) -> i32 {
    // Without this a run from a script is mute: when stderr is not a terminal,
    // Qt on Arch/CachyOS sends console.log to journald.
    std::env::set_var("QT_FORCE_STDERR_LOGGING", "1");
    // The window needs a render loop (ListView/GridView delegates are created
    // in it), but it has no business on the live desktop: if no platform was
    // given, we set offscreen ourselves.
    if std::env::var_os("QT_QPA_PLATFORM").is_none() {
        std::env::set_var("QT_QPA_PLATFORM", "offscreen");
    }
    cxx_qt::init_qml_module!("org.rexon.gui");

    // A safeguard for when the QML did not load and there is nobody to call
    // Qt.exit: silence must become a failure, not a quiet zero. The document
    // has its own 20 s guard — this one is longer.
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
        // Rows of the file choice block are the core's real answer, not a copy in
        // QML: the bench compares the markup with what the core actually returns.
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

/// A store item for the "storefiles" scenario (a live case, McOS 17.10.2026:
/// the add-ons archive comes first alphabetically, while the theme is what
/// should be installed). `two` means two files; otherwise one, and there
/// must be no choice block.
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

/// Test subject of the idle screensaver overlay bench: one OVERLAY window per
/// screen with the exclusive keyboard (`qml/OverlayProbe.qml`).
///
/// It covers everything and takes the keyboard, so it has its own watchdog on
/// top of the QML "no input" timer and the parent's process killer: even a
/// hung QML cannot turn it into an invisible wall for long.
fn run_overlay_probe() -> i32 {
    use rexon_gui::layer_shell_selftest::overlay as ov;
    cxx_qt::init_qml_module!("org.rexon.gui");

    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(25));
        eprintln!("проба заставки не закончилась за 25 с — снимаю её с экрана");
        std::process::exit(3);
    });

    let mut app = QGuiApplication::new();
    if let Some(app) = app.as_mut() {
        app.set_application_name(&QString::from("Rexon (overlay probe)"));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(mut engine) = engine.as_mut() {
        let bite = std::env::var(ov::BITE_ENV).unwrap_or_default();
        let wait_ms: i32 = std::env::var(ov::INPUT_WAIT_ENV)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8000);
        let mut props = QMap::<QMapPair_QString_QVariant>::default();
        props.insert(QString::from("bite"), QVariant::from(&QString::from(&bite)));
        props.insert(
            QString::from("wakeMotionPx"),
            QVariant::from(&rexon_core::idle_screensaver::WAKE_MOTION_PX),
        );
        props.insert(QString::from("inputWaitMs"), QVariant::from(&wait_ms));
        engine.as_mut().set_initial_properties(&props);
        engine.load(&QUrl::from("qrc:/qt/qml/org/rexon/gui/qml/OverlayProbe.qml"));
    }
    match app.as_mut() {
        Some(app) => app.exec(),
        None => 2,
    }
}

/// Test subject window of the wallpaper layer bench: a QML window whose
/// layer is set by Rust.
///
/// The window lives on the LIVE desktop, so the run has two independent
/// limiters: this guard and the process killer on the parent's side. Even if
/// the QML hangs dead, the window will disappear — together with the
/// process.
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
        // The bite kind goes into QML as an initial property of the root, not as a
        // context variable: context properties are not wrapped in cxx-qt-lib 0.9,
        // while setInitialProperties exists and works BEFORE the first binding.
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
