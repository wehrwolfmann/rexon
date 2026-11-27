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
        /// from AUR (the shared build queue; the user agreed in the question). If
        /// something did not install, the layout is not applied. The result comes
        /// with the `applied` signal.
        #[qinvokable]
        #[cxx_name = "applyAsync"]
        fn apply_async(self: Pin<&mut MatePanelActions>, id: &QString, install: bool);

        /// Applying has finished: a message for the user.
        #[qsignal]
        fn applied(self: Pin<&mut MatePanelActions>, text: QString);

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
            let text = mate_panel::apply_with_parts(&SystemIo, &ctx, &id, &offer, &mut |o| {
                if !o.repo.is_empty() {
                    mate_panel::live_install_repo_parts(&o.repo)?;
                }
                for (package, _) in &o.aur {
                    crate::appearance_actions::aur_install_wait(package)?;
                }
                Ok(())
            });
            let _ = handle.queue(move |mut object| {
                object.as_mut().applied(cxx_qt_lib::QString::from(&text));
            });
        });
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
