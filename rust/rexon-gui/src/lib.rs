// rexon-gui — the Rexon GUI layer in Rust: QObjects and list models for
// QML through cxx-qt. The target picture of the migration (decision of
// 2026-07-29): a Rust core + cxx-qt + the existing QML; only PySide6 goes
// away.
//
// This crate is the foundation for replacing the Python bridge
// rexon/qt_wallpaper_engine/qml_ui/bridge.py.
//
// There is and will be no logic here: the models only lay out by roles what
// rexon-core computed. This is a condition of testability — the core is
// tested without Qt.

// Warning: THE REFERENCE BELOW IS MANDATORY, AND IT IS NOT A FORMALITY.
// QApplication in Rust lives in cxx-qt-lib-extras, and ONE binary uses it —
// the main window. But the crate initializer that cxx-qt-build writes into
// rexon-gui ALWAYS calls the extras initializer, while its static archive
// is linked only into the artefacts whose Rust code references the crate.
// Without this line linking of the WHOLE package failed — of other
// binaries too (`we-socket-probe`, `rexon-wallpaper-host`), with "undefined
// symbol: cxx_qt_init_crate_cxx_qt_lib_extras", and with `cargo build --lib`
// this breakage is not visible at all. It must not be removed, even if
// "nobody uses it".
use cxx_qt_lib_extras as _;

unsafe extern "C" {
    /// Registration of the types in `waywallen.ui` (cpp/rexon_register_we.cpp).
    fn rexon_do_register_we_types();
}

/// Register the wallpaper tab's types in `waywallen.ui`.
///
/// An ordinary `extern "C"`, not a cxx bridge: registration passes and
/// returns nothing, there is no work for the generator here.
///
/// Call ONCE, before loading QML, from the main thread with a live
/// `QGuiApplication` (`we_env::prepare_process`). Order matters: first
/// register all types, then create the QML engine, otherwise the engine
/// deadlocks in `QQmlThread` on the global Qt Protobuf registration.
pub fn register_we_types() {
    unsafe { rexon_do_register_we_types() }
}

/// Changing the skin on the fly: one registry for all QML pages.
///
/// Warning: why at all. Widget tabs are repainted the moment the customer
/// chooses a skin in "Settings" (`ui_base.apply_skin` →
/// `QApplication.setPalette`). The ported QML tabs took the palette from a
/// `palette` property declared `CONSTANT` — there was no change signal at
/// all, the bindings did not recompute, and an open tab stayed in the
/// previous skin's colours until reopened. Half the window in the new skin
/// and half in the old — the customer counts that as a breakage (rule #13).
///
/// ⭐ Why a registry and not an edit in every page (rule #12). Pages take
/// their colours the same way (`ui_base::active_skin_colors`), and the
/// previous defect with a baked-in default skin lived in them the same way
/// too (commit 3ee6011). Here the mechanism is ONE: a page subscribes with
/// one line in `Initialize` and unsubscribes with one in `Drop`, and the
/// next ported tab gets repainting with the same two lines — not a fifth
/// copy of the broadcast.
///
/// A closure rather than a `CxxQtThread<T>` of a concrete type: DIFFERENT
/// page types are subscribed, and storage cannot be made generic over the
/// type — cxx-qt emits its own alias for every QObject. The same technique
/// as the progress registry (`query_base::progress`) and the acquisition
/// store (`remote_list_model`).
pub mod skin {
    use core::pin::Pin;
    use std::sync::{Mutex, OnceLock};

    /// A push to one subscribed page. `Err(())` means the object died and the
    /// entry should be removed.
    type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Notify)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Subscribe a live page to skin changes. Returns the entry number — the
    /// page must remove it in `Drop`.
    ///
    /// `emit` is this page's `paletteChanged` signal. Delivery goes through
    /// `CxxQtThread::queue`: on an already dead object it returns the typed
    /// error `ObjectDestroyed` rather than crashing, so there is neither
    /// `unwrap()` nor a preceding `is_destroyed()` here (between the check and
    /// the queueing the object may die).
    pub fn watch<T>(handle: cxx_qt::CxxQtThread<T>, emit: fn(Pin<&mut T>)) -> u64
    where
        T: cxx_qt::Threading + 'static,
    {
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((
            id,
            Box::new(move || match handle.queue(emit) {
                Ok(()) => Ok(()),
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }),
        ));
        id
    }

    /// Remove a subscription. Idempotent: `0` means the page never subscribed.
    pub fn unwatch(id: u64) {
        if id == 0 {
            return;
        }
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// How many pages are subscribed right now (for checks).
    pub fn watching() -> usize {
        registry()
            .lock()
            .expect("реестр скина не отравлен")
            .entries
            .len()
    }

    unsafe extern "C" {
        /// `cpp/rexon_gui_factory.cpp`.
        fn rexon_set_app_link_color(set: bool, r: u8, g: u8, b: u8);
    }

    /// Link colour of formatted text — into the application palette by the
    /// active skin (`ui_base::active_skin_link_rgb`). One point for all
    /// `Text.RichText` of all tabs and windows: "About", theme descriptions and
    /// comments, extension windows, vendored pages.
    ///
    /// Call from the main thread: at start-up BEFORE loading QML
    /// (`we_env::prepare_process`) and on a skin change ([`dispatch`]). Qt does
    /// not repaint text already shown — the colour gets into the document when
    /// the HTML is parsed, and an open tab will take the new colour with the
    /// next text change.
    pub fn apply_app_palette() {
        let link = rexon_core::ui_base::active_skin_link_rgb();
        let (r, g, b) = link.unwrap_or((0, 0, 0));
        unsafe { rexon_set_app_link_color(link.is_some(), r, g, b) }
    }

    /// Push all live pages: the skin changed, recompute the palette. Returns
    /// the number notified; dead entries are removed along the way.
    pub fn dispatch() -> usize {
        apply_app_palette();
        let mut told = 0usize;
        let mut reg = registry().lock().expect("реестр скина не отравлен");
        reg.entries.retain(|(_, notify)| match notify() {
            Ok(()) => {
                told += 1;
                true
            }
            Err(()) => false,
        });
        told
    }
}

/// One door to the pop-up status banner for all tabs.
///
/// Fallback fonts of right-to-left scripts for the whole application. Which
/// families is decided by the core (`qt_app::RTL_FALLBACK_FONTS`); here
/// there is only the Qt call through `cpp/rexon_gui_factory.cpp` (why not a
/// cxx bridge is explained there).
pub mod fonts {
    use rexon_core::qt_app::RTL_FALLBACK_FONTS;

    unsafe extern "C" {
        /// `cpp/rexon_gui_factory.cpp`.
        fn rexon_add_script_fallback_font(script: u8, family: *const u8, len: usize);
    }

    /// Install the fallback fonts. Call from the main thread AFTER the Qt
    /// application is created and BEFORE loading QML: the font database needs
    /// the platform, and Qt does not re-lay out already laid out text with a new
    /// fallback font.
    pub fn install_rtl_fallbacks() {
        for (script, family) in RTL_FALLBACK_FONTS {
            unsafe { rexon_add_script_fallback_font(script as u8, family.as_ptr(), family.len()) }
        }
    }
}

/// ⭐ Why here and not as a copy in every page (rule #12). The defect "the
/// message never goes away" was introduced THREE times — in the widget
/// "Settings" (28.06), in the widget "Steam" (08.07) and a third time in the
/// QML "Steam" tab, where the live check on 18.09 caught the banner hanging
/// for 29 minutes. The markup is now one for all tabs (`qml/RexonToast.qml`),
/// and this module feeds it: as long as the colours, paddings, lifetime and
/// width come from here, a tab cannot get a banner or numbers of its own.
///
/// The core computes everything anyway (`ui_base`) — here there is only
/// converting its answers into Qt types.
pub mod toast {
    use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};
    use rexon_core::ui_base;

    /// Banner colours and metrics as one map: QML reads it with a dot
    /// (`styles.toast.paddingH`). The same door as for the widget banner —
    /// `ui_base::toast_colors` and the `TOAST_*` constants next to it.
    pub fn style_map(dark: bool) -> QMap<QMapPair_QString_QVariant> {
        let skin = ui_base::toast_colors(dark);
        let col = |css: &str| {
            QVariant::from(&QString::from(ui_base::css_color_to_qml(css).as_str()))
        };
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in [
            ("background", col(skin.background)),
            ("border", col(skin.border)),
            ("textColor", col(skin.text)),
            ("borderWidth", QVariant::from(&ui_base::TOAST_BORDER_WIDTH)),
            ("radius", QVariant::from(&ui_base::TOAST_RADIUS)),
            ("paddingH", QVariant::from(&ui_base::TOAST_PADDING_H)),
            ("paddingV", QVariant::from(&ui_base::TOAST_PADDING_V)),
            ("margin", QVariant::from(&ui_base::TOAST_MARGIN)),
        ] {
            out.insert(QString::from(key), value);
        }
        out
    }

    /// After how many ms the message goes away by itself.
    pub fn timeout_ms() -> i32 {
        ui_base::TOAST_TIMEOUT_MS as i32
    }

    /// Banner width for text of this width.
    ///
    /// The widget banner measures the text on one line and adds ITS OWN
    /// paddings to it: for a QLabel both the border and the inner paddings come
    /// from the style sheet and are part of the content margins. A QML rectangle
    /// has none of them, so the same numbers are added by hand — from the core,
    /// not our own.
    pub fn width(page_width: i32, text_width: i32) -> i32 {
        let frame = 2 * (ui_base::TOAST_PADDING_H + ui_base::TOAST_BORDER_WIDTH);
        ui_base::toast_width(
            page_width.max(0),
            text_width.max(0) + frame + ui_base::TOAST_TEXT_SLACK,
            0,
            ui_base::TOAST_MAX_WIDTH,
        )
    }
}

// ── Translating QML strings: the same `.mo` catalogue as the core's ─────

/// String receiver — the `RexonTextSink` signature from
/// `cpp/rexon_gui_factory.h`.
type TextSink = unsafe extern "C" fn(target: *mut std::ffi::c_void, utf8: *const u8, len: i32);

/// String lookup — the `RexonAppTranslate` signature.
type AppTranslate = unsafe extern "C" fn(
    disambiguation: *const std::ffi::c_char,
    source: *const std::ffi::c_char,
    n: i32,
    sink: TextSink,
    target: *mut std::ffi::c_void,
) -> bool;

unsafe extern "C" {
    fn rexon_install_app_translator(context: *const std::ffi::c_char, lookup: Option<AppTranslate>) -> bool;
}

/// The window translator's callback: a QML string → `i18n::qt_translate`.
///
/// Qt calls it from any thread, including the QML loading thread; the core
/// reads the catalogue under a `RwLock` and waits for nothing.
unsafe extern "C" fn app_translate(
    disambiguation: *const std::ffi::c_char,
    source: *const std::ffi::c_char,
    n: i32,
    sink: TextSink,
    target: *mut std::ffi::c_void,
) -> bool {
    if source.is_null() {
        return false;
    }
    // QML sources are UTF-8 (that is how the `qsTranslate` engine hands them
    // over); a string that is not UTF-8 would not be found in the catalogue
    // anyway.
    let Ok(source) = unsafe { std::ffi::CStr::from_ptr(source) }.to_str() else {
        return false;
    };
    let disambiguation = if disambiguation.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(disambiguation) }.to_str().ok()
    };
    let Some(text) = rexon_core::i18n::qt_translate(disambiguation, source, n) else {
        return false;
    };
    let Ok(len) = i32::try_from(text.len()) else {
        return false;
    };
    unsafe { sink(target, text.as_ptr(), len) };
    true
}

/// Install the QML string translator (`qsTranslate("rexon", …)`) for the
/// language the core has already chosen (`i18n::init`). There is no
/// separate catalogue for QML: the translator asks the core, and the core
/// uses the same `.mo` as for the Rust strings. English (the source) has no
/// catalogue — the previous translator is only removed.
///
/// → false if there is no Qt application yet. Call from the main thread
/// after the application is created and before loading QML; again after the
/// core's language changes (the translator has a string cache).
pub fn install_app_translator() -> bool {
    let context = std::ffi::CString::new(rexon_core::i18n::QT_CONTEXT).expect("контекст без нуля");
    let lookup: Option<AppTranslate> =
        rexon_core::i18n::mo_path().is_some().then_some(app_translate as AppTranslate);
    // The translator copies the context for itself (a QByteArray); the string
    // may die.
    unsafe { rexon_install_app_translator(context.as_ptr(), lookup) }
}

unsafe extern "C" {
    fn rexon_qt_translations_dir(buf: *mut std::ffi::c_char, len: std::ffi::c_int) -> std::ffi::c_int;
    fn rexon_install_qt_translations(
        paths: *const *const std::ffi::c_char,
        count: std::ffi::c_int,
    ) -> std::ffi::c_int;
}

/// Enable the translation catalogues of Qt itself for the Rexon language
/// `language`: dialog buttons and the edit menu of text fields (a right
/// click on a `TextArea`) are drawn by Qt itself, and no translation of ours
/// reaches them.
///
/// The decision belongs to the core (`cli::qt_translation_files`): which
/// files, in what order, and that English (and a language Qt does not have)
/// has none at all. Here there are only two shim calls — find Qt's
/// translations directory and enable the named files: cxx-qt-lib 0.9 has
/// not a single `QTranslator` or `QLibraryInfo` wrapper.
///
/// Call in EVERY process with a Qt window (the main window, the crash
/// window) after the application is created and before loading QML. If
/// there is no `qtbase` catalogue for the language — one line to the log,
/// and the Qt labels are English; we say nothing about the aggregate `qt`,
/// most Qt6 builds do not have it.
pub fn install_qt_translations(language: &str) {
    use rexon_core::cli;
    let dir = qt_translations_dir();
    let files = cli::qt_translation_files(language, std::path::Path::new(&dir), &|p| p.is_file());
    if let Some(note) = cli::qt_translation_missing_note(language, &files) {
        rexon_core::log_info!("lang", "{note}");
    }
    let owned: Vec<std::ffi::CString> = files
        .iter()
        .filter_map(|p| std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok())
        .collect();
    let ptrs: Vec<*const std::ffi::c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    unsafe { rexon_install_qt_translations(ptrs.as_ptr(), ptrs.len() as std::ffi::c_int) };
}

/// The translations directory of Qt itself (`QLibraryInfo::TranslationsPath`);
/// empty means the shim did not answer, and then no file will be found
/// (English buttons).
fn qt_translations_dir() -> String {
    let mut buf = vec![0u8; 512];
    loop {
        let need = unsafe {
            rexon_qt_translations_dir(buf.as_mut_ptr().cast(), buf.len() as std::ffi::c_int)
        };
        let need = usize::try_from(need).unwrap_or(0);
        if need < buf.len() {
            buf.truncate(need);
            return String::from_utf8_lossy(&buf).into_owned();
        }
        buf.resize(need + 1, 0);
    }
}

/// Journal of the user's actions (the `ActionJournal` singleton) and the
/// window heartbeat for the main thread watchdog (`rexon_core::ui_watch`).
pub mod action_journal;
/// Links from third-party text: a card, a question, trusted sites (the `LinkGuard` singleton).
pub mod link_guard;
/// Side-actions object of the "Appearance" tab: the store, the disk
/// overview, pictures, installing, applying and removing themes. Before it
/// the page got `actions` only from Python.
pub mod animations_actions;
pub mod screen_effects_actions;
pub mod app_widgets_actions;
pub mod appearance_actions;
pub mod appearance_page;
pub mod compare_page;
pub mod crash_report;
pub mod desktops_actions;
pub mod desktops_page;
pub mod extensions_actions;
/// The first-run wizard for the Rust window: the core's decision "whether to
/// offer installation", the wizard's labels, background installation and
/// "do not ask again". `request_show` is the door of the "Install Rexon into
/// the system…" button from "Settings".
pub mod first_run_wizard;
pub mod extensions_page;
pub mod discover_queries;
pub mod display_queries;
pub mod layer_shell;
pub mod local_preview_render;
pub mod layer_shell_selftest;
/// The main window host in Rust: an `ApplicationWindow` instead of a
/// `QMainWindow`.
pub mod main_window;
/// The first-run tutorial for the Rust window; `request_replay` is the door
/// of the "Take the tutorial again" button.
pub mod onboarding_overlay;
pub mod query_base;
/// Shared engine of the selection storages: the `SelectStorage` base and the
/// `WallpaperSelectStorage` layer with one macro for both bridges. Not a
/// cxx-qt bridge.
pub mod select_storage_base;
/// Side-actions object of the "Settings" tab: machine snapshots, the skin,
/// autostart, the file manager, the environment's native applications, event
/// sounds, default application protection. Before it the page got `actions`
/// only from Python.
pub mod settings_actions;
pub mod settings_page;
/// Background tasks of the "Steam" tab: a port of the original's five
/// `QThread`s to async (tokio) — the inventory, splash markers, preview
/// frames, thumbnails and applying a background as a wallpaper. Not a
/// cxx-qt bridge: there is no Qt here at all.
pub mod steam_workers;
/// Side-actions object of the "Steam" tab: the splash showcase snapshot,
/// choosing your own clip, stars, three toggles, the full-screen show and
/// the WebAPI token. Before it the page got `actions` only from Python.
pub mod steam_actions;
/// The "Profile showcases" sub-tab: cutting a picture for a Steam profile
/// showcase.
pub mod steam_showcase_actions;
/// The "Screensaver" sub-tab: a Rexon screensaver for the Steam Big Picture
/// idle screen.
pub mod steam_screensaver_actions;
/// The "Steam" tab — a class B tab in QML.
pub mod steam_page;
/// The "64-bit integer in QML" probe: a live engine against the bridges. Not
/// a cxx-qt bridge — a bench, like `layer_shell_selftest`. Its static
/// counterpart is the `qml_int64` guard at the end of this file.
pub mod qml_int64_probe;
pub mod qvalue;
pub mod remote_list_model;
pub mod tag_queries;
pub mod user_property_model;
pub mod wallpaper_actions;
pub mod wallpaper_audio;
pub mod wallpaper_host;
pub mod web_host;
/// "Listen" buttons of sound themes: five sounds, the Store before
/// installation.
pub mod sound_preview_actions;
pub mod window_buttons_actions;
/// "Launch feedback": the cursor and the task bar while a program is
/// starting.
pub mod launch_feedback_actions;
/// The "Razer Axon" source of the "Appearance" wallpaper section and its
/// settings block.
pub mod axon_actions;
/// The Rexon wallpaper playlist: the block in "⚙️ Settings" of the "HD
/// Wallpapers" section.
pub mod wallpaper_playlist_actions;
/// "Accent colour" above the "Colour schemes" grid.
pub mod accent_actions;
/// Scheduled "Light and dark" above "Global themes" and "Colour schemes".
pub mod day_night_actions;
/// "Save current look…" above "Global themes".
pub mod global_save_actions;
pub mod dde_look_actions;
pub mod mate_panel_actions;
pub mod window_menu;
pub mod wallpaper_list_model;
pub mod wallpaper_page_golden;
pub mod wallpaper_page_selftest;
pub mod wallpaper_queries;
pub mod wallpaper_select_storage;
pub mod autostart_queries;
pub mod playlist_queries;
pub mod playlist_item_select_storage;
pub mod plugin_queries;
pub mod plugin_translations;
pub mod remote_store_queries;
pub mod source_queries;
pub mod settings_queries;
pub mod status_queries;
pub mod global_pause_queries;
pub mod thumbnail_request;
/// The Rexon system tray icon. NOT a cxx-qt bridge: QML has nothing to do
/// with it, and this file has no place in `.files()` in build.rs — only the
/// `cpp/rexon_tray.cpp` shim in `.cpp_files()`.
pub mod tray;
/// The "do not blank the screen" bridge (org.freedesktop.ScreenSaver). NOT a
/// cxx-qt bridge: only the `cpp/rexon_screensaver_bridge.cpp` shim in
/// `.cpp_files()`.
pub mod screensaver_bridge;
pub mod we_env;
pub mod we_stop_selftest;
pub mod we_transport;
pub mod we_action;
pub mod we_actions;
pub mod we_app;
pub mod we_daemon_dbus;
pub mod we_notify;
pub mod we_objmodel;
/// The "Wallpaper Engine" tab shell in QML: the rail and the place of the
/// vendored page.
// The hidden "Developer" page of the wallpaper tab: upstream updates and UI
// parity reconciliation.
pub mod we_dev_page;
pub mod we_page;
/// Background tasks of the WE tab's "Developer" page: a port of the
/// original's three `QThread`s to async (tokio) — checking upstream,
/// installing the engine AppImage and the OWE plugin archive. Not a cxx-qt
/// bridge: there is no Qt here at all.
pub mod we_update_workers;
pub mod we_util;
pub mod we_singletons;
pub mod we_socket;

/// Build composition guard: the lists in `build.rs` against the contents of
/// the directories.
///
/// WHY A TEST AND NOT A CHECK IN `build.rs`. A forgotten line shows up worst
/// exactly when a file has JUST been added: in that case `build.rs` may not
/// rerun at all, and any check inside it would stay silent. The test runs on
/// every `cargo test -p rexon-gui --lib` regardless of whether cargo
/// considered the build script stale, and re-reads the directories anew.
///
/// The comparison is with a SNAPSHOT of what `build.rs` actually handed to
/// `.files()` / `qml_files()` (files in OUT_DIR), not with the text of
/// `build.rs`: parsing the source again would check similarity, not the
/// fact.
#[cfg(test)]
mod toast_tests {
    use super::toast;
    use rexon_core::ui_base;

    /// The message lifetime comes from the core and is positive.
    ///
    /// ⭐ A guard against a third relapse: a banner that got zero never goes
    /// away — and that is exactly how the "Steam" tab kept a message for 29
    /// minutes. The markup treats zero honestly ("do not hide"), so the guard
    /// belongs here, at the source of the number.
    #[test]
    fn toast_timeout_comes_from_the_core_and_is_positive() {
        assert_eq!(toast::timeout_ms(), ui_base::TOAST_TIMEOUT_MS as i32);
        assert!(toast::timeout_ms() > 0, "плашка с нулевым сроком не гаснет");
    }

    /// Banner width is the core's calculation, and it always fits into the
    /// window.
    #[test]
    fn toast_width_stays_inside_the_page() {
        // A narrow page: the banner does not stick out past its edge.
        assert!(toast::width(300, 4000) <= 300);
        // A wide page: the banner does not grow beyond the core's cap.
        assert!(toast::width(4000, 4000) <= ui_base::TOAST_MAX_WIDTH);
        // Negative numbers come from QML on the first frame (there is no size yet)
        // and must not give a negative width.
        assert!(toast::width(-1, -1) >= 0);
    }
}

#[cfg(test)]
mod build_manifest {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    /// Bridges that went into `.files()`.
    const BRIDGE_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bridge_files.list"));
    /// Files that went into `qml_files()`.
    const QML_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/qml_files.list"));

    /// Parse the snapshot into a set of paths relative to the crate root.
    fn listed(manifest: &str) -> BTreeSet<String> {
        manifest
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// Names of files with the given extension in the crate directory, without
    /// walking subdirectories: both the bridges and the .qml files lie exactly
    /// one level deep (for the bridges this is a qmltyperegistrar requirement,
    /// see `build.rs`).
    fn files_in(dir: &str, ext: &str) -> Vec<PathBuf> {
        let path = crate_dir().join(dir);
        let mut found: Vec<PathBuf> = std::fs::read_dir(&path)
            .unwrap_or_else(|e| panic!("не читается каталог {}: {e}", path.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == ext))
            .collect();
        found.sort();
        found
    }

    /// Whether the file contains a real cxx-qt bridge.
    ///
    /// The yardstick is the attribute at the start of a line. A mention of
    /// `cxx_qt::bridge` in a comment does not make it a bridge: for example,
    /// `src/query_base.rs` explains in its header why it is NOT a bridge, and it
    /// has no place in `.files()`.
    fn is_bridge(path: &PathBuf) -> bool {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("не читается {}: {e}", path.display()));
        text.lines()
            .any(|line| line.trim_start().starts_with("#[cxx_qt::bridge"))
    }

    /// Paths listed in `build.rs` but missing on disk.
    fn missing_on_disk(listed: &BTreeSet<String>) -> Vec<String> {
        listed
            .iter()
            .filter(|rel| !crate_dir().join(rel).is_file())
            .cloned()
            .collect()
    }

    #[test]
    fn every_bridge_is_listed_in_build_rs() {
        let listed = listed(BRIDGE_MANIFEST);
        let forgotten: Vec<String> = files_in("src", "rs")
            .into_iter()
            .filter(is_bridge)
            .map(|p| format!("src/{}", p.file_name().unwrap().to_string_lossy()))
            .filter(|rel| !listed.contains(rel))
            .collect();
        assert!(
            forgotten.is_empty(),
            "мост cxx-qt не перечислен в .files() в rust/rexon-gui/build.rs: {}. \
             Допишите туда эти строки — иначе вкладка мёртвая: C++-половина \
             моста не собирается, и его типы не доходят до QML (прежний случай — \
             «undefined symbol: …cxxQtThreadQueue…» у живого Rexon)",
            forgotten.join(", ")
        );

        let stale = missing_on_disk(&listed);
        assert!(
            stale.is_empty(),
            "в .files() в rust/rexon-gui/build.rs перечислены несуществующие файлы: {}. \
             Уберите строки или верните файлы на место",
            stale.join(", ")
        );
    }

    #[test]
    fn every_qml_file_is_listed_in_build_rs() {
        let listed = listed(QML_MANIFEST);
        let forgotten: Vec<String> = files_in("qml", "qml")
            .into_iter()
            .map(|p| format!("qml/{}", p.file_name().unwrap().to_string_lossy()))
            .filter(|rel| !listed.contains(rel))
            .collect();
        assert!(
            forgotten.is_empty(),
            "файл .qml не перечислен в qml_files() в rust/rexon-gui/build.rs: {}. \
             Допишите туда эти строки — иначе сборка зелёная, а страница мёртвая: \
             файл не попадёт в запечённый ресурс модуля, и QML найдёт его только \
             в работе, отказом «qrc:/qt/qml/org/rexon/gui/…: No such file or directory»",
            forgotten.join(", ")
        );

        let stale = missing_on_disk(&listed);
        assert!(
            stale.is_empty(),
            "в qml_files() в rust/rexon-gui/build.rs перечислены несуществующие файлы: {}. \
             Уберите строки или верните файлы на место",
            stale.join(", ")
        );
    }
}

/// The "64-bit integer exposed to QML" guard: the bridges against the list
/// of exceptions.
///
/// WHY. cxx-qt emits `i64`/`u64` as `::std::int64_t` / `::std::uint64_t`,
/// and on 64-bit Linux these are `long` / `unsigned long` — SEPARATE Qt
/// metatypes. The QML engine does not know them, and a literal declarative
/// binding to such a property ("displayId: 7") fails together with the
/// WHOLE document: the page is not created at all.
///
/// Warning: WHY A TEST AND NOT THE COMPILER. The defect is entirely
/// invisible: the build, `cargo test` and the type gate
/// `scripts/test_we_rust_types.py` all agree — the signatures are correct
/// after all. It breaks only on screen, and only for a declarative binding.
/// The live measurement, the analysis and a reproducible probe are in
/// `src/qml_int64_probe.rs`; here is the static half that does not let such
/// a declaration appear AGAIN.
///
/// ONLY `extern "RustQt"` blocks are parsed — that is exactly the surface
/// cxx-qt emits into the metaobject. `extern "C++"` blocks are ordinary C++
/// calls bypassing Qt (`rexon_we_network_cache_size() -> i64`), and their
/// 64-bit types have nothing to do with QML.
///
/// The list of bridges is taken from the same `OUT_DIR` snapshot as for the
/// build composition guard: a new bridge falls under the check
/// automatically.
#[cfg(test)]
mod qml_int64 {
    /// Bridges that went into `.files()` (the same snapshot as for
    /// `build_manifest`).
    const BRIDGE_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bridge_files.list"));

    /// Types that cxx-qt emits as a 64-bit C++ integer.
    const WIDE: &[&str] = &["i64", "u64", "i128", "u128", "isize", "usize"];

    /// The ONLY exception: the transport binding of the queries.
    ///
    /// `sendRequest` and `deliverResponse` are not an interface for QML but the
    /// seam between Rust and the C++ transport (`cpp/rexon_we_transport.cpp`).
    /// The signal is listened to by C++, the invokable is called by
    /// `QMetaObject::invokeMethod` with the same metatype —
    /// `Q_ARG(::std::int64_t, token)`. They are called from QML nowhere:
    /// `grep -rn "deliverResponse\|sendRequest"` over the vendored QML is empty.
    /// The frame number must stay an exact integer, so `i64` is appropriate
    /// here, and `f64` would be a regression.
    ///
    /// Warning: the exception works by function NAME and only for these two
    /// names. A new 64-bit declaration, whatever it is called, will be named by
    /// the guard and will fail `cargo test`.
    const ALLOWED_FNS: &[&str] = &["send_request", "deliver_response"];

    /// One found hit.
    struct Hit {
        file: String,
        line: usize,
        what: String,
    }

    /// Strip comments: otherwise the guard would catch its own explanations
    /// ("`f64`, not `i64`"). There are no string literals with `//` in the
    /// bridges.
    fn strip_comment(line: &str) -> &str {
        match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        }
    }

    /// Whether a piece of text contains a 64-bit integer type as a separate
    /// word.
    fn has_wide(text: &str) -> bool {
        WIDE.iter().any(|ty| {
            text.match_indices(ty).any(|(at, _)| {
                let before = text[..at].chars().next_back();
                let after = text[at + ty.len()..].chars().next();
                let word = |c: Option<char>| {
                    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
                };
                !word(before) && !word(after)
            })
        })
    }

    /// Function name from a `fn name(...)` declaration.
    fn fn_name(sig: &str) -> String {
        sig.split_once("fn ")
            .map(|(_, rest)| rest)
            .unwrap_or(sig)
            .split(['(', '<', ' '])
            .next()
            .unwrap_or("")
            .to_owned()
    }

    /// Parse one bridge: return all 64-bit declarations in `extern "RustQt"`
    /// blocks.
    fn scan(rel: &str, src: &str) -> Vec<Hit> {
        let lines: Vec<&str> = src.lines().collect();
        let mut hits = Vec::new();
        // Brace depth at which the RustQt block opened.
        let mut depth: i32 = 0;
        let mut rust_qt_depth: Option<i32> = None;
        let mut index = 0;
        while index < lines.len() {
            let code = strip_comment(lines[index]);
            let trimmed = code.trim_start();

            let opens_rust_qt = trimmed.starts_with("extern \"RustQt\"")
                || trimmed.starts_with("unsafe extern \"RustQt\"");

            if rust_qt_depth.is_some() {
                // ── property ──────────────────────────────────────────────
                if trimmed.starts_with("#[qproperty(") {
                    let mut chunk = code.to_owned();
                    let mut last = index;
                    while chunk.matches('(').count() > chunk.matches(')').count()
                        && last + 1 < lines.len()
                    {
                        last += 1;
                        chunk.push(' ');
                        chunk.push_str(strip_comment(lines[last]).trim());
                    }
                    let ty = chunk
                        .split_once("#[qproperty(")
                        .map(|(_, rest)| rest)
                        .unwrap_or("")
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_owned();
                    if has_wide(&ty) {
                        let name = chunk
                            .split_once("#[qproperty(")
                            .map(|(_, rest)| rest)
                            .unwrap_or("")
                            .split(',')
                            .nth(1)
                            .unwrap_or("?")
                            .trim()
                            .to_owned();
                        hits.push(Hit {
                            file: rel.to_owned(),
                            line: index + 1,
                            what: format!("свойство `{name}` объявлено `{ty}`"),
                        });
                    }
                    index = last + 1;
                    continue;
                }
                // ── function (invokable, signal, property accessor) ───────
                if trimmed.starts_with("fn ") || trimmed.starts_with("unsafe fn ") {
                    let mut chunk = code.to_owned();
                    let mut last = index;
                    while !chunk.contains(';')
                        && !chunk.trim_end().ends_with('{')
                        && last + 1 < lines.len()
                    {
                        last += 1;
                        chunk.push(' ');
                        chunk.push_str(strip_comment(lines[last]).trim());
                    }
                    let name = fn_name(&chunk);
                    if has_wide(&chunk) && !ALLOWED_FNS.contains(&name.as_str()) {
                        hits.push(Hit {
                            file: rel.to_owned(),
                            line: index + 1,
                            what: format!("подпись `{name}` несёт 64-битное целое"),
                        });
                    }
                    index = last + 1;
                    continue;
                }
            }

            depth += code.matches('{').count() as i32;
            depth -= code.matches('}').count() as i32;
            if opens_rust_qt && rust_qt_depth.is_none() {
                // The block opened: remember the depth OUTSIDE it.
                rust_qt_depth = Some(depth - 1);
            } else if rust_qt_depth == Some(depth) {
                rust_qt_depth = None;
            }
            index += 1;
        }
        hits
    }

    #[test]
    fn no_wide_integers_reach_qml() {
        let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut hits = Vec::new();
        let mut checked = 0usize;
        for rel in BRIDGE_MANIFEST.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let path = crate_dir.join(rel);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("не читается {}: {e}", path.display()));
            checked += 1;
            hits.extend(scan(rel, &src));
        }
        assert!(
            checked > 0,
            "слепок мостов пуст — сторож ничего не проверил"
        );
        let report: Vec<String> = hits
            .iter()
            .map(|h| format!("{}:{} — {}", h.file, h.line, h.what))
            .collect();
        assert!(
            report.is_empty(),
            "64-битное целое торчит наружу в QML:\n  {}\n\n\
             Так делать нельзя: cxx-qt печатает `i64`/`u64` как `long` / \
             `unsigned long`, это отдельные метатипы Qt, и движок QML их не \
             знает. Литеральная декларативная привязка в такое свойство \
             («displayId: 7») ОТКАЗЫВАЕТ вместе со всем документом — \
             страница не создаётся вовсе, а ни сборка, ни остальные тесты \
             этого не видят.\n\
             Как чинить: объявить наружу `f64` (в QML — `double`), поле \
             внутри оставить точным, а на границе звать \
             `qvalue::i64_to_qml` / `u64_to_qml` и обратно \
             `qvalue::qml_to_i64` / `qml_to_u64`. Образец — \
             `src/playlist_item_select_storage.rs`.\n\
             ⚠️ Если значению НУЖНЫ все 64 бита (идентификатор Steam \
             Workshop, отметка времени в наносекундах), `double` не годится: \
             он держит целые точно лишь до 2^53. Такое значение выпускать \
             `QString`, а не числом.\n\
             Живая проверка после правки: QT_QPA_PLATFORM=offscreen \
             cargo run -p rexon-gui --bin rexon-gui-selftest -- --qml-int64-probe",
            report.join("\n  ")
        );
    }
}
