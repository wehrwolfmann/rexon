// ExtensionsActions — the side-actions object of the "Extensions" tab in
// Rust.
//
// WHY. Every QML page of the product expects, as the `actions` property, an
// object that can do what changes the world: install an .rxn, remove an
// extension, enable it, open a directory. When this object was written,
// there was only ONE such object in Rust — for "DE comparison", and even
// that one stopped at the execution boundary (its place is now taken by
// `DesktopsActions`). For "Extensions" the object lived in PySide6
// (`rexon/qt_extensions/window.py`, the `ExtensionsActions` class, 259 lines
// together with the window host), and as long as it was there, the page got
// `actions` from Python — that is, the tab could not leave PySide6 in
// principle.
//
// Here the same contract — the same SEVEN methods and the same two
// signals — is executed by Rust. The approach is the same as for the
// action objects of other tabs: the object decides nothing by itself, all
// decisions come from the core.
//
//   * WHAT to do when installing/removing a look is decided by the planner
//     `rexon_core::ext_manager` (it was ready long ago);
//   * it is DONE by the executor `rexon_core::ext_exec` — written together
//     with this object, because the planner had no real file system at all
//     (its input is an `FsSnap` snapshot);
//   * WHAT to tell the user — the core's texts (`rexon_core::qt_extensions`),
//     the very same ones the widget tab shows.
//
// Warning: RUNNING IN THE BACKGROUND IS PRESERVED, AND THIS IS NOT
// DECORATION. In the widget tab installation and removal ran in a
// `QThread`, and the window did not hang while an .rxn was being unpacked.
// A synchronous call, as in "DE comparison", would be a loss here:
// unpacking a big theme takes seconds, and `pkexec` waits for a password as
// long as it likes. So `installPath`/`uninstall` only START the job and
// return control at once, and the result arrives with the
// `installFinished`/`uninstallFinished` signal — the page already listens
// to it with a `Connections` block.
//
// Warning: TOGGLING IS THE OPPOSITE, SYNCHRONOUS: it is writing one state
// file, and the page needs the answer immediately. The returned string is
// "on" / "off" / "!error text", as in Python: a pair "did it work + the new
// state" cannot be expressed as one value, and two calls in a row could
// disagree with the disk.
//
// Warning: DIALOGS. `installDialog` and `confirm` were one-liners from
// QtWidgets in Python (`QFileDialog`, `QMessageBox`), while a QML process
// has no widgets at all. The analysis of the solution and its cost are in
// the header of `cpp/rexon_ext_dialogs.h`; in short: the dialog is brought
// up with the standard QML type `QtQuick.Dialogs`, and the synchronous
// contract is reconciled with it by a nested event loop, as with
// `QDialog::exec()`.

/// cxx-qt bridge of the actions object.
///
/// The type is declared `#[qml_element]`, so the page can create it for
/// itself (`ExtensionsActions { }`), and the window host can substitute its
/// own. No registration in `main_window.rs` is needed for this: that file
/// holds the section map, not a list of side objects.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    // Dialogs — see the header of `cpp/rexon_ext_dialogs.h`.
    unsafe extern "C++" {
        include!("rexon_ext_dialogs.h");

        #[rust_name = "pick_file"]
        fn rexon_ext_pick_file(title: &QString, filter: &QString, folder: &QString) -> QString;

        /// File save window (exporting a font group in "Settings").
        #[rust_name = "save_file"]
        fn rexon_ext_save_file(title: &QString, filter: &QString, path: &QString) -> QString;

        #[rust_name = "ask_confirm_skinned"]
        fn rexon_ext_confirm(title: &QString, text: &QString, skin: &QString) -> bool;

        #[rust_name = "open_folder_url"]
        fn rexon_ext_open_folder(path: &QString) -> bool;

        /// Font chooser window. It lives here too, because there is one dialog shim
        /// in the project: "Settings" (`settings_actions.rs`) use it as well, just
        /// as they already use `pick_file` for importing Windows themes.
        #[rust_name = "pick_font"]
        fn rexon_ext_pick_font(
            title: &QString,
            family: &QString,
            size: i32,
            weight: i32,
            italic: bool,
            outcome: &mut i32,
        ) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type ExtensionsActions = super::ExtensionsActionsRust;
    }

    unsafe extern "RustQt" {
        /// Installation has finished: `ok` — did it work, `message` — the name of
        /// the installed extension or the error text.
        #[qsignal]
        #[cxx_name = "installFinished"]
        fn install_finished(self: Pin<&mut ExtensionsActions>, ok: bool, message: QString);

        /// Removal has finished: `ok` — did it work, `message` — the identifier of
        /// the removed extension or the error text.
        #[qsignal]
        #[cxx_name = "uninstallFinished"]
        fn uninstall_finished(self: Pin<&mut ExtensionsActions>, ok: bool, message: QString);

        /// "Apply" has finished: `message` is the ready result line.
        #[qsignal]
        #[cxx_name = "applyFinished"]
        fn apply_finished(self: Pin<&mut ExtensionsActions>, message: QString);
    }

    extern "RustQt" {
        /// Whether an installation or removal is already running. One answer to
        /// both questions: the widget tab had two checks, but both led to the same
        /// thing — "wait, then press".
        #[qinvokable]
        fn busy(self: &ExtensionsActions) -> bool;

        /// Picker window for an .rxn. An empty string means the user changed their
        /// mind.
        #[qinvokable]
        #[cxx_name = "installDialog"]
        fn install_dialog(self: &ExtensionsActions) -> QString;

        /// Start installing the file in the background. `false` means it was not
        /// started.
        #[qinvokable]
        #[cxx_name = "installPath"]
        fn install_path(self: Pin<&mut ExtensionsActions>, path: &QString) -> bool;

        /// A "yes/no" question before an irreversible action.
        #[qinvokable]
        fn confirm(self: &ExtensionsActions, title: &QString, text: &QString) -> bool;

        /// Start removing the extension in the background. `false` means it was not
        /// started.
        #[qinvokable]
        fn uninstall(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> bool;

        /// Toggle the extension. "on" / "off" / "!error".
        #[qinvokable]
        fn toggle(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> QString;

        /// Open the extension's directory in the file manager.
        #[qinvokable]
        #[cxx_name = "openFolder"]
        fn open_folder(self: &ExtensionsActions, path: &QString) -> bool;

        /// Label of the "Apply" button — empty if this extension cannot be applied
        /// from here (not a wallpaper set, or the set has no files).
        #[qinvokable]
        #[cxx_name = "applyText"]
        fn apply_text(self: &ExtensionsActions, ext_id: &QString) -> QString;

        /// Put the set's wallpaper on the desktop (the first one of the set; the
        /// others are chosen in "Settings → Wallpaper library"). Returns the line
        /// "applying…"; the result will arrive with the `applyFinished` signal.
        #[qinvokable]
        fn apply(self: Pin<&mut ExtensionsActions>, ext_id: &QString) -> QString;
    }

    // The trait declaration must be IN THE BRIDGE: cxx-qt emits the thread type
    // alias from it, without which background work would not reach the signal.
    impl cxx_qt::Threading for ExtensionsActions {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::QString;

use rexon_core::ext_exec;
use rexon_core::qt_extensions as qe;

/// Rust state of the actions object: two busy flags, one per kind of work.
/// Not one shared flag: installation and removal run in different threads,
/// and a shared flag would be cleared by whichever finishes first.
#[derive(Default)]
pub struct ExtensionsActionsRust {
    install_busy: Arc<AtomicBool>,
    uninstall_busy: Arc<AtomicBool>,
}

/// Colours of the active skin and the button labels for the question
/// window — in the order the shim expects (`rexon_ext_confirm`): background,
/// card, text, accent, border, "Yes", "No"; separator U+001F.
pub fn confirm_skin() -> String {
    confirm_skin_labeled(&rexon_core::i18n::tr("Yes"), &rexon_core::i18n::tr("No"))
}

/// [`confirm_skin`] with custom button labels.
fn confirm_skin_labeled(yes: &str, no: &str) -> String {
    let colors = rexon_core::ui_base::active_skin_colors();
    let tone = |key: &str| colors.get(key).cloned().unwrap_or_default();
    [
        tone("bg_primary"),
        tone("bg_card"),
        tone("text_primary"),
        tone("accent"),
        tone("border"),
        yes.to_string(),
        no.to_string(),
    ]
    .join("\u{1f}")
}

/// A "yes/no" question in a Rexon-skinned window. `false` means "No", a
/// closed window, or a window that did not come up.
pub fn ask_confirm(title: &QString, text: &QString) -> bool {
    qobject::ask_confirm_skinned(title, text, &QString::from(&confirm_skin()))
}

/// [`ask_confirm`] with custom button labels ("Download and enable" / "Keep
/// mine"). `false` means the second button, a closed window, or a window
/// that did not come up.
pub fn ask_confirm_labeled(title: &str, text: &str, yes: &str, no: &str) -> bool {
    qobject::ask_confirm_skinned(&QString::from(title), &QString::from(text), &QString::from(&confirm_skin_labeled(yes, no)))
}

impl qobject::ExtensionsActions {
    fn busy(&self) -> bool {
        let state = self.rust();
        state.install_busy.load(Ordering::SeqCst) || state.uninstall_busy.load(Ordering::SeqCst)
    }

    fn install_dialog(&self) -> QString {
        // The title and the filter come from the core: the page already knows them
        // (`fileDialogTitle` / `fileDialogFilter`), and there must be no second
        // copy of them here.
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        qobject::pick_file(
            &QString::from(&qe::file_dialog_title()),
            &QString::from(&qe::file_dialog_filter()),
            &QString::from(&home),
        )
    }

    fn confirm(&self, title: &QString, text: &QString) -> bool {
        ask_confirm(title, text)
    }

    fn open_folder(&self, path: &QString) -> bool {
        qobject::open_folder_url(path)
    }

    fn install_path(mut self: Pin<&mut Self>, path: &QString) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().install_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let path = path.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // The installation result — the extension name AND warnings about skipped
            // steps (no converter, nothing to measure the wallpaper with). A warning
            // that did not reach the status line is as good as none.
            let outcome = ext_exec::install_from_file(&path)
                .map(|done| qe::installed_with_notes(&done.name, &done.notes));
            flag.store(false, Ordering::SeqCst);
            report(handle, outcome, true);
        });
        true
    }

    fn uninstall(mut self: Pin<&mut Self>, ext_id: &QString) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().uninstall_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let ext_id = ext_id.to_string();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let outcome = ext_exec::uninstall(&ext_id).map(|()| ext_id.clone());
            flag.store(false, Ordering::SeqCst);
            report(handle, outcome, false);
        });
        true
    }

    fn apply_text(&self, ext_id: &QString) -> QString {
        if pack_wallpapers(&ext_id.to_string()).is_empty() {
            QString::default()
        } else {
            QString::from(&rexon_core::qt_settings::text("lib.apply", &[]).unwrap_or_default())
        }
    }

    fn apply(mut self: Pin<&mut Self>, ext_id: &QString) -> QString {
        use rexon_core::{qt_settings as qs, wallpaper_engine, wallpaper_exec};
        let Some(first) = pack_wallpapers(&ext_id.to_string()).into_iter().next() else {
            return QString::from(&qs::text("lib.no_file", &[]).unwrap_or_default());
        };
        let basename = std::path::Path::new(&first.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let applying = qs::library_applying_status(&basename, &wallpaper_engine::format_label(&first.format));
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            // The same core door as for the "Wallpaper library" in "Settings": one
            // executor per process, and supervising live wallpapers is its business.
            let status = wallpaper_exec::run_in_slot(
                &wallpaper_exec::live_slot(),
                move |exec: &mut wallpaper_exec::WallpaperExec<wallpaper_exec::LiveSys>| {
                    let res = exec.apply(&first.path, &first.format, "");
                    qs::wallpaper_done_warned(res.ok, &res.message, res.warning.as_deref())
                },
            );
            if let Err(e) = handle.queue(move |mut object| {
                object.as_mut().apply_finished(QString::from(&status));
            }) {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("extensions", "итог применения обоев не доставлен: {e:?}");
                }
            }
        });
        QString::from(&applying)
    }

    fn toggle(self: Pin<&mut Self>, ext_id: &QString) -> QString {
        let id = ext_id.to_string();
        match ext_exec::toggle(&id) {
            Ok(true) => QString::from("on"),
            Ok(false) => QString::from("off"),
            // The "!" prefix is part of the contract with the page: by it the page
            // tells an error from a new state.
            Err(text) => QString::from(&format!("!{text}")),
        }
    }
}

/// Wallpapers of set `ext_id` from the extension directory — the same
/// entries the "Wallpaper library" in "Settings" shows. Empty means the
/// extension is not a wallpaper set or it has no files.
fn pack_wallpapers(ext_id: &str) -> Vec<rexon_core::wallpaper_engine::LibraryItem> {
    let dir = qe::extensions_dir();
    rexon_core::wallpaper_engine::list_installed_wallpapers(&dir.to_string_lossy())
        .into_iter()
        .filter(|item| item.id == ext_id)
        .collect()
}

/// Deliver the result of background work to the interface thread.
///
/// Neither `unwrap()` nor a preceding "is the object alive" check: between
/// the check and the queueing the page may close, and that is not an error —
/// there is simply nobody to show the report to any more.
fn report(
    handle: cxx_qt::CxxQtThread<qobject::ExtensionsActions>,
    outcome: Result<String, String>,
    install: bool,
) {
    let (ok, message) = match outcome {
        Ok(text) => (true, text),
        Err(text) => (false, text),
    };
    let queued = handle.queue(move |mut object| {
        let text = QString::from(&message);
        if install {
            object.as_mut().install_finished(ok, text);
        } else {
            object.as_mut().uninstall_finished(ok, text);
        }
    });
    if let Err(e) = queued {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            // Staying silent is not allowed: the installation result was lost, and the
            // tab would be left with the "Installing…" line forever.
            rexon_core::log_warn!("extensions", "итог действия «Расширений» не доставлен: {e:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract with the page: the toggle answer is "on", "off" or a text with
    /// the "!" prefix. The page parses it exactly so (`answer.substring(1)`), and
    /// there can be no third form of the answer.
    #[test]
    fn toggle_answer_of_unknown_extension_is_marked_as_error() {
        let answer = match ext_exec::toggle("rexon-нет-такого-расширения") {
            Ok(true) => "on".to_string(),
            Ok(false) => "off".to_string(),
            Err(text) => format!("!{text}"),
        };
        assert!(
            answer.starts_with('!'),
            "неизвестное расширение обязано ответить ошибкой, а не «выключено»: {answer}"
        );
    }
}
