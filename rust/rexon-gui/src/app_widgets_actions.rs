// AppWidgetsActions — the object of the "Icons and toolbars" section of
// the "Settings" tab: icons in menus and on buttons, toolbar labels.
//
// There is no logic here: snapshot and writing come from the core
// (`rexon_core::app_widgets`).

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
        /// Section snapshot (JSON `app_widgets::WidgetsState`).
        #[qinvokable]
        fn state(self: &AppWidgetsActions) -> QString;

        /// Icons in menus.
        #[qinvokable]
        #[cxx_name = "setMenuIcons"]
        fn set_menu_icons(self: &AppWidgetsActions, on: bool) -> QString;

        /// Icons on buttons.
        #[qinvokable]
        #[cxx_name = "setButtonIcons"]
        fn set_button_icons(self: &AppWidgetsActions, on: bool) -> QString;

        /// Labels of the main toolbar (`icons`/`text`/`beside`/`under`); outside KDE
        /// — of all toolbars.
        #[qinvokable]
        #[cxx_name = "setToolbar"]
        fn set_toolbar(self: &AppWidgetsActions, id: &QString) -> QString;

        /// Labels of the other toolbars (KDE only).
        #[qinvokable]
        #[cxx_name = "setOtherToolbars"]
        fn set_other_toolbars(self: &AppWidgetsActions, id: &QString) -> QString;

        /// "Defaults".
        #[qinvokable]
        fn defaults(self: &AppWidgetsActions) -> QString;
    }
}

use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::app_widgets::{self as aw, Toolbar, WidgetsChange};

#[derive(Default)]
pub struct AppWidgetsActionsRust;

fn apply(change: WidgetsChange) -> cxx_qt_lib::QString {
    cxx_qt_lib::QString::from(&aw::apply(&SystemIo, &LookCtx::for_look(), &change))
}

fn toolbar(id: &cxx_qt_lib::QString) -> Option<Toolbar> {
    Toolbar::from_id(&id.to_string())
}

impl qobject::AppWidgetsActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let theme = std::env::var("QT_QPA_PLATFORMTHEME").unwrap_or_default();
        let state = aw::read_state(&SystemIo, &LookCtx::for_look(), &theme);
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
        cxx_qt_lib::QString::from(&aw::apply_defaults(&SystemIo, &LookCtx::for_look()))
    }
}
