// LaunchFeedbackActions — объект раздела «Отклик при запуске» вкладки
// «Настройки» (курсор и панель задач, пока программа запускается).
//
// Логики здесь нет: снимок и запись — ядро (`rexon_core::launch_feedback`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type LaunchFeedbackActions = super::LaunchFeedbackActionsRust;
    }

    extern "RustQt" {
        /// Снимок раздела (JSON `launch_feedback::LaunchFeedbackState`).
        #[qinvokable]
        fn state(self: &LaunchFeedbackActions) -> QString;

        /// Вид курсора: `none` | `static` | `blinking` | `bouncing`.
        #[qinvokable]
        #[cxx_name = "setCursor"]
        fn set_cursor(self: &LaunchFeedbackActions, style: &QString) -> QString;

        /// Анимация на панели задач.
        #[qinvokable]
        #[cxx_name = "setTaskbar"]
        fn set_taskbar(self: &LaunchFeedbackActions, on: bool) -> QString;

        /// Через сколько секунд остановить анимацию.
        #[qinvokable]
        #[cxx_name = "setTimeout"]
        fn set_timeout(self: &LaunchFeedbackActions, seconds: i32) -> QString;

        /// «По умолчанию».
        #[qinvokable]
        fn defaults(self: &LaunchFeedbackActions) -> QString;
    }
}

use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::launch_feedback as lf;

#[derive(Default)]
pub struct LaunchFeedbackActionsRust;

impl qobject::LaunchFeedbackActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = lf::read_state(&SystemIo, &LookCtx::from_env());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn set_cursor(&self, style: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&lf::apply_cursor(&SystemIo, &LookCtx::from_env(), &style.to_string()))
    }

    fn set_taskbar(&self, on: bool) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&lf::apply_taskbar(&SystemIo, &LookCtx::from_env(), on))
    }

    fn set_timeout(&self, seconds: i32) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&lf::apply_timeout(&SystemIo, &LookCtx::from_env(), seconds.max(0) as u32))
    }

    fn defaults(&self) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&lf::apply_defaults(&SystemIo, &LookCtx::from_env()))
    }
}
