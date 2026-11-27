// DayNightActions — the object of the "Light and dark" block above the
// "Global themes" (Plasma) and "Colour schemes" grids.
//
// There is no logic here: snapshot, validation, writing, the timer and
// applying come from the core (`rexon_core::day_night`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type DayNightActions = super::DayNightActionsRust;
    }

    extern "RustQt" {
        /// Block snapshot (JSON `day_night::State`).
        #[qinvokable]
        fn state(self: &DayNightActions) -> QString;

        /// Save the setting (JSON `day_night::Config`) and apply it. The answer is
        /// JSON `{"ok": bool, "message": "..."}`.
        #[qinvokable]
        fn save(self: &DayNightActions, config: &QString) -> QString;

        /// Remember whether the block is expanded (`settings.json`). `false` means
        /// it was not written.
        #[qinvokable]
        #[cxx_name = "setExpanded"]
        fn set_expanded(self: &DayNightActions, on: bool) -> bool;
    }
}

use rexon_core::day_night;

#[derive(Default)]
pub struct DayNightActionsRust;

impl qobject::DayNightActions {
    fn state(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&serde_json::to_string(&day_night::live_state()).unwrap_or_default())
    }

    fn save(&self, config: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let (ok, message) = day_night::live_save(&config.to_string());
        cxx_qt_lib::QString::from(&serde_json::json!({ "ok": ok, "message": message }).to_string())
    }

    fn set_expanded(&self, on: bool) -> bool {
        day_night::live_set_expanded(on)
    }
}
