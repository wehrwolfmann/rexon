// App — the root QML singleton of the Wallpaper Engine tab.
//
// A port of waywallen's `ui/src/app.cpp` (MIT, (c) hypengw; attribution in
// NOTICE).
//
// What it does: holds the live models (`rendererManager`/`displayManager`/
// `libraryManager`/`gpuManager`/`presentationManager`), feeds them daemon events and manages the network cache of
// QML images, which the vendored settings page has as a separate section.
//
// Warning: four members that the Python bridge did NOT HAVE AT ALL,
// although the vendored QML calls them: `networkCacheSize`,
// `networkCacheMaximumSize`, `refreshNetworkCacheSize()` and
// `clearNetworkCache()` (`SettingsPage.qml:124/132/768/776/778`). The
// "Network image cache" section showed "NaN of NaN", and the "Clear"
// button failed with a TypeError. The cache did exist — `host.py` set it up
// with its factory — nobody just managed it. Now there is one cache, and it
// lives in the shim (`cpp/rexon_we_singletons.cpp`), while host.py only
// shows it the engine: two copies of the cache setup would silently drift
// apart (rule #12).
//
// GPUs (`gpuManager`) are the only model the daemon does not feed with
// events: it lists the cards once at start-up. So App itself asks for
// `gpu_list` — on creation if the socket is already open, and on every
// connection (in the original this is `GpuListQuery::reload` in
// `App::init`). The answer comes back here by request number
// (`deliverResponse`).

/// cxx-qt bridge: the `App` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "C++" {
        include!("rexon_we_objmodel.h");
        /// The live models are children of App, like the `Box<...>` in the
        /// original's `AppPrivate`.
        ///
        /// # Safety
        /// `parent` is a live QObject (ownership goes to Qt).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_renderer_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_display_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_library_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_gpu_manager(parent: *mut QObject) -> *mut QObject;
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_presentation_manager(parent: *mut QObject) -> *mut QObject;

        include!("rexon_we_singletons.h");
        /// Register as a receiver of daemon frames.
        ///
        /// # Safety
        /// `app` is a live QObject (a pointer to itself).
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_register_app(app: *mut QObject);

        /// Interface language of the vendored pages. Which language is decided by
        /// Rexon (`rexon_core::i18n`, the `ui_language` setting), which catalogue to
        /// load by the core (`rexon_core::remote_l10n`); here there is only Qt: the
        /// QTranslator and re-evaluating the engine's bindings.
        fn rexon_we_apply_ui_language(
            locale_name: &QString,
            qm_path: &QString,
            required: bool,
        ) -> QString;

        include!("rexon_register_we.h");
        /// Language of plugin labels — following the interface language, as in the
        /// original's `App::setUiLanguage`.
        fn rexon_we_set_plugin_locale(locale: &QString);

        fn rexon_we_network_cache_size() -> i64;
        fn rexon_we_network_cache_max() -> i64;
        fn rexon_we_set_network_cache_max(bytes: i64);
        fn rexon_we_clear_network_cache();
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(*mut QObject, renderer_manager, cxx_name = "rendererManager",
                    READ = renderer_manager, CONSTANT)]
        #[qproperty(*mut QObject, display_manager, cxx_name = "displayManager",
                    READ = display_manager, CONSTANT)]
        #[qproperty(*mut QObject, library_manager, cxx_name = "libraryManager",
                    READ = library_manager, CONSTANT)]
        /// The manager itself is one for App's whole life. The property's
        /// notification is ours, in the original it is constant: the card icon
        /// (`GpuTag.qml`) looks the card up with a `find` call inside a binding that
        /// reads this property, and without a signal it would not learn that the
        /// list changed — it would show an empty plate until the page is recreated.
        #[qproperty(*mut QObject, gpu_manager, cxx_name = "gpuManager",
                    READ = gpu_manager, NOTIFY = gpu_manager_changed)]
        /// Warning: both sizes are exposed as `f64`, not `i64`: we do not expose a
        /// 64-bit integer to QML (the analysis is in `src/qvalue.rs`). Cache bytes
        /// come nowhere near the `double` ceiling (2^53 = 9 PB).
        /// The interface language choice is SHARED with Rexon (`ui_language`):
        /// `system`, `en` or a Rexon catalogue code (`ru`).
        #[qproperty(QString, ui_language, cxx_name = "uiLanguage",
                    READ = ui_language, NOTIFY = ui_language_changed)]
        /// List for the "Language" combo box: `{code, label}` maps — the same list
        /// as in Rexon's "Settings" (`qt_settings::language_choices`): "As in the
        /// system", English, then the Rexon languages in their own names.
        /// Notified by the same signal as the choice (the original's
        /// `availableUiLanguages NOTIFY uiLanguageChanged`).
        #[qproperty(QList_QVariant, available_ui_languages,
                    cxx_name = "availableUiLanguages",
                    READ = available_ui_languages, NOTIFY = ui_language_changed)]
        #[qproperty(f64, network_cache_size, cxx_name = "networkCacheSize",
                    READ = network_cache_size, NOTIFY = network_cache_size_changed)]
        #[qproperty(f64, network_cache_maximum_size, cxx_name = "networkCacheMaximumSize",
                    READ = network_cache_maximum_size,
                    NOTIFY = network_cache_maximum_size_changed)]
        /// Current wallpaper displays (`PresentationManager`): from it the tab rail
        /// draws the "Current wallpapers" card (`qml_ui/PageHost.qml`).
        #[qproperty(*mut QObject, presentation_manager, cxx_name = "presentationManager",
                    READ = presentation_manager, CONSTANT)]
        /// Version of the VENDORED interface (`0.5.0`) — the `ui/UPSTREAM_TAG` tag,
        /// parsed in `rexon_core::qml_modules::ui_version`. The original has no
        /// such member: there the "Version %1" line on "About" is written by
        /// `Qt.application.version`, while for us that is the Rexon version (patch
        /// 0002).
        #[qproperty(QString, ui_version, cxx_name = "uiVersion",
                    READ = ui_version, CONSTANT)]
        /// The original has no such member (patch 0010): "Apply" on the wallpaper
        /// card is available even with zero displays where the display is brought
        /// up by Rexon (KDE — the plugin, X11 — the output client host): the frame
        /// is held back, the output comes up, and the wallpaper lands on all
        /// desktops (`waywallen_kde_plugin::apply_without_displays`).
        #[qproperty(bool, apply_without_displays, cxx_name = "applyWithoutDisplays",
                    READ = apply_without_displays, CONSTANT)]
        /// The original has no such member (patch 0010): starting a playlist with
        /// zero displays — the target is "all desktops", the frame is held back,
        /// the output comes up (`waywallen_kde_plugin::play_without_displays`).
        #[qproperty(bool, play_without_displays, cxx_name = "playWithoutDisplays",
                    READ = play_without_displays, CONSTANT)]
        /// Hint of the "Monitors" page with zero displays instead of the vendor's
        /// "install the extension by hand" (KDE) or a bare "No monitors registered"
        /// (X11) — patch 0010; empty means not our case.
        #[qproperty(QString, no_displays_hint, cxx_name = "noDisplaysHint",
                    READ = no_displays_hint, CONSTANT)]
        type App = super::AppRust;
    }

    unsafe extern "RustQt" {
        /// Transport error → a window toast (v0.2.6, `Window.qml onErrorOccurred`,
        /// for us `PageHost.qml`).
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut App>, error: QString);

        /// An informational transport message (not an error): a toast that goes
        /// away by itself after `duration_ms` (`ui_base::toast_timeout_for_text`).
        /// A Rexon signal, the vendor does not have it; `PageHost.qml` listens to
        /// it.
        #[qsignal]
        #[cxx_name = "noticeOccurred"]
        fn notice_occurred(self: Pin<&mut App>, text: QString, duration_ms: i32);

        #[qsignal]
        #[cxx_name = "uiLanguageChanged"]
        fn ui_language_changed(self: Pin<&mut App>);
        #[qsignal]
        #[cxx_name = "gpuManagerChanged"]
        fn gpu_manager_changed(self: Pin<&mut App>);

        #[qsignal]
        #[cxx_name = "networkCacheSizeChanged"]
        fn network_cache_size_changed(self: Pin<&mut App>);

        #[qsignal]
        #[cxx_name = "networkCacheMaximumSizeChanged"]
        fn network_cache_maximum_size_changed(self: Pin<&mut App>);
    }

    unsafe extern "RustQt" {
        #[cxx_name = "rendererManager"]
        fn renderer_manager(self: &App) -> *mut QObject;
        #[cxx_name = "displayManager"]
        fn display_manager(self: &App) -> *mut QObject;
        #[cxx_name = "libraryManager"]
        fn library_manager(self: &App) -> *mut QObject;
        #[cxx_name = "gpuManager"]
        fn gpu_manager(self: &App) -> *mut QObject;
        #[cxx_name = "presentationManager"]
        fn presentation_manager(self: &App) -> *mut QObject;
    }

    extern "RustQt" {
        #[cxx_name = "uiLanguage"]
        fn ui_language(self: &App) -> QString;
        #[cxx_name = "uiVersion"]
        fn ui_version(self: &App) -> QString;
        #[cxx_name = "applyWithoutDisplays"]
        fn apply_without_displays(self: &App) -> bool;
        #[cxx_name = "playWithoutDisplays"]
        fn play_without_displays(self: &App) -> bool;
        #[cxx_name = "noDisplaysHint"]
        fn no_displays_hint(self: &App) -> QString;
        #[cxx_name = "availableUiLanguages"]
        fn available_ui_languages(self: &App) -> QList_QVariant;

        #[cxx_name = "networkCacheSize"]
        fn network_cache_size(self: &App) -> f64;
        #[cxx_name = "networkCacheMaximumSize"]
        fn network_cache_maximum_size(self: &App) -> f64;

        /// Recompute the cache size (the settings page calls it on opening).
        #[qinvokable]
        #[cxx_name = "refreshNetworkCacheSize"]
        fn refresh_network_cache_size(self: Pin<&mut App>);

        /// A port of `App::setNetworkCacheMaximumSize`: a non-positive value and a
        /// repeat are ignored.
        #[qinvokable]
        #[cxx_name = "setNetworkCacheMaximumSize"]
        fn set_network_cache_maximum_size(self: Pin<&mut App>, size: f64);

        #[qinvokable]
        #[cxx_name = "clearNetworkCache"]
        fn clear_network_cache(self: Pin<&mut App>);

        /// Change the interface language (a port of `App::setUiLanguage`). The
        /// choice is written into the Rexon setting `ui_language` — the same one
        /// "Settings" change — the vendored pages are translated at once, the rest
        /// of Rexon after a restart. `false` means the language is not supported or
        /// the catalogue did not load; the vendored QML shows the toast "Failed to
        /// change the language".
        #[qinvokable]
        #[cxx_name = "setUiLanguage"]
        fn set_ui_language(self: Pin<&mut App>, language: &QString) -> bool;

        /// The "single interface window" switch from the settings page (vendor
        /// 0.3.7).
        ///
        /// Warning: a deliberate Rexon divergence. The original takes the bus name
        /// `org.waywallen.waywallen.UI` and exports a `Raise` method: a second
        /// launch of ITS interface does not open a window but raises the already
        /// open one. For us the interface is a tab inside Rexon, and taking this
        /// name would mean taking it away from the real waywallen window: on
        /// start-up it would see the name taken, call `Raise` into nowhere and
        /// silently close without ever opening. Rexon keeps its single window
        /// itself, so the switch only remembers the choice (`Global.qml` stores it
        /// in the settings), and no name is taken on the bus.
        #[qinvokable]
        #[cxx_name = "setSingleUiEnabled"]
        fn set_single_ui_enabled(self: Pin<&mut App>, enabled: bool);

        /// A raw daemon frame from the seam (`rexon_we_do_deliver_event`) — hand it
        /// to the live models. The name must match what
        /// `QMetaObject::invokeMethod` looks for.
        #[qinvokable]
        #[cxx_name = "deliverEvent"]
        fn deliver_event(self: Pin<&mut App>, frame: &QByteArray);

        /// Python client error → the `errorOccurred` signal.
        #[qinvokable]
        #[cxx_name = "clientError"]
        fn client_error(self: Pin<&mut App>, error: &QString);

        /// A full replacement of the renderer list with the `renderer_list` answer.
        ///
        /// The input is the RAW daemon frame — the same one the query itself parsed
        /// (`status_queries::RendererListQuery`). In the original this
        /// synchronization is done by the query itself; for us it is in another
        /// bridge, and the live models of `App` can be reached only through the
        /// seam.
        #[qinvokable]
        #[cxx_name = "replaceRenderers"]
        fn replace_renderers(self: Pin<&mut App>, frame: &QByteArray);

        /// The daemon's answer to a request sent by App itself (`gpu_list`). Called
        /// by the transport seam by request number; the name must match what
        /// `QMetaObject::invokeMethod` looks for.
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut App>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Initialize for App {}
    impl cxx_qt::Threading for App {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use prost::Message;

use rexon_core::proto::control as pb;
use rexon_core::remote_l10n as l10n;

use crate::we_objmodel::qobject::{
    DisplayManager, GpuManager, LibraryManager, PresentationManager, RendererManager,
};
use crate::we_socket::{self, DaemonEvent};

/// The model pointers are raw: Qt owns them (they are children of App and
/// will die with it) — the same scheme as for the queries' owned models.
pub struct AppRust {
    renderer_manager: *mut qobject::QObject,
    display_manager: *mut qobject::QObject,
    library_manager: *mut qobject::QObject,
    gpu_manager: *mut qobject::QObject,
    /// Number of the pending `gpu_list` request (`0` — not waiting). An answer
    /// with another number is from a previous connection, we do not take it.
    gpu_request: i64,
    presentation_manager: *mut qobject::QObject,
    /// Number of the pending `wallpaper_lookup` request for the current
    /// wallpaper card (`0` — not waiting). A new display snapshot starts a new
    /// request, and the answer to the previous one is no longer taken.
    presentation_request: i64,
    /// Subscription to transport events: through it we catch connection and
    /// disconnection.
    subscription: u64,
    /// Size cache: the signal is sent only on a REAL change (the branch of the
    /// original's `App::refreshNetworkCacheSize`).
    network_cache_size: i64,
    /// Interface language choice (`system`, `en`, `ru`) — the Rexon setting
    /// `ui_language`, not a separate one.
    ui_language: String,
    /// Languages that have a built `.qm` catalogue next to the module. Computed
    /// once: the catalogues come with the build and do not appear while
    /// running.
    ui_language_codes: Vec<String>,
    /// The directory where these `.qm` lie (next to the UI module).
    ui_module_dir: String,
    /// Version of the vendored interface (`uiVersion`): read once — the tag
    /// comes with the markup and does not change while running.
    ui_version: String,
}

impl Default for AppRust {
    fn default() -> Self {
        Self {
            renderer_manager: std::ptr::null_mut(),
            display_manager: std::ptr::null_mut(),
            library_manager: std::ptr::null_mut(),
            gpu_manager: std::ptr::null_mut(),
            gpu_request: 0,
            presentation_manager: std::ptr::null_mut(),
            presentation_request: 0,
            subscription: 0,
            network_cache_size: 0,
            ui_language: l10n::UI_LANGUAGE_SYSTEM.to_string(),
            ui_language_codes: Vec::new(),
            ui_module_dir: String::new(),
            ui_version: rexon_core::qml_modules::UPSTREAM_TAG_UNKNOWN.to_string(),
        }
    }
}

impl Drop for AppRust {
    fn drop(&mut self) {
        we_socket::unsubscribe(self.subscription);
    }
}

impl cxx_qt::Initialize for qobject::App {
    fn initialize(mut self: Pin<&mut Self>) {
        let parent: *mut qobject::QObject = {
            let this: &mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
            (this as *mut qobject::App).cast()
        };
        // The models are created in post-construction, not lazily in the getter:
        // QML reads the property on the very first binding, and the getter is
        // const by contract. The order is as in the original's `AppPrivate`.
        let renderers = unsafe { qobject::rexon_new_renderer_manager(parent) };
        let displays = unsafe { qobject::rexon_new_display_manager(parent) };
        let libraries = unsafe { qobject::rexon_new_library_manager(parent) };
        let gpus = unsafe { qobject::rexon_new_gpu_manager(parent) };
        let presentations = unsafe { qobject::rexon_new_presentation_manager(parent) };
        if let Some(manager) = unsafe { pin_as::<PresentationManager>(presentations) } {
            manager.attach();
        }
        {
            let mut rust = self.as_mut().rust_mut();
            rust.renderer_manager = renderers;
            rust.display_manager = displays;
            rust.library_manager = libraries;
            rust.gpu_manager = gpus;
            rust.presentation_manager = presentations;
        }
        // Language of the vendored pages — BEFORE the first QML load: the
        // translator must be in place by the time the engine compiles the page.
        //
        // ⭐ The language is the same one the whole of Rexon speaks in this process
        // (`i18n::current_language`). Previously the tab had its own choice in
        // `QSettings` (`ui/language`) and its own "System" by `QLocale`: with an
        // English Rexon on a Russian system the wallpaper tab stayed Russian, and
        // vice versa. The former `QSettings` key is no longer read.
        let module_dir = l10n::ui_module_dir(&crate::we_env::pkg_dir());
        let codes = discover_ui_languages(&module_dir);
        let spoken = rexon_core::i18n::current_language();
        apply_ui_language(&codes, &module_dir, &spoken);
        let tag_path = rexon_core::qml_modules::qml_path(
            &module_dir,
            rexon_core::qml_modules::UPSTREAM_TAG_REL,
        );
        let ui_version = rexon_core::qml_modules::ui_version(
            std::fs::read_to_string(&tag_path).ok().as_deref(),
        );
        {
            let mut rust = self.as_mut().rust_mut();
            rust.ui_version = ui_version;
            rust.ui_module_dir = module_dir;
            rust.ui_language_codes = codes;
            rust.ui_language = rexon_core::i18n::saved_setting();
        }

        // Register as a frame receiver at once: the daemon sends the display and
        // renderer snapshots once per WS connect.
        unsafe { qobject::rexon_we_register_app(parent) };

        // GPUs: ask at once (App may have been born with the socket already open —
        // then there will be no connection signal) and on every following
        // connection.
        let thread = self.qt_thread();
        let subscription = we_socket::subscribe(move |event| match event {
            DaemonEvent::Connected => {
                // The object may have died — that is no reason to crash.
                let _ = thread.queue(|app| app.request_gpu_list());
            }
            // Connection lost: there are no displays until the daemon sends a new
            // snapshot (in the original the list is cleared by `Backend::disconnected`).
            DaemonEvent::Disconnected { .. } => {
                let _ = thread.queue(|app| app.presentations_disconnected());
            }
            // A transport message for the user (for example, "the live wallpaper did
            // not start — the previous desktop wallpaper is restored") — as an
            // INFORMATIONAL toast that goes away by itself (`PageHost.qml`,
            // `onNoticeOccurred`).
            // Never: previously it went as an error toast: that one has no timeout, and
            // the plate hung for more than a minute over the panel buttons (VM, r1895,
            // 30.10.2026).
            DaemonEvent::Notice(text) => {
                let text = text.clone();
                let ms = rexon_core::ui_base::toast_timeout_for_text(&text);
                let ms = i32::try_from(ms).unwrap_or(i32::MAX);
                let _ = thread.queue(move |app| app.notice_occurred(QString::from(&text), ms));
            }
            _ => {}
        });
        self.as_mut().rust_mut().subscription = subscription;
        self.request_gpu_list();
    }
}

/// Languages of the built catalogues next to the UI module.
///
/// The original takes them from the `:/i18n` resource; for us the `.qm`
/// files lie next to `remote_l10n.py` — we read the directory and leave
/// selection and ordering to the core.
fn discover_ui_languages(module_dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(module_dir) else {
        rexon_core::log_warn!(
            "lang",
            "каталоги переводов вкладки обоев не прочитаны ({module_dir}) — в списке языков останутся «Системный» и English"
        );
        return Vec::new();
    };
    let names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    l10n::ui_language_codes(&names)
}

/// The vendor catalogue language for Rexon language `code` (`ru`, `pt_BR`,
/// `en`): its own catalogue (exactly or by the base language), otherwise
/// English — the vendor's strings are English anyway. There is no "system"
/// here: Rexon has already figured out the system.
fn vendor_language(code: &str, codes: &[String]) -> String {
    l10n::normalize_ui_language(code, codes)
        .filter(|c| c != l10n::UI_LANGUAGE_SYSTEM)
        .unwrap_or_else(|| l10n::UI_LANGUAGE_ENGLISH.to_string())
}

/// Give the wallpaper tab the Rexon language `rexon_language` (`ru`, `de`,
/// `en`, `pt_BR`); `None` means it failed.
///
/// Not "system": what language the system has is decided by Rexon, not by
/// the vendor's `QLocale`. There are two decisions here, and they are
/// DIFFERENT:
///   * the vendored pages' catalogue — the catalogue of the Rexon language,
///     and without one, the source (English) strings ([`vendor_language`]);
///   * plugin labels — the Rexon language as is: plugins have their own
///     catalogues (`i18n/ru.po`, `zh-CN.po`), and whether the vendored pages
///     have a catalogue does not matter to them. Previously the vendor
///     catalogue choice went here, and with a language that has no pages but
///     a translated plugin, the plugin labels stayed English.
fn apply_ui_language(codes: &[String], module_dir: &str, rexon_language: &str) -> Option<String> {
    let preference = vendor_language(rexon_language, codes);
    let plan = l10n::plan_ui_language(&preference, "", codes)?;
    let qm_path = if plan.qm_file.is_empty() {
        String::new()
    } else {
        std::path::Path::new(module_dir)
            .join(&plan.qm_file)
            .to_string_lossy()
            .into_owned()
    };
    let resolved = qobject::rexon_we_apply_ui_language(
        &QString::from(&plan.locale_name),
        &QString::from(&qm_path),
        plan.required,
    );
    if resolved.to_string().is_empty() {
        return None;
    }
    // Plugin labels follow the Rexon language (see above), not the page
    // catalogue; English is the one Qt resolved.
    let plugin_locale = if rexon_language.trim().is_empty() {
        resolved.to_string()
    } else {
        rexon_language.to_string()
    };
    qobject::rexon_we_set_plugin_locale(&QString::from(&plugin_locale));
    // A line of the same purpose as the original's `applyLanguage`: without it
    // "the pages are suddenly in English" is indistinguishable from "the
    // catalogue did not build" and from "the catalogue was not found".
    // Warning: BOTH the Rexon language and the page language are written: the
    // former line "choice=en" with a German Rexon read as "the tab chose
    // English instead of German", although there was nothing to choose from.
    rexon_core::log_info!(
        "lang",
        "язык вкладки обоев: Rexon={} → страницы waywallen={} (Qt: {}), каталог={}; подписи плагинов={}{}",
        rexon_language,
        plan.preference,
        resolved,
        if qm_path.is_empty() {
            "исходные строки".to_string()
        } else {
            qm_path
        },
        plugin_locale,
        if l10n::normalize_ui_language(rexon_language, codes).is_none() {
            " — своего каталога у страниц на этом языке нет, они по-английски"
        } else {
            ""
        }
    );
    Some(plan.preference)
}

/// An element of the language list in the form the vendored QML reads it:
/// `languages[i].code` and `languages[i].label`.
fn language_entry(code: &str, label: &QString) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(
        QString::from("code"),
        QVariant::from(&QString::from(code)),
    );
    map.insert(QString::from("label"), QVariant::from(label));
    QVariant::from(&map)
}

/// # Safety
/// `ptr` is a live manager of the corresponding type, created by the
/// factory.
unsafe fn pin_as<'a, T>(ptr: *mut qobject::QObject) -> Option<Pin<&'a mut T>> {
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { Pin::new_unchecked(&mut *ptr.cast::<T>()) })
}

impl qobject::App {
    fn renderer_manager(&self) -> *mut qobject::QObject {
        self.rust().renderer_manager
    }

    fn display_manager(&self) -> *mut qobject::QObject {
        self.rust().display_manager
    }

    fn library_manager(&self) -> *mut qobject::QObject {
        self.rust().library_manager
    }

    fn gpu_manager(&self) -> *mut qobject::QObject {
        self.rust().gpu_manager
    }

    fn presentation_manager(&self) -> *mut qobject::QObject {
        self.rust().presentation_manager
    }

    /// Displays and canvases — the display manager labels its targets by them.
    fn displays_ref<'a>(&self) -> Option<&'a DisplayManager> {
        let ptr = self.rust().display_manager;
        // SAFETY: the manager is a child of App and lives as long as it does.
        (!ptr.is_null()).then(|| unsafe { &*ptr.cast::<DisplayManager>() })
    }

    /// Ask the daemon for the wallpapers of the current displays
    /// (`wallpaper_lookup`).
    fn request_presentation_wallpapers(mut self: Pin<&mut Self>, ids: Vec<String>) {
        // The previous request, if any, belongs to the previous snapshot.
        self.as_mut().rust_mut().presentation_request = 0;
        if ids.is_empty() || !we_socket::is_open() {
            return;
        }
        let token = crate::discover_queries::next_token();
        let frame = rexon_core::we_client::wallpaper_lookup(token as u64, &ids);
        let owner: *mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
        // The number's owner is remembered BEFORE sending, as for `gpu_list`.
        unsafe { crate::we_transport::remember(token, owner.cast()) };
        self.as_mut().rust_mut().presentation_request = token;
        crate::we_transport::send(token, "wallpaper_lookup", &frame);
    }

    fn presentations_disconnected(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().presentation_request = 0;
        let manager = self.rust().presentation_manager;
        if let Some(manager) = unsafe { pin_as::<PresentationManager>(manager) } {
            manager.clear();
        }
    }

    /// Ask the daemon for the GPU list. The socket is closed — stay silent: the
    /// request will go out on connection.
    fn request_gpu_list(mut self: Pin<&mut Self>) {
        if !we_socket::is_open() {
            return;
        }
        let token = crate::discover_queries::next_token();
        let frame = rexon_core::we_client::gpu_list(token as u64);
        let owner: *mut qobject::App = unsafe { self.as_mut().get_unchecked_mut() };
        // The number's owner is remembered BEFORE sending: the answer may arrive
        // before we return.
        unsafe { crate::we_transport::remember(token, owner.cast()) };
        self.as_mut().rust_mut().gpu_request = token;
        crate::we_transport::send(token, "gpu_list", &frame);
    }

    fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if token != 0 && self.rust().presentation_request == token {
            self.as_mut().rust_mut().presentation_request = 0;
            // A refusal and a disconnection frame carry no wallpapers — the rows keep
            // what is already known.
            let Some(entries) = rexon_core::we_query::wallpaper_lookup_from_frame(frame.as_slice())
            else {
                return;
            };
            let displays = self.displays_ref();
            let manager = self.rust().presentation_manager;
            if let Some(manager) = unsafe { pin_as::<PresentationManager>(manager) } {
                manager.wallpapers_arrived(&entries, displays);
            }
            return;
        }
        if self.rust().gpu_request != token {
            return;
        }
        self.as_mut().rust_mut().gpu_request = 0;
        // A daemon refusal and a disconnection frame carry no list — the previous
        // cards stay in place.
        let Some(gpus) = rexon_core::we_query::gpu_list_from_frame(frame.as_slice()) else {
            return;
        };
        let count = gpus.len();
        let manager = self.rust().gpu_manager;
        let changed = unsafe { pin_as::<GpuManager>(manager) }
            .is_some_and(|manager| manager.replace_all(gpus));
        rexon_core::log_info!(
            "wallpaper-tab",
            "видеокарты от демона: {count}, список {}",
            if changed { "новый" } else { "прежний" }
        );
        if changed {
            // The previous card objects are removed — the icons on live pages must
            // find their cards anew.
            self.as_mut().gpu_manager_changed();
        }
    }

    fn ui_version(&self) -> QString {
        QString::from(&self.rust().ui_version)
    }

    /// The decision is the core's; the session does not change while running
    /// (`CONSTANT`).
    fn apply_without_displays(&self) -> bool {
        rexon_core::waywallen_kde_plugin::apply_without_displays(we_socket::display_setup().rexon_brings_display())
    }

    /// The decision is the core's, the same `apply_gate` as the transport's
    /// (`CONSTANT`).
    fn play_without_displays(&self) -> bool {
        rexon_core::waywallen_kde_plugin::play_without_displays(we_socket::display_setup().rexon_brings_display())
    }

    fn no_displays_hint(&self) -> QString {
        QString::from(&rexon_core::waywallen_kde_plugin::no_displays_hint_in(
            we_socket::display_setup(),
            we_socket::session_backend(),
        ))
    }

    fn ui_language(&self) -> QString {
        QString::from(&self.rust().ui_language)
    }

    fn available_ui_languages(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        // The same list as in Rexon's "Settings": one language for the program.
        for (code, label) in rexon_core::qt_settings::language_choices() {
            out.append(language_entry(&code, &QString::from(&label)));
        }
        out
    }

    fn set_ui_language(mut self: Pin<&mut Self>, language: &QString) -> bool {
        let (codes, module_dir, current) = {
            let rust = self.rust();
            (
                rust.ui_language_codes.clone(),
                rust.ui_module_dir.clone(),
                rust.ui_language.clone(),
            )
        };
        let wanted = rexon_core::i18n::normalize_setting(Some(&language.to_string()));
        if !rexon_core::qt_settings::language_choice_valid(&wanted) {
            return false;
        }
        // A repeat is not an error: the "Reset" button calls
        // `setUiLanguage("system")` even on an already system language, and
        // `false` would abort the whole reset with a toast.
        if wanted == current {
            return true;
        }
        let code = rexon_core::i18n::language_for_setting(Some(&wanted));
        if apply_ui_language(&codes, &module_dir, &code).is_none() {
            return false;
        }
        let saved = rexon_core::settings::SettingsManager::new().and_then(|mut m| {
            m.set(rexon_core::i18n::SETTINGS_KEY, serde_json::Value::from(wanted.as_str()))
        });
        if let Err(e) = saved {
            rexon_core::log_warn!("lang", "язык из вкладки обоев не сохранён в настройки Rexon: {e}");
            return false;
        }
        self.as_mut().rust_mut().ui_language = wanted;
        self.as_mut().ui_language_changed();
        true
    }

    fn network_cache_size(&self) -> f64 {
        crate::qvalue::i64_to_qml(self.rust().network_cache_size)
    }

    fn network_cache_maximum_size(&self) -> f64 {
        crate::qvalue::i64_to_qml(qobject::rexon_we_network_cache_max())
    }

    fn refresh_network_cache_size(mut self: Pin<&mut Self>) {
        let size = qobject::rexon_we_network_cache_size();
        if self.rust().network_cache_size == size {
            return;
        }
        self.as_mut().rust_mut().network_cache_size = size;
        self.as_mut().network_cache_size_changed();
    }

    fn set_network_cache_maximum_size(mut self: Pin<&mut Self>, size: f64) {
        let size = crate::qvalue::qml_to_i64(size);
        if size <= 0 || qobject::rexon_we_network_cache_max() == size {
            return;
        }
        qobject::rexon_we_set_network_cache_max(size);
        self.as_mut().network_cache_maximum_size_changed();
        self.refresh_network_cache_size();
    }

    fn clear_network_cache(self: Pin<&mut Self>) {
        qobject::rexon_we_clear_network_cache();
        self.refresh_network_cache_size();
    }

    /// No name is taken on the bus — why is explained at the declaration in
    /// the bridge.
    fn set_single_ui_enabled(self: Pin<&mut Self>, enabled: bool) {
        let _ = (self, enabled);
    }

    fn client_error(mut self: Pin<&mut Self>, error: &QString) {
        self.as_mut().error_occurred(error.clone());
    }

    fn deliver_event(self: Pin<&mut Self>, frame: &QByteArray) {
        // The frame is parsed ONCE for all three models: the event comes often
        // (the renderer's fps), and three separate protobuf decodes would be
        // triple work for nothing.
        let Ok(server_frame) = pb::ServerFrame::decode(frame.as_slice()) else {
            return;
        };
        let Some(pb::server_frame::Kind::Event(event)) = server_frame.kind else {
            return;
        };
        let rust = self.rust();
        let (renderers, displays, libraries) = (
            rust.renderer_manager,
            rust.display_manager,
            rust.library_manager,
        );
        if let Some(manager) = unsafe { pin_as::<RendererManager>(renderers) } {
            manager.handle_event(&event);
        }
        if let Some(manager) = unsafe { pin_as::<DisplayManager>(displays) } {
            manager.handle_event(&event);
        }
        if let Some(manager) = unsafe { pin_as::<LibraryManager>(libraries) } {
            manager.handle_event(&event);
        }
        // Displays: the snapshot arrives whole (on connection and on every change)
        // and replaces the list; any other event may have changed the name of a
        // display or canvas, and the display targets are labelled by them.
        let presentations = self.rust().presentation_manager;
        let displays_ref = self.displays_ref();
        let Some(mut manager) = (unsafe { pin_as::<PresentationManager>(presentations) }) else {
            return;
        };
        if let Some(pb::event::Payload::WallpaperPresentationSnapshot(snapshot)) = event.payload {
            let ids = manager.as_mut().replace_snapshot(snapshot.presentations, displays_ref);
            self.request_presentation_wallpapers(ids);
        } else if manager.has_presentations() {
            manager.rebuild_rows(displays_ref);
        }
    }

    fn replace_renderers(self: Pin<&mut Self>, frame: &QByteArray) {
        let renderers = self.rust().renderer_manager;
        if let Some(manager) = unsafe { pin_as::<RendererManager>(renderers) } {
            manager.replace_all_frame(frame.as_slice());
        }
    }
}
