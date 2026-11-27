// WindowMenuModel — the Rexon main window menu in Rust: the contents, the
// texts and the execution of the three items that have work behind them.
//
// WHY. The menu under the "☰" button is the only place in the window from
// which the customer reaches "Restore KDE theme", "About", "Uninstall
// Rexon" and "Quit". The Rust frame had NOT A SINGLE LINE of it: the QML
// window opened without a menu at all, that is, it could not replace the
// Python window in principle.
//
// The approach is the same as for the tabs' side-actions objects: the
// object decides nothing by itself.
//
//   * WHAT is in the menu, in what words and what is available —
//     `rexon_core::window_menu`;
//   * WHAT to remove, with which command and HOW to execute it — the core:
//     the decisions in `rexon_core::installer` (the very engine the Python
//     `RexonUninstaller` uses), the execution in `installer_exec::uninstall`
//     (detecting the file owner with the package manager,
//     `pkexec pacman -R` for a distribution package, the removal script of
//     a copy for everyone, a copy in the home folder, the profile, parsing
//     the outcome and the result text). The window has no executor of its
//     own: until G8 its second copy lived here;
//   * here there are only the live facts (the distribution family, our own
//     binary) and the background thread.
//
// Warning: UNINSTALLING THE PROGRAM BELONGS HERE. The owner's rule: an
// uninstall button lives only in PROGRAMS, games do not get one. Rexon is
// a program, and in the product this button sits exactly here.
//
// Warning: A DISTRIBUTION PACKAGE IS A SEPARATE INSTALLATION METHOD. Files
// in /usr/{bin,lib} are managed by the package manager, and manual
// detection does not see them at all: `rm -rf` on such paths would leave
// in the pacman/dpkg database a record of a package whose files vanished.
// So we first ask the manager for the OWNER of our files, and if there is
// an owner, we remove the package with its own means (`pkexec pacman -R`).
//
// Warning: UNINSTALLING RUNS IN THE BACKGROUND. Behind `pkexec` there is a
// password prompt that waits as long as it likes; a synchronous call would
// hang the window for all that time. The result arrives with the
// `uninstallFinished` signal.

/// cxx-qt bridge of the window menu.
///
/// The type is declared `#[qml_element]`, so both the window host and the
/// menu markup can create it for themselves.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Menu contents: [{id, label, shortcut, separatorBefore, enabled, destructive}, …]
        // in menu order. CONSTANT: the menu does not change during the window's
        // lifetime — exactly as in the product, where `_build_menubar` is called
        // once.
        #[qproperty(QList_QVariant, items, READ = items, CONSTANT)]
        // ── Label of the menu button itself ("☰").
        #[qproperty(QString, button_text, cxx_name = "buttonText",
                    READ = button_text, CONSTANT)]
        type WindowMenuModel = super::WindowMenuModelRust;
    }

    unsafe extern "RustQt" {
        /// Uninstalling has finished: `ok` — did it work, `title` and `message` —
        /// the title and text of the message to show the user.
        #[qsignal]
        #[cxx_name = "uninstallFinished"]
        fn uninstall_finished(
            self: Pin<&mut WindowMenuModel>,
            ok: bool,
            title: QString,
            message: QString,
        );

        /// Removing the packages replacing parts of Plasma has finished: the same
        /// structure as `uninstallFinished`.
        #[qsignal]
        #[cxx_name = "forksRemoveFinished"]
        fn forks_remove_finished(
            self: Pin<&mut WindowMenuModel>,
            ok: bool,
            title: QString,
            message: QString,
        );
    }

    extern "RustQt" {
        // Property readers. Declared separately from the callable methods: a
        // property has no `#[qinvokable]`, Qt reads it itself.
        fn items(self: &WindowMenuModel) -> QList_QVariant;
        #[cxx_name = "buttonText"]
        fn button_text(self: &WindowMenuModel) -> QString;
    }

    extern "RustQt" {
        // ── "About" ───────────────────────────────────────────────────────
        #[qinvokable]
        #[cxx_name = "aboutTitle"]
        fn about_title(self: &WindowMenuModel) -> QString;

        #[qinvokable]
        #[cxx_name = "aboutHtml"]
        fn about_html(self: &WindowMenuModel) -> QString;

        /// The log for the about box: {path, url, location, openLabel, copyLabel,
        /// copied}.
        #[qinvokable]
        #[cxx_name = "logInfo"]
        fn log_info(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// The log tail for "Copy log".
        #[qinvokable]
        #[cxx_name = "logTail"]
        fn log_tail(self: &WindowMenuModel) -> QString;

        // ── "Restore KDE theme" ───────────────────────────────────────────
        /// Title and text of the question before the reset plus the name of the
        /// theme that will be applied: {title, text, target}.
        #[qinvokable]
        #[cxx_name = "recoverPrompt"]
        fn recover_prompt(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Start the reset. {ok, title, text} — what to tell the user.
        #[qinvokable]
        #[cxx_name = "recoverApply"]
        fn recover_apply(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Whether packages replacing parts of Plasma are installed and what to say
        /// about them: {forks, title, text, removeLabel, themeLabel, cancelLabel}.
        /// An empty `forks` means there are no replacements, straight to the theme
        /// reset question. Only READING the package list; the found set is
        /// remembered, and exactly that set is removed later.
        #[qinvokable]
        #[cxx_name = "recoverForks"]
        fn recover_forks(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// What removing the replacements will run — WITHOUT starting it (read-only
        /// probes): {ok, title, text, runningTitle, runningText}. `ok=false` means
        /// there is nothing to restore the standard packages with, and `text`
        /// explains why.
        #[qinvokable]
        #[cxx_name = "forksNext"]
        fn forks_next(self: &WindowMenuModel, button_id: &QString) -> QString;

        #[qinvokable]
        #[cxx_name = "forksPreview"]
        fn forks_preview(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Start removing the replacements in the background. `false` means it is
        /// already running or there is nothing to remove. The result comes with the
        /// `forksRemoveFinished` signal.
        #[qinvokable]
        #[cxx_name = "forksRemoveRun"]
        fn forks_remove_run(self: Pin<&mut WindowMenuModel>) -> bool;

        // ── "Uninstall Rexon" ─────────────────────────────────────────────
        /// The whole uninstall window: {title, text, informative, files,
        /// keepLabel, deleteLabel, cancelLabel, defaultId}.
        ///
        /// Only READING the file system and querying the package manager — removes
        /// nothing.
        #[qinvokable]
        #[cxx_name = "uninstallPrompt"]
        fn uninstall_prompt(self: &WindowMenuModel) -> QMap_QString_QVariant;

        /// Start uninstalling in the background. `false` means it is already
        /// running.
        #[qinvokable]
        #[cxx_name = "uninstallRun"]
        fn uninstall_run(self: Pin<&mut WindowMenuModel>, keep_profile: bool) -> bool;

        /// Whether uninstalling is in progress.
        #[qinvokable]
        fn busy(self: &WindowMenuModel) -> bool;

        // ── Shared ────────────────────────────────────────────────────────
        /// Whether an item is available. Asked of the core, not decided by the
        /// markup.
        #[qinvokable]
        #[cxx_name = "isEnabled"]
        fn is_enabled(self: &WindowMenuModel, id: &QString) -> bool;
    }

    // The trait declaration must be IN THE BRIDGE: cxx-qt emits the thread type
    // alias from it, without which background work would not reach the signal.
    impl cxx_qt::Threading for WindowMenuModel {}
}

use core::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::distro_compat::Lang;
use rexon_core::installer_exec::{self as iexec, LiveInstallIo, LogSink, UninstallRequest};
use rexon_core::system::DistroDetector;
use rexon_core::system_de_exec::CancelToken;
use rexon_core::window_menu as wm;

/// Rust state of the object: the "running" flags and the set of Plasma
/// replacement packages shown to the user.
#[derive(Default)]
pub struct WindowMenuModelRust {
    uninstall_busy: Arc<AtomicBool>,
    forks_busy: Arc<AtomicBool>,
    /// What was shown in the warning is what gets removed. A second query
    /// between the question and the removal could remove something other than
    /// what the user saw.
    shown_forks: Mutex<Vec<String>>,
}

fn qstr(s: &str) -> QVariant {
    QVariant::from(&QString::from(s))
}

fn map(pairs: &[(&str, QVariant)]) -> QMap<QMapPair_QString_QVariant> {
    let mut m = QMap::<QMapPair_QString_QVariant>::default();
    for (k, v) in pairs {
        m.insert(QString::from(*k), v.clone());
    }
    m
}

impl qobject::WindowMenuModel {
    // ── Menu contents ─────────────────────────────────────────────────────

    fn items(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        // On a foreign desktop there is no "Restore KDE theme" item at all.
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        for item in wm::menu_items_for(&desktop) {
            out.append(QVariant::from(&map(&[
                ("id", qstr(item.id)),
                ("label", qstr(&item.label)),
                ("shortcut", qstr(item.shortcut)),
                ("separatorBefore", QVariant::from(&item.separator_before)),
                ("enabled", QVariant::from(&wm::is_enabled(item.id))),
                ("destructive", QVariant::from(&wm::is_destructive(item.id))),
            ])));
        }
        out
    }

    fn button_text(&self) -> QString {
        QString::from(wm::MENU_BUTTON)
    }

    fn is_enabled(&self, id: &QString) -> bool {
        wm::is_enabled(&id.to_string())
    }

    // ── "About" ───────────────────────────────────────────────────────────

    fn about_title(&self) -> QString {
        QString::from(&wm::about_title())
    }

    fn about_html(&self) -> QString {
        QString::from(&wm::about_dialog_html())
    }

    fn log_info(&self) -> QMap<QMapPair_QString_QVariant> {
        let path = rexon_core::applog::log_path().to_string_lossy().into_owned();
        map(&[
            ("path", qstr(&path)),
            ("url", qstr(&format!("file://{path}"))),
            ("location", qstr(&wm::log_location_text(&path))),
            ("openLabel", qstr(&wm::log_open_label())),
            ("copyLabel", qstr(&wm::log_copy_label())),
            ("copied", qstr(&wm::log_copied_text())),
        ])
    }

    fn log_tail(&self) -> QString {
        rexon_core::log_info!("about", "журнал скопирован из справки");
        QString::from(&rexon_core::applog::tail(wm::LOG_COPY_BYTES))
    }

    // ── "Restore KDE theme" ───────────────────────────────────────────────

    fn recover_prompt(&self) -> QMap<QMapPair_QString_QVariant> {
        let target = recover_target();
        // On a foreign desktop the route will refuse — the window says so in
        // advance rather than reporting "started" (`allowed=false` + `refusal`).
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        map(&[
            ("title", qstr(&wm::recover_title())),
            ("text", qstr(&wm::recover_question(&target))),
            ("target", qstr(&target)),
            ("allowed", QVariant::from(&wm::recover_allowed(&desktop))),
            ("refusal", qstr(&wm::recover_not_plasma_text(&desktop))),
        ])
    }

    fn recover_forks(&self) -> QMap<QMapPair_QString_QVariant> {
        let forks = rexon_core::system_exec::live().detect_plasma_forks();
        *self
            .rust()
            .shown_forks
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = forks.clone();
        let mut list = QList::<QVariant>::default();
        for f in &forks {
            list.append(qstr(f));
        }
        // The window and the button labels are the same as in the Python window
        // (`forks_prompt`).
        let Some(prompt) = wm::forks_prompt(&forks) else {
            return map(&[("forks", QVariant::from(&list))]);
        };
        let label = |id: &str| {
            prompt
                .buttons
                .iter()
                .find(|b| b.id == id)
                .map(|b| b.label.clone())
                .unwrap_or_default()
        };
        map(&[
            ("forks", QVariant::from(&list)),
            ("title", qstr(&prompt.title)),
            ("text", qstr(&prompt.text)),
            ("removeLabel", qstr(&label(wm::FORKS_BUTTON_REMOVE))),
            ("themeLabel", qstr(&label(wm::FORKS_BUTTON_THEME))),
            ("cancelLabel", qstr(&label(wm::FORKS_BUTTON_CANCEL))),
        ])
    }

    fn forks_next(&self, button_id: &QString) -> QString {
        QString::from(wm::forks_next(&button_id.to_string()).as_str())
    }

    fn forks_preview(&self) -> QMap<QMapPair_QString_QVariant> {
        let forks = self.shown_forks();
        let (ok, message) = rexon_core::system_exec::live().plasma_fork_reset_preview(Some(&forks));
        let step = wm::forks_preview_step(ok, &message);
        map(&[
            ("ok", QVariant::from(&step.ok)),
            ("title", qstr(&step.title)),
            ("text", qstr(&step.text)),
            ("runningTitle", qstr(&step.running_title)),
            ("runningText", qstr(&step.running_text)),
        ])
    }

    fn shown_forks(&self) -> Vec<String> {
        self.rust()
            .shown_forks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn forks_remove_run(mut self: Pin<&mut Self>) -> bool {
        let forks = self.as_ref().get_ref().shown_forks();
        if forks.is_empty() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().forks_busy.clone();
        if flag.swap(true, Ordering::SeqCst) {
            return false;
        }
        let handle = self.as_mut().qt_thread();
        // In the background: behind `pkexec` there is a password prompt that waits
        // as long as it likes, and the package transaction takes up to a minute.
        // Python hung the window for that time; here it stays alive.
        std::thread::spawn(move || {
            let (ok, message) =
                rexon_core::system_exec::live().remove_plasma_forks(Some(&forks));
            flag.store(false, Ordering::SeqCst);
            let (title, text) = wm::forks_finished(ok, &message);
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .forks_remove_finished(ok, QString::from(&title), QString::from(&text));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("window-menu", "итог снятия замен Plasma не доставлен: {e:?}");
                }
            }
        });
        true
    }

    fn recover_apply(&self) -> QMap<QMapPair_QString_QVariant> {
        let target = recover_target();
        // Fire and forget — like `subprocess.Popen` in the product: behind the flag
        // there is a Plasma theme switch and a plasmashell restart, and waiting for
        // it from the interface thread is not allowed. OUR OWN binary is called: it
        // handles the `--recover-kde` route in Rust before Qt comes up.
        let own_exe = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned());
        match spawn_detached(&wm::recover_argv(own_exe.as_deref())) {
            Ok(()) => map(&[
                ("ok", QVariant::from(&true)),
                ("title", qstr(&wm::recover_started_title())),
                ("text", qstr(&wm::recover_started_text(&target))),
            ]),
            Err(e) => map(&[
                ("ok", QVariant::from(&false)),
                ("title", qstr(&wm::error_title())),
                ("text", qstr(&wm::recover_failed_text(&e))),
            ]),
        }
    }

    // ── "Uninstall Rexon" ─────────────────────────────────────────────────

    fn uninstall_prompt(&self) -> QMap<QMapPair_QString_QVariant> {
        let io = LiveInstallIo::new();
        let listing = iexec::uninstall_files(&io, &live_uninstall_request(&io, false));
        let mut list = QList::<QVariant>::default();
        for f in &listing.removed(false) {
            list.append(qstr(f));
        }
        map(&[
            ("title", qstr(&wm::uninstall_title())),
            ("text", qstr(&wm::uninstall_text())),
            ("informative", qstr(&wm::uninstall_informative(&listing.program, &listing.user_data))),
            ("files", QVariant::from(&list)),
            ("keepLabel", qstr(&wm::uninstall_keep_label())),
            ("deleteLabel", qstr(&wm::uninstall_delete_label())),
            ("cancelLabel", qstr(&wm::uninstall_cancel_label())),
            ("defaultId", qstr(wm::UNINSTALL_DEFAULT_ID)),
        ])
    }

    fn busy(&self) -> bool {
        self.rust().uninstall_busy.load(Ordering::SeqCst)
    }

    fn uninstall_run(mut self: Pin<&mut Self>, keep_profile: bool) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let flag = self.as_ref().get_ref().rust().uninstall_busy.clone();
        flag.store(true, Ordering::SeqCst);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let io = LiveInstallIo::new();
            let req = live_uninstall_request(&io, keep_profile);
            let result = iexec::uninstall(&io, &req, Arc::new(LogSink), &CancelToken::new());
            let (ok, message) = (result.ok(), result.message);
            flag.store(false, Ordering::SeqCst);
            let (title, text) = if ok {
                (wm::uninstall_done_title(), wm::uninstall_done_text(&message))
            } else {
                (wm::uninstall_failed_title(), message)
            };
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .uninstall_finished(ok, QString::from(&title), QString::from(&text));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Staying silent is not allowed: the window would be left with
                    // "Uninstalling…" forever.
                    rexon_core::log_warn!("uninstall", "итог удаления не доставлен: {e:?}");
                }
            }
        });
        true
    }
}

// ============================================================================
// "Restore KDE theme": which theme will be applied
// ============================================================================

/// Theme name from the backup or a fallback. Reads the file; the decision
/// what counts as empty and what replaces it is made by the core.
fn recover_target() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let path = wm::recover_backup_path(&home, xdg.as_deref().filter(|v| !v.is_empty()));
    let saved = std::fs::read_to_string(&path).ok();
    wm::recover_target(saved.as_deref())
}

/// Fire and forget. The error is returned as text: the dialog must name the
/// reason, otherwise the press looks like "nothing happened".
fn spawn_detached(argv: &[String]) -> Result<(), String> {
    let Some((binary, args)) = argv.split_first() else {
        return Err(rexon_core::i18n::tr("empty command"));
    };
    std::process::Command::new(binary)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ============================================================================
// Uninstalling: live facts for the core executor
// ============================================================================

/// The interface language as the installer's pair (`Lang`) — the same as
/// the catalogue's.
fn ui_lang() -> Lang {
    Lang::from_code(&rexon_core::i18n::current_language())
}

/// What to remove on this machine. Decisions and execution are in the core
/// (`installer_exec::detect_distro_package`, `UninstallRequest::detect`);
/// here there are only live facts: the distribution family and the path of
/// our own binary.
///
/// The result language is the interface language (`rexon_core::i18n`): the
/// uninstall window titles go through the same catalogue, and a result in
/// another language would give a window in two languages. The installer
/// still chooses the "Russian/English" pair by itself (`Lang`), so it is
/// given the same language as the catalogue.
fn live_uninstall_request(io: &LiveInstallIo, keep_profile: bool) -> UninstallRequest {
    let family = DistroDetector::detect_live().family_live();
    let own_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    let package = iexec::detect_distro_package(io, family, own_exe.as_deref());
    UninstallRequest::detect(io, package, keep_profile, ui_lang())
}

#[cfg(test)]
mod tests {
    /// Not a single menu text is typed anew in this file: all of them come from
    /// the core. A copy-paste guard — a second copy of a human-readable string
    /// would silently drift away from the product (rule #12).
    #[test]
    fn no_human_text_is_typed_twice_in_this_file() {
        // Our own test module is not counted: the list of forbidden strings lies
        // below, and without this cut-off the guard would catch itself and could
        // never go green.
        let whole = include_str!("window_menu.rs");
        let src = whole
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("тестовый модуль на месте");
        for baked in [
            "Восстановить тему KDE",
            "О программе",
            "Удалить Rexon",
            "Выход",
            "Breeze Dark",
            "Сохранить резервные копии",
            "Только сбросить тему",
            "вернуть обычную Plasma",
            "Замены удалены",
            "--recover-kde",
            // The catalogue's English source texts are also only in the core.
            "Restore KDE Theme",
            "About Rexon",
            "Uninstall Rexon",
            "Remove Everything",
            "Reset Theme Only",
            "Replacements Removed",
        ] {
            let in_code: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(baked))
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            assert!(
                in_code.is_empty(),
                "человеческий текст «{baked}» набран здесь, а не взят у ядра: {in_code:?}"
            );
        }
    }

    /// G8: there is one uninstall executor — in the core. There is no second
    /// copy of the plan steps, command execution and path removal in the
    /// window: it would silently drift away from the core (rule #12). A foreign
    /// plan step is refused by the core
    /// (`installer_exec::tests::foreign_plan_step_is_refused`), the listing is
    /// read-only
    /// (`installer_exec::tests::nothing_installed_touches_nothing_not_even_settings`).
    #[test]
    fn window_has_no_uninstall_executor_of_its_own() {
        let whole = include_str!("window_menu.rs");
        let src = whole
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("тестовый модуль на месте");
        for own in [
            "fn exec_step",
            "fn run_cmd",
            "fn remove_path",
            "remove_dir_all",
            "PlanStep",
            "uninstall_system_plan",
            "\"pkexec\"",
        ] {
            let hits: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(own))
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect();
            assert!(hits.is_empty(), "окно снова исполняет удаление само («{own}»): {hits:?}");
        }
        assert!(src.contains("iexec::uninstall(&io"), "окно не зовёт исполнитель ядра");
        assert!(src.contains("iexec::uninstall_files(&io"), "перечень удаляемого берётся не у ядра");
    }
}
