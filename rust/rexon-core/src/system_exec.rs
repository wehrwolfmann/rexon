//! Live executor of the system layer: a port of the ACTING half of
//! `rexon/system.py`.
//!
//! WHY. The decisions of this layer moved into the core long ago and
//! entirely: [`crate::system`] holds the desktop database and the os-release
//! parsing, and [`crate::system_plan`] holds install plans, the texts of the
//! privileged helper scripts, rollback of Plasma forks, Plymouth and SDDM,
//! login screen detection. What was missing was exactly the part that
//! EXECUTES these plans: the trips outside, i.e. `pkexec`,
//! `systemctl is-enabled`, package manager queries, reading
//! `/etc/sddm.conf.d` and `/usr/share/plymouth/themes`. Python held them, and
//! because of that the emergency route `--recover-kde` (the very one that
//! must work when Qt is broken) depended on the interpreter with the bridge
//! wheel.
//!
//! The split here is the same as in the neighbouring executors
//! ([`crate::wallpaper_exec`], [`crate::splash_exec`], [`crate::ext_exec`],
//! [`crate::imp_live`], [`crate::appdefaults_exec`]): **the core decides,
//! the executor does**.
//!   * WHAT to do: [`crate::system_plan`] (plans, argv, all answer texts);
//!   * WITH WHAT: the [`SystemIo`] trait, each method exactly one side
//!     effect or observation. In the product it is implemented by
//!     [`LiveSystemIo`] (real file systems and subprocesses), in the parity
//!     checks by a fake executor, and the live machine is not touched at all;
//!   * the ORDER of steps: [`SystemExec`].
//!
//! ⛔ SECURITY BOUNDARY. Everything this module can change is changed by
//! exactly three `pkexec` runs (Plasma forks, Plymouth, SDDM), and each of
//! them is assembled not by this file but by the core. There is not a single
//! command line here: the executor takes a ready `argv` from the plan and
//! hands the exit code back to the core for interpretation. So "what exactly
//! will run" is verified by the core's parity checks, and "in what order and
//! based on which observations" by this file's checks on fake I/O.
//!
//! ⭐ WHAT WAS FIXED IN THE PORT (rule #16: we do not preserve defects of the
//! original):
//!
//!   1. **The login screen is reset with exactly the plan shown to the
//!      person.** The original route (`rexon/app.py:_reset_sddm`) computed the
//!      `sddm_reset_plan()` plan, printed the file list, and then called
//!      `reset_sddm_to_default()`, which scanned the directory AGAIN and
//!      deleted what it found the second time. Between the two scans the state
//!      could change (a neighbouring theme installer writes there too), and
//!      something other than promised was deleted. Here the plan is computed
//!      once ([`SystemExec::sddm_reset_plan`]) and executed as is
//!      ([`SystemExec::apply_sddm_reset`]); the old one-button form
//!      ([`SystemExec::reset_sddm_to_default`]) stays and calls both.
//!   2. **A dangling login screen symlink is now NAMED out loud.** The
//!      decision "a dangling symlink is not a login screen" was made in the
//!      core earlier ([`crate::system_plan::dm_symlink_unit`]), but there was
//!      no way to get the name of the dead target: `std::fs::canonicalize` on
//!      a dangling symlink simply fails. [`realpath_lenient`] walks the path
//!      link by link and returns the target even when it is not on disk, so
//!      [`SystemExec::dm_symlink_target`] returns `(Some(target), false)`, and
//!      the foolproof guard can tell the person WHOSE unit was removed instead
//!      of staying silent.
//!   3. **The distribution is detected once per executor.** The original
//!      `_is_pkg_installed` created `PackageManager(DistroDetector())` for
//!      EVERY package check, i.e. read `/etc/os-release` again and searched
//!      `PATH` for binaries again; choosing the default Plymouth theme calls
//!      it in a loop. Here the answer is computed lazily and remembered
//!      ([`SystemExec::family`]).
//!   4. **A privileged helper counts as deployed only if it can be RUN.** The
//!      original `_helper_available` only asked `os.path.exists`. A file
//!      without the exec bit (an interrupted install, an archive unpacked the
//!      wrong way) passed the check, `pkexec` failed on `exec`, and the caller
//!      got an operation failure, although a soft fallback to raw
//!      `pkexec <pm>` exists for exactly this case. Now the exec bit is
//!      checked too ([`SystemIo::is_executable`]), and the fallback works as
//!      intended. The regular install puts the helpers via `install -Dm755`
//!      (`installer::helper_install_block`), so on a healthy machine the
//!      answer is the same as before.
//!   5. **"Could not" and "did not wait long enough" are no longer the same.**
//!      The original `safe_run` returned `None` for both cases, and only
//!      `logout_session` told the timeout apart, with its own `except` branch.
//!      Here the command outcome is carried by [`RunOut::outcome`], and the
//!      original `None` semantics remain as one method [`RunOut::rc`], which
//!      is what the core is fed with.
//!
//! ⚠️ WHAT IS NOT HERE. Installing, switching and removing desktops
//! (`install_de`/`switch_de`/`remove_de`, `PackageManager.install_batch`,
//! `ensure_display_manager`, the AeroShell rebuild) is the next round: they
//! have their own executor with progress and a log tail, and there is no
//! reason to mix it into one file with the emergency rollback.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use crate::app;
use crate::qt_settings;
use crate::system::{self, DistroDetector, PkgFamily};
use crate::system_plan as plan;
use crate::utils::{self, RunOutcome, RunOutput, RunSpec};
use serde::Serialize;

// ============================================================================
// Executor trait
// ============================================================================

/// Result of running an external command: what the original got from
/// `safe_run`, plus the "could not / did not wait" distinction (see item 5 in
/// the module header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    pub outcome: RunOutcome,
    pub returncode: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RunOut {
    /// Outcome "the command gave no answer at all": `safe_run(...) is None`.
    pub fn no_answer(&self) -> bool {
        matches!(self.outcome, RunOutcome::Timeout | RunOutcome::Error)
    }

    /// Exit code in the form the core expects: `None` = there was no answer.
    pub fn rc(&self) -> Option<i32> {
        if self.no_answer() {
            None
        } else {
            self.returncode
        }
    }

    /// Answer "no such command / did not start": fills the branches where the
    /// executor never got to running it.
    pub fn missing() -> Self {
        RunOut {
            outcome: RunOutcome::Error,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

/// Side effects and observations of the system layer, exactly the ones
/// Python did. Not a single decision: each method is one trip outside.
///
/// The methods take `&self`, not `&mut self`: the deciding layer
/// [`crate::system_plan`] takes observations as closures
/// (`&dyn Fn(&str) -> Option<String>` and relatives), and with `&mut self`
/// each such call would run into a double borrow of the executor. It costs
/// fake executors nothing: they keep their log in a `RefCell`.
pub trait SystemIo {
    /// `os.path.exists` (symlinks are dereferenced, as in Python).
    fn exists(&self, path: &str) -> bool;
    /// `os.path.isfile`.
    fn is_file(&self, path: &str) -> bool;
    /// `os.path.isdir`.
    fn is_dir(&self, path: &str) -> bool;
    /// Whether the file has the exec bit (`os.access(p, os.X_OK)`).
    fn is_executable(&self, path: &str) -> bool;
    /// `os.path.realpath`, NON-strict: for a dangling symlink it returns the dead
    /// target, not an error (see item 2 in the module header). `None`: the path
    /// is empty or the file system returned an error.
    fn realpath(&self, path: &str) -> Option<String>;
    /// Read a text file; `None`: could not read (the equivalent of `except OSError`).
    fn read_text(&self, path: &str) -> Option<String>;
    /// File names of a directory WITHOUT order. `None`: the directory does not
    /// exist or is not readable (`except OSError`): that is NOT the same as an
    /// empty directory. The core sets the order: file system traversal is arbitrary.
    fn list_dir(&self, dir: &str) -> Option<Vec<String>>;
    /// `shutil.which(name) is not None` (taking binary aliases into account).
    fn which(&self, name: &str) -> bool;
    /// `safe_run(argv, capture_output=True, timeout=…, lang_c=…)`.
    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut;
    /// An environment variable; a missing one is an empty string.
    fn env(&self, name: &str) -> String;
    /// `resolve_tool(name)`: the PATH to a program, taking name aliases
    /// (`kwriteconfig6` → `kwriteconfig5`) and directories outside `PATH` into
    /// account.
    ///
    /// Separate from [`SystemIo::which`] on purpose: that one only needs a
    /// "present" answer, while argv builders need the path itself, the same one
    /// the command will later run with, otherwise the check and the run diverge.
    fn which_path(&self, name: &str) -> Option<String>;
    /// Write a whole text file. `Err` is the error text for the person (a port
    /// of `except OSError as e` showing `{e}`).
    fn write_text(&self, path: &str, content: &str) -> Result<(), String>;
    /// Copy a file together with its permissions (`shutil.copy2`). `Err` is the
    /// error text for the person.
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String>;
    /// Delete a file (not a directory). `Err` is the error text. By default the
    /// executor cannot delete: deletion is only needed by the font reset.
    fn unlink_file(&self, path: &str) -> Result<(), String> {
        Err(format!("cannot remove {path}"))
    }
    /// Replace the file `path` with a symbolic link to `target` (with one rename:
    /// the file is either the old one or already the link). Not supported by
    /// default, see [`SystemIo::unlink_file`].
    fn replace_with_symlink(&self, path: &str, target: &str) -> Result<(), String> {
        Err(format!("cannot link {path} to {target}"))
    }
    /// An operation as root through the window's privilege session
    /// ([`crate::privilege::run`]): `files` are the operation's file paths,
    /// progress goes to the log `log_path` (if any) and to the answer's `stdout`.
    /// `None`: there is no privilege session or the helper does not perform the
    /// operation, so the executor uses the old command (`run`). A helper refusal
    /// is `Some` with a non-zero code, and no old command follows it (Z5).
    /// Defaults to `None`: fakes and test benches live on the old path.
    fn run_priv(
        &self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        log_path: Option<&str>,
    ) -> Option<RunOut> {
        let _ = (op, files, log_path);
        None
    }
}

// ============================================================================
// Executor
// ============================================================================

/// Order of the system layer steps on top of [`SystemIo`].
///
/// Lives exactly as long as the operation: the two remembered answers
/// (`pkexec` present / distribution family) were process-global in the
/// original, so an answer computed once outlived both a polkit install and
/// any parity check. Here the memory is tied to the executor: a new executor
/// means a new answer, and a fake one inherits nothing from the live machine.
pub struct SystemExec<'a, IO: SystemIo + ?Sized> {
    io: &'a IO,
    /// Port of the `_polkit_state` cache.
    polkit: Cell<Option<bool>>,
    /// Port of `PackageManager(DistroDetector()).family`, computed lazily.
    family: Cell<Option<PkgFamily>>,
    /// Lines the original wrote to the log (`_log.error`), in order of
    /// appearance. The caller decides whether to show them to the person.
    logs: RefCell<Vec<String>>,
}

impl<'a, IO: SystemIo + ?Sized> SystemExec<'a, IO> {
    pub fn new(io: &'a IO) -> Self {
        SystemExec {
            io,
            polkit: Cell::new(None),
            family: Cell::new(None),
            logs: RefCell::new(Vec::new()),
        }
    }

    /// Accumulated log lines (port of `_log.error`/`_log.debug`).
    pub fn logs(&self) -> Vec<String> {
        self.logs.borrow().clone()
    }

    fn log(&self, line: String) {
        self.logs.borrow_mut().push(line);
    }

    // ── Package engine ───────────────────────────────────────────────────

    /// Port of `_run_pkg_query`: the SINGLE trip outside for all package engine
    /// questions. `(None, "")`: there was nothing to ask with (no plan, no
    /// binary) or the command gave no answer. That is NOT the same as a non-zero
    /// code: for a target existence probe, "could not ask" and "no such package"
    /// lead to different decisions.
    fn run_pkg_query(&self, query: Option<&plan::InstalledPkgQuery>) -> (Option<i32>, String) {
        let Some(q) = query else {
            return (None, String::new());
        };
        if !self.io.which(&q.binary) {
            return (None, String::new());
        }
        let out = self.io.run(&q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c);
        match out.rc() {
            Some(rc) => (Some(rc), out.stdout),
            None => (None, String::new()),
        }
    }

    /// Package manager family of this machine, computed once.
    pub fn family(&self) -> PkgFamily {
        if let Some(f) = self.family.get() {
            return f;
        }
        let detector = DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        );
        let f = detector.family(&|b| self.io.which(b));
        self.family.set(Some(f));
        f
    }

    /// Port of `installed_packages`: names of EVERYTHING installed. `[]`: could
    /// not ask.
    pub fn installed_packages(&self, family: PkgFamily) -> Vec<String> {
        let query = plan::installed_packages_query(family);
        let (rc, out) = self.run_pkg_query(query.as_ref());
        plan::parse_installed_packages(rc == Some(0), &out)
    }

    /// Port of `_pacman_qi_stdout`: stdout of the "installed package metadata"
    /// question, or `None`.
    pub fn pkg_meta_stdout(&self, package: &str, family: PkgFamily) -> Option<String> {
        let query = plan::installed_meta_query(family, package);
        let (rc, out) = self.run_pkg_query(query.as_ref());
        if rc == Some(0) {
            Some(out)
        } else {
            None
        }
    }

    /// Port of `_repo_target_exists`: whether such a target is in the
    /// repositories. `Some(true)` it is, `Some(false)` the manager clearly said
    /// "no", `None` could not ask (the decision on `None` stays as before,
    /// otherwise on non-Arch we would refuse for no reason).
    pub fn repo_target_exists(&self, package: &str, family: PkgFamily) -> Option<bool> {
        let query = plan::repo_version_query(family, package.trim());
        let (rc, out) = match query {
            None => (None, String::new()),
            Some(q) => {
                let ipq = plan::InstalledPkgQuery { binary: q.binary, cmd: q.cmd };
                self.run_pkg_query(Some(&ipq))
            }
        };
        plan::repo_target_exists(family, rc, &out)
    }

    /// Port of `PackageManager.is_installed` / `_is_pkg_installed`.
    /// Best-effort: `false` on any failure.
    pub fn is_pkg_installed(&self, package: &str) -> bool {
        let Ok(argv) = plan::query_cmd(self.family(), package) else {
            return false;
        };
        let out = self.io.run(&argv, None, false);
        plan::pkg_installed_from_rc(out.rc())
    }

    // ── Polkit and privileged helpers ───────────────────────────────────

    /// Port of `polkit_available`: whether `pkexec` is present, the SINGLE
    /// polkit check.
    ///
    /// The answer is remembered for the executor's lifetime, and an explanation
    /// of what is missing goes to the log exactly once.
    pub fn polkit_available(&self) -> bool {
        if let Some(known) = self.polkit.get() {
            return known;
        }
        let found = self.io.which("pkexec");
        self.polkit.set(Some(found));
        if !found {
            self.log(self.polkit_missing_message());
        }
        found
    }

    /// Port of `polkit_missing_message`: a human explanation of "no pkexec" with
    /// the package name for THIS distribution.
    pub fn polkit_missing_message(&self) -> String {
        system::polkit_missing_message(&utils::tool_package_hint(
            "pkexec",
            self.family().as_str(),
        ))
    }

    /// Port of `_helper_available`: whether a privileged helper and its polkit
    /// policy are deployed. One question for all three Rexon helpers (de/pkg/stage).
    ///
    /// ⭐ The exec bit is checked in addition to the original, see item 4 in the
    /// module header. The soft fallback (the caller goes with raw `pkexec <pm>`)
    /// is only enabled by this where `exec` used to fail.
    pub fn helper_available(&self, helper_path: &str, policy_path: &str) -> bool {
        self.polkit_available()
            && self.io.is_file(helper_path)
            && self.io.is_executable(helper_path)
            && self.io.exists(policy_path)
    }

    /// Port of `_de_helper_available` (installing desktops with one password).
    pub fn de_helper_available(&self) -> bool {
        self.helper_available(plan::de_helper_path(), plan::DE_POLICY_PATH)
    }

    /// Port of `_pkg_helper_available` (package manager operations).
    pub fn pkg_helper_available(&self) -> bool {
        self.helper_available(plan::pkg_helper_path(), plan::PKG_POLICY_PATH)
    }

    /// Port of `_stage_helper_available` (placing files into /usr/share).
    pub fn stage_helper_available(&self) -> bool {
        self.helper_available(plan::stage_helper_path(), plan::STAGE_POLICY_PATH)
    }

    /// Port of `privileged_pkg_cmd`: argv `pkexec rexon-pkg-helper …` or `None`,
    /// in which case the caller falls back to raw `pkexec <pm> …`.
    pub fn privileged_pkg_cmd(
        &self,
        family: PkgFamily,
        op: &str,
        packages: &[String],
    ) -> Option<Vec<String>> {
        plan::privileged_pkg_cmd(family, op, packages, self.pkg_helper_available())
    }

    /// Port of `privileged_stage_cmd`: argv `pkexec rexon-stage-helper …` or
    /// `None` (soft fallback to the raw path).
    pub fn privileged_stage_cmd(
        &self,
        category: &str,
        name: &str,
        staged: &str,
    ) -> Option<Vec<String>> {
        let is_dir = !staged.is_empty() && self.io.is_dir(staged);
        let real = if staged.is_empty() {
            None
        } else {
            self.io.realpath(staged)
        };
        plan::privileged_stage_cmd(
            category,
            name,
            staged,
            is_dir,
            real.as_deref(),
            self.stage_helper_available(),
        )
    }

    // ── Login screen (display manager) ──────────────────────────────────

    /// Port of `_dm_symlink_target`: `(symlink realpath, whether the target is alive)`.
    ///
    /// `(None, false)`: no symlink at all. `(Some(target), false)`: the symlink
    /// is DANGLING: after `pacman -R sddm` (including our own `remove_de` for KDE
    /// and LXQt) it stays in `/etc` with no unit behind it. What to do about it
    /// is decided by the core ([`plan::dm_symlink_unit`]); the dead target's name
    /// is passed out so there is something to tell the person.
    pub fn dm_symlink_target(&self) -> (Option<String>, bool) {
        let link = plan::DM_SYMLINK;
        // `exists` dereferences the symlink and gives false on a dangling one, so
        // we ask for realpath right away: it answers for a dead target too.
        let Some(target) = self.io.realpath(link) else {
            return (None, false);
        };
        if target == link && !self.io.exists(link) {
            // realpath resolved nothing and there is no file: there is no symlink.
            return (None, false);
        }
        let alive = self.io.exists(&target);
        (Some(target), alive)
    }

    /// Port of `_dm_is_enabled`: stdout of `systemctl is-enabled <unit>.service`.
    ///
    /// Only READS the unit state: enables and disables nothing. The exit code is
    /// deliberately ignored: `systemctl is-enabled` prints `disabled` with a
    /// non-zero code, and going by the code the answer would be lost.
    pub fn dm_is_enabled(&self, unit: &str) -> Option<String> {
        let argv = vec![
            "systemctl".to_string(),
            "is-enabled".to_string(),
            format!("{unit}.service"),
        ];
        let out = self.io.run(&argv, Some(5), false);
        if out.no_answer() {
            return None;
        }
        Some(out.stdout.trim().to_string())
    }

    /// Port of `active_login_manager`: name of the active login screen unit
    /// (without `.service`). The decision and unit tables are in the core.
    pub fn active_login_manager(&self) -> String {
        let (target, alive) = self.dm_symlink_target();
        plan::active_login_manager_from(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    /// Active login screen without the `sddm` substitution: `None` means nothing
    /// to determine it with. Decision: [`plan::active_login_manager_opt`].
    pub fn active_login_manager_opt(&self) -> Option<String> {
        let (target, alive) = self.dm_symlink_target();
        plan::active_login_manager_opt(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    /// Unit files that may be login screens: `(path, text)` in the priority
    /// order of the [`plan::DM_UNIT_DIRS`] directories. Read only.
    ///
    /// A masked unit (a link to `/dev/null`) is skipped: it does not override the
    /// package file in the list; the readiness check will see the mask.
    pub fn dm_unit_files(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for dir in plan::DM_UNIT_DIRS {
            let Some(mut names) = self.io.list_dir(dir) else { continue };
            names.sort();
            for name in names {
                let Some(unit) = plan::unit_name_from_path(&name) else { continue };
                if !plan::dm_unit_name_listable(unit) {
                    continue;
                }
                let path = format!("{dir}/{name}");
                if self.io.realpath(&path).as_deref() == Some("/dev/null") {
                    continue;
                }
                if !self.io.is_file(&path) {
                    continue;
                }
                let Some(text) = self.io.read_text(&path) else { continue };
                if plan::dm_unit_file_candidate(unit, &text) {
                    out.push((path, text));
                }
            }
        }
        out
    }

    /// Installed login screens (units without `.service`). Decision:
    /// [`plan::dm_units_from_unit_files`].
    pub fn installed_login_managers(&self) -> Vec<String> {
        plan::dm_units_from_unit_files(&self.dm_unit_files())
    }

    /// Probes for choosing the login screen switching mechanism. Only reading
    /// files and PATH, no commands.
    pub fn dm_backend_probe(&self) -> plan::DmBackendProbe {
        // display-manager.service is a real unit (openSUSE) if one of the paths
        // resolves to a file with that same name, not to a DM unit.
        let dm_service_is_regular_file = [plan::DM_SYMLINK, "/usr/lib/systemd/system/display-manager.service"]
            .iter()
            .any(|p| {
                self.io.is_file(p)
                    && self
                        .io
                        .realpath(p)
                        .is_some_and(|r| plan::unit_name_from_path(&r) == Some("display-manager"))
            });
        let systemd_running = self.io.is_dir("/run/systemd/system");
        plan::DmBackendProbe {
            systemd_running,
            has_systemctl: self.io.which("systemctl"),
            x11_default_dm_file: self.io.is_file("/etc/X11/default-display-manager"),
            dm_service_is_regular_file,
            suse_dm_alternative: self.io.exists("/etc/alternatives/default-displaymanager"),
            other_init: if systemd_running {
                String::new()
            } else {
                plan::other_init_name(&|p| self.io.exists(p), &|n| self.io.which(n))
            },
        }
    }

    /// Whether a program exists: an absolute path is an executable file, otherwise PATH.
    pub fn program_exists(&self, program: &str) -> bool {
        if program.starts_with('/') {
            self.io.is_executable(program)
        } else {
            !program.is_empty() && self.io.which(program)
        }
    }

    /// Installed LightDM greeters (`/usr/share/xgreeters`).
    pub fn lightdm_greeters(&self) -> Vec<plan::DmGreeter> {
        let dir = plan::XGREETERS_DIR;
        let Some(mut names) = self.io.list_dir(dir) else { return Vec::new() };
        names.sort();
        let files: Vec<(String, String)> = names
            .into_iter()
            .filter(|n| n.ends_with(".desktop"))
            .filter_map(|n| {
                let path = format!("{dir}/{n}");
                self.io.read_text(&path).map(|t| (path, t))
            })
            .collect();
        plan::xgreeters_from(&files, &|exec| self.program_exists(exec))
    }

    /// `greeter-session` from `lightdm --show-config`: the command only reads
    /// configs and works without root. `None`: not set or no answer.
    pub fn lightdm_greeter_session(&self) -> Option<String> {
        if !self.io.which("lightdm") {
            return None;
        }
        let argv = vec!["lightdm".to_string(), "--show-config".to_string()];
        let out = self.io.run(&argv, Some(5), true);
        if out.no_answer() {
            return None;
        }
        // LightDM 1.33 (Arch) prints the config to stderr; we look at both streams.
        plan::lightdm_greeter_from_show_config(&format!("{}\n{}", out.stdout, out.stderr))
    }

    /// Whether a system user exists now or is declared by `sysusers.d`.
    fn dm_user_ok(&self, user: &str) -> bool {
        if self
            .io
            .read_text("/etc/passwd")
            .is_some_and(|t| plan::passwd_has_user(&t, user))
        {
            return true;
        }
        ["/usr/lib/sysusers.d", "/etc/sysusers.d"].iter().any(|dir| {
            self.io.list_dir(dir).is_some_and(|names| {
                names.iter().filter(|n| n.ends_with(".conf")).any(|n| {
                    self.io
                        .read_text(&format!("{dir}/{n}"))
                        .is_some_and(|t| plan::sysusers_declares(&t, user))
                })
            })
        })
    }

    /// Number of sessions (`*.desktop`) in the directories.
    fn session_count(&self, dirs: &[&str]) -> usize {
        dirs.iter()
            .filter_map(|d| self.io.list_dir(d))
            .map(|names| names.iter().filter(|n| n.ends_with(".desktop")).count())
            .sum()
    }

    /// SDDM config texts in merge order: package `conf.d`, our own `conf.d`,
    /// then `/etc/sddm.conf`.
    fn sddm_conf_texts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for dir in [plan::SDDM_DISTRO_CONF_D, plan::SDDM_CONF_D] {
            out.extend(self.sddm_conf_entries(dir).into_iter().map(|(_, t)| t));
        }
        if let Some(t) = self.io.read_text("/etc/sddm.conf") {
            out.push(t);
        }
        out
    }

    /// Facts for the readiness check of one login screen. Only reading files and
    /// PATH; the only command is `lightdm --show-config` (for LightDM), and it
    /// only reads as well.
    pub fn dm_facts(
        &self,
        unit: &str,
        spec: Option<&plan::DmSpec>,
        exec_start: &[String],
        family: PkgFamily,
    ) -> plan::DmFacts {
        let binary = exec_start.first().cloned().unwrap_or_default();
        let masked = self
            .io
            .realpath(&format!("/etc/systemd/system/{unit}.service"))
            .as_deref()
            == Some("/dev/null");
        let mut facts = plan::DmFacts {
            unit: unit.to_string(),
            binary_exists: self.program_exists(&binary),
            masked,
            needs_xorg: spec.is_some_and(|s| s.needs_xorg),
            xorg: self.io.which("Xorg")
                || self.io.is_executable("/usr/bin/Xorg")
                || self.io.is_executable("/usr/lib/Xorg"),
            x_sessions: self.session_count(&["/usr/share/xsessions", "/usr/local/share/xsessions"]),
            wayland_sessions: self.session_count(&[
                "/usr/share/wayland-sessions",
                "/usr/local/share/wayland-sessions",
            ]),
            ..Default::default()
        };
        let mut user = spec.and_then(|s| s.user).map(String::from);
        if let Some(s) = spec {
            facts.pam = s.pam.to_string();
        }
        // greetd and everything started through it (cosmic-greeter): the greeter
        // and the user come from ITS OWN config.
        if exec_start.first().is_some_and(|b| b.rsplit('/').next() == Some("greetd")) {
            let config = self.io.read_text(&plan::greetd_config_path(exec_start)).unwrap_or_default();
            let command = plan::greetd_command_kind(&config);
            let program_exists = match &command {
                plan::GreetdCommand::Graphical(p) => self.program_exists(p),
                _ => false,
            };
            if let Some(u) = plan::greetd_session_user(&config) {
                user = Some(u);
            }
            facts.greetd = Some(plan::GreetdFacts { command, program_exists });
        }
        match unit {
            "lightdm" => {
                facts.lightdm = Some(plan::LightdmFacts {
                    greeter_session: self.lightdm_greeter_session(),
                    default_greeter: plan::lightdm_default_greeter(family).map(String::from),
                    greeters: self.lightdm_greeters(),
                });
            }
            "sddm" => {
                let texts = self.sddm_conf_texts();
                let confs: Vec<&str> = texts.iter().map(String::as_str).collect();
                let theme_dir = plan::ini_last_value(&confs, "Theme", "ThemeDir")
                    .unwrap_or_else(|| plan::SDDM_THEMES_DIR.to_string());
                let current = plan::ini_last_value(&confs, "Theme", "Current");
                let meta = current.as_deref().and_then(|t| {
                    self.io.read_text(&format!("{}/{t}/metadata.desktop", theme_dir.trim_end_matches('/')))
                });
                let theme = plan::sddm_theme_ready(current.as_deref(), meta.as_deref(), &|g| {
                    self.program_exists(g)
                });
                let x11 = plan::sddm_uses_x11(&confs);
                facts.needs_xorg = x11;
                let wayland_compositor = (!x11).then(|| {
                    let c = plan::sddm_wayland_compositor(&confs);
                    let ok = self.program_exists(&c);
                    (c, ok)
                });
                facts.sddm = Some(plan::SddmFacts { theme, wayland_compositor });
            }
            _ => {}
        }
        if !facts.pam.is_empty() {
            facts.pam_exists = ["/etc/pam.d", "/usr/lib/pam.d"]
                .iter()
                .any(|d| self.io.is_file(&format!("{d}/{}", facts.pam)));
        }
        if let Some(u) = user {
            facts.user_ok = self.dm_user_ok(&u);
            facts.user = Some(u);
        }
        facts
    }

    /// Services from the login screen unit's `Requires=` that are in no unit
    /// directory or are masked. Only reading files.
    pub fn missing_required_units(&self, unit_text: &str) -> Vec<String> {
        plan::unit_values(unit_text, "Unit", "Requires")
            .into_iter()
            .filter(|u| u.ends_with(".service") && !u.contains('@'))
            .filter(|u| {
                let masked = self.io.realpath(&format!("/etc/systemd/system/{u}")).as_deref() == Some("/dev/null");
                masked || !plan::DM_UNIT_DIRS.iter().any(|d| self.io.is_file(&format!("{d}/{u}")))
            })
            .collect()
    }

    /// Installed desktops that care which login screen is used
    /// ([`plan::DESKTOP_DM_RULES`]): ids from the core database. Only PATH and files.
    pub fn installed_dm_sensitive_desktops(&self) -> Vec<String> {
        plan::DESKTOP_DM_RULES
            .iter()
            .filter(|r| r.binaries.iter().all(|b| self.program_exists(b)))
            .map(|r| r.de_id.to_string())
            .collect()
    }

    /// Login screens on this system for the «Login screen» section: installed
    /// ones, which one is the default, whether each is ready to become the
    /// default. Read only: enables and disables nothing.
    ///
    /// Without systemd the list comes from programs in the table, the default
    /// from the init selection files, readiness is `Unsupported` (no button).
    pub fn login_managers(&self) -> Vec<plan::LoginManagerInfo> {
        let family = self.family();
        let backend = self.dm_switch_backend();
        let label = |unit: &str| {
            plan::login_manager_spec(unit).map_or(String::new(), |s| s.label.to_string())
        };
        if let plan::DmSwitchBackend::Unsupported(_) = backend {
            let active = plan::dm_from_init_files(
                self.io.read_text("/etc/conf.d/display-manager").as_deref(),
                self.io.read_text("/etc/X11/default-display-manager").as_deref(),
            );
            return plan::LOGIN_MANAGER_SPECS
                .iter()
                .filter_map(|s| {
                    let path = self.io.which_path(s.binary)?;
                    Some(plan::LoginManagerInfo {
                        unit: s.unit.to_string(),
                        label: s.label.to_string(),
                        binary: Some(path),
                        present: true,
                        active: active.as_deref() == Some(s.unit),
                        readiness: plan::DmReadiness::Unsupported,
                        warnings: Vec::new(),
                    })
                })
                .collect();
        }
        let files = self.dm_unit_files();
        let mut units = plan::dm_units_from_unit_files(&files);
        let active = self.active_login_manager_opt();
        if let Some(a) = &active {
            if !units.contains(a) {
                units.insert(0, a.clone());
            }
        }
        units
            .into_iter()
            .map(|unit| {
                let text = files
                    .iter()
                    .find(|(p, _)| plan::unit_name_from_path(p) == Some(unit.as_str()))
                    .map(|(_, t)| t.as_str())
                    .unwrap_or_default();
                let exec = plan::unit_exec_start(text).unwrap_or_default();
                let spec = plan::login_manager_spec(&unit);
                let mut facts = self.dm_facts(&unit, spec, &exec, family);
                facts.missing_units = self.missing_required_units(text);
                plan::LoginManagerInfo {
                    label: label(&unit),
                    binary: exec.first().cloned(),
                    present: facts.binary_exists,
                    active: active.as_deref() == Some(unit.as_str()),
                    readiness: plan::dm_readiness(spec, &backend, &facts),
                    warnings: plan::dm_warnings(&facts),
                    unit,
                }
            })
            .collect()
    }

    /// Login screen selection mechanism on this system.
    pub fn dm_switch_backend(&self) -> plan::DmSwitchBackend {
        plan::dm_switch_backend(self.family(), &self.dm_backend_probe())
    }

    /// Immutable system (Silverblue, Kinoite, Bazzite, SteamOS…): in the first
    /// version Rexon does not switch the login screen there.
    fn distro_immutable(&self) -> bool {
        DistroDetector::from_os_release_files(
            self.io.read_text("/etc/os-release").as_deref(),
            self.io.read_text("/usr/lib/os-release").as_deref(),
        )
        .is_immutable()
    }

    /// Which of `units` are running NOW (`systemctl is-active` in one call, one
    /// answer line per unit). Empty: nothing to ask with.
    fn running_login_managers(&self, units: &[&str]) -> Vec<String> {
        if units.is_empty() || !self.io.which("systemctl") {
            return Vec::new();
        }
        let mut argv = vec!["systemctl".to_string(), "is-active".to_string()];
        argv.extend(units.iter().map(|u| format!("{u}.service")));
        let out = self.io.run(&argv, Some(5), true);
        if out.no_answer() {
            return Vec::new();
        }
        units
            .iter()
            .zip(out.stdout.lines())
            .filter(|(_, state)| state.trim() == "active")
            .map(|(u, _)| (*u).to_string())
            .collect()
    }

    /// Snapshot of the «Login manager» section for the UI: installed login
    /// screens, the default one and the one running now, whether and with what
    /// text to offer «Make default». Read only: the commands are `lightdm
    /// --show-config` and `systemctl is-active`. Decision: [`login_screen_view`].
    pub fn login_screen_view(&self) -> LoginScreenView {
        let family = self.family();
        let backend = self.dm_switch_backend();
        let list = self.login_managers();
        let running = if backend.switch_supported() {
            let units: Vec<&str> = list.iter().map(|m| m.unit.as_str()).collect();
            self.running_login_managers(&units)
        } else {
            Vec::new()
        };
        let facts = LoginScreenFacts {
            family,
            helper: self.pkg_helper_available(),
            immutable: self.distro_immutable(),
            running,
            previous: self.io.read_text(plan::PREVIOUS_DM_FILE),
            desktops: self.installed_dm_sensitive_desktops(),
            autologin_notes: self.autologin_notes(&list),
        };
        login_screen_view(&list, &backend, &facts)
    }

    /// What happens to autologin if `new` becomes the default (the previous one
    /// is `old`). Read only: login screen settings files, PAM, `getent`.
    pub fn autologin_transfer(&self, old: &str, new: &str) -> crate::dm_autologin::Transfer {
        use crate::dm_autologin as al;
        let read = |p: &str| self.io.read_text(p);
        let list = |d: &str| self.io.list_dir(d).unwrap_or_default();
        let pam = al::autologin_spec(new).and_then(|s| {
            [format!("/etc/pam.d/{}", s.pam), format!("/usr/lib/pam.d/{}", s.pam)].iter().find_map(|p| read(p))
        });
        // We ask for the group only when there is autologin: otherwise it is not needed.
        let source_on = matches!(al::read_autologin(old, &read, &list), al::AutologinState::On(_));
        let group_members = pam.as_deref().filter(|_| source_on).and_then(al::pam_autologin_group).and_then(|g| {
            let out = self.io.run(&["getent".to_string(), "group".to_string(), g], Some(5), true);
            let line = out.stdout.lines().next().unwrap_or("").to_string();
            (out.rc() == Some(0) && !line.is_empty()).then(|| {
                line.split(':').nth(3).unwrap_or("").split(',').map(str::trim).filter(|m| !m.is_empty()).map(str::to_string).collect()
            })
        });
        let facts = al::TargetFacts {
            pam_autologin: pam,
            group_members,
            greeter_user: plan::login_manager_spec(new).and_then(|s| s.user).map(str::to_string),
        };
        al::plan_transfer(old, new, &read, &list, &facts)
    }

    /// Autologin warnings for the confirmation dialog of each row that can be
    /// made the default: `(unit, text)`.
    fn autologin_notes(&self, list: &[plan::LoginManagerInfo]) -> Vec<(String, String)> {
        let Some(old) = list.iter().find(|m| m.active) else { return Vec::new() };
        list.iter()
            .filter(|m| !m.active && m.present)
            .filter_map(|m| {
                let t = self.autologin_transfer(&old.unit, &m.unit);
                let label = if m.label.trim().is_empty() { m.unit.as_str() } else { m.label.as_str() };
                crate::dm_autologin::transfer_note(label, &t).map(|n| (m.unit.clone(), n))
            })
            .collect()
    }

    /// «Make default»: `(done, text for the person)`.
    ///
    /// Foolproof guard on the core side (rule #13): the UI has already locked the
    /// button, but here everything is checked again against the live system:
    /// the mechanism, immutability, whether such a login screen exists, whether
    /// it is already the default, whether it is ready and whether the greeter is
    /// one of the installed ones. On any refusal no password is asked. Then ONE
    /// `pkexec` call of the `switch-dm` helper, which repeats the checks itself
    /// and rolls back on a mismatch; there is no raw `pkexec systemctl`.
    pub fn switch_login_manager(&self, unit: &str, greeter: Option<&str>) -> (bool, String) {
        let unit = unit.trim();
        let greeter = greeter.map(str::trim).filter(|g| !g.is_empty());
        let backend = self.dm_switch_backend();
        if let Some(note) = plan::login_manager_unsupported_note(&backend, None) {
            return (false, note);
        }
        if self.distro_immutable() {
            return (false, login_manager_immutable_note());
        }
        let list = self.login_managers();
        let Some(info) = list.iter().find(|m| m.unit == unit) else {
            return (false, plan::login_manager_switch_failed(&login_manager_missing_reason()));
        };
        if info.active {
            return (false, login_manager_already_default(&info.label, unit));
        }
        let greeter = match &info.readiness {
            plan::DmReadiness::Blocked(reason) => {
                return (false, plan::login_manager_switch_failed(reason));
            }
            plan::DmReadiness::Unsupported => {
                return (false, plan::login_manager_switch_failed(&login_manager_cant_switch_reason()));
            }
            plan::DmReadiness::NeedsGreeterChoice(greeters) => match greeter {
                Some(g) if greeters.iter().any(|x| x.id == g) => Some(g),
                _ => return (false, login_manager_choose_greeter()),
            },
            // A ready LightDM: the greeter can be changed, but only to an installed
            // one. The others get no greeter passed at all.
            plan::DmReadiness::Ready if unit == "lightdm" => match greeter {
                Some(g) if self.lightdm_greeters().iter().any(|x| x.id == g) => Some(g),
                Some(_) => return (false, login_manager_choose_greeter()),
                None => None,
            },
            plan::DmReadiness::Ready => None,
        };
        let cmd = plan::switch_dm_cmd(self.family(), unit, greeter, self.pkg_helper_available());
        let old = list.iter().find(|m| m.active).map(|m| m.unit.clone());
        // Autologin the new one should end up with, for checking afterwards.
        let autologin_want = old.as_deref().map(|o| {
            crate::dm_autologin::read_autologin(o, &|p| self.io.read_text(p), &|d| self.io.list_dir(d).unwrap_or_default())
        });
        let autologin_moves = old.as_deref().is_some_and(|o| {
            let t = self.autologin_transfer(o, unit);
            !t.is_lost() && t.notes.iter().any(|n| matches!(n, crate::dm_autologin::Note::Moved { .. }))
        });
        // The window's privilege session (the same single password); without it,
        // the old package helper. Switching has no raw path in either case (Z5).
        let op = crate::privilege::op::PrivOp::SwitchDm {
            family: self.family(),
            unit: unit.to_string(),
            greeter: greeter.map(str::to_string),
        };
        let out = match (self.io.run_priv(&op, &[], None), &cmd) {
            (Some(out), _) => out,
            // Helper message language follows the person's locale: `lang_c` is off.
            (None, Some(cmd)) => self.io.run(cmd, Some(300), false),
            (None, None) => return (false, plan::login_manager_needs_helper()),
        };
        // "Not authorized": the password was not confirmed, even if the code differs.
        let rc = if out.stderr.contains("Not authorized") { Some(126) } else { out.rc() };
        // Manual command: the same as for the package helper.
        let manual_argv = cmd.unwrap_or_else(|| {
            plan::switch_dm_cmd(self.family(), unit, greeter, true).unwrap_or_default()
        });
        let manual = format!("sudo {}", manual_argv.get(1..).unwrap_or_default().join(" "));
        let (ok, mut message) = plan::switch_dm_outcome(rc, &out.stdout, &out.stderr, unit, &manual);
        if ok && autologin_moves {
            // Check against the new login screen's files, not the helper's words: the
            // old path (the script) does not carry autologin over.
            if let Some(note) = crate::dm_autologin::transfer_result_note(
                unit,
                &row_label(info),
                autologin_want.as_ref().unwrap_or(&crate::dm_autologin::AutologinState::Off),
                &|p| self.io.read_text(p),
                &|d| self.io.list_dir(d).unwrap_or_default(),
            ) {
                message.push_str("\n\n");
                message.push_str(&note);
            }
        }
        if ok {
            if let Some(hint) = old.and_then(|o| plan::login_manager_rollback_hint(&o, &backend)) {
                message.push_str("\n\n");
                message.push_str(&hint);
            }
        }
        (ok, message)
    }

    /// Port of `_display_manager_present`: an HONEST answer to "is there a login
    /// screen".
    ///
    /// Used by the foolproof guard `ensure_display_manager`: install SDDM ONLY
    /// when there really is no login screen. A dangling symlink does not count
    /// as a login screen, otherwise the guard stays silent exactly when needed.
    pub fn display_manager_present(&self) -> bool {
        let (target, alive) = self.dm_symlink_target();
        plan::display_manager_present_from(
            target.as_deref(),
            alive,
            self.io.which("systemctl"),
            &|u| self.dm_is_enabled(u),
        )
    }

    // ── Plymouth (boot splash) ──────────────────────────────────────────

    /// Port of `_plymouth_theme_from_default_link`: the theme from the
    /// `default.plymouth` link (needed where the binary is cut out of the
    /// package: Ubuntu).
    pub fn plymouth_theme_from_default_link(&self) -> Option<String> {
        let link = format!("{}/default.plymouth", plan::PLYMOUTH_THEMES_DIR);
        let target = self
            .io
            .realpath(&link)
            .filter(|t| self.io.exists(t));
        plan::plymouth_theme_from_link(target.as_deref())
    }

    /// Port of `detect_plymouth_theme`: the current active theme or `None`.
    /// The link is read LAZILY, only when the binary gave no name.
    pub fn detect_plymouth_theme(&self) -> Option<String> {
        if !self.io.which("plymouth-set-default-theme") {
            return self.plymouth_theme_from_default_link();
        }
        let out = self
            .io
            .run(&["plymouth-set-default-theme".to_string()], None, false);
        plan::plymouth_theme_from_output(out.rc(), &out.stdout)
            .or_else(|| self.plymouth_theme_from_default_link())
    }

    /// Port of `list_plymouth_themes`: installed themes. `[]` on any failure. A
    /// subdirectory is a theme if it has a `<theme>.plymouth` file inside,
    /// exactly how plymouth itself lists them.
    pub fn list_plymouth_themes(&self) -> Vec<String> {
        let dir = plan::PLYMOUTH_THEMES_DIR;
        let Some(names) = self.io.list_dir(dir) else {
            return Vec::new();
        };
        let entries: Vec<(String, bool)> = names
            .into_iter()
            .map(|n| {
                let is_dir = self.io.is_dir(&format!("{dir}/{n}"));
                (n, is_dir)
            })
            .collect();
        let probes: Vec<(String, bool)> = plan::plymouth_theme_candidates(&entries)
            .into_iter()
            .map(|t| {
                let file = plan::plymouth_theme_file_in(dir, &t);
                let present = self.io.is_file(&file);
                (t, present)
            })
            .collect();
        plan::plymouth_installed_themes(&probes)
    }

    /// Port of `_plymouth_package_default`: the default from `plymouthd.defaults`.
    pub fn plymouth_package_default(&self) -> Option<String> {
        let text = self.io.read_text(plan::PLYMOUTH_DEFAULTS);
        plan::plymouth_package_default(text.as_deref())
    }

    /// Port of `target_plymouth_theme`: the native default of THIS distribution.
    /// The steps are lazy, in the original's order: branding →
    /// `plymouthd.defaults` → fallback candidates.
    pub fn target_plymouth_theme(&self) -> Option<String> {
        let installed = self.list_plymouth_themes();
        plan::target_plymouth_theme(
            &installed,
            &|p| self.is_pkg_installed(p),
            &|| self.plymouth_package_default(),
        )
    }

    /// Port of `plymouth_reset_command`: a human-readable reset command (for
    /// display). `None`: there is nothing to reset to.
    pub fn plymouth_reset_command(&self, target: Option<&str>) -> Option<String> {
        let owned;
        let target = match target {
            Some(t) => t,
            None => {
                owned = self.target_plymouth_theme()?;
                &owned
            }
        };
        if target.is_empty() {
            return None;
        }
        plan::plymouth_reset_command(target, self.io.which("plymouth-set-default-theme"))
    }

    /// Port of `reset_plymouth_to_default`: reset the splash with one `pkexec`.
    ///
    /// Idempotent: current theme == target, so nothing runs.
    /// ⚠️ `-R` rebuilds the initramfs, the effect is only visible after a reboot.
    pub fn reset_plymouth_to_default(&self, target: Option<&str>) -> (bool, String) {
        let owned;
        let target: Option<&str> = match target {
            Some(t) => Some(t),
            None => {
                owned = self.target_plymouth_theme();
                owned.as_deref()
            }
        };
        // The original's laziness: without a target we do not read the theme, and
        // with plymouth-set-default-theme present we do not ask update-alternatives.
        let has_target = target.map(|t| !t.is_empty()).unwrap_or(false);
        let current = if has_target {
            self.detect_plymouth_theme()
        } else {
            None
        };
        let has_tool = has_target && self.io.which("plymouth-set-default-theme");
        let has_alt = has_target && !has_tool && self.io.which("update-alternatives");

        let plan_ = plan::plymouth_reset_plan(target, current.as_deref(), has_tool, has_alt, &|p| {
            self.io.is_file(p)
        });
        if let Some(pre) = plan::plymouth_reset_precheck(&plan_) {
            return (pre.ok, pre.message);
        }
        let plan::PlymouthReset::Run { cmd, shell, target } = plan_ else {
            // precheck returns None only for Run: this cannot be reached.
            unreachable!("plymouth_reset_precheck: не Run без ответа");
        };
        let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        let answer = plan::plymouth_reset_outcome(
            &target,
            &shell,
            out.rc(),
            &out.stdout,
            &out.stderr,
        );
        (answer.ok, answer.message)
    }

    // ── SDDM (login screen) ─────────────────────────────────────────────

    /// Port of `_sddm_conf_entries`: (name, contents) pairs of the directory's
    /// `*.conf` in SDDM merge order (lexicographic: later ones override earlier
    /// ones). An unreadable file stays in the list with EMPTY contents: it can
    /// be recognised by its name alone (a fork theme token).
    pub fn sddm_conf_entries(&self, conf_d: &str) -> Vec<(String, String)> {
        let Some(names) = self.io.list_dir(conf_d) else {
            return Vec::new();
        };
        let reads: Vec<(String, Option<String>)> = plan::sddm_conf_read_order(&names)
            .into_iter()
            .map(|name| {
                let text = self.io.read_text(&format!(
                    "{}/{name}",
                    conf_d.trim_end_matches('/')
                ));
                (name, text)
            })
            .collect();
        plan::sddm_conf_entries(&reads)
    }

    /// Port of `_sddm_kde_settings_text`: contents of `<conf_d>/kde_settings.conf`.
    /// It is put there by KDE/the distro, it is "out of the box", NOT Rexon: read
    /// separately from the common scan and never deleted.
    pub fn sddm_kde_settings_text(&self, conf_d: &str) -> Option<String> {
        self.io.read_text(&plan::sddm_kde_settings_path(conf_d))
    }

    /// Port of `_sddm_read_current_theme`: the current SDDM theme from `*.conf`.
    pub fn sddm_read_current_theme(&self, conf_d: &str) -> Option<String> {
        let entries = self.sddm_conf_entries(conf_d);
        plan::sddm_current_theme_from(&entries)
    }

    /// Port of `sddm_distro_default_theme`: the distro default theme, without
    /// hardcoding Breeze.
    pub fn sddm_distro_default_theme(&self) -> Option<String> {
        let distro = self.sddm_conf_entries(plan::SDDM_DISTRO_CONF_D);
        let kde = self.sddm_kde_settings_text(plan::SDDM_CONF_D);
        plan::sddm_distro_default_theme(&distro, kde.as_deref())
    }

    /// Port of `sddm_reset_plan`: what we will delete and which theme we return
    /// to, WITHOUT applying. An empty `to_remove` = an idempotent no-op.
    pub fn sddm_reset_plan(&self, conf_d: &str) -> plan::SddmResetPlan {
        let entries = self.sddm_conf_entries(conf_d);
        let distro = self.sddm_conf_entries(plan::SDDM_DISTRO_CONF_D);
        // kde_settings.conf is taken from THE SAME directory we scan: otherwise a
        // preview on a substituted directory would read the live /etc.
        let kde = self.sddm_kde_settings_text(conf_d);
        plan::sddm_reset_plan(&entries, &distro, kde.as_deref())
    }

    /// Execute an ALREADY computed SDDM reset plan: one `pkexec sh -c
    /// "rm -f …"` on specific paths. `(ok, message, removed)`.
    ///
    /// ⭐ Separate from [`Self::sddm_reset_plan`] on purpose, see item 1 in the
    /// module header: the route shows the person the plan and executes exactly
    /// it, not a recomputed second scan of the directory.
    pub fn apply_sddm_reset(
        &self,
        reset_plan: &plan::SddmResetPlan,
        conf_d: &str,
    ) -> (bool, String, Vec<String>) {
        match plan::sddm_reset_command(reset_plan, conf_d) {
            plan::SddmReset::AlreadyStock => (
                true,
                plan::sddm_already_stock_message(),
                Vec::new(),
            ),
            plan::SddmReset::Run { cmd, shell, removed, default_theme } => {
                let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
                let answer = plan::sddm_reset_outcome(
                    &removed,
                    default_theme.as_deref(),
                    &shell,
                    out.rc(),
                    &out.stdout,
                    &out.stderr,
                );
                (answer.ok, answer.message, answer.removed)
            }
        }
    }

    /// Port of `reset_sddm_to_default`: reset the login screen to the distro stock.
    /// Idempotent: nothing to delete → `(true, <"already stock" message>, [])`.
    pub fn reset_sddm_to_default(&self, conf_d: &str) -> (bool, String, Vec<String>) {
        let reset_plan = self.sddm_reset_plan(conf_d);
        self.apply_sddm_reset(&reset_plan, conf_d)
    }

    // ── Heavy Plasma forks (AeroThemePlasma / AeroShell) ────────────────

    /// Port of `detect_plasma_forks`: installed Plasma forks. Arch only: the
    /// forks live exclusively in the AUR; on other families `[]`.
    pub fn detect_plasma_forks(&self) -> Vec<String> {
        plan::filter_plasma_forks(&self.installed_packages(PkgFamily::Arch))
    }

    /// Port of `plasma_fork_recovery_command`: a human-readable command for a
    /// full rollback to stock (for display). `None`: no forks, or no steps came out.
    ///
    /// `memo` is the shared memory of `pacman -Si` probes: the same stock package
    /// shows up for several forks, and a full rollback goes over the set twice.
    fn recovery_command_with(
        &self,
        forks: &[String],
        memo: &RefCell<plan::TargetExistsMemo>,
    ) -> Option<String> {
        plan::plasma_fork_recovery_command(
            forks,
            &|p| self.pkg_meta_stdout(p, PkgFamily::Arch),
            &|p| {
                memo.borrow_mut()
                    .answer(p, &|n| self.repo_target_exists(n, PkgFamily::Arch))
            },
        )
    }

    /// Port of `plasma_fork_recovery_command` with its own probe memory.
    /// `forks = None`: the set is determined by a live package probe.
    pub fn plasma_fork_recovery_command(&self, forks: Option<&[String]>) -> Option<String> {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let memo = RefCell::new(plan::TargetExistsMemo::new());
        self.recovery_command_with(forks, &memo)
    }

    /// Port of `plasma_fork_reset_preview`: what will be done, WITHOUT a
    /// transaction. Read-only probes (`pacman -Qi`/`-Si`), `pkexec` is not touched.
    pub fn plasma_fork_reset_preview(&self, forks: Option<&[String]>) -> (bool, String) {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let answer = plan::plasma_fork_preview(
            forks,
            &|p| self.pkg_meta_stdout(p, PkgFamily::Arch),
            &|p| self.repo_target_exists(p, PkgFamily::Arch),
        );
        (answer.ok, answer.message)
    }

    /// Port of `remove_plasma_forks`: remove the forks and restore stock in ONE
    /// pkexec transaction (`pacman -Rns`, then `pacman -S <stock>`).
    ///
    /// ⛔ The `Blocked` branch is a refusal BEFORE pacman: removing a fork without
    /// having anything to restore stock with is exactly that half-broken system.
    /// The order of steps must not be changed (standalone forks depend on the
    /// provider).
    /// ⚠️ After success a logout/login is needed.
    pub fn remove_plasma_forks(&self, forks: Option<&[String]>) -> (bool, String) {
        let owned;
        let forks = match forks {
            Some(f) => f,
            None => {
                owned = self.detect_plasma_forks();
                &owned
            }
        };
        let memo = RefCell::new(plan::TargetExistsMemo::new());
        let qi = |p: &str| self.pkg_meta_stdout(p, PkgFamily::Arch);
        let exists = |p: &str| {
            memo.borrow_mut()
                .answer(p, &|n| self.repo_target_exists(n, PkgFamily::Arch))
        };
        let reset = plan::plasma_fork_reset_plan(forks, &qi, &exists);
        let (cmd, shell) = match reset {
            plan::PlasmaForkReset::NoForks { ok, message }
            | plan::PlasmaForkReset::NoActions { ok, message }
            | plan::PlasmaForkReset::Blocked { ok, message, .. } => return (ok, message),
            plan::PlasmaForkReset::Run { cmd, shell, .. } => (cmd, shell),
        };
        let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
        // Both recovery commands are computed LAZILY and only in their own branch:
        // "what we were going to run" on an authorization refusal (the system is
        // untouched), "what is left" by recomputing from the LIVE state after a
        // failure, since some steps could have gone through.
        let answer = plan::plasma_fork_reset_outcome(
            &shell,
            out.rc(),
            &out.stdout,
            &out.stderr,
            &|| self.recovery_command_with(forks, &memo),
            &|| self.plasma_fork_recovery_command(None),
        );
        (answer.ok, answer.message)
    }

    // ── Ending the session ──────────────────────────────────────────────

    /// Port of `logout_session`: end the current session (show the login
    /// screen). Does NOT power off the computer. The first successful command wins.
    ///
    /// ⭐ A timeout is called a timeout: in the original it was a separate
    /// `except` branch, and "did not start" was hidden in a general
    /// `except Exception`.
    pub fn logout_session(&self) -> (bool, String) {
        let cmds = plan::logout_cmds(
            &self.io.env("XDG_CURRENT_DESKTOP"),
            &self.io.env("DESKTOP_SESSION"),
            &self.io.env("XDG_SESSION_ID"),
        );
        let mut last_err = String::new();
        for cmd in &cmds {
            let out = self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c);
            let head = cmd.argv.first().cloned().unwrap_or_default();
            match out.outcome {
                RunOutcome::Ok => return (true, "ok".to_string()),
                RunOutcome::Failed => last_err = out.stderr.trim().to_string(),
                RunOutcome::Timeout => last_err = format!("{head}: timeout"),
                // TRANSLATORS: %1 is a command name.
                RunOutcome::Error => last_err = crate::i18n::trf("%1: didn't start", &[&head]),
            }
        }
        if last_err.is_empty() {
            (false, crate::i18n::tr("Couldn't log out"))
        } else {
            (false, last_err)
        }
    }

    // ── Emergency look rollback (`--recover-kde`) ───────────────────────
    //
    // Port of `rexon/app.py:682` and its nineteen helpers. The shortcut
    // «Restore desktop look» and the window menu item lead here, and the
    // person presses them when the look is ALREADY broken, so the route must
    // work without Qt at all and must not fail at any step. Hence its shape:
    // each step is best-effort, each says in words what it did, and none
    // cancels the following ones.
    //
    // Decisions and all texts: [`crate::app`]; here only the order and the
    // trips outside.

    /// Root of user configs of THIS process.
    fn config_home(&self) -> String {
        let xdg = self.io.env("XDG_CONFIG_HOME");
        let home = self.io.env("HOME");
        utils::config_home(
            if xdg.is_empty() { None } else { Some(xdg.as_str()) },
            &home,
        )
    }

    /// Path of the Plasma desktop and panel layout.
    fn appletsrc_path(&self) -> String {
        format!(
            "{}/plasma-org.kde.plasma.desktop-appletsrc",
            self.config_home()
        )
    }

    /// The whole `--recover-kde` route. → process exit code (0/1).
    ///
    /// `out` is the sink for lines for the person (`println!` in the product):
    /// the order and the words are exactly what the original printed.
    ///
    /// ⛔ BOUNDARY. With its own hands this route changes
    /// `plasma-org.kde.plasma.desktop-appletsrc` (panel surgery, with a backup
    /// next to it) and removes Rexon font layers: only our marked blocks and our
    /// files, by the core's plan (`qt_settings::font_layers_reset_plan`).
    /// Everything else is changed by other programs' commands:
    /// `plasma-apply-*`, `kwriteconfig`, `lookandfeeltool`, `systemctl
    /// --user`, and root-level ones only by `pkexec` from the core's plans.
    pub fn recover_kde(&self, out: &mut dyn FnMut(&str)) -> i32 {
        // Foolproof guard: only Plasma can be restored, and on another desktop we
        // say so out loud instead of pretending it worked.
        let xdg = self.io.env("XDG_CURRENT_DESKTOP");
        if !app::is_plasma_desktop(&xdg) {
            for line in app::not_plasma_lines(&xdg) {
                out(&line);
            }
            return 1;
        }

        // The theme backup location is shared with the window
        // ([`crate::window_menu::recover_backup_path`]): the dialog names to the
        // person the theme this route will apply.
        let home = self.io.env("HOME");
        let xdg = self.io.env("XDG_CONFIG_HOME");
        let backup = crate::window_menu::recover_backup_path(
            &home,
            if xdg.is_empty() { None } else { Some(xdg.as_str()) },
        );
        let saved = self.io.read_text(&backup);
        let target = app::recover_theme_target(saved.as_deref());

        // Heavy Plasma forks replace the Plasma binaries themselves and are NOT
        // cured by a theme reset: we warn BEFORE the reset and give the command for
        // a full rollback. Read-only probe, `pkexec` is not touched here.
        let forks = self.detect_plasma_forks();
        if !forks.is_empty() {
            let (can_reset, preview) = self.plasma_fork_reset_preview(Some(&forks));
            for line in app::fork_warning_lines(&forks, can_reset, &preview) {
                out(&line);
            }
        }

        out(&app::recovering_line(&target));
        let applied = self.io.run(
            &app::lookandfeel_apply_argv(&target),
            Some(app::LOOKANDFEEL_TIMEOUT_SECS),
            false,
        );
        if applied.rc() != Some(0) {
            // ⭐ Fixed in the port: in the original a missing
            // `plasma-apply-lookandfeel` raised FileNotFoundError straight out of
            // `main()`: the emergency route crashed with a stack trace exactly where
            // it had to explain what is going on.
            if applied.no_answer() {
                out(app::LOOKANDFEEL_MISSING_LINE);
            } else {
                out(&app::apply_failed_line(&applied.stderr));
            }
            return 1;
        }
        self.io.run(
            &app::colorscheme_apply_argv(),
            Some(app::COLORSCHEME_TIMEOUT_SECS),
            false,
        );
        out(&app::restored_line(&target));

        self.recover_kde_config(&target, &forks, out);
        self.recover_kde_wallpaper(out);
        0
    }

    /// Port of `_recover_kde_config`: full config reset (part 2).
    ///
    /// Resets to stock ALWAYS (fonts, KWin effects, Kvantum, boot splash and
    /// login screen): they are all idempotent, and one press of the button
    /// returns the state of a clean install. The aggressive part (cleaning the
    /// panel layout, which can wipe user widgets) only with Plasma forks or
    /// their traces in the config.
    pub fn recover_kde_config(
        &self,
        target: &str,
        forks: &[String],
        out: &mut dyn FnMut(&str),
    ) {
        self.reset_kde_fonts(out);
        self.reset_kwin_theme_effects(out);
        self.reset_kvantum_if_aero(out);
        self.reset_plymouth_step(out);
        self.reset_sddm_step(out);

        let aggressive = !forks.is_empty() || self.detect_plasma_fork_residue();
        if !aggressive {
            return;
        }
        out(app::PANEL_AGGRESSIVE_LINE);
        // SURGERY first: remove only the broken fork applets, keeping the user's
        // panel. If none were found and the layout is still "aero" (the
        // containment itself is replaced), then a full `--resetLayout`.
        if self.remove_fork_applets(out) == 0 && self.appletsrc_has_fork_residue() {
            self.reset_panel_layout(target, out);
        }
    }

    /// Port of `_reset_kde_fonts`: remove font overrides in kdeglobals.
    /// `--delete` removes the key, after which Plasma uses its own default:
    /// more reliable than writing a specific font.
    ///
    /// Together with the keys, the layers put by the «Fonts» page are removed
    /// ([`Self::reset_font_layers`]): otherwise menus and panels of GTK and Qt
    /// programs, antialiasing and DPI would stay ours.
    pub fn reset_kde_fonts(&self, out: &mut dyn FnMut(&str)) {
        self.reset_font_layers();
        // The document font in KDE lives in a GNOME key: removed together with the
        // page's other fonts.
        if let Some(gs) = self.io.which_path("gsettings") {
            self.io.run(&app::document_font_reset_argv(&gs), Some(app::KWRITECONFIG_TIMEOUT_SECS), false);
        }
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cmd in app::kde_font_reset_argv(&kw) {
            self.io
                .run(&cmd, Some(app::KWRITECONFIG_TIMEOUT_SECS), false);
        }
        out(app::FONTS_RESET_LINE);
    }

    /// Remove Rexon font layers (`qt_settings::font_layers_reset_plan`): our
    /// blocks in `gtk.css`, `fonts.conf`, `~/.Xresources`, the qt6ct/qt5ct style
    /// sheet with its line in `.conf`, the COSMIC fonts we wrote. Other content
    /// of the files stays. → how many files could not be restored (each as a
    /// line in the log).
    pub fn reset_font_layers(&self) -> usize {
        struct Fs<'b, IO: SystemIo + ?Sized>(&'b IO);
        impl<IO: SystemIo + ?Sized> qt_settings::FontLayerFs for Fs<'_, IO> {
            fn read(&self, path: &str) -> Option<String> {
                self.0.read_text(path)
            }
            fn is_link(&self, path: &str) -> bool {
                // A link: if the path itself resolves somewhere other than its directory
                // (the settings directory itself may be a link).
                let Some((dir, name)) = path.rsplit_once('/') else { return false };
                match (self.0.realpath(path), self.0.realpath(if dir.is_empty() { "/" } else { dir })) {
                    (Some(real), Some(real_dir)) => {
                        real != format!("{}/{name}", real_dir.trim_end_matches('/'))
                    }
                    _ => false,
                }
            }
        }
        let home = self.io.env("HOME");
        let plan = qt_settings::font_layers_reset_plan(&self.config_home(), &home, &Fs(self.io));
        let mut failed = 0;
        for undo in plan {
            let done = match &undo {
                qt_settings::FontLayerUndo::Write { path, text } => self.io.write_text(path, text),
                qt_settings::FontLayerUndo::Remove { path } => self.io.unlink_file(path),
                qt_settings::FontLayerUndo::Relink { path, target } => {
                    self.io.replace_with_symlink(path, target)
                }
            };
            if let Err(e) = done {
                failed += 1;
                self.log(format!("font layer reset: {undo:?}: {e}"));
            }
        }
        failed
    }

    /// Port of `_reset_kwin_theme_effects`: remove the fork themes' named effects
    /// and make the live KWin reread its config.
    pub fn reset_kwin_theme_effects(&self, out: &mut dyn FnMut(&str)) {
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cmd in app::kwin_effect_reset_argv(&kw) {
            self.io
                .run(&cmd, Some(app::KWRITECONFIG_TIMEOUT_SECS), false);
        }
        if !self.kwin_reconfigure() {
            self.log(app::KWIN_RECONFIGURE_FAILED_LOG.to_string());
        }
        out(app::KWIN_EFFECTS_RESET_LINE);
    }

    /// Port of `utils.kwin_reconfigure`: ask the live KWin to reread its config.
    /// → whether it worked.
    ///
    /// ⭐ Fixed in the port. In the original the fallback path was dead:
    /// `dbus_call` took the FIRST available binary (`dbus-send`, otherwise
    /// `qdbus`), and `_reset_kwin_reconfigure_fallback` asked the same resolver
    /// again, and either got the same `dbus-send` and left empty-handed, or got
    /// `qdbus` and repeated WORD FOR WORD the same command that had just failed.
    /// Here both binaries are tried in turn: if `dbus-send` does not answer, we
    /// try `qdbus`, and only then give up. Without this the kwinrc edit landed
    /// in the file, while the live KWin kept drawing the removed effects until
    /// the next login.
    pub fn kwin_reconfigure(&self) -> bool {
        let (dest, obj, method) = utils::KWIN_RECONFIGURE;
        for (tool, kind) in utils::DBUS_TOOL_PREFERENCE {
            let Some(exe) = self.io.which_path(tool) else {
                continue;
            };
            let argv = utils::dbus_call_argv(kind, &exe, dest, obj, method, &[]);
            let out = self
                .io
                .run(&argv, Some(app::KWIN_RECONFIGURE_TIMEOUT_SECS), false);
            if out.rc() == Some(0) {
                return true;
            }
        }
        false
    }

    /// Port of `_reset_kvantum_if_aero`: a Kvantum theme from the Aero family →
    /// Breeze widget style. A legitimate Kvantum (Dracula and relatives) is NOT
    /// touched.
    pub fn reset_kvantum_if_aero(&self, out: &mut dyn FnMut(&str)) {
        let path = format!("{}/{}", self.config_home(), app::KVANTUM_CONFIG_REL);
        if !self.io.is_file(&path) {
            return;
        }
        let Some(text) = self.io.read_text(&path) else {
            return;
        };
        // INI parsing with the core's common reader, no copy of the parser of our own.
        let cfg = crate::gtk_sanitizer::read_kde_ini(Some(&text));
        let theme = crate::gtk_sanitizer::kde_value(
            &cfg,
            app::KVANTUM_THEME_SECTION,
            app::KVANTUM_THEME_KEY,
        );
        if theme.is_empty() || !app::is_aero_kvantum_theme(&theme) {
            return;
        }
        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        self.io.run(
            &app::widget_style_breeze_argv(&kw),
            Some(app::KWRITECONFIG_TIMEOUT_SECS),
            false,
        );
        out(&app::kvantum_breeze_line(&theme));
    }

    /// Port of `_reset_plymouth`: restore the distro's NATIVE boot splash.
    ///
    /// Plymouth is system level (root + initramfs) and is not cured by a Plasma
    /// theme reset. Idempotent: if the theme is already stock nothing goes
    /// outside and the initramfs is not rebuilt.
    pub fn reset_plymouth_step(&self, out: &mut dyn FnMut(&str)) {
        let Some(current) = self.detect_plymouth_theme() else {
            return; // Plymouth is not installed: nothing to reset.
        };
        let target = self.target_plymouth_theme().unwrap_or_default();
        if target.is_empty() {
            out(app::PLYMOUTH_TARGET_UNKNOWN_LINE);
            return;
        }
        if current == target {
            out(&app::plymouth_already_stock_line(&target));
            return;
        }
        out(&app::plymouth_switch_line(&current, &target));
        let (ok, message) = self.reset_plymouth_to_default(Some(&target));
        if ok {
            out(&app::step_ok_line(&message));
            return;
        }
        out(&app::plymouth_failed_line(&message));
        if let Some(cmd) = self.plymouth_reset_command(Some(&target)) {
            out(&app::manual_command_line(&cmd));
        }
    }

    /// Port of `_reset_sddm`: return the login screen to the DISTRIBUTION stock.
    ///
    /// ⭐ Exactly what was shown to the person is deleted. The original computed
    /// the plan (`sddm_reset_plan`), printed it, and then called
    /// `reset_sddm_to_default()`, which scanned `/etc/sddm.conf.d` AGAIN and
    /// deleted the result of the second scan. Between the scans the state could
    /// change, and something other than promised was deleted. Here the plan is
    /// computed once and executed as is.
    ///
    /// Idempotent: if there is nothing to delete, `pkexec` does not run at all,
    /// and on a clean system the person is not asked for a password in vain.
    pub fn reset_sddm_step(&self, out: &mut dyn FnMut(&str)) {
        let conf_d = plan::SDDM_CONF_D;
        let reset_plan = self.sddm_reset_plan(conf_d);
        if reset_plan.to_remove.is_empty() {
            // The distro default comes from the ALREADY computed plan, not from a
            // second scan of the same directories: the answer is the same, there are
            // half as many file system trips, and the two answers have no way to diverge.
            out(&app::sddm_already_stock_line(
                reset_plan.default_theme.as_deref(),
            ));
            return;
        }
        out(&app::sddm_removing_line(
            &reset_plan.to_remove,
            reset_plan.default_theme.as_deref(),
        ));
        let (ok, message, _removed) = self.apply_sddm_reset(&reset_plan, conf_d);
        if ok {
            out(&app::step_ok_line(&message));
        } else {
            out(&app::sddm_failed_line(&message));
        }
    }

    /// Port of `_detect_plasma_fork_residue`: whether TRACES of a fork theme
    /// remain in the config when the packages themselves are already removed.
    ///
    /// We look at the panel layout and the login screen configs. What counts as
    /// a fork token is decided by the core ([`app::has_fork_token`]); here only
    /// reading.
    pub fn detect_plasma_fork_residue(&self) -> bool {
        let (_, sddm_dir, sddm_conf) = app::fork_residue_files(&self.io.env("HOME"));
        let mut candidates = vec![self.appletsrc_path()];
        // The `/etc/sddm.conf.d/*.conf` glob is expanded here; the order is set by
        // us, not by arbitrary file system traversal.
        let mut confs: Vec<String> = self
            .io
            .list_dir(sddm_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n.ends_with(".conf"))
            .map(|n| format!("{sddm_dir}/{n}"))
            .collect();
        confs.sort();
        candidates.extend(confs);
        candidates.push(sddm_conf.to_string());

        for path in candidates {
            if !self.io.is_file(&path) {
                continue;
            }
            let Some(text) = self.io.read_text(&path) else {
                continue;
            };
            if app::has_fork_token(&text) {
                return true;
            }
        }
        false
    }

    /// Port of `_remove_fork_applets`: surgically remove the fork theme's broken
    /// applets from the layout WITHOUT wiping the panel. → how many were removed.
    ///
    /// A backup is put next to it before overwriting. Parsing and the decision
    /// on "what is foreign" belong to the core ([`app::strip_fork_applets`]).
    pub fn remove_fork_applets(&self, out: &mut dyn FnMut(&str)) -> usize {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return 0;
        }
        let Some(text) = self.io.read_text(&appletsrc) else {
            return 0;
        };
        let (new_text, removed) = app::strip_fork_applets(&text);
        if removed.is_empty() {
            return 0;
        }
        let backup = app::appletsrc_backup_path(&appletsrc);
        if let Err(e) = self.io.copy_file(&appletsrc, &backup) {
            out(&app::applets_rewrite_failed_line(&e));
            return 0;
        }
        if let Err(e) = self.io.write_text(&appletsrc, &new_text) {
            out(&app::applets_rewrite_failed_line(&e));
            return 0;
        }
        out(&app::applets_removed_line(&removed, &backup));
        removed.len()
    }

    /// Port of `_appletsrc_has_fork_residue`: whether fork tokens remain in the
    /// layout AFTER the surgery (meaning the containment itself is replaced).
    pub fn appletsrc_has_fork_residue(&self) -> bool {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return false;
        }
        self.io
            .read_text(&appletsrc)
            .map(|t| app::has_fork_token(&t))
            .unwrap_or(false)
    }

    /// Port of `_reset_panel_layout`: restore the stock panel layout entirely.
    /// ⚠️ Wipes ALL user panels and widgets, which is why it is called only on
    /// fork theme traces, when surgery did not help.
    pub fn reset_panel_layout(&self, target: &str, out: &mut dyn FnMut(&str)) {
        let Some(tool) = self.io.which_path(app::LOOKANDFEELTOOL_BIN) else {
            out(app::LOOKANDFEELTOOL_MISSING_LINE);
            return;
        };
        self.io.run(
            &app::panel_reset_layout_argv(&tool, target),
            Some(app::PANEL_RESET_TIMEOUT_SECS),
            false,
        );
        out(app::PANEL_LAYOUT_RESET_LINE);
    }

    /// Port of `_recover_kde_wallpaper`: return the wallpapers of ALL desktops to
    /// the default ones and restart plasmashell.
    ///
    /// Stops any live and video wallpapers (the plugin goes back to
    /// `org.kde.image`): the window dialog warns the person about this before
    /// the run, otherwise vanished wallpapers read as a Rexon breakage.
    pub fn recover_kde_wallpaper(&self, out: &mut dyn FnMut(&str)) {
        let appletsrc = self.appletsrc_path();
        if !self.io.is_file(&appletsrc) {
            return;
        }
        // Wallpaper packages are iterated alphabetically: directory traversal is
        // arbitrary, and picking "the first one found" must be reproducible.
        let mut packages = self.io.list_dir(app::WALLPAPERS_ROOT).unwrap_or_default();
        packages.sort();
        let default_wp = app::default_wallpaper_dir(
            &|p| self.io.is_dir(p),
            &packages,
            &|p| self.io.exists(p),
        );

        let Some(text) = self.io.read_text(&appletsrc) else {
            return;
        };
        let mut desktops = app::desktop_containment_ids(&text);
        if desktops.is_empty() {
            desktops.push(app::FALLBACK_CONTAINMENT.to_string());
        }

        let Some(kw) = self.io.which_path(app::KWRITECONFIG_TOOL) else {
            return;
        };
        for cid in &desktops {
            self.io.run(
                &app::wallpaper_plugin_reset_argv(&kw, cid),
                Some(app::KWRITECONFIG_TIMEOUT_SECS),
                false,
            );
            if !default_wp.is_empty() {
                self.io.run(
                    &app::wallpaper_image_argv(&kw, cid, &default_wp),
                    Some(app::KWRITECONFIG_TIMEOUT_SECS),
                    false,
                );
            }
        }
        out(&app::wallpaper_reset_line(desktops.len(), &default_wp));

        // Restart plasmashell so what was written is picked up now, not after the
        // next login.
        for (cmd, timeout) in app::plasmashell_restart_argv()
            .iter()
            .zip(app::PLASMASHELL_RESTART_TIMEOUTS)
        {
            self.io.run(cmd, Some(*timeout), false);
        }
    }
}

// ============================================================================
// Live executor
// ============================================================================

fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// NON-strict `os.path.realpath`: walks the path link by link and follows
/// symlinks, but does NOT require the final target to exist.
///
/// ⚠️ Why our own and not `std::fs::canonicalize`. That one is strict: on a
/// dangling symlink (and that is the normal state of
/// `/etc/systemd/system/display-manager.service` after `pacman -R sddm`) it
/// returns an error, and the dead target would stay nameless. It was exactly
/// this namelessness that kept the foolproof guard silent.
pub fn realpath_lenient(path: &str) -> String {
    use std::ffi::OsString;

    let start = Path::new(path);
    let mut out = if start.is_absolute() {
        PathBuf::from("/")
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
    };
    let split = |p: &Path| -> Vec<OsString> {
        let mut v: Vec<OsString> = p
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => Some(s.to_os_string()),
                std::path::Component::ParentDir => Some(OsString::from("..")),
                _ => None,
            })
            .collect();
        v.reverse(); // pop from the tail → the order stays forward
        v
    };
    let mut stack = split(start);
    // Hop budget, like the kernel's ELOOP: a symlink loop must not hang the
    // executor.
    let mut budget = 40;
    while let Some(name) = stack.pop() {
        if name == "" {
            continue;
        }
        if name == ".." {
            out.pop();
            continue;
        }
        out.push(&name);
        if budget == 0 {
            continue;
        }
        if let Ok(target) = std::fs::read_link(&out) {
            budget -= 1;
            out.pop();
            if target.is_absolute() {
                out = PathBuf::from("/");
            }
            stack.extend(split(&target));
        }
    }
    out.to_string_lossy().into_owned()
}

/// The real executor: live file systems, `PATH` and subprocesses.
///
/// ⛔ Everything it can CHANGE is changed in a single way: by running a ready
/// `argv` from the core's plan. It has no commands of its own; it writes,
/// deletes and replaces with a link only the paths the core's plan named.
pub struct LiveSystemIo;

impl SystemIo for LiveSystemIo {
    fn run_priv(
        &self,
        op: &crate::privilege::op::PrivOp,
        files: &[String],
        log_path: Option<&str>,
    ) -> Option<RunOut> {
        crate::privilege::run::run_live(op, files, log_path, &mut |_| {})
    }

    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_file(&self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn is_executable(&self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    fn realpath(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        Some(realpath_lenient(path))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        std::fs::read(path)
            .ok()
            .map(|raw| String::from_utf8_lossy(&raw).into_owned())
    }

    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        let entries = std::fs::read_dir(dir).ok()?;
        Some(
            entries
                .flatten()
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .collect(),
        )
    }

    fn which(&self, name: &str) -> bool {
        // Port of `safe_check_command`: for names with aliases the answer covers ALL
        // variants, otherwise the check and the following run diverge.
        let path = path_env();
        if utils::tool_has_variants(name) {
            return utils::tool_argv(name, &[], path.as_deref()).is_some();
        }
        utils::lookup_tool(&[name], &[], path.as_deref()).is_some()
    }

    fn run(&self, argv: &[String], timeout_secs: Option<u64>, lang_c: bool) -> RunOut {
        if argv.is_empty() {
            return RunOut::missing();
        }
        let spec = RunSpec {
            argv: argv.to_vec(),
            output: RunOutput::Capture,
            timeout_secs,
            lang_c,
            ..RunSpec::default()
        };
        let report = utils::run(&spec, path_env().as_deref());
        RunOut {
            outcome: report.outcome,
            returncode: report.returncode,
            stdout: utils::decode_text(&report.stdout),
            stderr: utils::decode_text(&report.stderr),
        }
    }

    fn env(&self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn which_path(&self, name: &str) -> Option<String> {
        utils::resolve_tool(name, &[], path_env().as_deref())
    }

    /// A whole-file write through the core's SHARED door, not `fs::write`.
    ///
    /// Through this door, among other things, the default application protection
    /// reference (`SystemDefaultsProtector.snapshot`) goes into the user's
    /// profile; since 19.09 it is taken automatically on first run. A plain
    /// write truncates the file first: a window closed in that half second left
    /// a stub on disk. Here the target is either the old file or the new one in
    /// full.
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        utils::atomic_write_str_err(path, content)
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        std::fs::copy(src, dst).map(|_| ()).map_err(|e| e.to_string())
    }

    fn unlink_file(&self, path: &str) -> Result<(), String> {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }

    fn replace_with_symlink(&self, path: &str, target: &str) -> Result<(), String> {
        let tmp = format!("{path}.rexon-link-{}", std::process::id());
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(target, &tmp).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

// ============================================================================
// «Login manager» section: snapshot for the UI
// ============================================================================

/// A LightDM greeter in a section row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginGreeterRow {
    /// File name without `.desktop`: what goes to the helper.
    pub id: String,
    /// Name for the person (`Name=`, empty means `id`).
    pub name: String,
    /// Confirmation dialog text with this greeter.
    pub confirm: String,
}

/// A section row: one installed login screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginManagerRow {
    pub unit: String,
    /// Name for the person (an unknown one: the unit name).
    pub label: String,
    /// Default: the link points to it, the next boot will start with it.
    pub active: bool,
    /// Running now (`systemctl is-active`).
    pub running: bool,
    /// `ready` | `needs_greeter` | `blocked` | `unsupported`.
    pub state: &'static str,
    /// Why there is no button (grey under the name); empty means no reason.
    pub reason: String,
    /// Greeters to choose from (LightDM only).
    pub greeters: Vec<LoginGreeterRow>,
    /// Preselected greeter (`id`); empty means nothing to choose from.
    pub greeter: String,
    /// The «Make default» button is available.
    pub can_switch: bool,
    /// Confirmation dialog title.
    pub confirm_title: String,
    /// Confirmation dialog text without a greeter choice.
    pub confirm: String,
    /// Warnings that do not prevent switching.
    pub warnings: Vec<String>,
}

/// The whole «Login manager» section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginScreenView {
    pub managers: Vec<LoginManagerRow>,
    /// Selection mechanism: `systemd` | `debian` | `suse` | `nixos` | `other`.
    pub backend: &'static str,
    /// Rexon switches the login screen on this system.
    pub switchable: bool,
    /// Why there are no buttons on the whole system; empty means there are.
    pub note: String,
    /// The default is already another one, but the old one is still running: it
    /// takes effect after a reboot. Empty means no mismatch.
    pub pending: String,
    /// «How to restore the previous login screen»: the text of the file the
    /// helper left for the console ([`plan::PREVIOUS_DM_FILE`]); empty means no file.
    pub rollback: String,
    /// What installed desktops lose under the DEFAULT login screen
    /// ([`plan::dm_compat_issues`]): first "the desktop does not work", then
    /// minor things. Empty means it suits all desktops. Other rows have the same
    /// in their `warnings` (what happens if they are made the default).
    pub compat: Vec<String>,
}

/// Observations for [`login_screen_view`], gathered by the glue code, read only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginScreenFacts {
    pub family: PkgFamily,
    /// The `rexon-pkg-helper` helper is deployed together with the polkit policy.
    pub helper: bool,
    /// Immutable system ([`DistroDetector::is_immutable`]).
    pub immutable: bool,
    /// Login screens that are running now.
    pub running: Vec<String>,
    /// Text of [`plan::PREVIOUS_DM_FILE`], if the file exists.
    pub previous: Option<String>,
    /// Installed desktops that care which login screen is used
    /// ([`SystemExec::installed_dm_sensitive_desktops`]).
    pub desktops: Vec<String>,
    /// What happens to autologin when switching to a unit:
    /// `(unit, warning)` ([`crate::dm_autologin::transfer_note`]).
    pub autologin_notes: Vec<(String, String)>,
}

fn backend_key(backend: &plan::DmSwitchBackend) -> &'static str {
    match backend {
        plan::DmSwitchBackend::Systemd => "systemd",
        plan::DmSwitchBackend::SystemdDebian => "debian",
        plan::DmSwitchBackend::SystemdSuseAlternatives => "suse",
        plan::DmSwitchBackend::NixosDeclarative => "nixos",
        plan::DmSwitchBackend::Unsupported(_) => "other",
    }
}

fn row_label(info: &plan::LoginManagerInfo) -> String {
    if info.label.trim().is_empty() { info.unit.clone() } else { info.label.clone() }
}

/// Immutable system: Rexon does not touch the login screen yet.
pub fn login_manager_immutable_note() -> String {
    crate::i18n::tr(
        "On this system the login screen is part of the read-only base, so Rexon doesn't change it yet.",
    )
}

/// Refusal reason: there is no such login screen on the machine.
pub fn login_manager_missing_reason() -> String {
    crate::i18n::tr("this login screen isn't installed")
}

/// Refusal reason and row caption: Rexon does not make this login screen the
/// default (no readiness checks: Ly, LXDM, slim…).
pub fn login_manager_cant_switch_reason() -> String {
    crate::i18n::tr("Rexon can't make this login screen the default yet.")
}

/// Refusal: the selected one is already the default.
pub fn login_manager_already_default(label: &str, unit: &str) -> String {
    let name = if label.trim().is_empty() { unit } else { label };
    // TRANSLATORS: %1 is a login screen name (SDDM, LightDM, …).
    crate::i18n::trf("%1 is already the login screen.", &[&name])
}

/// Refusal: LightDM has no greeter selected from the installed ones.
pub fn login_manager_choose_greeter() -> String {
    crate::i18n::tr("First choose which greeter LightDM should show.")
}

/// The default and the running one differ, until a reboot.
pub fn login_manager_pending_note(new_label: &str, running_label: &str) -> String {
    crate::i18n::trf(
        // TRANSLATORS: %1 is the login screen chosen for the next boot, %2 the one running now.
        "%1 becomes the login screen after a restart. %2 is still running now.",
        &[&new_label, &running_label],
    )
}

/// Confirmation text + autologin warning (BEFORE the password).
fn with_autologin_note(confirm: String, unit: &str, facts: &LoginScreenFacts) -> String {
    match facts.autologin_notes.iter().find(|(u, _)| u == unit) {
        Some((_, note)) => format!("{confirm}\n\n{note}"),
        None => confirm,
    }
}

/// Decision for the «Login manager» section: a pure function over the
/// [`SystemExec::login_managers`] list and observations.
///
/// The button exists only where pressing it will really work (rule #13): a
/// systemd mechanism Rexon supports, not an immutable system, the helper in
/// place, the row is not the default, the program exists and readiness is
/// `Ready` or "choose a greeter". Otherwise the reason in words, in grey,
/// BEFORE the password, not after it.
pub fn login_screen_view(
    list: &[plan::LoginManagerInfo],
    backend: &plan::DmSwitchBackend,
    facts: &LoginScreenFacts,
) -> LoginScreenView {
    let note = plan::login_manager_unsupported_note(backend, None)
        .or_else(|| facts.immutable.then(login_manager_immutable_note))
        .or_else(|| (!facts.helper).then(plan::login_manager_needs_helper))
        .unwrap_or_default();
    let switchable = note.is_empty();
    let old = list.iter().find(|m| m.active).map(|m| m.unit.as_str());
    // Compatibility with desktops (read only): for the default one as a common
    // section line, for the others only what would be added to it when
    // switching to them (what was already said is not repeated). The same
    // lines go into the confirmation dialog, so they are seen BEFORE the password.
    let now_issues = old.map(|u| plan::dm_compat_issues(u, &facts.desktops)).unwrap_or_default();
    let compat: Vec<String> = now_issues.iter().map(|i| i.text.clone()).collect();
    let managers = list
        .iter()
        .map(|info| {
            let added: Vec<String> = if info.active {
                Vec::new()
            } else {
                // Compare by desktop, not by line: the login screen name in the texts is
                // different for each.
                plan::dm_compat_issues(&info.unit, &facts.desktops)
                    .into_iter()
                    .filter(|i| !now_issues.iter().any(|n| n.de_id == i.de_id))
                    .map(|i| i.text)
                    .collect()
            };
            let with_compat = |text: String| {
                if added.is_empty() {
                    text
                } else {
                    let mut parts = vec![text];
                    parts.extend(added.iter().cloned());
                    parts.join("\n\n")
                }
            };
            let (state, mut reason, greeters) = match &info.readiness {
                plan::DmReadiness::Ready => ("ready", String::new(), Vec::new()),
                plan::DmReadiness::NeedsGreeterChoice(list) => ("needs_greeter", String::new(), list.clone()),
                plan::DmReadiness::Blocked(r) => ("blocked", r.clone(), Vec::new()),
                plan::DmReadiness::Unsupported => ("unsupported", String::new(), Vec::new()),
            };
            if state == "unsupported" && !info.active {
                reason = match backend {
                    // Without systemd: the command for this init instead of the button.
                    plan::DmSwitchBackend::Unsupported(init) => plan::login_manager_unsupported_note(
                        backend,
                        plan::dm_manual_switch_command(init, facts.family, &info.unit).as_deref(),
                    )
                    .unwrap_or_default(),
                    _ if switchable => login_manager_cant_switch_reason(),
                    // One reason for the whole system: it is in `note`.
                    _ => String::new(),
                };
            }
            let can_switch = switchable
                && !info.active
                && info.present
                && matches!(state, "ready" | "needs_greeter");
            let suggested = info.suggested_greeter().map(|g| g.id.clone()).unwrap_or_default();
            LoginManagerRow {
                unit: info.unit.clone(),
                label: row_label(info),
                active: info.active,
                running: facts.running.iter().any(|u| *u == info.unit),
                state,
                reason,
                greeters: greeters
                    .iter()
                    .map(|g| LoginGreeterRow {
                        id: g.id.clone(),
                        name: if g.name.trim().is_empty() { g.id.clone() } else { g.name.clone() },
                        confirm: with_compat(with_autologin_note(
                            plan::login_manager_switch_confirm(old, &info.unit, Some(g), backend),
                            &info.unit,
                            facts,
                        )),
                    })
                    .collect(),
                greeter: suggested,
                can_switch,
                confirm_title: plan::login_manager_switch_title(&info.unit),
                confirm: with_compat(with_autologin_note(
                    plan::login_manager_switch_confirm(old, &info.unit, None, backend),
                    &info.unit,
                    facts,
                )),
                warnings: added.iter().cloned().chain(info.warnings.iter().cloned()).collect(),
            }
        })
        .collect::<Vec<_>>();
    let pending = match (
        managers.iter().find(|m| m.active),
        managers.iter().find(|m| m.running && !m.active),
    ) {
        (Some(main), Some(now)) if !main.running => login_manager_pending_note(&main.label, &now.label),
        _ => String::new(),
    };
    LoginScreenView {
        managers,
        backend: backend_key(backend),
        switchable,
        note,
        pending,
        rollback: facts.previous.as_deref().map(str::trim).unwrap_or_default().to_string(),
        compat,
    }
}

/// Executor on top of the live machine: what the product uses.
pub fn live() -> SystemExec<'static, LiveSystemIo> {
    SystemExec::new(&LiveSystemIo)
}
