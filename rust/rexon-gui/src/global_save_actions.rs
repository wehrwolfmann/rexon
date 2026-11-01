// GlobalSaveActions — окно «Сохранить текущее оформление…» над сеткой
// «Глобальных тем»: своя глобальная тема из того, что стоит сейчас.
//
// Логики здесь нет: снимок, подписи, проверка имени, пакет и превью — ядро
// (`rexon_core::global_save`). Снимок экрана ждёт человека (портал с
// выбором области), поэтому идёт в рабочем потоке, итог — сигналом
// `screenshotReady`.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type GlobalSaveActions = super::GlobalSaveActionsRust;
    }

    extern "RustQt" {
        /// Окно (JSON `global_save::SaveDialog`).
        #[qinvokable]
        fn state(self: &GlobalSaveActions) -> QString;

        /// Что не так с именем; пусто — годится.
        #[qinvokable]
        #[cxx_name = "nameProblem"]
        fn name_problem(self: &GlobalSaveActions, name: &QString) -> QString;

        /// Плитка цветами темы — путь к PNG; пусто — не вышло.
        #[qinvokable]
        fn tile(self: &GlobalSaveActions) -> QString;

        /// Сохранить (JSON `global_save::SaveRequest`). Ответ — JSON
        /// `global_save::SaveResult`.
        #[qinvokable]
        fn save(self: &GlobalSaveActions, request: &QString) -> QString;

        /// Снять экран. Итог — сигналом `screenshotReady`.
        #[qinvokable]
        #[cxx_name = "takeScreenshot"]
        fn take_screenshot(self: Pin<&mut GlobalSaveActions>);

        /// Итог снимка: путь к файлу либо причина (пусто и то и другое —
        /// человек отменил).
        #[qsignal]
        #[cxx_name = "screenshotReady"]
        fn screenshot_ready(self: Pin<&mut GlobalSaveActions>, path: QString, error: QString);
    }

    impl cxx_qt::Threading for GlobalSaveActions {}
}

use core::pin::Pin;

use cxx_qt::Threading;
use cxx_qt_lib::QString;
use rexon_core::global_save as gs;

#[derive(Default)]
pub struct GlobalSaveActionsRust;

impl qobject::GlobalSaveActions {
    fn state(&self) -> QString {
        QString::from(&serde_json::to_string(&gs::live_dialog()).unwrap_or_default())
    }

    fn name_problem(&self, name: &QString) -> QString {
        QString::from(&gs::live_name_problem(&name.to_string()))
    }

    fn tile(&self) -> QString {
        match gs::live_tile() {
            Ok(path) => QString::from(&path),
            Err(e) => {
                rexon_core::log_warn!("theme", "плитка превью не нарисована: {e}");
                QString::default()
            }
        }
    }

    fn save(&self, request: &QString) -> QString {
        QString::from(&serde_json::to_string(&gs::live_save(&request.to_string())).unwrap_or_default())
    }

    fn take_screenshot(self: Pin<&mut Self>) {
        let handle = self.qt_thread();
        std::thread::spawn(move || {
            let (path, error) = match gs::live_screenshot() {
                Ok(p) => (p, String::new()),
                Err(e) => (String::new(), e),
            };
            let _ = handle.queue(move |mut object| {
                object.as_mut().screenshot_ready(QString::from(&path), QString::from(&error));
            });
        });
    }
}
