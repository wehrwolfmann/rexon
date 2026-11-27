//! Executor of desktop operations: a port of the ACTING half of `install_de`
//! from `rexon/system.py` together with what it calls:
//! `PackageManager.install_batch`, `ensure_display_manager`, the MATE
//! post-install, enabling the Gentoo overlay and the default application
//! protection (`SystemDefaultsProtector`).
//!
//! WHY. The decisions of these operations moved into the core long ago:
//! plans and script texts in [`crate::system_plan`], distribution
//! compatibility in [`crate::distro_compat`], progress display in
//! [`crate::de_progress`], defaults protection in [`crate::defaults`]. Only
//! Python executed them, so the Rust window could only send the person to
//! the classic window on the «Install», «Check / Repair» and «Check my
//! desktop» buttons.
//!
//! The split is the same as in the neighbour [`crate::system_exec`]: **the
//! core decides, the executor does**.
//!   * WHAT to do: the core's plans and texts; there is not a single package
//!     manager command of our own here, nor a single path of our own to a
//!     privileged helper;
//!   * WITH WHAT: the [`DeOpsIo`] trait (an extension of [`SystemIo`]): in
//!     the product [`LiveDeOpsIo`], in parity checks a fake one, and the live
//!     machine is not touched at all;
//!   * the ORDER of steps and progress: [`DeOpsExec`], events go to [`DeOpSink`].
//!
//! ⛔ FOOLPROOF GUARDS that must work here too:
//!   * a distribution refusal is explained BEFORE the first side effect: no
//!     defaults snapshot, no panel copy, no password prompt;
//!   * there must be a login screen after the install: if none is enabled,
//!     SDDM is installed and enabled AUTOMATICALLY, without asking (decision:
//!     [`plan::ensure_dm_plan`], called for ANY desktop, as in the original).
//!
//! ⭐ CANCELLATION. Python had none at all: the progress window button was
//! disabled until the end. Here one can cancel, but ONLY while the system is
//! untouched: before the first package manager run and while waiting for
//! someone else's lock (that lasts up to five minutes, and sitting in front
//! of a window with no way out is exactly the trap rule #13 forbids). As soon
//! as the transaction starts, cancellation is disabled: aborting `pacman` in
//! the middle of an install means breaking the system with our own hands.
//!
//! ⭐ WHAT WAS FIXED IN THE PORT (rule #16):
//!   1. Progress lines the original wrote to the person in jargon ("Saving
//!      current defaults", "DE MIME associations", "ensure_display_manager:
//!      installing sddm code 1") are said here in plain language.
//!   2. The AUR build tail of the MATE post-install no longer moves the
//!      progress bar back: the original sent it as the `aur` stage with a 75 %
//!      share in the middle of a stage standing at 85 %.
//!   3. The install plan is checked for unknown steps BEFORE the first of
//!      them, not in the middle of execution: an unknown step found after the
//!      script was written would leave garbage in the temporary directory.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;

use crate::aeroshell;
use crate::backup;
use crate::compliance::{self as comp, ComplianceIo, ScanEntry};
use crate::de_progress::{self, InstallVerdict};
use crate::defaults as dfl;
use crate::distro_compat as dc;
use crate::system::{self, DistroDetector, PkgFamily};
use crate::privilege::op::{PkgSource, PrivOp};
use crate::system_exec::{RunOut, SystemExec, SystemIo};
use crate::system_plan::{self as plan, LockWaitStep, LogTailKind, PlanStep, RunStepId};
use crate::i18n::{ntr, tr, trf};
use crate::utils::{self, RunOutcome};

/// The "compositor + shell" bundle: install, login and removal.
mod shells;
pub use shells::{shell_install_live, shell_remove_live, shell_switch_live};

// ============================================================================
// Progress texts
// ============================================================================

/// A desktop that is not in the core's database.
pub fn unknown_de_message(de_id: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment id.
    trf("Rexon doesn't know the “%1” desktop.", &[&de_id])
}

/// Result of a cancelled operation. Cancellation is possible only while the
/// system is untouched, so this is a promise, not a hope.
pub fn cancelled_message() -> String {
    tr("The operation was cancelled. The system wasn't changed.")
}
/// Progress window title after cancellation: the progress stage key (also
/// written by `installer_exec` into `progress`), not a caption: the caption
/// is [`status_cancelled`].
pub const STATUS_CANCELLED: &str = "Отменено";
/// Progress window title after cancellation, in the UI language.
pub fn status_cancelled() -> String {
    // TRANSLATORS: status line of the operation window after cancelling.
    tr("Cancelled")
}
/// A second operation while the first one is running.
pub fn busy_message() -> String {
    tr("A desktop operation is already running — wait until it finishes.")
}

/// Operation window button captions. «Yes» and «No» as in the classic
/// window's question; the safe answer («No») has focus on the first frame.
pub fn dialog_yes() -> String {
    // TRANSLATORS: button in a question dialog.
    tr("Yes")
}
pub fn dialog_no() -> String {
    // TRANSLATORS: button in a question dialog.
    tr("No")
}
/// The cancel button: visible only while the system is untouched.
pub fn dialog_cancel() -> String {
    // TRANSLATORS: button that cancels a running desktop operation.
    tr("Cancel")
}

/// Footer line while the window waits for the person's answer.
pub fn awaiting_answer(title: &str) -> String {
    // TRANSLATORS: %1 is the title of a question window.
    trf("“%1” — answer in the window that opened.", &[&title])
}

/// Footer line with the operation result.
pub fn operation_footer(title: &str, status: &str) -> String {
    format!("{title} — {status}")
}

/// What kind of main window footer line this is: this decides how long it
/// lives.
///
/// WHY. The footer used to keep the last message forever: «Wayfire repair:
/// Done ✅» hung for hours while the person had long been working in
/// «Appearance» (screenshot of 16.11.2026), and read as something happening
/// now. The lifetime decision is made here; the window only executes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FooterKind {
    /// Waiting for the person or for work: a question or refusal is open, a
    /// check is running. No timer: the line is removed by the operation window
    /// itself (closing the question or refusal) or by the next result.
    #[default]
    Pending,
    /// A success result, a cancellation, a notice ("an operation is already
    /// running"): visible for [`FOOTER_NOTICE_MS`] and gone when switching to
    /// another section.
    Notice,
    /// An error: stays until the person clicks the footer (or a new message
    /// arrives). Switching sections does not remove it: a failure is easy to
    /// miss if it disappears by itself.
    Error,
}

/// How long a footer notice is visible, ms.
pub const FOOTER_NOTICE_MS: u32 = 12_000;

/// How the window treats a footer line of a given kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FooterHold {
    /// After how many milliseconds the line is removed by itself; 0: it is not removed.
    pub timeout_ms: u32,
    /// Whether it is removed when switching to another window section.
    pub clear_on_section_change: bool,
    /// Whether it is removed by clicking the footer.
    pub click_dismisses: bool,
}

pub fn footer_hold(kind: FooterKind) -> FooterHold {
    match kind {
        FooterKind::Pending => FooterHold {
            timeout_ms: 0,
            clear_on_section_change: false,
            click_dismisses: false,
        },
        FooterKind::Notice => FooterHold {
            timeout_ms: FOOTER_NOTICE_MS,
            clear_on_section_change: true,
            click_dismisses: true,
        },
        FooterKind::Error => FooterHold {
            timeout_ms: 0,
            clear_on_section_change: false,
            click_dismisses: true,
        },
    }
}

/// Kind of the result line: a failure is an error, success and cancellation
/// (the system is untouched) are a notice.
pub fn outcome_footer_kind(outcome: &DeOpOutcome) -> FooterKind {
    if outcome.ok || outcome.cancelled {
        FooterKind::Notice
    } else {
        FooterKind::Error
    }
}

/// Kind of the line by the success flag (saving and restoring the panel, etc.).
pub fn result_footer_kind(ok: bool) -> FooterKind {
    if ok {
        FooterKind::Notice
    } else {
        FooterKind::Error
    }
}

pub fn snapshot_message() -> String {
    tr("🔒 Remembering your default apps")
}
pub fn panel_backup_message() -> String {
    tr("🛡️ Saving copies of the panels of installed desktops")
}
pub fn post_install_message() -> String {
    tr("🔧 Post-install setup")
}
pub fn dm_step_message() -> String {
    tr("⚙️ Login screen")
}
pub fn restore_message() -> String {
    tr("🔒 Restoring your default apps")
}
pub fn mime_message() -> String {
    tr("🛡️ Setting up which apps open files in each desktop")
}

fn snapshot_failed(e: &str) -> String {
    trf("Couldn't remember the default apps: %1", &[&e])
}

fn panel_backup_failed(de: &str, e: &str) -> String {
    // TRANSLATORS: %1 is a desktop name, %2 what went wrong.
    trf("The panel copy of %1 wasn't saved: %2", &[&de, &e])
}

fn install_intro(main: usize, aur: usize) -> String {
    let head = ntr("📦 Installing: %n package", "📦 Installing: %n packages", main as u64);
    if aur > 0 {
        // TRANSLATORS: %1 is the sentence about packages, %2 how many more come from the AUR.
        trf("%1 + %2 from the AUR", &[&head, &aur])
    } else {
        head
    }
}

/// Intro of «Check / Repair»: the desktop is already installed, the package
/// manager will only verify the set and add what is missing, so «Install»
/// would be untrue here.
fn check_intro(main: usize, aur: usize) -> String {
    let head = ntr("🔍 Checking: %n package", "🔍 Checking: %n packages", main as u64);
    if aur > 0 {
        trf("%1 + %2 from the AUR", &[&head, &aur])
    } else {
        head
    }
}

/// Result of the main step when pacman answered "there is nothing to do".
fn main_already_installed() -> String {
    tr("✅ The main packages are all in place — nothing to install")
}

/// Result of «Check / Repair»: nothing was needed.
fn checked_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 checked — everything is in place", &[&display])
}

/// Result of «Check / Repair»: what was missing has been added.
fn repaired_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 checked — the missing parts are installed", &[&display])
}

/// Result of «Check / Repair»: not everything could be added (why: in the
/// lines above).
fn repair_incomplete_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("⚠️ %1 checked, but not everything could be installed — see above", &[&display])
}

/// Default applications for the desktop are written (details in the Rexon log).
fn defaults_set_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ Default apps for %1 are set", &[&display])
}

/// All protected defaults are in place: nothing to restore.
fn defaults_kept_message() -> String {
    tr("✅ Your default apps are in place")
}

/// A protected default that the install overwrote has been restored.
fn default_restored_line(label: &str) -> String {
    // TRANSLATORS: %1 is an app kind with its icon, e.g. “🗂️ File Manager”.
    trf("%1: your choice is back", &[&label])
}

/// Our own "open with" lists are written for the desktops `names`.
fn mimeapps_written_message(names: &str) -> String {
    // TRANSLATORS: %1 is a comma-separated list of desktop environment names.
    trf("✅ File associations saved for: %1", &[&names])
}

/// Default application log lines → what to show the person in the window:
/// only warnings and errors, without process numbers. Everything else (D-Bus
/// service names, file paths, "already so") is for the Rexon log.
pub fn window_defaults_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("⚠️") || l.starts_with("❌"))
        .map(|l| format!("  {}", strip_pids(l)))
        .collect()
}

/// Remove "(pid 4613)" and "pid 4613, " from the line: a process number
/// tells the person nothing (the Rexon log keeps the line in full).
fn strip_pids(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(i) = rest.find("pid ") {
        let digits = rest[i + 4..].chars().take_while(char::is_ascii_digit).count();
        let word_start = i == 0 || !rest[..i].chars().last().is_some_and(char::is_alphanumeric);
        if digits == 0 || !word_start {
            out.push_str(&rest[..i + 4]);
            rest = &rest[i + 4..];
            continue;
        }
        let end = i + 4 + digits;
        let before = &rest[..i];
        let after = &rest[end..];
        if before.ends_with('(') && after.starts_with(')') {
            // " (pid 1)" in full.
            out.push_str(before[..before.len() - 1].trim_end());
            rest = &after[1..];
        } else if after.starts_with(", ") {
            // «(pid 1, service mode)» → «(service mode)».
            out.push_str(before);
            rest = &after[2..];
        } else {
            out.push_str(before.trim_end());
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// pacman installed nothing: the whole set was already installed (`--needed`).
pub fn pacman_nothing_to_do(log: &str) -> bool {
    log.lines().any(|l| l.trim().trim_start_matches("::").trim().eq_ignore_ascii_case("there is nothing to do"))
}

fn post_install_failed(msg: &str) -> String {
    trf("Post-install setup didn't fully finish: %1", &[&msg])
}

fn dm_already(manager: &str) -> String {
    trf("⚙️ The login screen is already set up (%1) — leaving it", &[&manager])
}

fn dm_enabled(unit: &str) -> String {
    trf("✅ Login screen %1 enabled", &[&unit])
}

fn dm_enable_failed(unit: &str, why: &str) -> String {
    trf("Couldn't enable login screen %1: %2", &[&unit, &why])
}

fn switcher_message(display: &str) -> String {
    trf("🔄 Default apps for %1", &[&display])
}

fn restore_failed(e: &str) -> String {
    trf("Couldn't restore the default apps: %1", &[&e])
}

/// Start of «Add variant …»: without listing the desktop's whole set.
fn session_adding_message(kind: system::SessionKind, display: &str) -> String {
    match kind {
        // TRANSLATORS: %1 is a desktop environment name.
        system::SessionKind::Wayland => trf("📦 Adding the Wayland version of %1…", &[&display]),
        // TRANSLATORS: %1 is a desktop environment name.
        system::SessionKind::X11 => trf("📦 Adding the X11 version of %1…", &[&display]),
    }
}

/// Result of «Add variant …» with the session as the login screen writes it.
fn session_added_message(kind: system::SessionKind, display: &str, session: &str) -> String {
    match kind {
        system::SessionKind::Wayland => trf(
            // TRANSLATORS: %1 is a desktop environment name, %2 is the session name on the login screen.
            "✅ The Wayland version of %1 is added — choose “%2” on the login screen",
            &[&display, &session],
        ),
        system::SessionKind::X11 => trf(
            // TRANSLATORS: %1 is a desktop environment name, %2 is the session name on the login screen.
            "✅ The X11 version of %1 is added — choose “%2” on the login screen",
            &[&display, &session],
        ),
    }
}

fn installed_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 installed", &[&display])
}

fn temp_failed(e: &str) -> String {
    trf("Couldn't create a temporary file for the installation: %1", &[&e])
}

fn write_failed(path: &str, e: &str) -> String {
    trf("Couldn't write the temporary file %1: %2", &[&path, &e])
}

fn unknown_step_message(what: &str) -> String {
    trf("The installation plan has an unknown step (%1) — the installation wasn't started.", &[&what])
}

fn gentoo_overlay_intro(overlay: &str) -> String {
    trf("📦 Gentoo: adding the “%1” overlay…", &[&overlay])
}

fn gentoo_overlay_done(overlay: &str) -> String {
    trf("✅ Overlay “%1” is ready", &[&overlay])
}

fn gentoo_overlay_failed(overlay: &str) -> String {
    trf("Couldn't add the Gentoo overlay “%1”", &[&overlay])
}

fn ensure_dm_install_failed(package: &str, why: &str) -> String {
    trf("Couldn't install the login screen (%1): %2", &[&package, &why])
}

fn ensure_dm_enable_failed(why: &str) -> String {
    trf("Couldn't enable the login screen: %1", &[&why])
}

/// Exit code or "did not answer", for progress lines.
fn run_why(out: &RunOut) -> String {
    match out.outcome {
        RunOutcome::Timeout => tr("the command ran out of time"),
        RunOutcome::Error => tr("the command didn't start"),
        _ => {
            let text = if !out.stderr.trim().is_empty() {
                &out.stderr
            } else {
                &out.stdout
            };
            match text.trim().lines().last() {
                Some(line) if !line.trim().is_empty() => line.trim().to_string(),
                _ => trf("code %1", &[&out.returncode.unwrap_or(-1)]),
            }
        }
    }
}

// ============================================================================
// Executor trait
// ============================================================================

/// Clock readings for file names and timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    /// `datetime.now().isoformat()`.
    pub iso: String,
    /// `datetime.now().isoformat(timespec="seconds")`.
    pub iso_seconds: String,
    /// Epoch milliseconds, for bridge events.
    pub epoch_ms: u64,
}

/// BonziBuddy bridge event about a desktop operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeEvent {
    DeInstalled(String),
    DeSwitch { from: String, to: String },
    DeRemoved(String),
}

fn removing_message(display: &str) -> String {
    trf("🗑️ Removing %1", &[&display])
}

/// Everything the desktop being removed has is needed by the desktop the
/// person is sitting in, or by its login screen: nothing to remove.
fn remove_all_shared_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf(
        "Nothing to remove: everything %1 consists of is also used by your current desktop or its login screen.",
        &[&display],
    )
}

fn removed_message(display: &str) -> String {
    // TRANSLATORS: %1 is a desktop environment name.
    trf("✅ %1 removed", &[&display])
}

/// The removal did not fit into the time limit: not the same as "failed".
pub fn remove_timeout_message() -> String {
    tr("The removal ran out of time. Check whether another program is using the package manager.")
}

pub fn logout_message() -> String {
    tr("👋 Logging out…")
}
pub fn logout_failed_message() -> String {
    tr("Couldn't log out")
}

fn config_backup_message(display: &str) -> String {
    trf("💾 Saving a copy of the %1 settings", &[&display])
}

fn config_backup_done(name: &str, count: usize) -> String {
    // TRANSLATORS: %1 is the backup name, %n the number of files in it.
    crate::i18n::fill(&ntr("✅ Copy saved: %1 (%n file)", "✅ Copy saved: %1 (%n files)", count as u64), &[&name])
}

fn config_backup_failed(e: &str) -> String {
    trf("The settings copy wasn't saved: %1", &[&e])
}

fn logout_timeout(cmd: &str) -> String {
    // TRANSLATORS: %1 is a command name.
    trf("%1: didn't respond in time", &[&cmd])
}

fn logout_not_started(cmd: &str) -> String {
    // TRANSLATORS: %1 is a command name.
    trf("%1: didn't start", &[&cmd])
}

/// Side effects of desktop operations ON TOP of [`SystemIo`]. Not a single
/// decision: each method is one trip outside or one ready executor of the core.
pub trait DeOpsIo: SystemIo {
    /// Run with output (stdout and stderr together) into a file: for AUR builds,
    /// whose progress is read from that file on the fly.
    fn run_to_file(&self, argv: &[String], timeout_secs: Option<u64>, log_path: &str) -> RunOut;
    /// `tempfile.mkstemp(prefix, suffix)` → path.
    fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String>;
    /// `tempfile.mkdtemp(prefix, dir=parent)` → path.
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String>;
    /// Write a whole file and set permissions.
    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn make_dirs(&self, path: &str) -> Result<(), String>;
    /// Delete a file; errors do not matter.
    fn remove_file(&self, path: &str);
    /// `shutil.rmtree(path, ignore_errors=True)`.
    fn remove_tree(&self, path: &str);
    /// Monotonic seconds, for waiting on the lock.
    fn elapsed_secs(&self) -> f64;
    /// Sleep.
    fn sleep_secs(&self, secs: u64);
    /// Clock.
    fn clock(&self) -> Clock;
    /// Read a growing log, emitting progress lines. `None`: nothing to read with
    /// (a fake executor).
    fn tail_log(
        &self,
        path: &str,
        kind: LogTailKind,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Option<plan::LogTail>;
    /// Panel copies of every installed desktop that has one (Wehrwolfmann's
    /// decision of 25.11.2026: installing or switching desktops most often
    /// breaks ANOTHER desktop's panel, through branches they share) ->
    /// `(desktop key, outcome)` per desktop.
    fn panel_snapshots(&self, reason: &str) -> Vec<(String, Result<(), String>)>;
    /// Default applications for the desktop (`DEAppSwitcher.on_switch_de`).
    fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>);
    /// Copy of the desktop configs (`BackupManager.create_backup`) → the copy
    /// name and how many paths went into it.
    fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String>;
    /// BonziBuddy bridge event. The bridge is auxiliary: a failure does not matter.
    fn bridge_notify(&self, event: &BridgeEvent);
    /// Package manager lock files to wait for before running it.
    ///
    /// The path is a fact about the machine, not about the order of actions, so
    /// it comes from the executor: a test bench on fake `pkexec`/`pacman` waits
    /// for its own lock in a temporary directory, not for the real
    /// `/var/lib/pacman/db.lck` held by the owner's live `pacman -Syu` (up to
    /// 300 s of waiting).
    fn pkg_lock_paths(&self, family: PkgFamily) -> Vec<String> {
        plan::lock_paths(family).iter().map(|p| p.to_string()).collect()
    }
    /// Carry the variables from `~/.config/environment.d` to sessions that do not
    /// read that directory (`compositor_autostart::sync_session_env`). A fake
    /// executor does not touch the live profile: by default it does nothing; the
    /// live one calls the real sync.
    fn sync_session_env(&self) {}
}

/// Where the executor sends progress. Called from the worker thread AND from
/// the log reader thread, hence `Send + Sync`.
pub trait DeOpSink: Send + Sync {
    /// Progress message: stage, text, share (`None`: do not move the bar).
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>);
    /// Whether the operation can be cancelled now.
    fn cancellable(&self, allowed: bool);
}

/// Cancellation flag shared by the window and the worker thread.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// How the operation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeOpOutcome {
    pub ok: bool,
    pub message: String,
    /// Cancelled by the person while the system was untouched.
    pub cancelled: bool,
    /// What was done takes effect only after a reboot (a new kernel or kernel
    /// module, a login screen enabled for the first time): then the desktop
    /// install result says "reboot", otherwise "log out".
    pub restart_needed: bool,
}

/// Why the operation stopped before the end.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    Failed(String),
    Cancelled,
}

type Step<T> = Result<T, Stop>;

/// Event sender: remembers whether an error was announced (for the
/// [`de_progress::worker_error_emit`] rule), and is fit for the log reader thread.
#[derive(Clone)]
struct Emitter {
    sink: Arc<dyn DeOpSink>,
    error_seen: Arc<AtomicBool>,
}

impl Emitter {
    fn emit(&self, stage: &str, message: &str, fraction: Option<f64>) {
        if stage == de_progress::STAGE_ERROR {
            self.error_seen.store(true, Ordering::SeqCst);
        }
        self.sink.progress(stage, message, fraction);
    }

    /// Message of package manager steps: the bar share per stage is computed by
    /// the core (`install_de_fraction`): a port of the `_pkg_cb` closure.
    fn pkg(&self, stage: &str, message: &str) {
        self.emit(stage, message, plan::install_de_fraction(stage, message));
    }

    fn pkg_callback(&self) -> Arc<dyn Fn(&str, &str) + Send + Sync> {
        let me = self.clone();
        Arc::new(move |stage: &str, message: &str| me.pkg(stage, message))
    }
}

/// Rexon log: which group members were dropped from the install because of a
/// conflict (`plan::InstallPreview::conflict_skipped`): the window shows an
/// explanation, and the packages and the conflict pair stay here for analysis.
fn log_conflict_skips(p: &plan::InstallPreview) {
    for s in &p.conflict_skipped {
        let c = &s.conflict;
        crate::log_warn!(
            "desktops",
            "не ставим {} (стол {}): {} {} конфликтует с {} {}{} — держат: {}",
            s.package,
            if s.desktop.is_empty() { "?" } else { &s.desktop },
            c.new_package,
            c.new_version,
            c.installed,
            c.installed_version,
            if c.installed_is_new { " (встаёт в том же наборе)" } else { "" },
            if c.installed_desktops.is_empty() { "?".to_string() } else { c.installed_desktops.join(", ") },
        );
    }
}

// ============================================================================
// Executor
// ============================================================================

/// Order of desktop operation steps on top of [`DeOpsIo`].
pub struct DeOpsExec<'a, IO: DeOpsIo + ?Sized> {
    io: &'a IO,
    sys: SystemExec<'a, IO>,
    out: Emitter,
    cancel: CancelToken,
    cancellable: Cell<Option<bool>>,
    /// See [`DeOpOutcome::restart_needed`]; set by the operation steps.
    restart_needed: Cell<bool>,
    /// The main install step installed nothing (everything was already there).
    main_nothing_to_do: Cell<bool>,
}

impl<'a, IO: DeOpsIo + ?Sized> DeOpsExec<'a, IO> {
    pub fn new(io: &'a IO, sink: Arc<dyn DeOpSink>, cancel: CancelToken) -> Self {
        DeOpsExec {
            io,
            sys: SystemExec::new(io),
            out: Emitter {
                sink,
                error_seen: Arc::new(AtomicBool::new(false)),
            },
            cancel,
            cancellable: Cell::new(None),
            restart_needed: Cell::new(false),
            main_nothing_to_do: Cell::new(false),
        }
    }

    fn emit(&self, stage: &str, message: &str, fraction: Option<f64>) {
        self.out.emit(stage, message, fraction);
    }

    fn warn(&self, message: &str) {
        self.emit(de_progress::STAGE_WARNING, message, None);
    }

    /// Copies of the panels of all installed desktops. A copy that failed is
    /// a warning, never a reason to stop the installation.
    fn panel_snapshots(&self, reason: &str, progress: f64) {
        self.emit("panel_backup", &panel_backup_message(), Some(progress));
        for (de, outcome) in self.io.panel_snapshots(reason) {
            if let Err(msg) = outcome {
                let name = system::DE_META.iter().find(|m| m.id == de).map_or(de.as_str(), |m| m.display_name);
                self.warn(&panel_backup_failed(name, &msg));
            }
        }
    }

    fn raw(&self, message: &str) {
        self.emit(de_progress::STAGE_RAW, message, None);
    }

    /// Default applications log: in full to the Rexon log, to the window the
    /// warnings ([`window_defaults_lines`]) and one result line.
    fn show_defaults_lines(&self, lines: &[String], display: &str) {
        for line in lines {
            crate::log_info!("appdefaults", "{}", line.trim());
        }
        for line in window_defaults_lines(lines) {
            self.raw(&line);
        }
        self.raw(&format!("  {}", defaults_set_message(display)));
    }

    fn set_cancellable(&self, allowed: bool) {
        if self.cancellable.get() != Some(allowed) {
            self.cancellable.set(Some(allowed));
            self.out.sink.cancellable(allowed);
        }
    }

    /// A point where cancellation is still safe.
    fn checkpoint(&self) -> Step<()> {
        if self.cancel.is_cancelled() {
            Err(Stop::Cancelled)
        } else {
            Ok(())
        }
    }

    /// No cancellation from here on: system changes have started.
    fn point_of_no_return(&self) -> Step<()> {
        self.checkpoint()?;
        self.set_cancellable(false);
        Ok(())
    }

    /// Result: an error nobody mentioned along the way is announced here, by the
    /// same core rule as in the Python worker.
    fn finish(&self, result: Step<String>) -> DeOpOutcome {
        self.set_cancellable(false);
        match result {
            Ok(message) => DeOpOutcome {
                ok: true,
                message,
                cancelled: false,
                restart_needed: self.restart_needed.get(),
            },
            Err(Stop::Cancelled) => {
                self.warn(&cancelled_message());
                DeOpOutcome {
                    ok: false,
                    message: cancelled_message(),
                    cancelled: true,
                    restart_needed: false,
                }
            }
            Err(Stop::Failed(message)) => {
                let seen = self.out.error_seen.load(Ordering::SeqCst);
                if let Some(text) = de_progress::worker_error_emit(false, seen, &message) {
                    self.emit(de_progress::STAGE_ERROR, &text, None);
                }
                DeOpOutcome {
                    ok: false,
                    message,
                    cancelled: false,
                    restart_needed: false,
                }
            }
        }
    }

    // ── Environment ──────────────────────────────────────────────────────

    fn home(&self) -> String {
        let h = self.io.env("HOME");
        if h.is_empty() {
            "/".to_string()
        } else {
            h
        }
    }

    fn env_opt(&self, name: &str) -> Option<String> {
        let v = self.io.env(name);
        (!v.is_empty()).then_some(v)
    }

    fn config_home(&self) -> String {
        utils::config_home(self.env_opt("XDG_CONFIG_HOME").as_deref(), &self.home())
    }

    fn expand(&self, path: &str) -> String {
        utils::expand_user_xdg(
            path,
            &self.home(),
            self.env_opt("XDG_CONFIG_HOME").as_deref(),
            self.env_opt("XDG_DATA_HOME").as_deref(),
        )
    }

    fn detector(&self) -> DistroDetector {
        DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        )
    }

    /// Whether we are on SteamOS: the same detection that decides on the install
    /// (`DistroCompat::is_steamos`), from os-release via the same I/O.
    fn on_steamos(&self) -> bool {
        dc::is_steamos_id(&self.detector().distro_id())
    }

    fn lang(&self) -> dc::Lang {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            let v = self.io.env(key);
            if !v.is_empty() {
                return dc::Lang::from_locale(&v);
            }
        }
        dc::Lang::En
    }

    fn run_ok(&self, argv: &[String]) -> bool {
        self.io.run(argv, None, false).rc() == Some(0)
    }

    // ── Distribution compatibility ───────────────────────────────────────

    /// Verdict "can this desktop be installed here": a port of
    /// `distro_compat.explain_de_install`. The core decides
    /// (`DistroCompat::install_reason`); here only the raw material: os-release,
    /// the family and, on Astra SE, the ZPS (closed software environment) state.
    pub fn install_verdict(&self, de_id: &str) -> InstallVerdict {
        let det = self.detector();
        let compat = dc::DistroCompat::new(dc::Detector {
            distro_id: det.distro_id(),
            family: self.sys.family(),
            pretty_name: det.pretty_name(),
            variant_id: det
                .info
                .get("VARIANT_ID")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            name: det
                .info
                .get("NAME")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        });
        // ZPS is asked lazily, only on Astra SE, as in the original.
        let zps = det.distro_id() == "astra" && compat.is_astra_se() && self.zps_enabled();
        let reason = compat.install_reason(de_id, zps);
        let lang = self.lang();
        let verdict = InstallVerdict {
            allowed: reason.allowed(),
            outcome: reason.outcome().code().to_string(),
            title: reason.title(lang),
            explanation: reason.explanation(lang),
        };
        // Deepin only on Treeland (`crate::deepin_treeland`): on top of regular
        // Deepin a refusal before the password, otherwise an "experimental"
        // warning in the same question.
        if de_id != crate::deepin_treeland::DE_ID || !verdict.allowed {
            return verdict;
        }
        match crate::deepin_treeland::install_verdict(self.sys.family(), &|cmd: &plan::Cmd| self.preview_run(cmd)) {
            Some((allowed, title, explanation)) => InstallVerdict {
                allowed,
                outcome: if allowed { "warning" } else { "blocked" }.to_string(),
                title,
                explanation,
            },
            None => verdict,
        }
    }

    /// A probe command (without root) with the answer as [`plan::PreviewAnswer`].
    fn preview_run(&self, cmd: &plan::Cmd) -> plan::PreviewAnswer {
        let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
    }

    /// Port of `DistroCompat._is_zps_enabled`: the `digsig_verif` module in
    /// `lsmod`, then the config file.
    fn zps_enabled(&self) -> bool {
        let r = self.io.run(&["lsmod".to_string()], None, false);
        if r.rc() == Some(0) && dc::zps_enabled_from(Some(&r.stdout), None) {
            return true;
        }
        let conf = self.io.read_text("/etc/digsig/digsig_initramfs.conf");
        dc::zps_enabled_from(None, conf.as_deref())
    }

    // ── Install (also check and repair) ─────────────────────────────────

    /// Port of `install_de`. «Check / Repair» is the same thing: the package
    /// manager with `--needed` skips what is installed by itself.
    pub fn install_de(&self, de_id: &str, current_de: &str) -> DeOpOutcome {
        self.install_de_replacing(de_id, current_de, &[])
    }

    /// [`Self::install_de`] with consent to replace the installed packages
    /// `replace` («Replace River 0.5» on the card): the probe's conflict with
    /// them is resolved in the same transaction if it is a replacement from the
    /// table (`plan::conflict_swap_approved`). Any other conflict is the old
    /// refusal before the password.
    pub fn install_de_replacing(&self, de_id: &str, current_de: &str, replace: &[String]) -> DeOpOutcome {
        self.install_de_session(de_id, current_de, replace, None)
    }

    /// [`Self::install_de_replacing`] with the chosen session
    /// (`system::SESSION_VARIANTS`): for Wayland its add-on is added to the set
    /// (a compositor, the session bundle, sometimes from the AUR). `None` and
    /// X11: the set as before.
    pub fn install_de_session(
        &self,
        de_id: &str,
        current_de: &str,
        replace: &[String],
        session: Option<system::SessionKind>,
    ) -> DeOpOutcome {
        let result = self.install_de_steps(de_id, current_de, replace, session);
        self.finish(result)
    }

    fn install_de_steps(
        &self,
        de_id: &str,
        current_de: &str,
        replace: &[String],
        session: Option<system::SessionKind>,
    ) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == de_id) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        let display = meta.display_name;
        self.set_cancellable(true);

        // 1. Compatibility BEFORE any action.
        let verdict = self.install_verdict(de_id);
        if !verdict.allowed {
            let text = format!("{}\n\n{}", verdict.title, verdict.explanation);
            self.emit(de_progress::STAGE_ERROR, &text, None);
            return Err(Stop::Failed(text));
        }
        if verdict.outcome == "warning" {
            self.warn(&verdict.explanation);
        }
        self.checkpoint()?;

        let family = self.sys.family();
        let detector = self.detector();
        // Deepin only on Treeland: the set without deepin-session (provided by our
        // own build, installed before this step), otherwise pacman would pull it in
        // with deepin-kwin. No own build: refusal, Rexon does not install the whole
        // group (`crate::deepin_treeland`).
        let mut main_pkgs = if de_id == crate::deepin_treeland::DE_ID {
            match crate::deepin_treeland::install_set(family, &|cmd: &plan::Cmd| self.preview_run(cmd)) {
                Ok(set) => set,
                Err(text) => {
                    self.emit(de_progress::STAGE_ERROR, &text, None);
                    return Err(Stop::Failed(text));
                }
            }
        } else {
            system::get_packages(de_id, family)
        };
        // The add-on of the chosen session (Wayland for Xfce, LXQt, MATE).
        let session = session.unwrap_or(system::SessionKind::X11);
        for p in system::session_packages(de_id, family, session) {
            if !main_pkgs.contains(&p) {
                main_pkgs.push(p);
            }
        }
        let mut aur_wanted = system::get_aur_packages(de_id, family);
        for p in system::session_aur_packages(de_id, family, session) {
            if !aur_wanted.contains(&p) {
                aur_wanted.push(p);
            }
        }
        // What is marked as AUR in the database but is in the enabled repositories
        // (CachyOS: Mint themes and icons in `cachyos`) is installed by pacman in
        // the common transaction, without an AUR helper too.
        let (aur_in_repo, aur_pkgs) =
            plan::split_repo_available(family, &aur_wanted, &|cmd: &plan::Cmd| {
                let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
            });
        for p in aur_in_repo {
            if !main_pkgs.contains(&p) {
                main_pkgs.push(p);
            }
        }
        let aur_helper = plan::pick_aur_helper(family, &|h| self.io.which(h));
        // A Wayland session that exists only in the AUR: without an AUR helper it
        // would silently not get installed, and the person would look for it on the
        // login screen. Refusal before the password and before the first action.
        let session_aur = system::session_aur_packages(de_id, family, session);
        if aur_helper.is_none() && session_aur.iter().any(|p| aur_pkgs.contains(p)) {
            let text = crate::qt_desktops::session_no_aur_helper_text(display);
            self.emit(de_progress::STAGE_ERROR, &text, None);
            return Err(Stop::Failed(text));
        }

        // 1a. A trial transaction without root (Arch): if an optional group member
        //     in the repository cannot be installed right now (08.10.2026:
        //     switchboard-plug-online-accounts of Pantheon was waiting for a
        //     rebuild), we install the group without it and warn; if something the
        //     desktop cannot exist without cannot be installed, a clear refusal
        //     BEFORE the password instead of pacman aborting mid-install. No
        //     answer: install as before.
        // `deepin-kwin` (the Deepin X11 window manager) is never installed, neither
        // as a set member nor as a dependency.
        let preview = plan::install_preview(family, &main_pkgs, &|cmd: &plan::Cmd| self.preview_run(cmd)).map(|mut p| {
            crate::deepin_treeland::forbid_x11_wm(&mut p);
            p
        });
        // What from the set is installed only as a dependency (a DKMS module and
        // headers for the installed kernels) is marked `--asdeps` in the same transaction.
        let deps_only: Vec<String> = preview.as_ref().map(|p| p.deps_only.clone()).unwrap_or_default();
        // Replacement with consent: only what the probe found conflicting and what
        // the person agreed to replace; other names from `replace` are not used.
        let replace: Vec<String> = match &preview {
            Some(p) if plan::conflict_swap_approved(p, replace) => {
                plan::conflict_swaps(p).into_iter().map(|c| c.installed).collect()
            }
            _ => Vec::new(),
        };
        // A reboot is needed if a kernel or kernel module is installed
        // (`plan::restart_packages`) or there was no login screen before the
        // install: enabled now, it only comes up at boot, and without it there is
        // nowhere to "choose the desktop on the login screen".
        if preview.as_ref().is_some_and(|p| !p.restart_packages.is_empty()) || !self.sys.display_manager_present() {
            self.restart_needed.set(true);
        }
        let main_pkgs = match preview {
            Some(p) if !p.installable() && !replace.is_empty() => p.targets,
            Some(p) if !p.installable() => {
                let text = crate::qt_desktops::packages_notice(&p);
                self.emit(de_progress::STAGE_ERROR, &text, None);
                return Err(Stop::Failed(text));
            }
            Some(p) if !p.skipped.is_empty() || !p.conflict_skipped.is_empty() => {
                log_conflict_skips(&p);
                self.warn(&crate::qt_desktops::packages_notice(&p));
                p.targets
            }
            // The probe may have added a DKMS module and headers for the installed
            // kernels to the set (`plan::kernel_module_extras`): we install what was
            // checked.
            Some(p) => p.targets,
            None => main_pkgs,
        };
        self.checkpoint()?;

        // 2. Defaults snapshot and panel copy, before the package manager.
        self.emit("snapshot", &snapshot_message(), Some(0.02));
        if let Err(e) = self.defaults_snapshot() {
            self.warn(&snapshot_failed(&e));
        }
        self.panel_snapshots("before_install", 0.04);
        self.checkpoint()?;

        // 3. Packages. If the desktop is already installed, this is «Check /
        // Repair»: the package manager verifies the set (`--needed`) and adds only
        // what is missing, and that is what we say.
        let checking = self.sys.is_pkg_installed(system::get_check_binary(de_id).unwrap_or(de_id));
        let aur_missing_before: Vec<String> =
            aur_pkgs.iter().filter(|p| !self.sys.is_pkg_installed(p)).cloned().collect();
        self.main_nothing_to_do.set(false);
        let intro = if checking {
            check_intro(main_pkgs.len(), aur_pkgs.len())
        } else {
            install_intro(main_pkgs.len(), aur_pkgs.len())
        };
        self.emit("install", &intro, Some(0.05));
        let aur_for_batch: Vec<String> = if aur_helper.is_some() {
            aur_pkgs.clone()
        } else {
            Vec::new()
        };
        let used_de_helper = match self.install_batch(
            family,
            &detector,
            &main_pkgs,
            &deps_only,
            &replace,
            &aur_for_batch,
            aur_helper.as_deref(),
            de_id,
        ) {
            Ok(used) => used,
            Err(Stop::Failed(msg)) => {
                self.emit(de_progress::STAGE_ERROR, &msg, None);
                return Err(Stop::Failed(msg));
            }
            Err(stop) => return Err(stop),
        };
        // One can get here without ever running the package manager (nothing to
        // install): system changes follow anyway.
        self.set_cancellable(false);

        // 4. Post-install.
        self.emit("post_install", &post_install_message(), Some(0.85));
        let (post_ok, post_msg) = self.run_post_install(de_id, family, aur_helper.as_deref());
        if !post_ok {
            self.warn(&post_install_failed(&post_msg));
        }

        // 5. The login screen from the desktop's package list (if the helper has
        //    not already enabled it in the same authorization session).
        // If the login screen is already installed: one "already configured" line
        // instead of a header and two repeats (step 6 then stays silent).
        let dm_said = !used_de_helper && self.sys.display_manager_present();
        if dm_said {
            self.emit("dm_enable", &dm_already(&self.sys.active_login_manager()), Some(0.90));
        } else {
            self.emit("dm_enable", &dm_step_message(), Some(0.90));
        }
        if !used_de_helper && !dm_said {
            {
                let mut all = main_pkgs.clone();
                all.extend(aur_pkgs.iter().cloned());
                if let Some((unit, cmd)) = plan::dm_enable_step(
                    family,
                    &all,
                    false,
                    self.sys.pkg_helper_available(),
                    false,
                ) {
                    // The window's privilege session; without it, the old plan command.
                    let op = PrivOp::EnableDm { family, unit: unit.clone() };
                    let r = self
                        .io
                        .run_priv(&op, &[], None)
                        .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                    if r.rc() == Some(0) {
                        self.emit("dm_enable", &dm_enabled(&unit), None);
                    } else {
                        self.warn(&dm_enable_failed(&unit, &run_why(&r)));
                    }
                }
            }
        }

        // 6. Foolproof guard: there must be an enabled login screen.
        self.ensure_display_manager(family, dm_said);

        // 7. Default applications for the new desktop.
        self.emit("switcher", &switcher_message(display), Some(0.93));
        let (_, lines) = self.io.app_defaults_switch(de_id);
        self.show_defaults_lines(&lines, display);

        self.io
            .bridge_notify(&BridgeEvent::DeInstalled(de_id.to_string()));

        // 8. Restore the protected defaults the install overwrote.
        self.emit("restore", &restore_message(), Some(0.96));
        match self.defaults_restore() {
            Ok(results) => {
                let mut restored = 0;
                for (app_type, status) in results {
                    crate::log_info!("desktops", "умолчание {app_type}: {status}");
                    if status == "restored" {
                        restored += 1;
                        self.raw(&format!("  {}", default_restored_line(&dfl::app_type_label(&app_type))));
                    }
                }
                if restored == 0 {
                    self.raw(&format!("  {}", defaults_kept_message()));
                }
            }
            Err(e) => self.warn(&restore_failed(&e)),
        }

        // 9. An own mimeapps.list for each installed desktop.
        self.emit("mime", &mime_message(), Some(0.98));
        let mut installed: Vec<String> = system::DE_META
            .iter()
            .filter(|m| {
                let pkg = system::get_check_binary(m.id).unwrap_or(m.id);
                self.sys.is_pkg_installed(pkg)
            })
            .map(|m| m.id.to_string())
            .collect();
        if !installed.iter().any(|d| d == de_id) {
            installed.push(de_id.to_string());
        }
        for line in self.protect_de_mimeapps(current_de, &installed) {
            self.raw(&line);
        }

        let done = if checking {
            let still_missing = aur_pkgs.iter().any(|p| !self.sys.is_pkg_installed(p));
            let aur_added = aur_missing_before.iter().any(|p| self.sys.is_pkg_installed(p));
            if still_missing {
                repair_incomplete_message(display)
            } else if aur_added || !self.main_nothing_to_do.get() {
                repaired_message(display)
            } else {
                checked_message(display)
            }
        } else {
            installed_message(display)
        };
        self.emit("done", &done, Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    // ── Add a session to an installed desktop ───────────────────────────

    /// «➕ Add Wayland / X11 variant» on the card of an installed desktop: only
    /// what the `kind` session lacks is installed (`system::SESSION_VARIANTS`).
    /// The desktop is already installed, so the defaults snapshot, post-install,
    /// login screen, default applications and "open with" lists are not touched:
    /// a session does not change them.
    pub fn add_session(&self, de_id: &str, kind: system::SessionKind) -> DeOpOutcome {
        let result = self.add_session_steps(de_id, kind);
        self.finish(result)
    }

    fn add_session_steps(&self, de_id: &str, kind: system::SessionKind) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == de_id) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        let display = meta.display_name;
        let family = self.sys.family();
        let Some(variants) = system::session_variants(de_id, family) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        self.set_cancellable(true);
        let detector = self.detector();
        // The X11 session is in the desktop's main packages; the X server is pulled
        // in by their login screen (LightDM and SDDM depend on it).
        let mut main_pkgs = match kind {
            system::SessionKind::Wayland => system::session_packages(de_id, family, kind),
            system::SessionKind::X11 => system::get_packages(de_id, family),
        };
        let aur_wanted = system::session_aur_packages(de_id, family, kind);
        let (aur_in_repo, aur_pkgs) =
            plan::split_repo_available(family, &aur_wanted, &|cmd: &plan::Cmd| {
                let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
            });
        main_pkgs.extend(aur_in_repo);
        let mut seen = std::collections::HashSet::new();
        main_pkgs.retain(|p| seen.insert(p.clone()));
        let aur_helper = plan::pick_aur_helper(family, &|h| self.io.which(h));
        if aur_helper.is_none() && !aur_pkgs.is_empty() {
            let text = crate::qt_desktops::session_no_aur_helper_text(display);
            self.emit(de_progress::STAGE_ERROR, &text, None);
            return Err(Stop::Failed(text));
        }
        // A probe without root, as for the desktop install: if it cannot be
        // installed, a clear refusal before the password.
        let preview = plan::install_preview(family, &main_pkgs, &|cmd: &plan::Cmd| self.preview_run(cmd));
        let deps_only: Vec<String> = preview.as_ref().map(|p| p.deps_only.clone()).unwrap_or_default();
        let main_pkgs = match preview {
            Some(p) if !p.installable() => {
                let text = crate::qt_desktops::packages_notice(&p);
                self.emit(de_progress::STAGE_ERROR, &text, None);
                return Err(Stop::Failed(text));
            }
            Some(p) if !p.skipped.is_empty() || !p.conflict_skipped.is_empty() => {
                log_conflict_skips(&p);
                self.warn(&crate::qt_desktops::packages_notice(&p));
                p.targets
            }
            Some(p) => p.targets,
            None => main_pkgs,
        };
        self.checkpoint()?;

        self.emit("install", &session_adding_message(kind, display), Some(0.05));
        let aur_for_batch = if aur_helper.is_some() { aur_pkgs } else { Vec::new() };
        match self.install_batch(
            family,
            &detector,
            &main_pkgs,
            &deps_only,
            &[],
            &aur_for_batch,
            aur_helper.as_deref(),
            de_id,
        ) {
            Ok(_) => {}
            Err(Stop::Failed(msg)) => {
                self.emit(de_progress::STAGE_ERROR, &msg, None);
                return Err(Stop::Failed(msg));
            }
            Err(stop) => return Err(stop),
        }
        self.set_cancellable(false);
        let session = match kind {
            system::SessionKind::X11 => variants.x11_session_name,
            system::SessionKind::Wayland => variants.wayland_session_name,
        };
        self.emit("done", &session_added_message(kind, display, session), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    // ── Switching to another desktop ─────────────────────────────────────

    /// Port of `switch_de`: a copy of the current desktop's panel and configs,
    /// default applications for the new one, a bridge event, logout.
    pub fn switch_de(&self, target: &str, current_de: &str) -> DeOpOutcome {
        let result = self.switch_de_steps(target, current_de);
        self.finish(result)
    }

    fn switch_de_steps(&self, target: &str, current_de: &str) -> Step<String> {
        // Left part of the card: the desktop ON ITS OWN: this compositor's shell
        // marks are removed (see `crate::shell_bundle`).
        self.switch_steps_with(target, current_de, None)
    }

    /// Switch to a desktop; `shell`: log into it with this shell (the mode mark
    /// is put, not removed).
    fn switch_steps_with(
        &self,
        target: &str,
        current_de: &str,
        shell: Option<system::DesktopShell>,
    ) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == target) else {
            return Err(Stop::Failed(unknown_de_message(target)));
        };
        // Core guard: we do not switch to the current or to an unrecognised desktop.
        if let Some(refusal) = plan::switch_de_guard(current_de, target, meta.display_name) {
            return Err(Stop::Failed(refusal));
        }
        self.set_cancellable(true);

        self.panel_snapshots("before_switch", 0.10);
        self.checkpoint()?;

        if let Some(current) = system::DE_META.iter().find(|m| m.id == current_de) {
            self.emit(
                "config_backup",
                &config_backup_message(current.display_name),
                Some(0.30),
            );
            let home = self.home();
            let paths: Vec<String> = system::get_config_paths(current.id)
                .iter()
                .map(|p| self.expand(p).replace('~', &home))
                .collect();
            match self.io.de_config_backup(current.id, &paths) {
                Ok((name, count)) => {
                    self.emit("config_backup", &config_backup_done(&name, count), None)
                }
                Err(e) => self.warn(&config_backup_failed(&e)),
            }
        }

        // From here on default applications change and the session is closed.
        self.point_of_no_return()?;
        self.set_shell_mode(target, shell)?;
        self.emit("switcher", &switcher_message(meta.display_name), Some(0.60));
        let (_, lines) = self.io.app_defaults_switch(target);
        self.show_defaults_lines(&lines, meta.display_name);
        self.io.bridge_notify(&BridgeEvent::DeSwitch {
            from: current_de.to_string(),
            to: target.to_string(),
        });

        self.emit("done", &logout_message(), Some(1.0));
        self.logout_session()
    }

    /// Port of `logout_session`: the commands per environment come from the core
    /// (`logout_cmds`), the first successful one wins.
    fn logout_session(&self) -> Step<String> {
        let cmds = plan::logout_cmds(
            &self.io.env("XDG_CURRENT_DESKTOP"),
            &self.io.env("DESKTOP_SESSION"),
            &self.io.env("XDG_SESSION_ID"),
        );
        let mut last = String::new();
        for cmd in cmds {
            let name = cmd.argv.first().cloned().unwrap_or_default();
            let r = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
            match r.outcome {
                RunOutcome::Ok => return Ok(de_progress::MSG_OK.to_string()),
                RunOutcome::Timeout => last = logout_timeout(&name),
                RunOutcome::Error => last = logout_not_started(&name),
                RunOutcome::Failed => last = r.stderr.trim().to_string(),
            }
        }
        Err(Stop::Failed(if last.is_empty() {
            logout_failed_message()
        } else {
            last
        }))
    }

    // ── Desktop removal ──────────────────────────────────────────────────

    /// Port of `remove_de(force=False)`. ⛔ The window has NO way around the
    /// guard: the core will not remove the current desktop even on a direct call
    /// (architecture rule #13; the second line of defence: there is no button on
    /// the current desktop at all).
    pub fn remove_de(&self, de_id: &str, current_de: &str) -> DeOpOutcome {
        let result = self.remove_de_steps(de_id, current_de);
        self.finish(result)
    }

    fn remove_de_steps(&self, de_id: &str, current_de: &str) -> Step<String> {
        let Some(meta) = system::DE_META.iter().find(|m| m.id == de_id) else {
            return Err(Stop::Failed(unknown_de_message(de_id)));
        };
        // SteamOS (Steam Deck and Steam Machine): we do not touch packages in either
        // direction: removal would require lifting the system partition protection,
        // and the next update would bring everything back. Refusal before the first
        // side effect, with the same text as on the inactive button.
        if let Some(reason) = dc::SteamOsBlock::Desktops.on(self.on_steamos()) {
            self.emit(de_progress::STAGE_ERROR, &reason, None);
            return Err(Stop::Failed(reason));
        }
        let family = self.sys.family();
        let packages = system::get_packages(de_id, family);
        // The Wayland session bundle (lxqt-wayland-session and similar ones from
        // the AUR) holds the desktop's packages as a dependency: removed together
        // with it; the removal plan filters out what is not installed.
        let mut aur = system::get_aur_packages(de_id, family);
        for p in system::session_removal_packages(de_id, family) {
            if !packages.contains(&p) && !aur.contains(&p) {
                aur.push(p);
            }
        }
        let all = match plan::remove_de_decision(
            de_id,
            meta.display_name,
            current_de,
            false,
            &packages,
            &aur,
        ) {
            plan::RemoveDeDecision::Refused(text) => return Err(Stop::Failed(text)),
            plan::RemoveDeDecision::Proceed(all) => all,
        };
        // The current desktop's packages and its login screen are never removed
        // (the shared `lightdm` of Xfce and Cinnamon): `system::current_desktop_keep`.
        let keep = system::current_desktop_keep(
            family,
            current_de,
            self.sys.active_login_manager_opt().as_deref(),
        );
        let group = |p: &str| self.group_members_output(family, p);
        let all = system::without_current_desktop(family, &all, &keep, &group);
        if all.is_empty() {
            let message = remove_all_shared_message(meta.display_name);
            self.emit(de_progress::STAGE_ERROR, &message, None);
            return Err(Stop::Failed(message));
        }
        self.set_cancellable(true);
        self.emit("remove", &removing_message(meta.display_name), Some(0.10));
        let (ok, message) = self.pkg_remove(family, &all)?;
        if !ok {
            self.emit(de_progress::STAGE_ERROR, &message, None);
            return Err(Stop::Failed(message));
        }
        self.io
            .bridge_notify(&BridgeEvent::DeRemoved(de_id.to_string()));
        self.remove_desktop_leftovers(de_id);
        self.emit("done", &removed_message(meta.display_name), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    /// Port of `PackageManager.remove`: the probes (groups, is it installed, who
    /// needs it) run while the core's plan is built; here, waiting for the lock
    /// and the removal command itself.
    /// Output of the "what is in the group" query (`pacman -Qg`) on success.
    fn group_members_output(&self, family: PkgFamily, p: &str) -> Option<String> {
        let q = plan::package_group_query(family, p)?;
        if !self.io.which(&q.binary) {
            return None;
        }
        let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
        (r.rc() == Some(0)).then_some(r.stdout)
    }

    fn pkg_remove(&self, family: PkgFamily, packages: &[String]) -> Step<(bool, String)> {
        let group = |p: &str| self.group_members_output(family, p);
        let meta = |p: &str| self.sys.pkg_meta_stdout(p, family);
        let installed = |p: &str| self.sys.is_pkg_installed(p);
        let removal = plan::remove_plan(
            family,
            packages,
            &group,
            &meta,
            &installed,
            self.sys.pkg_helper_available(),
        )
        .map_err(Stop::Failed)?;
        if let Some((ok, msg)) = removal.result_without_exec {
            return Ok((ok, msg));
        }
        for step in &removal.steps {
            match step {
                PlanStep::WaitPkgLock { timeout_secs, .. } => {
                    // Lock paths come from the executor (see `DeOpsIo::pkg_lock_paths`).
                    let paths = self.io.pkg_lock_paths(family);
                    let (ok, msg) = self.wait_pkg_lock(&paths, *timeout_secs)?;
                    if !ok {
                        return Ok((false, msg));
                    }
                }
                PlanStep::Run {
                    id: RunStepId::PkgRemove,
                    cmd,
                    ..
                } => {
                    self.point_of_no_return()?;
                    // The window's privilege session removes the same set (`removal.safe`);
                    // without it, the old plan command.
                    let op = PrivOp::PkgRemove { family, packages: removal.safe.clone() };
                    let r = self
                        .io
                        .run_priv(&op, &[], None)
                        .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                    if r.outcome == RunOutcome::Error {
                        return Ok((false, tr("Command not found")));
                    }
                    if r.outcome == RunOutcome::Timeout {
                        return Ok((false, remove_timeout_message()));
                    }
                    if r.returncode == Some(0) {
                        // To the person, only what was skipped (needed by other desktops); the
                        // package manager output stays as the result for the code, as in the
                        // original.
                        if !removal.success_note.trim().is_empty() {
                            self.raw(removal.success_note.trim());
                        }
                        let removed = tr("Removed");
                        let out = if r.stdout.is_empty() {
                            removed.as_str()
                        } else {
                            r.stdout.as_str()
                        };
                        return Ok((true, format!("{out}{}", removal.success_note)));
                    }
                    let mut err = if !r.stderr.is_empty() {
                        r.stderr.clone()
                    } else if !r.stdout.is_empty() {
                        r.stdout.clone()
                    } else {
                        tr("Removal error")
                    };
                    if err.contains("Not authorized") || r.returncode == Some(126) {
                        err = removal.sudo_hint.clone();
                    }
                    return Ok((false, err));
                }
                _ => {}
            }
        }
        Ok((true, String::new()))
    }

    // ── Batch install ────────────────────────────────────────────────────

    /// Port of `PackageManager.install_batch` → whether rexon-de-helper was used.
    fn install_batch(
        &self,
        family: PkgFamily,
        detector: &DistroDetector,
        packages: &[String],
        deps_only: &[String],
        replace: &[String],
        aur_packages: &[String],
        aur_helper: Option<&str>,
        de_id: &str,
    ) -> Step<bool> {
        let steamos = detector.is_steamos();
        let prefix = if steamos {
            "rexon_steamos_"
        } else {
            "rexon_install_"
        };
        let tmp_sh = self
            .io
            .temp_file(prefix, ".sh")
            .map_err(|e| Stop::Failed(temp_failed(&e)))?;
        let tmp_log = match self.io.temp_file(prefix, ".log") {
            Ok(p) => p,
            Err(e) => {
                self.io.remove_file(&tmp_sh);
                return Err(Stop::Failed(temp_failed(&e)));
            }
        };
        let batch = match plan::install_batch_plan_with_deps(
            family,
            detector,
            packages,
            deps_only,
            replace,
            aur_packages,
            aur_helper,
            true,
            Some(de_id),
            self.sys.de_helper_available(),
            &tmp_sh,
            &tmp_log,
        ) {
            Ok(b) => b,
            Err(e) => {
                self.io.remove_file(&tmp_sh);
                self.io.remove_file(&tmp_log);
                return Err(Stop::Failed(e));
            }
        };

        // Unknown step: refusal BEFORE the first action (item 3 of the header).
        for step in &batch.steps {
            if let PlanStep::Run { id, .. } = step {
                if !matches!(
                    id,
                    RunStepId::PkgMain | RunStepId::PkgNix | RunStepId::PkgAur | RunStepId::PkgUpdateDb
                ) {
                    self.io.remove_file(&tmp_sh);
                    self.io.remove_file(&tmp_log);
                    return Err(Stop::Failed(unknown_step_message(id.as_str())));
                }
            }
        }

        // The wrapper script is only needed by the old path; otherwise the empty file goes.
        let writes_sh = batch
            .steps
            .iter()
            .any(|s| matches!(s, PlanStep::WriteFile { path, .. } if *path == tmp_sh));
        let tmp_sh = if writes_sh {
            Some(tmp_sh)
        } else {
            self.io.remove_file(&tmp_sh);
            None
        };
        let cleanup = || {
            if let Some(p) = &tmp_sh {
                self.io.remove_file(p);
            }
            self.io.remove_file(&tmp_log);
        };

        if let Some((ok, msg)) = &batch.early_result {
            cleanup();
            if !ok {
                let stage = if family == PkgFamily::Nix {
                    "nixos"
                } else {
                    "immutable"
                };
                self.out.pkg(stage, msg);
                return Err(Stop::Failed(msg.clone()));
            }
            return Ok(false);
        }

        if steamos {
            self.out.pkg("steamos", &tr("🎮 SteamOS: installing through steamos-readonly…"));
            self.out
                .pkg("steamos", &tr("⚠️ Packages may be removed when SteamOS updates!"));
        } else if !packages.is_empty() {
            self.out.pkg("update", &tr("📥 Updating the package database…"));
        }

        // To the window's privilege session: a plain desktop set (all names from
        // its table, no DKMS and no replacement) is `PkgInstall` by the table; a set
        // beyond it (Deepin on Treeland, a group member, DKMS `--asdeps`, replacing
        // a conflicting package) is `DeInstallSet`, whose core rule the helper
        // repeats itself against the machine's pacman. No session: the old plan
        // wrapper. Nix: without root; SteamOS installs desktops as an image
        // (`crate::steamos_sysext`), it only gets here by the old path.
        let plain = deps_only.is_empty()
            && replace.is_empty()
            && de_id != crate::deepin_treeland::DE_ID
            && crate::privilege::validate::check_install(
                family,
                packages,
                crate::privilege::validate::InstallSource::Desktop { de_id },
            )
            .is_ok();
        let session_op = if family == PkgFamily::Nix || steamos || packages.is_empty() {
            None
        } else if plain {
            Some(PrivOp::PkgInstall {
                family,
                packages: packages.to_vec(),
                source: PkgSource::Desktop { de_id: de_id.to_string() },
            })
        } else if family == PkgFamily::Arch {
            Some(PrivOp::DeInstallSet {
                family,
                de_id: Some(de_id.to_string()),
                packages: packages.to_vec(),
                // As in the plan: mark only what is in the set.
                deps_only: deps_only.iter().filter(|d| packages.contains(d)).cloned().collect(),
                replace: replace.to_vec(),
                // The plan wrapper updates the database before installing: same here.
                update_first: true,
            })
        } else {
            None
        };
        let mut session_used = false;
        let mut r_main: Option<RunOut> = None;
        let mut main_seen = false;
        // Waiting for the lock before the main step is fatal; before the AUR tail it
        // is not (as in the original).
        let mut main_pending = !packages.is_empty();

        for step in &batch.steps {
            match step {
                PlanStep::WriteFile {
                    path,
                    content,
                    mode,
                } => {
                    if let Err(e) = self.io.write_file_mode(path, content, *mode) {
                        cleanup();
                        return Err(Stop::Failed(write_failed(path, &e)));
                    }
                }
                PlanStep::EnsureGentooOverlay { overlay } => {
                    if let Err(stop) = self.point_of_no_return() {
                        cleanup();
                        return Err(stop);
                    }
                    self.out.pkg("overlay", &gentoo_overlay_intro(overlay));
                    if !self.ensure_gentoo_overlay(family, de_id) {
                        cleanup();
                        return Err(Stop::Failed(gentoo_overlay_failed(overlay)));
                    }
                    self.out.pkg("overlay_done", &gentoo_overlay_done(overlay));
                }
                PlanStep::WaitPkgLock { timeout_secs, .. } => match self
                    .wait_pkg_lock(&self.io.pkg_lock_paths(family), *timeout_secs)
                {
                    Err(stop) => {
                        cleanup();
                        return Err(stop);
                    }
                    Ok((false, msg)) if main_pending => {
                        cleanup();
                        return Err(Stop::Failed(msg));
                    }
                    Ok(_) => {}
                },
                PlanStep::RemoveFiles { paths } => {
                    for p in paths {
                        self.io.remove_file(p);
                    }
                }
                PlanStep::Run { id, cmd, .. } => match id {
                    RunStepId::PkgMain => {
                        if let Err(stop) = self.point_of_no_return() {
                            cleanup();
                            return Err(stop);
                        }
                        self.out
                            .pkg("install", &plan::pkg_install_start_message(packages.len(), steamos));
                        let mut tail =
                            self.io
                                .tail_log(&tmp_log, LogTailKind::Pacman, self.out.pkg_callback());
                        // The session helper's progress goes to the same log `tail_log` reads, so
                        // the person sees the same lines.
                        let via_session =
                            session_op.as_ref().and_then(|op| self.io.run_priv(op, &[], Some(&tmp_log)));
                        session_used = via_session.is_some();
                        let r = via_session
                            .unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                        if let Some(t) = tail.as_mut() {
                            t.stop();
                        }
                        // "there is nothing to do": the whole set was already installed.
                        let log = self.io.read_text(&tmp_log).unwrap_or_default();
                        self.main_nothing_to_do
                            .set(pacman_nothing_to_do(&log) || pacman_nothing_to_do(&r.stdout));
                        r_main = Some(r);
                        main_seen = true;
                        main_pending = false;
                    }
                    RunStepId::PkgNix => {
                        if let Err(stop) = self.point_of_no_return() {
                            cleanup();
                            return Err(stop);
                        }
                        r_main = Some(self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                        main_seen = true;
                        main_pending = false;
                    }
                    RunStepId::PkgAur => {
                        // The main step result is checked BEFORE the AUR tail.
                        let (ok, msg) =
                            self.batch_main_result(r_main.as_ref(), main_seen, packages, steamos, family);
                        if !ok {
                            return Err(Stop::Failed(msg));
                        }
                        main_seen = false;
                        if let Err(stop) = self.point_of_no_return() {
                            return Err(stop);
                        }
                        self.batch_run_aur(&cmd.argv, cmd.timeout_secs, aur_packages, steamos);
                    }
                    _ => {
                        // Updating the database on Nix is a simple command.
                        let _ = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                    }
                },
            }
        }

        if main_seen {
            let (ok, msg) =
                self.batch_main_result(r_main.as_ref(), main_seen, packages, steamos, family);
            if !ok {
                return Err(Stop::Failed(msg));
            }
        }
        // The login screen is enabled by the DE helper; with a privilege session it
        // was not there, so enabling is a separate step (`EnableDm`).
        Ok(batch.used_de_helper && !session_used)
    }

    /// Port of `_batch_main_result`: texts and progress lines come from the core.
    fn batch_main_result(
        &self,
        r: Option<&RunOut>,
        main_seen: bool,
        packages: &[String],
        steamos: bool,
        family: PkgFamily,
    ) -> (bool, String) {
        let result = r.and_then(|r| {
            r.rc()
                .map(|rc| (rc, r.stderr.as_str(), r.stdout.as_str()))
        });
        let agent = || self.polkit_agent_running();
        let installed = || {
            plan::POLKIT_AGENT_PATHS.iter().find(|p| self.io.is_file(p)).map(|p| p.to_string())
        };
        let out = plan::batch_main_result(result, main_seen, packages, steamos, family, &agent, &installed);
        for (stage, message) in &out.logs {
            // "Main packages installed (6)" when "nothing to install" is untrue: the
            // number there is the set's names (groups), not what was installed.
            if stage == "done_main" && self.main_nothing_to_do.get() {
                self.out.pkg(stage, &main_already_installed());
            } else {
                self.out.pkg(stage, message);
            }
        }
        (out.ok, out.message)
    }

    /// Whether a password window (polkit agent) is running for our session: we
    /// ask polkit itself (`plan::polkit_agent_probe_argv`). If asking failed, we
    /// assume there is an agent (as in the original).
    ///
    /// ⚠️ Fixed in the port: the original searched `pgrep -f "polkit.*agent|polkitd"`,
    /// and `polkitd` matches the system service, which always runs, so the
    /// answer was "yes" even without a single agent.
    fn polkit_agent_running(&self) -> bool {
        let mut session = self.io.env("XDG_SESSION_ID");
        if session.is_empty() {
            let r = self.io.run(&plan::login_session_argv(), Some(10), true);
            if r.rc() == Some(0) {
                session = r.stdout.trim().to_string();
            }
        }
        if !plan::login_session_id_ok(&session) {
            return true;
        }
        let r = self.io.run(&plan::polkit_agent_probe_argv(&session), Some(10), true);
        plan::polkit_agent_probe_answer(r.rc(), &r.stderr).unwrap_or(true)
    }

    /// Port of `_batch_run_aur`.
    fn batch_run_aur(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        aur_packages: &[String],
        steamos: bool,
    ) {
        let (stage, intro) = plan::batch_aur_intro(steamos, aur_packages.len());
        self.out.pkg(&stage, &intro);
        let emit = self.out.pkg_callback();
        let forward: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |_stage: &str, msg: &str| emit("aur", msg));
        let r = self.run_aur_with_progress(argv, timeout_secs, forward);
        let ok = r.rc() == Some(0);
        let (mut failed, mut installed) = (Vec::new(), Vec::new());
        if !steamos && !ok {
            for p in aur_packages {
                if self.sys.is_pkg_installed(p) {
                    installed.push(p.clone());
                } else {
                    failed.push(p.clone());
                }
            }
        }
        for (stage, message) in plan::batch_aur_result(
            steamos,
            ok,
            aur_packages.len(),
            &failed,
            &installed,
            r.outcome == RunOutcome::Timeout,
        ) {
            self.out.pkg(&stage, &message);
        }
        // Why it did not install, in words, from the helper's output, not "see the
        // log" (`aur_install::failure_cause`).
        if !ok && r.outcome != RunOutcome::Timeout && (steamos || !failed.is_empty()) {
            if let Some(cause) = crate::aur_install::failure_cause(&r.stdout) {
                let reason = crate::aur_install::failure_text(&cause);
                self.out.pkg("aur_warn", &format!("   {}", trf("Reason: %1", &[&reason])));
            }
        }
    }

    /// Port of `_run_aur_with_progress`: the build output goes to a file, the
    /// progress is read by the core's reader (the `aur` kind).
    fn run_aur_with_progress(
        &self,
        argv: &[String],
        timeout_secs: Option<u64>,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> RunOut {
        let Ok(log) = self.io.temp_file("rexon_aur_", ".log") else {
            return RunOut::missing();
        };
        let mut tail = self.io.tail_log(&log, LogTailKind::Aur, emit);
        let mut r = self.io.run_to_file(argv, timeout_secs, &log);
        if let Some(t) = tail.as_mut() {
            t.stop();
        }
        let text = self.io.read_text(&log).unwrap_or_default();
        self.io.remove_file(&log);
        if !r.no_answer() {
            r.stdout = text;
            r.stderr = String::new();
        }
        r
    }

    /// Waiting for the package manager lock with cancellation. The decision on
    /// each round is the core's (`lock_wait_step`); here the clock, sleep and the
    /// cancellation flag.
    fn wait_pkg_lock(&self, paths: &[String], timeout_secs: u64) -> Step<(bool, String)> {
        let start = self.io.elapsed_secs();
        let mut notified = false;
        loop {
            self.checkpoint()?;
            let locked = !paths.is_empty() && paths.iter().any(|p| self.io.exists(p));
            match plan::lock_wait_step(locked, self.io.elapsed_secs() - start, timeout_secs, notified)
            {
                LockWaitStep::Ready { message } => return Ok((true, message)),
                LockWaitStep::TimedOut { message } => return Ok((false, message)),
                LockWaitStep::Notify {
                    stage,
                    message,
                    poll_secs,
                } => {
                    notified = true;
                    self.out.pkg(&stage, &message);
                    self.io.sleep_secs(poll_secs);
                }
                LockWaitStep::Wait { poll_secs } => self.io.sleep_secs(poll_secs),
            }
        }
    }

    /// Port of `ensure_gentoo_overlay`.
    fn ensure_gentoo_overlay(&self, family: PkgFamily, de_id: &str) -> bool {
        if family != PkgFamily::Gentoo {
            return true;
        }
        let Some(overlay) = plan::gentoo_overlay_for(de_id) else {
            return true;
        };
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let listed = self
            .io
            .run(&s(&["eselect", "repository", "list", "-i"]), None, false);
        if !listed.no_answer() && listed.stdout.contains(overlay) {
            return true;
        }
        if self.io.which("eselect") {
            if let Some(tool) =
                plan::gentoo_overlay_tool_install_cmd(family, self.sys.pkg_helper_available())
            {
                // The window's privilege session; without it, the old command.
                let op = PrivOp::PkgInstall {
                    family,
                    packages: vec![plan::GENTOO_OVERLAY_TOOL_PACKAGE.to_string()],
                    source: PkgSource::GentooTool,
                };
                if self.io.run_priv(&op, &[], None).is_none() {
                    let _ = self.io.run(&tool, None, false);
                }
            }
            let enabled = self.io.run(
                &s(&["pkexec", "eselect", "repository", "enable", overlay]),
                None,
                false,
            );
            if enabled.rc() == Some(0) {
                if let Some(sync) = plan::sync_repo_cmd(family, overlay) {
                    let _ = self.io.run(&sync, None, false);
                }
                return true;
            }
        }
        if self.io.which("layman") {
            let r = self
                .io
                .run(&s(&["pkexec", "layman", "-a", overlay]), Some(300), false);
            if r.rc() == Some(0) {
                return true;
            }
        }
        false
    }

    /// Port of `ensure_display_manager`: the decision is [`plan::ensure_dm_plan`].
    /// Does not break the install: the login screen is a best effort on top of
    /// the already installed desktop, but every failure is said out loud.
    /// `already_said`: the "login screen already configured" line is already in
    /// the window.
    fn ensure_display_manager(&self, family: PkgFamily, already_said: bool) {
        let dm_plan = plan::ensure_dm_plan(
            family,
            self.sys.display_manager_present(),
            self.io.which("systemctl"),
            self.sys.pkg_helper_available(),
        );
        match dm_plan {
            plan::EnsureDmPlan::Present { message, .. } => {
                if !already_said {
                    self.emit("dm_ensure", &message, None);
                }
            }
            plan::EnsureDmPlan::Skipped { message, .. } => {
                if let Some(m) = message {
                    self.emit("dm_ensure", &m, None);
                }
            }
            plan::EnsureDmPlan::Install {
                message,
                fraction,
                package,
                install,
                enable,
                done_message,
                done_fraction,
                ..
            } => {
                self.emit("dm_ensure", &message, Some(fraction));
                // The window's privilege session; without it, the old plan commands.
                let op = PrivOp::PkgInstall {
                    family,
                    packages: vec![package.clone()],
                    source: PkgSource::DisplayManager,
                };
                let r = self
                    .io
                    .run_priv(&op, &[], None)
                    .unwrap_or_else(|| self.io.run(&install.argv, install.timeout_secs, install.lang_c));
                if r.rc() != Some(0) {
                    self.warn(&ensure_dm_install_failed(&package, &run_why(&r)));
                    return;
                }
                // The guard installs and enables SDDM specifically (`plan::ensure_dm_plan`).
                let op = PrivOp::EnableDm { family, unit: "sddm".to_string() };
                let r = self
                    .io
                    .run_priv(&op, &[], None)
                    .unwrap_or_else(|| self.io.run(&enable.argv, enable.timeout_secs, enable.lang_c));
                if r.rc() != Some(0) {
                    self.warn(&ensure_dm_enable_failed(&run_why(&r)));
                    return;
                }
                self.emit("dm_ensure", &done_message, Some(done_fraction));
            }
        }
    }

    // ── Post-install ─────────────────────────────────────────────────────

    /// Port of `run_post_install`: hooks for MATE (the Ubuntu MATE look) and for
    /// River (the starter init: without it River starts without key bindings).
    /// For the tiling ones and Wayfire, also the keyboard layout in the config
    /// ([`crate::keyboard_layout`]): by themselves they enable only `us`. Wayfire
    /// and LXQt get the screen lock (the key and the locker command): without
    /// it «Lock» does not work on a clean machine.
    /// Same for sway, Hyprland and niri: the locker is installed, but their
    /// stock configs lack the key and/or idle locking. The lock runs before
    /// the layout: it may create the user's sway config (`include` of the
    /// system one), and the layout then goes into that file.
    fn run_post_install(
        &self,
        de_id: &str,
        family: PkgFamily,
        aur_helper: Option<&str>,
    ) -> (bool, String) {
        let (ok, message) = match de_id {
            "mate" => self.post_install_mate(family, aur_helper),
            "river" => self.post_install_river(),
            "wayfire" => self.post_install_wayfire_lock(),
            "lxqt" => self.post_install_lxqt_lock(),
            "sway" | "hyprland" | "niri" => self.post_install_tiling_lock(de_id),
            _ => (true, String::new()),
        };
        self.post_install_lock_look(de_id);
        // A layout failure is a warning, not a failure: the desktop is already installed.
        let keyboard = crate::keyboard_layout::ensure_after_install(self.io, de_id, &self.home());
        match &keyboard {
            Some((true, line)) => self.post(line),
            Some((false, line)) => self.warn(line),
            None => {}
        }
        match (message.is_empty(), keyboard) {
            (true, Some((_, line))) => (ok, line),
            (true, None) => (ok, tr("No post-install actions")),
            (false, _) => (ok, message),
        }
    }

    /// River starter init, only if there is no own one yet
    /// ([`plan::river_starter_init`]). If it fails, a warning rather than an
    /// install failure: the packages are already installed.
    fn post_install_river(&self) -> (bool, String) {
        let path = plan::river_init_path(&self.io.env("XDG_CONFIG_HOME"), &self.home());
        if self.io.exists(&path) {
            let line = plan::river_init_kept(&path);
            self.post(&line);
            return (true, line);
        }
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        let written = self
            .io
            .make_dirs(dir)
            .and_then(|()| self.io.write_file_mode(&path, plan::river_starter_init(), plan::RIVER_INIT_MODE));
        match written {
            Ok(()) => {
                let line = plan::river_init_written(&path);
                self.post(&line);
                (true, line)
            }
            Err(e) => {
                let line = plan::river_init_failed(&path, &e);
                (false, line)
            }
        }
    }

    /// Wayfire screen lock: the key and swayidle in `wayfire.ini`
    /// ([`plan::wayfire_lock_edit`]). The file is the same one Wayfire reads:
    /// `~/.config/wayfire/wayfire.ini` if it exists, otherwise
    /// `~/.config/wayfire.ini`. A failure is a warning, not a failure.
    fn post_install_wayfire_lock(&self) -> (bool, String) {
        let cfg = self.config_home();
        let vendored = format!("{cfg}/wayfire/wayfire.ini");
        let path = if self.io.is_file(&vendored) { vendored } else { format!("{cfg}/wayfire.ini") };
        let current = self.io.read_text(&path).unwrap_or_default();
        self.write_lock_setup("Wayfire", "Super+Shift+Esc", &path, plan::wayfire_lock_edit(&current))
    }

    /// LXQt screen lock: the commands for both sessions in `session.conf`
    /// ([`plan::lxqt_lock_edit`]).
    fn post_install_lxqt_lock(&self) -> (bool, String) {
        let path = format!("{}/lxqt/session.conf", self.config_home());
        let current = self.io.read_text(&path);
        self.write_lock_setup("LXQt", "Ctrl+Alt+L (Wayland: Super+L)", &path, plan::lxqt_lock_edit(current.as_deref()))
    }

    /// Screen lock for tiling desktops: sway — Super+Shift+Esc and swayidle,
    /// Hyprland — Super+Shift+Esc → hyprlock and hypridle (+ `hypridle.conf`
    /// unless the user has one), niri — swayidle (its sample already has the
    /// key). Same config as the layout uses
    /// ([`crate::keyboard_layout::load_config`]); the user's own settings are
    /// left alone ([`plan::sway_lock_edit`] and friends). Lock programs
    /// missing (no package in this family) — a plain log line, config
    /// untouched: a binding to a missing program would only confuse.
    fn post_install_tiling_lock(&self, de_id: &str) -> (bool, String) {
        let Some(cfg) = crate::keyboard_layout::load_config(self.io, de_id, &self.home()) else {
            return (true, String::new());
        };
        let desktop = cfg.desk.display_name();
        let (locker, idle, keys) = match de_id {
            "hyprland" => (plan::HYPR_LOCK_COMMAND, plan::HYPR_IDLE_COMMAND, "Super+Shift+Esc"),
            "niri" => ("swaylock", "swayidle", "Super+Alt+L"),
            _ => ("swaylock", "swayidle", "Super+Shift+Esc"),
        };
        let missing: Vec<&str> = [locker, idle].into_iter().filter(|p| !self.io.which(p)).collect();
        if !missing.is_empty() {
            let line = plan::lock_setup_no_program(desktop, &missing.join(", "));
            self.post(&line);
            return (true, line);
        }
        let Some(main) = cfg.text.as_deref() else {
            let line = plan::lock_setup_no_config(desktop, &cfg.path);
            self.post(&line);
            return (true, line);
        };
        let config_home = self.config_home();
        let idle_elsewhere = plan::idle_service_paths(&config_home, idle).iter().any(|p| self.io.exists(p));
        let text = match de_id {
            "hyprland" => {
                let lua = cfg.desk == crate::keyboard_layout::Desk::HyprLua;
                plan::hypr_lock_edit(lua, main, &cfg.included, idle_elsewhere).map(|edit| {
                    let conf = format!("{config_home}/hypr/hypridle.conf");
                    if edit.idle && !self.io.exists(&conf) {
                        let written = self.io.make_dirs(&format!("{config_home}/hypr")).and_then(|()| self.io.write_text(&conf, &plan::hypridle_conf()));
                        if let Err(e) = written {
                            self.warn(&plan::lock_setup_failed(desktop, &conf, &e));
                        }
                    }
                    edit.text
                })
            }
            "niri" => plan::niri_idle_edit(main, &cfg.included, idle_elsewhere),
            _ => plan::sway_lock_edit(main, &cfg.included, idle_elsewhere),
        };
        self.write_lock_setup(desktop, keys, &cfg.path, text)
    }

    /// Lock screen look for the desktops whose lock is swaylock (sway, niri,
    /// Wayfire, River, LXQt Wayland) or hyprlock (Hyprland): without a config
    /// swaylock is a white screen and hyprlock a blurred screenshot. Written
    /// only when the locker is installed and no config exists anywhere it
    /// looks ([`plan::swaylock_config_paths`], [`plan::hyprlock_config_paths`]):
    /// the user's or the distribution's own look is never replaced. A failure
    /// is a warning: locking itself still works.
    fn post_install_lock_look(&self, de_id: &str) {
        let config_home = self.config_home();
        // `own` is the user's file among the paths: the one in `config_home`.
        let (locker, paths, own, text) = match de_id {
            "hyprland" => (plan::HYPR_LOCK_COMMAND, plan::hyprlock_config_paths(&config_home), 0, plan::hyprlock_conf()),
            "sway" | "niri" | "wayfire" | "river" | "lxqt" => {
                ("swaylock", plan::swaylock_config_paths(&self.home(), &config_home), 1, plan::swaylock_config())
            }
            _ => return,
        };
        if !self.io.which(locker) || paths.iter().any(|p| self.io.exists(p)) {
            return;
        }
        let path = &paths[own];
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        match self.io.make_dirs(dir).and_then(|()| self.io.write_text(path, &text)) {
            Ok(()) => self.post(&plan::lock_look_written(locker, path)),
            Err(e) => self.warn(&plan::lock_setup_failed(locker, path, &e)),
        }
    }

    fn write_lock_setup(&self, desktop: &str, keys: &str, path: &str, text: Option<String>) -> (bool, String) {
        let Some(text) = text else {
            let line = plan::lock_setup_kept(desktop, path);
            self.post(&line);
            return (true, line);
        };
        let dir = path.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
        match self.io.make_dirs(dir).and_then(|()| self.io.write_text(path, &text)) {
            Ok(()) => {
                let line = plan::lock_setup_written(desktop, keys, path);
                self.post(&line);
                (true, line)
            }
            Err(e) => {
                let line = plan::lock_setup_failed(desktop, path, &e);
                self.warn(&line);
                (true, line)
            }
        }
    }

    fn post(&self, message: &str) {
        self.out.pkg("post", message);
    }

    /// Port of `_post_install_mate`: bring MATE to the Ubuntu MATE look.
    fn post_install_mate(&self, family: PkgFamily, aur_helper: Option<&str>) -> (bool, String) {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let mut errors: Vec<String> = Vec::new();

        // 1. Captions of the Control Center's "Common tasks" in the person's language.
        self.post(&tr("🌐 Fixing the MATE Control Center localization…"));
        let lang = self.io.env("LANG");
        match plan::mate_locale_actions(&lang) {
            Some((key, actions)) => {
                let argv = s(&[
                    "gsettings",
                    "set",
                    "org.mate.control-center",
                    "cc-actions-list",
                    actions,
                ]);
                if self.run_ok(&argv) {
                    self.post(&format!("  {}", trf("✅ “Common tasks” localized (%1)", &[&key])));
                } else {
                    self.post(&format!("  {}", tr("⚠️ gsettings isn't available (will apply when you log in to MATE)")));
                }
            }
            None => self.post(&format!("  {}", trf("ℹ️ Locale %1 — skipping", &[&lang]))),
        }

        // 2. All eight Ubuntu MATE panel layouts.
        self.post(&tr("📐 Loading all 8 Ubuntu MATE layouts…"));
        self.post("   (Familiar, Redmond, Cupertino, Mutiny, Pantheon,");
        self.post("    Contemporary, Netbook, Traditional)");
        if !self.io.which("git") {
            let hint = utils::tool_package_hint("git", family.as_str());
            self.post(format!("  {}", trf("⚠️ git isn't installed, skipping the layouts. %1", &[&hint])).trim_end());
            errors.push(tr("git not found"));
        } else {
            self.mate_layouts(&mut errors);
        }

        // 3. Packages beyond the distribution, BEFORE writing the look (see the
        //    original: otherwise a theme that never got installed went into the defaults).
        for (tag, title, pkgs) in plan::MATE_EXTRA_PACKAGES {
            let pkgs: Vec<String> = pkgs.iter().map(|p| p.to_string()).collect();
            self.ensure_extra_packages(family, aur_helper, &tr(title), &pkgs, &mut errors, tag);
        }

        // 4. The look, only from what is really on disk.
        self.post(&tr("🎨 Setting up the MATE appearance (Yaru-MATE, Ubuntu fonts)…"));
        let look = self.mate_look();
        let mut all_set = true;
        for argv in plan::mate_user_settings_argv(&look) {
            if !self.run_ok(&argv) {
                all_set = false;
            }
        }
        if all_set {
            for line in plan::mate_gschema_success_lines(&look) {
                self.post(&line);
            }
        } else {
            self.post(&format!("  {}", tr("⚠️ Some appearance settings couldn't be written (no MATE schemas?)")));
            errors.push("mate look settings failed".to_string());
        }
        self.drop_legacy_mate_gschema();

        // 5. The lock screen like the desktop wallpaper, not the stock "stripes"
        //    (`wallpaper_engine::mate_lock_follows_desktop_argv`). The person's own
        //    picture stays.
        let get = s(&["gsettings", "get", crate::wallpaper_engine::MATE_SCREENSAVER_SCHEMA, "picture-filename"]);
        let r = self.io.run(&get, None, false);
        if r.rc() == Some(0)
            && crate::wallpaper_engine::mate_lock_picture_is_stock(&r.stdout)
            && self.run_ok(&crate::wallpaper_engine::mate_lock_as_desktop_set_argv())
        {
            self.post(&format!("  {}", plan::mate_lock_follows_desktop_line()));
        }

        let (lines, result) = plan::mate_post_install_summary(errors.len());
        for line in lines {
            self.post(&line);
        }
        (true, result)
    }

    fn mate_layouts(&self, errors: &mut Vec<String>) {
        let layouts_dst = "/usr/share/mate-panel/layouts";
        let repo = match self.io.temp_dir("rexon_mate_settings_", None) {
            Ok(p) => p,
            Err(e) => {
                self.post(&format!("  {}", trf("⚠️ Couldn't create a temporary folder: %1", &[&e])));
                errors.push("mkdtemp failed".to_string());
                return;
            }
        };
        // `git clone` into an existing directory will not work: as in the original,
        // we remove the empty directory before cloning.
        self.io.remove_tree(&repo);
        let clone = [
            "git",
            "clone",
            "--depth",
            "1",
            "https://github.com/ubuntu-mate/ubuntu-mate-settings.git",
        ]
        .iter()
        .map(|x| x.to_string())
        .chain(std::iter::once(repo.clone()))
        .collect::<Vec<_>>();
        if !self.run_ok(&clone) {
            self.post(&format!("  {}", tr("⚠️ Couldn't clone ubuntu-mate-settings (no internet?)")));
            errors.push("git clone failed".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        let src = format!("{repo}/usr/share/mate-panel/layouts");
        if !self.io.is_dir(&src) {
            self.post(&format!("  {}", tr("⚠️ The layouts folder wasn't found")));
            errors.push("layouts dir missing in repo".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        let mut files: Vec<String> = self
            .io
            .list_dir(&src)
            .unwrap_or_default()
            .into_iter()
            .filter(|f| f.ends_with(".layout") || f.ends_with(".dock"))
            .collect();
        files.sort();
        if files.is_empty() {
            self.post(&format!("  {}", tr("⚠️ No layout files found")));
            errors.push("no layout files in repo".to_string());
            self.io.remove_tree(&repo);
            return;
        }
        // The layout helper only accepts directories under /tmp.
        let staged = match self.io.temp_dir("rexon_layouts_stage_", Some("/tmp")) {
            Ok(p) => p,
            Err(e) => {
                self.post(&format!("  {}", trf("⚠️ Couldn't create a temporary folder: %1", &[&e])));
                errors.push("mkdtemp stage failed".to_string());
                self.io.remove_tree(&repo);
                return;
            }
        };
        for f in &files {
            let dst = format!("{staged}/{f}");
            if self.io.copy_file(&format!("{src}/{f}"), &dst).is_err() {
                continue;
            }
            if f.ends_with(".layout") {
                if let Some(text) = self.io.read_text(&dst) {
                    let _ = self
                        .io
                        .write_text(&dst, &plan::mate_layout_fix_snap_paths(&text));
                }
            }
        }
        let r2 = match self.sys.privileged_stage_cmd("mate-layouts", "rexon", &staged) {
            // The window's privilege session (the layout directory as an archive over a
            // descriptor); without it, the old layout helper.
            Some(argv) => {
                let op = PrivOp::Stage {
                    category: "mate-layouts".to_string(),
                    name: "rexon".to_string(),
                    source: crate::privilege::op::LocalFile { slot: 0, label: staged.clone() },
                };
                self.io
                    .run_priv(&op, std::slice::from_ref(&staged), None)
                    .unwrap_or_else(|| self.io.run(&argv, None, false))
            }
            None => {
                let staged_files: Vec<String> = files.iter().map(|f| format!("{staged}/{f}")).collect();
                let script = plan::mate_layouts_stage_script(layouts_dst, &staged_files);
                self.run_root_script("rexon_layouts_", &script)
            }
        };
        self.io.remove_tree(&staged);
        if r2.rc() == Some(0) {
            let n_layout = files.iter().filter(|f| f.ends_with(".layout")).count();
            let n_dock = files.iter().filter(|f| f.ends_with(".dock")).count();
            self.post(&format!(
                "  {}",
                // TRANSLATORS: %1 layout files and %2 dock files were copied.
                trf("✅ Copied: %1 layouts, %2 dock files", &[&n_layout, &n_dock])
            ));
            self.post(&format!("  {}", tr("✅ Snap paths adapted for Arch/CachyOS")));
        } else {
            self.post(&format!(
                "  {}",
                tr("⚠️ Couldn't copy the layouts (administrator password not confirmed?)")
            ));
            errors.push("pkexec copy layouts failed".to_string());
        }
        self.io.remove_tree(&repo);
    }

    /// `pkexec bash <temporary script>`: the script is assembled by the core.
    fn run_root_script(&self, prefix: &str, script: &str) -> RunOut {
        let Ok(tmp) = self.io.temp_file(prefix, ".sh") else {
            return RunOut::missing();
        };
        let r = match self.io.write_file_mode(&tmp, script, 0o755) {
            Ok(()) => self.io.run(
                &["pkexec".to_string(), "bash".to_string(), tmp.clone()],
                None,
                false,
            ),
            Err(_) => RunOut::missing(),
        };
        self.io.remove_file(&tmp);
        r
    }

    /// Port of `_ensure_extra_packages`.
    fn ensure_extra_packages(
        &self,
        family: PkgFamily,
        aur_helper: Option<&str>,
        title: &str,
        packages: &[String],
        errors: &mut Vec<String>,
        tag: &str,
    ) {
        self.post(title);
        if family != PkgFamily::Arch {
            self.post(&plan::extra_packages_texts(tag, &[], None).not_arch);
            return;
        }
        let missing: Vec<String> = packages
            .iter()
            .filter(|p| !self.sys.is_pkg_installed(p))
            .cloned()
            .collect();
        let texts = plan::extra_packages_texts(tag, &missing, aur_helper);
        if missing.is_empty() {
            self.post(&texts.already);
            return;
        }
        if aur_helper.is_none() {
            self.post(&texts.no_helper);
            errors.push(texts.no_helper_error);
            return;
        }
        self.post(&texts.installing);
        // Build progress without a share: the stage stays at its place on the bar (item 2).
        let sink = self.out.clone();
        let forward: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |_stage: &str, msg: &str| sink.emit("aur", msg, None));
        let r = self.run_aur_with_progress(&texts.argv, Some(texts.timeout_secs), forward);
        if r.rc() == Some(0) {
            self.post(&texts.done);
        } else if r.outcome == RunOutcome::Timeout {
            self.post(&texts.failed_timeout);
            errors.push(texts.failed_timeout_error);
        } else {
            self.post(&texts.failed);
            errors.push(texts.failed_error);
        }
    }

    /// Port of `_names_in`: subdirectory names; with `marker`, only those that
    /// contain it.
    fn names_in(&self, dirs: &[&str], marker: Option<&str>) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        for d in dirs {
            let root = self.expand(d);
            let Some(mut entries) = self.io.list_dir(&root) else {
                continue;
            };
            entries.sort();
            for name in entries {
                let path = format!("{root}/{name}");
                if !self.io.is_dir(&path) {
                    continue;
                }
                if let Some(m) = marker {
                    if !self.io.is_dir(&format!("{path}/{m}")) {
                        continue;
                    }
                }
                if !found.contains(&name) {
                    found.push(name);
                }
            }
        }
        found
    }

    /// Port of `_mate_look`.
    fn mate_look(&self) -> plan::MateLook {
        const THEME_DIRS: &[&str] = &["/usr/share/themes", "~/.themes", "~/.local/share/themes"];
        const ICON_DIRS: &[&str] = &["/usr/share/icons", "~/.icons", "~/.local/share/icons"];
        let fonts = self
            .io
            .run(&["fc-list".to_string(), ":family".to_string()], None, false);
        let input = plan::MateLookInput {
            gtk_themes: self.names_in(THEME_DIRS, Some("gtk-3.0")),
            marco_themes: self.names_in(THEME_DIRS, Some("metacity-1")),
            icon_themes: self.names_in(ICON_DIRS, None),
            cursor_themes: self.names_in(ICON_DIRS, Some("cursors")),
            ubuntu_font: fonts.rc() == Some(0) && fonts.stdout.contains("Ubuntu"),
        };
        let look = plan::mate_look(&input);
        for note in &look.notes {
            self.post(note);
        }
        look
    }

    /// Port of `_drop_legacy_mate_gschema`.
    fn drop_legacy_mate_gschema(&self) {
        let path = plan::MATE_GSCHEMA_PATH;
        if !self.io.exists(path) {
            return;
        }
        let r = self.run_root_script("rexon_gschema_", &plan::mate_gschema_cleanup_script());
        if r.rc() == Some(0) {
            self.post(&format!("  {}", tr("✅ Removed the system-wide schema override from older versions")));
        } else {
            self.post(&format!(
                "  {}",
                trf("⚠️ The system-wide schema override from older versions is still there: %1", &[&path])
            ));
        }
    }

    // ── Default application protection ───────────────────────────────────
    //
    // Port of the executing half of `SystemDefaultsProtector`: decisions are in
    // `crate::defaults`, here the `xdg-mime` queries, reading and writing the
    // `~/.config/rexon/*.json` files.

    fn protector_dir(&self) -> String {
        format!("{}/rexon", self.config_home())
    }

    fn read_json(&self, path: &str) -> Option<Value> {
        serde_json::from_str(&self.io.read_text(path)?).ok()
    }

    fn protector_rules(&self) -> dfl::Rules {
        let rules = dfl::Rules::defaults();
        match self.read_json(&format!("{}/protection_rules.json", self.protector_dir())) {
            Some(Value::Object(m)) => rules.updated(&m),
            _ => rules,
        }
    }

    fn protector_snapshot_file(&self) -> String {
        format!("{}/protected_defaults.json", self.protector_dir())
    }

    fn protector_pending_file(&self) -> String {
        format!("{}/pending_choices.json", self.protector_dir())
    }

    fn protector_protected(&self) -> Vec<(String, Option<String>)> {
        match self.read_json(&self.protector_snapshot_file()) {
            Some(v @ Value::Object(_)) => dfl::load_protected_apps(&v),
            _ => Vec::new(),
        }
    }

    fn protector_pending(&self) -> Vec<(String, (String, String))> {
        let Some(Value::Object(m)) = self.read_json(&self.protector_pending_file()) else {
            return Vec::new();
        };
        m.iter()
            .filter_map(|(k, v)| {
                let old = v.get("old")?.as_str()?.to_string();
                let new = v.get("new")?.as_str()?.to_string();
                Some((k.clone(), (old, new)))
            })
            .collect()
    }

    fn xdg_query(&self, mime: &str) -> Option<String> {
        let argv = vec![
            "xdg-mime".to_string(),
            "query".to_string(),
            "default".to_string(),
            mime.to_string(),
        ];
        let r = self.io.run(&argv, None, false);
        if r.rc() != Some(0) {
            return None;
        }
        let v = r.stdout.trim().to_string();
        (!v.is_empty()).then_some(v)
    }

    /// Port of `_get_current`: all application types + the terminal.
    fn defaults_current(&self) -> Vec<(String, Option<String>)> {
        let mut out: Vec<(String, Option<String>)> = dfl::MIME_TYPES
            .iter()
            .map(|(t, mime)| (t.to_string(), self.xdg_query(mime)))
            .collect();
        out.push(("terminal".to_string(), self.current_terminal()));
        out
    }

    /// Port of `_get_current_terminal` (with the same laziness of sources).
    fn current_terminal(&self) -> Option<String> {
        let de = self.io.env("XDG_CURRENT_DESKTOP");
        // `xdg-terminal-exec` reads the own desktop's list before the common one.
        let xdg_list = dfl::effective_terminals_list(
            crate::appdefaults::terminals_list_filename(&de)
                .and_then(|name| self.io.read_text(&format!("{}/{name}", self.config_home()))),
            self.io
                .read_text(&format!("{}/xdg-terminals.list", self.config_home())),
        );
        if let Some(content) = &xdg_list {
            if content.lines().next().is_some_and(|l| !l.trim().is_empty()) {
                if let Some(t) = dfl::get_current_terminal(Some(content), &de, None, None) {
                    return Some(t);
                }
                return self.detect_installed_terminal();
            }
        }
        let de_l = de.to_lowercase();
        let mut kdeglobals = None;
        let mut gsettings_exec = None;
        let gsettings = |schema: &str| {
            let argv = vec![
                "gsettings".to_string(),
                "get".to_string(),
                schema.to_string(),
                "exec".to_string(),
            ];
            let r = self.io.run(&argv, None, false);
            (r.rc() == Some(0)).then_some(r.stdout)
        };
        if de_l.contains("kde") || de_l.contains("plasma") {
            kdeglobals = self
                .io
                .read_text(&format!("{}/kdeglobals", self.config_home()));
        } else if let Some(schema) = dfl::terminal_exec_schema(&de) {
            gsettings_exec = gsettings(schema);
        }
        dfl::get_current_terminal(
            xdg_list.as_deref(),
            &de,
            kdeglobals.as_deref(),
            gsettings_exec.as_deref(),
        )
        .or_else(|| self.detect_installed_terminal())
    }

    /// Port of `detect_installed_terminal`.
    fn detect_installed_terminal(&self) -> Option<String> {
        let available: Vec<String> = dfl::TERMINAL_CANDIDATES
            .iter()
            .filter(|(b, _)| self.io.which(b))
            .map(|(b, _)| b.to_string())
            .collect();
        if available.is_empty() {
            return None;
        }
        let mut dirs = vec![format!("{}/.local/share/applications", self.home())];
        let data_dirs = self
            .env_opt("XDG_DATA_DIRS")
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
        dirs.extend(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(|d| format!("{d}/applications")),
        );
        let wanted: Vec<&str> = dfl::TERMINAL_CANDIDATES
            .iter()
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect();
        let mut present: Vec<String> = Vec::new();
        for d in dirs {
            for name in self.io.list_dir(&d).unwrap_or_default() {
                if wanted.contains(&name.as_str()) && !present.contains(&name) {
                    present.push(name);
                }
            }
        }
        dfl::pick_terminal(&available, &present)
    }

    fn desktop_file_exists(&self, desktop: &str) -> bool {
        self.io.exists(&format!("/usr/share/applications/{desktop}"))
            || self
                .io
                .exists(&format!("{}/.local/share/applications/{desktop}", self.home()))
    }

    /// Port of `_set_default` together with `_set_terminal`.
    fn set_default(&self, app_type: &str, desktop: &str) {
        match dfl::set_default(app_type, desktop) {
            dfl::SetDefault::Terminal(t) => {
                let de = self.io.env("XDG_CURRENT_DESKTOP");
                for step in dfl::set_terminal_plan(&t, &de, &self.home()) {
                    match step {
                        dfl::TermStep::WriteFile { path, content } => {
                            let _ = self.io.write_text(&path, &content);
                        }
                        dfl::TermStep::MakeDirs { path } => {
                            let _ = self.io.make_dirs(&path);
                        }
                        dfl::TermStep::Run { argv } => {
                            let _ = self.io.run(&argv, None, false);
                        }
                        dfl::TermStep::SetIni {
                            path,
                            section,
                            key,
                            value,
                        } => {
                            let existing = self.io.read_text(&path);
                            let text =
                                dfl::set_ini_value(existing.as_deref(), &section, &key, &value);
                            let _ = self.io.write_text(&path, &text);
                        }
                    }
                }
                // `rexon-terminal.conf` went into environment.d: without this `TERMINAL`
                // would reach sway/labwc/Hyprland without uwsm only on the next look change.
                self.io.sync_session_env();
            }
            dfl::SetDefault::Xdg(cmds) => {
                for argv in cmds {
                    let _ = self.io.run(&argv, None, false);
                    // `xdg-mime default` writes only the shared list; our list of the
                    // current desktop is read before it and would hide the choice.
                    if let [_, _, desktop, mime] = argv.as_slice() {
                        self.follow_choice(mime, desktop);
                    }
                }
            }
        }
    }

    /// Carry a just-made choice into Rexon's lists of the current session
    /// ([`dfl::follow_choice_in_desktop_list`]).
    fn follow_choice(&self, mime: &str, desktop: &str) {
        let cfg = self.config_home();
        for name in dfl::session_desktop_lists(&self.io.env("XDG_CURRENT_DESKTOP")) {
            let path = format!("{cfg}/{name}");
            let Some(text) = self.io.read_text(&path) else { continue };
            if let Some(next) = dfl::follow_choice_in_desktop_list(&text, mime, desktop) {
                if self.io.write_text(&path, &next).is_ok() {
                    crate::log_info!("desktops", "{name}: {mime} -> {desktop} (follows the choice)");
                }
            }
        }
    }

    /// Port of `SystemDefaultsProtector.snapshot`. Returns how many applications
    /// are now remembered: window startup writes this number to the log, while
    /// the desktop install only needs the success flag.
    fn defaults_snapshot(&self) -> Result<usize, String> {
        self.io.make_dirs(&self.protector_dir())?;
        let rules = self.protector_rules();
        let protected =
            dfl::compute_snapshot(&self.defaults_current(), &rules, &self.protector_protected());
        let text = dfl::serialize_snapshot(&protected, &self.io.clock().iso);
        self.io.write_text(&self.protector_snapshot_file(), &text)?;
        Ok(protected.len())
    }

    /// First capture of the protection reference, at window startup.
    ///
    /// Whether to capture is decided by a pure core function
    /// ([`dfl::should_snapshot_at_startup`]); the capture itself is the same
    /// [`Self::defaults_snapshot`] as before a desktop install, there is no
    /// second copy. `None`: there was nothing to capture (the reference already
    /// exists or protection is off), and not a single query to the system was made.
    fn defaults_snapshot_if_never(&self) -> Option<Result<usize, String>> {
        let rules = self.protector_rules();
        let remembered = self.protector_protected();
        if !dfl::should_snapshot_at_startup(&rules, &remembered) {
            return None;
        }
        Some(self.defaults_snapshot())
    }

    /// Port of `SystemDefaultsProtector.restore_defaults(de_switched=False)`.
    fn defaults_restore(&self) -> Result<Vec<(String, String)>, String> {
        let rules = self.protector_rules();
        if !rules.enabled() {
            return Ok(Vec::new());
        }
        let current = self.defaults_current();
        let outcome = dfl::restore_defaults(
            &self.protector_protected(),
            &rules,
            &current,
            false,
            &self.protector_pending(),
            &|d| self.desktop_file_exists(d),
        );
        for (app_type, desktop) in &outcome.set_calls {
            self.set_default(app_type, desktop);
        }
        self.io.make_dirs(&self.protector_dir())?;
        self.io.write_text(
            &self.protector_pending_file(),
            &dfl::serialize_pending(&outcome.pending),
        )?;
        Ok(outcome.results)
    }

    /// What a removed desktop leaves behind in the home directory and nobody
    /// reads any more:
    ///   * Rexon's `<desktop>-mimeapps.list` and `<desktop>-xdg-terminals.list`
    ///     of that desktop (only files with a Rexon mark);
    ///   * dangling links in `~/.config/systemd/user/*.wants` — units the
    ///     removed packages took with them (a live case: Deepin's calendar
    ///     timer link kept pointing into nowhere for weeks).
    ///
    /// Our confined autostart copies and the labwc portals file follow their
    /// system files at the next login (`appdefaults_exec`). Silent for the
    /// person: each removal goes to the Rexon log.
    fn remove_desktop_leftovers(&self, de_id: &str) {
        let cfg = self.config_home();
        for name in dfl::desktop_owned_files(de_id) {
            let path = format!("{cfg}/{name}");
            if self.io.read_text(&path).is_some_and(|t| dfl::is_rexon_desktop_file(&t)) {
                self.io.remove_file(&path);
                crate::log_info!("desktops", "{de_id} removed: removed our {path}");
            }
        }
        let units = format!("{cfg}/systemd/user");
        for dir in self.io.list_dir(&units).unwrap_or_default() {
            if !dir.ends_with(".wants") {
                continue;
            }
            let wants = format!("{units}/{dir}");
            for link in self.io.list_dir(&wants).unwrap_or_default() {
                let path = format!("{wants}/{link}");
                // `exists` follows the link: false means its target is gone.
                if !self.io.exists(&path) {
                    self.io.remove_file(&path);
                    crate::log_info!("desktops", "{de_id} removed: removed dangling unit link {path}");
                }
            }
        }
    }

    /// Port of `protect_de_mimeapps` → lines for the progress log.
    fn protect_de_mimeapps(&self, current_de: &str, installed: &[String]) -> Vec<String> {
        let config_dir = self.config_home();
        let outcome = dfl::protect_de_mimeapps(
            current_de,
            installed,
            &config_dir,
            &self.io.clock().iso_seconds,
            &|d| self.desktop_file_exists(d),
            &|p| self.io.read_text(p),
        );
        // To the window, one line with the desktops by name; id, "created/updated",
        // the entry count and the path go to the Rexon log.
        let mut written: Vec<&str> = Vec::new();
        let mut writes = outcome.writes.iter().zip(outcome.valid_counts.iter());
        for (de_id, action) in &outcome.results {
            match action.as_str() {
                "skipped" => crate::log_info!("desktops", "mimeapps {de_id}: no installed applications"),
                "created" | "updated" => {
                    if let Some(((path, content), count)) = writes.next() {
                        let _ = self.io.write_text(path, content);
                        crate::log_info!("desktops", "mimeapps {de_id}: {action} (entries: {count}) -> {path}");
                        let name = system::DE_META
                            .iter()
                            .find(|m| m.id == de_id.as_str())
                            .map(|m| m.display_name)
                            .unwrap_or(de_id.as_str());
                        if !written.contains(&name) {
                            written.push(name);
                        }
                    }
                }
                _ => {}
            }
        }
        // A list older versions wrote under a name no desktop reads
        // (`cinnamon-mimeapps.list`): the right one was just written, the old one
        // is junk. Only Rexon's own file is removed.
        for (old, right) in dfl::MISNAMED_MIMEAPPS {
            let old_path = format!("{config_dir}/{old}");
            let right_path = format!("{config_dir}/{right}");
            match dfl::misnamed_mimeapps_fix(
                self.io.read_text(&old_path).as_deref(),
                self.io.read_text(&right_path).as_deref(),
            ) {
                dfl::MisnamedFix::Nothing => {}
                dfl::MisnamedFix::Move { content } => {
                    if self.io.write_text(&right_path, &content).is_ok() {
                        self.io.remove_file(&old_path);
                        crate::log_info!("desktops", "moved {old_path} -> {right_path}");
                    }
                }
                dfl::MisnamedFix::DropOld => {
                    self.io.remove_file(&old_path);
                    crate::log_info!("desktops", "removed unread {old_path} ({right} exists)");
                }
            }
        }
        if written.is_empty() {
            Vec::new()
        } else {
            vec![format!("  {}", mimeapps_written_message(&written.join(", ")))]
        }
    }
}

// ============================================================================
// Desktop completeness: «Check and install missing»
// ============================================================================
//
// Port of the «Settings» tab button (`rexon/qt_settings/window.py`,
// `_compliance_task`). Python called the core check right away with the
// right to fix, and it installed what was missing under pkexec silently: no
// question, no progress window, one password per package. Here the same
// check ([`comp::Compliance`]) runs in two steps, and the second only after
// «Yes»:
//   1. [`DeOpsExec::completeness_survey`]: a check WITHOUT a password and
//      network (the same mode as at login): what is in place and what is
//      left to install ([`comp::deferred_fixes`]);
//   2. [`DeOpsExec::complete_de`]: the same check with the right to fix.
//      Each repair command (pkexec, the AUR helper, a network download) goes
//      through THIS executor (`DeOpsIo::run`), with progress in the operation
//      window, waiting for someone else's lock and cancellation at the same
//      points as the desktop install. Other check runs (`pacman -Q`,
//      `gsettings`) go through its own executor `ComplianceIo`.
//
// ⭐ WHAT WAS FIXED IN THE PORT (rule #16):
//   1. A password refusal is not repeated once per package: after the first
//      closed password window the remaining repair commands do not run, and
//      the person sees one explanation, not a series of windows.
//   2. Before each install we wait until the package installer is free: a
//      neighbouring transaction's post-hook holds the lock for up to ~30 s,
//      and Python got "failed to install" for no reason.
//   3. What is missing is named in words before the password prompt: Python
//      asked for the password without saying what for.

/// Title of the install-missing progress window.
pub fn completeness_operation_title() -> String {
    tr("Installing What's Missing")
}
/// Title of the question window.
pub fn completeness_confirm_title() -> String {
    tr("Install What's Missing?")
}
/// First progress line: a repeated check before installing.
pub fn completeness_checking_message() -> String {
    tr("🔍 Checking what's missing")
}
/// Result when everything asked about has been installed.
pub fn completeness_done_message() -> String {
    tr("Everything that was missing is installed.")
}

/// Question text: what exactly will be installed and that a password will be needed.
pub fn completeness_confirm_text(labels: &[String]) -> String {
    let list: Vec<String> = labels.iter().map(|l| format!("• {l}")).collect();
    trf(
        // TRANSLATORS: %1 is a bulleted list of missing parts, one per line.
        "Your desktop is missing:\n%1\n\nRexon will install it for you. \
         You'll need the administrator password.",
        &[&list.join("\n")],
    )
}

/// Result when not everything could be installed.
pub fn completeness_partial_message(labels: &[String]) -> String {
    trf("Not everything could be installed. Not installed: %1.", &[&labels.join("; ")])
}

/// Result of a password refusal: the first line is the same as for the
/// desktop install and removal refusals; the button is named with the same
/// core text that is written on it.
pub fn completeness_denied_message() -> String {
    let button = crate::qt_settings::text("compl.button", &[]).unwrap_or_default();
    trf(
        // TRANSLATORS: %1 is the label of the “Check and Install Missing” button.
        "The administrator password wasn't confirmed — the installation was stopped.\n\
         Click “%1” again when you're ready to enter the password.",
        &[&button],
    )
}

/// Progress line when a repair command failed.
fn fix_failed_message(why: &str) -> String {
    trf("Didn't work: %1", &[&why])
}

/// A repair command is something that asks for the administrator password or
/// goes to the network. Everything else the check runs with its own executor.
fn is_fix_command(argv: &[String]) -> bool {
    match argv.first().map(String::as_str) {
        Some("pkexec" | "git") => true,
        Some(prog) => comp::AUR_HELPERS.contains(&prog),
        None => false,
    }
}

/// Packages the command installs (`pacman -S …`, the AUR helper, `… install …`).
fn fix_targets(argv: &[String]) -> Vec<String> {
    let Some(pos) = argv.iter().position(|a| a == "-S" || a == "install") else {
        return Vec::new();
    };
    // AUR helper flags with a value (`--sudo pkexec`, `--answerclean None`):
    // the value is not a package.
    let mut out = Vec::new();
    let mut skip_value = false;
    for a in &argv[pos + 1..] {
        if skip_value {
            skip_value = false;
            continue;
        }
        if a.starts_with('-') {
            skip_value = crate::aur_install::VALUE_FLAGS.contains(&a.as_str());
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// What to say in the progress window about a repair command, without argv and paths.
pub(crate) fn fix_step_message(argv: &[String]) -> String {
    let prog = argv.first().map(String::as_str).unwrap_or("");
    let targets = fix_targets(argv).join(", ");
    if prog == "git" {
        tr("📥 Downloading the missing files from the internet")
    } else if comp::AUR_HELPERS.contains(&prog) {
        trf("📦 Installing from the AUR: %1", &[&targets])
    } else if !targets.is_empty() {
        trf("📦 Installing: %1", &[&targets])
    } else {
        tr("📂 Copying files to the system folder")
    }
}

/// Result of the first step: the check without a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessSurvey {
    /// Recognised desktop; `None`: there were no checks (the desktop was not recognised).
    pub de_id: Option<String>,
    /// "Check → status" in check order.
    pub results: comp::Results,
    /// Check log (for the program log, not for the window).
    pub logs: Vec<String>,
    /// What is left to install: the check key and the caption for the person.
    pub fixes: Vec<(String, String)>,
}

/// Result of the second step: installing what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletenessOutcome {
    pub outcome: DeOpOutcome,
    pub results: comp::Results,
    pub logs: Vec<String>,
}

/// Check executor for the duration of the install: repair commands go
/// through [`DeOpsExec`] (lock, cancellation, progress, pkexec code), the
/// rest goes to the check's executor as is.
struct FixRouter<'r, 'a, C: ComplianceIo, IO: DeOpsIo + ?Sized> {
    exec: &'r DeOpsExec<'a, IO>,
    inner: &'r mut C,
    lock_paths: Vec<String>,
    /// How many repairs were expected after the first step, for the bar.
    expected: usize,
    started: usize,
    /// Why no more repair commands are run.
    stop: Option<Stop>,
}

impl<C: ComplianceIo, IO: DeOpsIo + ?Sized> FixRouter<'_, '_, C, IO> {
    /// `session`: the operation and its files for the window's privilege
    /// session; without it, the old `argv` command (a session refusal is not
    /// repeated with it, Z5).
    fn fix(
        &mut self,
        argv: &[String],
        timeout: Option<u64>,
        session: Option<(&PrivOp, &[String])>,
    ) -> Option<comp::RunResult> {
        let prog = argv.first().map(String::as_str).unwrap_or("");
        // We wait for the lock before each command that may call the package
        // manager: after a neighbouring transaction its post-hooks hold it.
        if prog != "git" && !self.lock_paths.is_empty() {
            match self.exec.wait_pkg_lock(&self.lock_paths, plan::PKG_LOCK_TIMEOUT_SECS) {
                Ok((true, _)) => {}
                Ok((false, message)) => {
                    self.stop = Some(Stop::Failed(message));
                    return None;
                }
                Err(stop) => {
                    self.stop = Some(stop);
                    return None;
                }
            }
        }
        if let Err(stop) = self.exec.point_of_no_return() {
            self.stop = Some(stop);
            return None;
        }
        self.started += 1;
        let fraction = self.started as f64 / (self.expected.max(self.started) + 1) as f64;
        self.exec.emit("install", &fix_step_message(argv), Some(fraction));

        let r = session
            .and_then(|(op, files)| self.exec.io.run_priv(op, files, None))
            .unwrap_or_else(|| self.exec.io.run(argv, timeout, false));
        if prog == "pkexec" {
            if r.outcome == RunOutcome::Error {
                // No password prompt program: nothing more to try with.
                self.stop = Some(Stop::Failed(self.exec.sys.polkit_missing_message()));
                return None;
            }
            if let Some(code) = r.returncode {
                if code != 0 && plan::pkexec_auth_denied(code, &r.stderr) {
                    self.stop = Some(Stop::Failed(completeness_denied_message()));
                    return Some(comp::RunResult {
                        ok: false,
                        stdout: r.stdout,
                    });
                }
            }
        }
        match r.outcome {
            RunOutcome::Ok | RunOutcome::Failed => {
                let ok = r.returncode == Some(0);
                if !ok {
                    self.exec.warn(&fix_failed_message(&run_why(&r)));
                }
                Some(comp::RunResult {
                    ok,
                    stdout: r.stdout,
                })
            }
            RunOutcome::Timeout | RunOutcome::Error => {
                self.exec.warn(&fix_failed_message(&run_why(&r)));
                None
            }
        }
    }
}

impl<C: ComplianceIo, IO: DeOpsIo + ?Sized> ComplianceIo for FixRouter<'_, '_, C, IO> {
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<comp::RunResult> {
        if !is_fix_command(argv) {
            return self.inner.run(argv, timeout);
        }
        if self.stop.is_some() {
            // The password was already not confirmed or the operation was cancelled:
            // we do not show the person the next password window.
            return None;
        }
        self.fix(argv, timeout, None)
    }
    fn run_priv(
        &mut self,
        op: &PrivOp,
        files: &[String],
        argv: &[String],
        timeout: Option<u64>,
    ) -> Option<comp::RunResult> {
        if !is_fix_command(argv) {
            return self.inner.run_priv(op, files, argv, timeout);
        }
        if self.stop.is_some() {
            return None;
        }
        self.fix(argv, timeout, Some((op, files)))
    }
    fn which(&mut self, name: &str) -> bool {
        self.inner.which(name)
    }
    fn env(&mut self, name: &str) -> String {
        self.inner.env(name)
    }
    fn exists(&mut self, path: &str) -> bool {
        self.inner.exists(path)
    }
    fn is_file(&mut self, path: &str) -> bool {
        self.inner.is_file(path)
    }
    fn is_dir(&mut self, path: &str) -> bool {
        self.inner.is_dir(path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        self.inner.read_text(path)
    }
    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>> {
        self.inner.scandir(path)
    }
    fn listdir(&mut self, path: &str) -> Vec<String> {
        self.inner.listdir(path)
    }
    fn makedirs(&mut self, path: &str) {
        self.inner.makedirs(path)
    }
    fn write_text(&mut self, path: &str, content: &str) {
        self.inner.write_text(path, content)
    }
    fn replace(&mut self, src: &str, dst: &str) {
        self.inner.replace(src, dst)
    }
    fn realpath(&mut self, path: &str) -> String {
        self.inner.realpath(path)
    }
    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String {
        self.inner.mkdtemp(prefix, dir)
    }
    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
        self.inner.mkstemp(prefix, suffix)
    }
    fn chmod(&mut self, path: &str, mode: u32) {
        self.inner.chmod(path, mode)
    }
    fn remove_file(&mut self, path: &str) {
        self.inner.remove_file(path)
    }
    fn remove_tree(&mut self, path: &str) {
        self.inner.remove_tree(path)
    }
    fn copy_file(&mut self, src: &str, dst: &str) {
        self.inner.copy_file(src, dst)
    }
}

impl<IO: DeOpsIo + ?Sized> DeOpsExec<'_, IO> {
    /// Run of the current desktop check. The environment and helper presence
    /// come from THIS executor, as for the desktop install.
    fn run_compliance<C: ComplianceIo>(
        &self,
        cio: &mut C,
        allow_privileged: bool,
    ) -> (Option<String>, comp::Results, Vec<String>) {
        let desktop = self.io.env("XDG_CURRENT_DESKTOP");
        let home = self.home();
        let xdg_config = self.env_opt("XDG_CONFIG_HOME");
        let family = self.sys.family();
        let pkg_helper = self.sys.pkg_helper_available();
        let stage_helper = self.sys.stage_helper_available();
        let mut logs = Vec::new();
        let results = comp::Compliance::with_xdg(
            cio,
            &home,
            xdg_config.as_deref(),
            family,
            allow_privileged,
            pkg_helper,
            stage_helper,
        )
        .run_for_current_de(&desktop, &mut logs);
        (comp::detect_current_de(&desktop), results, logs)
    }

    /// Step 1: check without a password and network: what is in place, what is missing.
    pub fn completeness_survey<C: ComplianceIo>(&self, cio: &mut C) -> CompletenessSurvey {
        let (de_id, results, logs) = self.run_compliance(cio, false);
        let fixes = comp::deferred_fixes(&results);
        CompletenessSurvey {
            de_id,
            results,
            logs,
            fixes,
        }
    }

    /// Step 2: installing what is missing. `expected` is what the person was
    /// promised in the question (the result of step 1); it decides whether
    /// everything worked.
    pub fn complete_de<C: ComplianceIo>(
        &self,
        cio: &mut C,
        expected: &[(String, String)],
    ) -> CompletenessOutcome {
        let mut results = Vec::new();
        let mut logs = Vec::new();
        let step = self.complete_de_steps(cio, expected, &mut results, &mut logs);
        CompletenessOutcome {
            outcome: self.finish(step),
            results,
            logs,
        }
    }

    fn complete_de_steps<C: ComplianceIo>(
        &self,
        cio: &mut C,
        expected: &[(String, String)],
        results: &mut comp::Results,
        logs: &mut Vec<String>,
    ) -> Step<String> {
        self.set_cancellable(true);
        self.emit("check", &completeness_checking_message(), None);
        self.checkpoint()?;
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        let mut router = FixRouter {
            exec: self,
            inner: cio,
            lock_paths,
            expected: expected.len(),
            started: 0,
            stop: None,
        };
        let (_, res, lg) = self.run_compliance(&mut router, true);
        let stop = router.stop.take();
        *results = res;
        *logs = lg;
        if let Some(stop) = stop {
            return Err(stop);
        }
        let status = |key: &str| {
            results
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let left: Vec<String> = expected
            .iter()
            .filter(|(key, _)| !matches!(status(key), Some("fixed" | "ok")))
            .map(|(_, label)| label.clone())
            .collect();
        if left.is_empty() {
            Ok(completeness_done_message())
        } else {
            Err(Stop::Failed(completeness_partial_message(&left)))
        }
    }
}

/// Progress sink with nothing to show (questions without a progress window).
struct SilentSink;

impl DeOpSink for SilentSink {
    fn progress(&self, _: &str, _: &str, _: Option<f64>) {}
    fn cancellable(&self, _: bool) {}
}

/// Step 1 on the live machine: check without a password and network.
pub fn completeness_survey_live() -> CompletenessSurvey {
    let io = LiveDeOpsIo::new();
    let mut cio = crate::appdefaults_exec::LiveComplianceIo;
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).completeness_survey(&mut cio)
}

/// Step 2 on the live machine: install with progress in the window.
pub fn complete_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    expected: &[(String, String)],
) -> CompletenessOutcome {
    complete_de_with(&LiveDeOpsIo::new(), sink, cancel, expected)
}

/// Step 2 on the given executor with a live check: the same wiring as
/// [`complete_de_live`]; a test bench passes a live executor with its own lock here.
pub fn complete_de_with<IO: DeOpsIo + ?Sized>(
    io: &IO,
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    expected: &[(String, String)],
) -> CompletenessOutcome {
    let mut cio = crate::appdefaults_exec::LiveComplianceIo;
    DeOpsExec::new(io, sink, cancel).complete_de(&mut cio, expected)
}

// ============================================================================
// The AeroShell look: check and rebuild
// ============================================================================
//
// Port of `check_aeroshell` / `rebuild_aeroshell` (`rexon/system.py`) and of
// the `offer_aeroshell_rebuild` window (`rexon/qt_main/de_progress.py`).
// Decisions are in [`crate::aeroshell`]: whether it diverges, what to build
// with, the command, parsing the result. Here the order and the trips
// outside, with the same executor that installs desktops:
//   1. [`DeOpsExec::aeroshell_survey`]: a check without a password: the
//      package list, KWin and Plasma versions, the look packages' files;
//   2. [`DeOpsExec::rebuild_aeroshell`]: a rebuild ONLY after «Yes» in the
//      window: "what to build with" checks before the first side effect,
//      waiting for the package manager lock (with cancellation), running the
//      builder with progress in the window.
//
// ⭐ WHAT WAS FIXED IN THE PORT (rule #16):
//   1. One can cancel until the build starts, including while waiting for
//      someone else's lock (in Python the progress window held on for up to
//      five minutes).
//   2. A password refusal is called a password refusal, not fifteen lines of
//      builder output, and says what to do.
//   3. A build failure starts with what matters most to the person (the
//      system is intact), and the output tail follows rather than replaces it.

impl<IO: DeOpsIo + ?Sized> DeOpsExec<'_, IO> {
    /// Check of the look against the installed Plasma, without a password and
    /// without network. Not Arch, no pacman or no look packages: an empty result.
    ///
    /// `read_file`: contents of the effect module (a binary file: [`SystemIo`]
    /// reads only text).
    pub fn aeroshell_survey(&self, read_file: &dyn Fn(&str) -> Option<Vec<u8>>) -> aeroshell::Report {
        let family = self.sys.family();
        if family != PkgFamily::Arch {
            return aeroshell::Report::empty();
        }
        let ask = |query: Option<plan::InstalledPkgQuery>| -> Option<RunOut> {
            let q = query?;
            if !self.io.which(&q.binary) {
                return None;
            }
            Some(self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c))
        };
        let installed = ask(plan::installed_packages_query(family))
            .map(|r| plan::parse_installed_packages(r.rc() == Some(0), &r.stdout))
            .unwrap_or_default();
        let forks = plan::filter_plasma_forks(&installed);
        if forks.is_empty() {
            return aeroshell::Report::empty();
        }
        let version = |package: &str| {
            ask(plan::installed_version_query(family, package))
                .and_then(|r| plan::parse_installed_version(family, r.rc() == Some(0), &r.stdout))
        };
        let kwin = version("kwin");
        // The stack version is taken from plasma-workspace: the libplasma fork
        // REPLACES the stock libplasma, and asking it would compare the package
        // with itself.
        let plasma = version("plasma-workspace");
        let file_list = |package: &str| {
            ask(plan::package_files_query(family, package))
                .and_then(|r| plan::package_files_output(r.rc() == Some(0), &r.stdout))
        };
        let qt = version("qt6-base");
        // `ldd -r` on the Qt module: missing libraries and symbols. Read only; no
        // ldd, the step is skipped.
        let has_ldd = self.io.which("ldd");
        let ldd = |path: &str| -> Option<String> {
            if !has_ldd {
                return None;
            }
            let r = self.io.run(&["ldd".to_string(), "-r".to_string(), path.to_string()], Some(15), true);
            (r.outcome != RunOutcome::Timeout && r.outcome != RunOutcome::Error)
                .then(|| format!("{}\n{}", r.stdout, r.stderr))
        };
        aeroshell::check_modules_with(
            &forks,
            kwin.as_deref(),
            plasma.as_deref(),
            qt.as_deref(),
            &file_list,
            read_file,
            &ldd,
        )
    }

    /// Check and advice: what to offer the person. Read only, no password.
    ///
    /// For a lonely fork ([`aeroshell::lonely_libplasma_fork`]) a rollback
    /// preview is made in advance (`pacman -Qi`/`-Si`): if the stock package is
    /// not in the repositories, offering to "restore" makes no sense: the person
    /// gets the refusal right away and in words, not after the password.
    pub fn aeroshell_advice(
        &self,
        read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
        file_mtime: &dyn Fn(&str) -> Option<u64>,
    ) -> (aeroshell::Report, aeroshell::Advice) {
        let mut report = self.aeroshell_survey(read_file);
        // The session journal is a precise confirmation and refinement of the
        // version check (and the only source for third-party modules). For a lonely
        // fork it adds nothing: the advice there is to restore stock Plasma.
        if !report.lonely_fork {
            let findings = self.plugin_journal(read_file, file_mtime);
            aeroshell::apply_journal(&mut report, findings);
        }
        let preview = (report.lonely_fork && !report.restore.is_empty())
            .then(|| self.sys.plasma_fork_reset_preview(Some(&report.restore)));
        // Network only when without it the advice would be "rebuild".
        let upstream = match aeroshell::advise(&report, preview.clone()) {
            aeroshell::Advice::Rebuild { packages } => self.aeroshell_upstream(&report, &packages),
            _ => None,
        };
        let advice = aeroshell::advise_with_upstream(&report, preview, upstream);
        if advice != aeroshell::Advice::Nothing {
            return (report, advice);
        }
        // Everything works now: is a Plasma waiting in the repositories that will
        // break the set (warn BEFORE the update).
        let advice = self.aeroshell_before_update(&report);
        (report, advice)
    }

    /// KWin and Plasma modules that failed to load in this session
    /// ([`crate::plugin_journal`]). Read only, no root: the user journal of the
    /// current boot, `pacman -Qo` / `-Qm`. No journalctl or journal: empty,
    /// silently.
    fn plugin_journal(
        &self,
        read_file: &dyn Fn(&str) -> Option<Vec<u8>>,
        file_mtime: &dyn Fn(&str) -> Option<u64>,
    ) -> Vec<crate::plugin_journal::Finding> {
        use crate::plugin_journal as pj;
        if !self.io.which("journalctl") {
            return Vec::new();
        }
        let mut text = None;
        for grep in [true, false] {
            let r = self.io.run(&pj::journal_argv(grep), Some(pj::JOURNAL_TIMEOUT_SECS), true);
            match pj::journal_answer(grep, r.rc(), &r.stdout, &r.stderr) {
                pj::JournalAnswer::Text => {
                    text = Some(r.stdout);
                    break;
                }
                pj::JournalAnswer::RetryWithoutGrep => continue,
                pj::JournalAnswer::Empty | pj::JournalAnswer::Unavailable => break,
            }
        }
        let Some(text) = text else { return Vec::new() };
        let failures = pj::failures_from_journal(&text);
        if failures.is_empty() {
            return Vec::new();
        }
        let family = self.sys.family();
        let home = self.home();
        let data_home = utils::data_home(self.env_opt("XDG_DATA_HOME").as_deref(), &home);
        let config_home = self.config_home();
        let data_dirs = pj::data_dirs(&self.io.env("XDG_DATA_DIRS"));
        let plugin_dirs = pj::qt_plugin_dirs(&self.io.env("QT_PLUGIN_PATH"));
        let locale = crate::i18n::current_language();
        let owners = |paths: &[String]| {
            let mut map = std::collections::HashMap::new();
            for q in plan::file_owner_queries(family, paths).unwrap_or_default() {
                if !self.io.which(&q.binary) {
                    break;
                }
                let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
                map.extend(crate::installed_scan::parse_qo_output(&r.stdout));
            }
            map
        };
        // Packages not from the repositories: one query for the whole check.
        let foreign_list: std::cell::OnceCell<Vec<String>> = std::cell::OnceCell::new();
        let foreign = |pkg: &str| {
            foreign_list
                .get_or_init(|| {
                    let Some(q) = plan::foreign_packages_query(family) else { return Vec::new() };
                    if !self.io.which(&q.binary) {
                        return Vec::new();
                    }
                    let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
                    r.stdout
                        .lines()
                        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                        .collect()
                })
                .iter()
                .any(|p| p == pkg)
        };
        let list = |dir: &str| {
            self.io
                .list_dir(dir)
                .unwrap_or_default()
                .into_iter()
                .map(|name| format!("{}/{name}", dir.trim_end_matches('/')))
                .collect()
        };
        let read = |path: &str| read_file(path).or_else(|| self.io.read_text(path).map(String::into_bytes));
        let env = pj::ResolveEnv {
            home: &home,
            data_home: &data_home,
            config_home: &config_home,
            data_dirs: &data_dirs,
            plugin_dirs: &plugin_dirs,
            locale: &locale,
            mtime_us: file_mtime,
            read: &read,
            list: &list,
            owners: &owners,
            foreign: &foreign,
        };
        pj::resolve(&failures, &env)
    }

    /// Warning before an update ([`aeroshell::advise_before_update`]). What will
    /// come is from `checkupdates` (its own copy of the database, no
    /// privileges); if it is missing or failed, [`aeroshell::Advice::Nothing`].
    /// Network access to the fork authors only when the repositories have a new
    /// Plasma series.
    fn aeroshell_before_update(&self, report: &aeroshell::Report) -> aeroshell::Advice {
        if report.rebuild.is_empty() {
            return aeroshell::Advice::Nothing;
        }
        let Some(updates) = self.pending_updates() else {
            return aeroshell::Advice::Nothing;
        };
        let new_series = aeroshell::pending_plasma(&updates)
            .is_some_and(|(from, to)| aeroshell::plasma_series(&from) != aeroshell::plasma_series(&to));
        let fork = if new_series { self.aeroshell_fork_version(&report.rebuild) } else { None };
        aeroshell::advise_before_update(report, &updates, fork.as_deref())
    }

    /// What a system update will bring: the parsed `checkupdates` output. None:
    /// not Arch, the program is missing or it failed (no network), an honest
    /// "we do not know". Code 2 from `checkupdates` means "no updates".
    fn pending_updates(&self) -> Option<Vec<aeroshell::PendingUpdate>> {
        crate::update_guard::pending_updates(self.io, self.sys.family())
    }

    /// Whether the libplasma fork authors have caught up with Plasma from the
    /// repositories: `(Plasma version, fork version in the default branch)`.
    /// `None`: no libplasma fork in the set, or it could not be found out
    /// (silently: without network the check is simply skipped). Read only, no
    /// password.
    fn aeroshell_upstream(&self, report: &aeroshell::Report, packages: &[String]) -> Option<(String, String)> {
        let family = self.sys.family();
        packages.iter().find(|p| p.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX))?;
        // The Plasma the system is heading to: the installed one or one already in
        // the repository database (Discover pulls it in too), whichever is newer.
        let repo = plan::repo_version_query(family, "libplasma").and_then(|q| {
            if !self.io.which(&q.binary) {
                return None;
            }
            let r = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
            (r.rc() == Some(0)).then(|| plan::parse_repo_version(family, &r.stdout)).flatten()
        });
        let plasma = aeroshell::newest_version(&[report.plasma_version.clone(), repo])?;
        Some((plasma, self.aeroshell_fork_version(packages)?))
    }

    /// Version of the libplasma fork in its repository's default branch
    /// (`PROJECT_VERSION` of its `CMakeLists.txt`). None: no fork among
    /// `packages`, or it could not be found out (no network, git, curl, foreign
    /// hosting). Read only, no password.
    fn aeroshell_fork_version(&self, packages: &[String]) -> Option<String> {
        let family = self.sys.family();
        let fork = packages
            .iter()
            .find(|p| p.starts_with(aeroshell::LIBPLASMA_FORK_PREFIX))?;
        let project_url = self
            .sys
            .pkg_meta_stdout(fork, family)
            .and_then(|out| crate::system::parse_qi_field(&out, "URL").into_iter().next())?;
        let git_url = aeroshell::upstream_git_url(&project_url)?;
        if !self.io.which("git") || !self.io.which("curl") {
            return None;
        }
        let timeout = Some(aeroshell::UPSTREAM_TIMEOUT_SECS);
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<String>>();
        // No questions into the void: git must not ask for a login for a private
        // or moved repository.
        let head = self.io.run(
            &argv(&["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", &git_url, "HEAD"]),
            timeout,
            true,
        );
        if head.rc() != Some(0) {
            return None;
        }
        let branch = aeroshell::default_branch(&head.stdout)?;
        let raw = aeroshell::raw_file_url(&project_url, &branch, "CMakeLists.txt")?;
        let secs = aeroshell::UPSTREAM_TIMEOUT_SECS.to_string();
        let cmake = self.io.run(&argv(&["curl", "-fsSL", "-m", &secs, &raw]), timeout, true);
        if cmake.rc() != Some(0) {
            return None;
        }
        aeroshell::cmake_project_version(&cmake.stdout)
    }

    /// Restore stock Plasma instead of a fork left without its shell.
    /// ⛔ Called only after «Yes» in the window. The rollback itself is shared
    /// (`SystemExec::remove_plasma_forks`, one password prompt); here, progress
    /// in the window and waiting for someone else's lock with cancellation.
    pub fn restore_stock_plasma(&self, forks: &[String]) -> DeOpOutcome {
        let step = self.restore_stock_plasma_steps(forks);
        self.finish(step)
    }

    fn restore_stock_plasma_steps(&self, forks: &[String]) -> Step<String> {
        self.set_cancellable(true);
        self.emit(aeroshell::REBUILD_START_STAGE, &aeroshell::restore_start_message(), Some(0.05));
        self.checkpoint()?;
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        if !lock_paths.is_empty() {
            let (free, message) = self.wait_pkg_lock(&lock_paths, plan::PKG_LOCK_TIMEOUT_SECS)?;
            if !free {
                return Err(Stop::Failed(message));
            }
        }
        self.point_of_no_return()?;
        let (ok, message) = self.sys.remove_plasma_forks(Some(forks));
        if !ok {
            return Err(Stop::Failed(message));
        }
        self.emit(aeroshell::REBUILD_DONE_STAGE, &aeroshell::restore_done_message(), Some(1.0));
        Ok(message)
    }

    /// Install or remove the background check user units
    /// ([`crate::update_guard::watch_plan`]; old AeroThemePlasma units are
    /// removed). No root: only `~/.config/systemd/user` and `systemctl --user`.
    /// If everything matches, nothing is done and `systemctl` is not called.
    pub fn update_watch(&self, wanted: bool, exec_start: Option<&str>, program: Option<&str>) -> Result<(), String> {
        let plan = crate::update_guard::watch_plan(&self.config_home(), wanted, exec_start, program, &|p| {
            self.io.read_text(p)
        });
        crate::update_guard::apply_watch_plan(self.io, &plan)
    }

    /// Rebuild of the look packages. ⛔ Called only after «Yes» in the window.
    pub fn rebuild_aeroshell(&self, packages: &[String]) -> DeOpOutcome {
        let step = self.rebuild_aeroshell_steps(packages);
        self.finish(step)
    }

    fn rebuild_aeroshell_steps(&self, packages: &[String]) -> Step<String> {
        self.set_cancellable(true);
        // What to build with and what to ask for privileges with, before the first side effect.
        let helper = aeroshell::pick_helper(&|name| self.io.which(name));
        let chroot = helper
            .as_deref()
            .is_some_and(|h| aeroshell::chroot_available(h, &|name| self.io.which(name)));
        let polkit_ok = self.sys.polkit_available();
        let missing = if polkit_ok {
            String::new()
        } else {
            self.sys.polkit_missing_message()
        };
        let rebuild = aeroshell::rebuild_plan(packages, helper.as_deref(), chroot, polkit_ok, &missing);
        if let Some(error) = rebuild.error {
            return Err(Stop::Failed(error));
        }
        let helper = helper.unwrap_or_default();
        self.emit(&rebuild.start_stage, &rebuild.start_message, Some(0.05));
        self.checkpoint()?;

        // Hooks of a neighbouring transaction hold the lock for up to half a
        // minute: otherwise the builder fails for no reason. Waiting can be cancelled.
        let lock_paths = self.io.pkg_lock_paths(self.sys.family());
        if !lock_paths.is_empty() {
            let (free, message) = self.wait_pkg_lock(&lock_paths, plan::PKG_LOCK_TIMEOUT_SECS)?;
            if !free {
                return Err(Stop::Failed(message));
            }
        }
        self.point_of_no_return()?;

        let log = self
            .io
            .temp_file("rexon_aeroshell_", ".log")
            .map_err(|e| Stop::Failed(aeroshell::rebuild_spawn_failure(&helper, &e)))?;
        let out = self.out.clone();
        let emit: Arc<dyn Fn(&str, &str) + Send + Sync> =
            Arc::new(move |stage: &str, message: &str| out.emit(stage, message, None));
        let mut tail = self.io.tail_log(&log, LogTailKind::Build, emit);
        // Timeout by command class (building from source, hours).
        let r = self
            .io
            .run_to_file(&aeroshell::rebuild_argv_c_locale(&rebuild.argv), None, &log);
        let live_tail = tail.as_mut().map(|t| t.stop()).is_some();
        let text = self.io.read_text(&log).unwrap_or_default();
        self.io.remove_file(&log);
        let lines: Vec<String> = text.lines().filter_map(aeroshell::rebuild_line).collect();
        if !live_tail {
            // No reader (a fake executor): progress after the end.
            for line in &lines {
                self.emit(aeroshell::REBUILD_LINE_STAGE, line, None);
            }
        }

        match r.outcome {
            RunOutcome::Error => {
                return Err(Stop::Failed(aeroshell::rebuild_spawn_failure(&helper, &r.stderr)))
            }
            RunOutcome::Timeout => {
                return Err(Stop::Failed(aeroshell::rebuild_timeout_message()))
            }
            RunOutcome::Ok | RunOutcome::Failed => {}
        }
        let code = r.returncode.unwrap_or(-1);
        if code != 0 && aeroshell::rebuild_auth_denied(&text) {
            return Err(Stop::Failed(aeroshell::rebuild_denied_message()));
        }
        let from = lines.len().saturating_sub(aeroshell::REBUILD_TAIL_LINES);
        let outcome = aeroshell::rebuild_outcome(code, &lines[from..]);
        if !outcome.ok {
            return Err(Stop::Failed(aeroshell::rebuild_failure_message(&outcome.message)));
        }
        if let (Some(stage), Some(message)) = (&outcome.log_stage, &outcome.log_message) {
            self.emit(stage, message, Some(1.0));
        }
        Ok(outcome.message)
    }
}

/// Look check on the live machine.
pub fn aeroshell_survey_live() -> aeroshell::Report {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .aeroshell_survey(&|path| std::fs::read(path).ok())
}

/// KWin and Plasma modules that failed to load in this session (the user
/// journal, `pacman -Qo`), for the session watchdog
/// ([`crate::session_watch`]): who is to blame for crashes after login.
pub fn plugin_findings_live() -> Vec<crate::plugin_journal::Finding> {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .plugin_journal(&|path| std::fs::read(path).ok(), &file_mtime_us_live)
}

/// Check and advice on the live machine.
pub fn aeroshell_advice_live() -> (aeroshell::Report, aeroshell::Advice) {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new())
        .aeroshell_advice(&|path| std::fs::read(path).ok(), &file_mtime_us_live)
}

/// File modification time, epoch microseconds; None: no file.
fn file_mtime_us_live(path: &str) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_micros() as u64)
}

/// Background check units on the live machine: needed on Arch (no password
/// required). If there is nothing to start Rexon with, watching is removed
/// (a dead launch line is worse than no watching).
pub fn update_watch_live() -> Result<(), String> {
    let entry = crate::installer::launch_entry_live();
    let exec = crate::app::unit_launch_command(&entry, None, &[crate::update_guard::CHECK_FLAG]);
    let program = match &entry {
        crate::installer::LaunchEntry::MainWindow(path) => Some(path.clone()),
        crate::installer::LaunchEntry::Python => None,
    };
    let io = LiveDeOpsIo::new();
    let ex = DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new());
    let wanted = ex.sys.family() == PkgFamily::Arch && exec.is_some();
    // The Plasma login watchdog goes together with watching: installed while
    // AeroThemePlasma is installed, and removed together with it.
    if let Err(e) = crate::session_guard::sync_env_script_live() {
        crate::log_warn!("session-guard", "сценарий входа: {e}");
    }
    ex.update_watch(wanted, exec.as_deref(), program.as_deref())
}

/// Windowless check (`rexon --aeroshell-check [--login]`), called by the
/// user unit. Wait for the end of the pacman transaction → check (read only)
/// → on trouble a notification with an «Open Rexon» button. The password is
/// never asked here. → process exit code (0: the check ran, whatever it found).
pub fn aeroshell_check_route_live(login: bool, out: &mut dyn FnMut(&str)) -> i32 {
    // The check was turned off in the settings: stay silent here too.
    let saved = crate::settings::SettingsManager::new()
        .ok()
        .and_then(|m| m.get(aeroshell::CHECK_ON_START_SETTING).and_then(Value::as_bool));
    if !aeroshell::check_on_start(saved) {
        out("aeroshell-check: disabled in settings");
        return 0;
    }
    // The trigger fell in the middle of an update: wait for its end: checking
    // a half-updated system means scaring for nothing.
    let started = std::time::Instant::now();
    while std::path::Path::new(aeroshell::PACMAN_DB_LOCK).exists() {
        if started.elapsed().as_secs() > aeroshell::LOCK_WAIT_SECS {
            out("aeroshell-check: package manager still busy, giving up");
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    let (report, advice) = aeroshell_advice_live();
    let fingerprint = aeroshell::notify_fingerprint(&report, &advice);
    out(&format!("aeroshell-check: {fingerprint}"));
    let last = std::fs::read_to_string(aeroshell_notice_stamp_live()).ok();
    let notify = aeroshell::should_notify(&advice, &fingerprint, last.as_deref(), login);
    aeroshell_notice_mark_live(&fingerprint);
    if !notify {
        return 0;
    }
    let Some(notice) = aeroshell::notice_for_report(&advice, &report) else {
        return 0;
    };
    let io = LiveDeOpsIo::new();
    let icon = "rexon";
    if io.which("notify-send") {
        // `--wait` keeps the process until the notification is closed; the pressed
        // button arrives on stdout by its name.
        let argv: Vec<String> = vec![
            "notify-send".into(),
            format!("--app-name={}", utils::NOTIFY_APP_NAME),
            format!("--icon={icon}"),
            "--urgency=critical".into(),
            "--wait".into(),
            format!("--action=open={}", notice.button),
            notice.summary.clone(),
            notice.body.clone(),
        ];
        let r = io.run(&argv, Some(aeroshell::NOTIFY_WAIT_SECS), false);
        if r.stdout.trim() == "open" {
            open_rexon_for_repair(out);
        }
    } else if io.which("dbus-send") {
        // Without notify-send there is no button: the notification service sends
        // the click as a signal, and dbus-send does not catch it. The text still
        // gets through.
        let argv = utils::notify_dbus_argv("dbus-send", &notice.summary, &notice.body, icon, 0);
        let _ = io.run(&argv, Some(10), false);
    }
    0
}

/// File with the fingerprint of the state already reported
/// ([`aeroshell::notify_fingerprint`]). Shared by the notification and the
/// Rexon window: a warning shown by the window is not repeated as a notification.
fn aeroshell_notice_stamp_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let cache = utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref().filter(|v| !v.is_empty()), &home);
    format!("{cache}/rexon/aeroshell-notified")
}

/// Whether this state was already reported (the Rexon window at startup and the notification).
pub fn aeroshell_notice_seen_live(fingerprint: &str) -> bool {
    std::fs::read_to_string(aeroshell_notice_stamp_live()).is_ok_and(|last| last.trim() == fingerprint)
}

/// Remember that this state was reported.
pub fn aeroshell_notice_mark_live(fingerprint: &str) {
    let stamp = aeroshell_notice_stamp_live();
    if let Some(dir) = std::path::Path::new(&stamp).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&stamp, fingerprint);
}

/// Open Rexon right on the check ([`aeroshell::OPEN_FLAG`]) as a separate
/// process, without waiting for it.
fn open_rexon_for_repair(out: &mut dyn FnMut(&str)) {
    if let Err(e) = crate::installer::open_rexon_live(&[aeroshell::OPEN_FLAG]) {
        out(&format!("aeroshell-check: {e}"));
    }
}

/// Restore stock Plasma on the live machine with progress in the window.
pub fn restore_stock_plasma_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    forks: &[String],
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).restore_stock_plasma(forks)
}

/// Rebuild the look on the live machine with progress in the window.
pub fn rebuild_aeroshell_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    packages: &[String],
) -> DeOpOutcome {
    let mut outcome = rebuild_aeroshell_with(&LiveDeOpsIo::new(), sink, cancel, packages);
    // The login watchdog turned the look off until the rebuild: restore the saved one.
    if outcome.ok {
        if let Some(extra) = crate::session_guard::after_rebuild_live() {
            outcome.message = format!("{}\n{extra}", outcome.message);
        }
    }
    outcome
}

/// Rebuild the look on the given executor: the same wiring as
/// [`rebuild_aeroshell_live`]; a test bench passes a live executor with its
/// own lock here.
pub fn rebuild_aeroshell_with<IO: DeOpsIo + ?Sized>(
    io: &IO,
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    packages: &[String],
) -> DeOpOutcome {
    DeOpsExec::new(io, sink, cancel).rebuild_aeroshell(packages)
}

/// Install a desktop on the live machine in one call: what the window uses.
/// `session`: the session chosen in the question dialog (`None`: there was no choice).
pub fn install_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
    session: Option<system::SessionKind>,
) -> DeOpOutcome {
    // SteamOS (Steam Deck and Steam Machine): we do not install packages: the
    // desktop is built as a systemd-sysext image (`crate::steamos_sysext`).
    if dc::running_on_steamos() {
        return crate::steamos_sysext::install_live(sink, cancel, de_id);
    }
    // Deepin only on Treeland: first our own deepin-session build.
    if de_id == crate::deepin_treeland::DE_ID {
        return crate::deepin_treeland::install_from_card_live(sink, cancel, current_de);
    }
    if session.is_some() {
        let io = LiveDeOpsIo::new();
        return DeOpsExec::new(&io, sink, cancel).install_de_session(de_id, current_de, &[], session);
    }
    install_de_packages_live(sink, cancel, de_id, current_de)
}

/// «Add Wayland / X11 variant» to an installed desktop on the live machine.
pub fn add_session_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    kind: system::SessionKind,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).add_session(de_id, kind)
}

/// Install the desktop set with the package manager, without special paths
/// (Deepin on Treeland calls it after its own build).
pub fn install_de_packages_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).install_de(de_id, current_de)
}

/// Install a desktop replacing a conflicting package (`replace`, the
/// person's consent) on the live machine: the replace button on the desktop card.
pub fn install_de_replacing_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
    replace: &[String],
    session: Option<system::SessionKind>,
) -> DeOpOutcome {
    if dc::running_on_steamos() {
        return crate::steamos_sysext::install_live(sink, cancel, de_id);
    }
    // Deepin has no replacements in the table; the path is the same as the install.
    if de_id == crate::deepin_treeland::DE_ID {
        return crate::deepin_treeland::install_from_card_live(sink, cancel, current_de);
    }
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).install_de_session(de_id, current_de, replace, session)
}

/// Remove a desktop on the live machine in one call (the core guard is inside).
pub fn remove_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    de_id: &str,
    current_de: &str,
) -> DeOpOutcome {
    if dc::running_on_steamos() {
        return crate::steamos_sysext::remove_live(sink, cancel, de_id, current_de);
    }
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).remove_de(de_id, current_de)
}

/// Switch desktops on the live machine in one call.
pub fn switch_de_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    target: &str,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).switch_de(target, current_de)
}

/// First capture of the application protection reference on the live machine.
///
/// Called at window startup. `None`: the reference is already captured or
/// protection is off; in that case the system is not asked anything, only
/// two profile files are read.
pub fn startup_defaults_snapshot_live() -> Option<Result<usize, String>> {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).defaults_snapshot_if_never()
}

/// Compatibility verdict on the live machine, for the confirmation dialog.
pub fn install_verdict_live(de_id: &str) -> InstallVerdict {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, Arc::new(SilentSink), CancelToken::new()).install_verdict(de_id)
}

// ============================================================================
// Live executor
// ============================================================================

/// The real executor: file system, subprocesses, clock. Observations and
/// runs go through the same [`crate::system_exec::LiveSystemIo`], there are
/// no commands of its own here.
pub struct LiveDeOpsIo {
    base: crate::system_exec::LiveSystemIo,
    start: std::time::Instant,
    /// Own lock files instead of the system ones (`None`: the system ones by
    /// family, the pacman lock from the real `DBPath`).
    lock_paths: Option<Vec<String>>,
}

impl LiveDeOpsIo {
    pub fn new() -> Self {
        LiveDeOpsIo {
            base: crate::system_exec::LiveSystemIo,
            start: std::time::Instant::now(),
            lock_paths: None,
        }
    }

    /// A live executor waiting for OTHER lock files, for test benches on a fake
    /// package manager. The product calls [`LiveDeOpsIo::new`].
    pub fn with_pkg_lock_paths(paths: Vec<String>) -> Self {
        LiveDeOpsIo {
            lock_paths: Some(paths),
            ..Self::new()
        }
    }

    fn path_env() -> Option<String> {
        std::env::var("PATH").ok()
    }

    fn unique_name(prefix: &str, suffix: &str) -> String {
        use std::sync::atomic::AtomicU64;
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{prefix}{}_{nanos:x}_{n}{suffix}", std::process::id())
    }
}

impl Default for LiveDeOpsIo {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemIo for LiveDeOpsIo {
    fn run_priv(&self, op: &PrivOp, files: &[String], log_path: Option<&str>) -> Option<RunOut> {
        crate::privilege::run::run_live(op, files, log_path, &mut |_| {})
    }

    fn exists(&self, path: &str) -> bool {
        self.base.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.base.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.base.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.base.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.base.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.base.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.base.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.base.which(name)
    }
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        self.base.run(argv, timeout_secs, lang_c)
    }
    fn env(&self, name: &str) -> String {
        self.base.env(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.base.which_path(name)
    }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.base.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.base.copy_file(src, dst)
    }
}

impl DeOpsIo for LiveDeOpsIo {
    fn sync_session_env(&self) {
        let _ = crate::compositor_autostart::sync_session_env_live();
    }

    fn pkg_lock_paths(&self, family: PkgFamily) -> Vec<String> {
        match &self.lock_paths {
            Some(paths) => paths.clone(),
            // The pacman lock comes from the real `DBPath`, not a hardcoded path.
            None => plan::lock_paths_live(family),
        }
    }

    fn run_to_file(&self, argv: &[String], timeout_secs: Option<u64>, log_path: &str) -> RunOut {
        if argv.is_empty() {
            return RunOut::missing();
        }
        let spec = utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::File(log_path.to_string()),
            timeout_secs,
            stderr_to_stdout: true,
            ..utils::RunSpec::default()
        };
        let report = utils::run(&spec, Self::path_env().as_deref());
        RunOut {
            outcome: report.outcome,
            returncode: report.returncode,
            stdout: String::new(),
            stderr: report.error,
        }
    }

    fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = std::env::temp_dir();
        for _ in 0..16 {
            let path = dir.join(Self::unique_name(prefix, suffix));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(_) => return Ok(path.to_string_lossy().into_owned()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(tr("no free file name found"))
    }

    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        use std::os::unix::fs::DirBuilderExt;
        let dir = parent
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        for _ in 0..16 {
            let path = dir.join(Self::unique_name(prefix, ""));
            match std::fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(path.to_string_lossy().into_owned()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err(tr("no free file name found"))
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, content).map_err(|e| e.to_string())?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())
    }

    fn make_dirs(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn remove_file(&self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn remove_tree(&self, path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn elapsed_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn sleep_secs(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn clock(&self) -> Clock {
        let now = crate::qt_app::Timestamp::now_local().unwrap_or_default();
        let mut seconds = now.clone();
        seconds.microsecond = 0;
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Clock {
            iso: now.isoformat(),
            iso_seconds: seconds.isoformat(),
            epoch_ms,
        }
    }

    fn tail_log(
        &self,
        path: &str,
        kind: LogTailKind,
        emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
    ) -> Option<plan::LogTail> {
        Some(plan::tail_log(path, kind, move |s: &str, m: &str| emit(s, m)))
    }

    fn panel_snapshots(&self, reason: &str) -> Vec<(String, Result<(), String>)> {
        let mut out = Vec::new();
        for meta in system::DE_META {
            let bins = system::de_installed_binaries(meta.id);
            if bins.is_empty() || !bins.iter().all(|b| system::binary_in_path(b)) || !backup::panel_supported_live(meta.id) {
                continue;
            }
            let outcome = backup::panel_save_live(meta.id, reason).map(|_| ());
            out.push((meta.id.to_string(), outcome));
        }
        if out.iter().any(|(_, o)| o.is_ok()) {
            let bridge = crate::rexon_bridge::RexonBridge::new_default();
            let clock = self.clock();
            let _ = bridge
                .ensure_dirs()
                .and_then(|_| bridge.notify_backup_created(&clock.iso, clock.epoch_ms));
            bridge.cleanup_commands(300.0, clock.epoch_ms as f64 / 1000.0);
        }
        out
    }

    fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>) {
        use crate::appdefaults_exec::{AppDefaultsEnv, DeAppSwitcher, LiveAppDefaultsIo};
        let mut logs = Vec::new();
        let ok = DeAppSwitcher::new(&LiveAppDefaultsIo, AppDefaultsEnv::from_process())
            .on_switch_de(de_id, &mut logs);
        (ok, logs)
    }

    fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String> {
        use std::path::Path;
        let home = self.env("HOME");
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
        let root = format!("{}/rexon/backups", utils::data_home(xdg_data.as_deref(), &home));
        // Moving from the old location, like `BackupManager.__init__`.
        let old = format!("{home}/.rexon-backups");
        if Path::new(&old).is_dir() && !Path::new(&root).is_dir() {
            if let Some(parent) = Path::new(&root).parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&old, &root).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let meta_file = format!("{root}/metadata.json");
        let mut metadata = std::fs::read_to_string(&meta_file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        let stamp = crate::qt_app::Timestamp::now_local()
            .unwrap_or_default()
            .file_stamp();
        let name = backup::backup_name(de_id, &stamp);
        let path = format!("{root}/{name}");
        std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        let present: Vec<String> = paths
            .iter()
            .filter(|p| Path::new(p.as_str()).exists())
            .cloned()
            .collect();
        let mut saved = Vec::new();
        for (src, dst) in backup::snapshot_items(&present, &path, &home) {
            backup::copy_config_into(&src, &dst).map_err(|e| format!("{src}: {e}"))?;
            saved.push(src);
        }
        metadata.insert(name.clone(), backup::de_meta_entry(de_id, &stamp, &saved, &path));
        let text =
            serde_json::to_string_pretty(&Value::Object(metadata)).map_err(|e| e.to_string())?;
        // The core's shared door: half of the inventory is copies of desktop
        // settings that are on disk but could no longer be restored otherwise.
        crate::utils::atomic_write(&meta_file, &text).map_err(|e| e.to_string())?;
        Ok((name, saved.len()))
    }

    fn bridge_notify(&self, event: &BridgeEvent) {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        let clock = self.clock();
        let sent = bridge.ensure_dirs().and_then(|_| match event {
            BridgeEvent::DeInstalled(de) => {
                bridge.notify_de_installed(de, &clock.iso, clock.epoch_ms)
            }
            BridgeEvent::DeSwitch { from, to } => {
                bridge.notify_de_switch(from, to, &clock.iso, clock.epoch_ms)
            }
            BridgeEvent::DeRemoved(de) => {
                bridge.notify_de_removed(de, &clock.iso, clock.epoch_ms)
            }
        });
        if let Err(e) = sent {
            crate::log_warn!("bridge", "мост BonziBuddy: событие не отправлено — {e}");
        }
        bridge.cleanup_commands(300.0, clock.epoch_ms as f64 / 1000.0);
    }
}

// ============================================================================
// Checks on a fake executor
// ============================================================================

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    pub(crate) type Responder = Box<dyn Fn(&[String]) -> RunOut>;

    pub(crate) fn out(rc: i32, stdout: &str, stderr: &str) -> RunOut {
        RunOut {
            outcome: if rc == 0 {
                RunOutcome::Ok
            } else {
                RunOutcome::Failed
            },
            returncode: Some(rc),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    /// Fake executor: an in-memory file system, a call log, command answers from
    /// a scenario closure. The live machine is not touched at all.
    pub(crate) struct FakeIo {
        pub calls: RefCell<Vec<String>>,
        pub files: RefCell<HashMap<String, String>>,
        pub dirs: RefCell<HashSet<String>>,
        pub bins: HashSet<String>,
        pub env: HashMap<String, String>,
        pub responder: Responder,
        pub clock: Cell<f64>,
        pub temp_n: Cell<u32>,
        pub cancel_on_sleep: Option<CancelToken>,
        /// There is a window privilege session: its helper's answer to any operation.
        pub session: Option<RunOut>,
        /// Operations that went to the session helper.
        pub priv_ops: RefCell<Vec<PrivOp>>,
    }

    impl FakeIo {
        pub(crate) fn arch(responder: Responder) -> Self {
            let mut files = HashMap::new();
            files.insert(
                "/etc/os-release".to_string(),
                "ID=arch\nNAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\n".to_string(),
            );
            let bins = ["pacman", "pkexec", "systemctl", "xdg-mime", "pgrep"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut env = HashMap::new();
            env.insert("HOME".to_string(), "/home/t".to_string());
            env.insert("LANG".to_string(), "ru_RU.UTF-8".to_string());
            FakeIo {
                calls: RefCell::new(Vec::new()),
                files: RefCell::new(files),
                dirs: RefCell::new(HashSet::new()),
                bins,
                env,
                responder,
                clock: Cell::new(0.0),
                temp_n: Cell::new(0),
                cancel_on_sleep: None,
                session: None,
                priv_ops: RefCell::new(Vec::new()),
            }
        }

        fn note(&self, s: String) {
            self.calls.borrow_mut().push(s);
        }

        pub(crate) fn runs(&self) -> Vec<String> {
            self.calls
                .borrow()
                .iter()
                .filter_map(|c| c.strip_prefix("run: ").map(str::to_string))
                .collect()
        }

        pub(crate) fn index_of(&self, needle: &str) -> Option<usize> {
            self.calls.borrow().iter().position(|c| c.contains(needle))
        }
    }

    impl SystemIo for FakeIo {
        fn run_priv(&self, op: &PrivOp, _files: &[String], log: Option<&str>) -> Option<RunOut> {
            let r = self.session.clone()?;
            self.note(format!("priv: {}", serde_json::to_string(op).unwrap_or_default()));
            if let Some(log) = log {
                self.note(format!("priv_log: {log}"));
            }
            self.priv_ops.borrow_mut().push(op.clone());
            Some(r)
        }
        fn exists(&self, path: &str) -> bool {
            self.files.borrow().contains_key(path) || self.dirs.borrow().contains(path)
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.borrow().contains_key(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.borrow().contains(path)
        }
        fn is_executable(&self, _path: &str) -> bool {
            false
        }
        fn realpath(&self, _path: &str) -> Option<String> {
            None
        }
        fn read_text(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn list_dir(&self, _dir: &str) -> Option<Vec<String>> {
            None
        }
        fn which(&self, name: &str) -> bool {
            self.bins.contains(name)
        }
        fn run(&self, argv: &[String], _timeout: Option<u64>, _lang_c: bool) -> RunOut {
            self.note(format!("run: {}", argv.join(" ")));
            (self.responder)(argv)
        }
        fn env(&self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn which_path(&self, _name: &str) -> Option<String> {
            None
        }
        fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
            self.note(format!("write: {path}"));
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
            let text = self.read_text(src).ok_or("нет файла")?;
            self.write_text(dst, &text)
        }
    }

    impl DeOpsIo for FakeIo {
        fn run_to_file(&self, argv: &[String], timeout: Option<u64>, log: &str) -> RunOut {
            let r = self.run(argv, timeout, false);
            // The builder output goes to its log, as with the live executor. Only for
            // the look rebuild log: other checks expect an empty file.
            if log.starts_with("/tmp/rexon_aeroshell_") {
                self.files
                    .borrow_mut()
                    .insert(log.to_string(), format!("{}{}", r.stdout, r.stderr));
            }
            r
        }
        fn temp_file(&self, prefix: &str, suffix: &str) -> Result<String, String> {
            let n = self.temp_n.get() + 1;
            self.temp_n.set(n);
            let p = format!("/tmp/{prefix}{n}{suffix}");
            self.files.borrow_mut().insert(p.clone(), String::new());
            Ok(p)
        }
        fn temp_dir(&self, prefix: &str, _parent: Option<&str>) -> Result<String, String> {
            let n = self.temp_n.get() + 1;
            self.temp_n.set(n);
            let p = format!("/tmp/{prefix}{n}");
            self.dirs.borrow_mut().insert(p.clone());
            Ok(p)
        }
        fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
            self.note(format!("write_mode: {path} {mode:o}"));
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        fn make_dirs(&self, path: &str) -> Result<(), String> {
            self.dirs.borrow_mut().insert(path.to_string());
            Ok(())
        }
        fn remove_file(&self, path: &str) {
            self.note(format!("remove: {path}"));
            self.files.borrow_mut().remove(path);
        }
        fn remove_tree(&self, path: &str) {
            self.dirs.borrow_mut().remove(path);
        }
        fn elapsed_secs(&self) -> f64 {
            self.clock.get()
        }
        fn sleep_secs(&self, secs: u64) {
            self.note(format!("sleep: {secs}"));
            self.clock.set(self.clock.get() + secs as f64);
            if let Some(token) = &self.cancel_on_sleep {
                token.cancel();
            }
        }
        fn clock(&self) -> Clock {
            Clock {
                iso: "2026-09-12T10:00:00.5".to_string(),
                iso_seconds: "2026-09-12T10:00:00".to_string(),
                epoch_ms: 1,
            }
        }
        fn tail_log(
            &self,
            _path: &str,
            _kind: LogTailKind,
            _emit: Arc<dyn Fn(&str, &str) + Send + Sync>,
        ) -> Option<plan::LogTail> {
            None
        }
        fn panel_snapshots(&self, reason: &str) -> Vec<(String, Result<(), String>)> {
            self.note(format!("panel_snapshots: {reason}"));
            let mut out = vec![("kde".to_string(), Ok(()))];
            if self.env.contains_key("FAKE_PANEL_COPY_FAILS") {
                out.push(("mate".to_string(), Err("disk full".to_string())));
            }
            out
        }
        fn app_defaults_switch(&self, de_id: &str) -> (bool, Vec<String>) {
            self.note(format!("app_defaults: {de_id}"));
            (true, vec!["  ✅ профиль".to_string()])
        }
        fn bridge_notify(&self, event: &BridgeEvent) {
            self.note(format!("bridge: {event:?}"));
        }
        fn sync_session_env(&self) {
            self.note("sync_session_env".to_string());
        }
        fn de_config_backup(&self, de_id: &str, paths: &[String]) -> Result<(String, usize), String> {
            self.note(format!("config_backup: {de_id} {}", paths.len()));
            Ok((format!("{de_id}_20260912_100000"), paths.len()))
        }
    }

    /// Fake progress sink.
    #[derive(Default)]
    pub(crate) struct Recorder {
        pub events: Mutex<Vec<(String, String, Option<f64>)>>,
        pub cancellable: Mutex<Vec<bool>>,
    }

    impl DeOpSink for Recorder {
        fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
            self.events
                .lock()
                .unwrap()
                .push((stage.to_string(), message.to_string(), fraction));
        }
        fn cancellable(&self, allowed: bool) {
            self.cancellable.lock().unwrap().push(allowed);
        }
    }

    impl Recorder {
        pub(crate) fn stages(&self) -> Vec<String> {
            self.events.lock().unwrap().iter().map(|e| e.0.clone()).collect()
        }
        pub(crate) fn count(&self, stage: &str) -> usize {
            self.stages().iter().filter(|s| *s == stage).count()
        }
        pub(crate) fn messages(&self, stage: &str) -> Vec<String> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.0 == stage)
                .map(|e| e.1.clone())
                .collect()
        }
    }

    /// Answers of a "healthy machine": login screen enabled, packages not
    /// installed, password accepted.
    pub(crate) fn healthy(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["systemctl", "is-enabled", unit] if *unit == "sddm.service" => out(0, "enabled\n", ""),
            ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
            ["pacman", "-Q", ..] => out(1, "", "error: package was not found"),
            ["xdg-mime", "query", "default", _] => out(0, "org.kde.dolphin.desktop\n", ""),
            _ => out(0, "", ""),
        }
    }

    fn run_install(io: &FakeIo, de: &str, cancel: CancelToken) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).install_de(de, "kde");
        (outcome, rec)
    }

    /// Panel copies of all installed desktops are taken before the package
    /// manager, from any session (not only Plasma), and a failed copy is a
    /// warning that does not stop the installation.
    #[test]
    fn install_copies_every_panel_and_survives_a_failed_copy() {
        let mut io = FakeIo::arch(Box::new(healthy));
        io.env.insert("XDG_CURRENT_DESKTOP".into(), "MATE".into());
        io.env.insert("FAKE_PANEL_COPY_FAILS".into(), "1".into());
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let copies = io.index_of("panel_snapshots: before_install").expect("copies taken");
        let run = io.index_of("run: pkexec bash").unwrap();
        assert!(copies < run, "copies come before the package manager");
        let warnings = rec.messages(de_progress::STAGE_WARNING);
        assert!(warnings.iter().any(|w| w.contains("MATE") && w.contains("disk full")), "{warnings:#?}");
        assert_eq!(rec.messages("panel_backup"), vec![panel_backup_message()]);
    }

    /// Success: the lock is checked BEFORE the password, one main step, temporary
    /// files removed, login screen untouched (it already exists), result "done".
    #[test]
    fn install_success_runs_plan_in_order() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let main: Vec<_> = runs.iter().filter(|r| r.starts_with("pkexec bash /tmp/rexon_install_")).collect();
        assert_eq!(main.len(), 1, "{runs:#?}");
        // The lock is checked before the password: no sleep steps (no lock), but
        // the script is written before the run.
        let write = io.index_of("write_mode: /tmp/rexon_install_1.sh").unwrap();
        let run = io.index_of("run: pkexec bash").unwrap();
        assert!(write < run);
        assert!(io.index_of("remove: /tmp/rexon_install_1.sh").unwrap() > run);
        assert!(io.index_of("remove: /tmp/rexon_install_2.log").unwrap() > run);
        // The login screen is already enabled: no SDDM install, no systemctl enable.
        assert!(
            !runs.iter().any(|r| r.contains("systemctl enable") || r.contains("-S sddm")),
            "{runs:#?}"
        );
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        // Cancellation was available at the start and disabled by the main step.
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        // The defaults snapshot is written before the package manager.
        assert!(io.index_of("write: /home/t/.config/rexon/protected_defaults.json").unwrap() < run);
        assert!(io.index_of("bridge: DeInstalled(\"xfce\")").is_some());
    }

    /// What is marked as AUR in the database but is in the machine's
    /// repositories (CachyOS: Mint themes and icons in `cachyos`) goes into the
    /// common pacman transaction; what is not in the repositories stays AUR.
    /// `pacman -Si` decides, not the table.
    #[test]
    fn install_moves_repo_available_aur_packages_to_pacman() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let in_repo = ["mint-themes", "mint-x-icons", "mint-y-icons"];
        let io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Si", p] if in_repo.contains(p) => {
                    out(0, &format!("Repository      : cachyos\nName            : {p}\n"), "")
                }
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "cinnamon-mint", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let main = system::get_packages("cinnamon-mint", PkgFamily::Arch).len();
        let aur = system::get_aur_packages("cinnamon-mint", PkgFamily::Arch).len();
        assert_eq!(rec.messages("install").first(), Some(&install_intro(main + 3, aur - 3)), "{:#?}", rec.events);
        let runs = io.runs();
        for p in in_repo {
            assert!(runs.iter().any(|r| r == &format!("pacman -Si {p}")), "{runs:#?}");
        }

        // None was found: everything stays in the AUR, as in the table.
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "cinnamon-mint", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(rec.messages("install").first(), Some(&install_intro(main, aur)));
    }

    /// Password refusal (pkexec 126): the person sees the explanation ONCE, the
    /// install does not go on: no default applications, no login screen.
    #[test]
    fn install_pkexec_denied_stops_with_one_error() {
        // Texts come from the translation catalog; the source language (Russian is
        // checked by the core's checks on po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Error executing command as another user: Request dismissed")
            } else {
                healthy(argv)
            }
        }));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert!(!outcome.cancelled);
        assert!(outcome.message.contains("The administrator password wasn't confirmed"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
        assert!(io.index_of("app_defaults:").is_none());
        assert_eq!(io.runs().iter().filter(|r| r.starts_with("pkexec")).count(), 1);
        // Temporary files are removed on refusal too.
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// "Is a password window running?" is asked of polkit itself, not by process
    /// names: the old `pgrep` pattern always matched the system service
    /// polkitd, and the answer was "yes" even on bare sway.
    #[test]
    fn denied_password_asks_polkit_whether_agent_runs() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for (agent_answer, expected) in [
            (out(0, "()\n", ""), "The service that shows the password window isn't running."),
            (
                out(
                    1,
                    "",
                    "Error: GDBus.Error:org.freedesktop.PolicyKit1.Error.Failed: \
                     An authentication agent already exists for the given subject\n",
                ),
                "The administrator password wasn't confirmed",
            ),
        ] {
            let mut io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
                "pkexec" => out(126, "", "Error executing command as another user: Not authorized"),
                "gdbus" => agent_answer.clone(),
                _ => healthy(argv),
            }));
            io.env.insert("XDG_SESSION_ID".to_string(), "2".to_string());
            let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
            assert!(outcome.message.starts_with(expected), "{}", outcome.message);
            let probe = io.runs().into_iter().find(|r| r.starts_with("gdbus")).expect("polkit не спросили");
            assert!(probe.contains("RegisterAuthenticationAgent"), "{probe}");
            assert!(probe.contains("<'2'>"), "{probe}");
            assert!(!io.runs().iter().any(|r| r.starts_with("pgrep")));
        }
    }

    /// No password window: the hint names the agent that is really on this
    /// machine (polkit-gnome has its own path in each distribution), not a path
    /// guessed for everyone.
    #[test]
    fn missing_agent_hint_names_agent_found_here() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let no_agent = "Error executing command as another user: No authentication agent found.\n";
        let io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
            "pkexec" => out(127, "", no_agent),
            _ => healthy(argv),
        }));
        let agent = "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1";
        io.files.borrow_mut().insert(agent.to_string(), String::new());
        let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.message.contains(&format!("\n  {agent} &\n")), "{}", outcome.message);
        assert!(!outcome.message.contains("KDE:"), "{}", outcome.message);

        // Not a single agent on the machine: general advice, without an invented path.
        let io = FakeIo::arch(Box::new(move |argv: &[String]| match argv[0].as_str() {
            "pkexec" => out(127, "", no_agent),
            _ => healthy(argv),
        }));
        let (outcome, _) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.message.contains("polkit-gnome"), "{}", outcome.message);
        assert!(!outcome.message.contains("/usr/lib"), "{}", outcome.message);
    }

    /// A busy lock that never got released: the password is NOT asked, the
    /// result is the core's text about a stuck transaction.
    #[test]
    fn install_lock_never_freed_does_not_ask_password() {
        let io = FakeIo::arch(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(outcome.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1, "ожидание объявлено один раз");
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// Main step failure (the transaction did not go through): the package
    /// manager's text reaches the person, the following steps do not run.
    #[test]
    fn install_main_step_failure_reports_manager_error() {
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(1, "", "error: failed to commit transaction (conflicting files)")
            } else {
                healthy(argv)
            }
        }));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("failed to commit transaction"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(!rec.stages().contains(&"done".to_string()));
    }

    /// A conflict that still reached the real transaction (recorded pacman
    /// output of 19.10.2026): the person sees an explanation, not pacman's raw
    /// English text.
    #[test]
    fn install_conflict_from_pacman_is_explained() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(
                    1,
                    "",
                    ":: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict. Remove kwin? [y/N] \n\
                     error: unresolvable package conflicts detected\n\
                     error: failed to prepare transaction (conflicting dependencies)\n\
                     :: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict\n",
                )
            } else {
                healthy(argv)
            }
        }));
        let (outcome, _rec) = run_install(&io, "pantheon", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(
            outcome.message,
            plan::conflict_explanation(&[plan::PkgConflict {
                new_package: "deepin-kwin".into(),
                installed: "kwin".into(),
                ..plan::PkgConflict::default()
            }])
        );
        assert!(outcome.message.contains("KDE Plasma"), "{}", outcome.message);
    }

    /// A conflict found by the probe BEFORE the password: refusal with an
    /// explanation, pkexec does not run at all.
    #[test]
    fn install_refuses_conflicting_set_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sp", ..] => out(0, "deepin-kwin\nlightdm\n", ""),
                ["pacman", "-Qq"] => out(0, "glibc\nkwin\n", ""),
                ["pacman", "-Si", ..] => out(0, "Name            : deepin-kwin\nConflicts With  : kwin  kwin-x11\n", ""),
                ["pacman", "-Qi"] => out(0, "Name            : kwin\nConflicts With  : None\n", ""),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "pantheon", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("deepin-kwin") && outcome.message.contains("KDE Plasma"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Probes of the River set on a machine with River 0.5 from extra.
    fn river_over_05(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["pacman", "-Sp", ..] => out(0, "river-classic\nwaybar\n", ""),
            ["pacman", "-Qq"] => out(0, "glibc\nriver\n", ""),
            ["pacman", "-Si", ..] => out(
                0,
                "Name            : river-classic\nVersion         : 0.3.17-2.1\nConflicts With  : river\n",
                "",
            ),
            ["pacman", "-Qi"] => out(
                0,
                "Name            : river\nVersion         : 0.5.0-1.1\nRequired By     : None\n\
                 Conflicts With  : river-classic\n",
                "",
            ),
            _ => healthy(argv),
        }
    }

    /// River 0.5 is installed, no consent to replace: refusal before the
    /// password with a text about the River branches, not a generic "conflicts
    /// with an installed package".
    #[test]
    fn install_river_over_05_refuses_without_consent() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(river_over_05));
        let (outcome, rec) = run_install(&io, "river", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.contains("River 0.5") && outcome.message.contains("river-classic"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// With consent to replace river the install goes ahead: one pkexec via the
    /// wrapper (not the helper), the River starter init is put in place.
    #[test]
    fn install_river_over_05_replaces_with_consent() {
        let io = FakeIo::arch(Box::new(river_over_05));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "river",
            "kde",
            &["river".to_string()],
        );
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let main: Vec<_> = runs.iter().filter(|r| r.starts_with("pkexec bash /tmp/rexon_install_")).collect();
        assert_eq!(main.len(), 1, "{runs:#?}");
        assert!(io.files.borrow().contains_key("/home/t/.config/river/init"));
    }

    /// Consent to replace does not resolve a foreign conflict: deepin-kwin next
    /// to KDE Plasma and "consent" to replace kwin is the old refusal before the password.
    #[test]
    fn consent_does_not_lift_a_foreign_conflict() {
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sp", ..] => out(0, "deepin-kwin\nlightdm\n", ""),
                ["pacman", "-Qq"] => out(0, "glibc\nkwin\n", ""),
                ["pacman", "-Si", ..] => out(0, "Name            : deepin-kwin\nConflicts With  : kwin  kwin-x11\n", ""),
                ["pacman", "-Qi"] => out(0, "Name            : kwin\nConflicts With  : None\n", ""),
                _ => healthy(argv),
            }
        }));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "pantheon",
            "kde",
            &["kwin".to_string()],
        );
        assert!(!outcome.ok);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
    }

    /// Answers of a machine with (or without) our own deepin-session build for
    /// Treeland installed.
    fn deepin_machine(built: bool) -> impl Fn(&[String]) -> RunOut {
        move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "deepin-session-treeland" | "deepin-session"] if built => {
                    out(0, "deepin-session-treeland 2.0.33-1.1\n", "")
                }
                ["pacman", "-Sg", "deepin"] => out(0, "deepin ddm\ndeepin deepin-session\ndeepin treeland\n", ""),
                ["pacman", "-Sp", ..] => out(0, "ddm\ntreeland\n", ""),
                _ => healthy(argv),
            }
        }
    }

    /// Deepin without our own build: the executor does not install the whole
    /// group (with deepin-session and deepin-kwin): refusal before the password.
    #[test]
    fn deepin_without_own_build_is_refused_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(deepin_machine(false)));
        let (outcome, rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(!outcome.ok);
        assert!(outcome.message.starts_with("Rexon supports Deepin only on Treeland"), "{}", outcome.message);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec") || r.contains("-Sp")), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Our own build is installed: the variant set is installed: group members
    /// without deepin-session, and no command names deepin-kwin.
    #[test]
    fn deepin_with_own_build_installs_the_treeland_set() {
        let io = FakeIo::arch(Box::new(deepin_machine(true)));
        let (outcome, _rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let probe = runs.iter().find(|r| r.contains("pacman -Sp")).expect("проба набора");
        let words: Vec<&str> = probe.split_whitespace().collect();
        assert!(words.contains(&"treeland") && words.contains(&"ddm"), "{probe}");
        assert!(!words.iter().any(|w| ["deepin", "deepin-session", "deepin-kwin"].contains(w)), "{probe}");
        assert!(!runs.iter().any(|r| r.contains("deepin-kwin")), "{runs:#?}");
        assert!(runs.iter().any(|r| r.starts_with("pkexec bash /tmp/rexon_install_")), "{runs:#?}");
    }

    /// There is a session: Deepin on Treeland as a variant set via the
    /// `DeInstallSet` operation (the helper repeats the core rule itself),
    /// without `pkexec` and without deepin-session / deepin-kwin.
    #[test]
    fn deepin_set_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(deepin_machine(true))), out(0, "", ""));
        let (outcome, _rec) = run_install(&io, "deepin", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec bash")), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        let set = ops
            .iter()
            .find_map(|op| match op {
                PrivOp::DeInstallSet { family: PkgFamily::Arch, de_id: Some(de), packages, deps_only, replace, update_first: true }
                    if de == "deepin" && deps_only.is_empty() && replace.is_empty() =>
                {
                    Some(packages.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{ops:?}"));
        assert!(set.contains(&"treeland".to_string()) && set.contains(&"ddm".to_string()), "{set:?}");
        assert!(!set.iter().any(|p| ["deepin", "deepin-session", "deepin-kwin"].contains(&p.as_str())), "{set:?}");
    }

    /// There is a session: replacing River 0.5 with consent is `DeInstallSet`
    /// with exactly the `replace` the probe approved (`plan::conflict_swaps`),
    /// without `pkexec`.
    #[test]
    fn river_replacement_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(river_over_05)), out(0, "", ""));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).install_de_replacing(
            "river",
            "kde",
            &["river".to_string(), "kwin".to_string()],
        );
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec bash")), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        assert!(
            ops.iter().any(|op| matches!(op,
                PrivOp::DeInstallSet { de_id: Some(de), replace, .. } if de == "river" && replace == &["river".to_string()])),
            "{ops:?}"
        );
    }

    /// The question before installing Deepin, with an "experimental" warning;
    /// on top of regular Deepin, a refusal.
    #[test]
    fn deepin_verdict_is_experimental_or_refused() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let fresh = FakeIo::arch(Box::new(deepin_machine(false)));
        let rec: Arc<dyn DeOpSink> = Arc::new(Recorder::default());
        let v = DeOpsExec::new(&fresh, rec.clone(), CancelToken::new()).install_verdict("deepin");
        assert!(v.allowed && v.outcome == "warning", "{v:?}");
        let ordinary = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "deepin-session"] => out(0, "deepin-session 2.0.33-1\n", ""),
                _ => healthy(argv),
            }
        }));
        let v = DeOpsExec::new(&ordinary, rec, CancelToken::new()).install_verdict("deepin");
        assert!(!v.allowed, "{v:?}");
        assert_eq!(v.title, "Rexon supports Deepin only on Treeland");
    }

    /// Cancellation while waiting for someone else's lock: no password is asked,
    /// temporary files are removed, the result is "cancelled, system unchanged".
    #[test]
    fn install_cancel_while_waiting_lock_changes_nothing() {
        let mut io = FakeIo::arch(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (outcome, _rec) = run_install(&io, "xfce", token);
        assert!(outcome.cancelled, "{outcome:?}");
        assert_eq!(outcome.message, cancelled_message());
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
        assert!(!io.files.borrow().keys().any(|k| k.starts_with("/tmp/rexon_install_")));
    }

    /// Cancellation pressed before the start: no snapshot, no panel copy, no commands.
    #[test]
    fn install_cancelled_before_start_touches_nothing() {
        let io = FakeIo::arch(Box::new(healthy));
        let token = CancelToken::new();
        token.cancel();
        let (outcome, _rec) = run_install(&io, "xfce", token);
        assert!(outcome.cancelled);
        assert!(io.index_of("write").is_none(), "{:#?}", io.calls.borrow());
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
    }

    /// A distribution refusal is explained BEFORE the first side effect.
    #[test]
    fn install_blocked_distro_explains_before_any_side_effect() {
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert(
            "/etc/os-release".to_string(),
            "ID=osnova\nNAME=\"OSnova\"\nPRETTY_NAME=\"ОСнова\"\n".to_string(),
        );
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.index_of("write").is_none(), "{:#?}", io.calls.borrow());
        assert!(io.index_of("panel_snapshots").is_none());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }

    // ── Window privilege session (B8) ───────────────────────────────────

    fn with_session(mut io: FakeIo, answer: RunOut) -> FakeIo {
        io.session = Some(answer);
        io
    }

    /// There is a session: the desktop set goes to the helper as an operation
    /// with this desktop's table, progress goes to the log `tail_log` reads; no `pkexec`.
    #[test]
    fn install_goes_through_privilege_session() {
        let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        let [PrivOp::PkgInstall { family: PkgFamily::Arch, packages, source }] = ops.as_slice() else {
            panic!("{ops:?}");
        };
        assert!(!packages.is_empty());
        assert_eq!(source, &PkgSource::Desktop { de_id: "xfce".into() });
        assert!(io.index_of("priv_log: /tmp/rexon_install_").is_some(), "{:#?}", io.calls.borrow());
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// The set that went to the session helper as one `PkgInstall` operation.
    fn session_install_set(io: &FakeIo) -> Vec<String> {
        let ops = io.priv_ops.borrow();
        ops.iter()
            .find_map(|op| match op {
                PrivOp::PkgInstall { packages, source: PkgSource::Desktop { .. }, .. } => Some(packages.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{ops:?}"))
    }

    fn run_install_session(io: &FakeIo, de: &str, session: Option<system::SessionKind>) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, CancelToken::new()).install_de_session(de, "kde", &[], session);
        (outcome, rec)
    }

    /// Choosing «Wayland» for Xfce adds labwc and Xwayland to the set, and the
    /// set still passes the privilege session rule as a desktop set (not
    /// `DeInstallSet`); «X11» and "no choice" give the set as before.
    #[test]
    fn install_session_choice_changes_the_set() {
        let base = system::get_packages("xfce", PkgFamily::Arch);
        for (session, want_labwc) in [
            (None, false),
            (Some(system::SessionKind::X11), false),
            (Some(system::SessionKind::Wayland), true),
        ] {
            let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
            let (outcome, _) = run_install_session(&io, "xfce", session);
            assert!(outcome.ok, "{session:?}: {outcome:?}");
            let set = session_install_set(&io);
            for p in &base {
                assert!(set.contains(p), "{session:?}: {set:?}");
            }
            for extra in ["labwc", "xorg-xwayland"] {
                assert_eq!(set.iter().any(|p| p == extra), want_labwc, "{session:?}: {set:?}");
            }
        }
        // LXQt: its own session bundle.
        let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        let (outcome, _) = run_install_session(&io, "lxqt", Some(system::SessionKind::Wayland));
        assert!(outcome.ok, "{outcome:?}");
        assert!(session_install_set(&io).iter().any(|p| p == "lxqt-wayland-session"));
    }

    /// «➕ Add Wayland variant» for LXQt (live log of 08.11.2026): only the
    /// session add-on is installed, without the desktop set; the desktop install
    /// steps (defaults, post-install, login screen, "open with") do not run; the
    /// result is about the session, with its name on the login screen.
    #[test]
    fn add_session_installs_only_the_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).add_session("lxqt", system::SessionKind::Wayland);
        assert!(outcome.ok, "{outcome:?}");
        let set = session_install_set(&io);
        assert!(set.iter().any(|p| p == "lxqt-wayland-session"), "{set:?}");
        assert!(!set.iter().any(|p| p == "lxqt" || p == "openbox" || p == "sddm"), "{set:?}");
        let stages = rec.stages();
        for step in ["snapshot", "post_install", "dm_enable", "switcher", "restore", "mime"] {
            assert!(!stages.iter().any(|s| s == step), "{step}: {stages:?}");
        }
        let done = rec.messages("done");
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(done[0].contains("Wayland version of LXQt is added") && done[0].contains("“LXQt (Wayland)”"), "{done:?}");
        let intro = rec.messages("install");
        assert!(intro.iter().any(|m| m.contains("Adding the Wayland version of LXQt")), "{intro:?}");

        // X11: the desktop set (the X server is a dependency of its login screen).
        let io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(&io, sink, CancelToken::new()).add_session("xfce", system::SessionKind::X11);
        assert!(outcome.ok, "{outcome:?}");
        let set = session_install_set(&io);
        assert!(set.iter().any(|p| p == "xfce4") && set.iter().any(|p| p == "lightdm"), "{set:?}");
        assert!(!set.iter().any(|p| p == "labwc"), "{set:?}");
    }

    /// A Wayland session only from the AUR (sample: the MATE on Wayfire entry
    /// removed on 16.11.2026; the mechanism waits for MATE (Polo)): without an
    /// AUR helper, a refusal BEFORE the password (otherwise the session would
    /// silently not get installed), with a helper the compositor is in the
    /// common transaction and the session bundle goes via the AUR helper.
    #[test]
    fn install_aur_wayland_session_needs_aur_helper() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        system::test_session_variants::with_aur_sample(|| {
            let not_in_repo = |argv: &[String]| -> RunOut {
                let a: Vec<&str> = argv.iter().map(String::as_str).collect();
                match a.as_slice() {
                    ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                    _ => healthy(argv),
                }
            };
            let io = with_session(FakeIo::arch(Box::new(not_in_repo)), out(0, "", ""));
            let (outcome, rec) = run_install_session(&io, "mate", Some(system::SessionKind::Wayland));
            assert!(!outcome.ok);
            assert!(outcome.message.contains("yay or paru"), "{}", outcome.message);
            assert!(io.priv_ops.borrow().is_empty(), "{:?}", io.priv_ops.borrow());
            assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
            assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);

            let mut io = with_session(FakeIo::arch(Box::new(not_in_repo)), out(0, "", ""));
            io.bins.insert("paru".to_string());
            let (outcome, _) = run_install_session(&io, "mate", Some(system::SessionKind::Wayland));
            assert!(outcome.ok, "{outcome:?}");
            assert!(session_install_set(&io).iter().any(|p| p == "wayfire"));
            assert!(
                io.runs().iter().any(|r| r.starts_with("paru") && r.contains("mate-wayland-session")),
                "{:#?}",
                io.runs()
            );
        });
    }

    /// MATE is installed only with the X11 session (16.11.2026): choosing
    /// Wayland in the dialog adds nothing: neither Wayfire nor `mate-wayland-session`.
    #[test]
    fn install_mate_is_x11_only() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = with_session(FakeIo::arch(Box::new(healthy)), out(0, "", ""));
        io.bins.insert("paru".to_string());
        let (outcome, _) = run_install_session(&io, "mate", Some(system::SessionKind::Wayland));
        assert!(outcome.ok, "{outcome:?}");
        let set = session_install_set(&io);
        assert!(!set.iter().any(|p| p == "wayfire"), "{set:?}");
        assert!(!io.runs().iter().any(|r| r.contains("mate-wayland-session")), "{:#?}", io.runs());
    }

    /// The MATE Menu is built in a transaction of its own (24.11.2026): the AUR
    /// package has been failing to install, and its failure must neither take the
    /// other MATE add-ons down nor fail the desktop install. Brisk is not built
    /// at all.
    #[test]
    fn install_mate_builds_mate_menu_alone_and_survives_its_failure() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let menu_fails = |argv: &[String]| -> RunOut {
            if argv.first().map(String::as_str) == Some("paru") && argv.iter().any(|a| a == "mate-menu") {
                return out(1, "error: failed to commit transaction (conflicting files)", "");
            }
            healthy(argv)
        };
        let mut io = with_session(FakeIo::arch(Box::new(menu_fails)), out(0, "", ""));
        io.bins.insert("paru".to_string());
        let (outcome, _) = run_install_session(&io, "mate", Some(system::SessionKind::X11));
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let paru: Vec<&String> = runs.iter().filter(|r| r.contains("paru ")).collect();
        let menu: Vec<&&String> = paru.iter().filter(|r| r.contains("mate-menu")).collect();
        assert_eq!(menu.len(), 1, "{paru:#?}");
        assert!(menu[0].trim_end().ends_with(" mate-menu"), "mate-menu goes in a transaction of its own: {}", menu[0]);
        assert!(paru.iter().any(|r| r.contains("ayatana-indicator-application")), "{paru:#?}");
        assert!(!runs.iter().any(|r| r.contains("brisk")), "{runs:#?}");
    }

    /// The "no login screen" guard through the session: SDDM from the login
    /// screen table and enabling its unit as operations, without `pkexec`.
    #[test]
    fn missing_login_manager_is_fixed_through_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
                _ => healthy(argv),
            }
        }));
        let io = with_session(io, out(0, "", ""));
        let (outcome, _rec) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        assert!(ops.contains(&PrivOp::PkgInstall {
            family: PkgFamily::Arch,
            packages: vec!["sddm".into()],
            source: PkgSource::DisplayManager,
        }), "{ops:?}");
        assert!(ops.contains(&PrivOp::EnableDm { family: PkgFamily::Arch, unit: "sddm".into() }), "{ops:?}");
    }

    /// A session helper refusal is an install error, and no old command follows
    /// it (Z5).
    #[test]
    fn install_session_refusal_has_no_raw_retry() {
        let refused = RunOut {
            outcome: RunOutcome::Failed,
            returncode: Some(1),
            stdout: String::new(),
            stderr: "rexon-priv-helper: помощник отказал: refused.not-in-table".into(),
        };
        let io = with_session(FakeIo::arch(Box::new(healthy)), refused);
        let (outcome, rec) = run_install(&io, "xfce", CancelToken::new());
        assert!(!outcome.ok, "{outcome:?}");
        assert!(rec.count(de_progress::STAGE_ERROR) >= 1);
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        assert_eq!(io.priv_ops.borrow().len(), 1);
    }

    /// There is a session: desktop removal is an operation on the same set the
    /// old command would remove.
    #[test]
    fn remove_goes_through_privilege_session() {
        let plain = FakeIo::arch(Box::new(removable));
        let (outcome, _) = run_remove(&plain, "xfce", "kde");
        assert!(outcome.ok);
        let raw = plain.runs().into_iter().find(|r| r.starts_with("pkexec pacman -Rns --noconfirm")).unwrap();
        let want: Vec<String> =
            raw.split_whitespace().skip(4).filter(|w| *w != "--").map(str::to_string).collect();

        let io = with_session(FakeIo::arch(Box::new(removable)), out(0, "", ""));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        assert_eq!(
            io.priv_ops.borrow().as_slice(),
            [PrivOp::PkgRemove { family: PkgFamily::Arch, packages: want }]
        );
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// A tiling WM on a machine without a login screen: SDDM is installed and
    /// enabled AUTOMATICALLY, without asking.
    #[test]
    fn tiling_wm_without_login_manager_gets_sddm() {
        // Texts come from the translation catalog; the source language (Russian is
        // checked by the core's checks on po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["systemctl", "is-enabled", ..] => out(1, "disabled\n", ""),
                _ => healthy(argv),
            }
        }));
        let (outcome, rec) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        assert!(runs.iter().any(|r| r.starts_with("pkexec pacman -S") && r.contains("sddm")), "{runs:#?}");
        assert!(runs.iter().any(|r| r == "pkexec systemctl enable sddm.service"), "{runs:#?}");
        assert!(rec.messages("dm_ensure").iter().any(|m| m.contains("SDDM installed")));
        // There was no login screen: it only comes up at boot, so the result says
        // "reboot".
        assert!(outcome.restart_needed, "{outcome:?}");
    }

    /// River: river-classic is installed, and after the packages a starter init
    /// (executable) is put in place: without it River starts without a single
    /// key binding. The person's own init is not touched.
    #[test]
    fn river_gets_starter_init_once() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "river", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        // The login screen is already installed, no kernel or kernel module in the
        // plan: logging out is enough (the live install of 24.10.2026 asked for a reboot).
        assert!(!outcome.restart_needed, "{outcome:?}");
        let runs = io.runs();
        assert!(runs.iter().any(|r| r.starts_with("pacman -Sp") && r.contains(" river-classic ")), "{runs:#?}");
        let init = "/home/t/.config/river/init";
        // The starter init plus the layout by language (the fake machine has LANG=ru).
        let kb = crate::keyboard_layout::detect(None, None, None, "ru_RU.UTF-8").unwrap();
        let expected = match crate::keyboard_layout::apply(
            crate::keyboard_layout::Desk::River, plan::river_starter_init(), &[], &kb,
        ) {
            crate::keyboard_layout::Apply::Write(t) => t,
            other => panic!("{other:?}"),
        };
        assert_eq!(io.files.borrow().get(init).map(String::as_str), Some(expected.as_str()));
        assert!(expected.contains("riverctl keyboard-layout -options grp:alt_shift_toggle us,ru\n"));
        assert!(io.calls.borrow().iter().any(|c| c == &format!("write_mode: {init} 755")), "{:#?}", io.calls.borrow());

        // The person's own init: they set the layout themselves, not touched at all.
        let mine = "#!/bin/sh\nriverctl keyboard-layout de\nmine\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert(init.to_string(), mine.to_string());
        let (outcome, _) = run_install(&io, "river", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(init).map(String::as_str), Some(mine));
    }

    /// LXQt: «Lock» gets a locker in both sessions (commands in `session.conf`);
    /// the person's own command is not touched.
    #[test]
    fn lxqt_gets_lock_commands() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let conf = "/home/t/.config/lxqt/session.conf";
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, rec) = run_install(&io, "lxqt", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(io.runs().iter().any(|r| r.starts_with("pacman -Sp") && r.contains(" xscreensaver")), "{:#?}", io.runs());
        assert_eq!(io.files.borrow().get(conf), plan::lxqt_lock_edit(None).as_ref());
        assert!(rec.messages("post").iter().any(|m| m.contains("screen lock set up")), "{:#?}", rec.messages("post"));

        let mine = "[General]\nlock_command=i3lock\nlock_command_wayland=waylock\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert(conf.into(), mine.into());
        let (outcome, _) = run_install(&io, "lxqt", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(conf).map(String::as_str), Some(mine));
    }

    /// For the tiling ones and Wayfire the layout is added after the install:
    /// for sway its own config with an `include` of the system one, for Wayfire
    /// the `[input]` section. Rexon does not touch a layout set by the person;
    /// for English without a system layout there is nothing to write.
    #[test]
    fn tiling_desktops_get_keyboard_layout() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert("/etc/sway/config".into(), "set $mod Mod4\n".into());
        let (outcome, _) = run_install(&io, "sway", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let cfg = io.files.borrow().get("/home/t/.config/sway/config").cloned().unwrap_or_default();
        assert!(cfg.starts_with("include /etc/sway/config\n"), "{cfg}");
        assert!(cfg.contains("xkb_layout \"us,ru\"") && cfg.contains("xkb_options \"grp:alt_shift_toggle\""), "{cfg}");

        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let ini = io.files.borrow().get("/home/t/.config/wayfire.ini").cloned().unwrap_or_default();
        assert!(ini.contains("\n[input]\n") && ini.contains("xkb_layout = us,ru\n"), "{ini}");
        // Next to it, the screen lock (Wayfire has none by default).
        assert_eq!(plan::wayfire_lock_edit(&ini), None, "{ini}");
        assert!(ini.contains("command_lock = swaylock -f\n"), "{ini}");

        // An own layout in wayfire.ini: its lines are unchanged, only the lock is
        // added.
        let own = "[input]\nxkb_layout = de\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert("/home/t/.config/wayfire.ini".into(), own.into());
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let expected = plan::wayfire_lock_edit(own).unwrap();
        assert!(expected.starts_with(own));
        assert_eq!(io.files.borrow().get("/home/t/.config/wayfire.ini"), Some(&expected));

        // Own lock and own layout: the file is bit for bit the same.
        let own = "[input]\nxkb_layout = de\n\n[autostart]\nidle = swayidle before-sleep swaylock\n\n[command]\nbinding_lock = <super> KEY_L\ncommand_lock = swaylock\n";
        let io = FakeIo::arch(Box::new(healthy));
        io.files.borrow_mut().insert("/home/t/.config/wayfire.ini".into(), own.into());
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/wayfire.ini").map(String::as_str), Some(own));

        // English and an empty system: nothing to write for the layout, the file
        // has only the lock.
        let mut io = FakeIo::arch(Box::new(healthy));
        io.env.insert("LANG".into(), "en_US.UTF-8".into());
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/wayfire.ini"), plan::wayfire_lock_edit("").as_ref());

        // Hyprland without a config and without a template: the file is not created
        // (Hyprland puts its own at first login), the log has a hint.
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.files.borrow().keys().any(|k| k.contains("/hypr/")), "{:?}", io.files.borrow().keys());
    }

    /// Tiling desktops lock after install: sway gets the key and swayidle in
    /// the user's config with `include` of the system one (the layout goes
    /// into the same file), Hyprland a hyprlock binding, hypridle autostart
    /// and `hypridle.conf`, niri swayidle. The user's own settings are left
    /// alone; programs missing — config untouched, a plain log line.
    #[test]
    fn tiling_desktops_get_screen_lock() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let with_bins = |bins: &[&str]| {
            let mut io = FakeIo::arch(Box::new(healthy));
            io.bins.extend(bins.iter().map(|b| b.to_string()));
            io
        };

        // sway: no user config — one is created with include of the system one.
        let io = with_bins(&["swaylock", "swayidle"]);
        io.files.borrow_mut().insert("/etc/sway/config".into(), "set $mod Mod4\n# exec swayidle -w\n".into());
        let (outcome, rec) = run_install(&io, "sway", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let cfg = io.files.borrow().get("/home/t/.config/sway/config").cloned().unwrap_or_default();
        assert!(cfg.starts_with("include /etc/sway/config\n"), "{cfg}");
        assert!(cfg.contains("bindsym Mod4+Shift+Escape exec swaylock -f\n"), "{cfg}");
        assert!(cfg.contains(&format!("exec {}\n", plan::SWAYIDLE_LOCK)), "{cfg}");
        assert!(cfg.contains("xkb_layout \"us,ru\""), "{cfg}");
        assert!(rec.messages("post").iter().any(|m| m.contains("screen lock set up")), "{:#?}", rec.messages("post"));

        // sway: the user's own settings — the file stays byte-for-byte.
        let mine = "include /etc/sway/config\nbindsym $mod+l exec gtklock\nexec swayidle -w timeout 600 gtklock\ninput * xkb_layout de\n";
        let io = with_bins(&["swaylock", "swayidle"]);
        io.files.borrow_mut().insert("/home/t/.config/sway/config".into(), mine.into());
        let (outcome, _) = run_install(&io, "sway", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/sway/config").map(String::as_str), Some(mine));

        // sway: swayidle enabled as a systemd unit — only the key is added.
        let io = with_bins(&["swaylock", "swayidle"]);
        io.files.borrow_mut().insert("/home/t/.config/sway/config".into(), "include /etc/sway/config\ninput * xkb_layout de\n".into());
        io.files.borrow_mut().insert("/home/t/.config/systemd/user/sway-session.target.wants/swayidle.service".into(), String::new());
        let (outcome, _) = run_install(&io, "sway", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let cfg = io.files.borrow().get("/home/t/.config/sway/config").cloned().unwrap_or_default();
        assert!(cfg.contains("bindsym Mod4+Shift+Escape") && !cfg.contains("exec swayidle"), "{cfg}");

        // Hyprland (Lua) from the package sample: binding, hypridle and its config.
        let io = with_bins(&["hyprlock", "hypridle"]);
        io.files.borrow_mut().insert("/usr/share/hypr/hyprland.lua".into(), "local mainMod = \"SUPER\"\nhl.bind(mainMod .. \" + Q\", hl.dsp.exec_cmd(\"kitty\"))\n".into());
        let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let lua = io.files.borrow().get("/home/t/.config/hypr/hyprland.lua").cloned().unwrap_or_default();
        assert!(lua.contains("hl.bind(\"SUPER + SHIFT + Escape\", hl.dsp.exec_cmd(\"hyprlock\"))\n"), "{lua}");
        assert!(lua.contains("hl.exec_cmd(\"hypridle\")"), "{lua}");
        assert_eq!(io.files.borrow().get("/home/t/.config/hypr/hypridle.conf"), Some(&plan::hypridle_conf()));

        // Hyprland: own hypridle.conf and own autostart — nothing is touched.
        let mine = "bind = $mainMod, L, exec, hyprlock\nexec-once = hypridle\n";
        let io = with_bins(&["hyprlock", "hypridle"]);
        io.files.borrow_mut().insert("/home/t/.config/hypr/hyprland.conf".into(), mine.into());
        io.files.borrow_mut().insert("/home/t/.config/hypr/hypridle.conf".into(), "general {}\n".into());
        let mut io = io;
        io.env.insert("LANG".into(), "en_US.UTF-8".into());
        let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/hypr/hyprland.conf").map(String::as_str), Some(mine));
        assert_eq!(io.files.borrow().get("/home/t/.config/hypr/hypridle.conf").map(String::as_str), Some("general {}\n"));

        // Hyprland without hyprlock/hypridle (family without packages): the
        // config is untouched, the log says what is missing.
        let io = FakeIo::arch(Box::new(healthy));
        let conf = "input {\n    kb_layout = de\n}\n";
        io.files.borrow_mut().insert("/home/t/.config/hypr/hyprland.conf".into(), conf.into());
        let (outcome, rec) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/hypr/hyprland.conf").map(String::as_str), Some(conf));
        assert!(rec.messages("post").iter().any(|m| m.contains("screen lock programs are missing (hyprlock, hypridle)")), "{:#?}", rec.messages("post"));

        // niri: swayidle goes into the user's config; a second run changes nothing.
        let io = with_bins(&["swaylock", "swayidle"]);
        let kdl = "input {\n    keyboard {\n        xkb {\n            layout \"de\"\n        }\n    }\n}\nspawn-at-startup \"waybar\"\n";
        io.files.borrow_mut().insert("/home/t/.config/niri/config.kdl".into(), kdl.into());
        let (outcome, _) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        let out = io.files.borrow().get("/home/t/.config/niri/config.kdl").cloned().unwrap_or_default();
        assert!(out.starts_with(kdl) && out.contains("spawn-at-startup \"swayidle\" \"-w\""), "{out}");
        let (outcome, _) = run_install(&io, "niri", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get("/home/t/.config/niri/config.kdl"), Some(&out));
    }

    /// Lock screen look after install: swaylock's config for every desktop
    /// that locks with swaylock, hyprlock's for Hyprland. A config of the
    /// user's own (any of the paths the locker reads, the system one too)
    /// stays byte for byte and nothing is added next to it; without the
    /// locker nothing is written.
    #[test]
    fn lockers_get_own_look_unless_configured() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let with_bins = |bins: &[&str]| {
            let mut io = FakeIo::arch(Box::new(healthy));
            io.bins.extend(bins.iter().map(|b| b.to_string()));
            io
        };
        let sway_conf = "/home/t/.config/swaylock/config";
        let hypr_conf = "/home/t/.config/hypr/hyprlock.conf";

        for de in ["sway", "niri", "wayfire", "river", "lxqt"] {
            let io = with_bins(&["swaylock", "swayidle"]);
            let (outcome, rec) = run_install(&io, de, CancelToken::new());
            assert!(outcome.ok, "{de}: {outcome:?}");
            assert_eq!(io.files.borrow().get(sway_conf), Some(&plan::swaylock_config()), "{de}");
            assert!(rec.messages("post").iter().any(|m| m.contains("the lock screen got a dark background")), "{de}: {:#?}", rec.messages("post"));
        }

        // The user's own config (each place swaylock reads): kept, no second file.
        for own in [sway_conf, "/home/t/.swaylock/config", "/etc/swaylock/config"] {
            let io = with_bins(&["swaylock", "swayidle"]);
            io.files.borrow_mut().insert(own.into(), "color=ff00ff\n".into());
            let (outcome, _) = run_install(&io, "sway", CancelToken::new());
            assert!(outcome.ok, "{outcome:?}");
            assert_eq!(io.files.borrow().get(own).map(String::as_str), Some("color=ff00ff\n"), "{own}");
            if own != sway_conf {
                assert!(!io.files.borrow().contains_key(sway_conf), "{own}");
            }
        }

        // No swaylock — nothing is written.
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _) = run_install(&io, "wayfire", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.files.borrow().contains_key(sway_conf));

        // Hyprland: hyprlock.conf next to hypridle.conf; own one is kept.
        let io = with_bins(&["hyprlock", "hypridle"]);
        io.files.borrow_mut().insert("/home/t/.config/hypr/hyprland.conf".into(), "input {\n    kb_layout = de\n}\n".into());
        let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(hypr_conf), Some(&plan::hyprlock_conf()));
        assert!(!io.files.borrow().contains_key(sway_conf));

        for own in [hypr_conf, "/etc/xdg/hypr/hyprlock.conf"] {
            let io = with_bins(&["hyprlock", "hypridle"]);
            io.files.borrow_mut().insert(own.into(), "background {\n    path = screenshot\n}\n".into());
            let (outcome, _) = run_install(&io, "hyprland", CancelToken::new());
            assert!(outcome.ok, "{outcome:?}");
            assert_eq!(io.files.borrow().get(own).map(String::as_str), Some("background {\n    path = screenshot\n}\n"), "{own}");
            if own != hypr_conf {
                assert!(!io.files.borrow().contains_key(hypr_conf), "{own}");
            }
        }

        // A second run changes nothing: the file is ours now, but it is a file.
        let io = with_bins(&["swaylock", "swayidle"]);
        run_install(&io, "sway", CancelToken::new());
        io.files.borrow_mut().insert(sway_conf.into(), "color=000000\n".into());
        run_install(&io, "sway", CancelToken::new());
        assert_eq!(io.files.borrow().get(sway_conf).map(String::as_str), Some("color=000000\n"));
    }

    /// An unknown desktop does not reach a single command.
    #[test]
    fn unknown_desktop_is_refused() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, rec) = run_install(&io, "no-such-de", CancelToken::new());
        assert!(!outcome.ok);
        assert_eq!(outcome.message, unknown_de_message("no-such-de"));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.runs().is_empty());
    }

    fn run_switch(io: &FakeIo, target: &str, current: &str) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, CancelToken::new()).switch_de(target, current);
        (outcome, rec)
    }

    /// We do not switch to the current or an unrecognised desktop: no copies, no
    /// logout.
    #[test]
    fn switch_guard_refuses_without_side_effects() {
        for current in ["xfce", "unknown"] {
            let io = FakeIo::arch(Box::new(healthy));
            let (outcome, rec) = run_switch(&io, "xfce", current);
            assert!(!outcome.ok);
            assert_eq!(
                Some(outcome.message.clone()),
                plan::switch_de_guard(current, "xfce", "XFCE")
            );
            assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
            assert!(io.calls.borrow().is_empty(), "{:#?}", io.calls.borrow());
        }
    }

    /// Order as in the original: copies → default applications → bridge →
    /// logout; the first successful logout command is the last one.
    #[test]
    fn switch_backs_up_then_logs_out() {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.iter().any(|a| a == "--dest=org.kde.LogoutPrompt") {
                out(1, "", "The name is not activatable")
            } else {
                healthy(argv)
            }
        }));
        io.env.insert("XDG_CURRENT_DESKTOP".into(), "KDE".into());
        io.env.insert("XDG_SESSION_ID".into(), "7".into());
        let (outcome, rec) = run_switch(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let plasma = io.index_of("panel_snapshots: before_switch").unwrap();
        let config = io.index_of("config_backup: kde").unwrap();
        let apps = io.index_of("app_defaults: xfce").unwrap();
        let bridge = io.index_of("bridge: DeSwitch").unwrap();
        let logout = io.index_of("run: dbus-send").unwrap();
        assert!(plasma < config && config < apps && apps < bridge && bridge < logout);
        let runs = io.runs();
        assert_eq!(runs.iter().filter(|r| r.starts_with("dbus-send")).count(), 2, "{runs:#?}");
        assert!(!runs.iter().any(|r| r.starts_with("loginctl")));
        assert_eq!(rec.messages("done"), vec![logout_message()]);
    }

    /// No logout command worked: the person sees why.
    #[test]
    fn switch_logout_failure_is_reported() {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("xfce4-session-logout")
                || argv.first().map(String::as_str) == Some("loginctl")
            {
                out(1, "", "нет такой сессии")
            } else {
                healthy(argv)
            }
        }));
        io.env.insert("XDG_CURRENT_DESKTOP".into(), "XFCE".into());
        io.env.insert("XDG_SESSION_ID".into(), "7".into());
        let (outcome, rec) = run_switch(&io, "kde", "xfce");
        assert!(!outcome.ok);
        assert_eq!(outcome.message, "нет такой сессии");
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.runs().iter().any(|r| r == "loginctl terminate-session 7"));
    }

    fn run_remove(io: &FakeIo, de: &str, current: &str) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, CancelToken::new()).remove_de(de, current);
        (outcome, rec)
    }

    /// The desktop's packages are installed, nobody outside needs them, the password is accepted.
    fn removable(argv: &[String]) -> RunOut {
        let a: Vec<&str> = argv.iter().map(String::as_str).collect();
        match a.as_slice() {
            ["pacman", "-Q", _] => out(0, "", ""),
            ["pacman", flag, ..] if flag.contains('g') || flag.contains('i') => {
                out(1, "", "")
            }
            _ => healthy(argv),
        }
    }

    /// ⛔ The current desktop is not removed EVEN on a direct executor call: not
    /// a single package manager probe, no password. And it has no button.
    #[test]
    fn current_desktop_cannot_be_removed_even_by_direct_call() {
        // Texts come from the translation catalog; the source language (Russian is
        // checked by the core's checks on po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, rec) = run_remove(&io, "kde", "kde");
        assert!(!outcome.ok);
        assert!(outcome.message.starts_with("You can't remove your current desktop"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.calls.borrow().is_empty(), "{:#?}", io.calls.borrow());
        let kinds: Vec<&str> = crate::qt_desktops::card_buttons(true, true, true)
            .iter()
            .map(|b| b.kind)
            .collect();
        assert!(!kinds.contains(&"remove"), "{kinds:?}");
    }

    /// The usual path: probes → lock → one privileged removal command → bridge
    /// event → "removed".
    #[test]
    fn remove_waits_lock_then_runs_privileged_remove() {
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let removes: Vec<_> = runs
            .iter()
            .filter(|r| r.starts_with("pkexec pacman -Rns --noconfirm"))
            .collect();
        assert_eq!(removes.len(), 1, "{runs:#?}");
        assert!(io.index_of("bridge: DeRemoved(\"xfce\")").unwrap() > io.index_of("run: pkexec").unwrap());
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// MATE on Wayfire is gone from the product (16.11.2026), but a
    /// `mate-wayland-session` that an earlier release installed is still removed
    /// with MATE: it holds MATE's packages as a dependency, and left behind it
    /// would make `-Rns` refuse the removal. Not installed — not asked for.
    #[test]
    fn remove_mate_takes_the_old_wayland_session_bundle_along() {
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, _) = run_remove(&io, "mate", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let remove = runs.iter().find(|r| r.starts_with("pkexec pacman -Rns --noconfirm")).unwrap_or_else(|| panic!("{runs:#?}"));
        assert!(remove.split_whitespace().any(|w| w == "mate-wayland-session"), "{remove}");

        let not_there = |argv: &[String]| -> RunOut {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "mate-wayland-session"] => out(1, "", "error: package was not found"),
                _ => removable(argv),
            }
        };
        let io = FakeIo::arch(Box::new(not_there));
        let (outcome, _) = run_remove(&io, "mate", "kde");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let remove = runs.iter().find(|r| r.starts_with("pkexec pacman -Rns --noconfirm")).unwrap_or_else(|| panic!("{runs:#?}"));
        assert!(!remove.contains("mate-wayland-session"), "{remove}");
    }

    /// After the desktop is removed, Rexon's own per-desktop files of it go;
    /// the person's file of the same name and other desktops' files stay.
    #[test]
    fn remove_takes_our_desktop_files_along() {
        let io = FakeIo::arch(Box::new(removable));
        let ours = dfl::serialize_mimeapps_list(&[("inode/directory".into(), "thunar.desktop".into())], "ts");
        let cfg = "/home/t/.config";
        io.files.borrow_mut().insert(format!("{cfg}/xfce-mimeapps.list"), ours.clone());
        io.files.borrow_mut().insert(
            format!("{cfg}/xfce-xdg-terminals.list"),
            dfl::desktop_terminals_list_text("xfce4-terminal.desktop", false),
        );
        io.files.borrow_mut().insert(format!("{cfg}/kde-mimeapps.list"), ours.clone());
        let (outcome, _rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.files.borrow().contains_key(&format!("{cfg}/xfce-mimeapps.list")));
        assert!(!io.files.borrow().contains_key(&format!("{cfg}/xfce-xdg-terminals.list")));
        assert!(io.files.borrow().contains_key(&format!("{cfg}/kde-mimeapps.list")));

        // The person's own list under the same name is not ours to remove.
        let io = FakeIo::arch(Box::new(removable));
        io.files.borrow_mut().insert(format!("{cfg}/xfce-mimeapps.list"), "[Default Applications]\n".into());
        run_remove(&io, "xfce", "kde");
        assert!(io.files.borrow().contains_key(&format!("{cfg}/xfce-mimeapps.list")));
    }

    /// Sitting in Xfce, removing Cinnamon does not remove the shared login
    /// screen `lightdm` and the LightDM greeters (the hole of 28.10.2026).
    #[test]
    fn remove_other_desktop_keeps_shared_login_screen() {
        let io = FakeIo::arch(Box::new(removable));
        let (outcome, _rec) = run_remove(&io, "cinnamon-mint", "xfce");
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let remove = runs
            .iter()
            .find(|r| r.starts_with("pkexec pacman -Rns --noconfirm"))
            .unwrap_or_else(|| panic!("{runs:#?}"));
        let names: Vec<&str> = remove.split_whitespace().collect();
        assert!(names.iter().any(|n| n.starts_with("cinnamon")), "{remove}");
        assert!(!names.contains(&"lightdm"), "{remove}");
        assert!(!names.iter().any(|n| system::is_lightdm_greeter(n)), "{remove}");
    }

    /// Password refusal: the core's hint, the desktop is not announced as removed.
    #[test]
    fn remove_denied_password_shows_hint() {
        // Texts come from the translation catalog; the source language (Russian is
        // checked by the core's checks on po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Request dismissed")
            } else {
                removable(argv)
            }
        }));
        let (outcome, rec) = run_remove(&io, "xfce", "kde");
        assert!(!outcome.ok);
        assert!(outcome.message.contains("The administrator password wasn't confirmed"), "{}", outcome.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert!(io.index_of("bridge:").is_none());
    }

    /// None of the desktop's packages are installed: no password is asked.
    #[test]
    fn remove_with_nothing_installed_does_not_ask_password() {
        let io = FakeIo::arch(Box::new(healthy));
        let (outcome, _rec) = run_remove(&io, "xfce", "kde");
        assert!(outcome.ok, "{outcome:?}");
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
    }

    /// This module's progress lines are polite and without the old jargon.
    #[test]
    fn progress_texts_have_no_jargon() {
        for text in [
            snapshot_message(),
            restore_message(),
            mime_message(),
            post_install_message(),
            install_intro(3, 1),
            ensure_dm_enable_failed("code 1"),
        ] {
            for jargon in ["defaults", "MIME", "ensure_display_manager", "snapshot"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
        assert_eq!(install_intro(1, 0), "📦 Installing: 1 package");
        assert_eq!(install_intro(5, 2), "📦 Installing: 5 packages + 2 from the AUR");
    }

    // ── Desktop completeness ─────────────────────────────────────────────

    /// Fake side of the check: which packages are installed, which programs
    /// exist. Repair commands do NOT come here: `FakeIo` executes them, and its
    /// log shows whether a password was asked.
    #[derive(Default)]
    struct FakeChecks {
        installed: HashSet<String>,
        bins: HashSet<String>,
        env: HashMap<String, String>,
        runs: Vec<String>,
        /// `dconf dump /org/mate/panel/` answer and applet descriptions
        /// (`/usr/share/mate-panel/applets/<name>` → text); none by default.
        panel: Option<String>,
        applets: Vec<(String, String)>,
    }

    impl FakeChecks {
        fn with(installed: &[&str], bins: &[&str]) -> Self {
            let mut env = HashMap::new();
            env.insert("LANG".to_string(), "ru_RU.UTF-8".to_string());
            FakeChecks {
                installed: installed.iter().map(|s| s.to_string()).collect(),
                bins: bins.iter().map(|s| s.to_string()).collect(),
                env,
                runs: Vec::new(),
                panel: None,
                applets: Vec::new(),
            }
        }
    }

    impl ComplianceIo for FakeChecks {
        fn run(&mut self, argv: &[String], _timeout: Option<u64>) -> Option<comp::RunResult> {
            self.runs.push(argv.join(" "));
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            if let (["dconf", "dump", _], Some(panel)) = (a.as_slice(), &self.panel) {
                return Some(comp::RunResult { ok: true, stdout: panel.clone() });
            }
            let ok = match a.as_slice() {
                ["pacman", "-Q", pkg] => self.installed.contains(*pkg),
                _ => true,
            };
            Some(comp::RunResult {
                ok,
                stdout: "'Adwaita'".to_string(),
            })
        }
        fn which(&mut self, name: &str) -> bool {
            self.bins.contains(name)
        }
        fn env(&mut self, name: &str) -> String {
            self.env.get(name).cloned().unwrap_or_default()
        }
        fn exists(&mut self, _: &str) -> bool {
            false
        }
        fn is_file(&mut self, _: &str) -> bool {
            false
        }
        fn is_dir(&mut self, _: &str) -> bool {
            false
        }
        fn read_text(&mut self, path: &str) -> Option<String> {
            let name = path.strip_prefix(crate::mate_panel::SYSTEM_APPLETS_DIR)?.trim_start_matches('/');
            self.applets.iter().find(|(n, _)| n == name).map(|(_, t)| t.clone())
        }
        fn scandir(&mut self, _: &str) -> Option<Vec<ScanEntry>> {
            None
        }
        fn listdir(&mut self, path: &str) -> Vec<String> {
            if path == crate::mate_panel::SYSTEM_APPLETS_DIR {
                return self.applets.iter().map(|(n, _)| n.clone()).collect();
            }
            Vec::new()
        }
        fn makedirs(&mut self, _: &str) {}
        fn write_text(&mut self, _: &str, _: &str) {}
        fn replace(&mut self, _: &str, _: &str) {}
        fn realpath(&mut self, path: &str) -> String {
            path.to_string()
        }
        fn mkdtemp(&mut self, prefix: &str, _: Option<&str>) -> String {
            format!("/tmp/{prefix}1")
        }
        fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
            format!("/tmp/{prefix}1{suffix}")
        }
        fn chmod(&mut self, _: &str, _: u32) {}
        fn remove_file(&mut self, _: &str) {}
        fn remove_tree(&mut self, _: &str) {}
        fn copy_file(&mut self, _: &str, _: &str) {}
    }

    /// A COSMIC machine: this desktop has two repairs (wallpapers and icons) and
    /// one check without a repair (gnome-keyring).
    fn cosmic_io(responder: Responder) -> FakeIo {
        let mut io = FakeIo::arch(responder);
        io.env
            .insert("XDG_CURRENT_DESKTOP".to_string(), "COSMIC".to_string());
        io
    }

    fn pkexec_runs(io: &FakeIo) -> Vec<String> {
        io.runs()
            .into_iter()
            .filter(|r| r.starts_with("pkexec"))
            .collect()
    }

    fn survey(io: &FakeIo, checks: &mut FakeChecks) -> CompletenessSurvey {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .completeness_survey(checks)
    }

    fn complete(
        io: &FakeIo,
        checks: &mut FakeChecks,
        expected: &[(String, String)],
        cancel: CancelToken,
    ) -> (CompletenessOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).complete_de(checks, expected);
        (outcome, rec)
    }

    /// Everything is in place: nothing to install, no password asked at all.
    #[test]
    fn completeness_all_present_asks_nothing() {
        let io = cosmic_io(Box::new(healthy));
        let mut checks =
            FakeChecks::with(&["cosmic-wallpapers", "cosmic-icon-theme", "gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        assert_eq!(s.de_id.as_deref(), Some("cosmic"));
        assert!(s.fixes.is_empty(), "{s:?}");
        assert!(s.results.iter().all(|(_, v)| v == "ok"), "{s:?}");
        assert!(io.runs().is_empty(), "проверка без пароля что-то запустила: {:?}", io.runs());
    }

    /// Something missing → plan in words → «Yes» → success: the password is
    /// asked only in the second step, exactly once per missing item, and the
    /// result is "everything installed".
    #[test]
    fn completeness_missing_plan_then_success() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(healthy));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let label = |k: &str| comp::fix_label(k).unwrap().to_string();
        assert_eq!(
            s.fixes,
            vec![
                ("cosmic_wallpapers".to_string(), label("cosmic_wallpapers")),
                ("cosmic_icons".to_string(), label("cosmic_icons")),
            ]
        );
        assert!(pkexec_runs(&io).is_empty(), "пароль спрошен до вопроса");
        let labels: Vec<String> = s.fixes.iter().map(|f| f.1.clone()).collect();
        let question = completeness_confirm_text(&labels);
        let bullets = format!("• {}\n• {}", label("cosmic_wallpapers"), label("cosmic_icons"));
        assert!(question.contains(&bullets), "{question}");
        assert!(question.contains("administrator password"), "{question}");

        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(done.outcome.ok, "{done:?}");
        assert_eq!(done.outcome.message, completeness_done_message());
        let runs = pkexec_runs(&io);
        assert_eq!(runs.len(), 2, "{runs:#?}");
        assert!(runs[0].ends_with("cosmic-wallpapers"), "{runs:#?}");
        assert!(runs[1].ends_with("cosmic-icon-theme"), "{runs:#?}");
        let status = |k: &str| done.results.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(status("cosmic_wallpapers").as_deref(), Some("fixed"));
        assert_eq!(status("cosmic_icons").as_deref(), Some("fixed"));
        // Progress in words, without argv; cancellation was there and went out by the first password.
        let steps = rec.messages("install");
        assert_eq!(steps, vec!["📦 Installing: cosmic-wallpapers", "📦 Installing: cosmic-icon-theme"]);
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        // The package check went through the check executor, not through pkexec.
        assert!(checks.runs.iter().any(|r| r == "pacman -Q cosmic-wallpapers"));
        assert!(!checks.runs.iter().any(|r| r.starts_with("pkexec")));
    }

    /// B9: there is a privilege session: the install goes to the helper as an
    /// operation by the repair table (`DesktopRepair`), one package per repair;
    /// no `pkexec`.
    #[test]
    fn completeness_fix_goes_through_privilege_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = with_session(cosmic_io(Box::new(healthy)), out(0, "", ""));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, _) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(done.outcome.ok, "{done:?}");
        assert!(pkexec_runs(&io).is_empty(), "{:#?}", io.runs());
        let want: Vec<PrivOp> = ["cosmic-wallpapers", "cosmic-icon-theme"]
            .iter()
            .map(|p| PrivOp::PkgInstall {
                family: PkgFamily::Arch,
                packages: vec![p.to_string()],
                source: PkgSource::DesktopRepair,
            })
            .collect();
        assert_eq!(io.priv_ops.borrow().as_slice(), want.as_slice());
        // Each name passes the helper's check against this table.
        for op in &want {
            let PrivOp::PkgInstall { family, packages, source } = op else { unreachable!() };
            assert!(crate::privilege::validate::check_install(*family, packages, source.install_source("cosmic")).is_ok());
        }
    }

    /// Password refusal: one password window, one explanation; the second
    /// package no longer asks for the password.
    #[test]
    fn completeness_password_refused_asks_once() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec") {
                out(126, "", "Error executing command as another user: Request dismissed")
            } else {
                healthy(argv)
            }
        }));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert!(!done.outcome.cancelled);
        assert_eq!(done.outcome.message, completeness_denied_message());
        assert!(done.outcome.message.starts_with("The administrator password wasn't confirmed"));
        // The button caption comes from the «Settings» catalog in the UI language.
        let button = crate::qt_settings::text("compl.button", &[]).unwrap();
        assert!(done.outcome.message.contains(&format!("“{button}”")));
        assert_eq!(pkexec_runs(&io).len(), 1, "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
    }

    /// A package failure: the second package is still installed, the result
    /// names in words only what did not install, and the package manager's
    /// reason is in the progress.
    #[test]
    fn completeness_package_failure_names_what_is_left() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = cosmic_io(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("pkexec")
                && argv.last().map(String::as_str) == Some("cosmic-wallpapers")
            {
                out(1, "", "error: target not found: cosmic-wallpapers")
            } else {
                healthy(argv)
            }
        }));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, rec) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert_eq!(
            done.outcome.message,
            completeness_partial_message(&[comp::fix_label("cosmic_wallpapers").unwrap().to_string()])
        );
        assert_eq!(pkexec_runs(&io).len(), 2);
        let warnings = rec.messages(de_progress::STAGE_WARNING);
        assert_eq!(warnings, vec!["Didn't work: error: target not found: cosmic-wallpapers"]);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
    }

    /// Cancellation before the first password: nothing was run, the result is
    /// "cancelled". And while waiting for someone else's lock too.
    #[test]
    fn completeness_cancel_before_password_changes_nothing() {
        let io = cosmic_io(Box::new(healthy));
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let token = CancelToken::new();
        token.cancel();
        let (done, _) = complete(&io, &mut checks, &s.fixes, token);
        assert!(done.outcome.cancelled, "{done:?}");
        assert!(pkexec_runs(&io).is_empty());

        let mut io = cosmic_io(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (done, rec) = complete(&io, &mut checks, &s.fixes, token);
        assert!(done.outcome.cancelled, "{done:?}");
        assert!(pkexec_runs(&io).is_empty());
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1);
    }

    /// The lock never got released: no password asked, the result is the core's text.
    #[test]
    fn completeness_lock_never_freed_does_not_ask_password() {
        let io = cosmic_io(Box::new(healthy));
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let mut checks = FakeChecks::with(&["gnome-keyring"], &[]);
        let s = survey(&io, &mut checks);
        let (done, _) = complete(&io, &mut checks, &s.fixes, CancelToken::new());
        assert!(!done.outcome.ok);
        assert_eq!(done.outcome.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(pkexec_runs(&io).is_empty());
    }

    /// ⭐ Caption completeness guard: EVERY repair the check can defer has a
    /// caption for the question window, and the table has no extra ones. The
    /// checks of all desktops run on an "empty" machine with nothing installed.
    #[test]
    fn every_deferred_fix_has_a_label() {
        let mut deferred = std::collections::BTreeSet::new();
        for de in ["mate", "kde", "gnome", "cosmic", "xfce", "cinnamon-mint", "budgie", "hyprland"] {
            assert!(comp::has_checks(de), "{de}");
            let mut checks = FakeChecks::with(&[], &["git", "yay"]);
            // A panel that still has a working Brisk menu (an earlier release put it there).
            checks.panel = Some(
                "[general]\nobject-id-list=['brisk']\n\n[objects/brisk]\napplet-iid='BriskMenuFactory::BriskMenu'\n".into(),
            );
            checks.applets =
                vec![("brisk.mate-panel-applet".into(), "[Applet Factory]\nId=BriskMenuFactory\n\n[BriskMenu]\n".into())];
            let mut log = Vec::new();
            let results = comp::Compliance::new(&mut checks, "/home/t", PkgFamily::Arch, false, false, false)
                .run_for_de(de, &mut log);
            for (key, label) in comp::deferred_fixes(&results) {
                assert!(comp::fix_label(&key).is_some(), "{de}: у починки «{key}» нет подписи");
                assert_ne!(key, label);
                deferred.insert(key);
            }
        }
        let labelled: std::collections::BTreeSet<String> =
            comp::FIX_LABELS.iter().map(|(k, _)| k.to_string()).collect();
        assert_eq!(deferred, labelled, "таблица подписей разошлась с проверками");
    }

    /// English progress texts of desktop operations have no Russian letters and
    /// read as phrases (translation rule: Russian only in `po/ru.po`).
    #[test]
    fn english_de_op_texts_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        let e = "E";
        let failed = RunOut { outcome: RunOutcome::Timeout, ..out(1, "", "") };
        let labels = vec!["COSMIC wallpapers".to_string()];
        let texts = [
            unknown_de_message("xyz"),
            cancelled_message(),
            status_cancelled(),
            busy_message(),
            dialog_yes(),
            dialog_no(),
            dialog_cancel(),
            awaiting_answer("Install KDE Plasma"),
            snapshot_message(),
            panel_backup_message(),
            post_install_message(),
            dm_step_message(),
            restore_message(),
            mime_message(),
            snapshot_failed(e),
            panel_backup_failed("MATE", e),
            install_intro(1, 0),
            install_intro(5, 2),
            post_install_failed(e),
            dm_already("sddm"),
            dm_enabled("sddm.service"),
            dm_enable_failed("sddm.service", e),
            switcher_message("KDE Plasma"),
            restore_failed(e),
            installed_message("KDE Plasma"),
            temp_failed(e),
            write_failed("/tmp/x", e),
            unknown_step_message(e),
            gentoo_overlay_intro("guru"),
            gentoo_overlay_done("guru"),
            gentoo_overlay_failed("guru"),
            ensure_dm_install_failed("sddm", e),
            ensure_dm_enable_failed(e),
            run_why(&failed),
            run_why(&out(3, "", "")),
            removing_message("KDE Plasma"),
            removed_message("KDE Plasma"),
            remove_timeout_message(),
            logout_message(),
            logout_failed_message(),
            config_backup_message("KDE Plasma"),
            config_backup_done("kde-2026", 1),
            config_backup_done("kde-2026", 3),
            config_backup_failed(e),
            logout_timeout("qdbus6"),
            logout_not_started("qdbus6"),
            completeness_operation_title(),
            completeness_confirm_title(),
            completeness_checking_message(),
            completeness_done_message(),
            completeness_confirm_text(&labels),
            completeness_partial_message(&labels),
            completeness_denied_message(),
            fix_failed_message(e),
        ];
        for text in &texts {
            assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
        }
        assert_eq!(install_intro(1, 0), "📦 Installing: 1 package");
        assert_eq!(install_intro(5, 2), "📦 Installing: 5 packages + 2 from the AUR");
        assert_eq!(config_backup_done("kde-2026", 1), "✅ Copy saved: kde-2026 (1 file)");
        assert_eq!(run_why(&out(3, "", "")), "code 3");
        assert_eq!(unknown_de_message("xyz"), "Rexon doesn't know the “xyz” desktop.");
    }

    /// Install-missing texts have no service words; progress does not show argv.
    #[test]
    fn completeness_texts_have_no_jargon() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let labels: Vec<String> = comp::FIX_LABELS.iter().map(|(_, l)| l.to_string()).collect();
        let texts = [
            completeness_confirm_text(&labels),
            completeness_partial_message(&labels),
            completeness_denied_message(),
            completeness_operation_title(),
            completeness_confirm_title(),
            completeness_checking_message(),
            completeness_done_message(),
            fix_step_message(&s(&["git", "clone", "--depth", "1", "https://x", "/tmp/r"])),
            fix_step_message(&s(&["pkexec", plan::STAGE_HELPER_PATH, "mate-layouts", "rexon", "/tmp/s"])),
            fix_step_message(&s(&["pkexec", "bash", "/tmp/rexon_layouts_1.sh"])),
            fix_step_message(&s(&["yay", "-S", "--needed", "--noconfirm", "mint-themes"])),
            fix_step_message(&s(&["pkexec", plan::PKG_HELPER_PATH, "arch", "install", "budgie-extras"])),
        ];
        for text in &texts {
            for jargon in ["pkexec", "needs_action", "helper", "хелпер", "/tmp", "/usr", "--", "git", "DE", "compliance"] {
                assert!(!text.contains(jargon), "жаргон «{jargon}»: {text}");
            }
        }
        assert_eq!(texts[10], "📦 Installing from the AUR: mint-themes");
        assert_eq!(texts[11], "📦 Installing: budgie-extras");
    }

    // ── The AeroShell look ───────────────────────────────────────────────

    const EFFECT_SO: &str = "/usr/lib/qt6/plugins/kwin/effects/plugins/aeroglide.so";
    const KWIN_COMPONENTS: &str = "aeroshell-kwin-components-git";
    const LIBPLASMA: &str = "aeroshell-libplasma-git";

    /// A machine with the look: the effect module built for KWin 6.7.4, the
    /// libplasma fork for Plasma 6.7.4, an icon package without built code.
    /// `kwin` is the KWin version installed now; `build` is the builder's answer.
    fn aero_io(kwin: &'static str, build: Responder) -> FakeIo {
        FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(
                    0,
                    &format!("bash\n{KWIN_COMPONENTS}\n{LIBPLASMA}\naerothemeplasma-icons-git\nkwin\n"),
                    "",
                ),
                ["pacman", "-Q", "kwin"] => out(0, &format!("kwin {kwin}-1\n"), ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.4-1.1\n", ""),
                ["pacman", "-Ql", KWIN_COMPONENTS] => {
                    out(0, &format!("{KWIN_COMPONENTS} {EFFECT_SO}\n"), "")
                }
                ["pacman", "-Ql", LIBPLASMA] => out(
                    0,
                    &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.4\n{LIBPLASMA} /usr/lib/libPlasma.so.7\n"),
                    "",
                ),
                ["pacman", "-Ql", "aerothemeplasma-icons-git"] => {
                    out(0, "aerothemeplasma-icons-git /usr/share/icons/a.png\n", "")
                }
                ["env", "LANG=C", "LC_ALL=C", ..] => build(argv),
                _ => healthy(argv),
            }
        }))
    }

    fn aero_survey(io: &FakeIo) -> aeroshell::Report {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new()).aeroshell_survey(
            &|path: &str| {
                (path == EFFECT_SO).then(|| b"\0org.kde.kwin.EffectPluginFactory6.7.4\0".to_vec())
            },
        )
    }

    fn aero_rebuild(io: &FakeIo, packages: &[String], cancel: CancelToken) -> (DeOpOutcome, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let outcome = DeOpsExec::new(io, sink, cancel).rebuild_aeroshell(packages);
        (outcome, rec)
    }

    fn builder_runs(io: &FakeIo) -> Vec<String> {
        io.runs().into_iter().filter(|r| r.starts_with("env ")).collect()
    }

    fn built(_: &[String]) -> RunOut {
        out(0, "==> Making package: aeroshell-libplasma-git\n\n==> Finished making\n", "")
    }

    /// A machine like the owner's on 04.10.2026: only the libplasma fork is left
    /// from the set, no shell, versions match. `si_libplasma`: whether the stock
    /// libplasma is in the repositories; `remove`: the rollback transaction answer.
    fn lonely_io(si_libplasma: bool, remove: Responder) -> FakeIo {
        FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("bash\n{LIBPLASMA}\nkwin\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.7.5-1.1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.5-1.1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => out(
                    0,
                    &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n{LIBPLASMA} /usr/lib/libPlasma.so.7\n"),
                    "",
                ),
                ["pacman", "-Qi", LIBPLASMA] => out(
                    0,
                    "Name            : aeroshell-libplasma-git\n\
                     Provides        : libplasma  aeroshell-libplasma\n\
                     Replaces        : plasma-framework\n",
                    "",
                ),
                ["pacman", "-Si", "libplasma"] if si_libplasma => {
                    out(0, "Repository      : extra\nName            : libplasma\nVersion         : 6.7.5-1\n", "")
                }
                ["pacman", "-Si", _] => out(1, "", "error: package 'x' was not found"),
                ["pkexec", ..] => remove(argv),
                _ => healthy(argv),
            }
        }))
    }

    fn lonely_advice(io: &FakeIo) -> (aeroshell::Report, aeroshell::Advice) {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .aeroshell_advice(&|_p: &str| None, &|_p: &str| None)
    }

    /// Lonely fork: the advice is to restore stock Plasma, there is no rebuild,
    /// and the password is not asked before «Yes».
    #[test]
    fn lonely_fork_advises_restore_without_touching_anything() {
        let io = lonely_io(true, Box::new(|_| out(0, "", "")));
        let (report, advice) = lonely_advice(&io);
        assert!(report.lonely_fork && !report.needs_rebuild, "{report:?}");
        assert_eq!(advice, aeroshell::Advice::RestoreStock { forks: vec![LIBPLASMA.to_string()] });
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ")), "{runs:#?}");
        assert!(runs.iter().any(|r| r == "pacman -Si libplasma"), "{runs:#?}");
    }

    /// Session journal: KWin called the set's effect incompatible, although the
    /// versions in the files match: the advice is "rebuild", with versions from
    /// the journal; a third-party widget is named on a separate line. Only the
    /// user journal and the package database are read, no root.
    #[test]
    fn advice_reads_session_journal() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let journal = [
            serde_json::json!({"MESSAGE": "Effect \"aeroglide\" was built for a different version of KWin and needs to be rebuilt, expected \"org.kde.kwin.EffectPluginFactory6.7.4\" got \"org.kde.kwin.EffectPluginFactory6.7.3\"", "__REALTIME_TIMESTAMP": "500"}),
            serde_json::json!({"MESSAGE": "error when loading applet \"Nori.Weather\" QList(\"x\", \"y\")", "__REALTIME_TIMESTAMP": "600"}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        let mut io = aero_io(
            "6.7.4",
            Box::new(|_| out(0, "", "")),
        );
        let inner = std::mem::replace(&mut io.responder, Box::new(|_| out(0, "", "")));
        io.responder = Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["journalctl", "--user", "-b", ..] => out(0, &journal, ""),
                ["pacman", "-Qq"] => out(
                    0,
                    &format!("{KWIN_COMPONENTS}\n{LIBPLASMA}\naerothemeplasma-desktop-git\nkwin\n"),
                    "",
                ),
                ["pacman", "-Qo", ..] => out(0, &format!("{EFFECT_SO} is owned by {KWIN_COMPONENTS} 6.7.0_61-1\n"), ""),
                ["pacman", "-Qm"] => out(0, &format!("{KWIN_COMPONENTS} 6.7.0_61-1\n{LIBPLASMA} 6.7.4-1\n"), ""),
                _ => inner(argv),
            }
        });
        io.bins.insert("journalctl".to_string());
        let (report, advice) = DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new()).aeroshell_advice(
            &|path: &str| (path == EFFECT_SO).then(|| b"\0org.kde.kwin.EffectPluginFactory6.7.4\0".to_vec()),
            &|path: &str| (path == EFFECT_SO).then_some(100),
        );
        assert!(report.stale.contains(&KWIN_COMPONENTS.to_string()), "{report:#?}");
        let v = report.packages.iter().find(|p| p.package == KWIN_COMPONENTS).unwrap();
        assert_eq!((v.built_for.as_deref(), v.installed.as_deref()), (Some("6.7.3"), Some("6.7.4")));
        assert_eq!(report.journal.len(), 2, "{:#?}", report.journal);
        let lines = aeroshell::journal_lines(&report);
        assert!(lines[0].contains("KWin 6.7.3") && lines[0].contains(KWIN_COMPONENTS), "{lines:?}");
        assert!(lines[1].contains("Nori.Weather") && lines[1].contains("isn't installed anymore"), "{lines:?}");
        assert!(!matches!(advice, aeroshell::Advice::Nothing), "{advice:?}");
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ") || r.starts_with("journalctl ")), "{runs:#?}");
        assert!(runs.iter().any(|r| r.starts_with("journalctl --user -b") && r.contains("--grep=")), "{runs:#?}");
    }

    /// Stock libplasma is not in the repositories: a refusal in words right away.
    #[test]
    fn lonely_fork_without_stock_package_is_blocked() {
        let io = lonely_io(false, Box::new(|_| out(0, "", "")));
        let (_, advice) = lonely_advice(&io);
        assert!(matches!(advice, aeroshell::Advice::RestoreBlocked { .. }), "{advice:?}");
    }

    /// «Yes» → one rollback transaction under one password, progress in the window.
    #[test]
    fn restore_stock_runs_one_transaction() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = lonely_io(true, Box::new(|_| out(0, "готово\n", "")));
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        let done = DeOpsExec::new(&io, sink, CancelToken::new())
            .restore_stock_plasma(&[LIBPLASMA.to_string()]);
        assert!(done.ok, "{done:?}");
        let pkexecs: Vec<String> = io.runs().into_iter().filter(|r| r.starts_with("pkexec")).collect();
        assert_eq!(pkexecs.len(), 1, "{pkexecs:#?}");
        assert!(pkexecs[0].contains("pacman -S --noconfirm --needed libplasma"), "{pkexecs:?}");
        assert_eq!(rec.messages("install"), vec![aeroshell::restore_start_message()]);
        assert_eq!(rec.messages("done"), vec![aeroshell::restore_done_message()]);
    }

    /// The shell is installed, Plasma moved to 6.8, and the fork branch is still
    /// 6.7: the advice is to wait for the authors, not to rebuild. `fork_cmake`
    /// is the raw file answer (None: network unavailable).
    fn upstream_io(fork_cmake: Option<&'static str>) -> FakeIo {
        let mut io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("{LIBPLASMA}\naerothemeplasma-desktop-git\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.8.0-1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.8.0-1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => {
                    out(0, &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n"), "")
                }
                ["pacman", "-Qi", LIBPLASMA] => {
                    out(0, "Name            : x\nURL             : https://gitgud.io/aeroshell/libplasma\n", "")
                }
                ["pacman", "-Si", "libplasma"] => out(0, "Version         : 6.8.0-1\n", ""),
                ["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", url, "HEAD"] => {
                    assert_eq!(*url, "https://gitgud.io/aeroshell/libplasma.git");
                    out(0, "ref: refs/heads/Plasma/6.7\tHEAD\n6ed0921\tHEAD\n", "")
                }
                ["curl", "-fsSL", "-m", _, url] => {
                    assert_eq!(*url, "https://gitgud.io/aeroshell/libplasma/-/raw/Plasma/6.7/CMakeLists.txt");
                    match fork_cmake {
                        Some(text) => out(0, text, ""),
                        None => out(6, "", "curl: (6) Could not resolve host"),
                    }
                }
                _ => healthy(argv),
            }
        }));
        io.bins.extend(["git", "curl"].map(String::from));
        io
    }

    #[test]
    fn rebuild_waits_while_fork_branch_lags_behind() {
        let io = upstream_io(Some("cmake_minimum_required(VERSION 3.16)\nset(PROJECT_VERSION \"6.7.5\")\n"));
        let (report, advice) = lonely_advice(&io);
        assert!(report.needs_rebuild && !report.lonely_fork, "{report:?}");
        assert_eq!(
            advice,
            aeroshell::Advice::WaitForUpstream { plasma: "6.8.0".into(), fork: "6.7.5".into() }
        );
        assert!(!io.runs().iter().any(|r| r.contains("pkexec") || r.starts_with("env LANG")));
    }

    #[test]
    fn rebuild_offered_once_fork_caught_up() {
        let io = upstream_io(Some("set(PROJECT_VERSION \"6.8.0\")\n"));
        let (_, advice) = lonely_advice(&io);
        assert_eq!(advice, aeroshell::Advice::Rebuild { packages: vec![LIBPLASMA.to_string()] });
    }

    /// No network: the check is silently skipped, the advice is unchanged.
    #[test]
    fn rebuild_offered_without_network() {
        let io = upstream_io(None);
        let (_, advice) = lonely_advice(&io);
        assert_eq!(advice, aeroshell::Advice::Rebuild { packages: vec![LIBPLASMA.to_string()] });
    }

    /// Everything works now (6.7.5), and `checkupdates` sees 6.8.0 in the
    /// repositories. `checkupdates` is its answer (None: no program),
    /// `fork_cmake` is the `CMakeLists.txt` of the fork's default branch.
    fn before_update_io(checkupdates: Option<(i32, &'static str)>, fork_cmake: &'static str) -> FakeIo {
        let mut io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Qq"] => out(0, &format!("{LIBPLASMA}\naerothemeplasma-desktop-git\n"), ""),
                ["pacman", "-Q", "kwin"] => out(0, "kwin 6.7.5-1.1\n", ""),
                ["pacman", "-Q", "plasma-workspace"] => out(0, "plasma-workspace 6.7.5-1.1\n", ""),
                ["pacman", "-Ql", LIBPLASMA] => {
                    out(0, &format!("{LIBPLASMA} /usr/lib/libPlasma.so.6.7.5\n"), "")
                }
                ["pacman", "-Qi", LIBPLASMA] => {
                    out(0, "Name            : x\nURL             : https://gitgud.io/aeroshell/libplasma\n", "")
                }
                ["checkupdates"] => {
                    let (rc, text) = checkupdates.expect("checkupdates не должен зваться");
                    out(rc, text, if rc == 1 { "==> ERROR: Cannot fetch updates" } else { "" })
                }
                ["env", "GIT_TERMINAL_PROMPT=0", "git", "ls-remote", "--symref", _, "HEAD"] => {
                    out(0, "ref: refs/heads/Plasma/6.7\tHEAD\n6ed0921\tHEAD\n", "")
                }
                ["curl", "-fsSL", "-m", _, _] => out(0, fork_cmake, ""),
                _ => healthy(argv),
            }
        }));
        io.bins.extend(["git", "curl"].map(String::from));
        if checkupdates.is_some() {
            io.bins.insert("checkupdates".to_string());
        }
        io
    }

    const PENDING_68: &str = "kwin 6.7.5-1.1 -> 6.8.0-1\nplasma-workspace 6.7.5-1.1 -> 6.8.0-1\n";

    /// A new series in the repositories, the authors still on 6.7: "do not
    /// update"; nothing is installed and no password is asked.
    #[test]
    fn pending_plasma_series_with_lagging_fork_says_hold() {
        let io = before_update_io(Some((0, PENDING_68)), "set(PROJECT_VERSION \"6.7.5\")\n");
        let (report, advice) = lonely_advice(&io);
        assert!(!report.needs_rebuild && !report.lonely_fork, "{report:?}");
        assert_eq!(advice, aeroshell::Advice::HoldUpdate { plasma: "6.8.0".into(), fork: "6.7.5".into() });
        let runs = io.runs();
        assert!(runs.iter().any(|r| r == "checkupdates"), "{runs:?}");
        assert!(!runs.iter().any(|r| r.contains("pkexec") || r.contains("sudo") || r.contains("-Sy")), "{runs:?}");
    }

    /// The authors are already on 6.8 or their version could not be found out:
    /// softly, a rebuild will be needed after the update.
    #[test]
    fn pending_plasma_series_caught_up_or_unknown_is_soft() {
        let soft = aeroshell::Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.8.0".into() };
        let io = before_update_io(Some((0, PENDING_68)), "set(PROJECT_VERSION \"6.8.0\")\n");
        assert_eq!(lonely_advice(&io).1, soft);
        // The branch's raw file did not parse (a foreign answer): the version is unknown.
        let io = before_update_io(Some((0, PENDING_68)), "<html>not found</html>");
        assert_eq!(lonely_advice(&io).1, soft);
    }

    /// A point release: softly, and without going to the authors over the network.
    #[test]
    fn pending_point_release_is_soft_without_network() {
        let io = before_update_io(
            Some((0, "kwin 6.7.5-1.1 -> 6.7.6-1\nplasma-workspace 6.7.5-1.1 -> 6.7.6-1\n")),
            "set(PROJECT_VERSION \"6.7.5\")\n",
        );
        assert_eq!(
            lonely_advice(&io).1,
            aeroshell::Advice::RebuildAfterUpdate { from: "6.7.5".into(), to: "6.7.6".into() }
        );
        assert!(!io.runs().iter().any(|r| r.contains("git ls-remote") || r.starts_with("curl")));
    }

    /// No `checkupdates`, it failed (code 1) or there are no updates (code 2):
    /// we stay silent and do not touch the system database.
    #[test]
    fn no_checkupdates_or_failure_is_quiet() {
        for answer in [None, Some((1, "")), Some((2, ""))] {
            let io = before_update_io(answer, "set(PROJECT_VERSION \"6.7.5\")\n");
            assert_eq!(lonely_advice(&io).1, aeroshell::Advice::Nothing, "{answer:?}");
            assert!(!io.runs().iter().any(|r| r.contains("git ls-remote")), "{answer:?}");
        }
    }

    /// Watching: installed once (files + daemon-reload + enable), a repeat does
    /// nothing, removal is disable, files, daemon-reload.
    #[test]
    fn update_watch_installs_once_and_removes() {
        use crate::update_guard as ug;
        let mut io = FakeIo::arch(Box::new(healthy));
        io.bins.insert("systemctl".to_string());
        let ex = DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new());
        let home_cfg = ex.config_home();
        let dir = ug::user_unit_dir(&home_cfg);
        let exec = "/usr/lib/rexon/rexon-main-window --update-check";
        let program = Some("/usr/lib/rexon/rexon-main-window");
        ex.update_watch(true, Some(exec), program).unwrap();
        let path_unit = io.files.borrow().get(&format!("{dir}/{}", ug::WATCH_PATH_UNIT)).cloned().unwrap();
        assert!(path_unit.contains("PathChanged=/var/lib/pacman/local"), "{path_unit}");
        let service = io.files.borrow().get(&format!("{dir}/{}", ug::WATCH_SERVICE_UNIT)).cloned().unwrap();
        assert!(service.contains("ExecStart=/usr/lib/rexon/rexon-main-window --update-check"), "{service}");
        let sc = |io: &FakeIo| io.runs().into_iter().filter(|r| r.starts_with("systemctl")).collect::<Vec<_>>();
        assert_eq!(
            sc(&io),
            vec![
                "systemctl --user daemon-reload".to_string(),
                "systemctl --user enable --now rexon-update-check.path rexon-update-check.timer".to_string(),
            ]
        );
        // A repeat with the same: no writes, no systemctl.
        ex.update_watch(true, Some(exec), program).unwrap();
        assert_eq!(sc(&io).len(), 2);
        // Removal.
        ex.update_watch(false, None, None).unwrap();
        let runs = sc(&io);
        assert_eq!(runs[2], "systemctl --user disable --now rexon-update-check.path rexon-update-check.timer");
        assert_eq!(runs[3], "systemctl --user daemon-reload");
        assert!(!io.files.borrow().contains_key(&format!("{dir}/{}", ug::WATCH_PATH_UNIT)));
        // Nothing to remove: nothing is called.
        ex.update_watch(false, None, None).unwrap();
        assert_eq!(sc(&io).len(), 4);
        // Old AeroThemePlasma units are removed, the shared ones installed.
        io.files.borrow_mut().insert(format!("{dir}/rexon-aeroshell-check.timer"), String::new());
        ex.update_watch(true, Some(exec), program).unwrap();
        let runs = sc(&io);
        assert_eq!(runs[4], "systemctl --user disable --now rexon-aeroshell-check.timer");
        assert_eq!(runs[5], "systemctl --user daemon-reload");
        assert!(!io.files.borrow().contains_key(&format!("{dir}/rexon-aeroshell-check.timer")));
    }

    /// Everything is fresh: the check only asks pacman, no window, no build.
    #[test]
    fn aeroshell_up_to_date_asks_nothing() {
        let io = aero_io("6.7.4", Box::new(built));
        let report = aero_survey(&io);
        assert!(!report.needs_rebuild, "{report:?}");
        assert!(report.stale.is_empty());
        assert_eq!(report.packages.len(), 3, "{report:?}");
        assert_eq!(report.kwin_version.as_deref(), Some("6.7.4"));
        let runs = io.runs();
        assert!(runs.iter().all(|r| r.starts_with("pacman ")), "{runs:#?}");
    }

    /// Not Arch: there are no look packages there, pacman is not asked.
    #[test]
    fn aeroshell_survey_off_arch_is_empty() {
        let io = aero_io("6.7.5", Box::new(built));
        io.files.borrow_mut().insert(
            "/etc/os-release".to_string(),
            "ID=fedora\nNAME=Fedora\n".to_string(),
        );
        let report = aero_survey(&io);
        assert_eq!(report, aeroshell::Report::empty());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }

    /// Outdated → plan → «Yes» → success: the whole built set is rebuilt with
    /// one command, build progress goes to the window, the temporary log is removed.
    #[test]
    fn aeroshell_stale_plan_then_success() {
        // Texts come from the translation catalog; the source language (Russian is
        // checked by the core's checks on po/ru.po).
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        let report = aero_survey(&io);
        assert!(report.needs_rebuild);
        assert_eq!(report.stale, vec![KWIN_COMPONENTS.to_string()]);
        assert_eq!(report.rebuild, vec![KWIN_COMPONENTS.to_string(), LIBPLASMA.to_string()]);
        let details = aeroshell::report_details(&report);
        assert!(details.contains("built for 6.7.4, installed 6.7.5 — outdated"), "{details}");
        assert!(builder_runs(&io).is_empty(), "сборка до «Да»");

        let (done, rec) = aero_rebuild(&io, &report.rebuild, CancelToken::new());
        assert!(done.ok, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_success_message());
        assert_eq!(
            builder_runs(&io),
            vec![format!(
                "env LANG=C LC_ALL=C yay -S --rebuild --diffmenu=false --noconfirm --sudo pkexec \
                 --sudoflags  {KWIN_COMPONENTS} {LIBPLASMA}"
            )]
        );
        assert_eq!(rec.messages("install"), vec![aeroshell::rebuild_start_message(2)]);
        assert_eq!(
            rec.messages(aeroshell::REBUILD_LINE_STAGE),
            vec!["==> Making package: aeroshell-libplasma-git", "==> Finished making"]
        );
        assert_eq!(rec.messages("done"), vec![aeroshell::rebuild_done_message()]);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 0);
        let flags = rec.cancellable.lock().unwrap().clone();
        assert_eq!(flags.first(), Some(&true));
        assert_eq!(flags.last(), Some(&false));
        let run = io.index_of("run: env LANG=C").unwrap();
        assert!(io.index_of("remove: /tmp/rexon_aeroshell_").unwrap() > run);
    }

    /// paru and devtools present: a build in a clean environment.
    #[test]
    fn aeroshell_rebuild_prefers_paru_chroot() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.extend(["yay", "paru", "mkarchroot"].map(String::from));
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.ok, "{done:?}");
        let runs = builder_runs(&io);
        assert_eq!(runs.len(), 1);
        assert!(runs[0].starts_with("env LANG=C LC_ALL=C paru -S --rebuild --chroot --skipreview"), "{runs:?}");
    }

    /// Neither paru nor yay: a refusal before the first side effect: no lock, no
    /// builder, one explanation.
    #[test]
    fn aeroshell_no_builder_refuses_before_anything() {
        let io = aero_io("6.7.5", Box::new(built));
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_no_helper());
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);
        assert_eq!(rec.count("install"), 0);
    }

    /// Nothing to ask for the password with: the core's explanation with the package name.
    #[test]
    fn aeroshell_without_polkit_refuses() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.bins.remove("pkexec");
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok);
        assert_eq!(done.message, SystemExec::new(&io).polkit_missing_message());
        assert!(builder_runs(&io).is_empty());
    }

    /// Password refusal: one explanation in words instead of the builder's tail.
    #[test]
    fn aeroshell_password_refused_says_so_once() {
        let mut io = aero_io(
            "6.7.5",
            Box::new(|_: &[String]| {
                out(
                    1,
                    "==> Finished making: aeroshell-libplasma-git\n",
                    "Error executing command as another user: Request dismissed\n \
                     -> error installing repo packages: exit status 126\n",
                )
            }),
        );
        io.bins.insert("yay".to_string());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled, "{done:?}");
        assert_eq!(done.message, aeroshell::rebuild_denied_message());
        assert_eq!(builder_runs(&io).len(), 1);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1, "{:#?}", rec.stages());
    }

    /// Build failure: first "the system is intact", then the output tail; without
    /// output, at least the exit code.
    #[test]
    fn aeroshell_build_failure_keeps_tail() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = aero_io(
            "6.7.5",
            Box::new(|_: &[String]| out(1, "==> ERROR: A failure occurred in build().\n    Aborting...\n", "")),
        );
        io.bins.insert("yay".to_string());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok);
        assert!(done.message.starts_with(&aeroshell::rebuild_failed_head()), "{}", done.message);
        // The main thing in a failure is that the system is intact; the text is the one aeroshell takes.
        let intact = crate::de_progress::aeroshell_fail_note();
        assert!(done.message.contains(&intact), "{}", done.message);
        assert!(done.message.ends_with("==> ERROR: A failure occurred in build().\n    Aborting..."), "{}", done.message);
        assert_eq!(rec.count(de_progress::STAGE_ERROR), 1);

        let mut io = aero_io("6.7.5", Box::new(|_: &[String]| out(2, "", "")));
        io.bins.insert("yay".to_string());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.message.ends_with(&aeroshell::rebuild_exit_code_message(2)), "{}", done.message);

        let mut io = aero_io("6.7.5", Box::new(|_: &[String]| RunOut::missing()));
        io.bins.insert("yay".to_string());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(done.message.starts_with("Couldn't start yay"), "{}", done.message);
    }

    /// Cancellation before the build: nothing was run; and while waiting for
    /// someone else's lock too.
    #[test]
    fn aeroshell_cancel_before_build_changes_nothing() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        let token = CancelToken::new();
        token.cancel();
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], token);
        assert!(done.cancelled, "{done:?}");
        assert_eq!(done.message, cancelled_message());
        assert!(builder_runs(&io).is_empty());

        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let token = CancelToken::new();
        io.cancel_on_sleep = Some(token.clone());
        let (done, rec) = aero_rebuild(&io, &[LIBPLASMA.to_string()], token);
        assert!(done.cancelled, "{done:?}");
        assert!(builder_runs(&io).is_empty());
        assert_eq!(rec.count(plan::PKG_LOCK_WAIT_STAGE), 1);
    }

    /// The lock never got released: the builder does not run, the result is the core's text.
    #[test]
    fn aeroshell_lock_never_freed_does_not_build() {
        let mut io = aero_io("6.7.5", Box::new(built));
        io.bins.insert("yay".to_string());
        io.files
            .borrow_mut()
            .insert("/var/lib/pacman/db.lck".to_string(), String::new());
        let (done, _) = aero_rebuild(&io, &[LIBPLASMA.to_string()], CancelToken::new());
        assert!(!done.ok && !done.cancelled);
        assert_eq!(done.message, plan::pkg_lock_timeout_message(plan::PKG_LOCK_TIMEOUT_SECS));
        assert!(builder_runs(&io).is_empty());
    }

    // ── First capture of the application protection reference ───────────
    //
    // The «Default application protection» section is on by default, but until
    // the first reference there is nothing to compare a hijack against. Both
    // decision branches are checked, and that a second run does NOT rewrite the
    // reference.

    /// A fake profile without a reference: `xdg-mime` answers any query, the
    /// terminal comes from `xdg-terminals.list`.
    fn protector_io() -> FakeIo {
        let mut io = FakeIo::arch(Box::new(|argv: &[String]| {
            if argv.first().map(String::as_str) == Some("xdg-mime") {
                return out(0, "some-app.desktop\n", "");
            }
            out(1, "", "")
        }));
        io.files.borrow_mut().insert(
            "/home/t/.config/xdg-terminals.list".to_string(),
            "konsole.desktop\n".to_string(),
        );
        io.env.insert("XDG_CURRENT_DESKTOP".to_string(), "KDE".to_string());
        io
    }

    const SNAPSHOT_FILE: &str = "/home/t/.config/rexon/protected_defaults.json";

    fn snapshot_if_never(io: &FakeIo) -> Option<Result<usize, String>> {
        DeOpsExec::new(io, Arc::new(Recorder::default()), CancelToken::new())
            .defaults_snapshot_if_never()
    }

    /// The decision is a pure core function: both branches.
    #[test]
    fn startup_snapshot_decision_both_ways() {
        let rules = dfl::Rules::defaults();
        // Nothing remembered: capture.
        assert!(dfl::should_snapshot_at_startup(&rules, &[]));
        // A reference already exists: do not touch.
        let remembered = vec![("file_manager".to_string(), Some("dolphin.desktop".to_string()))];
        assert!(!dfl::should_snapshot_at_startup(&rules, &remembered));
        // Protection is off: do not capture, even if nothing is remembered.
        let off = rules.updated(&serde_json::from_str(r#"{"enabled": false}"#).unwrap());
        assert!(!dfl::should_snapshot_at_startup(&off, &[]));
    }

    /// Clean profile: the reference is captured automatically and is not empty.
    #[test]
    fn startup_snapshot_taken_on_clean_profile() {
        let io = protector_io();
        let count = snapshot_if_never(&io).expect("на чистом профиле снимаем").unwrap();
        assert!(count > 0, "эталон пуст: {count}");
        let text = io.files.borrow().get(SNAPSHOT_FILE).cloned().expect("файл эталона");
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(dfl::load_protected_apps(&parsed).len(), count);
        // The terminal is asked too, and it is from the xdg list.
        assert_eq!(
            dfl::load_protected_apps(&parsed)
                .iter()
                .find(|(k, _)| k == "terminal")
                .and_then(|(_, v)| v.clone()),
            Some("konsole.desktop".to_string())
        );
    }

    /// Second run on the same profile: the reference is NOT rewritten and the
    /// system is not asked anything.
    #[test]
    fn startup_snapshot_not_retaken() {
        let io = protector_io();
        snapshot_if_never(&io).expect("первый запуск снимает").unwrap();
        let first = io.files.borrow().get(SNAPSHOT_FILE).cloned().unwrap();
        io.calls.borrow_mut().clear();
        // The fake executor's clock runs: on a recapture `last_updated` would
        // differ, and the content comparison would catch it.
        io.clock.set(io.clock.get() + 3600.0);

        assert!(snapshot_if_never(&io).is_none(), "второй раз снимать нечего");
        assert_eq!(io.files.borrow().get(SNAPSHOT_FILE).cloned().unwrap(), first);
        assert!(io.runs().is_empty(), "лишние запросы к системе: {:#?}", io.runs());
        assert!(
            !io.calls.borrow().iter().any(|c| c.starts_with("write:")),
            "эталон переписан: {:#?}",
            io.calls.borrow()
        );
    }

    /// The reference was deleted by hand: it fixes itself on the next run.
    #[test]
    fn startup_snapshot_restored_after_deletion() {
        let io = protector_io();
        snapshot_if_never(&io).expect("первый запуск снимает").unwrap();
        io.files.borrow_mut().remove(SNAPSHOT_FILE);
        let count = snapshot_if_never(&io).expect("эталона нет — снимаем снова").unwrap();
        assert!(count > 0);
        assert!(io.files.borrow().contains_key(SNAPSHOT_FILE));
    }

    /// Default terminal: after writing `rexon-terminal.conf` the session
    /// variables are immediately carried to compositors without environment.d.
    #[test]
    fn terminal_default_syncs_session_env_after_write() {
        let io = protector_io();
        DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new())
            .set_default("terminal", "kitty.desktop");
        let conf = io
            .index_of("environment.d/rexon-terminal.conf")
            .expect("rexon-terminal.conf записан");
        let sync = io.index_of("sync_session_env").expect("переменные сеанса не синхронизированы");
        assert!(conf < sync, "синхронизация раньше записи: {:#?}", io.calls.borrow());

        // An ordinary default application does not touch the session variables.
        let io = protector_io();
        DeOpsExec::new(&io, Arc::new(Recorder::default()), CancelToken::new())
            .set_default("browser", "firefox.desktop");
        assert!(io.index_of("sync_session_env").is_none(), "{:#?}", io.calls.borrow());
    }

    /// Protection is off: the profile is not touched at all.
    #[test]
    fn startup_snapshot_skipped_when_protection_off() {
        let io = protector_io();
        io.files.borrow_mut().insert(
            "/home/t/.config/rexon/protection_rules.json".to_string(),
            r#"{"enabled": false}"#.to_string(),
        );
        assert!(snapshot_if_never(&io).is_none());
        assert!(!io.files.borrow().contains_key(SNAPSHOT_FILE));
        assert!(io.runs().is_empty(), "{:#?}", io.runs());
    }

    /// Footer: a success result goes away by itself and on a section change, an
    /// error stays until a click, waiting for an answer is not removed by a
    /// timer (16.11.2026: «Wayfire repair: Done ✅» hung for hours).
    #[test]
    fn footer_lifetime_by_kind() {
        let notice = footer_hold(FooterKind::Notice);
        assert!(notice.timeout_ms > 0 && notice.timeout_ms <= 15_000);
        assert!(notice.clear_on_section_change && notice.click_dismisses);

        let error = footer_hold(FooterKind::Error);
        assert_eq!(error.timeout_ms, 0);
        assert!(!error.clear_on_section_change && error.click_dismisses);

        let pending = footer_hold(FooterKind::Pending);
        assert_eq!(pending.timeout_ms, 0);
        assert!(!pending.clear_on_section_change && !pending.click_dismisses);

        let outcome = |ok, cancelled| DeOpOutcome {
            ok,
            message: String::new(),
            cancelled,
            restart_needed: false,
        };
        assert_eq!(outcome_footer_kind(&outcome(true, false)), FooterKind::Notice);
        assert_eq!(outcome_footer_kind(&outcome(false, true)), FooterKind::Notice);
        assert_eq!(outcome_footer_kind(&outcome(false, false)), FooterKind::Error);
        assert_eq!(result_footer_kind(true), FooterKind::Notice);
        assert_eq!(result_footer_kind(false), FooterKind::Error);
    }
}
