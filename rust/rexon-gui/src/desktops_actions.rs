// DesktopsActions — the side-actions object of the "Desktops" tab in Rust.
//
// WHY. `DesktopsPage.qml` expects, in its `actions` property, an object with
// nine methods and one signal. While there was no such object in Rust, the
// window host (`main_window.rs`) could not give the "Desktops" section its
// own page: the `SECTION_PAGES` table showed the "DE comparison"
// subsection in it, because the desktops page without an action object
// would have given dead buttons and an empty version line. Here this object
// appears, and the section gets its own page.
//
// THE BOUNDARY IS DRAWN BY ONE CRITERION — what the action DOES to the
// machine:
//
//   * reads and navigates the interface → executed by Rust:
//       - `requestVersion(deId)` — the package version from the
//         distribution's repositories (`system_plan::repo_version_query` +
//         `parse_repo_version`, launched through `utils::run`), in the
//         background, the answer with the `versionReady` signal;
//       - `openCompare()` — switching to the built-in "DE comparison"; the
//         host switches pages, so the object only announces the request with
//         the `compareRequested` signal.
//
//   * installs and repairs a desktop → executed by the CORE
//     (`rexon_core::system_de_exec`, a port of `install_de`):
//       - `repairCurrent` — "Check my desktop";
//       - `repair` — "Check / Repair" on a card;
//       - `install` — "Install" on a card;
//       - `switchTo` — "Switch" (copies, logging out of the session);
//       - `remove` — "Remove": three safeguards — the current desktop has no
//         button, `qt_desktops::remove_plan` (not the current one and not
//         the last), and `remove_de(force=false)` in the core, which will
//         refuse a direct call too.
//     The order is as in Python (`DesktopsActions` + `confirm_de_install` +
//     `run_de_operation`), but without nested modal windows: one wizard
//     dialog per operation (rule #16) — the `operation` map, the log lines
//     with the `operationLog` signal, the outcome with the
//     `desktopsChanged` signal. A distribution refusal is shown BEFORE the
//     password prompt. The progress runs in a worker thread; cancellation
//     is possible only while the system is untouched.
//
//   * applies and reverts "As in the distribution" looks → executed by the
//     CORE (`rexon_core::distro_looks`: `offer_live`, `apply_live`,
//     `revert_live`), the window's texts and decisions are
//     `qt_desktops::look_*`:
//       - `lookApply` — the plan in the background, the question (the
//         "optional" checkbox — `setOperationOption`), the progress and the
//         outcome in the same operation window;
//       - `lookRevert` — "Revert to what it was".
//     Programs from the official repositories are installed through the
//     password window, from AUR — never: such packages are named to the user
//     as a list.
//
//   * takes and restores a copy of the current desktop's panel → executed
//     by Rust:
//       - `panelBackup` / `panelRestore`.
//     The executor is the core's panel protection for every desktop
//     (`backup::panel_save_live`, `panel_restore_live`), called with THE
//     SAME calls as in "Settings" (`settings_actions::panel_save_now` /
//     `panel_restore_now`, and the same restore question) — there is no
//     second copy in the product.
//
// Warning: there must be no SILENT "nothing happened" here (rule #13): the
// customer would take such a button for a breakage. So every method not yet
// ported returns `false` AND writes into `status` a line that the window
// shows in the footer. The same object also stands behind "DE comparison":
// the comparison page has the same `install`/`switchTo`, and there is no
// second action object with a stub any more.
//
// The background work technique is repeated after `ExtensionsActions`
// verbatim: a thread, `qt_thread()`, delivering the outcome with a signal.
// `pacman -Si` takes seconds, and a synchronous request would stop the tab
// exactly where it draws the card.

/// cxx-qt bridge of the action object.
///
/// The type is declared `#[qml_element]`, so the window host creates it
/// itself (`DesktopsActions { }`) and hands it to the page through the
/// `actions` property.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        /// How the last side action ended. The window shows this line in the
        /// footer: without it pressing a button with no executor behind it yet would
        /// look like a breakage.
        #[qproperty(QString, status, READ = status, NOTIFY = status_changed)]
        /// The desktop operation window: a question, a refusal, the progress or the
        /// outcome. A map with `phase` (`""` — no window, `blocked`, `confirm`,
        /// `password`, `running`, `done`) and all the labels — given by the core.
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        type DesktopsActions = super::DesktopsActionsRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DesktopsActions>);

        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut DesktopsActions>);

        /// A line of the progress log — appended by the window rather than sent
        /// whole as a map: a desktop installation has thousands of them.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut DesktopsActions>, line: QString);

        /// The set of installed desktops may have changed — the page will
        /// recompute itself. Operations now run in the background, and the methods
        /// no longer have a synchronous "did it work".
        #[qsignal]
        #[cxx_name = "desktopsChanged"]
        fn desktops_changed(self: Pin<&mut DesktopsActions>);

        /// The answer to `requestVersion`: `(deId, version)`. An empty version
        /// means "the package is not in the repositories or asking failed" — the
        /// page quietly hides the line instead of scaring with an error.
        #[qsignal]
        #[cxx_name = "versionReady"]
        fn version_ready(self: Pin<&mut DesktopsActions>, de_id: QString, version: QString);

        /// The answer to `requestInstalledVersion`: `(deId, ready string)`; empty
        /// means the version could not be found out, and the line is hidden.
        #[qsignal]
        #[cxx_name = "installedVersionReady"]
        fn installed_version_ready(self: Pin<&mut DesktopsActions>, de_id: QString, line: QString);

        /// The answer to `requestPackages`: `(deId, whether installed, the line
        /// about packages, a warning, the set will not install due to a conflict)`.
        /// An empty line means asking failed, and the page keeps its own count of
        /// names. `blocked` — the buttons that install the set are inactive
        /// (`qd::PackagesView::install_blocked`). `status` replaces the header
        /// ("Partially installed"), empty means the header as is.
        #[qsignal]
        #[cxx_name = "packagesReady"]
        fn packages_ready(
            self: Pin<&mut DesktopsActions>,
            de_id: QString,
            installed: bool,
            text: QString,
            notice: QString,
            blocked: bool,
            status: QString,
        );

        /// The second half of the answer to `requestPackages`: a package of the set
        /// is waiting to be rebuilt against a new library, and it can be built from
        /// the Arch sources — `(deId, whether installed, the button label, the
        /// explanation)`. An empty label means there is nothing to offer.
        #[qsignal]
        #[cxx_name = "rebuildOfferReady"]
        fn rebuild_offer_ready(
            self: Pin<&mut DesktopsActions>,
            de_id: QString,
            installed: bool,
            button: QString,
            hint: QString,
        );

        /// A request to show the built-in "DE comparison". The comparison is the
        /// section's second page, and the host switches them: the action object
        /// itself does not know how the window is arranged.
        #[qsignal]
        #[cxx_name = "compareRequested"]
        fn compare_requested(self: Pin<&mut DesktopsActions>);
    }

    extern "RustQt" {
        fn status(self: &DesktopsActions) -> QString;
        fn operation(self: &DesktopsActions) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// "Yes" in the question window — start the operation.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut DesktopsActions>);

        /// "No", "Got it", "Close" — remove the window (does not work while the
        /// operation is running).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut DesktopsActions>);

        /// After how many milliseconds the footer is removed by itself (0 — it is
        /// not removed). Decided by the core by the kind of line
        /// (`system_de_exec::footer_hold`); the window starts a timer and calls
        /// `expireStatus` by it.
        #[qinvokable]
        #[cxx_name = "statusTimeoutMs"]
        fn status_timeout_ms(self: &DesktopsActions) -> i32;

        /// The footer timer expired.
        #[qinvokable]
        #[cxx_name = "expireStatus"]
        fn expire_status(self: Pin<&mut DesktopsActions>);

        /// A click on the footer.
        #[qinvokable]
        #[cxx_name = "dismissStatus"]
        fn dismiss_status(self: Pin<&mut DesktopsActions>);

        /// The user moved to another section of the window.
        #[qinvokable]
        #[cxx_name = "sectionChanged"]
        fn section_changed(self: Pin<&mut DesktopsActions>);

        /// "Cancel" — only while the system is untouched.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut DesktopsActions>);

        /// The password step (SteamOS, "deck" has no password): set the password
        /// from two fields and continue the operation waiting for "Yes".
        #[qinvokable]
        #[cxx_name = "submitPassword"]
        fn submit_password(self: Pin<&mut DesktopsActions>, first: &QString, second: &QString);
    }

    extern "RustQt" {
        /// Installing a desktop. The signatures are the same as in the Python
        /// `DesktopsActions` class, and that is a condition of the check:
        /// `DesktopsPage.qml` must not change by a single line.
        #[qinvokable]
        fn install(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Checking and repairing a desktop.
        #[qinvokable]
        fn repair(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Switching to a desktop. The name is not `switch`: in JavaScript that is
        /// a keyword, and such a method cannot be called from QML.
        #[qinvokable]
        #[cxx_name = "switchTo"]
        fn switch_to(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Removing a desktop (both safeguards are in the core,
        /// `qt_desktops::remove_plan`).
        #[qinvokable]
        fn remove(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// "Check my desktop" — repairing the CURRENT desktop.
        #[qinvokable]
        #[cxx_name = "repairCurrent"]
        fn repair_current(self: Pin<&mut DesktopsActions>) -> bool;

        /// Take a copy of the KDE panel settings.
        #[qinvokable]
        #[cxx_name = "panelBackup"]
        fn panel_backup(self: Pin<&mut DesktopsActions>) -> bool;

        /// Restore the KDE panel from the last copy taken.
        #[qinvokable]
        #[cxx_name = "panelRestore"]
        fn panel_restore(self: Pin<&mut DesktopsActions>) -> bool;

        /// Show the built-in "DE comparison".
        #[qinvokable]
        #[cxx_name = "openCompare"]
        fn open_compare(self: Pin<&mut DesktopsActions>);

        /// The "Shells" block: install shell `shellId` into compositor `deId` (a
        /// question with warnings, then the packages and the autostart line).
        #[qinvokable]
        #[cxx_name = "shellInstall"]
        fn shell_install(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Log into the compositor with the shell (logging out of the session).
        #[qinvokable]
        #[cxx_name = "shellSwitch"]
        fn shell_switch(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Remove the shell from the compositor; the desktop itself stays.
        #[qinvokable]
        #[cxx_name = "shellRemove"]
        fn shell_remove(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Ask for the version of the desktop's representative package in the
        /// repositories. The answer comes with the `versionReady` signal.
        #[qinvokable]
        #[cxx_name = "requestVersion"]
        fn request_version(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// The version of the installed desktop (the same key package as for
        /// "Available…") and, if the repository has a different one, that one. The
        /// answer comes with the `installedVersionReady` signal.
        #[qinvokable]
        #[cxx_name = "requestInstalledVersion"]
        fn request_installed_version(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// How many packages will REALLY be installed (groups expanded,
        /// dependencies taken) and whether the set in the repository is broken — by
        /// a trial transaction without root rights. The answer comes with the
        /// `packagesReady` signal.
        #[qinvokable]
        #[cxx_name = "requestPackages"]
        fn request_packages(self: Pin<&mut DesktopsActions>, de_id: &QString, installed: bool);

        /// "Rebuild from the Arch sources" under the warning about a package that
        /// is waiting to be rebuilt: a question, then building and installing in the
        /// same operation window (`repo_rebuild::rebuild_live`).
        #[qinvokable]
        #[cxx_name = "rebuildFromSources"]
        fn rebuild_from_sources(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// The "As in the distribution" block: "Apply" look `look_id`. First the
        /// plan is assembled in the background (package probes — seconds), then the
        /// question in the operation window, applying and the outcome.
        #[qinvokable]
        #[cxx_name = "lookApply"]
        fn look_apply(self: Pin<&mut DesktopsActions>, look_id: &QString);

        /// "Revert to what it was" for look `look_id`: the question, the progress
        /// and the outcome.
        #[qinvokable]
        #[cxx_name = "lookRevert"]
        fn look_revert(self: Pin<&mut DesktopsActions>, look_id: &QString);

        /// The "optional" checkbox in the question window (SteamOS game mode) was
        /// toggled: the question is rebuilt by a different plan.
        #[qinvokable]
        #[cxx_name = "setOperationOption"]
        fn set_operation_option(self: Pin<&mut DesktopsActions>, checked: bool);

        /// A variant switch in the question window (SteamOS style: Vapor or VGUI2)
        /// — property `name` of the look got value `value`: the question is rebuilt
        /// by a different plan.
        #[qinvokable]
        #[cxx_name = "setOperationChoice"]
        fn set_operation_choice(self: Pin<&mut DesktopsActions>, name: &QString, value: &QString);
    }

    // The trait declaration must be IN THE BRIDGE: cxx-qt emits the thread type
    // alias from it, without which the background request would not reach the
    // signal.
    impl cxx_qt::Threading for DesktopsActions {}
}

use core::pin::Pin;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::de_progress as dp;
use rexon_core::distro_looks as dl;
use rexon_core::qt_compare as qc;
use rexon_core::qt_desktops as qd;
use rexon_core::system;
use rexon_core::system_de_exec as dx;
use rexon_core::shell_bundle as sb;
use rexon_core::system::DesktopShell;
use rexon_core::deepin_treeland as dt;
use rexon_core::repo_rebuild;
use rexon_core::screensaver_guard as sg;
use rexon_core::steamos_sysext as sx;
use rexon_core::system_plan;
use rexon_core::utils;

/// Rust state of the action object.
///
/// The version cache lives for the session — exactly as in Python: the
/// package manager is not poked a second time for the same version. Next to
/// it is the list of requests in flight: the page already guards itself with
/// the `versionPending` flag, but it is one per card, and requests come here
/// both with a desktop change and with the appearance of the action object.
#[derive(Default)]
pub struct DesktopsActionsRust {
    status: String,
    /// The kind of the footer line — its lifetime depends on it
    /// (`dx::footer_hold`).
    status_kind: dx::FooterKind,
    versions: Arc<Mutex<HashMap<String, String>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
    /// Installation preview by desktop: `None` inside means asking failed.
    /// Reset after every operation: the system composition changed.
    previews: Arc<Mutex<HashMap<String, Option<system_plan::InstallPreview>>>>,
    previews_in_flight: Arc<Mutex<HashSet<String>>>,
    /// The desktop's packages that are waiting to be rebuilt and are built from
    /// the Arch sources (`repo_rebuild::find_candidates`). They live and are
    /// reset together with the preview.
    rebuild_offers: Arc<Mutex<HashMap<String, Vec<repo_rebuild::Candidate>>>>,
    /// Deepin on Treeland (`deepin_treeland::card_state`): what to offer on the
    /// Deepin card with the same button under the warning. Lives and is reset
    /// together with the preview.
    treeland_offers: Arc<Mutex<HashMap<String, dt::Offer>>>,
    /// A set conflict is resolved by a replacement with the user's consent
    /// (`system_plan::conflict_swaps`, River 0.5 → river-classic): what to
    /// replace. The same button under the warning; lives and is reset together
    /// with the preview.
    swap_offers: Arc<Mutex<HashMap<String, Vec<system_plan::PkgConflict>>>>,
    /// Installed desktops to which a missing session can be added
    /// (`system::missing_session`): Wayland or X11. The same button under the
    /// warning; lives and is reset together with the preview.
    session_offers: Arc<Mutex<HashMap<String, system::SessionKind>>>,
    op: OpView,
    pending: Option<PendingOp>,
    cancel: Option<dx::CancelToken>,
    /// "Check / Repair" of the current desktop found that the desktop's own
    /// screen saver is not running or a foreign one is: the question is shown
    /// when the outcome window is closed (`sg::survey_live`).
    screensaver_offer: Option<sg::Restore>,
}

/// What the operation window shows. All labels come from the core.
///
/// Shared by all owners of the `DeOperationDialog.qml` window — "Desktops",
/// "Comparison" and "Settings" (installing what is missing): the handling of
/// the progress and the outcome is one, so that the windows do not drift
/// apart in behaviour.
#[derive(Default, Clone)]
pub(crate) struct OpView {
    pub(crate) phase: String,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) informative: String,
    pub(crate) status: String,
    pub(crate) bar: i32,
    pub(crate) current: String,
    pub(crate) close_text: String,
    pub(crate) close_enabled: bool,
    pub(crate) cancellable: bool,
    pub(crate) result_text: String,
    /// An irreversible action: "Yes" in red.
    pub(crate) danger: bool,
    /// Our own "Yes"/"No" labels of the question (empty — "Yes"/"No"): for
    /// restoring the look these are "🛠 Restore" and "Not now", as in the Python
    /// window.
    pub(crate) yes_text: String,
    pub(crate) no_text: String,
    /// Password step: why the password was not accepted (empty — all is well).
    pub(crate) password_error: String,
    /// Password step: `passwd` is running — the fields and buttons are
    /// disabled.
    pub(crate) password_busy: bool,
    /// The "optional" checkbox in the question window (empty — no checkbox) and
    /// the warning under it (simple HTML).
    pub(crate) option_text: String,
    pub(crate) option_hint: String,
    pub(crate) option_checked: bool,
    /// Variant switches of the question window (empty — there are none).
    pub(crate) choices: Vec<qd::LookSelect>,
}

impl OpView {
    /// The progress window of a just started operation.
    pub(crate) fn running(title: &str) -> Self {
        OpView {
            phase: "running".to_string(),
            title: title.to_string(),
            status: dp::initial_status(),
            close_text: dp::close_text_busy(),
            ..OpView::default()
        }
    }

    /// The question window.
    pub(crate) fn confirm(title: &str, text: &str) -> Self {
        OpView {
            phase: "confirm".to_string(),
            title: title.to_string(),
            text: html(text),
            ..OpView::default()
        }
    }

    /// The SteamOS password step: "deck" has no password, and `pkexec` cannot
    /// be passed without one. "Yes" — "Set password", "No" — "Cancel".
    pub(crate) fn password(device: sx::Device) -> Self {
        OpView {
            phase: "password".to_string(),
            title: sx::password_title(),
            text: html(&sx::password_text(device)),
            yes_text: sx::password_button(),
            no_text: dx::dialog_cancel(),
            ..OpView::default()
        }
    }

    /// The map for the markup: the state and the button labels.
    pub(crate) fn to_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut m = QMap::<QMapPair_QString_QVariant>::default();
        let s = |v: &str| QVariant::from(&QString::from(v));
        m.insert(QString::from("phase"), s(&self.phase));
        m.insert(QString::from("title"), s(&self.title));
        m.insert(QString::from("text"), s(&self.text));
        m.insert(QString::from("informative"), s(&self.informative));
        m.insert(QString::from("status"), s(&self.status));
        m.insert(QString::from("bar"), QVariant::from(&self.bar));
        m.insert(QString::from("barMax"), QVariant::from(&dp::BAR_MAX));
        m.insert(QString::from("current"), s(&self.current));
        m.insert(QString::from("closeText"), s(&self.close_text));
        m.insert(QString::from("closeEnabled"), QVariant::from(&self.close_enabled));
        m.insert(QString::from("cancellable"), QVariant::from(&self.cancellable));
        m.insert(QString::from("resultText"), s(&self.result_text));
        m.insert(QString::from("danger"), QVariant::from(&self.danger));
        let or = |own: &str, fallback: String| {
            if own.is_empty() {
                fallback
            } else {
                own.to_string()
            }
        };
        m.insert(QString::from("yesText"), s(&or(&self.yes_text, dx::dialog_yes())));
        m.insert(QString::from("noText"), s(&or(&self.no_text, dx::dialog_no())));
        m.insert(QString::from("okText"), s(&dp::confirm_blocked_button()));
        m.insert(QString::from("cancelText"), s(&dx::dialog_cancel()));
        if self.phase == "password" {
            m.insert(QString::from("passwordNewText"), s(&sx::password_field_new()));
            m.insert(QString::from("passwordRepeatText"), s(&sx::password_field_repeat()));
        }
        m.insert(QString::from("passwordError"), s(&self.password_error));
        m.insert(QString::from("passwordBusy"), QVariant::from(&self.password_busy));
        m.insert(QString::from("optionText"), s(&self.option_text));
        m.insert(QString::from("optionHint"), s(&self.option_hint));
        m.insert(QString::from("optionChecked"), QVariant::from(&self.option_checked));
        let mut choices = QList::<QVariant>::default();
        for c in &self.choices {
            let mut options = QList::<QVariant>::default();
            for o in &c.options {
                let mut om = QMap::<QMapPair_QString_QVariant>::default();
                om.insert(QString::from("value"), s(&o.value));
                om.insert(QString::from("label"), s(&o.label));
                om.insert(QString::from("description"), s(&o.description));
                om.insert(QString::from("enabled"), QVariant::from(&o.enabled));
                options.append(QVariant::from(&om));
            }
            let mut cm = QMap::<QMapPair_QString_QVariant>::default();
            cm.insert(QString::from("name"), s(&c.name));
            cm.insert(QString::from("label"), s(&c.label));
            cm.insert(QString::from("current"), s(&c.current));
            cm.insert(QString::from("options"), QVariant::from(&options));
            choices.append(QVariant::from(&cm));
        }
        m.insert(QString::from("choices"), QVariant::from(&choices));
        m
    }

    /// A progress message. `false` means the window is no longer in progress
    /// and there is nothing to change; otherwise a line for the window's log,
    /// if there is one.
    pub(crate) fn apply_progress(
        &mut self,
        stage: &str,
        message: &str,
        fraction: Option<f64>,
    ) -> Option<Option<String>> {
        if self.phase != "running" {
            return None;
        }
        let update = dp::on_progress(stage, message, fraction);
        if let Some(bar) = update.bar {
            self.bar = bar;
        }
        if let Some(status) = update.status {
            self.status = status.to_string();
        }
        if let Some(current) = update.current {
            self.current = current;
        }
        Some(update.log_line)
    }

    /// The outcome of the operation. `result` is the note under the log on
    /// success. Returns a line for the window's log, if there is one.
    pub(crate) fn apply_done(&mut self, outcome: &dx::DeOpOutcome, result: String) -> Option<String> {
        let done = dp::on_done(outcome.ok, &outcome.message);
        self.phase = "done".to_string();
        if let Some(bar) = done.bar {
            self.bar = bar;
        }
        self.status = if outcome.cancelled {
            dx::status_cancelled()
        } else {
            done.status
        };
        if let Some(current) = done.current {
            self.current = current;
        }
        self.close_text = done.close_text;
        self.close_enabled = done.close_enabled;
        self.cancellable = false;
        if outcome.ok {
            self.result_text = result;
        }
        done.log_line
    }
}

/// An operation waiting for "Yes".
#[derive(Clone)]
enum PendingOp {
    /// Installing a desktop — it is also checking and repairing.
    Install {
        de_id: String,
        /// Installed packages the user agreed to replace (empty — an ordinary
        /// installation).
        replace: Vec<String>,
        operation_title: String,
        done_title: String,
        done_text: String,
        /// The outcome if the executor says "a reboot is needed" (empty — the
        /// same).
        done_text_restart: String,
        /// The chosen session (`None` — the desktop has no choice).
        session: Option<system::SessionKind>,
        /// "➕ Add variant …" on an installed desktop's card: only the `session`
        /// session is installed (`dx::add_session_live`), not the desktop's set.
        add_session: bool,
    },
    /// Switching to another desktop with logging out of the session.
    Switch {
        target: String,
        operation_title: String,
    },
    /// Removing a desktop (the safeguards are already passed, the core will
    /// check once more).
    Remove {
        de_id: String,
        operation_title: String,
    },
    /// Building packages waiting to be rebuilt from the Arch sources.
    Rebuild {
        packages: Vec<String>,
        operation_title: String,
    },
    /// Deepin on Treeland: move the installed ordinary Deepin (X11) to Treeland
    /// (`update == false`) or update our deepin-session build.
    Treeland {
        update: bool,
        operation_title: String,
    },
    /// The "compositor + shell" pair: installation, login, removal.
    Shell {
        job: ShellJob,
        wm: String,
        shell: DesktopShell,
        operation_title: String,
        /// The note under the log on success (simple HTML).
        result: String,
    },
    /// "Apply" an "As in the distribution" look: the core's offer and the
    /// choice of the "optional" checkbox; the session desktop is taken at the
    /// press.
    Look {
        offer: Box<dl::Offer>,
        /// The chosen variants (empty — the defaults).
        values: rexon_core::rxn_manifest::PropValues,
        opt_in: bool,
        de: String,
        operation_title: String,
    },
    /// "Revert to what it was" of a look.
    LookRevert {
        id: String,
        de: String,
        operation_title: String,
    },
    /// Bring the desktop's own screen saver back (found by "Check / Repair").
    ScreensaverRestore(sg::Restore),
}

/// What to do with the shell.
#[derive(Clone, Copy)]
enum ShellJob {
    Install,
    Switch,
    Remove,
}

/// What the worker thread executes.
enum Job {
    Install(String, Vec<String>, Option<system::SessionKind>),
    /// Add a session to an installed desktop.
    AddSession(String, system::SessionKind),
    Switch(String),
    Remove(String),
    Rebuild(Vec<String>),
    Treeland { update: bool },
    Shell(ShellJob, String, DesktopShell),
    /// The look, the plan (`None` — nothing to apply), what to revert first,
    /// the desktop.
    LookApply {
        look: Box<dl::Look>,
        plan: Option<Box<dl::LookPlan>>,
        replaces: Vec<(String, String)>,
        de: String,
    },
    LookRevert {
        id: String,
        de: String,
    },
    ScreensaverRestore(sg::Restore),
}

/// The core's simple HTML for the window: line breaks become `<br>` (in
/// `RichText` a bare line break is a space).
fn html(text: &str) -> String {
    text.replace('\n', "<br>")
}

impl qobject::DesktopsActions {
    fn status(&self) -> QString {
        QString::from(&self.rust().status)
    }

    /// The footer line and its kind. A repeat of the same info or error line
    /// also notifies: the window restarts its timer, otherwise a repeated
    /// "the operation is already running" would disappear before its time.
    fn set_status(mut self: Pin<&mut Self>, text: String, kind: dx::FooterKind) {
        if self.rust().status == text
            && self.rust().status_kind == kind
            && (kind == dx::FooterKind::Pending || text.is_empty())
        {
            return;
        }
        self.as_mut().rust_mut().status = text;
        self.as_mut().rust_mut().status_kind = kind;
        self.as_mut().status_changed();
    }

    fn status_timeout_ms(&self) -> i32 {
        if self.rust().status.is_empty() {
            return 0;
        }
        dx::footer_hold(self.rust().status_kind).timeout_ms as i32
    }

    fn expire_status(self: Pin<&mut Self>) {
        let hold = dx::footer_hold(self.rust().status_kind);
        if hold.timeout_ms > 0 {
            self.set_status(String::new(), dx::FooterKind::Pending);
        }
    }

    fn dismiss_status(self: Pin<&mut Self>) {
        if dx::footer_hold(self.rust().status_kind).click_dismisses {
            self.set_status(String::new(), dx::FooterKind::Pending);
        }
    }

    fn section_changed(self: Pin<&mut Self>) {
        if dx::footer_hold(self.rust().status_kind).clear_on_section_change {
            self.set_status(String::new(), dx::FooterKind::Pending);
        }
    }

    // ── Reading and navigation: executed by Rust ─────────────────────────

    fn open_compare(mut self: Pin<&mut Self>) {
        self.as_mut().compare_requested();
    }

    fn request_version(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }

        // Already asked — we give the answer immediately, with the same signal. An
        // empty string in the cache is an answer too: "no package", and there is
        // no reason to ask again.
        let cached = self
            .as_ref()
            .get_ref()
            .rust()
            .versions
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(version) = cached {
            self.as_mut()
                .version_ready(QString::from(&de_id), QString::from(&version));
            return;
        }

        {
            let Ok(mut flight) = self.as_ref().get_ref().rust().in_flight.lock() else {
                return;
            };
            if !flight.insert(de_id.clone()) {
                return;
            }
        }

        let versions = self.as_ref().get_ref().rust().versions.clone();
        let in_flight = self.as_ref().get_ref().rust().in_flight.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let version = repo_version(&de_id).unwrap_or_default();
            if let Ok(mut map) = versions.lock() {
                map.insert(de_id.clone(), version.clone());
            }
            if let Ok(mut flight) = in_flight.lock() {
                flight.remove(&de_id);
            }
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .version_ready(QString::from(&de_id), QString::from(&version));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Silence is not allowed: without an answer the page would stay with
                    // `versionPending` raised and would never ask again.
                    rexon_core::log_warn!("desktops", "версия стола не доставлена странице: {e:?}");
                }
            }
        });
    }

    fn request_installed_version(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }
        let versions = self.as_ref().get_ref().rust().versions.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let cached = versions.lock().ok().and_then(|map| map.get(&de_id).cloned());
            let available = match cached {
                Some(v) => v,
                None => {
                    let v = repo_version(&de_id).unwrap_or_default();
                    if let Ok(mut map) = versions.lock() {
                        map.insert(de_id.clone(), v.clone());
                    }
                    v
                }
            };
            let installed = installed_version(&de_id).unwrap_or_default();
            let newer = repo_is_newer(&installed, &available);
            let line = qd::installed_version_line(&installed, &available, newer);
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .installed_version_ready(QString::from(&de_id), QString::from(&line));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("desktops", "версия установленного стола не доставлена странице: {e:?}");
                }
            }
        });
    }

    fn request_packages(mut self: Pin<&mut Self>, de_id: &QString, installed: bool) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }
        let cached = self
            .as_ref()
            .get_ref()
            .rust()
            .previews
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(preview) = cached {
            let view = packages_view_of(&de_id, installed, preview.as_ref());
            self.as_mut().packages_ready(
                QString::from(&de_id),
                installed,
                QString::from(&view.text),
                QString::from(&view.notice),
                view.install_blocked,
                QString::from(&view.status_override),
            );
            let offered = self
                .as_ref()
                .get_ref()
                .rust()
                .rebuild_offers
                .lock()
                .ok()
                .is_some_and(|map| map.get(&de_id).is_some_and(|c| !c.is_empty()));
            let treeland = self
                .as_ref()
                .get_ref()
                .rust()
                .treeland_offers
                .lock()
                .ok()
                .and_then(|map| map.get(&de_id).cloned());
            let swaps = self
                .as_ref()
                .get_ref()
                .rust()
                .swap_offers
                .lock()
                .ok()
                .and_then(|map| map.get(&de_id).cloned())
                .unwrap_or_default();
            let session = self
                .as_ref()
                .get_ref()
                .rust()
                .session_offers
                .lock()
                .ok()
                .and_then(|map| map.get(&de_id).copied());
            let session_de = session.map(|kind| (de_id.as_str(), kind));
            let (button, hint) = offer_texts(treeland.as_ref(), &swaps, offered, session_de);
            self.as_mut().rebuild_offer_ready(
                QString::from(&de_id),
                installed,
                QString::from(&button),
                QString::from(&hint),
            );
            return;
        }
        {
            let Ok(mut flight) = self.as_ref().get_ref().rust().previews_in_flight.lock() else {
                return;
            };
            if !flight.insert(de_id.clone()) {
                return;
            }
        }
        let previews = self.as_ref().get_ref().rust().previews.clone();
        let offers = self.as_ref().get_ref().rust().rebuild_offers.clone();
        let treeland_offers = self.as_ref().get_ref().rust().treeland_offers.clone();
        let swap_offers = self.as_ref().get_ref().rust().swap_offers.clone();
        let session_offers = self.as_ref().get_ref().rust().session_offers.clone();
        let in_flight = self.as_ref().get_ref().rust().previews_in_flight.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let family = system::DistroDetector::detect_live().family_live();
            let packages = system::get_packages(&de_id, family);
            // The probe of the ordinary set. For Deepin it cannot be replaced by the
            // probe of the variant entirely: `card_state` takes it as a fallback (with
            // deepin-kwin forbidden) if the Treeland probe did not work out, and
            // outside Arch and on an immutable system it is the only one.
            let preview = system_plan::install_preview_live(family, &packages);
            // Deepin is installed only on Treeland: if the ordinary (X11) one is
            // installed, offer to move it to Treeland; if our build is already
            // installed, show the probe of its set and offer an update.
            let treeland = dt::card_state_live(&de_id, family, preview.as_ref());
            let preview = treeland.preview.map(Some).unwrap_or(preview);
            // AUR packages in the desktop's set: one RPC request for the whole list
            // (risk signals for the card's warning line).
            // Also here — the AUR add-on of a Wayland session (none for now; for MATE
            // (Polo)): the question window shows its risk signals without the network.
            let session_aur = system::session_aur_packages(&de_id, family, system::SessionKind::Wayland);
            if !installed {
                let mut names = system::get_aur_packages(&de_id, family);
                names.extend(session_aur.iter().cloned());
                aur_trust_warm(&names);
            } else {
                aur_trust_warm(&session_aur);
            }
            // The desktop is installed, but one of its sessions cannot be logged into
            // — offer to add the missing one.
            let session_offer = installed_sessions_live(&de_id, family)
                .filter(|_| installed)
                .and_then(system::missing_session);
            if let Ok(mut map) = session_offers.lock() {
                match session_offer {
                    Some(kind) => map.insert(de_id.clone(), kind),
                    None => map.remove(&de_id),
                };
            }
            // What from the set does not install because it is waiting to be rebuilt
            // against a new library — that can be built from the Arch sources. With a
            // conflict with something installed a rebuild will not help — we do not
            // offer it (`qd::rebuild_can_help`).
            let broken: Vec<String> = preview
                .as_ref()
                .filter(|p| qd::rebuild_can_help(p))
                .map(|p| p.blocked.iter().chain(&p.skipped).cloned().collect())
                .unwrap_or_default();
            let candidates = repo_rebuild::find_candidates_live(&broken);
            // A conflict with another branch of the same program (River 0.5) — a
            // replacement button with a question.
            let swaps = preview.as_ref().map(system_plan::conflict_swaps).unwrap_or_default();
            let (button, hint) = offer_texts(
                treeland.offer.as_ref(),
                &swaps,
                !candidates.is_empty(),
                session_offer.map(|kind| (de_id.as_str(), kind)),
            );
            if let Ok(mut map) = swap_offers.lock() {
                if swaps.is_empty() {
                    map.remove(&de_id);
                } else {
                    map.insert(de_id.clone(), swaps);
                }
            }
            if let Ok(mut map) = offers.lock() {
                map.insert(de_id.clone(), candidates);
            }
            if let Ok(mut map) = treeland_offers.lock() {
                match treeland.offer {
                    Some(offer) => map.insert(de_id.clone(), offer),
                    None => map.remove(&de_id),
                };
            }
            if let Ok(mut map) = previews.lock() {
                map.insert(de_id.clone(), preview.clone());
            }
            if let Ok(mut flight) = in_flight.lock() {
                flight.remove(&de_id);
            }
            let view = packages_view_of(&de_id, installed, preview.as_ref());
            let queued = handle.queue(move |mut object| {
                object.as_mut().packages_ready(
                    QString::from(&de_id),
                    installed,
                    QString::from(&view.text),
                    QString::from(&view.notice),
                    view.install_blocked,
                    QString::from(&view.status_override),
                );
                object.as_mut().rebuild_offer_ready(
                    QString::from(&de_id),
                    installed,
                    QString::from(&button),
                    QString::from(&hint),
                );
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("desktops", "состав пакетов стола не доставлен странице: {e:?}");
                }
            }
        });
    }

    // ── Installation, switching, removal: the core has no executor yet ───

    fn install(self: Pin<&mut Self>, de_id: &QString) -> bool {
        // A port of `DesktopsActions.install`: the shared question with the
        // compatibility analysis (`confirm_de_install`), then `install_de` in the
        // core.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        // A desktop with two sessions (Cinnamon, Xfce, LXQt, MATE): "X11 / Wayland
        // / Both" cards in the same window, with the recommended one preselected.
        if let Some(pick) = session_offer_default(&de_id) {
            self.ask_install_session(&de_id, pick);
            return false;
        }
        self.ask_install(&de_id, qd::install_texts(&name), Vec::new(), None, None);
        false
    }

    /// The installation question with the chosen session variant `pick`.
    fn ask_install_session(self: Pin<&mut Self>, de_id: &str, pick: system::SessionPick) {
        let family = system::DistroDetector::detect_live().family_live();
        let helper = aur_helper_present(family);
        let risk = session_aur_risk(de_id, family);
        let texts = qd::install_texts_with_session(de_id, family, pick, helper, &risk);
        let choice = qd::session_choice(de_id, family, pick, helper);
        // Wayland is unavailable (MATE without an AUR helper) — X11 is installed.
        let kind = qd::session_pick_effective(de_id, family, pick, helper).install_kind();
        self.ask_install(de_id, texts, Vec::new(), Some(kind), choice);
    }

    fn repair(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // A port of `DesktopsActions.repair`: a repeated standard installation —
        // the package manager with `--needed` skips what is installed by itself.
        // The progress and the outcome come later with signals, so the method
        // always answers `false`.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        let current = current_de().unwrap_or_default();
        if self.as_mut().refused_on_steamos("repair", &de_id, &current, &qd::repair_texts(&name).operation_title) {
            return false;
        }
        self.ask_install(&de_id, qd::repair_texts(&name), Vec::new(), None, None);
        false
    }

    /// SteamOS: a button the card shows as inactive
    /// (`qd::steamos_button_block`) does not go further on a direct call either
    /// — a window with the same reason. `true` means refused.
    fn refused_on_steamos(mut self: Pin<&mut Self>, kind: &str, de_id: &str, current: &str, operation_title: &str) -> bool {
        let steamos = rexon_core::distro_compat::running_on_steamos();
        let Some(why) = qd::steamos_button_block(kind, de_id, de_id == current, steamos) else {
            return false;
        };
        self.as_mut().set_status(dx::operation_footer(operation_title, &why), dx::FooterKind::Pending);
        self.show_notice(operation_title, &why);
        true
    }

    fn switch_to(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // A port of `DesktopsActions.switchTo`: a question, then `switch_de` in the
        // core (copies, default applications, logging out of the session). The core
        // will not switch to the current desktop — it has no button anyway.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return false;
        }
        // The switching texts are shared with "DE comparison" — they are verbatim
        // the same there, and a second copy would silently drift apart.
        let texts = qc::switch_texts(&name);
        self.as_mut().rust_mut().pending = Some(PendingOp::Switch {
            target: de_id,
            operation_title: texts.operation_title.clone(),
        });
        self.as_mut()
            .set_status(dx::awaiting_answer(&texts.operation_title), dx::FooterKind::Pending);
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: texts.confirm_title.to_string(),
            text: html(&texts.confirm_text),
            ..OpView::default()
        });
        false
    }

    fn remove(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // A port of `DesktopsActions.remove`. There are three safeguards, and the
        // core decides all of them:
        //   1. the current desktop has no "Remove" button (`card_buttons`);
        //   2. `qt_desktops::remove_plan` — not the current one and not the last
        //      installed one, otherwise a window with an explanation without a
        //      remove button;
        //   3. `remove_de(force=false)` in the executor — a direct call will refuse
        //      too.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return false;
        }
        let current = current_de().unwrap_or_default();
        if self.as_mut().refused_on_steamos("remove", &de_id, &current, &qd::remove_operation_title(&name)) {
            return false;
        }
        let others = system::DE_META
            .iter()
            .filter(|m| m.id != de_id && crate::compare_page::de_installed(m.id))
            .count();
        let plan = qd::remove_plan(&de_id, &name, &current, others);
        let operation_title = qd::remove_operation_title(&name);
        if !plan.allowed() {
            self.as_mut().rust_mut().pending = None;
            self.as_mut()
                .set_status(dx::operation_footer(&operation_title, &plan.title), dx::FooterKind::Pending);
            self.set_op(OpView {
                phase: "blocked".to_string(),
                title: plan.title.to_string(),
                text: html(&plan.text),
                ..OpView::default()
            });
            return false;
        }
        let text = qd::steamos_remove_text(&de_id, sx::live_release().is_some()).unwrap_or(plan.text);
        self.as_mut().rust_mut().pending = Some(PendingOp::Remove {
            de_id,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title), dx::FooterKind::Pending);
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: plan.title.to_string(),
            text: html(&text),
            danger: true,
            ..OpView::default()
        });
        false
    }

    fn repair_current(mut self: Pin<&mut Self>) -> bool {
        // A port of `DesktopsActions.repairCurrent`: repairing the CURRENT
        // desktop. Not recognized — a window with an explanation, not silence.
        let current = current_de().filter(|de| known_name(de).is_some());
        let Some(current) = current else {
            let (title, text) = (qd::repair_current_unknown_title(), qd::repair_current_unknown_text());
            self.as_mut().set_status(format!("{title} — {text}"), dx::FooterKind::Pending);
            self.show_notice(&title, &text);
            return false;
        };
        let texts = qd::repair_texts(qd::display_name(&current));
        if self.as_mut().refused_on_steamos("repair_current", &current, &current, &texts.operation_title) {
            return false;
        }
        self.ask_install(&current, texts, Vec::new(), None, None);
        false
    }

    // ── Operation window ─────────────────────────────────────────────────

    fn operation(&self) -> QMap<QMapPair_QString_QVariant> {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn busy(&self) -> bool {
        self.rust().op.phase == "running"
    }

    /// A message window with one "Got it" button.
    fn show_notice(self: Pin<&mut Self>, title: &str, text: &str) {
        self.set_op(OpView {
            phase: "blocked".to_string(),
            title: title.to_string(),
            text: html(text),
            ..OpView::default()
        });
    }

    /// A port of `confirm_de_install`: a distribution refusal — as a window
    /// without an "install" button BEFORE the password prompt; a warning — in
    /// the same question. `replace` is the consent to replace installed packages
    /// (the replacement button); then "Yes" is red: what is installed will be
    /// removed.
    ///
    /// `session` is the session that will be installed (`None` — the desktop has
    /// no choice); `choice` is the session switch in the window (`None` — do not
    /// show).
    fn ask_install(
        mut self: Pin<&mut Self>,
        de_id: &str,
        texts: qd::OpTexts,
        replace: Vec<String>,
        session: Option<system::SessionKind>,
        choice: Option<qd::LookSelect>,
    ) {
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return;
        }
        // SteamOS: an image desktop gets its own question (built from Valve
        // packages, rebuilt after updates), not the package one.
        let version = sx::live_release().map(|r| r.version_id);
        let texts = qd::steamos_op_texts(texts, de_id, version.as_deref());
        let verdict = dx::install_verdict_live(de_id);
        match dp::confirm_install_plan(Some(&verdict), &texts.confirm_html) {
            dp::ConfirmPlan::Blocked {
                window_title,
                text,
                informative,
                ..
            } => {
                self.as_mut().rust_mut().pending = None;
                self.as_mut()
                    .set_status(dx::operation_footer(&texts.operation_title, &window_title), dx::FooterKind::Pending);
                self.set_op(OpView {
                    phase: "blocked".to_string(),
                    title: window_title,
                    text: html(&text),
                    informative: html(&informative),
                    ..OpView::default()
                });
            }
            dp::ConfirmPlan::Ask { body } => {
                let danger = !replace.is_empty();
                self.as_mut().rust_mut().pending = Some(PendingOp::Install {
                    de_id: de_id.to_string(),
                    replace,
                    operation_title: texts.operation_title.clone(),
                    done_title: texts.done_title.to_string(),
                    done_text: texts.done_text.clone(),
                    done_text_restart: texts.done_text_restart.clone(),
                    session,
                    add_session: false,
                });
                self.as_mut()
                    .set_status(dx::awaiting_answer(&texts.operation_title), dx::FooterKind::Pending);
                self.set_op(OpView {
                    phase: "confirm".to_string(),
                    title: texts.confirm_title.to_string(),
                    text: html(&body),
                    danger,
                    choices: choice.into_iter().collect(),
                    ..OpView::default()
                });
            }
        }
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        if self.busy() {
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        // SteamOS: "deck" without a password cannot pass `pkexec` — first the
        // password step in this same window, and the operation waits for it in
        // `pending`.
        match steamos_admin_gate(&pending) {
            None | Some(sx::AdminGate::Ready) => {}
            Some(sx::AdminGate::AskPassword) => {
                let title = pending_title(&pending);
                self.as_mut().rust_mut().pending = Some(pending);
                self.as_mut().set_status(dx::awaiting_answer(&title), dx::FooterKind::Pending);
                self.set_op(OpView::password(sx::live_device()));
                return;
            }
            Some(sx::AdminGate::Refused(why)) => {
                let title = pending_title(&pending);
                self.as_mut().set_status(dx::operation_footer(&title, &why), dx::FooterKind::Pending);
                self.show_notice(&title, &why);
                return;
            }
        }
        self.start_operation(pending);
    }

    /// Start the operation answered with "Yes" (and, if needed, with a password
    /// set): the progress window and the worker thread.
    fn start_operation(mut self: Pin<&mut Self>, pending: PendingOp) {
        // The "reboot" outcome instead of the ordinary one — when the executor
        // decided by the installation plan that what was done will not work
        // without a reboot (`DeOpOutcome::restart_needed`).
        let mut restart_result: Option<String> = None;
        let (operation_title, result, job) = match pending {
            PendingOp::Install {
                de_id,
                replace,
                operation_title,
                done_title,
                done_text,
                done_text_restart,
                session,
                add_session,
            } => {
                if !done_text_restart.is_empty() {
                    restart_result = Some(format!("<b>{done_title}</b><br>{}", html(&done_text_restart)));
                }
                // An empty outcome (a SteamOS image desktop): the executor writes it.
                let result = if done_text.is_empty() {
                    String::new()
                } else {
                    format!("<b>{done_title}</b><br>{}", html(&done_text))
                };
                let job = match session {
                    Some(kind) if add_session => Job::AddSession(de_id, kind),
                    _ => Job::Install(de_id, replace, session),
                };
                (operation_title, result, job)
            }
            // Success of a switch is logging out of the session; there is no outcome
            // note.
            PendingOp::Switch {
                target,
                operation_title,
            } => (operation_title, String::new(), Job::Switch(target)),
            PendingOp::Remove {
                de_id,
                operation_title,
            } => (operation_title, String::new(), Job::Remove(de_id)),
            // The outcome (what was built and that Arch will send its own rebuild) is
            // written into the log by the window according to `DeOpOutcome`; no note
            // under the log is needed.
            PendingOp::Rebuild {
                packages,
                operation_title,
            } => (operation_title, String::new(), Job::Rebuild(packages)),
            PendingOp::Treeland {
                update,
                operation_title,
            } => {
                let result = if update { String::new() } else { html(&dt::install_result_text()) };
                (operation_title, result, Job::Treeland { update })
            }
            PendingOp::Shell {
                job,
                wm,
                shell,
                operation_title,
                result,
            } => (operation_title, result, Job::Shell(job, wm, shell)),
            // The outcome (what was applied, what was skipped, what to install from
            // AUR) is written by the core — the worker thread assembles the note.
            PendingOp::Look {
                offer,
                values,
                opt_in,
                de,
                operation_title,
            } => {
                let plan = qd::look_chosen_plan(&offer, &values, opt_in).cloned().map(Box::new);
                let offer = *offer;
                let job = Job::LookApply {
                    look: Box::new(offer.look),
                    plan,
                    replaces: offer.replaces,
                    de,
                };
                (operation_title, String::new(), job)
            }
            PendingOp::LookRevert {
                id,
                de,
                operation_title,
            } => (operation_title, String::new(), Job::LookRevert { id, de }),
            PendingOp::ScreensaverRestore(r) => {
                (sg::restore_operation_title(), String::new(), Job::ScreensaverRestore(r))
            }
        };
        let token = dx::CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        self.as_mut().set_op(OpView::running(&operation_title));
        let handle = self.as_mut().qt_thread();
        let current = current_de().unwrap_or_else(|| "unknown".to_string());
        // Installing or repairing the CURRENT desktop is its "Check / Repair":
        // the session's screen saver is checked too.
        let check_saver = matches!(&job, Job::Install(de_id, ..) if *de_id == current);
        std::thread::spawn(move || {
            let sink: Arc<dyn dx::DeOpSink> = Arc::new(QtSink {
                handle: handle.clone(),
            });
            // The outcome of an "As in the distribution" look — in the core's words
            // under the log.
            let mut job_result: Option<String> = None;
            let outcome = match job {
                Job::LookApply {
                    look,
                    plan,
                    replaces,
                    de,
                } => {
                    let report = |stage: &str, message: &str| sink.progress(stage, message, None);
                    let applied = match plan {
                        Some(plan) => dl::apply_live(&look, &plan, &replaces, &de, &report),
                        None => Err(qd::look_nothing_to_apply()),
                    };
                    look_outcome(&*sink, applied.map(|a| a.message), &mut job_result)
                }
                Job::LookRevert { id, de } => {
                    let reverted = dl::revert_live(&id, &de).map(|r| r.message);
                    look_outcome(&*sink, reverted, &mut job_result)
                }
                Job::ScreensaverRestore(r) => look_outcome(&*sink, sg::restore_live(&r), &mut job_result),
                Job::AddSession(de_id, kind) => dx::add_session_live(sink, token, &de_id, kind),
                Job::Install(de_id, replace, session) if replace.is_empty() => {
                    dx::install_de_live(sink, token, &de_id, &current, session)
                }
                Job::Install(de_id, replace, session) => {
                    dx::install_de_replacing_live(sink, token, &de_id, &current, &replace, session)
                }
                Job::Switch(target) => dx::switch_de_live(sink, token, &target, &current),
                Job::Remove(de_id) => dx::remove_de_live(sink, token, &de_id, &current),
                Job::Rebuild(packages) => repo_rebuild::rebuild_live(sink, token, &packages),
                Job::Treeland { update: false } => dt::install_live(sink, token, &current),
                Job::Treeland { update: true } => dt::update_live(sink, token),
                Job::Shell(ShellJob::Install, wm, shell) => dx::shell_install_live(sink, token, &wm, shell),
                Job::Shell(ShellJob::Switch, wm, shell) => {
                    dx::shell_switch_live(sink, token, &wm, shell, &current)
                }
                Job::Shell(ShellJob::Remove, wm, shell) => {
                    dx::shell_remove_live(sink, token, &wm, shell, &current)
                }
            };
            // The set of desktops may have changed (even on failure — part of it got
            // installed): several notification daemons have one bus name, and without
            // our choice someone else's would draw the notifications.
            rexon_core::notifications_slot::sync_live();
            // The same for screen saver launchers: a new desktop may have brought
            // another saver whose settings would show in foreign desktops.
            sg::sync_live();
            let saver_offer = if check_saver { sg::survey_live() } else { None };
            let mut result = match restart_result {
                Some(text) if outcome.restart_needed => text,
                _ => job_result.unwrap_or(result),
            };
            if saver_offer.is_some() {
                result = format!("{result}<br><br>{}", html(&sg::found_note()));
            }
            deliver(&handle, move |object| {
                object.finish_operation(outcome, operation_title, result, saver_offer);
            });
        });
    }

    fn finish_operation(
        mut self: Pin<&mut Self>,
        outcome: dx::DeOpOutcome,
        title: String,
        result: String,
        saver_offer: Option<sg::Restore>,
    ) {
        self.as_mut().rust_mut().screensaver_offer = saver_offer;
        let mut op = self.rust().op.clone();
        if let Some(line) = op.apply_done(&outcome, result) {
            self.as_mut().operation_log(QString::from(&line));
        }
        self.as_mut().rust_mut().cancel = None;
        let footer = dx::operation_footer(&title, &op.status);
        self.as_mut().set_op(op);
        self.as_mut().set_status(footer, dx::outcome_footer_kind(&outcome));
        // The system composition may have changed even on failure (part of it got
        // installed): the previous preview is no longer true.
        if let Ok(mut map) = self.as_ref().get_ref().rust().previews.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().rebuild_offers.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().treeland_offers.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().swap_offers.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().session_offers.lock() {
            map.clear();
        }
        if outcome.ok {
            self.desktops_changed();
        }
    }

    fn apply_progress(mut self: Pin<&mut Self>, stage: String, message: String, fraction: Option<f64>) {
        let mut op = self.rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
        }
        // Most pacman lines do not touch the bar and the labels; the `operation`
        // notification recomputes all bindings of the window, so it is sent only
        // when what is visible has really changed.
        let old = &self.rust().op;
        if op.bar != old.bar || op.status != old.status || op.current != old.current {
            self.set_op(op);
        }
    }

    fn apply_cancellable(self: Pin<&mut Self>, allowed: bool) {
        let mut op = self.rust().op.clone();
        if op.phase != "running" {
            return;
        }
        op.cancellable = allowed;
        self.set_op(op);
    }

    fn dismiss_operation(mut self: Pin<&mut Self>) {
        if self.busy() || self.rust().op.password_busy {
            return;
        }
        // The question or refusal window was closed — the footer line "answer in
        // the window that opened" is no longer true and has to be removed. The
        // outcome of a finished operation stays: it is still correct.
        let phase = self.rust().op.phase.clone();
        if phase == "confirm" || phase == "blocked" || phase == "password" {
            self.as_mut().set_status(String::new(), dx::FooterKind::Pending);
        }
        self.as_mut().rust_mut().pending = None;
        // "Check / Repair" found a screen saver problem: the outcome window is
        // closed, and now comes the question with the one button that fixes it.
        if phase == "done" {
            if let Some(offer) = self.as_mut().rust_mut().screensaver_offer.take() {
                let text = sg::restore_text(&offer);
                self.as_mut().rust_mut().pending = Some(PendingOp::ScreensaverRestore(offer));
                self.as_mut()
                    .set_status(dx::awaiting_answer(&sg::restore_operation_title()), dx::FooterKind::Pending);
                self.set_op(OpView {
                    phase: "confirm".to_string(),
                    title: sg::restore_title(),
                    text,
                    ..OpView::default()
                });
                return;
            }
        }
        self.set_op(OpView::default());
    }

    fn cancel_operation(mut self: Pin<&mut Self>) {
        if !self.busy() || !self.rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.rust().cancel {
            token.cancel();
        }
        // The button is disabled at once: a second click adds nothing, and the
        // executor will stop at the nearest safe point.
        let mut op = self.rust().op.clone();
        op.cancellable = false;
        self.as_mut().set_op(op);
    }

    fn submit_password(mut self: Pin<&mut Self>, first: &QString, second: &QString) {
        let mut op = self.rust().op.clone();
        if op.phase != "password" || op.password_busy || self.rust().pending.is_none() {
            return;
        }
        let (first, second) = (first.to_string(), second.to_string());
        if let Err(why) = sx::check_new_password(&first, &second) {
            op.password_error = why;
            self.set_op(op);
            return;
        }
        // `passwd` with PAM delays takes seconds; the window waits with the fields
        // disabled, and it cannot be closed during that time.
        op.password_error = String::new();
        op.password_busy = true;
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let result = sx::set_password_live(&first);
            drop(first);
            deliver(&handle, move |object| object.password_done(result));
        });
    }

    /// `passwd` answered: set — the operation goes on; not set — its words
    /// under the fields, another password can be tried.
    fn password_done(mut self: Pin<&mut Self>, result: Result<(), String>) {
        let mut op = self.rust().op.clone();
        if op.phase != "password" {
            return;
        }
        op.password_busy = false;
        if let Err(why) = result {
            rexon_core::log_warn!("steamos", "passwd не задал пароль: {why}");
            op.password_error = why;
            self.set_op(op);
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            self.set_op(OpView::default());
            return;
        };
        self.as_mut().start_operation(pending);
        self.as_mut().operation_log(QString::from(&sx::password_set_text()));
    }

    // ── Panel protection: executed by the core, with the same call as "Settings" ─
    //
    // Parity with the Python tab (`DesktopsActions.panelBackup/panelRestore`
    // in `rexon/qt_desktops/window.py`): the copy is taken without a question,
    // the restore only after "Yes" to the same question of the core; the
    // outcome is the core's `panel_*_result` texts. Both methods return
    // `false`: the set of installed desktops does not change from the panel,
    // and there is no reason for the page to recompute it.

    fn panel_backup(self: Pin<&mut Self>) -> bool {
        // One executor for every desktop and both tabs: a new copy never
        // replaces an older one (up to ten per desktop, the original kept),
        // so there is nothing to ask.
        let de = current_de().unwrap_or_default();
        let (ok, message) = match crate::settings_actions::panel_save_now(&de) {
            Ok(message) => (true, message),
            Err(detail) => (false, detail),
        };
        self.set_status(panel_status(&qd::panel_backup_result(ok, &message)), dx::result_footer_kind(ok));
        false
    }

    fn panel_restore(mut self: Pin<&mut Self>) -> bool {
        let de = current_de().unwrap_or_default();
        let Some(snap) = rexon_core::backup::panel_latest_live(&de) else {
            let why = rexon_core::backup::msg_no_plasma_backups();
            self.set_status(panel_status(&qd::panel_restore_result(false, &why)), dx::result_footer_kind(false));
            return false;
        };
        // The same question as in "Settings": the desktop, the date, the other
        // desktops whose shortcuts change too, a note when they wait for a login.
        let (_, question) = crate::settings_actions::panel_restore_question(&snap);
        let agreed = crate::extensions_actions::ask_confirm(
            &QString::from(&qd::panel_restore_confirm_title()),
            &QString::from(&question),
        );
        if !agreed {
            // The user changed their mind — there is nothing to say (as in Python).
            return false;
        }
        let (ok, message) = match crate::settings_actions::panel_restore_now(&snap.path) {
            Ok((_, message)) => (true, message),
            Err(message) => (false, message),
        };
        let status = panel_status(&qd::panel_restore_result(ok, &message));
        self.as_mut().set_status(status.clone(), dx::result_footer_kind(ok));
        // GNOME: the settings were restored, the missing extensions from the copy
        // are installed from extensions.gnome.org in a worker thread (the shell's
        // confirmation window waits for the user); the outcome is appended to the
        // status line.
        if ok && snap.de == "gnome" {
            let handle = self.as_mut().qt_thread();
            let dir = snap.path.clone();
            std::thread::spawn(move || {
                let Some(report) = rexon_core::backup::gnome_ext_copy_deliver_live_from(&dir) else {
                    return;
                };
                let text = format!("{status} — {}", report.replace('\n', " "));
                if let Err(e) = handle.queue(move |object| object.set_status(text, dx::FooterKind::Notice)) {
                    if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                        rexon_core::log_warn!("desktops", "GNOME extension delivery outcome was not delivered: {e:?}");
                    }
                }
            });
        }
        false
    }

    // ── The "Shells" block ───────────────────────────────────────────────
    //
    // The question, the refusal and the progress — in the same operation
    // window as for desktops. Refusals (SteamOS, not in the repositories, no
    // desktop itself) — as a window without "Yes" BEFORE the password; the
    // password is asked by the core's executor only if packages need to be
    // installed or removed.

    /// Parse the "compositor, shell" pair and show a refusal if there is none.
    fn shell_target(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> Option<(String, DesktopShell)> {
        let wm = de_id.to_string();
        let shell = sb::shell_by_id(&shell_id.to_string());
        match shell {
            Some(shell) if sb::SHELL_WMS.contains(&wm.as_str()) => {
                if self.busy() {
                    self.set_status(dx::busy_message(), dx::FooterKind::Notice);
                    return None;
                }
                Some((wm, shell))
            }
            _ => {
                self.as_mut().report_unknown(&wm);
                None
            }
        }
    }

    // ── A package waiting to be rebuilt: build from the Arch sources ─────

    fn rebuild_from_sources(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return;
        }
        // The same button on the Deepin card — move the ordinary Deepin to
        // Treeland (`Offer::Install`) or update our build.
        let treeland = self
            .as_ref()
            .get_ref()
            .rust()
            .treeland_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(offer) = treeland {
            let (update, operation_title, text) = match &offer {
                dt::Offer::Install => (false, dt::install_title(), dt::install_confirm_text()),
                dt::Offer::Update { local, repo } => (true, dt::update_title(), dt::update_confirm_text(local, repo)),
            };
            self.as_mut().rust_mut().pending = Some(PendingOp::Treeland {
                update,
                operation_title: operation_title.clone(),
            });
            self.as_mut().set_status(dx::awaiting_answer(&operation_title), dx::FooterKind::Pending);
            self.set_op(OpView::confirm(&operation_title, &text));
            return;
        }
        // The same button — replacing another branch of the same program (River
        // 0.5): the ordinary installation question with the replacement text and
        // the consent to it.
        let swaps = self
            .as_ref()
            .get_ref()
            .rust()
            .swap_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned())
            .unwrap_or_default();
        if !swaps.is_empty() {
            let Some(name) = known_name(&de_id) else {
                self.report_unknown(&de_id);
                return;
            };
            let mut texts = if crate::compare_page::de_installed(&de_id) {
                qd::repair_texts(&name)
            } else {
                qd::install_texts(&name)
            };
            texts.confirm_title = system_plan::conflict_swap_title(&swaps);
            texts.confirm_html = system_plan::conflict_swap_confirm_text(&swaps, &name);
            let replace: Vec<String> = swaps.iter().map(|c| c.installed.clone()).collect();
            self.ask_install(&de_id, texts, replace, None, None);
            return;
        }
        let candidates = self
            .as_ref()
            .get_ref()
            .rust()
            .rebuild_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned())
            .unwrap_or_default();
        // The same button — add a missing session (Wayland or X11) to an installed
        // desktop (it yields to a rebuild: `offer_texts` decides the same way).
        let session = self
            .as_ref()
            .get_ref()
            .rust()
            .session_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).copied());
        if let Some(kind) = session.filter(|_| candidates.is_empty()) {
            let family = system::DistroDetector::detect_live().family_live();
            let helper = aur_helper_present(family);
            let pick = match kind {
                system::SessionKind::X11 => system::SessionPick::X11,
                system::SessionKind::Wayland => system::SessionPick::Wayland,
            };
            if qd::session_pick_effective(&de_id, family, pick, helper) != pick {
                // MATE without an AUR helper: explain rather than install in vain.
                let name = qd::display_name(&de_id).to_string();
                let title = qd::session_add_texts(&de_id, family, kind, helper, "").confirm_title;
                self.show_notice(&title, &qd::session_no_aur_helper_text(&name));
                return;
            }
            let texts = qd::session_add_texts(&de_id, family, kind, helper, &session_aur_risk(&de_id, family));
            self.as_mut().ask_install(&de_id, texts, Vec::new(), Some(kind), None);
            if let Some(PendingOp::Install { add_session, .. }) = self.as_mut().rust_mut().pending.as_mut() {
                *add_session = true;
            }
            return;
        }
        if candidates.is_empty() {
            // The button outlived the offer: the system composition changed after the
            // operation, and there is nothing to build any more.
            self.set_status(repo_rebuild::offer_gone(), dx::FooterKind::Notice);
            return;
        }
        let packages: Vec<String> = candidates.iter().map(|c| c.package.clone()).collect();
        let operation_title = repo_rebuild::operation_title(&packages);
        self.as_mut().rust_mut().pending = Some(PendingOp::Rebuild {
            packages,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title), dx::FooterKind::Pending);
        self.set_op(OpView::confirm(&operation_title, &repo_rebuild::confirm_text(&candidates)));
    }

    fn ask_shell(mut self: Pin<&mut Self>, pending: PendingOp, title: &str, text: &str, danger: bool) {
        let operation_title = match &pending {
            PendingOp::Shell { operation_title, .. } => operation_title.clone(),
            _ => String::new(),
        };
        self.as_mut().rust_mut().pending = Some(pending);
        self.as_mut().set_status(dx::awaiting_answer(&operation_title), dx::FooterKind::Pending);
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: title.to_string(),
            text: html(text),
            danger,
            ..OpView::default()
        });
    }

    fn shell_install(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let wm_name = qd::display_name(&wm).to_string();
        let family = system::DistroDetector::detect_live().family_live();
        let refusal = if let Some(why) = rexon_core::distro_compat::SteamOsBlock::Desktops.live() {
            Some(why)
        } else if let Err(u) = sb::packages(shell, &wm, family) {
            Some(sb::unavailable_text(u, shell, &wm_name))
        } else if !crate::compare_page::de_installed(&wm) {
            Some(sb::install_desktop_first(&wm_name))
        } else {
            None
        };
        let texts_title = qd::shell_install_texts(shell, &wm, &[], &[]).operation_title;
        if let Some(why) = refusal {
            self.as_mut().set_status(dx::operation_footer(&texts_title, &why), dx::FooterKind::Pending);
            self.show_notice(&texts_title, &why);
            return false;
        }
        // DMS looks into ~/.config/hypr bypassing XDG_CONFIG_HOME — and so do we.
        let hypr = format!("{}/.config/hypr", std::env::var("HOME").unwrap_or_default());
        let hypr_lua = std::path::Path::new(&format!("{hypr}/hyprland.lua")).is_file();
        let hypr_strays = std::path::Path::new(&format!("{hypr}/hyprland.conf")).is_file()
            || std::fs::read_dir(format!("{hypr}/dms"))
                .map(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "conf")))
                .unwrap_or(false);
        let warnings = sb::install_warnings(shell, &wm, hypr_lua, hypr_strays);
        // We name what will really be installed (empty — no password is needed).
        let candidates = sb::packages(shell, &wm, family).unwrap_or_default();
        self.ask_shell_after_query(candidates, false, move |packages| {
            let t = qd::shell_install_texts(shell, &wm, packages, &warnings);
            let pending = PendingOp::Shell {
                job: ShellJob::Install,
                wm,
                shell,
                operation_title: t.operation_title.clone(),
                result: format!("<b>{}</b><br>{}", t.done_title, html(&t.done_text)),
            };
            (pending, t.confirm_title, t.confirm_html, false)
        });
        false
    }

    /// Ask the question about the pair once it becomes clear which packages
    /// from `candidates` are installed: the installed ones (`installed`) or the
    /// not installed ones are kept.
    ///
    /// `pacman -Q` for each package takes seconds — in the interface thread the
    /// window froze for that time. The check runs in the background, and the
    /// question arrives in the Qt thread ready.
    fn ask_shell_after_query(
        mut self: Pin<&mut Self>,
        candidates: Vec<&'static str>,
        installed: bool,
        question: impl FnOnce(&[&str]) -> (PendingOp, String, String, bool) + Send + 'static,
    ) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let sys = rexon_core::system_exec::live();
            let packages: Vec<&str> = candidates.into_iter().filter(|p| sys.is_pkg_installed(p) == installed).collect();
            let (pending, title, text, danger) = question(&packages);
            deliver(&handle, move |object| object.ask_shell(pending, &title, &text, danger));
        });
    }

    fn shell_switch(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let t = qd::shell_switch_texts(shell, &wm);
        let pending = PendingOp::Shell {
            job: ShellJob::Switch,
            wm,
            shell,
            operation_title: t.operation_title.clone(),
            result: String::new(),
        };
        self.ask_shell(pending, &t.confirm_title, &t.confirm_html, false);
        false
    }

    fn shell_remove(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let family = system::DistroDetector::detect_live().family_live();
        let home = std::env::var("HOME").unwrap_or_default();
        let cfg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
        let read = |p: &str| std::fs::read_to_string(p).ok();
        let still: Vec<&str> = sb::SHELL_WMS
            .iter()
            .copied()
            .filter(|o| *o != wm && sb::block_installed(o, shell, &home, cfg.as_deref(), &read))
            .collect();
        let candidates = sb::packages_to_remove(shell, &wm, family, &still);
        self.ask_shell_after_query(candidates, true, move |packages| {
            let t = qd::shell_remove_texts(shell, &wm, packages);
            let pending = PendingOp::Shell {
                job: ShellJob::Remove,
                wm,
                shell,
                operation_title: t.operation_title.clone(),
                result: String::new(),
            };
            (pending, t.confirm_title, t.confirm_html, true)
        });
        false
    }

    // ── The "As in the distribution" block ───────────────────────────────
    //
    // In the same operation window as for desktops: the plan in the
    // background → the question (the "optional" checkbox) → (the SteamOS
    // password) → the progress in a worker thread → the outcome. Texts and
    // decisions are the core's (`qt_desktops::look_*`, `distro_looks`).

    fn look_apply(mut self: Pin<&mut Self>, look_id: &QString) {
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return;
        }
        let id = look_id.to_string();
        let Some(de) = current_de() else {
            let (title, text) = qd::look_unknown_desktop();
            self.show_notice(&title, &text);
            return;
        };
        let title = qd::look_apply_title(&qd::look_name(&id));
        let mut op = OpView::running(&title);
        op.status = qd::look_checking();
        self.as_mut().set_status(qd::look_checking(), dx::FooterKind::Pending);
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let offer = dl::offer_live(&id, &de);
            deliver(&handle, move |object| object.look_offered(offer, de, title));
        });
    }

    /// The plan is assembled: a question or a refusal.
    fn look_offered(mut self: Pin<&mut Self>, offer: Result<dl::Offer, String>, de: String, title: String) {
        match offer {
            Ok(offer) => self.show_look_confirm(offer, Default::default(), false, de, title),
            Err(why) => {
                self.as_mut().set_status(dx::operation_footer(&title, &why), dx::FooterKind::Pending);
                self.show_notice(&title, &why);
            }
        }
    }

    fn show_look_confirm(
        mut self: Pin<&mut Self>,
        offer: dl::Offer,
        values: rexon_core::rxn_manifest::PropValues,
        opt_in: bool,
        de: String,
        title: String,
    ) {
        match qd::look_confirm(&offer, &values, opt_in) {
            qd::LookConfirm::Blocked { title: window_title, text } => {
                self.as_mut().rust_mut().pending = None;
                self.as_mut().set_status(dx::operation_footer(&title, &window_title), dx::FooterKind::Pending);
                self.set_op(OpView {
                    phase: "blocked".to_string(),
                    title: window_title,
                    text,
                    ..OpView::default()
                });
            }
            qd::LookConfirm::Ask {
                title: window_title,
                text,
                yes_text,
                no_text,
                choices,
                option,
            } => {
                let (option_text, option_hint) = option.map(|o| (o.text, o.hint)).unwrap_or_default();
                self.as_mut().rust_mut().pending = Some(PendingOp::Look {
                    offer: Box::new(offer),
                    values,
                    opt_in,
                    de,
                    operation_title: title.clone(),
                });
                self.as_mut().set_status(dx::awaiting_answer(&title), dx::FooterKind::Pending);
                self.set_op(OpView {
                    phase: "confirm".to_string(),
                    title: window_title,
                    text,
                    yes_text,
                    no_text,
                    option_checked: opt_in && !option_text.is_empty(),
                    option_text,
                    option_hint,
                    choices,
                    ..OpView::default()
                });
            }
        }
    }

    fn set_operation_option(mut self: Pin<&mut Self>, checked: bool) {
        if self.rust().op.phase != "confirm" {
            return;
        }
        match self.as_mut().rust_mut().pending.take() {
            Some(PendingOp::Look {
                offer,
                values,
                de,
                operation_title,
                ..
            }) => self.show_look_confirm(*offer, values, checked, de, operation_title),
            other => self.as_mut().rust_mut().pending = other,
        }
    }

    fn set_operation_choice(mut self: Pin<&mut Self>, name: &QString, value: &QString) {
        if self.rust().op.phase != "confirm" {
            return;
        }
        match self.as_mut().rust_mut().pending.take() {
            Some(PendingOp::Look {
                offer,
                mut values,
                opt_in,
                de,
                operation_title,
            }) => {
                values.insert(name.to_string(), serde_json::Value::String(value.to_string()));
                self.show_look_confirm(*offer, values, opt_in, de, operation_title)
            }
            // The session switch in the "Install?" window: the question is rebuilt —
            // the explanation and the outcome line change.
            Some(PendingOp::Install { de_id, replace, .. })
                if name.to_string() == qd::SESSION_CHOICE_NAME && replace.is_empty() =>
            {
                // The values come from our own variants; an unknown one is the safe X11,
                // not a window without an answer to "Yes".
                let pick = system::SessionPick::from_value(&value.to_string()).unwrap_or(system::SessionPick::X11);
                self.ask_install_session(&de_id, pick);
            }
            other => self.as_mut().rust_mut().pending = other,
        }
    }

    fn look_revert(mut self: Pin<&mut Self>, look_id: &QString) {
        if self.busy() {
            self.set_status(dx::busy_message(), dx::FooterKind::Notice);
            return;
        }
        let id = look_id.to_string();
        let Some(de) = current_de() else {
            let (title, text) = qd::look_unknown_desktop();
            self.show_notice(&title, &text);
            return;
        };
        let name = qd::look_name(&id);
        let (title, text, yes_text) = qd::look_revert_confirm(&name);
        let operation_title = qd::look_revert_title(&name);
        self.as_mut().rust_mut().pending = Some(PendingOp::LookRevert {
            id,
            de,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title), dx::FooterKind::Pending);
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title,
            text: html(&text),
            yes_text,
            no_text: dx::dialog_cancel(),
            ..OpView::default()
        });
    }

    fn report_unknown(self: Pin<&mut Self>, de_id: &str) {
        self.set_status(rexon_core::i18n::trf("Rexon doesn't know the desktop “%1”.", &[&de_id]), dx::FooterKind::Error);
    }
}

// ============================================================================
// Delivering the progress from the worker thread
// ============================================================================

/// Receiver of the core executor's progress: every event goes to the Qt
/// thread.
struct QtSink {
    handle: CxxQtThread<qobject::DesktopsActions>,
}

impl dx::DeOpSink for QtSink {
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
        let (stage, message) = (stage.to_string(), message.to_string());
        deliver(&self.handle, move |object| {
            object.apply_progress(stage, message, fraction);
        });
    }

    fn cancellable(&self, allowed: bool) {
        deliver(&self.handle, move |object| object.apply_cancellable(allowed));
    }
}

/// Queue work into the Qt thread; if the window is already closed —
/// silently, otherwise into the log.
fn deliver(
    handle: &CxxQtThread<qobject::DesktopsActions>,
    work: impl FnOnce(Pin<&mut qobject::DesktopsActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("desktops", "ход операции со столом не доставлен окну: {e:?}");
        }
    }
}

/// A footer line from the core's KDE panel outcome. In Python this was a
/// window with a title and text; the footer has one line, so a line break
/// inside the text becomes a space.
fn panel_status(texts: &qd::PanelTexts) -> String {
    format!("{} — {}", texts.title, texts.text.replace('\n', " "))
}

/// The display name of a desktop; `None` means the core does not know such
/// a desktop.
/// SteamOS and an image desktop (installation, check, removal): what to do
/// before the helper under the password. `None` — not SteamOS or not an
/// image operation.
fn steamos_admin_gate(pending: &PendingOp) -> Option<sx::AdminGate> {
    // The look installs programs through the password window — on SteamOS
    // "deck" without a password cannot pass it. If it installs nothing, no
    // password is needed.
    if let PendingOp::Look { offer, values, opt_in, .. } = pending {
        let installs = qd::look_chosen_plan(offer, values, *opt_in).is_some_and(|p| !p.install.is_empty());
        if !installs || sx::live_release().is_none() {
            return None;
        }
        return Some(sx::admin_gate(sx::live_password_state(), sx::live_device()));
    }
    let de_id = match pending {
        PendingOp::Install { de_id, .. } | PendingOp::Remove { de_id, .. } => de_id,
        _ => return None,
    };
    if !matches!(sx::support(de_id), sx::Support::Supported(_)) || sx::live_release().is_none() {
        return None;
    }
    Some(sx::admin_gate(sx::live_password_state(), sx::live_device()))
}

/// Title of the operation waiting for "Yes" — for the footer and the
/// refusal window.
fn pending_title(pending: &PendingOp) -> String {
    match pending {
        PendingOp::Install { operation_title, .. }
        | PendingOp::Switch { operation_title, .. }
        | PendingOp::Remove { operation_title, .. }
        | PendingOp::Rebuild { operation_title, .. }
        | PendingOp::Treeland { operation_title, .. }
        | PendingOp::Shell { operation_title, .. }
        | PendingOp::Look { operation_title, .. }
        | PendingOp::LookRevert { operation_title, .. } => operation_title.clone(),
        PendingOp::ScreensaverRestore(_) => sg::restore_operation_title(),
    }
}

/// The outcome of applying or reverting a look: success — a note under the
/// log in the core's words; failure — the reason as an error line in the log
/// (the window's outcome on failure does not write its own line).
fn look_outcome(sink: &dyn dx::DeOpSink, result: Result<String, String>, job_result: &mut Option<String>) -> dx::DeOpOutcome {
    match result {
        Ok(message) => {
            *job_result = Some(qd::look_result_html(&message));
            dx::DeOpOutcome {
                ok: true,
                message: dp::MSG_OK.to_string(),
                cancelled: false,
                restart_needed: false,
            }
        }
        Err(why) => {
            sink.progress(dp::STAGE_ERROR, &why, None);
            dx::DeOpOutcome {
                ok: false,
                message: why,
                cancelled: false,
                restart_needed: false,
            }
        }
    }
}

fn known_name(de_id: &str) -> Option<String> {
    system::DE_META
        .iter()
        .find(|meta| meta.id == de_id)
        .map(|meta| meta.display_name.to_string())
}

/// The current desktop by the session environment; `None` means not
/// recognized.
///
/// The same answer is given by `SystemInfo.current_de` in Python and by
/// `DesktopsModel` next door: there is no third copy of the detection here,
/// the core is called.
fn current_de() -> Option<String> {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    system::detect_current_de(&xdg, &session, wayland).filter(|de| !de.is_empty())
}

/// The card's "packages" block from the trial transaction's answer; without
/// an answer — the core's name count, as on the page before the answer.
/// Labels of the rebuild offer: empty means there is nothing to offer.
/// The button label and the explanation: move the ordinary Deepin to
/// Treeland or update our deepin-session build.
fn treeland_offer_texts(offer: &dt::Offer) -> (String, String) {
    match offer {
        dt::Offer::Install => (dt::install_button(), dt::install_hint()),
        dt::Offer::Update { local, repo } => (dt::update_button(), dt::update_hint(local, repo)),
    }
}

/// Labels of the button under the card's warning: the Deepin on Treeland
/// variant, otherwise replacing the conflicting one, otherwise a rebuild.
///
/// Last — adding a Wayland session to an installed desktop (`session_de` is
/// the desktop it is offered to): a broken set matters more than a wish.
fn offer_texts(
    treeland: Option<&dt::Offer>,
    swaps: &[system_plan::PkgConflict],
    rebuild_offered: bool,
    session_de: Option<(&str, system::SessionKind)>,
) -> (String, String) {
    match treeland {
        Some(offer) => treeland_offer_texts(offer),
        None if !swaps.is_empty() => (
            system_plan::conflict_swap_button(swaps),
            system_plan::conflict_swap_hint(swaps),
        ),
        None if rebuild_offered => rebuild_offer_texts(true),
        None => match session_de {
            Some((de, kind)) => (qd::session_offer_button(kind), qd::session_offer_hint(de, kind)),
            None => (String::new(), String::new()),
        },
    }
}

fn rebuild_offer_texts(offered: bool) -> (String, String) {
    if offered {
        (repo_rebuild::offer_button(), repo_rebuild::offer_hint())
    } else {
        (String::new(), String::new())
    }
}

fn packages_view_of(
    de_id: &str,
    installed: bool,
    preview: Option<&system_plan::InstallPreview>,
) -> qd::PackagesView {
    let family = system::DistroDetector::detect_live().family_live();
    let main = system::get_packages(de_id, family).len();
    let aur_names = system::get_aur_packages(de_id, family);
    let mut view = qd::packages_view(installed, preview, main, aur_names.len());
    // Which sessions of an installed desktop work: "Sessions: X11 + Wayland".
    if installed {
        let line = installed_sessions_live(de_id, family)
            .map(|s| qd::session_status_line(de_id, family, s))
            .unwrap_or_default();
        if !line.is_empty() {
            view.text = if view.text.is_empty() { line } else { format!("{}\n{line}", view.text) };
        }
    }
    // Risk signals of AUR packages (the same as the badge on an AUR tile in
    // "Appearance"): only data already obtained by `aur_trust_warm` — without
    // the network, this path is called from the Qt thread too. It does not
    // forbid the installation.
    if !installed && !aur_names.is_empty() {
        let now = rexon_core::aur_trust::now_secs();
        let views = rexon_core::aur_trust::rpc_views(None, &aur_names, now);
        let path = rexon_core::aur_trust::live_cache_path();
        let risk = rexon_core::aur_trust::desktop_notice_for(&aur_names, &views, &path, now);
        if !risk.is_empty() {
            if !view.notice.is_empty() {
                view.notice.push('\n');
            }
            view.notice.push_str(&risk);
        }
    }
    view
}

/// Ask AUR about the packages of the desktop's set (one RPC request, the
/// answer is kept in memory). Without the network — silently nothing.
/// The default session if the installation window needs the "X11 / Wayland"
/// switch: the desktop has both sessions in this family and this is not
/// SteamOS (there desktops are installed as an image, without a package
/// set).
fn session_offer_default(de_id: &str) -> Option<system::SessionPick> {
    if rexon_core::distro_compat::running_on_steamos() {
        return None;
    }
    let family = system::DistroDetector::detect_live().family_live();
    qd::session_default(de_id, family)
}

/// Sessions of an installed desktop from the system files
/// (`system::installed_sessions`). On SteamOS desktops are installed as an
/// image — there is no session choice there.
fn installed_sessions_live(de_id: &str, family: system::PkgFamily) -> Option<(bool, bool)> {
    if rexon_core::distro_compat::running_on_steamos() {
        return None;
    }
    system::installed_sessions(
        de_id,
        family,
        &|f| std::path::Path::new(f).exists(),
        &system::binary_in_path,
    )
}

/// Whether there is an AUR helper (yay, paru…) — the same choice as the
/// executor's.
fn aur_helper_present(family: system::PkgFamily) -> bool {
    system_plan::pick_aur_helper(family, &system::binary_in_path).is_some()
}

/// Risk signals of the AUR add-on of a Wayland session — from what
/// `aur_trust_warm` already obtained, without the network (called from the
/// Qt thread).
fn session_aur_risk(de_id: &str, family: system::PkgFamily) -> String {
    let names = system::session_aur_packages(de_id, family, system::SessionKind::Wayland);
    if names.is_empty() {
        return String::new();
    }
    let now = rexon_core::aur_trust::now_secs();
    let views = rexon_core::aur_trust::rpc_views(None, &names, now);
    let path = rexon_core::aur_trust::live_cache_path();
    rexon_core::aur_trust::desktop_notice_for(&names, &views, &path, now)
}

fn aur_trust_warm(names: &[String]) {
    if names.is_empty() {
        return;
    }
    let mut net = rexon_core::net::UreqNet::new();
    rexon_core::aur_trust::rpc_views(Some(&mut net), names, rexon_core::aur_trust::now_secs());
}

// ============================================================================
// Package version from the distribution's repositories
// ============================================================================

/// The version of the desktop's representative package in the repositories
/// of the CURRENT distribution, or `None`.
///
/// A port of `rexon.system.query_repo_version`: the core makes the decisions
/// (choosing the command and parsing the output), here there is only
/// `which` and the launch. Quietly, without panicking: "asking failed" for
/// this line is the same as "no package", it simply will not be shown.
fn repo_version(de_id: &str) -> Option<String> {
    let package = system::get_check_binary(de_id)?;
    let family = system::DistroDetector::detect_live().family_live();
    let query = system_plan::repo_version_query(family, package)?;
    // The binary is checked BEFORE the launch — as in the original: on a
    // distribution without this manager the launch would give "not found"
    // instead of an honest "I do not know".
    utils::which(&query.binary, None)?;

    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            ..Default::default()
        },
        None,
    );
    let rc = report.returncode?;
    let stdout = utils::decode_text(&report.stdout);
    if stdout.is_empty() {
        return None;
    }
    if query.check_returncode && rc != 0 {
        return None;
    }
    system_plan::parse_repo_version(family, &stdout)
}

/// The installed version of the desktop's key package (without the epoch
/// and the release number, like the version from the repository), or
/// `None`.
fn installed_version(de_id: &str) -> Option<String> {
    let package = system::get_check_binary(de_id)?;
    let family = system::DistroDetector::detect_live().family_live();
    let query = system_plan::installed_version_query(family, package)?;
    utils::which(&query.binary, None)?;
    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            ..Default::default()
        },
        None,
    );
    let ok = report.returncode == Some(0);
    let raw = system_plan::parse_installed_version(family, ok, &utils::decode_text(&report.stdout))?;
    let clean = system_plan::clean_pkg_version(&raw);
    (!clean.is_empty()).then_some(clean)
}

/// Whether the version in the repository is newer than the installed one:
/// `vercmp` from pacman (Arch). Where there is none — `None`, and the line
/// does not claim an update came out.
fn repo_is_newer(installed: &str, available: &str) -> Option<bool> {
    if installed.is_empty() || available.is_empty() || installed == available {
        return Some(false);
    }
    utils::which("vercmp", None)?;
    let report = utils::run(
        &utils::RunSpec {
            argv: vec!["vercmp".into(), installed.to_string(), available.to_string()],
            output: utils::RunOutput::Capture,
            timeout_secs: Some(5),
            ..Default::default()
        },
        None,
    );
    let out = utils::decode_text(&report.stdout);
    let n: i32 = out.trim().parse().ok()?;
    Some(n < 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Panel answer samples are taken from the same place the product takes
    // them.
    use rexon_core::backup;

    /// The contract with the page: every desktop of the database has a
    /// representative package that can be looked up in the repositories. A
    /// forgotten desktop would silently leave its card without the "Available in
    /// your distribution" line.
    #[test]
    fn every_desktop_has_a_package_to_ask_about() {
        for meta in system::DE_META {
            assert!(
                system::get_check_binary(meta.id).is_some(),
                "у стола «{}» нет пакета для запроса версии",
                meta.id
            );
        }
    }

    /// An unknown desktop does not bring the object down and does not pretend
    /// to be a known one.
    #[test]
    fn unknown_desktop_is_reported_not_swallowed() {
        assert!(known_name("no-such-de").is_none());
        assert!(known_name("kde").is_some());
        assert!(repo_version("no-such-de").is_none());
    }

    /// The KDE panel outcome — the core's texts (the same as in the Python tab
    /// window), assembled into ONE footer line: the title, a dash, the text
    /// without a line break.
    ///
    /// Warning: THE INPUT SAMPLES ARE TAKEN FROM THE CORE, NOT TYPED HERE BY
    /// HAND. What arrives here is exactly what `backup::plasma_backup_run` /
    /// `plasma_restore_run` returned, and a copy rewritten by hand silently goes
    /// stale: the line passes through the footer, the test stays green and
    /// describes a product that no longer exists (that is exactly what happened
    /// with the former "Backup created (2 files)" and "No Plasma backups" — the
    /// product has long been saying something else).
    #[test]
    fn panel_result_is_one_footer_line_of_core_texts() {
        // The source language is pinned: the comparison does not depend on the
        // machine's locale, and the Russian text is guarded by the core's
        // comparison (`qt_desktops_parity`).
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let created = backup::msg_backup_created(2);
        let line = panel_status(&qd::panel_backup_result(true, &created));
        assert_eq!(
            line,
            format!("Panel Saved — A copy of your panel settings has been saved. {created}")
        );

        // The restore went through: the core's outcome has ITS OWN line break (the
        // tail about restarting the panel), and the footer must straighten it into
        // a space.
        let restored = format!(
            "{}{}",
            backup::msg_restored_files(3, "20260730_010203"),
            backup::msg_plasmashell_restarted()
        );
        assert!(restored.contains('\n'), "{restored}");
        let line = panel_status(&qd::panel_restore_result(true, &restored));
        assert_eq!(
            line,
            format!(
                "Panel Restored — The panel has been restored from the copy. {}",
                restored.replace('\n', " ")
            )
        );
        assert!(!line.contains('\n'), "{line}");

        // The restore did not work out: the core's reason reaches the footer
        // whole.
        let why = backup::msg_no_plasma_backups();
        let line = panel_status(&qd::panel_restore_result(false, &why));
        assert!(line.starts_with("Couldn't Restore — "), "{line}");
        assert!(line.ends_with(&why), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }
}
