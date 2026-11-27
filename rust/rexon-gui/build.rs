// Build of the QML module `org.rexon.gui`: cxx-qt runs moc +
// qmltyperegistrar over the bridge and bakes the .qml into a static
// resource (qrc:/qt/qml/org/rexon/gui/). A static plugin because the build
// is pure cargo, without CMake: a dynamic plugin requires a qmldir layout
// next to the binary (a limitation documented in cxx-qt-build).
use cxx_qt_build::{CxxQtBuilder, QmlModule};

// The lists below are the single source of truth about the module's
// composition. They are moved into constants not for looks: their snapshot
// goes into OUT_DIR, and a guard in the crate's tests (`src/lib.rs`, the
// `build_manifest` module) compares the contents of the directories with
// it. A forgotten line is caught by a test, not by a live crash.
//
// Warning: make the edit HERE, not in the call below: the call takes these
// constants.

/// Pages and controls baked into the module's static resource.
const QML_FILES: &[&str] = &[
    "qml/LayerShellProbe.qml",
    // Idle screensaver overlay bench (`layer_shell_selftest::overlay`).
    "qml/OverlayProbe.qml",
    // Live models in real QML (the Discover grid, the wallpaper property
    // editor, hiding the banner) — scenarios of `rexon-gui-selftest`.
    "qml/LiveModelsProbe.qml",
    "qml/WallpaperVideo.qml",
    // Web wallpapers: the same technique, the same set of placements, but the
    // content is drawn by QtWebEngine.
    "qml/WallpaperWeb.qml",
    // The waywallen daemon output client on X11 (`--mode waywallen`): desktop
    // windows with the waywallen output item.
    "qml/WallpaperWaywallen.qml",
    // Stage 3 of leaving PySide6: the tabs are rewritten in QML one at a time.
    // The first is "DE comparison".
    "qml/ComparePage.qml",
    // The main window whose host is in Rust: an `ApplicationWindow` instead of
    // the product's `QMainWindow`. A separate entry point for the developer
    // (the `rexon-main-window` binary); the live Rexon does not load it.
    "qml/MainWindow.qml",
    // The "Desktops" tab — a class B tab in QML.
    "qml/DesktopsPage.qml",
    // The desktop operation window — shared by "Desktops" and "DE comparison".
    "qml/DeOperationDialog.qml",
    // The "Settings" tab — the second class B tab in QML.
    "qml/SettingsPage.qml",
    // The "Animation" and "Window title buttons" sections of the same tab — as
    // separate components with their own action objects.
    "qml/AnimationsSection.qml",
    // "Desktop effects" (all KWin effects) — KDE only.
    "qml/DesktopEffectsSection.qml",
    // "Screen effects" (CRT, videotape…) — a section of "Appearance"; the
    // tiles are drawn by the preview shader (`bake_preview_shader`).
    "qml/ScreenEffectsSection.qml",
    "qml/WindowButtonsSection.qml",
    // "Launch feedback" (as in the KDE "Cursors" module) — in the same place.
    "qml/LaunchFeedbackSection.qml",
    // The "Razer Axon" source of the "HD Wallpapers" section (the Axon
    // library) and its settings block in "⚙️ Settings" of the same section.
    "qml/AxonLibraryView.qml",
    "qml/AxonSettingsSection.qml",
    // The Rexon wallpaper playlist in "⚙️ Settings" of the "HD Wallpapers"
    // section.
    "qml/WallpaperPlaylistSection.qml",
    // "Accent colour" above the "Colour schemes" grid of "Appearance".
    "qml/AccentSection.qml",
    // "Light and dark" (scheduled) above "Global themes" and "Colour schemes".
    "qml/DayNightSection.qml",
    // "Save current look…" above "Global themes".
    "qml/GlobalSaveSection.qml",
    // Our own colour scheme editor — when there is no KDE editor.
    "qml/ColorSchemeEditor.qml",
    // "Icons and toolbars" (like the "Settings" of the KDE application style).
    "qml/AppWidgetsSection.qml",
    // "Font management" inside the fonts section (the actions belong to the
    // page itself, `SettingsActions`).
    "qml/FontManagerSection.qml",
    // The "Deepin windows" section: where Deepin changes window corners,
    // transparency and accent — shown only in a Deepin session.
    "qml/DdeLookSection.qml",
    // The "MATE panel" section: ready-made panel layouts — only in a MATE
    // session. A standalone component: it will later move to "Appearance".
    "qml/MatePanelSection.qml",
    // The "Extensions" tab — a class B tab in QML.
    "qml/ExtensionsPage.qml",
    // The "Steam" tab — a class B tab in QML.
    "qml/SteamPage.qml",
    // The "Profile showcases" sub-tab of the same tab — as a separate
    // component with its own action object.
    "qml/SteamShowcaseSection.qml",
    "qml/SteamMediaCard.qml",
    // The "Screensaver" sub-tab: a Rexon screensaver for the Big Picture idle
    // screen.
    "qml/SteamScreensaverSection.qml",
    // The "Appearance" tab — the last class B tab in QML.
    "qml/AppearancePage.qml",
    // The "Login manager" block in the login screen section of the same tab
    // (the data is the `loginScreenReady` probe, the action is
    // `makeDefaultLoginManager`).
    "qml/LoginManagerSection.qml",
    // File choice for a store item with several files — as a separate
    // component for the sake of the `LiveModelsProbe.qml` bench (the
    // "storefiles" scenario).
    "qml/StoreFileChoice.qml",
    // Conky widget settings in the section card — as a separate component:
    // position, background, window, screen, clock and network
    // (`conky_overlay`).
    "qml/ConkySettings.qml",
    // Palette of the Conky widget's found colours in the same card
    // (`conky_colors`).
    "qml/ConkyColours.qml",
    // Conky widget sets above the section grid (`conky_sets`).
    "qml/ConkySets.qml",
    // Mini-map of the screen with dragging of Conky widgets (`conky_layout`).
    "qml/ConkyScreenMap.qml",
    // The "Wallpaper Engine" tab — a shell around waywallen's vendored QML:
    // the rail and the page's place. The pages are vendored and come as a
    // separate module `waywallen.ui`, so they are not baked into this static
    // resource.
    "qml/WallpaperEnginePage.qml",
    // The hidden "Developer" page of the same tab is OURS, not vendored:
    // upstream updates and UI parity reconciliation. That is why it is baked
    // here, unlike the six vendored pages.
    "qml/WeDeveloperPage.qml",
    // Controls in the application skin — shared by all QML tabs: the default
    // QtQuick Controls style (Basic) does not follow the application palette,
    // and changing the style for the whole process is not allowed — the
    // vendored QML of the Wallpaper Engine tab lives next door.
    "qml/RexonButton.qml",
    "qml/RexonDangerButton.qml",
    "qml/RexonCheckBox.qml",
    "qml/RexonComboBox.qml",
    "qml/RexonSlider.qml",
    "qml/RexonTextField.qml",
    // Text aligned to the start of the line, also for a right-to-left
    // language (why a separate type — in the file header).
    "qml/RexonText.qml",
    // Text with links from third-party text and the question window before an
    // external site — one for all tabs (the decision is the core's
    // `link_policy`).
    "qml/RexonLinkText.qml",
    "qml/RexonLinkDialog.qml",
    // Directional icons and the ← → keys by mirroring of the place — one for
    // all pagers and chevrons (why a separate type — in the file header).
    "qml/RexonArrows.qml",
    // `%1` substitutions in QML strings — with isolates for a right-to-left
    // language, like `i18n::fill` in the core (why a separate type — in the
    // file header).
    "qml/RexonBidi.qml",
    // The pop-up status banner — one for all tabs: the hide timer lives in it
    // rather than being rewritten in every page.
    "qml/RexonToast.qml",
    // The large picture viewer and the way into it from a thumbnail — also one
    // for all tabs: a magnifier under the cursor, a click, paging, zoom.
    "qml/RexonImageViewer.qml",
    "qml/RexonZoomArea.qml",
    // The main window menu ("☰") and the dialogs behind its items: the About
    // box, the question before resetting the KDE theme, the Rexon removal
    // window.
    "qml/WindowMenu.qml",
    "qml/WindowMenuDialogs.qml",
    // The first-run tutorial over the Rust window.
    "qml/OnboardingOverlay.qml",
    // The first-run wizard: a welcome with the choice "who to install for".
    // Warning: the overlay is called FirstRunWizardOverlay, NOT
    // FirstRunWizard: the name `FirstRunWizard` in the module is already taken
    // by the bridge (`#[qml_element]` in src/first_run_wizard.rs), and a file
    // of the same name would make the type ambiguous ("Ambiguous type detected
    // … defined multiple times"), and the bridge unreachable from the markup.
    // The pair is named the same way as the tutorial's: `OnboardingModel` +
    // `OnboardingOverlay.qml`.
    "qml/FirstRunWizardOverlay.qml",
];

/// cxx-qt bridges that go through moc and qmltyperegistrar.
///
/// A bridge must lie in the same directory as the other #[qml_element]
/// bridges: qmltyperegistrar takes only the file name from moc, without the
/// path prefix (QTBUG-93443), so subdirectories for bridges are not
/// allowed.
const BRIDGE_FILES: &[&str] = &[
    // The main window host in Rust: the set of sections and the palette.
    "src/main_window.rs",
    // Journal of the user's actions: the `ActionJournal` singleton for the
    // shared controls, the window menu and the section strip.
    "src/action_journal.rs",
    // Links from third-party text: the `LinkGuard` singleton of the shared
    // `RexonLinkText.qml` component (our own store card, the question, ↗).
    "src/link_guard.rs",
    // The first-run tutorial: the scenario and progress from the core for the
    // overlay.
    "src/onboarding_overlay.rs",
    // The first-run wizard: the core's decision, labels, background
    // installation. The wizard's markup is `qml/FirstRunWizardOverlay.qml` in
    // the list above.
    "src/first_run_wizard.rs",
    // The "DE comparison" tab — the first class B tab in QML.
    "src/compare_page.rs",
    // The "Desktops" tab — a class B tab in QML.
    "src/desktops_page.rs",
    // Side-actions object of the same tab: the package version from the
    // repositories and switching to the built-in "DE comparison". Before it
    // the page got `actions` only from Python.
    "src/desktops_actions.rs",
    // The "Settings" tab — the second class B tab in QML.
    "src/settings_page.rs",
    // Side-actions object of the same tab: machine snapshots, the skin,
    // autostart, the file manager, the environment's native applications,
    // event sounds, default application protection, importing Windows themes.
    // Before it the page got `actions` from Python.
    "src/settings_actions.rs",
    // The "Animation" and "Window title buttons" sections: the snapshot and
    // the write are the core's (`animations`, `window_buttons`).
    "src/animations_actions.rs",
    "src/screen_effects_actions.rs",
    "src/app_widgets_actions.rs",
    "src/window_buttons_actions.rs",
    // "Launch feedback": the snapshot and the write are the core's
    // (`launch_feedback`).
    "src/launch_feedback_actions.rs",
    // The "Razer Axon" source: the library, the query, "Apply", our settings.
    "src/axon_actions.rs",
    // The Rexon wallpaper playlist: the model, edits, switching — the core's
    // (`wallpaper_playlist`).
    "src/wallpaper_playlist_actions.rs",
    // "Accent colour": the snapshot and the write are the core's (`accent`).
    "src/accent_actions.rs",
    // "Light and dark": the snapshot, the write, the timer — the core's
    // (`day_night`).
    "src/day_night_actions.rs",
    // "Save current look…": the snapshot, the package, the preview — the
    // core's (`global_save`).
    "src/global_save_actions.rs",
    // The "Deepin windows" section: the snapshot is the core's (`dde`).
    "src/dde_look_actions.rs",
    // The "MATE panel" section: snapshot, apply, revert — the core's
    // (`mate_panel`).
    "src/mate_panel_actions.rs",
    // "Listen" buttons of sound themes (the core — `sound_preview`).
    "src/sound_preview_actions.rs",
    // The "Extensions" tab — a class B tab in QML.
    "src/extensions_page.rs",
    // Side-actions object of the same tab: installing .rxn, removing, enabling
    // and disabling. Before it the page got `actions` from Python.
    "src/extensions_actions.rs",
    // The "Steam" tab — a class B tab in QML.
    "src/steam_page.rs",
    // Side-actions object of the same tab: the splash showcase snapshot,
    // choosing your own clip, stars, three toggles, the full-screen show and
    // the WebAPI token. Before it the page got `actions` from Python.
    "src/steam_actions.rs",
    // The "Profile showcases" sub-tab: cutting a picture for the showcase and
    // the background for the preview (the core — `steam_showcase`).
    "src/steam_showcase_actions.rs",
    // The "Screensaver" sub-tab: installing, updating and removing the
    // screensaver in the Steam folder (the core — `steam_screensaver`).
    "src/steam_screensaver_actions.rs",
    // The "Appearance" tab — the last class B tab in QML.
    "src/appearance_page.rs",
    // Side-actions object of the same tab: the store, AUR, the disk overview,
    // tile and carousel pictures, installing, applying and removing themes.
    // Before it the page got `actions` from Python.
    "src/appearance_actions.rs",
    // The "Wallpaper Engine" tab — a shell around the vendored QML.
    //
    // Warning: forgetting this line means getting a dead tab. While the crate
    // was also built as a `cdylib`, the build stayed GREEN (undefined symbols
    // were let through at link time), and the bridge failed only on `dlopen`
    // by the live Rexon ("undefined symbol: …WallpaperEngineModel…"). We
    // stepped on this on 30.08.2026; since then the omission is caught by the
    // guard in the crate's tests.
    "src/we_page.rs",
    // The hidden "Developer" page of the wallpaper tab.
    "src/we_dev_page.rs",
    // Side-actions object of the same tab: restarting the wallpaper daemon.
    // Before it the page got `actions` from Python.
    "src/we_actions.rs",
    "src/discover_queries.rs",
    "src/display_queries.rs",
    "src/global_pause_queries.rs",
    "src/we_action.rs",
    "src/we_app.rs",
    "src/we_daemon_dbus.rs",
    "src/we_notify.rs",
    "src/we_objmodel.rs",
    "src/we_util.rs",
    "src/we_socket.rs",
    "src/layer_shell.rs",
    "src/wallpaper_host.rs",
    // `src/web_host.rs` is no longer a bridge: QtWebEngineQuick is opened with
    // `dlopen` only in the web mode of the wallpaper host (the analysis is
    // there).
    "src/wallpaper_audio.rs",
    "src/remote_list_model.rs",
    "src/tag_queries.rs",
    "src/thumbnail_request.rs",
    "src/source_queries.rs",
    "src/settings_queries.rs",
    "src/status_queries.rs",
    "src/remote_store_queries.rs",
    "src/plugin_queries.rs",
    // Plugin labels: the plugin author's catalogue over our Russian layer.
    "src/plugin_translations.rs",
    "src/playlist_queries.rs",
    // Choosing the playlist composition: the type is declared right in
    // WallpaperPage.qml 0.3.7.
    "src/playlist_item_select_storage.rs",
    "src/autostart_queries.rs",
    "src/user_property_model.rs",
    "src/wallpaper_actions.rs",
    // The main window menu: the set of items, dialog texts and removing Rexon.
    // Without this line the bridge will not go through moc, and the menu will
    // not appear in QML.
    "src/window_menu.rs",
    "src/wallpaper_list_model.rs",
    "src/wallpaper_queries.rs",
    "src/wallpaper_select_storage.rs",
];

/// Names of the snapshots in OUT_DIR. The guard in `src/lib.rs` reads the
/// same strings.
const QML_MANIFEST: &str = "qml_files.list";
const BRIDGE_MANIFEST: &str = "bridge_files.list";

/// Put into OUT_DIR exactly what goes to the builder.
///
/// Warning: checking the directory contents right here is pointless: when a
/// NEW file is added, build.rs may not rerun at all — and that is exactly
/// the case to catch. So the check is done by a test that always runs,
/// while build.rs only honestly reports its list.
fn write_manifest(name: &str, files: &[&str]) {
    let out_dir = std::env::var("OUT_DIR").expect("cargo обязан задать OUT_DIR");
    let path = std::path::Path::new(&out_dir).join(name);
    let body = files.join("\n");
    // Write only on change: a needless rewrite of a file in OUT_DIR would make
    // cargo consider the build dirty.
    if std::fs::read_to_string(&path).ok().as_deref() != Some(body.as_str()) {
        std::fs::write(&path, body).expect("не удалось записать слепок состава сборки");
    }
}

fn main() {
    write_manifest(QML_MANIFEST, QML_FILES);
    write_manifest(BRIDGE_MANIFEST, BRIDGE_FILES);
    build_translations();
    let preview_shader = bake_preview_shader();

    // Build against the same Qt6 that PySide6 uses — during the strangler
    // migration the Rust models live in one process with Python, and the ABI
    // must match. Without a hint cxx-qt may pick qmake from Qt5, which is also
    // present in the system (the same reason as in rexon-qt-filter).
    if std::env::var_os("QMAKE").is_none() {
        for cand in ["/usr/bin/qmake6", "/usr/lib/qt6/bin/qmake"] {
            if std::path::Path::new(cand).exists() {
                std::env::set_var("QMAKE", cand);
                break;
            }
        }
    }

    // LayerShellQt is a KDE library (the layer-shell-qt package) with no Rust
    // bindings, so it is linked as an ordinary C++ dependency of the
    // cpp/rexon_layer_shell.cpp shim. The headers lie in the system include
    // tree (/usr/include/LayerShellQt/); it installs no separate .pc file —
    // only a CMake config, which does not help a cargo build.
    //
    // The presence check is here rather than "some day at link time": without
    // it a missing package would surface as a wall of compiler errors.
    let layer_shell_header = ["/usr/include/LayerShellQt/window.h"]
        .iter()
        .find(|p| std::path::Path::new(p).exists());
    assert!(
        layer_shell_header.is_some(),
        "не найден заголовок LayerShellQt (/usr/include/LayerShellQt/window.h). \
         Слой обоев без него собрать нельзя — поставьте пакет layer-shell-qt"
    );
    println!("cargo:rustc-link-lib=LayerShellQtInterface");

    // SAFETY (`cc_builder`): the callback only ADDS Qt header directories and
    // does not touch the flags and paths that cxx-qt-build itself sets.
    unsafe {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.rexon.gui")
            .version(1, 0)
            .qml_files(QML_FILES.iter().copied()),
    )
    .qt_module("Quick")
    // The tray icon: QSystemTrayIcon, QMenu, QAction and finding the main
    // window live in QtWidgets. The PRODUCT gets no new dependency from this —
    // the Rexon process is a PySide6 QApplication anyway, that is, libQt6Widgets
    // is already loaded in it; here we only stop pretending it is not there.
    .qt_module("Widgets")
    .qt_module("DBus")
    .qt_module("Network")
    // Batch 7: the WE tab transport (QWebSocket) moved here from client.py —
    // without this module the socket does not link.
    .qt_module("WebSockets")
    // QtWebEngineQuick is NOT linked here: the wallpaper host opens it with
    // `dlopen` only in web mode (src/web_host.rs). A direct reference would
    // pull Chromium into every video wallpaper start of the same binary.
    // The video wallpaper frame stream takes the frame from QVideoSink
    // (cpp/rexon_wallpaper_host.cpp): on the offscreen platform VideoOutput
    // does not draw, and a window snapshot is black.
    // The product gets no new dependency — the video wallpaper QML loads
    // QtMultimedia anyway.
    .qt_module("Multimedia")
    // Theme samples on the "Appearance" tiles are drawn from the themes' own
    // artwork (Plasma style frames and Aurorae — SVG parts by name):
    // cpp/rexon_local_preview.cpp calls QSvgRenderer. The qt6-svg package is
    // in the dependencies anyway: Qt reads SVG icons with it.
    .qt_module("Svg")
    .include_dir("cpp")
    // waywallen's logo and release notes at the address of their markup
    // (`qrc:/waywallen/ui/assets/…`): without this their pages look for files
    // in the resources of someone else's binary and do not find them. The
    // analysis is in the .qrc itself.
    .qrc("waywallen_assets.qrc")
    // The "Screen effects" tile shader: next to the module's markup, QML takes
    // it by the relative address `shaders/screen-effects.frag.qsb`.
    .qrc(&preview_shader)
    // The Flash mode of the wallpaper host takes the window's wl_surface
    // through `qpa/qplatformnativeinterface.h` (cpp/rexon_wallpaper_host.cpp):
    // this header lies in directories with the Qt version number, which are
    // not among cxx-qt-build's public paths.
    .cc_builder(|cc| {
        for dir in qt_private_include_dirs() {
            cc.include(dir);
        }
    })
    // The gadget shim is ONE per project: the source lives in rexon-qt-filter,
    // where the filter editor model put it, and here it is included as the
    // same file. There is no second copy of the shim in the project.
    .include_dir("../rexon-qt-filter/cpp")
    .cpp_files([
        "cpp/rexon_gui_factory.cpp",
        "cpp/rexon_layer_shell.cpp",
        // The video wallpaper host: the X11 window identifier and the frame stream
        // in mmap — neither can be reached from Rust (no QImage in cxx-qt-lib
        // 0.9).
        "cpp/rexon_wallpaper_host.cpp",
        // The local theme preview placeholder: QPainter over a QPixmap, drawText
        // with a rectangle and QFontMetrics — none of this is in cxx-qt-lib 0.9.
        // The plan of operations is built by the core and executed by
        // src/local_preview_render.rs.
        "cpp/rexon_local_preview.cpp",
        // A snapshot of a form from real widgets in a Qt style (the
        // rexon-theme-shot helper): QApplication, QStyleFactory, QWidget, grab() —
        // cxx-qt-lib has no widgets.
        "cpp/rexon_theme_shot.cpp",
        // Registration of the types in `waywallen.ui` for the live Rexon: included
        // here because the name of the generated cxx-qt header is known only after
        // the bridges below are processed.
        "cpp/rexon_register_we.cpp",
        // The system tray icon: QSystemTrayIcon + QMenu + intercepting the main
        // window's close. Neither is in cxx-qt-lib 0.9.
        "cpp/rexon_tray.cpp",
        // The "do not blank the screen" bridge: the service on the bus is a
        // QDBusVirtualObject subclass, and cxx-qt-lib has no QtDBus. The decisions
        // are in the core (rexon_core::screensaver_bridge).
        "cpp/rexon_screensaver_bridge.cpp",
        // Dialogs of the "Extensions" tab: choosing a .rxn and a yes/no question.
        // There are no widget QFileDialog/QMessageBox in the QML process, and the
        // QML types are asynchronous — the analysis of the decision is in the
        // header's preamble.
        "cpp/rexon_ext_dialogs.cpp",
        // The return address of queries: number → a QPointer to the query and a
        // call of its `deliverResponse` through the metaobject (the reason for
        // C++ is in the .h header).
        "cpp/rexon_we_transport.cpp",
        // Batch 7: the transport itself — what cxx-qt-lib lacks entirely
        // (creating a QObject, QtDBus, QTimer, QProcess, a nested QEventLoop).
        "cpp/rexon_we_socket.cpp",
        // Batch 5: Qt services of the singletons (D-Bus, network cache,
        // clipboard) and the live models of App.
        "cpp/rexon_we_singletons.cpp",
        "cpp/rexon_we_objmodel.cpp",
        // Base of the Action singleton: a signal with default arguments. A header
        // rather than a .cpp — cxx-qt-build runs moc precisely over headers.
        "cpp/rexon_we_action_base.h",
        "../rexon-qt-filter/cpp/rexon_gadget_shim.cpp",
    ])
    .files(BRIDGE_FILES.iter().copied())
    .build();
    }
}

/// Directories of Qt's private headers with the version number
/// (`/usr/include/qt6/QtGui/6.x.y` and `…/QtGui`, the same for QtCore). The
/// version is taken from the directory itself, not from qmake: there is
/// only one in the header tree.
fn qt_private_include_dirs() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new("/usr/include/qt6");
    let mut dirs = Vec::new();
    for module in ["QtCore", "QtGui"] {
        let Ok(entries) = std::fs::read_dir(root.join(module)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && name.starts_with(|c: char| c.is_ascii_digit()) {
                dirs.push(entry.path());
                dirs.push(entry.path().join(module));
            }
        }
    }
    dirs
}

/// Interface translation catalogues (`po/*.po` → `.mo`; both Rust and QML
/// read it) next to the built binaries:
/// `target/<profile>/locale/<language>/LC_MESSAGES/`.
///
/// That is exactly where `rexon_core::i18n` looks for them for a binary from
/// the development tree, and the package moves the same directory to
/// `/usr/share/locale`. The build is here rather than a separate command so
/// that `cargo build-app` cannot produce a fresh binary with old
/// translations: a forgotten step would mean an English interface for a
/// Russian customer without a single error.
///
/// The build itself is in `scripts/i18n-update.sh --build` (one door for the
/// build, the package and a human). Without the translation programs the
/// build stops with an explanation rather than silently releasing the
/// product without languages.
/// Build the "Screen effects" preview shader into a `.qsb` (the Qt 6
/// `ShaderEffect` format) from `qml/shaders/screen-effects.frag`. The
/// `.frag` itself is generated by the core
/// (`rexon_core::screen_effects::preview_glsl`), and its freshness is
/// checked by the core test `preview_shader_is_current`. `qsb` comes from
/// qt6-shadertools (already in the package's makedepends). Puts the `.qsb`
/// and a `.qrc` for it into OUT_DIR (the resource address is
/// `qrc:/qt/qml/org/rexon/gui/qml/shaders/screen-effects.frag.qsb`) and
/// returns the path to this `.qrc`.
fn bake_preview_shader() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = manifest.join("qml/shaders/screen-effects.frag");
    println!("cargo:rerun-if-changed={}", src.display());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("screen-effects.frag.qsb");
    // qsb lies next to the other Qt tools: we ask qmake, then the usual places.
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    for qmake in ["/usr/bin/qmake6", "/usr/lib/qt6/bin/qmake"] {
        if let Ok(o) = std::process::Command::new(qmake).args(["-query", "QT_HOST_BINS"]).output() {
            let dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !dir.is_empty() {
                cands.push(std::path::Path::new(&dir).join("qsb"));
            }
        }
    }
    cands.extend(["/usr/lib/qt6/bin/qsb", "/usr/bin/qsb6", "/usr/bin/qsb"].map(std::path::PathBuf::from));
    let qsb = cands
        .into_iter()
        .find(|p| p.is_file())
        .expect("не найден qsb (пакет qt6-shadertools): без него не собрать плитки «Эффектов экрана»");
    let status = std::process::Command::new(&qsb)
        .args(["--glsl", "100 es,120,150", "--hlsl", "50", "--msl", "12", "-o"])
        .arg(&out)
        .arg(&src)
        .status()
        .expect("qsb запускается");
    assert!(status.success(), "qsb не собрал {}", src.display());
    let qrc = out.with_file_name("screen_effects_preview.qrc");
    let body = format!(
        "<RCC>\n    <qresource prefix=\"/qt/qml/org/rexon/gui\">\n        <file alias=\"qml/shaders/screen-effects.frag.qsb\">{}</file>\n    </qresource>\n</RCC>\n",
        out.display()
    );
    if std::fs::read_to_string(&qrc).ok().as_deref() != Some(body.as_str()) {
        std::fs::write(&qrc, body).expect("не удалось записать .qrc шейдера");
    }
    qrc
}

/// The profile directory (`target/<profile>`, where the binaries lie) by
/// OUT_DIR.
///
/// The depth of OUT_DIR depends on the cargo version: before 1.100 it is
/// `<profile>/build/<crate>-<hash>/out`, since 1.100
/// `<profile>/build/<crate>/<hash>/out`. The former "three levels up" count
/// gave `<profile>/build` on the new layout: fresh .mo files went to
/// `build/locale`, and the package on 15.11.2026 took the catalogue of a
/// previous build with old strings from `<profile>/locale`. So we look for
/// the nearest `build` directory from below and take its parent — that is
/// correct for both layouts (and with `--target`).
fn profile_dir(out_dir: &std::path::Path) -> std::path::PathBuf {
    out_dir
        .ancestors()
        .skip(1)
        .find(|p| p.file_name().is_some_and(|n| n == "build"))
        .and_then(|p| p.parent())
        .unwrap_or_else(|| panic!("в OUT_DIR нет каталога build: {}", out_dir.display()))
        .to_path_buf()
}

fn build_translations() {
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..");
    let po = root.join("po");
    let script = root.join("scripts/i18n-update.sh");
    println!("cargo:rerun-if-changed={}", po.display());
    println!("cargo:rerun-if-changed={}", script.display());
    if !po.join("LINGUAS").is_file() {
        return;
    }
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let dest = profile_dir(&out_dir).join("locale");
    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg("--build")
        .arg(&dest)
        .status()
        .expect("bash запускается");
    assert!(
        status.success(),
        "каталоги перевода не собрались ({}): нужен msgfmt (пакет gettext); \
         подробности — выше в выводе сборки",
        script.display()
    );
}
