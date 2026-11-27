// DdeLookActions — the object of the "Deepin windows" section of the
// "Settings" tab: an explanation of where window corners, transparency and
// accent are changed in Deepin.
//
// There is no logic here: the snapshot comes from the core
// (`rexon_core::dde::read_state`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type DdeLookActions = super::DdeLookActionsRust;
    }

    extern "RustQt" {
        /// Section snapshot (JSON `dde::LookState`).
        #[qinvokable]
        fn state(self: &DdeLookActions) -> QString;
    }
}

use rexon_core::animations::LookCtx;
use rexon_core::dde;

#[derive(Default)]
pub struct DdeLookActionsRust;

impl qobject::DdeLookActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let de = LookCtx::from_env().de;
        // Treeland detection — only in a Deepin session: outside it the section is
        // hidden.
        let state = dde::read_state(&de, de == "deepin" && dde::treeland_session_live());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }
}
