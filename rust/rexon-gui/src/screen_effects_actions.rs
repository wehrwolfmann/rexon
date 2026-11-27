// ScreenEffectsActions — объект раздела «Эффекты экрана» вкладки «Оформление».
//
// Логики здесь нет: снимок, таблицу поддержки и запись делает ядро
// (`rexon_core::screen_effects`). Объект только переводит вызовы QML в
// вызовы ядра и отдаёт готовые строки (JSON снимка, фраза итога). Запись
// короткая (файл шейдера, наш файл конфига, `hyprctl reload`), поэтому идёт
// сразу, без рабочего потока — как у «Анимации».

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type ScreenEffectsActions = super::ScreenEffectsActionsRust;
    }

    extern "RustQt" {
        /// Снимок раздела (JSON `screen_effects::ScreenEffectsState`).
        #[qinvokable]
        fn state(self: &ScreenEffectsActions) -> QString;

        /// «Применить»: пресет по имени (пусто — выключить), сила 0…1,
        /// скорость 0…`max_speed`. Ответ — фраза для плашки.
        #[qinvokable]
        fn apply(self: &ScreenEffectsActions, preset: &QString, strength: f64, speed: f64) -> QString;
    }
}

use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::screen_effects as se;

#[derive(Default)]
pub struct ScreenEffectsActionsRust;

impl qobject::ScreenEffectsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = se::read_state(&SystemIo, &LookCtx::from_env());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn apply(&self, preset: &cxx_qt_lib::QString, strength: f64, speed: f64) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&se::apply_from_ui(
            &SystemIo,
            &LookCtx::from_env(),
            &preset.to_string(),
            strength,
            speed,
        ))
    }
}
