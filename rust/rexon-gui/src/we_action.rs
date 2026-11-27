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
    }
}

use core::pin::Pin;
use cxx_qt_lib::QString;

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
}
