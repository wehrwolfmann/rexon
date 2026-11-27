// Action — the QML singleton of shared actions of the Wallpaper Engine
// tab.
//
// A port of waywallen's `ui/src/action.cpp` (MIT, (c) hypengw; attribution
// in NOTICE).
//
// Warning: two fixes compared with our Python bridge — the reference is
// the ORIGINAL:
//
//   1. In the original `toast` is a SIGNAL with default arguments, listened
//      to by the vendored `Window.qml:86` (`function onToast(text,
//      duration, flags, action)`). The Python bridge renamed the signal to
//      `toasted` and made `toast` a slot, and the vendor's handler NEVER
//      fired: only our shell saw the toasts. Here `toast` is a signal again
//      with the same defaults — it lives in the moc-processed base
//      `cpp/rexon_we_action_base.h` (why exactly so is analysed in that
//      file's header).
//   2. The Python bridge had NO `copyToClipboard` AT ALL, while the vendored
//      `Global.qml:44` calls it on the "Copy" button of an error toast: the
//      press failed with a TypeError and the text did not reach the
//      clipboard.
//
// A third, minor one: in the original `enterWallpaperSelect` emits
// `wallpaperSelectStorageChanged` ONLY when the storage changes, ours
// emitted it always (an extra binding recomputation on every entry into
// selection mode).

/// cxx-qt bridge: the `Action` singleton.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("rexon_we_action_base.h");
        /// Base with the `toast` signal and its default arguments.
        ///
        /// Warning: without `#[qobject]` — for every `#[qobject]` type in
        /// `extern "C++"` cxx-qt emits a pair of cast shims keyed by the TYPE name,
        /// and a second such bridge in the crate would give a `duplicate symbol` at
        /// link time (the same trap as with the model base).
        type RexonWeActionBase;
    }

    unsafe extern "C++" {
        // The QObject has to be declared by hand: cxx-qt emits the declaration by
        // itself only for QObjects without a base, and Action has its own base
        // (see above).
        #[namespace = ""]
        type QObject = cxx_qt::QObject;

        include!("rexon_we_singletons.h");

        /// Remember the selection storage under a QPointer.
        ///
        /// Not a struct field: in the original this is a `QPointer<QObject>`, and
        /// not for decoration — the storage lives in a QML page and dies together
        /// with it, while the singleton outlives the page. A bare pointer would
        /// then dangle, and the very first property read from QML would crash the
        /// tab.
        ///
        /// # Safety
        /// `storage` is either nullptr or a live QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_we_action_set_storage(storage: *mut QObject);
        fn rexon_we_action_storage() -> *mut QObject;

        /// Text to the clipboard (QGuiApplication::clipboard()).
        fn rexon_we_copy_to_clipboard(text: &QString);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[base = RexonWeActionBase]
        #[qproperty(*mut QObject, wallpaper_select_storage,
                    cxx_name = "wallpaperSelectStorage",
                    READ = wallpaper_select_storage,
                    NOTIFY = wallpaper_select_storage_changed)]
        type Action = super::ActionRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "wallpaperSelectStorageChanged"]
        fn wallpaper_select_storage_changed(self: Pin<&mut Action>);

        #[qsignal]
        #[cxx_name = "wallpaperSelectEntered"]
        unsafe fn wallpaper_select_entered(self: Pin<&mut Action>, storage: *mut QObject);

        /// waywallen 0.6.2: the answer to `usePlasmaWallpaperPlugin()`; `error` is
        /// empty on success.
        #[qsignal]
        #[cxx_name = "plasmaWallpaperPluginSet"]
        fn plasma_wallpaper_plugin_set(self: Pin<&mut Action>, error: QString);
    }

    unsafe extern "RustQt" {
        #[cxx_name = "wallpaperSelectStorage"]
        fn wallpaper_select_storage(self: &Action) -> *mut QObject;

        /// Enter wallpaper selection mode with this storage.
        #[qinvokable]
        #[cxx_name = "enterWallpaperSelect"]
        unsafe fn enter_wallpaper_select(self: Pin<&mut Action>, storage: *mut QObject);
    }

    extern "RustQt" {
        /// Copy text to the clipboard (the error toast button).
        #[qinvokable]
        #[cxx_name = "copyToClipboard"]
        fn copy_to_clipboard(self: Pin<&mut Action>, text: &QString);

        /// waywallen 0.6.2: the org.waywallen.kde wallpaper plugin is installed
        /// (the user's copy or a package).
        #[qinvokable]
        #[cxx_name = "hasPlasmaWallpaperPlugin"]
        fn has_plasma_wallpaper_plugin(self: &Action) -> bool;

        /// waywallen 0.6.2: ask plasmashell to use the plugin on every desktop.
        /// The answer comes as `plasmaWallpaperPluginSet`.
        #[qinvokable]
        #[cxx_name = "usePlasmaWallpaperPlugin"]
        fn use_plasma_wallpaper_plugin(self: Pin<&mut Action>);
    }

    impl cxx_qt::Threading for Action {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use rexon_core::waywallen_kde_plugin as wkp;

/// The singleton has no state of its own: the original's only field is a
/// `QPointer` to the selection storage, and the QPointer lives in the shim
/// (see the header).
#[derive(Default)]
pub struct ActionRust;

impl qobject::Action {
    pub fn wallpaper_select_storage(&self) -> *mut qobject::QObject {
        qobject::rexon_we_action_storage()
    }

    /// # Safety
    /// `storage` is either nullptr or a live QObject: the shim puts it under a
    /// `QPointer`, which reads the metaobject at that address.
    pub unsafe fn enter_wallpaper_select(mut self: Pin<&mut Self>, storage: *mut qobject::QObject) {
        // The change signal — only on a REAL change (the original's branch).
        if qobject::rexon_we_action_storage() != storage {
            unsafe { qobject::rexon_we_action_set_storage(storage) };
            self.as_mut().wallpaper_select_storage_changed();
        }
        // unsafe: the argument is a raw pointer; the call itself dereferences
        // nothing.
        unsafe { self.as_mut().wallpaper_select_entered(storage) };
    }

    pub fn copy_to_clipboard(self: Pin<&mut Self>, text: &QString) {
        qobject::rexon_we_copy_to_clipboard(text);
    }

    pub fn has_plasma_wallpaper_plugin(&self) -> bool {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        wkp::plugin_installed(&env("HOME"), &env("XDG_DATA_HOME"), &env("XDG_DATA_DIRS"), &|p| {
            std::path::Path::new(p).is_file()
        })
    }

    pub fn use_plasma_wallpaper_plugin(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let error = select_plugin_everywhere();
            let _ = handle.queue(move |object| {
                object.plasma_wallpaper_plugin_set(QString::from(&error));
            });
        });
    }
}

/// Assign the plugin on every desktop over the bus (the original's
/// `evaluateScript` call with a 10 s timeout). Empty — success, else the error
/// text; like the original, the bus tool's own message is passed on as is.
fn select_plugin_everywhere() -> String {
    use rexon_core::utils;
    let Some((kind, exe)) = utils::DBUS_TOOL_PREFERENCE
        .iter()
        .find_map(|(tool, kind)| utils::resolve_tool(tool, &[], None).map(|exe| (*kind, exe)))
    else {
        return utils::msg_no_dbus_tool(&["qdbus6"]);
    };
    let (dest, path, method) = utils::PLASMA_EVALUATE_SCRIPT;
    let js = wkp::select_plugin_js();
    let argv = utils::dbus_call_argv(kind, &exe, dest, path, method, &[utils::DbusArg::Str(js)]);
    match rexon_core::splash_exec::run_capture(&argv, 10.0) {
        Some(out) if out.rc == 0 => String::new(),
        Some(out) if !out.stderr.trim().is_empty() => out.stderr.trim().to_string(),
        _ => rexon_core::i18n::tr("the process did not start or did not respond in time"),
    }
}
