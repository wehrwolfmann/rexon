// AccentActions — объект блока «Цвет акцента» над сеткой «Цветовых схем».
//
// Логики здесь нет: снимок, запись и возврат — ядро (`rexon_core::accent`).

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
        /// Снимок блока (JSON `accent::AccentState`).
        #[qinvokable]
        fn state(self: &AccentActions) -> QString;

        /// Выбор: `scheme`, `wallpaper` или цвет `#rrggbb`. Ответ — JSON
        /// `{"ok": bool, "message": "..."}`.
        #[qinvokable]
        fn apply(self: &AccentActions, choice: &QString) -> QString;

        /// «Вернуть как было». Ответ — как у `apply`.
        #[qinvokable]
        fn revert(self: &AccentActions) -> QString;

        /// Открыть настройки окружения (COSMIC).
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
