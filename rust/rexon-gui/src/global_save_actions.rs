// GlobalSaveActions — the "Save current look…" window above the "Global
// themes" grid: a global theme of your own from what is set right now.
//
// There is no logic here: snapshot, labels, name validation, package and
// preview come from the core (`rexon_core::global_save`). The screenshot
// waits for the user (a portal with area selection), so it runs in a
// worker thread, and the result comes with the `screenshotReady` signal.

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
        /// Window (JSON `global_save::SaveDialog`).
        #[qinvokable]
        fn state(self: &GlobalSaveActions) -> QString;

        /// What is wrong with the name; empty means it is fine.
        #[qinvokable]
        #[cxx_name = "nameProblem"]
        fn name_problem(self: &GlobalSaveActions, name: &QString) -> QString;

        /// A tile in the theme colours — the path to a PNG; empty means it failed.
        #[qinvokable]
        fn tile(self: &GlobalSaveActions) -> QString;

        /// Save (JSON `global_save::SaveRequest`). The answer is JSON
        /// `global_save::SaveResult`.
        #[qinvokable]
        fn save(self: &GlobalSaveActions, request: &QString) -> QString;

        /// Take a screenshot. The result comes with the `screenshotReady` signal.
        #[qinvokable]
        #[cxx_name = "takeScreenshot"]
        fn take_screenshot(self: Pin<&mut GlobalSaveActions>);

        /// Screenshot result: a file path or a reason (both empty means the user
        /// cancelled).
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
