// WindowButtonsActions — объект раздела «Кнопки заголовка окна» вкладки
// «Оформление» (раскладка кнопок перетаскиванием и галочками, флажки
// заголовка KDE и размер рамки).
//
// Логики здесь нет: снимок и запись — ядро (`rexon_core::window_buttons`),
// тот же путь, что у стенда `de_matrix_stand`.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type WindowButtonsActions = super::WindowButtonsActionsRust;
    }

    extern "RustQt" {
        /// Снимок раздела (JSON `window_buttons::ButtonsState`).
        #[qinvokable]
        fn state(self: &WindowButtonsActions) -> QString;

        /// Готовая раскладка: сторона кнопок управления (`right`/`left`) и
        /// набор кнопок через запятую.
        #[qinvokable]
        #[cxx_name = "setLayout"]
        fn set_layout(self: &WindowButtonsActions, side: &QString, buttons: &QString) -> QString;

        /// Поставить (`on`) или снять кнопку `id` на стороне `side`
        /// (`left`/`right`); остальные кнопки остаются, где стоят.
        #[qinvokable]
        #[cxx_name = "toggleButton"]
        fn toggle_button(self: &WindowButtonsActions, side: &QString, id: &QString, on: bool) -> QString;

        /// Перетаскивание: кнопку `id` из `from` (`left`/`right` с позицией
        /// `from_index` или `shelf`) на `to` в позицию `to_index` (место в
        /// стороне уже без кнопки) или на полку.
        #[qinvokable]
        #[cxx_name = "moveButton"]
        fn move_button(
            self: &WindowButtonsActions,
            id: &QString,
            from: &QString,
            from_index: i32,
            to: &QString,
            to_index: i32,
        ) -> QString;

        /// Шаг кнопкой «влево / вправо / на полку» (`left`/`right`/`shelf`):
        /// JSON `{message, at: [сторона, позиция] | null}`.
        #[qinvokable]
        #[cxx_name = "nudgeButton"]
        fn nudge_button(self: &WindowButtonsActions, side: &QString, index: i32, dir: &QString) -> QString;

        /// «Добавить слева / справа» с полки.
        #[qinvokable]
        #[cxx_name = "addButton"]
        fn add_button(self: &WindowButtonsActions, id: &QString, side: &QString) -> QString;

        /// Размер рамки (id варианта из снимка).
        #[qinvokable]
        #[cxx_name = "setBorder"]
        fn set_border(self: &WindowButtonsActions, value: &QString) -> QString;

        /// Флажок заголовка KDE (id из снимка, `flags`).
        #[qinvokable]
        #[cxx_name = "setFlag"]
        fn set_flag(self: &WindowButtonsActions, id: &QString, on: bool) -> QString;

        /// «По умолчанию».
        #[qinvokable]
        fn defaults(self: &WindowButtonsActions) -> QString;
    }
}

use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::window_buttons as wb;

#[derive(Default)]
pub struct WindowButtonsActionsRust;

impl qobject::WindowButtonsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = wb::read_state(&SystemIo, &LookCtx::from_env());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_layout(&self, side: &cxx_qt_lib::QString, buttons: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let buttons: Vec<String> = buttons.to_string().split(',').filter(|b| !b.is_empty()).map(String::from).collect();
        cxx_qt_lib::QString::from(&wb::apply_preset(&SystemIo, &LookCtx::from_env(), &side.to_string(), &buttons))
    }

    fn toggle_button(&self, side: &cxx_qt_lib::QString, id: &cxx_qt_lib::QString, on: bool) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&wb::apply_toggle(&SystemIo, &LookCtx::from_env(), &side.to_string(), &id.to_string(), on))
    }

    fn move_button(
        &self,
        id: &cxx_qt_lib::QString,
        from: &cxx_qt_lib::QString,
        from_index: i32,
        to: &cxx_qt_lib::QString,
        to_index: i32,
    ) -> cxx_qt_lib::QString {
        let msg = wb::apply_move(
            &SystemIo,
            &LookCtx::from_env(),
            &id.to_string(),
            &from.to_string(),
            from_index.max(0) as usize,
            &to.to_string(),
            to_index.max(0) as usize,
        );
        cxx_qt_lib::QString::from(&msg)
    }

    fn nudge_button(&self, side: &cxx_qt_lib::QString, index: i32, dir: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let (message, at) =
            wb::apply_nudge(&SystemIo, &LookCtx::from_env(), &side.to_string(), index.max(0) as usize, &dir.to_string());
        cxx_qt_lib::QString::from(&serde_json::json!({ "message": message, "at": at }).to_string())
    }

    fn add_button(&self, id: &cxx_qt_lib::QString, side: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&wb::apply_add(&SystemIo, &LookCtx::from_env(), &id.to_string(), &side.to_string()))
    }

    fn set_border(&self, value: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&wb::apply_border(&SystemIo, &LookCtx::from_env(), &value.to_string()))
    }

    fn set_flag(&self, id: &cxx_qt_lib::QString, on: bool) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&wb::apply_flag(&SystemIo, &LookCtx::from_env(), &id.to_string(), on))
    }

    fn defaults(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&wb::apply_defaults(&SystemIo, &LookCtx::from_env()))
    }
}
