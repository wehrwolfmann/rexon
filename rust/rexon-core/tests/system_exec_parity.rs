//! Checks of the live executor of the system layer
//! ([`rexon_core::system_exec`]).
//!
//! HOW THIS IS TESTED. The executor runs on STAND-IN I/O
//! ([`FakeIo`]): a stand-in file system, a stand-in `PATH`, stand-in
//! command answers. Not a single file of the live machine is read, not a single subprocess
//! is started, `pkexec` is never called. This is exactly why the
//! executor is split into "decision" (the core) and "action" (the trait): this
//! code can remove Plasma packages, rebuild the initramfs and delete the login
//! screen config — it must not touch a live machine, yet the order of steps
//! and the observations must be checked.
//!
//! ⚠️ WATCHDOG. The `guard_*` checks watch not texts but boundaries:
//!   * `guard_only_pkexec_changes_system` — during the whole emergency rollback
//!     the executor must not run A SINGLE modifying command other than
//!     `pkexec` from the core's plan;
//!   * `guard_dangling_symlink_is_not_login_screen` — a dangling
//!     `display-manager.service` symlink does not count as a login screen (otherwise the
//!     foolproofing stays silent exactly when it is needed);
//!   * `guard_blocked_fork_reset_runs_nothing` — when there is nothing to restore the stock with,
//!     `pacman -Rns` is not run at all;
//!   * `guard_idempotent_resets_run_nothing` — resetting an already stock Plymouth
//!     and SDDM does not go outside at all.
//! All four are tested for their bite: removing the corresponding condition in the
//! executor fails exactly them.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use rexon_core::system::PkgFamily;
use rexon_core::system_exec::{RunOut, SystemExec, SystemIo};
use rexon_core::system_plan as plan;
use rexon_core::utils::RunOutcome;

// ============================================================================
// Stand-in executor
// ============================================================================

/// A recorded side call: operation name + argument.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Op(String, String);

#[derive(Clone)]
struct Reply {
    outcome: RunOutcome,
    returncode: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Reply {
    fn ok(stdout: &str) -> Self {
        Reply {
            outcome: RunOutcome::Ok,
            returncode: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }
    fn failed(rc: i32, stdout: &str, stderr: &str) -> Self {
        Reply {
            outcome: RunOutcome::Failed,
            returncode: Some(rc),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }
    fn timeout() -> Self {
        Reply {
            outcome: RunOutcome::Timeout,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
    /// The command did not start at all (no binary, no permission).
    fn error() -> Self {
        Reply {
            outcome: RunOutcome::Error,
            returncode: None,
            stdout: String::new(),
            stderr: String::new(),
        }
    }
}

#[derive(Default)]
struct FakeIo {
    /// Regular files: path → contents.
    files: BTreeMap<String, String>,
    /// Directories.
    dirs: BTreeSet<String>,
    /// Symlinks: path → target (the target may NOT exist — a dangling symlink).
    links: BTreeMap<String, String>,
    /// Files with the executable bit.
    execs: BTreeSet<String>,
    /// What is "in PATH".
    tools: BTreeSet<String>,
    /// Environment variables.
    env: BTreeMap<String, String>,
    /// Command answers by the full argv (`join(" ")`).
    replies: BTreeMap<String, Reply>,
    /// Log of side calls in execution order.
    log: RefCell<Vec<Op>>,
    /// Files WRITTEN by the executor: path → contents. Kept separate from
    /// `files` on purpose — this shows what the route touched on disk, and
    /// the watchdog has something to watch.
    written: RefCell<BTreeMap<String, String>>,
    /// Paths whose writing must fail (no permission, no space).
    readonly: BTreeSet<String>,
    /// The window has a privilege session: the session helper's answer to any operation.
    session: Option<Reply>,
    /// Operations that went to the session helper.
    priv_ops: RefCell<Vec<rexon_core::privilege::op::PrivOp>>,
}

impl FakeIo {
    fn new() -> Self {
        FakeIo::default()
    }

    fn file(mut self, path: &str, content: &str) -> Self {
        self.files.insert(path.to_string(), content.to_string());
        // Parent directories exist — otherwise `list_dir` would have to be
        // declared in every test.
        let mut p = path;
        while let Some((dir, _)) = p.rsplit_once('/') {
            if dir.is_empty() {
                break;
            }
            self.dirs.insert(dir.to_string());
            p = dir;
        }
        self
    }

    fn dir(mut self, path: &str) -> Self {
        self.dirs.insert(path.to_string());
        self
    }

    fn link(mut self, path: &str, target: &str) -> Self {
        self.links.insert(path.to_string(), target.to_string());
        self
    }

    /// File + executable bit (that is how a deployed privileged helper lies).
    fn exec_file(mut self, path: &str) -> Self {
        self.execs.insert(path.to_string());
        self.file(path, "#!/bin/bash\n")
    }

    fn tool(mut self, name: &str) -> Self {
        self.tools.insert(name.to_string());
        self
    }

    fn env_var(mut self, name: &str, value: &str) -> Self {
        self.env.insert(name.to_string(), value.to_string());
        self
    }

    fn reply(mut self, argv: &str, r: Reply) -> Self {
        self.replies.insert(argv.to_string(), r);
        self
    }

    /// Deployed polkit + all three Rexon helpers — exactly where
    /// the engine looks on THIS machine.
    ///
    /// ⚠️ Not the `*_HELPER_PATH` constant: there are two directories and both are legitimate
    /// (`/usr/lib` for the package, `/usr/local/lib` for a manual install), and
    /// `SystemExec` asks `plan::*_helper_path()` for the path. While the manual install
    /// path stood here, the checks were green only on a machine without the
    /// package: install our own package — and the fake laid out the helper
    /// not at the path the engine was going to run.
    fn with_helpers(self) -> Self {
        self.tool("pkexec")
            .exec_file(plan::pkg_helper_path())
            .file(plan::PKG_POLICY_PATH, "<policyconfig/>")
            .exec_file(plan::de_helper_path())
            .file(plan::DE_POLICY_PATH, "<policyconfig/>")
            .exec_file(plan::stage_helper_path())
            .file(plan::STAGE_POLICY_PATH, "<policyconfig/>")
    }

    fn resolve(&self, path: &str) -> String {
        let mut p = path.to_string();
        for _ in 0..10 {
            match self.links.get(&p) {
                Some(t) => p = t.clone(),
                None => break,
            }
        }
        p
    }

    fn ops(&self) -> Vec<Op> {
        self.log.borrow().clone()
    }

    /// argv of all started commands, in start order.
    fn commands(&self) -> Vec<String> {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "run")
            .map(|o| o.1)
            .collect()
    }

    fn readonly_path(mut self, path: &str) -> Self {
        self.readonly.insert(path.to_string());
        self
    }

    /// How many times the directory was listed.
    fn lists_of(&self, dir: &str) -> usize {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "list_dir" && o.1 == dir)
            .count()
    }

    /// What the route wrote to disk: (path, contents), in path order.
    fn written(&self) -> Vec<(String, String)> {
        self.written.borrow().clone().into_iter().collect()
    }

    /// How many times the path was read.
    fn reads_of(&self, path: &str) -> usize {
        self.ops()
            .into_iter()
            .filter(|o| o.0 == "read_text" && o.1 == path)
            .count()
    }
}

impl SystemIo for FakeIo {
    fn run_priv(
        &self,
        op: &rexon_core::privilege::op::PrivOp,
        _files: &[String],
        _log: Option<&str>,
    ) -> Option<RunOut> {
        let r = self.session.clone()?;
        self.priv_ops.borrow_mut().push(op.clone());
        Some(RunOut { outcome: r.outcome, returncode: r.returncode, stdout: r.stdout, stderr: r.stderr })
    }

    fn exists(&self, path: &str) -> bool {
        let r = self.resolve(path);
        self.files.contains_key(&r) || self.dirs.contains(&r)
    }

    fn is_file(&self, path: &str) -> bool {
        self.files.contains_key(&self.resolve(path))
    }

    fn is_dir(&self, path: &str) -> bool {
        self.dirs.contains(&self.resolve(path))
    }

    fn is_executable(&self, path: &str) -> bool {
        let r = self.resolve(path);
        self.files.contains_key(&r) && self.execs.contains(&r)
    }

    fn realpath(&self, path: &str) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        Some(self.resolve(path))
    }

    fn read_text(&self, path: &str) -> Option<String> {
        self.log
            .borrow_mut()
            .push(Op("read_text".into(), path.into()));
        let r = self.resolve(path);
        // What was written overrides the original: a route that rewrote a file
        // must read ITS OWN contents, not the test's addition.
        if let Some(text) = self.written.borrow().get(&r) {
            return Some(text.clone());
        }
        self.files.get(&r).cloned()
    }

    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        let d = self.resolve(dir);
        self.log.borrow_mut().push(Op("list_dir".into(), d.clone()));
        if !self.dirs.contains(&d) {
            return None;
        }
        let prefix = format!("{}/", d.trim_end_matches('/'));
        let mut names: BTreeSet<String> = BTreeSet::new();
        for key in self.files.keys().chain(self.dirs.iter()) {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if !rest.is_empty() && !rest.contains('/') {
                    names.insert(rest.to_string());
                }
            }
        }
        // The order is DELIBERATELY reversed: walking a real FS is arbitrary, and
        // the order must be set by the core, not by the executor.
        Some(names.into_iter().rev().collect())
    }

    fn which(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    fn run(&self, argv: &[String], _timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
        let key = argv.join(" ");
        self.log.borrow_mut().push(Op("run".into(), key.clone()));
        match self.replies.get(&key) {
            Some(r) => RunOut {
                outcome: r.outcome,
                returncode: r.returncode,
                stdout: r.stdout.clone(),
                stderr: r.stderr.clone(),
            },
            None => RunOut::missing(),
        }
    }

    fn env(&self, name: &str) -> String {
        self.env.get(name).cloned().unwrap_or_default()
    }

    fn which_path(&self, name: &str) -> Option<String> {
        // Name synonyms (`kwriteconfig6` → `kwriteconfig5`) — as in the live
        // resolver: otherwise the check on a machine with Plasma 5 would diverge from
        // the product.
        let (names, _dirs) = rexon_core::utils::tool_candidates(name);
        names
            .into_iter()
            .find(|n| self.tools.contains(*n))
            .map(|n| format!("/usr/bin/{n}"))
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        let r = self.resolve(path);
        self.log.borrow_mut().push(Op("write_text".into(), r.clone()));
        if self.readonly.contains(&r) {
            return Err("Permission denied".to_string());
        }
        self.written.borrow_mut().insert(r, content.to_string());
        Ok(())
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        let (s, d) = (self.resolve(src), self.resolve(dst));
        self.log
            .borrow_mut()
            .push(Op("copy_file".into(), format!("{s} -> {d}")));
        if self.readonly.contains(&d) {
            return Err("Permission denied".to_string());
        }
        let text = self
            .written
            .borrow()
            .get(&s)
            .cloned()
            .or_else(|| self.files.get(&s).cloned())
            .ok_or_else(|| "No such file or directory".to_string())?;
        self.written.borrow_mut().insert(d, text);
        Ok(())
    }
}

/// A CachyOS (Arch) machine — the common base of half the checks.
fn arch() -> FakeIo {
    FakeIo::new()
        .file("/etc/os-release", "ID=cachyos\nID_LIKE=arch\nNAME=CachyOS\n")
        .tool("pacman")
}

const DM_LINK: &str = "/etc/systemd/system/display-manager.service";

// ============================================================================
// Package engine and distribution
// ============================================================================

#[test]
fn family_read_once_and_cached() {
    let _lang = i18n_pin::en();
    let io = arch();
    let ex = SystemExec::new(&io);
    assert_eq!(ex.family(), PkgFamily::Arch);
    assert_eq!(ex.family(), PkgFamily::Arch);
    assert_eq!(ex.family(), PkgFamily::Arch);
    // The original (`_is_pkg_installed`) created a DistroDetector on EVERY call.
    assert_eq!(io.reads_of("/etc/os-release"), 1);
}

#[test]
fn installed_packages_parsed_from_pacman() {
    let _lang = i18n_pin::en();
    let io = arch().reply("pacman -Qq", Reply::ok("bash\nplasma-desktop\nsddm\n"));
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.installed_packages(PkgFamily::Arch),
        vec!["bash", "plasma-desktop", "sddm"]
    );
}

#[test]
fn installed_packages_empty_without_manager() {
    let _lang = i18n_pin::en();
    // No binary → the command is NOT started at all (port of `_run_pkg_query`).
    let io = FakeIo::new().file("/etc/os-release", "ID=arch\n");
    let ex = SystemExec::new(&io);
    assert!(ex.installed_packages(PkgFamily::Arch).is_empty());
    assert!(io.commands().is_empty());
}

#[test]
fn is_pkg_installed_reads_return_code() {
    let _lang = i18n_pin::en();
    let io = arch()
        .reply("pacman -Q bash", Reply::ok("bash 5.2\n"))
        .reply("pacman -Q nope", Reply::failed(1, "", "error: package not found"));
    let ex = SystemExec::new(&io);
    assert!(ex.is_pkg_installed("bash"));
    assert!(!ex.is_pkg_installed("nope"));
    // Did not start at all — also "not installed", without a panic.
    assert!(!ex.is_pkg_installed("unasked"));
}

#[test]
fn repo_target_exists_tells_no_from_cannot_ask() {
    let _lang = i18n_pin::en();
    let io = arch()
        .reply("pacman -Si libplasma", Reply::ok("Repository : extra\nVersion : 6.5\n"))
        .reply("pacman -Si plasma-framework", Reply::failed(1, "", "error: target not found"));
    let ex = SystemExec::new(&io);
    assert_eq!(ex.repo_target_exists("libplasma", PkgFamily::Arch), Some(true));
    assert_eq!(
        ex.repo_target_exists("plasma-framework", PkgFamily::Arch),
        Some(false)
    );
    // There was no answer — this is NOT "no such package".
    assert_eq!(ex.repo_target_exists("silent", PkgFamily::Arch), None);
}

// ============================================================================
// Polkit and privileged helpers
// ============================================================================

#[test]
fn polkit_missing_named_once_with_package_hint() {
    // The golden values are the original's Russian texts: the language is pinned (without the lock the check
    // went red or green depending on neighboring threads).
    let _lang = i18n_pin::ru();
    let io = arch();
    let ex = SystemExec::new(&io);
    assert!(!ex.polkit_available());
    assert!(!ex.polkit_available());
    let logs = ex.logs();
    assert_eq!(logs.len(), 1, "объяснение пишется РОВНО один раз: {logs:?}");
    assert!(logs[0].contains("Не найден pkexec"), "{}", logs[0]);
    assert!(logs[0].contains("polkit"), "имя пакета для Arch: {}", logs[0]);
}

#[test]
fn polkit_present_says_nothing() {
    let _lang = i18n_pin::en();
    let io = arch().tool("pkexec");
    let ex = SystemExec::new(&io);
    assert!(ex.polkit_available());
    assert!(ex.logs().is_empty());
}

#[test]
fn helper_available_requires_exec_bit() {
    let _lang = i18n_pin::en();
    // ⭐ Fixed during the port: the original asked only os.path.exists, and
    // a helper without the executable bit passed the check — pkexec failed on exec, and
    // the soft fallback to the raw path did not kick in.
    let ok = arch().with_helpers();
    assert!(SystemExec::new(&ok).pkg_helper_available());

    let mut broken = arch().with_helpers();
    broken.execs.remove(plan::pkg_helper_path());
    let ex = SystemExec::new(&broken);
    assert!(!ex.pkg_helper_available());
    // …and the caller gets None = "take the raw path", not an operation refusal.
    assert_eq!(
        ex.privileged_pkg_cmd(PkgFamily::Arch, "install", &["sddm".to_string()]),
        None
    );
}

#[test]
fn helper_unavailable_without_polkit() {
    let _lang = i18n_pin::en();
    let mut io = arch().with_helpers();
    io.tools.remove("pkexec");
    let ex = SystemExec::new(&io);
    assert!(!ex.de_helper_available());
    assert!(!ex.pkg_helper_available());
    assert!(!ex.stage_helper_available());
}

#[test]
fn privileged_pkg_cmd_from_plan() {
    let _lang = i18n_pin::en();
    let io = arch().with_helpers();
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.privileged_pkg_cmd(PkgFamily::Arch, "install", &["sddm".to_string()]),
        Some(vec![
            "pkexec".to_string(),
            plan::pkg_helper_path().to_string(),
            "arch".to_string(),
            "install".to_string(),
            "sddm".to_string(),
        ])
    );
    // A suspicious package name → the raw path (double protection with the bash helper).
    assert_eq!(
        ex.privileged_pkg_cmd(PkgFamily::Arch, "install", &["--root=/".to_string()]),
        None
    );
}

#[test]
fn privileged_stage_cmd_needs_staged_dir_under_tmp() {
    let _lang = i18n_pin::en();
    let io = arch()
        .with_helpers()
        .dir("/tmp/rexon-stage-1")
        .dir("/home/u/theme");
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.privileged_stage_cmd("gdm-bg", "Breeze", "/tmp/rexon-stage-1"),
        Some(vec![
            "pkexec".to_string(),
            plan::stage_helper_path().to_string(),
            "gdm-bg".to_string(),
            "Breeze".to_string(),
            "/tmp/rexon-stage-1".to_string(),
        ])
    );
    // Not under /tmp — the helper is not called (protection against symlink escape).
    assert_eq!(
        ex.privileged_stage_cmd("gdm-bg", "Breeze", "/home/u/theme"),
        None
    );
    // No directory at all.
    assert_eq!(
        ex.privileged_stage_cmd("gdm-bg", "Breeze", "/tmp/nope"),
        None
    );
}

// ============================================================================
// Login screen
// ============================================================================

#[test]
fn dm_symlink_target_alive() {
    let _lang = i18n_pin::en();
    let io = arch()
        .file("/usr/lib/systemd/system/sddm.service", "[Unit]\n")
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service")
        .tool("systemctl");
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.dm_symlink_target(),
        (Some("/usr/lib/systemd/system/sddm.service".to_string()), true)
    );
    assert_eq!(ex.active_login_manager(), "sddm");
    // The symlink is alive → `systemctl is-enabled` is not asked at all.
    assert!(io.commands().is_empty());
}

#[test]
fn dm_symlink_target_dangling_is_named() {
    let _lang = i18n_pin::en();
    // ⭐ Fixed during the port: the dead target is now named out loud —
    // canonicalize on a dangling symlink simply failed, and there was nothing to tell the person
    // about whose unit was removed.
    let io = arch()
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service")
        .tool("systemctl");
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.dm_symlink_target(),
        (Some("/usr/lib/systemd/system/sddm.service".to_string()), false)
    );
}

#[test]
fn dm_symlink_target_absent() {
    let _lang = i18n_pin::en();
    let io = arch().tool("systemctl");
    let ex = SystemExec::new(&io);
    assert_eq!(ex.dm_symlink_target(), (None, false));
}

#[test]
fn active_login_manager_probes_units_then_defaults() {
    let _lang = i18n_pin::en();
    // No symlink (early releases of Plasma Login Manager `Provides: None`).
    let io = arch()
        .tool("systemctl")
        .reply(
            "systemctl is-enabled plasmalogin.service",
            Reply::failed(1, "", "Failed to get unit file state"),
        )
        .reply("systemctl is-enabled sddm.service", Reply::ok("enabled\n"));
    let ex = SystemExec::new(&io);
    assert_eq!(ex.active_login_manager(), "sddm");
    // The query is LAZY: it never gets to gdm/lightdm.
    assert_eq!(
        io.commands(),
        vec![
            "systemctl is-enabled plasmalogin.service",
            "systemctl is-enabled sddm.service",
        ]
    );

    // Nobody answered — the plan's default.
    let bare = arch().tool("systemctl");
    assert_eq!(
        SystemExec::new(&bare).active_login_manager(),
        plan::DEFAULT_LOGIN_MANAGER
    );
}

#[test]
fn active_login_manager_reads_disabled_from_nonzero_exit() {
    let _lang = i18n_pin::en();
    // `systemctl is-enabled` prints `disabled` with a NONzero code: the answer
    // must not be discarded by the exit code, otherwise the state is lost.
    let io = arch()
        .tool("systemctl")
        .reply(
            "systemctl is-enabled plasmalogin.service",
            Reply::failed(1, "disabled\n", ""),
        )
        .reply(
            "systemctl is-enabled sddm.service",
            Reply::failed(1, "disabled\n", ""),
        )
        .reply("systemctl is-enabled gdm.service", Reply::ok("enabled\n"));
    let ex = SystemExec::new(&io);
    assert_eq!(ex.dm_is_enabled("plasmalogin").as_deref(), Some("disabled"));
    assert_eq!(ex.active_login_manager(), "gdm");
}

#[test]
fn guard_dangling_symlink_is_not_login_screen() {
    let _lang = i18n_pin::en();
    // ⚠️ WATCHDOG. A recorded trap: a dangling symlink can pass itself off as a
    // login screen, and the foolproofing SILENTLY let the installation through. No
    // unit is enabled → the honest answer is "there is no login screen".
    let dangling = arch()
        .tool("systemctl")
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service");
    assert!(
        !SystemExec::new(&dangling).display_manager_present(),
        "висячий симлинк выдал себя за живой экран входа"
    );

    // The bite is tested like this: the symlink target is put back on disk — and the answer
    // must become positive, WITHOUT a single unit query.
    let alive = arch()
        .tool("systemctl")
        .file("/usr/lib/systemd/system/sddm.service", "[Unit]\n")
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service");
    let ex = SystemExec::new(&alive);
    assert!(ex.display_manager_present());
    assert!(alive.commands().is_empty());
}

#[test]
fn display_manager_present_true_without_systemd() {
    let _lang = i18n_pin::en();
    // A non-systemd system: the login screen is managed by init itself, we do not interfere.
    let io = arch();
    assert!(SystemExec::new(&io).display_manager_present());
    assert!(io.commands().is_empty());
}

#[test]
fn display_manager_present_from_enabled_unit() {
    let _lang = i18n_pin::en();
    let io = arch()
        .tool("systemctl")
        .reply("systemctl is-enabled greetd.service", Reply::ok("enabled\n"));
    assert!(SystemExec::new(&io).display_manager_present());
}

// ============================================================================
// Plymouth
// ============================================================================

fn plymouth_theme(io: FakeIo, name: &str) -> FakeIo {
    io.file(
        &plan::plymouth_theme_file_in(plan::PLYMOUTH_THEMES_DIR, name),
        "[Plymouth Theme]\n",
    )
}

#[test]
fn detect_plymouth_theme_from_binary() {
    let _lang = i18n_pin::en();
    let io = arch()
        .tool("plymouth-set-default-theme")
        .reply("plymouth-set-default-theme", Reply::ok("cachyos-bootanimation\n"));
    assert_eq!(
        SystemExec::new(&io).detect_plymouth_theme().as_deref(),
        Some("cachyos-bootanimation")
    );
}

#[test]
fn detect_plymouth_theme_falls_back_to_default_link() {
    let _lang = i18n_pin::en();
    // Ubuntu: the binary is cut out of the package, the theme is visible only through alternatives.
    let io = plymouth_theme(arch(), "bgrt").link(
        &format!("{}/default.plymouth", plan::PLYMOUTH_THEMES_DIR),
        &plan::plymouth_theme_file_in(plan::PLYMOUTH_THEMES_DIR, "bgrt"),
    );
    assert_eq!(
        SystemExec::new(&io).detect_plymouth_theme().as_deref(),
        Some("bgrt")
    );
    // No binary — nothing to run.
    assert!(io.commands().is_empty());
}

#[test]
fn detect_plymouth_theme_none_when_nothing_knows() {
    let _lang = i18n_pin::en();
    let io = arch();
    assert_eq!(SystemExec::new(&io).detect_plymouth_theme(), None);
}

#[test]
fn list_plymouth_themes_needs_theme_file() {
    let _lang = i18n_pin::en();
    // An empty directory is left after removing the theme package — it did not become a theme.
    let io = plymouth_theme(plymouth_theme(arch(), "bgrt"), "spinner")
        .dir(&format!("{}/leftover", plan::PLYMOUTH_THEMES_DIR));
    assert_eq!(
        SystemExec::new(&io).list_plymouth_themes(),
        vec!["bgrt", "spinner"],
        "порядок задаёт ядро, а не обход каталога"
    );
}

#[test]
fn list_plymouth_themes_empty_without_dir() {
    let _lang = i18n_pin::en();
    let io = arch();
    assert!(SystemExec::new(&io).list_plymouth_themes().is_empty());
}

#[test]
fn target_plymouth_theme_prefers_distro_branding() {
    let _lang = i18n_pin::en();
    let io = plymouth_theme(plymouth_theme(arch(), "bgrt"), "cachyos-bootanimation")
        .file(plan::PLYMOUTH_DEFAULTS, "Theme=bgrt\n")
        .reply(
            "pacman -Q cachyos-plymouth-bootanimation",
            Reply::ok("cachyos-plymouth-bootanimation 1.0\n"),
        );
    let ex = SystemExec::new(&io);
    assert_eq!(
        ex.target_plymouth_theme().as_deref(),
        Some("cachyos-bootanimation")
    );
    // The branding matched → plymouthd.defaults is not read (steps are lazy).
    assert_eq!(io.reads_of(plan::PLYMOUTH_DEFAULTS), 0);
}

#[test]
fn target_plymouth_theme_falls_back_to_package_default() {
    let _lang = i18n_pin::en();
    // The branding theme exists but its package does not — this is not the distro stock.
    let io = plymouth_theme(plymouth_theme(arch(), "bgrt"), "cachyos-bootanimation")
        .file(plan::PLYMOUTH_DEFAULTS, "Theme=bgrt\n")
        .reply(
            "pacman -Q cachyos-plymouth-bootanimation",
            Reply::failed(1, "", "not found"),
        );
    assert_eq!(
        SystemExec::new(&io).target_plymouth_theme().as_deref(),
        Some("bgrt")
    );
}

#[test]
fn plymouth_reset_command_matches_available_tool() {
    let _lang = i18n_pin::en();
    let with_tool = plymouth_theme(arch(), "bgrt").tool("plymouth-set-default-theme");
    assert_eq!(
        SystemExec::new(&with_tool).plymouth_reset_command(Some("bgrt")),
        Some("sudo plymouth-set-default-theme bgrt -R".to_string())
    );
    let alt = plymouth_theme(arch(), "bgrt");
    assert_eq!(
        SystemExec::new(&alt).plymouth_reset_command(Some("bgrt")),
        Some(
            "sudo update-alternatives --set default.plymouth \
             /usr/share/plymouth/themes/bgrt/bgrt.plymouth && sudo update-initramfs -u"
                .to_string()
        )
    );
    assert_eq!(SystemExec::new(&alt).plymouth_reset_command(Some("")), None);
}

#[test]
fn reset_plymouth_runs_one_pkexec_from_plan() {
    let _lang = i18n_pin::en();
    let io = plymouth_theme(arch(), "bgrt")
        .tool("pkexec")
        .tool("plymouth-set-default-theme")
        .reply("plymouth-set-default-theme", Reply::ok("aeroshell-boot\n"))
        .reply(
            "pkexec sh -c set -e; plymouth-set-default-theme bgrt -R",
            Reply::ok(""),
        );
    let ex = SystemExec::new(&io);
    let (ok, msg) = ex.reset_plymouth_to_default(Some("bgrt"));
    assert!(ok, "{msg}");
    assert!(msg.contains("bgrt"), "{msg}");
    assert_eq!(
        io.commands(),
        vec![
            "plymouth-set-default-theme",
            "pkexec sh -c set -e; plymouth-set-default-theme bgrt -R",
        ]
    );
}

#[test]
fn reset_plymouth_reports_pkexec_denial() {
    let _lang = i18n_pin::en();
    let io = plymouth_theme(arch(), "bgrt")
        .tool("pkexec")
        .tool("plymouth-set-default-theme")
        .reply("plymouth-set-default-theme", Reply::ok("aeroshell-boot\n"))
        .reply(
            "pkexec sh -c set -e; plymouth-set-default-theme bgrt -R",
            Reply::failed(126, "", "Error executing command as another user: Not authorized"),
        );
    let (ok, msg) = SystemExec::new(&io).reset_plymouth_to_default(Some("bgrt"));
    assert!(!ok);
    assert!(msg.contains("plymouth-set-default-theme bgrt -R"), "{msg}");
}

#[test]
fn reset_plymouth_without_target_says_so() {
    let _lang = i18n_pin::en();
    let io = arch().tool("pkexec");
    let (ok, msg) = SystemExec::new(&io).reset_plymouth_to_default(None);
    assert!(!ok);
    assert_eq!(msg, plan::plymouth_no_target_message());
    assert!(io.commands().is_empty());
}

// ============================================================================
// SDDM
// ============================================================================

const FAKE_CONF_D: &str = "/fake/etc/sddm.conf.d";

fn sddm_machine() -> FakeIo {
    arch()
        .tool("pkexec")
        .file(
            &format!("{FAKE_CONF_D}/kde_settings.conf"),
            "[Theme]\nCurrent=breeze\n",
        )
        .file(
            &format!("{FAKE_CONF_D}/zz-rexon-theme.conf"),
            "[Theme]\nCurrent=Se7enAero\n",
        )
        .file(
            &format!("{FAKE_CONF_D}/10-wackyideas.conf"),
            "[Theme]\nCurrent=WackyIdeas\n",
        )
        .file(
            &format!("{FAKE_CONF_D}/99-user.conf"),
            "[General]\nNumlock=on\n",
        )
        .file(
            &format!("{}/00-distro.conf", plan::SDDM_DISTRO_CONF_D),
            "[Theme]\nCurrent=breeze\n",
        )
}

#[test]
fn sddm_conf_entries_sorted_and_only_conf() {
    let _lang = i18n_pin::en();
    let io = sddm_machine().file(&format!("{FAKE_CONF_D}/README"), "не конфиг\n");
    let names: Vec<String> = SystemExec::new(&io)
        .sddm_conf_entries(FAKE_CONF_D)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        names,
        vec![
            "10-wackyideas.conf",
            "99-user.conf",
            "kde_settings.conf",
            "zz-rexon-theme.conf",
        ],
        "SDDM мёрджит лексикографически — порядок обязателен"
    );
}

#[test]
fn sddm_read_current_theme_last_wins() {
    let _lang = i18n_pin::en();
    let io = sddm_machine();
    assert_eq!(
        SystemExec::new(&io).sddm_read_current_theme(FAKE_CONF_D).as_deref(),
        Some("Se7enAero"),
        "поздний файл перекрывает ранние"
    );
}

#[test]
fn sddm_reset_plan_keeps_kde_and_user_files() {
    let _lang = i18n_pin::en();
    let io = sddm_machine();
    let p = SystemExec::new(&io).sddm_reset_plan(FAKE_CONF_D);
    assert_eq!(p.to_remove, vec!["10-wackyideas.conf", "zz-rexon-theme.conf"]);
    assert_eq!(p.default_theme.as_deref(), Some("breeze"));
}

#[test]
fn apply_sddm_reset_executes_exactly_the_shown_plan() {
    let _lang = i18n_pin::en();
    // ⭐ Fixed during the port: the route showed the plan but executed a SECOND
    // scan of the directory. Here exactly what was computed and shown is executed:
    // we supply a directory whose contents are DIFFERENT by the time of execution.
    let io = sddm_machine().reply(
        &format!(
            "pkexec sh -c set -e; rm -f {FAKE_CONF_D}/10-wackyideas.conf \
             {FAKE_CONF_D}/zz-rexon-theme.conf"
        ),
        Reply::ok(""),
    );
    let ex = SystemExec::new(&io);
    let p = ex.sddm_reset_plan(FAKE_CONF_D);
    let (ok, msg, removed) = ex.apply_sddm_reset(&p, FAKE_CONF_D);
    assert!(ok, "{msg}");
    assert_eq!(removed, p.to_remove);
    assert!(msg.contains("breeze"), "{msg}");
    assert_eq!(io.commands().len(), 1, "ровно один pkexec: {:?}", io.commands());
}

#[test]
fn reset_sddm_reports_failure_with_manual_command() {
    let _lang = i18n_pin::en();
    let io = sddm_machine().reply(
        &format!(
            "pkexec sh -c set -e; rm -f {FAKE_CONF_D}/10-wackyideas.conf \
             {FAKE_CONF_D}/zz-rexon-theme.conf"
        ),
        Reply::failed(1, "", "rm: отказано в доступе"),
    );
    let (ok, msg, removed) = SystemExec::new(&io).reset_sddm_to_default(FAKE_CONF_D);
    assert!(!ok);
    assert!(removed.is_empty(), "провал не смеет рапортовать удалённые");
    assert_eq!(msg, "rm: отказано в доступе", "текст беды — как есть");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn reset_sddm_denial_shows_manual_command() {
    // The golden values are the original's Russian texts: the check goes through po/ru.po.
    let _lang = i18n_pin::ru();
    let io = sddm_machine().reply(
        &format!(
            "pkexec sh -c set -e; rm -f {FAKE_CONF_D}/10-wackyideas.conf \
             {FAKE_CONF_D}/zz-rexon-theme.conf"
        ),
        Reply::failed(126, "", "Not authorized"),
    );
    let (ok, msg, removed) = SystemExec::new(&io).reset_sddm_to_default(FAKE_CONF_D);
    assert!(!ok);
    assert!(removed.is_empty());
    assert!(msg.contains("Пароль администратора не подтверждён"), "{msg}");
    assert!(msg.contains("sudo rm -f"), "{msg}");
}

#[test]
fn sddm_distro_default_theme_from_distro_dir() {
    let _lang = i18n_pin::en();
    let io = arch().file(
        &format!("{}/00-distro.conf", plan::SDDM_DISTRO_CONF_D),
        "[Theme]\nCurrent=maldives\n",
    );
    assert_eq!(
        SystemExec::new(&io).sddm_distro_default_theme().as_deref(),
        Some("maldives")
    );
}

// ============================================================================
// Plasma forks
// ============================================================================

/// A machine with AeroShell installed: a libplasma provider + a standalone fork.
fn aeroshell_machine() -> FakeIo {
    arch()
        .tool("pkexec")
        .reply(
            "pacman -Qq",
            Reply::ok("bash\naeroshell-libplasma-git\naeroshell-workspace-git\nsddm\n"),
        )
        .reply(
            "pacman -Qi aeroshell-libplasma-git",
            Reply::ok("Name : aeroshell-libplasma-git\nProvides : libplasma=6.5\nReplaces : plasma-framework\n"),
        )
        .reply(
            "pacman -Qi aeroshell-workspace-git",
            Reply::ok("Name : aeroshell-workspace-git\nProvides : None\nReplaces : None\n"),
        )
        .reply("pacman -Si libplasma", Reply::ok("Repository : extra\n"))
}

#[test]
fn detect_plasma_forks_filters_installed_list() {
    let _lang = i18n_pin::en();
    let io = aeroshell_machine();
    assert_eq!(
        SystemExec::new(&io).detect_plasma_forks(),
        vec!["aeroshell-libplasma-git", "aeroshell-workspace-git"]
    );
}

#[test]
fn plasma_fork_preview_shows_exact_command() {
    let _lang = i18n_pin::en();
    let io = aeroshell_machine();
    let (ok, msg) = SystemExec::new(&io).plasma_fork_reset_preview(None);
    assert!(ok, "{msg}");
    assert_eq!(
        msg,
        "sudo pacman -Rns aeroshell-workspace-git  &&  sudo pacman -S libplasma"
    );
    // The preview must not go to pkexec at all.
    assert!(
        !io.commands().iter().any(|c| c.starts_with("pkexec")),
        "{:?}",
        io.commands()
    );
}

#[test]
fn plasma_fork_preview_nothing_when_no_forks() {
    let _lang = i18n_pin::en();
    let io = arch().reply("pacman -Qq", Reply::ok("bash\nsddm\n"));
    let (ok, msg) = SystemExec::new(&io).plasma_fork_reset_preview(None);
    assert!(!ok);
    assert_eq!(msg, plan::plasma_fork_nothing_message());
}

#[test]
fn guard_blocked_fork_reset_runs_nothing() {
    let _lang = i18n_pin::en();
    // ⚠️ WATCHDOG. The fork's stock was not found in the repositories (KDE renamed
    // plasma-framework → libplasma). `-Rns` is irreversible, there is nothing to restore with —
    // so NOT A SINGLE command goes outside.
    let io = arch()
        .tool("pkexec")
        .reply("pacman -Qq", Reply::ok("aeroshell-libplasma-git\n"))
        .reply(
            "pacman -Qi aeroshell-libplasma-git",
            Reply::ok("Provides : None\nReplaces : plasma-framework\n"),
        )
        .reply(
            "pacman -Si plasma-framework",
            Reply::failed(1, "", "error: target not found: plasma-framework"),
        )
        // The core's fallback map also offers `libplasma` (the fork is called
        // `aeroshell-libplasma-git`) — the repositories do not know it either.
        .reply(
            "pacman -Si libplasma",
            Reply::failed(1, "", "error: target not found: libplasma"),
        );
    let ex = SystemExec::new(&io);
    let (ok, msg) = ex.remove_plasma_forks(None);
    assert!(!ok);
    assert!(msg.contains("plasma-framework"), "{msg}");
    assert!(
        !io.commands().iter().any(|c| c.starts_with("pkexec")),
        "запущено то, что запускать нельзя: {:?}",
        io.commands()
    );
}

#[test]
fn remove_plasma_forks_one_pkexec_in_fixed_order() {
    let _lang = i18n_pin::en();
    let shell = "pacman -Rns --noconfirm aeroshell-workspace-git; \
                 pacman -S --noconfirm --needed libplasma";
    let io = aeroshell_machine().reply(
        &format!("pkexec sh -c set -e; {shell}"),
        Reply::ok("удаление… установка… готово\n"),
    );
    let ex = SystemExec::new(&io);
    let (ok, msg) = ex.remove_plasma_forks(None);
    assert!(ok, "{msg}");
    assert!(msg.contains("готово"), "{msg}");
    let pkexecs: Vec<String> = io
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("pkexec"))
        .collect();
    assert_eq!(pkexecs.len(), 1, "один пароль на всю транзакцию: {pkexecs:?}");
}

/// The window removes exactly the set it showed the person: the package
/// manager is not queried again, the password is asked once.
#[test]
fn remove_plasma_forks_takes_the_shown_set_without_asking_again() {
    let _lang = i18n_pin::en();
    let shell = "pacman -Rns --noconfirm aeroshell-workspace-git; \
                 pacman -S --noconfirm --needed libplasma";
    let io = aeroshell_machine().reply(
        &format!("pkexec sh -c set -e; {shell}"),
        Reply::ok("готово\n"),
    );
    let ex = SystemExec::new(&io);
    let shown = vec![
        "aeroshell-libplasma-git".to_string(),
        "aeroshell-workspace-git".to_string(),
    ];
    let (ok, msg) = ex.remove_plasma_forks(Some(&shown));
    assert!(ok, "{msg}");
    let cmds = io.commands();
    assert!(
        !cmds.iter().any(|c| c == "pacman -Qq"),
        "показанный набор опрошен заново: {cmds:?}"
    );
    assert_eq!(cmds.iter().filter(|c| c.starts_with("pkexec")).count(), 1, "{cmds:?}");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs a live machine: builds po/ru.po with the real msgfmt")]
fn remove_plasma_forks_memoizes_repo_probe() {
    // The golden values are the original's Russian texts: the check goes through po/ru.po.
    let _lang = i18n_pin::ru();
    // The plan and the human-readable command walk the forks twice — `pacman -Si`
    // for one name must be asked once.
    let shell = "pacman -Rns --noconfirm aeroshell-workspace-git; \
                 pacman -S --noconfirm --needed libplasma";
    let io = aeroshell_machine().reply(
        &format!("pkexec sh -c set -e; {shell}"),
        Reply::failed(126, "", "Not authorized"),
    );
    let ex = SystemExec::new(&io);
    let (ok, msg) = ex.remove_plasma_forks(None);
    assert!(!ok);
    assert!(msg.contains("Пароль администратора не подтверждён"), "{msg}");
    assert!(msg.contains("sudo pacman -Rns aeroshell-workspace-git"), "{msg}");
    let si = io
        .commands()
        .into_iter()
        .filter(|c| c == "pacman -Si libplasma")
        .count();
    assert_eq!(si, 1, "проба репозитория спрошена {si} раз(а) вместо одного");
}

// ============================================================================
// Ending the session
// ============================================================================

#[test]
fn logout_session_first_success_wins() {
    let _lang = i18n_pin::en();
    let io = arch()
        .env_var("XDG_CURRENT_DESKTOP", "KDE")
        .env_var("XDG_SESSION_ID", "3")
        .reply(
            "dbus-send --session --type=method_call --dest=org.kde.LogoutPrompt \
             /LogoutPrompt org.kde.LogoutPrompt.promptLogout",
            Reply::ok(""),
        );
    let ex = SystemExec::new(&io);
    assert_eq!(ex.logout_session(), (true, "ok".to_string()));
    assert_eq!(io.commands().len(), 1, "лишние попытки после успеха");
}

#[test]
fn logout_session_names_timeout() {
    let _lang = i18n_pin::en();
    // ⭐ Fixed during the port: "did not wait long enough" is no longer mixed with "could not".
    let io = arch()
        .env_var("XDG_CURRENT_DESKTOP", "hyprland")
        .reply("hyprctl dispatch exit", Reply::timeout());
    let (ok, msg) = SystemExec::new(&io).logout_session();
    assert!(!ok);
    assert_eq!(msg, "hyprctl: timeout");
}

#[test]
fn logout_session_no_commands_at_all() {
    let _lang = i18n_pin::ru();
    let io = arch();
    assert_eq!(
        SystemExec::new(&io).logout_session(),
        (false, "Не удалось выйти из сессии".to_string())
    );
}

/// The logout refusal in English: both "logging out failed" and "the command
/// did not start" — without a single Russian letter.
#[test]
fn english_logout_session_has_no_russian() {
    let _lang = i18n_pin::en();
    let io = arch();
    assert_eq!(
        SystemExec::new(&io).logout_session(),
        (false, "Couldn't log out".to_string())
    );
    let io = arch()
        .env_var("XDG_CURRENT_DESKTOP", "hyprland")
        .reply("hyprctl dispatch exit", Reply::error());
    let (ok, msg) = SystemExec::new(&io).logout_session();
    assert!(!ok);
    assert_eq!(msg, "hyprctl: didn't start");
}

/// The same in Russian — verbatim as it was before translation.
#[test]
fn russian_logout_session_reads_as_before() {
    let _lang = i18n_pin::ru();
    let io = arch()
        .env_var("XDG_CURRENT_DESKTOP", "hyprland")
        .reply("hyprctl dispatch exit", Reply::error());
    let (ok, msg) = SystemExec::new(&io).logout_session();
    assert!(!ok);
    assert_eq!(msg, "hyprctl: не запустилась");
}

// ============================================================================
// Boundary watchdogs
// ============================================================================

/// Commands the executor may run without `pkexec`: all of them only
/// READ. The list is closed — a new command must get here deliberately.
const READ_ONLY_HEADS: &[&str] = &[
    "pacman -Qq",
    "pacman -Q ",
    "pacman -Qi ",
    "pacman -Si ",
    "systemctl is-enabled ",
    "plymouth-set-default-theme",
];

fn is_read_only(cmd: &str) -> bool {
    cmd == "plymouth-set-default-theme"
        || READ_ONLY_HEADS
            .iter()
            .any(|h| h.ends_with(' ') && cmd.starts_with(h) || *h == cmd)
}

#[test]
fn guard_only_pkexec_changes_system() {
    let _lang = i18n_pin::en();
    // ⚠️ WATCHDOG. The full emergency rollback: Plasma forks → Plymouth → SDDM.
    // The executor may change the system ONLY through `pkexec` with argv
    // from the core's plan; everything else is a read probe from the closed list.
    let fork_shell = "pacman -Rns --noconfirm aeroshell-workspace-git; \
                      pacman -S --noconfirm --needed libplasma";
    let io = aeroshell_machine()
        .tool("plymouth-set-default-theme")
        .file(plan::PLYMOUTH_DEFAULTS, "Theme=bgrt\n")
        .file(
            &plan::plymouth_theme_file_in(plan::PLYMOUTH_THEMES_DIR, "bgrt"),
            "[Plymouth Theme]\n",
        )
        .file(
            &format!("{FAKE_CONF_D}/zz-rexon-theme.conf"),
            "[Theme]\nCurrent=Se7enAero\n",
        )
        .reply("plymouth-set-default-theme", Reply::ok("aeroshell-boot\n"))
        .reply("pacman -Q cachyos-plymouth-bootanimation", Reply::failed(1, "", ""))
        .reply(&format!("pkexec sh -c set -e; {fork_shell}"), Reply::ok("ok\n"))
        .reply(
            "pkexec sh -c set -e; plymouth-set-default-theme bgrt -R",
            Reply::ok(""),
        )
        .reply(
            &format!("pkexec sh -c set -e; rm -f {FAKE_CONF_D}/zz-rexon-theme.conf"),
            Reply::ok(""),
        );

    let ex = SystemExec::new(&io);
    assert!(ex.remove_plasma_forks(None).0);
    assert!(ex.reset_plymouth_to_default(None).0);
    assert!(ex.reset_sddm_to_default(FAKE_CONF_D).0);

    let mut pkexecs = 0;
    for cmd in io.commands() {
        if cmd.starts_with("pkexec ") {
            pkexecs += 1;
            continue;
        }
        assert!(
            is_read_only(&cmd),
            "исполнитель запустил изменяющую команду вне pkexec: {cmd}"
        );
    }
    assert_eq!(pkexecs, 3, "по одному паролю на каждую из трёх операций");
}

#[test]
fn guard_idempotent_resets_run_nothing() {
    let _lang = i18n_pin::en();
    // ⚠️ WATCHDOG. The "restore to out of the box" button is pressed again: the system
    // is already stock, and there must be no trips outside at all.
    let io = arch()
        .tool("pkexec")
        .tool("plymouth-set-default-theme")
        .file(
            &plan::plymouth_theme_file_in(plan::PLYMOUTH_THEMES_DIR, "bgrt"),
            "[Plymouth Theme]\n",
        )
        .file(plan::PLYMOUTH_DEFAULTS, "Theme=bgrt\n")
        .file(
            &format!("{FAKE_CONF_D}/kde_settings.conf"),
            "[Theme]\nCurrent=breeze\n",
        )
        .reply("plymouth-set-default-theme", Reply::ok("bgrt\n"))
        .reply("pacman -Q cachyos-plymouth-bootanimation", Reply::failed(1, "", ""));

    let ex = SystemExec::new(&io);
    let (ok, msg) = ex.reset_plymouth_to_default(None);
    assert!(ok, "{msg}");
    assert_eq!(msg, plan::plymouth_already_default_message("bgrt"));

    let (ok, msg, removed) = ex.reset_sddm_to_default(FAKE_CONF_D);
    assert!(ok, "{msg}");
    assert_eq!(msg, plan::sddm_already_stock_message());
    assert!(removed.is_empty());

    assert!(
        !io.commands().iter().any(|c| c.starts_with("pkexec")),
        "идемпотентный сброс сходил в pkexec: {:?}",
        io.commands()
    );
}

// ============================================================================
// Emergency appearance rollback (`--recover-kde`)
// ============================================================================
//
// ⛔ THERE IS NO LIVE MACHINE HERE. The route can apply a theme, remove login
// screen overrides, rebuild the initramfs and rewrite the panel layout — all
// of this runs on [`FakeIo`]: stand-in files, a stand-in `PATH`,
// a stand-in `$XDG_CONFIG_HOME`. Not a single subprocess, not a single write to
// the real disk.

/// A machine with Plasma and its own config root: the common base of the route checks.
fn plasma() -> FakeIo {
    arch()
        .env_var("HOME", "/home/tester")
        .env_var("XDG_CONFIG_HOME", "/cfg")
        .env_var("XDG_CURRENT_DESKTOP", "KDE")
        .tool("kwriteconfig6")
        .file(BACKUP, "org.kde.oxygen.desktop\n")
        .reply("pacman -Qq", Reply::ok("bash\nplasma-desktop\n"))
        .reply(&format!("plasma-apply-lookandfeel -a {SAVED_THEME}"), Reply::ok(""))
        .reply("plasma-apply-colorscheme BreezeDark", Reply::ok(""))
}

/// The theme backup in the stand-in config root.
const BACKUP: &str = "/cfg/rexon/last_kde_lookandfeel.txt";
/// The desktops and panels layout in the same place.
const APPLETSRC: &str = "/cfg/plasma-org.kde.plasma.desktop-appletsrc";
/// The theme from the backup — exactly the one the route must apply.
const SAVED_THEME: &str = "org.kde.oxygen.desktop";

/// A layout with ONE desktop and a panel holding the
/// «Folder View» applet. The panel must not pass for a desktop (fix of the original).
const LAYOUT_CLEAN: &str = "\
[Containments][2]\n\
plugin=org.kde.plasma.folder\n\
\n\
[Containments][3]\n\
plugin=org.kde.panel\n\
\n\
[Containments][3][Applets][7]\n\
plugin=org.kde.plasma.folder\n";

/// The same layout, but with a fork theme applet in the panel.
const LAYOUT_FORKED: &str = "\
[Containments][2]\n\
plugin=org.kde.plasma.folder\n\
\n\
[Containments][3]\n\
plugin=org.kde.panel\n\
\n\
[Containments][3][Applets][5]\n\
plugin=org.kde.plasma.sevenstart\n\
\n\
[Containments][3][Applets][5][Configuration]\n\
size=42\n";

/// Run the route and collect what was said to the person.
fn recover(io: &FakeIo) -> (i32, Vec<String>) {
    let mut said: Vec<String> = Vec::new();
    let rc = {
        let ex = SystemExec::new(io);
        let mut out = |s: &str| said.push(s.to_string());
        ex.recover_kde(&mut out)
    };
    (rc, said)
}

/// ⛔ On another desktop the route must REFUSE in words and touch
/// nothing: only Plasma can be restored, and the shortcut has a neutral name.
#[test]
fn recover_kde_refuses_on_foreign_desktop() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma().env_var("XDG_CURRENT_DESKTOP", "GNOME");
    let (rc, said) = recover(&io);
    assert_eq!(rc, 1, "чужой рабочий стол — это неудача, а не тихий ноль");
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(said[0].contains("только KDE Plasma"), "{}", said[0]);
    assert!(said[1].contains("GNOME"), "имя чужого стола названо: {}", said[1]);
    assert!(
        io.commands().is_empty() && io.written().is_empty(),
        "на чужом рабочем столе не запускается и не пишется НИЧЕГО: {:?}",
        io.commands()
    );
}

/// An empty `$XDG_CURRENT_DESKTOP` means "unknown", not "another desktop":
/// the route is called from a tty too, when the session no longer comes up.
#[test]
fn recover_kde_runs_without_desktop_variable() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma().env_var("XDG_CURRENT_DESKTOP", "");
    let (rc, said) = recover(&io);
    assert_eq!(rc, 0);
    assert!(said.iter().any(|l| l.contains("Восстановлено")), "{said:?}");
}

/// A full pass over a healthy machine: the same sequence of steps and the
/// same words the original printed.
#[test]
fn recover_kde_full_route_order() {
    let _lang = i18n_pin::en();
    let io = plasma()
        .file(APPLETSRC, LAYOUT_CLEAN)
        .file("/cfg/Kvantum/kvantum.kvconfig", "[General]\ntheme=SevenAero\n")
        .dir("/usr/share/wallpapers/Next")
        .tool("dbus-send")
        .reply(
            "/usr/bin/dbus-send --session --print-reply --dest=org.kde.KWin \
             --type=method_call /KWin org.kde.KWin.reconfigure",
            Reply::ok(""),
        );
    let (rc, said) = recover(&io);
    assert_eq!(rc, 0);

    assert_eq!(
        said,
        vec![
            rexon_core::app::recovering_line(SAVED_THEME),
            rexon_core::app::restored_line(SAVED_THEME),
            rexon_core::app::FONTS_RESET_LINE.to_string(),
            rexon_core::app::KWIN_EFFECTS_RESET_LINE.to_string(),
            rexon_core::app::kvantum_breeze_line("SevenAero"),
            rexon_core::app::sddm_already_stock_line(None),
            rexon_core::app::wallpaper_reset_line(1, "/usr/share/wallpapers/Next/"),
        ],
        "порядок и слова шагов"
    );

    let cmds = io.commands();
    // The theme is applied FIRST among the modifying commands, and exactly the one that was
    // in the backup.
    assert_eq!(
        cmds[1],
        format!("plasma-apply-lookandfeel -a {SAVED_THEME}"),
        "{cmds:?}"
    );
    assert_eq!(cmds[2], "plasma-apply-colorscheme BreezeDark");
    // The wallpaper is rewritten for ONE desktop: the «Folder View» applet in the panel
    // does not count as a desktop (fix of the original).
    let wallpaper: Vec<&String> = cmds.iter().filter(|c| c.contains("wallpaperplugin")).collect();
    assert_eq!(wallpaper.len(), 1, "{cmds:?}");
    assert!(wallpaper[0].contains("Containments --group 2"), "{}", wallpaper[0]);
    // It ends with a plasmashell restart — otherwise what was written is picked up
    // only at the next login.
    assert_eq!(
        cmds[cmds.len() - 2..],
        [
            "systemctl --user reset-failed plasma-plasmashell.service".to_string(),
            "systemctl --user restart plasma-plasmashell.service".to_string(),
        ]
    );
    assert!(io.written().is_empty(), "на исправной машине ничего не переписывается");
}

/// The theme did not apply — the route exits with failure RIGHT AWAY and goes no further:
/// there is no point resetting configs on top of a theme that did not work.
#[test]
fn recover_kde_stops_when_theme_fails() {
    let _lang = i18n_pin::en();
    let io = plasma().reply(
        &format!("plasma-apply-lookandfeel -a {SAVED_THEME}"),
        Reply::failed(1, "", "Theme «oxygen» not found"),
    );
    let (rc, said) = recover(&io);
    assert_eq!(rc, 1);
    assert!(said.last().unwrap().contains("not found"), "{said:?}");
    assert!(
        !io.commands().iter().any(|c| c.contains("colorscheme")),
        "цветовая схема после неудачи не применяется: {:?}",
        io.commands()
    );
}

/// ⭐ Fix of the original: a missing `plasma-apply-lookandfeel` in Python
/// raised FileNotFoundError straight out of `main()` — the emergency route crashed
/// with a traceback exactly where it must explain.
#[test]
fn recover_kde_names_missing_plasma_tool() {
    let _lang = i18n_pin::en();
    let mut io = plasma();
    io.replies.remove(&format!("plasma-apply-lookandfeel -a {SAVED_THEME}"));
    let (rc, said) = recover(&io);
    assert_eq!(rc, 1);
    assert_eq!(said.last().unwrap(), rexon_core::app::LOOKANDFEEL_MISSING_LINE);
}

/// Plasma forks are named BEFORE the reset, together with the full rollback command.
#[test]
fn recover_kde_warns_about_plasma_forks_first() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma()
        .file(APPLETSRC, LAYOUT_CLEAN)
        .reply("pacman -Qq", Reply::ok("bash\naeroshell-workspace-git\n"))
        .reply(
            "pacman -Qi aeroshell-workspace-git",
            Reply::ok("Name : aeroshell-workspace-git\nProvides : None\nReplaces : None\n"),
        );
    let (_rc, said) = recover(&io);
    assert!(
        said[0].contains("ПАКЕТЫ-ЗАМЕНЫ частей Plasma"),
        "предупреждение идёт ПЕРВЫМ, до применения темы: {said:?}"
    );
    assert!(said[1].contains("aeroshell-workspace-git"), "{}", said[1]);
    assert!(
        said.iter().any(|l| l.contains("Восстанавливаю KDE-тему")),
        "предупреждение не отменяет сам сброс: {said:?}"
    );
}

/// Panel surgery: only the fork theme applets are removed, a backup is left
/// next to it, the panel is not removed as a whole.
#[test]
fn recover_kde_strips_fork_applets_and_keeps_backup() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma().file(APPLETSRC, LAYOUT_FORKED).dir("/usr/share/wallpapers");
    let (rc, said) = recover(&io);
    assert_eq!(rc, 0);
    assert!(
        said.iter().any(|l| l == rexon_core::app::PANEL_AGGRESSIVE_LINE),
        "следы форка обязаны включить чистку раскладки: {said:?}"
    );
    assert!(
        said.iter().any(|l| l.contains("убраны сломанные виджеты от замен Plasma: 1")),
        "{said:?}"
    );

    let written = io.written();
    assert_eq!(
        written.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
        vec![APPLETSRC, "/cfg/plasma-org.kde.plasma.desktop-appletsrc.rexon-bak"],
        "маршрут пишет РОВНО два файла: новую раскладку и копию старой"
    );
    let (_, new_layout) = written.iter().find(|(p, _)| p == APPLETSRC).unwrap();
    assert!(!new_layout.contains("sevenstart"), "{new_layout}");
    assert!(new_layout.contains("org.kde.panel"), "панель на месте: {new_layout}");
    // A full `--resetLayout` was not called: the surgery succeeded, and the
    // user's widgets survived.
    assert!(
        !io.commands().iter().any(|c| c.contains("--resetLayout")),
        "{:?}",
        io.commands()
    );
}

/// The layout could not be rewritten — we say so and do NOT consider the applets
/// removed (otherwise the full reset would not work as the fallback).
#[test]
fn recover_kde_says_when_layout_cannot_be_rewritten() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma()
        .file(APPLETSRC, LAYOUT_FORKED)
        .readonly_path(APPLETSRC)
        .tool("lookandfeeltool");
    let (_rc, said) = recover(&io);
    assert!(
        said.iter().any(|l| l.contains("Не удалось переписать панель")),
        "{said:?}"
    );
    // Traces of the fork remained → the full layout reset kicks in.
    assert!(
        io.commands()
            .iter()
            .any(|c| c == &format!("/usr/bin/lookandfeeltool -a {SAVED_THEME} --resetLayout")),
        "{:?}",
        io.commands()
    );
}

/// There is no full reset tool — the route says so in words rather than
/// staying silent (rule #13).
#[test]
fn recover_kde_names_missing_lookandfeeltool() {
    let _lang = i18n_pin::en();
    let io = plasma().file(APPLETSRC, LAYOUT_FORKED).readonly_path(APPLETSRC);
    let (_rc, said) = recover(&io);
    assert!(
        said.iter()
            .any(|l| l == rexon_core::app::LOOKANDFEELTOOL_MISSING_LINE),
        "{said:?}"
    );
}

/// ⭐ The login screen is reset by THE SAME plan that was shown to the person.
///
/// A WATCHDOG with a bite: the original computed the plan, printed it, and then scanned
/// the directory AGAIN and deleted the result of the second scan. Going back to that form means
/// a second `list_dir` of the same directory, and this check goes red.
#[test]
fn guard_sddm_reset_shows_the_plan_it_executes() {
    // The words compared are Russian — as the original printed them (the `po/ru.po` catalog).
    let _lang = i18n_pin::ru();
    let io = plasma()
        .tool("pkexec")
        .file("/etc/sddm.conf.d/zz-rexon-theme.conf", "[Theme]\nCurrent=Sweet\n")
        .file("/etc/sddm.conf.d/kde_settings.conf", "[Theme]\nCurrent=breeze\n");
    let ex = SystemExec::new(&io);
    let mut said: Vec<String> = Vec::new();
    ex.reset_sddm_step(&mut |s| said.push(s.to_string()));

    let shown = said
        .iter()
        .find(|l| l.contains("убираю сторонние настройки"))
        .unwrap_or_else(|| panic!("план не показан: {said:?}"));
    assert!(shown.contains("zz-rexon-theme.conf"), "{shown}");
    assert!(
        !shown.contains("kde_settings.conf"),
        "дистро-файл не наш и не удаляется: {shown}"
    );

    let removal = io
        .commands()
        .into_iter()
        .find(|c| c.contains("rm -f"))
        .unwrap_or_else(|| panic!("удаление не запускалось: {:?}", io.commands()));
    assert!(removal.starts_with("pkexec "), "root-ное только через pkexec: {removal}");
    assert!(removal.contains("/etc/sddm.conf.d/zz-rexon-theme.conf"), "{removal}");
    assert!(!removal.contains("kde_settings.conf"), "{removal}");
    assert_eq!(
        io.lists_of("/etc/sddm.conf.d"),
        1,
        "каталог сканируется РОВНО один раз: второй скан — это удаление не того, что обещано"
    );
}

/// The login screen is already stock — `pkexec` is not run at all (on a clean
/// machine the password is not asked for nothing).
#[test]
fn guard_sddm_already_stock_runs_nothing() {
    // The language is pinned: the string and the expectation are computed at different moments, and
    // a neighboring check managed to switch the language between them.
    let _lang = i18n_pin::en();
    let io = plasma().tool("pkexec").dir("/etc/sddm.conf.d");
    let ex = SystemExec::new(&io);
    let mut said: Vec<String> = Vec::new();
    ex.reset_sddm_step(&mut |s| said.push(s.to_string()));
    assert_eq!(said, vec![rexon_core::app::sddm_already_stock_line(None)]);
    assert!(io.commands().is_empty(), "{:?}", io.commands());
}

/// ⭐ Fix of the original: the `reconfigure` fallback was dead — the same
/// resolver returned the same binary, and the command was repeated verbatim. Here
/// `qdbus` is tried after a `dbus-send` that did not answer.
#[test]
fn kwin_reconfigure_falls_back_to_qdbus() {
    let _lang = i18n_pin::en();
    let io = plasma()
        .tool("dbus-send")
        .tool("qdbus6")
        .reply(
            "/usr/bin/dbus-send --session --print-reply --dest=org.kde.KWin \
             --type=method_call /KWin org.kde.KWin.reconfigure",
            Reply::timeout(),
        )
        .reply(
            "/usr/bin/qdbus6 org.kde.KWin /KWin org.kde.KWin.reconfigure",
            Reply::ok(""),
        );
    let ex = SystemExec::new(&io);
    assert!(ex.kwin_reconfigure(), "{:?}", io.commands());
    assert_eq!(io.commands().len(), 2, "пробуются ОБА бинаря: {:?}", io.commands());
}

/// The bus could not be reached at all — the edit went into the file, and the person is
/// told so in the log, instead of "silently nothing happened".
#[test]
fn kwin_reconfigure_failure_is_named_in_log() {
    let _lang = i18n_pin::en();
    let io = plasma();
    let ex = SystemExec::new(&io);
    let mut said: Vec<String> = Vec::new();
    ex.reset_kwin_theme_effects(&mut |s| said.push(s.to_string()));
    assert_eq!(said, vec![rexon_core::app::KWIN_EFFECTS_RESET_LINE.to_string()]);
    assert_eq!(ex.logs(), vec![rexon_core::app::KWIN_RECONFIGURE_FAILED_LOG.to_string()]);
}

/// ⛔ BOUNDARY WATCHDOG. During the whole emergency rollback not a single
/// command changing the system goes outside other than `pkexec` from the core's plans, and
/// not a single file is written except the panel layout and its backup.
#[test]
fn guard_recover_kde_touches_only_what_it_promised() {
    let _lang = i18n_pin::en();
    let io = plasma()
        .tool("pkexec")
        .tool("dbus-send")
        .file(APPLETSRC, LAYOUT_FORKED)
        .file("/etc/sddm.conf.d/zz-rexon-theme.conf", "[Theme]\nCurrent=Sweet\n")
        .file("/cfg/Kvantum/kvantum.kvconfig", "[General]\ntheme=SevenAero\n")
        .dir("/usr/share/wallpapers/Next");
    let (rc, _said) = recover(&io);
    assert_eq!(rc, 0);

    for cmd in io.commands() {
        let head = cmd.split(' ').next().unwrap_or_default();
        let root_level = cmd.contains("/etc/") || cmd.contains(" rm -f");
        assert!(
            !root_level || head == "pkexec",
            "системное меняется только через pkexec: {cmd}"
        );
        assert!(!cmd.contains("sudo "), "своего повышения прав здесь нет: {cmd}");
    }
    let touched: Vec<String> = io.written().into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        touched,
        vec![
            APPLETSRC.to_string(),
            format!("{APPLETSRC}.rexon-bak"),
        ],
        "маршрут пишет своими руками только раскладку панели"
    );
}

/// A live FS, but our own directory instead of `$HOME` and `$XDG_CONFIG_HOME`, and no programs
/// at all: a font reset in a sandbox.
struct SandboxIo {
    home: String,
}

impl SystemIo for SandboxIo {
    fn exists(&self, path: &str) -> bool { rexon_core::system_exec::LiveSystemIo.exists(path) }
    fn is_file(&self, path: &str) -> bool { rexon_core::system_exec::LiveSystemIo.is_file(path) }
    fn is_dir(&self, path: &str) -> bool { rexon_core::system_exec::LiveSystemIo.is_dir(path) }
    fn is_executable(&self, _: &str) -> bool { false }
    fn realpath(&self, path: &str) -> Option<String> { rexon_core::system_exec::LiveSystemIo.realpath(path) }
    fn read_text(&self, path: &str) -> Option<String> { rexon_core::system_exec::LiveSystemIo.read_text(path) }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> { rexon_core::system_exec::LiveSystemIo.list_dir(dir) }
    fn which(&self, _: &str) -> bool { false }
    fn run(&self, _: &[String], _: Option<u64>, _: bool) -> RunOut { RunOut::missing() }
    fn env(&self, name: &str) -> String {
        match name {
            "HOME" => self.home.clone(),
            "XDG_CONFIG_HOME" => format!("{}/.config", self.home),
            _ => String::new(),
        }
    }
    fn which_path(&self, _: &str) -> Option<String> { None }
    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        rexon_core::system_exec::LiveSystemIo.write_text(path, content)
    }
    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        rexon_core::system_exec::LiveSystemIo.copy_file(src, dst)
    }
    fn unlink_file(&self, path: &str) -> Result<(), String> {
        rexon_core::system_exec::LiveSystemIo.unlink_file(path)
    }
    fn replace_with_symlink(&self, path: &str, target: &str) -> Result<(), String> {
        rexon_core::system_exec::LiveSystemIo.replace_with_symlink(path, target)
    }
}

/// The font reset («Restore KDE Theme») removes ALL layers put by
/// the «Fonts» page — on real files: the layers are written by the same core
/// functions as in the product, other lines remain, ours are gone, files where
/// nothing remained are deleted, and `gtk.css`, which was a link to the theme, is a
/// link again.
#[test]
fn font_reset_strips_every_rexon_layer_on_real_files() {
    use rexon_core::qt_settings::{self as qs, FontFile, FontFileIo};
    use std::fs;

    let root = std::env::temp_dir().join(format!("rexon-font-reset-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    let cfg = home.join(".config");
    for dir in ["gtk-3.0", "gtk-4.0", "qt6ct", "qt5ct", "fontconfig", "cosmic/com.system76.CosmicTk/v1"] {
        fs::create_dir_all(cfg.join(dir)).unwrap();
    }
    let theme = root.join("theme/gtk-4.0/gtk.css");
    fs::create_dir_all(theme.parent().unwrap()).unwrap();
    fs::write(&theme, "window { color: blue; }\n").unwrap();

    // Someone else's content before us.
    let gtk3_foreign = "@import url(\"colors.css\");\nwindow { color: red; }\n";
    fs::write(cfg.join("gtk-3.0/gtk.css"), gtk3_foreign).unwrap();
    std::os::unix::fs::symlink(&theme, cfg.join("gtk-4.0/gtk.css")).unwrap();
    fs::write(
        cfg.join("qt6ct/qt6ct.conf"),
        "[Appearance]\nstyle=Fusion\n\n[Interface]\nstylesheets=/usr/share/qt6ct/qss/fusion-fixes.qss\n",
    )
    .unwrap();
    fs::write(cfg.join("qt5ct/qt5ct.conf"), "[Appearance]\nstyle=Fusion\n").unwrap();
    fs::write(home.join(".Xresources"), "Xcursor.size: 24\n").unwrap();
    let cosmic = cfg.join("cosmic/com.system76.CosmicTk/v1");
    let cosmic_foreign = "(\n    family: \"Inter\",\n    weight: Normal,\n    stretch: Normal,\n    style: Normal,\n)";
    fs::write(cosmic.join("monospace_font"), cosmic_foreign).unwrap();

    // Our layers — with the same functions the «Fonts» page writes with.
    struct Files(std::path::PathBuf);
    impl FontFileIo for Files {
        fn read(&self, rel: &str) -> Option<String> { fs::read_to_string(self.0.join(rel)).ok() }
        fn link_target(&self, rel: &str) -> Option<String> {
            let p = self.0.join(rel);
            fs::symlink_metadata(&p).ok()?.file_type().is_symlink().then(|| {
                fs::canonicalize(&p).unwrap().to_string_lossy().into_owned()
            })
        }
    }
    let base = cfg.to_string_lossy().into_owned();
    let files = Files(cfg.clone());
    let write = |file: FontFile| {
        for w in file.file_writes(&base, &files, "Fira Sans", 9).expect("слой пишется") {
            let p = cfg.join(&w.rel_path);
            if w.replace_link {
                fs::remove_file(&p).unwrap();
            }
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, &w.text).unwrap();
        }
    };
    for version in ["3.0", "4.0"] {
        for target in ["small", "menu"] {
            write(FontFile::GtkCss { version: version.into(), target: target.into() });
        }
    }
    for tool in ["qt6ct", "qt5ct"] {
        write(FontFile::QtCtQss { tool: tool.into(), target: "toolbar".into() });
    }
    write(FontFile::CosmicTk { key: "interface_font".into() });
    let fc = qs::fontconfig_with_block(None, &qs::fontconfig_block(&qs::FontRendering::default(), Some(120))).unwrap();
    fs::write(cfg.join("fontconfig/fonts.conf"), fc).unwrap();
    let xr = qs::xresources_with_dpi(Some("Xcursor.size: 24\n"), Some(120)).unwrap();
    fs::write(home.join(".Xresources"), xr).unwrap();

    // The layers are in place — otherwise there is nothing to check.
    assert!(fs::read_to_string(cfg.join("gtk-3.0/gtk.css")).unwrap().contains("Rexon: fonts"));
    assert!(!fs::symlink_metadata(cfg.join("gtk-4.0/gtk.css")).unwrap().file_type().is_symlink());
    assert!(cfg.join("qt6ct/qss/rexon-fonts.qss").exists());
    assert!(fs::read_to_string(cfg.join("qt5ct/qt5ct.conf")).unwrap().contains("rexon-fonts.qss"));
    assert!(fs::read_to_string(home.join(".Xresources")).unwrap().contains("Xft.dpi"));
    assert!(cosmic.join("interface_font").exists());

    let io = SandboxIo { home: home.to_string_lossy().into_owned() };
    let ex = SystemExec::new(&io);
    let mut said = Vec::new();
    ex.reset_kde_fonts(&mut |s| said.push(s.to_string()));
    assert!(ex.logs().is_empty(), "{:?}", ex.logs());

    assert_eq!(fs::read_to_string(cfg.join("gtk-3.0/gtk.css")).unwrap(), gtk3_foreign);
    let gtk4 = cfg.join("gtk-4.0/gtk.css");
    assert!(fs::symlink_metadata(&gtk4).unwrap().file_type().is_symlink(), "снова ссылка на тему");
    assert_eq!(fs::canonicalize(&gtk4).unwrap(), fs::canonicalize(&theme).unwrap());
    assert!(!cfg.join("qt6ct/qss/rexon-fonts.qss").exists());
    assert!(!cfg.join("qt5ct/qss/rexon-fonts.qss").exists());
    let qt6 = fs::read_to_string(cfg.join("qt6ct/qt6ct.conf")).unwrap();
    assert_eq!(qs::qtct_conf_stylesheets(&qt6), vec!["/usr/share/qt6ct/qss/fusion-fixes.qss".to_string()]);
    assert!(qt6.contains("style=Fusion"));
    let qt5 = fs::read_to_string(cfg.join("qt5ct/qt5ct.conf")).unwrap();
    assert!(qs::qtct_conf_stylesheets(&qt5).is_empty() && qt5.contains("style=Fusion"), "{qt5}");
    assert!(!cfg.join("fontconfig/fonts.conf").exists(), "fonts.conf заводили мы — убран");
    assert_eq!(fs::read_to_string(home.join(".Xresources")).unwrap(), "Xcursor.size: 24\n");
    assert!(!cosmic.join("interface_font").exists());
    assert_eq!(fs::read_to_string(cosmic.join("monospace_font")).unwrap(), cosmic_foreign, "чужой шрифт COSMIC цел");

    // A second reset touches nothing.
    assert_eq!(ex.reset_font_layers(), 0);
    assert_eq!(fs::read_to_string(home.join(".Xresources")).unwrap(), "Xcursor.size: 24\n");
    fs::remove_dir_all(&root).unwrap();
}

// ============================================================================
// Login screen: which is the default, which are installed
// ============================================================================

#[test]
fn active_login_manager_opt_reads_symlink_then_units() {
    let _lang = i18n_pin::en();
    let live = arch()
        .tool("systemctl")
        .file("/usr/lib/systemd/system/plasmalogin.service", "[Unit]\n")
        .link(DM_LINK, "/usr/lib/systemd/system/plasmalogin.service");
    assert_eq!(
        SystemExec::new(&live).active_login_manager_opt().as_deref(),
        Some("plasmalogin")
    );
    assert!(live.commands().is_empty(), "живой симлинк решает без systemctl");

    // A dangling symlink and nothing enabled — "we do not know", without "sddm".
    let dangling = arch()
        .tool("systemctl")
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service");
    let ex = SystemExec::new(&dangling);
    assert_eq!(ex.active_login_manager_opt(), None);
    assert_eq!(ex.active_login_manager(), "sddm", "прежний ответ не изменился");
    assert!(
        dangling.commands().iter().all(|c| c.starts_with("systemctl is-enabled ")),
        "только чтение состояния юнитов: {:?}",
        dangling.commands()
    );
}

#[test]
fn installed_login_managers_scan_unit_dirs() {
    let _lang = i18n_pin::en();
    let alias = "[Service]\nExecStart=/usr/bin/x\n[Install]\nAlias=display-manager.service\n";
    let io = arch()
        .file("/usr/lib/systemd/system/sddm.service", alias)
        .file("/usr/lib/systemd/system/lightdm.service", alias)
        .file("/usr/lib/systemd/system/cosmic-greeter-daemon.service", "[Install]\nWantedBy=multi-user.target\n")
        .file("/usr/lib/systemd/system/getty@.service", alias)
        .file("/usr/lib/systemd/system/lemurs.service", alias)
        .file("/usr/lib/systemd/system/sshd.service", "[Install]\nWantedBy=multi-user.target\n")
        .file("/dev/null", "")
        .dir("/etc/systemd/system")
        // A mask does not hide the packaged lightdm in the list.
        .link("/etc/systemd/system/lightdm.service", "/dev/null")
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service");
    let ex = SystemExec::new(&io);
    assert_eq!(ex.installed_login_managers(), vec!["sddm", "lightdm", "lemurs"]);
    assert!(io.commands().is_empty(), "список строится без команд");
    assert_eq!(io.reads_of("/usr/lib/systemd/system/getty@.service"), 0, "шаблоны не читаются");
}

#[test]
fn dm_switch_backend_from_probes() {
    let _lang = i18n_pin::en();
    let io = arch().tool("systemctl").dir("/run/systemd/system");
    assert_eq!(SystemExec::new(&io).dm_switch_backend(), plan::DmSwitchBackend::Systemd);
    assert!(io.commands().is_empty(), "пробы — без команд");

    let debian = FakeIo::new()
        .file("/etc/os-release", "ID=debian\nNAME=Debian\n")
        .tool("apt-get")
        .tool("systemctl")
        .dir("/run/systemd/system")
        .file("/etc/X11/default-display-manager", "/usr/sbin/gdm3\n");
    assert_eq!(SystemExec::new(&debian).dm_switch_backend(), plan::DmSwitchBackend::SystemdDebian);

    let suse = FakeIo::new()
        .file("/etc/os-release", "ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n")
        .tool("zypper")
        .tool("systemctl")
        .dir("/run/systemd/system")
        .file("/usr/lib/systemd/system/display-manager.service", "[Service]\nExecStart=/usr/lib/X11/display-manager start\n")
        .link(DM_LINK, "/usr/lib/systemd/system/display-manager.service")
        .file("/usr/lib/X11/displaymanagers/sddm", "")
        .link("/etc/alternatives/default-displaymanager", "/usr/lib/X11/displaymanagers/sddm");
    assert_eq!(
        SystemExec::new(&suse).dm_switch_backend(),
        plan::DmSwitchBackend::SystemdSuseAlternatives
    );

    let artix = arch().tool("rc-update").file("/etc/conf.d/display-manager", "DISPLAYMANAGER=\"sddm\"\n");
    assert_eq!(
        SystemExec::new(&artix).dm_switch_backend(),
        plan::DmSwitchBackend::Unsupported("OpenRC".into())
    );
}

#[test]
fn lightdm_greeters_and_session_read_only() {
    let _lang = i18n_pin::en();
    let io = arch()
        .tool("lightdm")
        .tool("slick-greeter")
        .file("/usr/share/xgreeters/lightdm-slick-greeter.desktop", "[Desktop Entry]\nName=Slick Greeter\nExec=slick-greeter\n")
        .file("/usr/share/xgreeters/lightdm-gtk-greeter.desktop", "[Desktop Entry]\nName=LightDM GTK Greeter\nExec=lightdm-gtk-greeter\n")
        .reply(
            "lightdm --show-config",
            // LightDM prints the config to stderr.
            Reply {
                outcome: RunOutcome::Ok,
                returncode: Some(0),
                stdout: String::new(),
                stderr: "   [Seat:*]\nA  greeter-session=lightdm-slick-greeter\n".into(),
            },
        );
    let ex = SystemExec::new(&io);
    let ids: Vec<String> = ex.lightdm_greeters().into_iter().map(|g| g.id).collect();
    assert_eq!(ids, vec!["lightdm-slick-greeter"], "у GTK нет программы");
    assert_eq!(ex.lightdm_greeter_session().as_deref(), Some("lightdm-slick-greeter"));
    assert_eq!(io.commands(), vec!["lightdm --show-config"]);
}

/// A stand-in machine like Wehrwolfmann's: plasmalogin is the default; sddm,
/// lightdm (two greeters, GTK and Slick), greetd with agreety and
/// cosmic-greeter are installed. No helpers — the test adds them.
fn owner_like_machine() -> FakeIo {
    let sys = "/usr/lib/systemd/system";
    let alias = |exec: &str| format!("[Service]\nExecStart={exec}\n[Install]\nAlias=display-manager.service\n");
    let mut io = arch()
        .tool("systemctl")
        .dir("/run/systemd/system")
        .file(&format!("{sys}/plasmalogin.service"), &alias("/usr/bin/plasmalogin"))
        .file(&format!("{sys}/sddm.service"), &alias("/usr/bin/sddm"))
        .file(&format!("{sys}/lightdm.service"), &alias("/usr/bin/lightdm"))
        .file(&format!("{sys}/greetd.service"), &alias("greetd"))
        .file(&format!("{sys}/cosmic-greeter.service"), &alias("greetd --config /etc/greetd/cosmic-greeter.toml"))
        .file(&format!("{sys}/cosmic-greeter-daemon.service"), "[Install]\nWantedBy=multi-user.target\n")
        .link(DM_LINK, &format!("{sys}/plasmalogin.service"))
        .exec_file("/usr/bin/plasmalogin")
        .exec_file("/usr/bin/sddm")
        .exec_file("/usr/bin/lightdm")
        .tool("greetd")
        .tool("lightdm")
        .tool("lightdm-gtk-greeter")
        .tool("slick-greeter")
        .tool("cosmic-greeter-start")
        .tool("sddm-greeter-qt6")
        .tool("kwin_wayland")
        .tool("Xorg")
        .file("/etc/greetd/config.toml", "[default_session]\ncommand = \"agreety --cmd /bin/sh\"\nuser = \"greeter\"\n")
        .file("/etc/greetd/cosmic-greeter.toml", "[default_session]\ncommand = \"cosmic-greeter-start\"\nuser = \"cosmic-greeter\"\n")
        .file("/usr/share/xgreeters/lightdm-gtk-greeter.desktop", "[Desktop Entry]\nName=LightDM GTK Greeter\nExec=lightdm-gtk-greeter\n")
        .file("/usr/share/xgreeters/lightdm-slick-greeter.desktop", "[Desktop Entry]\nName=Slick Greeter\nExec=slick-greeter\n")
        .file("/etc/sddm.conf.d/kde_settings.conf", "[Theme]\nCurrent=breeze\n")
        .file("/usr/lib/sddm/sddm.conf.d/zz-wayland.conf", "[General]\nDisplayServer=wayland\n[Wayland]\nCompositorCommand=kwin_wayland --drm\n")
        .file("/usr/share/sddm/themes/breeze/metadata.desktop", "[SddmGreeterTheme]\nQtVersion=6\n")
        .file("/etc/passwd", "plasmalogin:x:1:1::/:/bin/false\nsddm:x:2:2::/:/bin/false\nlightdm:x:3:3::/:/bin/false\ncosmic-greeter:x:4:4::/:/bin/false\n")
        .file("/usr/lib/sysusers.d/greetd.conf", "u greeter - \"greetd greeter user\" - /bin/bash\n")
        .file("/usr/share/xsessions/mate.desktop", "")
        .file("/usr/share/wayland-sessions/plasma.desktop", "");
    for pam in ["/usr/lib/pam.d/plasmalogin", "/etc/pam.d/sddm", "/etc/pam.d/lightdm", "/etc/pam.d/greetd"] {
        io = io.file(pam, "");
    }
    io.reply("lightdm --show-config", Reply::ok("   [Seat:*]\nA  session-wrapper=/etc/lightdm/Xsession\n"))
}

/// Wehrwolfmann's machine in a stand-in FS: plasmalogin is the default; sddm and
/// cosmic-greeter are ready; LightDM waits for a greeter choice (preselected —
/// Slick); greetd is blocked because of agreety. Not a single command except
/// reading `lightdm --show-config`.
#[test]
fn login_managers_on_owner_like_machine() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine();
    let list = SystemExec::new(&io).login_managers();
    let units: Vec<&str> = list.iter().map(|m| m.unit.as_str()).collect();
    assert_eq!(units, vec!["plasmalogin", "sddm", "lightdm", "greetd", "cosmic-greeter"]);
    let by = |u: &str| list.iter().find(|m| m.unit == u).unwrap();
    assert!(by("plasmalogin").active);
    assert_eq!(list.iter().filter(|m| m.active).count(), 1);
    assert_eq!(by("plasmalogin").readiness, plan::DmReadiness::Ready);
    assert_eq!(by("sddm").readiness, plan::DmReadiness::Ready);
    assert_eq!(by("cosmic-greeter").readiness, plan::DmReadiness::Ready);
    assert_eq!(by("cosmic-greeter").label, "COSMIC Greeter");
    assert!(matches!(by("lightdm").readiness, plan::DmReadiness::NeedsGreeterChoice(_)));
    assert_eq!(by("lightdm").suggested_greeter().unwrap().id, "lightdm-slick-greeter");
    assert_eq!(
        by("greetd").readiness,
        plan::DmReadiness::Blocked("greetd is set up for text sign-in — the desktop won't start.".into())
    );
    assert!(list.iter().all(|m| m.present && m.warnings.is_empty()));
    assert_eq!(io.commands(), vec!["lightdm --show-config"], "только чтение");
}

#[test]
fn login_managers_without_systemd_show_only() {
    let _lang = i18n_pin::en();
    let io = arch()
        .tool("rc-update")
        .tool("lightdm")
        .tool("sddm")
        .file("/etc/conf.d/display-manager", "DISPLAYMANAGER=\"lightdm\"\n");
    let list = SystemExec::new(&io).login_managers();
    let units: Vec<(&str, bool)> = list.iter().map(|m| (m.unit.as_str(), m.active)).collect();
    assert_eq!(units, vec![("sddm", false), ("lightdm", true)]);
    assert!(list.iter().all(|m| m.readiness == plan::DmReadiness::Unsupported));
    assert!(io.commands().is_empty());
}

// ── The «Login manager» section: a snapshot for the interface (G1) ──────────────────

const IS_ACTIVE_ALL: &str =
    "systemctl is-active plasmalogin.service sddm.service lightdm.service greetd.service cosmic-greeter.service";

fn is_active(states: &[&str]) -> Reply {
    // `is-active` ends with code 3 if at least one is not running — the answer
    // lines are still there.
    Reply::failed(3, &(states.join("\n") + "\n"), "")
}

#[test]
fn login_screen_view_on_owner_like_machine() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine()
        .with_helpers()
        .reply(IS_ACTIVE_ALL, is_active(&["active", "inactive", "inactive", "inactive", "inactive"]));
    let view = SystemExec::new(&io).login_screen_view();
    assert_eq!(view.backend, "systemd");
    assert!(view.switchable);
    assert_eq!(view.note, "");
    assert_eq!(view.pending, "", "основной и работающий совпадают");
    assert_eq!(view.rollback, "", "файла с подсказкой нет");
    let by = |u: &str| view.managers.iter().find(|m| m.unit == u).unwrap();
    let main = by("plasmalogin");
    assert!(main.active && main.running && !main.can_switch, "у основного — метка, а не кнопка");
    assert!(by("sddm").can_switch && by("cosmic-greeter").can_switch);
    assert_eq!(by("sddm").state, "ready");
    assert_eq!(by("sddm").confirm_title, "Make SDDM the login screen?");
    assert!(
        by("sddm").confirm.contains("sudo systemctl enable --force plasmalogin.service && sudo reboot"),
        "окно подтверждения называет, как вернуть прежний: {}",
        by("sddm").confirm
    );
    let lightdm = by("lightdm");
    assert_eq!(lightdm.state, "needs_greeter");
    assert!(lightdm.can_switch);
    assert_eq!(lightdm.greeter, "lightdm-slick-greeter", "предвыбор — Slick");
    let names: Vec<&str> = lightdm.greeters.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, vec!["LightDM GTK Greeter", "Slick Greeter"]);
    assert!(lightdm.greeters[1].confirm.contains("Greeter: Slick Greeter."));
    let greetd = by("greetd");
    assert_eq!(greetd.state, "blocked");
    assert!(!greetd.can_switch, "UI запирает кнопку");
    assert_eq!(greetd.reason, "greetd is set up for text sign-in — the desktop won't start.");
    assert_eq!(io.commands(), vec!["lightdm --show-config".to_string(), IS_ACTIVE_ALL.to_string()]);
    assert!(io.commands().iter().all(|c| !c.starts_with("pkexec")), "проба только читает");
}

/// Autologin on the default one: the confirmation window BEFORE the password says what will
/// happen to it on every other login screen — it will carry over or disappear.
#[test]
fn login_screen_view_warns_about_autologin_before_switch() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine()
        .with_helpers()
        .file("/etc/plasmalogin.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n")
        .reply(IS_ACTIVE_ALL, is_active(&["active", "inactive", "inactive", "inactive", "inactive"]));
    let view = SystemExec::new(&io).login_screen_view();
    let by = |u: &str| view.managers.iter().find(|m| m.unit == u).unwrap();
    assert!(by("sddm").confirm.contains("Automatic sign-in for alice moves to SDDM as well."), "{}", by("sddm").confirm);
    let lightdm = &by("lightdm").greeters[1].confirm;
    assert!(lightdm.contains("moves to LightDM") && lightdm.contains("game mode"), "{lightdm}");
    let cosmic = &by("cosmic-greeter").confirm;
    assert!(cosmic.contains("Automatic sign-in for alice will be lost"), "{cosmic}");
    assert!(!by("plasmalogin").confirm.contains("Automatic sign-in"), "у основного — ничего");
    assert!(io.commands().iter().all(|c| !c.starts_with("pkexec")), "проба только читает");
}

#[test]
fn login_screen_view_without_helper_locks_every_button() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine().reply(IS_ACTIVE_ALL, is_active(&["active"; 5]));
    let view = SystemExec::new(&io).login_screen_view();
    assert!(!view.switchable);
    assert_eq!(view.note, plan::login_manager_needs_helper());
    assert!(view.managers.iter().all(|m| !m.can_switch));
}

/// The default is already SDDM (switched), but Plasma Login Manager is still running:
/// the section says it takes effect after a reboot and shows the rollback
/// hint from the helper's file.
#[test]
fn login_screen_view_tells_restart_pending_and_rollback() {
    let _lang = i18n_pin::en();
    let previous = "Before Rexon, the login screen was plasmalogin.\n    sudo systemctl enable --force plasmalogin.service && sudo reboot\n";
    let io = owner_like_machine()
        .with_helpers()
        .link(DM_LINK, "/usr/lib/systemd/system/sddm.service")
        .file(plan::PREVIOUS_DM_FILE, previous)
        .reply(IS_ACTIVE_ALL, is_active(&["active", "inactive", "inactive", "inactive", "inactive"]));
    let view = SystemExec::new(&io).login_screen_view();
    let by = |u: &str| view.managers.iter().find(|m| m.unit == u).unwrap();
    assert!(by("sddm").active && !by("sddm").running);
    assert!(by("plasmalogin").running && !by("plasmalogin").active);
    assert!(by("plasmalogin").can_switch, "вернуть прежний — той же кнопкой");
    assert_eq!(
        view.pending,
        "SDDM becomes the login screen after a restart. Plasma Login Manager is still running now."
    );
    assert_eq!(view.rollback, previous.trim());
}

#[test]
fn login_screen_view_without_systemd_shows_command_instead_of_button() {
    let _lang = i18n_pin::en();
    let io = arch()
        .tool("rc-update")
        .tool("lightdm")
        .tool("sddm")
        .with_helpers()
        .file("/etc/conf.d/display-manager", "DISPLAYMANAGER=\"lightdm\"\n");
    let view = SystemExec::new(&io).login_screen_view();
    assert_eq!(view.backend, "other");
    assert!(!view.switchable);
    assert!(view.note.contains("OpenRC"), "{}", view.note);
    let sddm = view.managers.iter().find(|m| m.unit == "sddm").unwrap();
    assert!(!sddm.can_switch);
    assert!(sddm.reason.contains("DISPLAYMANAGER=\"sddm\""), "команда для этой init: {}", sddm.reason);
    assert!(io.commands().is_empty(), "без systemd — ни одной команды");
}

#[test]
fn login_screen_view_on_immutable_system_has_no_buttons() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine()
        .with_helpers()
        .file("/etc/os-release", "ID=kinoite\nID_LIKE=fedora\nNAME=Fedora Kinoite\n")
        .tool("rpm-ostree")
        .tool("dnf")
        .reply(IS_ACTIVE_ALL, is_active(&["active"; 5]));
    let view = SystemExec::new(&io).login_screen_view();
    assert!(!view.switchable);
    assert_eq!(view.note, rexon_core::system_exec::login_manager_immutable_note());
    assert!(view.managers.iter().all(|m| !m.can_switch));
}

#[test]
fn login_screen_view_unknown_login_screen_is_listed_without_button() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine()
        .with_helpers()
        .file(
            "/usr/lib/systemd/system/lemurs.service",
            "[Service]\nExecStart=/usr/bin/lemurs\n[Install]\nAlias=display-manager.service\n",
        )
        .exec_file("/usr/bin/lemurs");
    let view = SystemExec::new(&io).login_screen_view();
    let lemurs = view.managers.iter().find(|m| m.unit == "lemurs").unwrap();
    assert_eq!(lemurs.label, "lemurs", "незнакомый — по имени юнита");
    assert_eq!(lemurs.state, "unsupported");
    assert!(!lemurs.can_switch);
    assert_eq!(lemurs.reason, rexon_core::system_exec::login_manager_cant_switch_reason());
}

// ── «Make default» (G2): a stub executor instead of pkexec ─────────

fn switch_argv(unit: &str, greeter: Option<&str>) -> String {
    plan::switch_dm_cmd(PkgFamily::Arch, unit, greeter, true).unwrap().join(" ")
}

fn pkexec_runs(io: &FakeIo) -> Vec<String> {
    io.commands().into_iter().filter(|c| c.starts_with("pkexec")).collect()
}

/// The core refuses by itself, even if the interface did not lock the button (rule
/// #13), and the password is not asked.
#[test]
fn switch_login_manager_refuses_before_password() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine().with_helpers();
    let ex = SystemExec::new(&io);
    let cases: Vec<(&str, Option<&str>, String)> = vec![
        ("greetd", None, plan::login_manager_switch_failed("greetd is set up for text sign-in — the desktop won't start.")),
        ("plasmalogin", None, "Plasma Login Manager is already the login screen.".into()),
        ("lightdm", None, "First choose which greeter LightDM should show.".into()),
        ("lightdm", Some("lightdm-webkit2-greeter"), "First choose which greeter LightDM should show.".into()),
        ("lightdm", Some("../../bin/sh"), "First choose which greeter LightDM should show.".into()),
        ("lemurs", None, plan::login_manager_switch_failed("this login screen isn't installed")),
    ];
    for (unit, greeter, want) in cases {
        assert_eq!(ex.switch_login_manager(unit, greeter), (false, want), "{unit} {greeter:?}");
    }
    assert!(pkexec_runs(&io).is_empty(), "ни одного pkexec: {:?}", io.commands());
}

#[test]
fn switch_login_manager_without_helper_has_no_raw_pkexec() {
    let _lang = i18n_pin::en();
    let io = owner_like_machine().tool("pkexec");
    let got = SystemExec::new(&io).switch_login_manager("sddm", None);
    assert_eq!(got, (false, plan::login_manager_needs_helper()));
    assert!(pkexec_runs(&io).is_empty());
}

#[test]
fn switch_login_manager_refuses_where_rexon_does_not_switch() {
    let _lang = i18n_pin::en();
    let kinoite = owner_like_machine()
        .with_helpers()
        .file("/etc/os-release", "ID=kinoite\nID_LIKE=fedora\nNAME=Fedora Kinoite\n");
    let got = SystemExec::new(&kinoite).switch_login_manager("sddm", None);
    assert_eq!(got, (false, rexon_core::system_exec::login_manager_immutable_note()));
    assert!(pkexec_runs(&kinoite).is_empty());

    let artix = arch().tool("rc-update").tool("sddm").with_helpers();
    let (ok, msg) = SystemExec::new(&artix).switch_login_manager("sddm", None);
    assert!(!ok && msg.contains("OpenRC"), "{msg}");
    assert!(pkexec_runs(&artix).is_empty());
}

/// A ready SDDM: one helper call; the result is "takes effect after
/// a reboot" and how to restore the old one from the console.
#[test]
fn switch_login_manager_runs_helper_once_and_says_how_to_go_back() {
    let _lang = i18n_pin::en();
    let argv = switch_argv("sddm", None);
    let io = owner_like_machine().with_helpers().reply(&argv, Reply::ok("rexon-pkg-helper: done\n"));
    let (ok, msg) = SystemExec::new(&io).switch_login_manager("sddm", None);
    assert!(ok, "{msg}");
    assert!(msg.starts_with("Done: SDDM will be the login screen after a restart."), "{msg}");
    assert!(msg.contains("Ctrl+Alt+F3"), "{msg}");
    assert!(msg.contains("sudo systemctl enable --force plasmalogin.service && sudo reboot"), "{msg}");
    assert_eq!(pkexec_runs(&io), vec![argv]);
    assert!(
        io.commands().iter().all(|c| !c.contains(" start") && !c.contains("restart") && !c.contains(" stop")),
        "в живом сеансе службы не трогаются: {:?}",
        io.commands()
    );
}

/// The window has a privilege session: the switch goes to the session helper as an operation
/// (and also without the old package helper), `pkexec` is not called. A refusal of
/// the session helper is a refusal, without the old command (Z5).
#[test]
fn switch_login_manager_goes_through_privilege_session() {
    use rexon_core::privilege::op::PrivOp;
    let _lang = i18n_pin::en();
    for io in [owner_like_machine().with_helpers(), owner_like_machine().tool("pkexec")] {
        let io = FakeIo { session: Some(Reply::ok("")), ..io };
        let (ok, msg) = SystemExec::new(&io).switch_login_manager("sddm", None);
        assert!(ok, "{msg}");
        assert!(msg.starts_with("Done: SDDM will be the login screen after a restart."), "{msg}");
        assert!(pkexec_runs(&io).is_empty(), "{:?}", io.commands());
        assert_eq!(
            io.priv_ops.borrow().as_slice(),
            [PrivOp::SwitchDm { family: PkgFamily::Arch, unit: "sddm".into(), greeter: None }]
        );
    }
    let refused = Reply::failed(1, "", "rexon-priv-helper: помощник отказал: refused.display-manager");
    let io = FakeIo { session: Some(refused), ..owner_like_machine().with_helpers() };
    let (ok, _) = SystemExec::new(&io).switch_login_manager("sddm", None);
    assert!(!ok);
    assert!(pkexec_runs(&io).is_empty(), "после отказа сеанса прежней команды нет: {:?}", io.commands());
    // The password was not confirmed — the old explanation with the manual command.
    let io = FakeIo { session: Some(Reply::failed(126, "", "")), ..owner_like_machine().with_helpers() };
    let (ok, msg) = SystemExec::new(&io).switch_login_manager("sddm", None);
    assert!(!ok);
    assert!(msg.contains(&format!("sudo {}", &switch_argv("sddm", None)["pkexec ".len()..])), "{msg}");
}

#[test]
fn switch_login_manager_passes_chosen_greeter_to_helper() {
    let _lang = i18n_pin::en();
    let argv = switch_argv("lightdm", Some("lightdm-slick-greeter"));
    assert!(argv.ends_with("switch-dm lightdm lightdm-slick-greeter"), "{argv}");
    let io = owner_like_machine().with_helpers().reply(&argv, Reply::ok(""));
    let (ok, msg) = SystemExec::new(&io).switch_login_manager("lightdm", Some("lightdm-slick-greeter"));
    assert!(ok, "{msg}");
    assert_eq!(pkexec_runs(&io), vec![argv]);
    // A ready non-LightDM gets no greeter passed at all.
    let sddm = switch_argv("sddm", None);
    let io = owner_like_machine().with_helpers().reply(&sddm, Reply::ok(""));
    assert!(SystemExec::new(&io).switch_login_manager("sddm", Some("lightdm-slick-greeter")).0);
    assert_eq!(pkexec_runs(&io), vec![sddm]);
}

#[test]
fn switch_login_manager_password_not_confirmed() {
    let _lang = i18n_pin::en();
    let argv = switch_argv("sddm", None);
    for reply in [
        Reply::failed(126, "", "Error executing command as another user: Not authorized"),
        Reply::failed(127, "", ""),
        // A different code, but pkexec said "Not authorized" — the same refusal.
        Reply::failed(1, "", "Error executing command as another user: Not authorized\n"),
    ] {
        let io = owner_like_machine().with_helpers().reply(&argv, reply);
        let (ok, msg) = SystemExec::new(&io).switch_login_manager("sddm", None);
        assert!(!ok);
        assert!(msg.starts_with("The administrator password wasn't confirmed."), "{msg}");
        assert!(msg.contains(&format!("sudo {}", plan::pkg_helper_path())), "ручная команда без pkexec: {msg}");
        assert!(!msg.contains("pkexec"), "{msg}");
    }
}

#[test]
fn switch_login_manager_reports_helper_reason() {
    let _lang = i18n_pin::en();
    let argv = switch_argv("sddm", None);
    let io = owner_like_machine()
        .with_helpers()
        .reply(&argv, Reply::failed(1, "", "rexon-pkg-helper: the link points elsewhere — the previous login screen is back\n"));
    let got = SystemExec::new(&io).switch_login_manager("sddm", None);
    assert_eq!(
        got,
        (false, "Couldn't change the login screen: the link points elsewhere — the previous login screen is back".into())
    );
}
