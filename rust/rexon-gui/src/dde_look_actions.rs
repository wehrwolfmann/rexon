// DdeLookActions — объект раздела «Окна Deepin» вкладки «Настройки»:
// объяснение, где в Deepin меняются углы, прозрачность и акцент окон.
//
// Логики здесь нет: снимок — ядро (`rexon_core::dde::read_state`).

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
        /// Снимок раздела (JSON `dde::LookState`).
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
        // Опознание Treeland — только в сеансе Deepin: вне его раздел скрыт.
        let state = dde::read_state(&de, de == "deepin" && dde::treeland_session_live());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }
}
