// ScreenEffectsActions — the object of the "Screen effects" section of the
// "Appearance" tab.
//
// There is no logic here: the snapshot, the support table and writing are
// done by the core (`rexon_core::screen_effects`). The object only
// translates QML calls into core calls and hands back ready strings (the
// snapshot JSON, the result phrase).
//
// Warning: writing happens IN A WORKER THREAD. On 15.11.2026 (Cinnamon 6.8
// Wayland) the window froze while the user rapidly pressed "Apply"/"Restore
// as it was" and switched effects: every action synchronously started
// several `gsettings`/`gdbus` right in the main thread, and now waiting for
// the shell extension's answer (up to `CONFIRM_WAIT`) has been added on
// top. So `request` only puts the action into a "last one wins" queue
// (`LatestQueue`: a waiting action is replaced by the new one, no backlog
// builds up) and returns immediately; the result arrives with the
// `finished` signal, and while the action is running — `busy` (the
// section's buttons are disabled).

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, busy)]
        type ScreenEffectsActions = super::ScreenEffectsActionsRust;
    }

    extern "RustQt" {
        /// Section snapshot (JSON `screen_effects::ScreenEffectsState`) for
        /// environment `de` (the "Current environment" choice; empty means the
        /// running one).
        #[qinvokable]
        fn state(self: &ScreenEffectsActions, de: &QString) -> QString;

        /// A section action for environment `de`: a preset by name (empty — turn
        /// off), strength 0…1, speed 0…`max_speed`. `tag` says what it is
        /// ("apply", "install" — install the missing program first, then
        /// apply; "restore", "revert") and comes back in `finished`.
        /// Does not wait: the result comes with the `finished` signal.
        #[qinvokable]
        fn request(self: Pin<&mut ScreenEffectsActions>, tag: &QString, de: &QString, preset: &QString, strength: f64, speed: f64);

        /// Action result: the request's `tag`, the phrase for the banner, whether
        /// anything was written, whether the effect is on (confirmed by the
        /// extension), the new section snapshot (JSON, as for `state`).
        #[qsignal]
        fn finished(self: Pin<&mut ScreenEffectsActions>, tag: QString, text: QString, changed: bool, shown: bool, state: QString);
    }

    impl cxx_qt::Threading for ScreenEffectsActions {}
}

use core::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use rexon_core::animations::{LookCtx, SystemIo};
use rexon_core::screen_effects as se;

/// One section action.
struct Request {
    tag: String,
    de: String,
    preset: String,
    strength: f64,
    speed: f64,
}

#[derive(Default)]
pub struct ScreenEffectsActionsRust {
    busy: bool,
    queue: Arc<se::LatestQueue<Request>>,
}

fn snapshot(de: &str) -> String {
    let family = se::live_install_family();
    let state = se::read_state_for(&SystemIo, &se::ctx_for(LookCtx::from_env(), de), family.as_deref());
    serde_json::to_string(&state).unwrap_or_default()
}

impl qobject::ScreenEffectsActions {
    fn state(&self, de: &QString) -> QString {
        QString::from(&snapshot(&de.to_string()))
    }

    fn request(mut self: Pin<&mut Self>, tag: &QString, de: &QString, preset: &QString, strength: f64, speed: f64) {
        let req = Request { tag: tag.to_string(), de: de.to_string(), preset: preset.to_string(), strength, speed };
        let queue = self.rust().queue.clone();
        let start = queue.push(req);
        self.as_mut().set_busy(true);
        if !start {
            return;
        }
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            while let Some(req) = queue.take() {
                let ctx = se::ctx_for(LookCtx::from_env(), &req.de);
                let new = se::settings_from_ui(&req.preset, req.strength, req.speed);
                let out = if req.tag == "install" {
                    // The missing program (picom) is installed the usual Rexon way —
                    // the password window, no «Yes/No» — and then the effect goes on.
                    let family = se::live_install_family();
                    let packages = se::install_packages(&SystemIo, &ctx, family.as_deref());
                    se::install_then_apply(&SystemIo, &ctx, &new, &packages, &se::live_install, &std::thread::sleep)
                } else {
                    se::apply_confirmed(&SystemIo, &ctx, &new, &std::thread::sleep)
                };
                let state = snapshot(&req.de);
                // The window is closed — there is nobody to report to, but the action
                // (for example, rolling back a trial) is still finished.
                let _ = handle.queue(move |mut obj| {
                    // Busy while there is something in the queue: a new press may have come
                    // between the end of this action and this line.
                    let busy = obj.rust().queue.busy();
                    obj.as_mut().set_busy(busy);
                    obj.as_mut().finished(
                        QString::from(&req.tag),
                        QString::from(&out.text),
                        out.changed,
                        out.shown,
                        QString::from(&state),
                    );
                });
            }
            // The last "busy" may have gone out before the thread released the queue —
            // clear it once more.
            let _ = handle.queue(|mut obj| {
                let busy = obj.rust().queue.busy();
                obj.as_mut().set_busy(busy);
            });
        });
    }
}
