// AppWidgetsActions — объект раздела «Значки и панели инструментов» вкладки
// «Настройки»: значки в меню и на кнопках, подписи панелей инструментов.
//
// Логики здесь нет: снимок и запись — ядро (`rexon_core::app_widgets`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type AppWidgetsActions = super::AppWidgetsActionsRust;
    }

    extern "RustQt" {
        /// Снимок раздела (JSON `app_widgets::WidgetsState`).
        #[qinvokable]
        fn state(self: &AppWidgetsActions) -> QString;

        /// Значки в меню.
        #[qinvokable]
        #[cxx_name = "setMenuIcons"]
        fn set_menu_icons(self: &AppWidgetsActions, on: bool) -> QString;

        /// Значки на кнопках.
        #[qinvokable]
        #[cxx_name = "setButtonIcons"]
        fn set_button_icons(self: &AppWidgetsActions, on: bool) -> QString;

        /// Подписи основной панели (`icons`/`text`/`beside`/`under`); вне KDE —
        /// всех панелей.
        #[qinvokable]
        #[cxx_name = "setToolbar"]
        fn set_toolbar(self: &AppWidgetsActions, id: &QString) -> QString;

        /// Подписи прочих панелей (только KDE).
        #[qinvokable]
        #[cxx_name = "setOtherToolbars"]
        fn set_other_toolbars(self: &AppWidgetsActions, id: &QString) -> QString;

        /// «По умолчанию».
        #[qinvokable]
        fn defaults(self: &AppWidgetsActions) -> QString;
    }
}

use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::app_widgets::{self as aw, Toolbar, WidgetsChange};

#[derive(Default)]
pub struct AppWidgetsActionsRust;

fn apply(change: WidgetsChange) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&aw::apply(&SystemIo, &LookCtx::from_env(), &change))
}

fn toolbar(id: &cxx_qt_lib::QString) -> Option<Toolbar> {
    Toolbar::from_id(&id.to_string())
}

impl qobject::AppWidgetsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let theme = std::env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default();
        let state = aw::read_state(&SystemIo, &LookCtx::from_env(), &theme);
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_menu_icons(&self, on: bool) -> cxx_qt_lib::QString {
        apply(WidgetsChange::MenuIcons(on))
    }

    fn set_button_icons(&self, on: bool) -> cxx_qt_lib::QString {
        apply(WidgetsChange::ButtonIcons(on))
    }

    fn set_toolbar(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        toolbar(id).map(|t| apply(WidgetsChange::Toolbar(t))).unwrap_or_default()
    }

    fn set_other_toolbars(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        toolbar(id).map(|t| apply(WidgetsChange::OtherToolbars(t))).unwrap_or_default()
    }

    fn defaults(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&aw::apply_defaults(&SystemIo, &LookCtx::from_env()))
    }
}
