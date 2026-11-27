// MatePanelActions — the object of the "MATE panel" section (ready panel
// layouts). The section is self-contained: it can be put into any tab
// ("Settings" now, "Appearance" later) without rework.
//
// There is no logic here: snapshot, labels, applying and restoring come
// from the core (`rexon_core::mate_panel`).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type MatePanelActions = super::MatePanelActionsRust;
    }

    impl cxx_qt::Threading for MatePanelActions {}

    extern "RustQt" {
        /// Section snapshot (JSON `mate_panel::PanelState`).
        #[qinvokable]
        fn state(self: &MatePanelActions) -> QString;

        /// Confirmation text before applying layout `id`, and with an empty `id` —
        /// before restoring the previous panel (JSON `{title, question}`).
        #[qinvokable]
        fn confirm(self: &MatePanelActions, id: &QString) -> QString;

        /// Apply layout `id` → a message for the user.
        #[qinvokable]
        fn apply(self: &MatePanelActions, id: &QString) -> QString;

        /// The question "build the missing parts of the layout from AUR?" (JSON
        /// `{title, question}`; `{}` means there is nothing to ask: packages from
        /// the repositories are installed without a question).
        #[qinvokable]
        #[cxx_name = "partsQuestion"]
        fn parts_question(self: &MatePanelActions, id: &QString) -> QString;

        /// Apply the layout in the background. Missing parts from the repositories
        /// are always installed (the password window); `install` also builds parts
        /// from AUR (the shared build queue; the user agreed in the question). Each
        /// part on its own: what did not install is replaced as described under the
        /// layout (`mate_panel::apply_with_parts`). The result comes with the
        /// `applied` signal.
        #[qinvokable]
        #[cxx_name = "applyAsync"]
        fn apply_async(self: Pin<&mut MatePanelActions>, id: &QString, install: bool);

        /// Applying has finished: JSON `mate_panel::PartsApplied` — the message,
        /// whether to offer "Retry", the build log for "Open log".
        #[qsignal]
        fn applied(self: Pin<&mut MatePanelActions>, result: QString);

        /// Open a build log the way the system opens text files ("Open log").
        /// `false` — there is no such file.
        #[qinvokable]
        #[cxx_name = "openLog"]
        fn open_log(self: &MatePanelActions, path: &QString) -> bool;

        /// Restore the original panel in the background: restoring waits until the
        /// panel removes its applets (up to 3 s), and the interface does not freeze
        /// meanwhile. The result comes with the `restored` signal.
        #[qinvokable]
        #[cxx_name = "restoreAsync"]
        fn restore_async(self: Pin<&mut MatePanelActions>);

        /// Restoring has finished: a message for the user.
        #[qsignal]
        fn restored(self: Pin<&mut MatePanelActions>, text: QString);
    }
}

use std::pin::Pin;

use cxx_qt::Threading;
use rexon_core::animations::SystemIo;
use rexon_core::mate_panel::{self, PanelCtx};

#[derive(Default)]
pub struct MatePanelActionsRust;

impl qobject::MatePanelActions {
    fn state(&self) -> cxx_qt_lib::QString {
        let state = mate_panel::read_state(&SystemIo, &PanelCtx::for_look());
        cxx_qt_lib::QString::from(&serde_json::to_string(&state).unwrap_or_default())
    }

    fn confirm(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let id = id.to_string();
        let (title, question) =
            if id.is_empty() { mate_panel::restore_confirm_texts() } else { mate_panel::confirm_texts(&id) };
        let json = serde_json::json!({ "title": title, "question": question });
        cxx_qt_lib::QString::from(&json.to_string())
    }

    fn apply(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        cxx_qt_lib::QString::from(&mate_panel::apply(&SystemIo, &PanelCtx::for_look(), &id.to_string()))
    }

    fn parts_question(&self, id: &cxx_qt_lib::QString) -> cxx_qt_lib::QString {
        let id = id.to_string();
        let offer = mate_panel::live_parts_offer(&SystemIo, &PanelCtx::for_look(), &id);
        let json = match mate_panel::parts_question(&id, &offer) {
            Some((title, question)) => serde_json::json!({ "title": title, "question": question }),
            None => serde_json::json!({}),
        };
        cxx_qt_lib::QString::from(&json.to_string())
    }

    fn apply_async(mut self: Pin<&mut Self>, id: &cxx_qt_lib::QString, install: bool) {
        let id = id.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let ctx = PanelCtx::for_look();
            let offer = mate_panel::live_parts_offer(&SystemIo, &ctx, &id);
            let offer = if install { offer } else { offer.repo_only() };
            let result = mate_panel::apply_with_parts(&SystemIo, &ctx, &id, &offer, &mut |o| {
                // Each part on its own: one failed build does not stop the others.
                let mut failed = Vec::new();
                if !o.repo.is_empty() {
                    if let Err(why) = mate_panel::live_install_repo_parts(&o.repo) {
                        failed.extend(o.repo.iter().map(|p| mate_panel::PartFailure {
                            package: p.clone(),
                            why: why.clone(),
                            log: String::new(),
                        }));
                    }
                }
                for (package, _) in &o.aur {
                    if let Err(why) = crate::appearance_actions::aur_install_wait_short(package) {
                        let home = std::env::var("HOME").unwrap_or_default();
                        failed.push(mate_panel::PartFailure {
                            package: package.clone(),
                            why,
                            log: rexon_core::aur_install::log_path(&home, package),
                        });
                    }
                }
                failed
            });
            let text = serde_json::to_string(&result).unwrap_or_default();
            let _ = handle.queue(move |mut object| {
                object.as_mut().applied(cxx_qt_lib::QString::from(&text));
            });
        });
    }

    fn open_log(&self, path: &cxx_qt_lib::QString) -> bool {
        let path = path.to_string();
        if !std::path::Path::new(&path).is_file() {
            return false;
        }
        crate::appearance_actions::qobject::open_url_external(&cxx_qt_lib::QString::from(&format!("file://{path}")))
    }

    fn restore_async(mut self: Pin<&mut Self>) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let text = mate_panel::restore(&SystemIo, &PanelCtx::for_look());
            let _ = handle.queue(move |mut object| {
                object.as_mut().restored(cxx_qt_lib::QString::from(&text));
            });
        });
    }
}
