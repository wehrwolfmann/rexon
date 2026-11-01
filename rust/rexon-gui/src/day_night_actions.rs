// DayNightActions — объект блока «Светлое и тёмное» над сетками «Глобальных
// тем» (Plasma) и «Цветовых схем».
//
// Логики здесь нет: снимок, проверка, запись, таймер и применение — ядро
// (`rexon_core::day_night`).

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
        /// Снимок блока (JSON `day_night::State`).
        #[qinvokable]
        fn state(self: &DayNightActions) -> QString;

        /// Сохранить настройку (JSON `day_night::Config`) и применить. Ответ —
        /// JSON `{"ok": bool, "message": "..."}`.
        #[qinvokable]
        fn save(self: &DayNightActions, config: &QString) -> QString;

        /// Запомнить, раскрыт ли блок (`settings.json`). `false` — не
        /// записалось.
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
