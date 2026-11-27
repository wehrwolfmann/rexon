// FirstRunWizard — the first-run wizard bridge between the core and QML.
//
// WHY. The decision "whether to offer installation", all the wizard's
// labels and the installation itself already exist in the core
// (`rexon_core::installer` and `rexon_core::installer_exec`). The Rust
// window lacked a door to them: `rexon-main-window` did not check whether
// Rexon was installed at all, and a person who unpacked the archive was
// left without a launcher in the menu and without the add-ons for live
// wallpapers, learning nothing about it.
//
// BOUNDARY. There is NOT A SINGLE decision or label here:
//
//   * whether to offer the wizard and in which mode —
//     `installer::first_run_offer` (the live entry `first_run_offer_live`);
//   * all the texts of the welcome page, of the "do not ask for a password"
//     consent, of progress and the result — `installer::welcome_texts`,
//     `passwordless_consent_texts`, `progress_texts`, `result_texts`;
//   * what to copy and where, what to ask for the password with, how to
//     parse the outcome — `installer_exec::install`;
//   * "do not ask again" — `installer::session_first_run_decline`, the same
//     settings key the decision reads.
//
// Here there are only the machine's live facts (the distribution family,
// our own binary, the home folder), the background thread and converting
// the core's maps for QML. The copy-paste guard for labels is the test at
// the end of the file.
//
// Warning: THE PROGRESS WINDOW IS SHARED. The `operation` property and the
// `operationLog` signal have THE SAME shape as in `DesktopsActions` and
// `SettingsActions` (the `OpView` map), so progress and result of the
// installation are shown by the existing `qml/DeOperationDialog.qml`
// without a single edit: `DeOperationDialog { actions: wizard }`.
//
// Warning: INSTALLATION RUNS IN THE BACKGROUND. Behind `pkexec` there is a
// password window that waits as long as it likes, and installing missing
// components takes minutes. A synchronous call would hang the window for
// all that time (that is what the Python wizard did).
//
// Warning: DRY RUN. With `REXON_INSTALL_DRY_RUN=1` a recording executor
// [`DryRunIo`] is substituted: not a single write to disk, not a single
// command started, and the live installation executor (`LiveInstallIo`) is
// not created at all. Needed for accepting the wizard by eye on the work
// machine.

/// cxx-qt bridge of the wizard.
///
/// The type is declared `#[qml_element]`, so the markup creates it for
/// itself (`FirstRunWizard { id: wizard }`).
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
        /// Installation progress window: progress or the result. The same contract
        /// as `DesktopsActions.operation` — one window for everyone
        /// (`DeOperationDialog.qml`).
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        type FirstRunWizard = super::FirstRunWizardRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut FirstRunWizard>);

        /// A progress log line — appended by the window rather than resent as a
        /// whole map.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut FirstRunWizard>, line: QString);

        /// Installation has finished: `ok` — did it work. By it the markup closes
        /// the wizard and releases the tutorial.
        #[qsignal]
        #[cxx_name = "installFinished"]
        fn install_finished(self: Pin<&mut FirstRunWizard>, ok: bool);

        /// The wizard is asked to be shown: the "Install Rexon into the system…"
        /// button on the "Settings" page ([`super::request_show`]).
        #[qsignal]
        #[cxx_name = "showRequested"]
        fn show_requested(self: Pin<&mut FirstRunWizard>);
    }

    extern "RustQt" {
        fn operation(self: &FirstRunWizard) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// The core's decision and ALL the wizard's labels as one map:
        /// `{kind: "offer"|"skip", reason, defaultMode, systemBlocked,
        /// forced, dryRun, dryRunNotice, welcome: {…}, passwordless: {…}}`.
        ///
        /// The settings are only read: the settings directory is not created for
        /// the sake of the answer.
        #[qinvokable]
        fn start(self: &FirstRunWizard) -> QMap_QString_QVariant;

        /// Consent "do not ask for a password for Rexon actions" — a separate check
        /// box on the welcome page. Remembered until [`install`] and effective only
        /// for an installation for all users.
        #[qinvokable]
        #[cxx_name = "setPasswordlessRule"]
        fn set_passwordless_rule(self: Pin<&mut FirstRunWizard>, allow: bool);

        /// Install Rexon in mode `mode` (`"system"`, `"user"`, `"portable"`) in the
        /// background. `false` means an installation is already running or the mode
        /// is unknown. Progress is the `operation` property and the `operationLog`
        /// signal, the result is `installFinished`.
        #[qinvokable]
        fn install(self: Pin<&mut FirstRunWizard>, mode: &QString) -> bool;

        /// "Not now". `remember` — "Don't ask again" is checked: then the refusal
        /// goes into the settings. `false` means writing failed (the reason is in
        /// the log), and the wizard will be offered again.
        #[qinvokable]
        fn decline(self: &FirstRunWizard, remember: bool) -> bool;

        /// Whether an installation is in progress.
        #[qinvokable]
        fn busy(self: &FirstRunWizard) -> bool;

        /// Dry run (`REXON_INSTALL_DRY_RUN=1`): nothing is written or started. The
        /// markup tells the user so — otherwise "done" without an installation
        /// would look like a deception.
        #[qinvokable]
        #[cxx_name = "dryRun"]
        fn dry_run(self: &FirstRunWizard) -> bool;

        /// "Yes" in the question window — start the deferred installation. The
        /// wizard asks its own question with the welcome page, so usually there is
        /// no deferred mode and the method stays silent.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut FirstRunWizard>);

        /// "Close" — remove the progress window (has no effect while the
        /// installation is running).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut FirstRunWizard>);

        /// "Cancel" — only while the system has not been touched.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut FirstRunWizard>);
    }

    impl cxx_qt::Initialize for FirstRunWizard {}
    impl cxx_qt::Threading for FirstRunWizard {}
}

use core::pin::Pin;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};
use serde_json::Value;

use crate::desktops_actions::OpView;
use crate::qvalue::json_to_variant;
use rexon_core::distro_compat::Lang;
use rexon_core::installer::{self as inst, InstallMode};
use rexon_core::installer_exec::{self as iexec, InstallIo, InstallRequest, LiveInstallIo, PayloadSource};
use rexon_core::settings::SettingsManager;
use rexon_core::system::DistroDetector;
use rexon_core::system_de_exec::{CancelToken, DeOpOutcome, DeOpSink};
use rexon_core::system_exec::{LiveSystemIo, RunOut, SystemIo};
use rexon_core::utils::RunOutcome;

type VMap = QMap<QMapPair_QString_QVariant>;

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// The dry run variable: the wizard goes all the way through, but not a
/// single write or start is made.
pub const DRY_RUN_ENV: &str = "REXON_INSTALL_DRY_RUN";

/// Whether dry run is on. Both an empty value and `0` count as off:
/// `REXON_INSTALL_DRY_RUN=0` must mean "no", not "yes".
pub fn dry_run_enabled() -> bool {
    match std::env::var(DRY_RUN_ENV) {
        Ok(v) => !matches!(v.trim(), "" | "0" | "no" | "false"),
        Err(_) => false,
    }
}

/// The core's decision WITHOUT taking the "do not ask again" answer into
/// account — the flag "whether to show the "Install Rexon into the
/// system…" button in Settings" (Wehrwolfmann's decision 15.09.2026). The
/// button itself lifts the refusal, so it must not be hidden because of the
/// refusal; and for an installed Rexon (a package, Steam, an installation
/// for everyone or for oneself) there is no button at all.
///
/// The settings are substituted empty — the only difference from
/// [`inst::first_run_offer_live`].
pub fn offer_ignoring_decline() -> inst::FirstRunOffer {
    inst::first_run_offer(
        &inst::LiveFirstRunProbe::for_this_binary(),
        &serde_json::Map::new(),
    )
}

// ============================================================================
// Registry of live wizards for the button in "Settings"
// ============================================================================

type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

fn registry() -> &'static Mutex<(u64, Vec<(u64, Notify)>)> {
    static REGISTRY: OnceLock<Mutex<(u64, Vec<(u64, Notify)>)>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new((0, Vec::new())))
}

/// Ask the window to show the installation wizard (the button in
/// "Settings").
///
/// `true` means at least one live wizard accepted the request, i.e. the
/// Rust window is open and will show the wizard by itself. `false` means
/// there is no window with a wizard in the process (the tab is open as a
/// standalone window), and the caller is obliged to tell the user so. The
/// same technique as for "Take the tutorial again"
/// (`onboarding_overlay::request_replay`).
pub fn request_show() -> bool {
    let mut reg = registry().lock().expect("the wizard registry is not poisoned");
    let mut told = 0usize;
    reg.1.retain(|(_, notify)| match notify() {
        Ok(()) => {
            told += 1;
            true
        }
        Err(()) => false,
    });
    told > 0
}

// ============================================================================
// Object
// ============================================================================

/// Rust state of the bridge.
#[derive(Default)]
pub struct FirstRunWizardRust {
    op: OpView,
    /// The mode waiting for "Yes" (the welcome page asks the question itself).
    pending: Option<InstallMode>,
    cancel: Option<CancelToken>,
    /// Consent to the "do not ask for a password for Rexon actions" rule.
    passwordless: bool,
    show_watch: u64,
}

impl Drop for FirstRunWizardRust {
    fn drop(&mut self) {
        if self.show_watch != 0 {
            let mut reg = registry().lock().expect("the wizard registry is not poisoned");
            reg.1.retain(|(id, _)| *id != self.show_watch);
        }
    }
}

impl cxx_qt::Initialize for qobject::FirstRunWizard {
    fn initialize(mut self: Pin<&mut Self>) {
        let handle = self.as_ref().get_ref().qt_thread();
        let id = {
            let mut reg = registry().lock().expect("the wizard registry is not poisoned");
            reg.0 += 1;
            let id = reg.0;
            reg.1.push((
                id,
                Box::new(move || match handle.queue(qobject::FirstRunWizard::show_requested) {
                    Ok(()) => Ok(()),
                    Err(ThreadingQueueError::ObjectDestroyed) => Err(()),
                    Err(_) => Ok(()),
                }),
            ));
            id
        };
        let mut rust = self.as_mut().rust_mut();
        rust.passwordless = inst::PASSWORDLESS_CONSENT_DEFAULT;
        rust.show_watch = id;
    }
}

impl qobject::FirstRunWizard {
    // ── Decision and labels ──────────────────────────────────────────────

    fn start(&self) -> VMap {
        let lang = ui_lang();
        let offer = inst::first_run_offer_live();
        let home = std::env::var("HOME").unwrap_or_default();
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
        let distro_id = DistroDetector::detect_live().distro_id();
        let (kind, default_mode, system_blocked, forced) = match &offer {
            inst::FirstRunOffer::Offer {
                default_mode,
                system_blocked,
                forced,
            } => (
                "offer",
                default_mode.as_str(),
                system_blocked.clone().unwrap_or_default(),
                *forced,
            ),
            inst::FirstRunOffer::Skip(_) => ("skip", "", String::new(), false),
        };
        // The labels are composed by the core — here there is only forwarding
        // (rule #12).
        let welcome = inst::welcome_texts(&distro_id, &home, xdg_data.as_deref(), lang);
        let consent = inst::passwordless_consent_texts(lang);
        let dry_run_notice = inst::dry_run_notice(lang);
        let mut out = VMap::default();
        let mut put = |key: &str, value: QVariant| {
            out.insert(QString::from(key), value);
        };
        put("kind", qstr(kind));
        // A line for the log, not for a human: why it was decided so.
        put("reason", qstr(&offer.log_line()));
        put("defaultMode", qstr(default_mode));
        put("systemBlocked", qstr(&system_blocked));
        put("forced", QVariant::from(&forced));
        put("dryRun", QVariant::from(&dry_run_enabled()));
        // The dry run explanation — only when it is on: otherwise the line would
        // sit on the welcome page of every customer.
        put(
            "dryRunNotice",
            qstr(if dry_run_enabled() { &dry_run_notice } else { "" }),
        );
        put("welcome", json_to_variant(&serde_value(&welcome)));
        put("passwordless", json_to_variant(&serde_value(&consent)));
        out
    }

    fn dry_run(&self) -> bool {
        dry_run_enabled()
    }

    fn set_passwordless_rule(mut self: Pin<&mut Self>, allow: bool) {
        self.as_mut().rust_mut().passwordless = allow;
    }

    fn decline(&self, remember: bool) -> bool {
        if !remember {
            return true;
        }
        let mut manager = match SettingsManager::new() {
            Ok(m) => m,
            Err(e) => {
                rexon_core::log_warn!("first-run", "settings did not open: {e}");
                return false;
            }
        };
        match inst::session_first_run_decline(&mut manager, true) {
            Ok(()) => true,
            Err(e) => {
                rexon_core::log_warn!("first-run", "the refusal was not saved: {e}");
                false
            }
        }
    }

    // ── Progress window ──────────────────────────────────────────────────

    fn operation(&self) -> VMap {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn busy(&self) -> bool {
        self.rust().op.phase == "running"
    }

    fn install(self: Pin<&mut Self>, mode: &QString) -> bool {
        let Some(mode) = InstallMode::parse(&mode.to_string()) else {
            rexon_core::log_warn!("first-run", "unknown mode \"{mode}\"");
            return false;
        };
        self.run_install(mode)
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        let Some(mode) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        self.run_install(mode);
    }

    /// Start the installation in the background. `false` means it is already
    /// running.
    fn run_install(mut self: Pin<&mut Self>, mode: InstallMode) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let lang = ui_lang();
        let texts = inst::progress_texts(mode, lang);
        let passwordless = self.as_ref().get_ref().rust().passwordless;
        let token = CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        self.as_mut().rust_mut().pending = None;
        let mut op = OpView::running(&texts.title);
        // The core's subtitle already mentions the password window — it stays as
        // the status line until the executor names its stage.
        op.status = texts.subtitle;
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let sink: Arc<dyn DeOpSink> = Arc::new(QtSink {
                handle: handle.clone(),
            });
            let result = run_install_with_io(mode, passwordless, lang, sink, &token);
            deliver(&handle, move |object| object.finish_install(result));
        });
        true
    }

    /// Installation result: the progress window turns into a result window,
    /// the labels come from the core (`installer::result_texts`).
    fn finish_install(mut self: Pin<&mut Self>, result: iexec::InstallResult) {
        let lang = ui_lang();
        let ok = result.ok();
        let outcome = DeOpOutcome {
            ok,
            message: result.message.clone(),
            cancelled: result.cancelled(),
            restart_needed: false,
        };
        let texts = result.outcome.as_ref().map(|o| inst::result_texts(o, lang));
        let summary = texts
            .as_ref()
            .map(|t| {
                let mut body = format!("<b>{}</b><br>{}", html(&t.title), html(&t.description));
                for note in &t.notes {
                    body.push_str("<br>");
                    body.push_str(&html(note));
                }
                body
            })
            .unwrap_or_default();
        let mut op = self.as_ref().get_ref().rust().op.clone();
        if let Some(line) = op.apply_done(&outcome, summary.clone()) {
            self.as_mut().operation_log(QString::from(&line));
        }
        // Failure: the core's result must be visible too, not only in the log.
        if !ok {
            op.result_text = summary;
        }
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().set_op(op);
        self.as_mut().install_finished(ok);
    }

    fn apply_progress(mut self: Pin<&mut Self>, stage: String, message: String, fraction: Option<f64>) {
        let mut op = self.as_ref().get_ref().rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
        }
        // There are no installation stages in `de_progress::STAGE_TITLES`, and the
        // core gives no title for them: the status line is the executor's last
        // message, otherwise the window would show "Preparing..." for the whole
        // installation. The shared stages ("error", "warning") have their own
        // title — we leave them alone.
        if !message.is_empty() && matches!(stage.as_str(), iexec::STAGE_PREPARE | iexec::STAGE_INSTALL) {
            op.status = message;
        }
        self.set_op(op);
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
        if self.as_ref().get_ref().busy() {
            return;
        }
        self.as_mut().rust_mut().pending = None;
        self.set_op(OpView::default());
    }

    fn cancel_operation(self: Pin<&mut Self>) {
        if !self.as_ref().get_ref().busy() || !self.as_ref().get_ref().rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.as_ref().get_ref().rust().cancel {
            token.cancel();
        }
        // The button goes off at once: a second click adds nothing.
        let mut op = self.as_ref().get_ref().rust().op.clone();
        op.cancellable = false;
        self.set_op(op);
    }
}

/// The language of the wizard's labels is the INTERFACE language
/// (`rexon_core::i18n`: the user's setting ▸ the locale ▸ English), not the
/// bare process locale.
///
/// The wizard's texts are composed by the core installer in
/// "Russian/English" pairs (`installer.rs`, `Lang`); previously the pair was
/// chosen by `Lang::from_env`, and with a language chosen in the settings
/// the wizard would speak a different language from the rest of the window.
fn ui_lang() -> Lang {
    Lang::from_code(&rexon_core::i18n::current_language())
}

/// Plain HTML for the window: line breaks are `<br>` (in `RichText` a bare
/// newline would become a space). The same technique as for the desktop
/// operation window.
fn html(text: &str) -> String {
    text.replace('\n', "<br>")
}

/// Core structure → JSON. A serialization failure is unreachable (all
/// fields are strings, numbers and lists), but it must not be kept quiet.
fn serde_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|e| {
        rexon_core::log_warn!("first-run", "labels not converted to JSON: {e}");
        Value::Null
    })
}

// ============================================================================
// Executing the installation
// ============================================================================

/// What to install on this machine. Decisions and execution are in the
/// core; here there are only live facts: the distribution family, our own
/// binary, the developer tree and the home folder.
fn live_request<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    passwordless: bool,
) -> InstallRequest {
    let own_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let home = io.env("HOME");
    let xdg_data = io.env("XDG_DATA_HOME");
    let payload = PayloadSource::for_running(
        io,
        &own_exe,
        &rexon_core::cli::repo_root(),
        &home,
        Some(xdg_data.as_str()).filter(|x| !x.is_empty()),
    );
    let detector = DistroDetector::detect_live();
    let mut req = InstallRequest::new(payload, detector.family_live(), &detector.distro_id(), lang);
    req.entry_env = std::env::var(inst::ENTRY_ENV).ok();
    req.passwordless_rule = passwordless;
    req
}

/// Installation with the chosen executor. The live executor is NOT created
/// in a dry run: `LiveInstallIo` stands only in the second branch.
fn run_install_with_io(
    mode: InstallMode,
    passwordless: bool,
    lang: Lang,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> iexec::InstallResult {
    if dry_run_enabled() {
        let io = DryRunIo::new();
        let req = live_request(&io, lang, passwordless);
        let result = iexec::install(&io, mode, &req, sink, cancel);
        for line in io.journal() {
            rexon_core::log_info!("first-run", "installation dry run: {line}");
        }
        return result;
    }
    let io = LiveInstallIo::new();
    let req = live_request(&io, lang, passwordless);
    iexec::install(&io, mode, &req, sink, cancel)
}

/// Receiver of the core executor's progress: every event goes to the Qt
/// thread.
struct QtSink {
    handle: CxxQtThread<qobject::FirstRunWizard>,
}

impl DeOpSink for QtSink {
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

/// Queue work in the Qt thread; if the window is already closed —
/// silently, otherwise to the log.
fn deliver(
    handle: &CxxQtThread<qobject::FirstRunWizard>,
    work: impl FnOnce(Pin<&mut qobject::FirstRunWizard>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("first-run", "installation progress not delivered to the window: {e:?}");
        }
    }
}

// ============================================================================
// Dry run
// ============================================================================

/// A recording executor: observations are taken from the live machine
/// (`LiveSystemIo` — read-only), while everything that changes the machine
/// is written to the log and not done.
///
/// Warning: THE LIVE INSTALLATION EXECUTOR (`LiveInstallIo`) TAKES NO PART
/// HERE: it has not a single method that could not be called by mistake.
/// The observations are taken from a separate reading executor of the core,
/// so this type cannot write anything in principle.
///
/// What the run created is remembered in memory: without this the core
/// executor would not find the just "written" file on the next step and
/// would fail for no reason. For the same reason, after "running" the root
/// script the paths of an installation for all users count as laid out —
/// otherwise a dry run would always hit the "Rexon is in place" check and
/// there would be no way to show the success result by eye.
struct DryRunIo {
    live: LiveSystemIo,
    journal: RefCell<Vec<String>>,
    files: RefCell<HashSet<String>>,
    dirs: RefCell<HashSet<String>>,
    executable: RefCell<HashSet<String>>,
    /// Roots that the root script "laid out".
    pretend: RefCell<Vec<String>>,
}

impl DryRunIo {
    fn new() -> Self {
        DryRunIo {
            live: LiveSystemIo,
            journal: RefCell::new(Vec::new()),
            files: RefCell::new(HashSet::new()),
            dirs: RefCell::new(HashSet::new()),
            executable: RefCell::new(HashSet::new()),
            pretend: RefCell::new(Vec::new()),
        }
    }

    /// What the run would have done to the machine — one line per action.
    fn journal(&self) -> Vec<String> {
        self.journal.borrow().clone()
    }

    fn note(&self, line: String) {
        self.journal.borrow_mut().push(line);
    }

    fn add_file(&self, path: &str, mode: u32) {
        self.files.borrow_mut().insert(path.to_string());
        if mode & 0o111 != 0 {
            self.executable.borrow_mut().insert(path.to_string());
        }
    }

    fn under_pretend(&self, path: &str) -> bool {
        self.pretend
            .borrow()
            .iter()
            .any(|root| path == root || path.starts_with(&format!("{root}/")))
    }
}

impl SystemIo for DryRunIo {
    fn exists(&self, path: &str) -> bool {
        self.files.borrow().contains(path)
            || self.dirs.borrow().contains(path)
            || self.under_pretend(path)
            || self.live.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.files.borrow().contains(path) || self.under_pretend(path) || self.live.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.dirs.borrow().contains(path) || self.under_pretend(path) || self.live.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.executable.borrow().contains(path)
            || self.under_pretend(path)
            || self.live.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.live.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.live.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.live.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.live.which(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.live.which_path(name)
    }
    fn env(&self, name: &str) -> String {
        self.live.env(name)
    }

    // ── Side actions: only the log ───────────────────────────────────────

    fn run(&self, argv: &[String], _timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
        self.note(format!("would run: {}", argv.join(" ")));
        // The root script "laid out" the files of an installation for everyone:
        // then the core checks that Rexon is in place.
        if argv.first().is_some_and(|p| p.ends_with("pkexec")) {
            let sys = inst::install_paths(InstallMode::System, "", None);
            let mut pretend = self.pretend.borrow_mut();
            for dir in [sys.install_dir, sys.lib_dir, sys.desktop_dir, sys.icon_root] {
                pretend.extend(dir);
            }
        }
        RunOut {
            outcome: RunOutcome::Ok,
            returncode: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.note(format!("would write {path} ({} characters)", content.chars().count()));
        self.add_file(path, 0o644);
        Ok(())
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.note(format!("would copy {src} → {dst}"));
        self.add_file(dst, 0o644);
        Ok(())
    }
}

impl InstallIo for DryRunIo {
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        let base = parent.unwrap_or("/tmp").trim_end_matches('/').to_string();
        let path = format!("{base}/{prefix}dry-run");
        self.note(format!("would create the temporary folder {path}"));
        self.dirs.borrow_mut().insert(path.clone());
        Ok(path)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
        self.note(format!("would rename {src} → {dst}"));
        self.dirs.borrow_mut().insert(dst.to_string());
        Ok(())
    }

    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
        self.note(format!("would create the folder {path} (mode {:o})", mode.unwrap_or(0o755)));
        self.dirs.borrow_mut().insert(path.to_string());
        Ok(())
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        self.note(format!(
            "would write {path} (mode {mode:o}, {} characters)",
            content.chars().count()
        ));
        self.add_file(path, mode);
        Ok(())
    }

    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
        self.note(format!("would copy {src} → {dst} (mode {mode:o})"));
        self.add_file(dst, mode);
        Ok(())
    }

    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        _skip: &dyn Fn(&str) -> bool,
        _mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String> {
        self.note(format!("would copy the tree {src} → {dst}"));
        self.dirs.borrow_mut().insert(dst.to_string());
        Ok(0)
    }

    fn remove_file(&self, path: &str) {
        self.note(format!("would remove the file {path}"));
        self.files.borrow_mut().remove(path);
        self.executable.borrow_mut().remove(path);
    }

    fn remove_tree(&self, path: &str) {
        self.note(format!("would remove the folder {path}"));
        self.dirs.borrow_mut().remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source text of the file without its own test module: the list of
    /// forbidden strings lies in the tests, and without the cut-off the guard
    /// would catch itself.
    fn code() -> &'static str {
        include_str!("first_run_wizard.rs")
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("the test module is in place")
    }

    /// Not a single wizard label is typed anew in this file: all of them come
    /// from the core. A copy-paste guard — a second copy of a human-readable
    /// string would silently drift away from the product (rule #12). The model
    /// is `window_menu.rs::no_human_text_is_typed_twice_in_this_file`.
    #[test]
    fn no_human_text_is_typed_twice_in_this_file() {
        let src = code();
        for baked in [
            "Welcome to Rexon",
            "Install Rexon",
            "For everyone on this computer",
            "Just for you",
            "Standalone folder",
            "Don't ask again",
            "\"Skip\"",
            "\"Retry\"",
            "Installing Rexon",
            "Please wait",
            "administrator password",
            "Don't ask for the password",
        ] {
            let hits: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(baked))
                .filter(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with("///"))
                .collect();
            assert!(
                hits.is_empty(),
                "human text \"{baked}\" is typed here instead of taken from the core: {hits:?}"
            );
        }
    }

    /// The bridge does not decide by itself: the settings key, the modes, the
    /// "whether to offer" flag and parsing the outcome — only through core
    /// calls.
    #[test]
    fn decisions_are_asked_of_the_core() {
        let src = code();
        assert!(src.contains("inst::first_run_offer_live()"), "the decision is not taken from the core");
        assert!(src.contains("inst::session_first_run_decline"), "the refusal bypasses the core");
        assert!(src.contains("iexec::install(&io"), "the installation bypasses the core executor");
        assert!(src.contains("inst::result_texts"), "the outcome is not built by the core");
        for own in ["installer_first_run_declined", "\"pkexec\".to_string()"] {
            assert!(!src.contains(own), "the bridge has its own copy of \"{own}\"");
        }
    }

    /// Dry run: a fake executor, not a single write or start. Checked on a real
    /// path in a fake directory — the file does not appear on disk after the
    /// "write".
    #[test]
    fn dry_run_substitutes_a_recording_io() {
        let scratch = std::env::temp_dir().join("rexon-first-run-wizard-dry-run");
        let _ = std::fs::remove_dir_all(&scratch);
        let path = scratch.join("launcher");
        let path = path.to_string_lossy().into_owned();

        let io = DryRunIo::new();
        io.make_dirs(&scratch.to_string_lossy(), Some(0o755)).unwrap();
        io.write_file_mode(&path, "#!/bin/sh\n", 0o755).unwrap();
        let out = io.run(&["/usr/bin/pkexec".into(), "bash".into(), "/tmp/x".into()], None, false);

        // Neither the folder nor the file exists on disk — everything stayed in
        // the log.
        assert!(!scratch.exists(), "the dry run created a real folder {scratch:?}");
        assert_eq!(out.returncode, Some(0), "the run must go on without starting the command");
        let journal = io.journal();
        assert_eq!(journal.len(), 3, "not all actions are in the log: {journal:?}");
        assert!(journal[2].starts_with("would run:"), "{journal:?}");

        // What was written is visible to the core executor: otherwise the next
        // step would fail.
        assert!(SystemIo::is_file(&io, &path));
        assert!(SystemIo::is_executable(&io, &path));
        // After the "root script" the installation for everyone counts as in
        // place.
        let sys = inst::install_paths(InstallMode::System, "", None);
        assert!(SystemIo::exists(&io, &sys.install_dir.unwrap()));
    }

    /// The dry run variable is read honestly: `0` means "no".
    #[test]
    fn dry_run_flag_is_read_strictly() {
        // The variable's value is taken from the process, so we check the parsing
        // on the same table as `dry_run_enabled`.
        for (value, want) in [("1", true), ("yes", true), ("", false), ("0", false), ("no", false)] {
            let on = !matches!(value.trim(), "" | "0" | "no" | "false");
            assert_eq!(on, want, "value \"{value}\"");
        }
    }

    /// The wizard's progress window is the same as in "Desktops": the keys of
    /// the `operation` map are read by the shared `DeOperationDialog.qml`.
    #[test]
    fn operation_map_matches_the_shared_dialog() {
        let op = OpView::running("check");
        let map = op.to_map();
        for key in [
            "phase", "title", "text", "informative", "status", "bar", "barMax", "current",
            "closeText", "closeEnabled", "cancellable", "resultText", "danger", "yesText",
            "noText", "okText", "cancelText",
        ] {
            assert!(map.contains(&QString::from(key)), "the window map has no key \"{key}\"");
        }
    }
}
