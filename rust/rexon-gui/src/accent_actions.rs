// AccentActions — the object of the "Accent colour" block above the
// "Colour schemes" grid.
//
// There is no logic here: snapshot, writing and restoring come from the
// core (`rexon_core::accent`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AccentActions = super::AccentActionsRust;
    }

    extern "RustQt" {
        /// Block snapshot (JSON `accent::AccentState`).
        #[qinvokable]
        fn state(self: &AccentActions) -> QString;

        /// Choice: `scheme`, `wallpaper` or a `#rrggbb` colour. The answer is JSON
        /// `{"ok": bool, "message": "..."}`.
        #[qinvokable]
        fn apply(self: &AccentActions, choice: &QString) -> QString;

        /// "Restore as it was". The answer is as for `apply`.
        #[qinvokable]
        fn revert(self: &AccentActions) -> QString;

        /// Open the environment settings (COSMIC).
        #[qinvokable]
        #[cxx_name = "openNative"]
        fn open_native(self: &AccentActions) -> bool;
    }
}

use rexon_core::accent;

#[derive(Default)]
pub struct AccentActionsRust;

fn answer((ok, message): (bool, String)) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&serde_json::json!({ "ok": ok, "message": message }).to_string())
}

impl qobject::AccentActions {
    fn state(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&serde_json::to_string(&accent::live_state()).unwrap_or_default())
    }

    fn apply(&self, choice: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        answer(accent::live_apply(&choice.to_string()))
    }

    fn revert(&self) -> cxx_qt_lib::QString {
        answer(accent::live_revert())
    }

    fn open_native(&self) -> bool {
        accent::live_open_native()
    }
}
