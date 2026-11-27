//! Live executor of the `--apply-defaults` route — default applications
//! at session login.
//!
//! WHY. The decisions of this route have long been in Rust: the "desktop →
//! programs" tables, parsing `$XDG_CURRENT_DESKTOP`, the per-DE `mimeapps.list` text,
//! DE-specific terminal commands and the whole completeness check flow
//! ([`crate::appdefaults`], [`crate::compliance`]). But there was not a single
//! ACTION: the step order and file writes were held by Python
//! (`rexon/app.py:625` → `DEAppSwitcher` + `ComplianceCheck`). Because of that,
//! autostart at login had to bring up the interpreter with the
//! bridge wheel, and the route was not available to the Rust process at all.
//!
//! The split here is the same as in the neighboring executors — `wallpaper_exec`,
//! `splash_exec`, `ext_exec`, `imp_live`: **the core decides, the executor does**.
//!   * WHAT to do — [`crate::appdefaults`] (the `generate_profile` outcome, the
//!     `apply_de_specific` plan, `xdg-mime`/`busctl` argv, autostart texts) and
//!     [`crate::compliance`] (the whole check flow);
//!   * WITH WHAT — the [`AppDefaultsIo`] trait: each method is exactly one side
//!     call or observation. In the product it is executed by [`LiveAppDefaultsIo`]
//!     (the real FS and subprocesses), in checks — by a fake executor, and
//!     the live machine does not change at all;
//!   * the ORDER of steps — [`DeAppSwitcher`] and [`apply_defaults_route`].
//!
//! The completeness check needs its own executor ([`LiveComplianceIo`],
//! an implementation of [`crate::compliance::ComplianceIo`]): until now it lived only
//! in the `rexon-py` bridge on top of a Python object, and the Rust "Settings" tab
//! honestly wrote that it could not run the check. Now there is one executor
//! for the product, and it sits next to the route that sets it up; the
//! "Check and install missing" button goes through `system_de_exec::complete_de`.
//!
//! ⭐ WHAT WAS FIXED DURING THE PORT (rule #16 — we do not preserve the original's
//! defects):
//!
//!   1. **Autostart no longer points into the void.** The original, not finding
//!      `rexon` in `PATH`, wrote the path
//!      `/usr/local/lib/rexon/rexon_apply_defaults.sh` into autostart and tried to create a
//!      bash wrapper there. A regular user process cannot write to `/usr/local/lib`:
//!      `_create_apply_script` caught the error, wrote a line to the
//!      log — and autostart was left with `Exec=` pointing to a NONEXISTENT file.
//!      After that the application defaults were never applied, and there was no way
//!      to find out: the next session login silently did nothing.
//!      Here the fallback is the running Rexon binary ITSELF
//!      ([`AppDefaultsIo::self_exe`]): it handles `--apply-defaults` itself,
//!      no intermediary is needed. If even that is not found, autostart is NOT installed and
//!      says why (rule #13) instead of reporting success.
//!   2. **Skipping a desktop is no longer called creating a file.** The original
//!      in `apply_for_current_de` printed "📝 Created kde-mimeapps.list" on
//!      ANY success of `generate_profile` — including the outcome "the desktop is not
//!      installed, skipping", when no file appeared. The outcome is now
//!      distinguishable by type ([`GenerateOutcome`]), and the log gets what actually
//!      happened.
//!   3. **A failed `helpers.rc` write is no longer swallowed.** For XFCE the
//!      original wrote the write error to the debug log and returned the logs
//!      as if the setting had been applied. Now a failure is a line in the same
//!      log the person sees at login.
//!   4. **`~/.local/share/dbus-1/services` respects `$XDG_DATA_HOME`.**
//!      In the original, the FileManager1 override path was expanded by a helper
//!      that knows both XDG branches, but in the module itself it was a constant
//!      computed at import — that is, before the process environment
//!      could be known. Here the path is built from the launch environment.
//!   5. **"Open containing folder" no longer depends on who got there
//!      first.** The original assumed the defaults are mimeapps and stopped
//!      there. But the "show file in folder" button goes by the name
//!      `org.freedesktop.FileManager1` on the user bus, and while the name
//!      is held by a LIVE foreign daemon, D-Bus does not look at any `.service` —
//!      neither the vendor one nor our override. A caught case (09.09.2026, the owner's live
//!      machine): `xdg-mime query default inode/directory` →
//!      `org.kde.dolphin.desktop`, while folders were opened by Thunar, brought up by KDE from
//!      a saved session (`ksmserverrc`, `program1=Thunar`,
//!      `restartCommand1=Thunar,…,--daemon`). The route now checks the slot
//!      at every login and returns it to the assigned manager
//!      ([`DeAppSwitcher::ensure_filemanager1`]). This also fixes the old
//!      attempt to evict the owner: it compared `/proc/<pid>/comm` with
//!      lowercase short names, while Thunar's `comm` is `Thunar`, so
//!      it never fired on it.
//!
//! The python3 bash autostart wrapper is gone everywhere: the Python facade went
//! to the archive (02.10.2026), and its text (`apply_script_content`) was removed
//! from the core along with it — see item 1.

use std::path::Path;

use crate::appdefaults::{self as ad, ProfileOutcome};
use crate::i18n::{tr, trf};
use crate::compliance::{self as comp, ComplianceIo, RunResult, ScanEntry};
use crate::system::{DistroDetector, PkgFamily};
use crate::system_plan;
use crate::utils::{self, RunOutcome, RunOutput, RunSpec};

// ============================================================================
// The executor trait
// ============================================================================

/// The result of `safe_run(argv, capture_output=True)`: `ok` = return code 0.
/// `None` from [`AppDefaultsIo::run`] — the command did not start or did not fit
/// into the timeout (the same safe default as `safe_run` → `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOut {
    pub ok: bool,
    pub stdout: String,
}

/// The directory of vendor D-Bus activation files: the FileManager1 service text is taken
/// from there and shadowed by a user copy.
pub const VENDOR_DBUS_SERVICES: &str = "/usr/share/dbus-1/services";

/// Directories where `.desktop` is searched — port of `_DESKTOP_DIRS` (order matters:
/// the user layer shadows the system one, flatpak comes last).
const DESKTOP_DIRS: &[&str] = &[
    "~/.local/share/applications",
    "/usr/share/applications",
    "/usr/local/share/applications",
    "/var/lib/flatpak/exports/share/applications",
    "~/.local/share/flatpak/exports/share/applications",
];

/// Side actions and observations of the route — exactly the ones Python made.
///
/// The methods take `&self`, not `&mut self` (unlike the neighboring
/// [`ComplianceIo`]), deliberately: the deciding layer [`crate::appdefaults`]
/// takes observations as `&dyn Fn(&str) -> bool` closures, and with
/// `&mut self` each such call would run into a double borrow of the
/// executor. It costs fake executors nothing — they keep counters
/// and the log in `Cell`/`RefCell`.
pub trait AppDefaultsIo {
    /// `os.path.exists`.
    fn exists(&self, path: &str) -> bool;
    /// `safe_check_command(name)` — whether a program exists (taking synonyms into account).
    fn which(&self, name: &str) -> bool;
    /// `shutil.which(name)` — the path to a program.
    fn which_path(&self, name: &str) -> Option<String>;
    /// Reading a text file; `None` — could not read (like `except OSError`).
    fn read_text(&self, path: &str) -> Option<String>;
    /// Writing a whole text file.
    fn write_text(&self, path: &str, content: &str) -> Result<(), String>;
    /// `os.makedirs(path, exist_ok=True)`.
    fn makedirs(&self, path: &str) -> Result<(), String>;
    /// File names in a directory (a nonexistent directory — empty).
    fn list_dir(&self, dir: &str) -> Vec<String>;
    /// `safe_run(argv, capture_output=True)`.
    fn run(&self, argv: &[String]) -> Option<RunOut>;
    /// The contents of `/proc/<pid>/comm` without the newline.
    fn proc_comm(&self, pid: i64) -> Option<String>;
    /// PIDs of processes that have WINDOWS on screen. `None` — could not
    /// find out; this is NOT "no windows", and the decision is then made cautiously
    /// (see [`crate::appdefaults::fm1_verdict`]).
    fn window_pids(&self) -> Option<Vec<i64>>;
    /// `os.kill(pid, SIGTERM)`.
    fn terminate(&self, pid: i64);
    /// The path to the running Rexon binary (`/proc/self/exe`).
    fn self_exe(&self) -> Option<String>;
    /// Which product entry point is currently selected — for the live executor this is
    /// [`crate::installer::launch_entry_live`], the same rule as for the shortcut.
    fn launch_entry(&self) -> crate::installer::LaunchEntry;
    /// The autostart line in configs of compositors that do not execute `.desktop`
    /// (`crate::compositor_autostart`): install it for the record
    /// `desktop` and return the final record text (with `NotShowIn=`) and
    /// the refusal for the current session. By default — touch nothing
    /// (fake executors of the checks do not touch live configs).
    fn login_lines(&self, desktop: &str) -> (String, Option<String>) {
        (desktop.to_string(), None)
    }
}

// ============================================================================
// Launch environment
// ============================================================================

/// Environment values the route depends on. They are read ONCE at
/// entry ([`AppDefaultsEnv::from_process`]) and then travel as values: this way
/// the whole executor runs on a fake `$XDG_CONFIG_HOME` without touching
/// the live session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppDefaultsEnv {
    pub home: String,
    pub xdg_config_home: Option<String>,
    pub xdg_data_home: Option<String>,
    pub xdg_current_desktop: String,
    pub desktop_session: String,
    pub has_wayland: bool,
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl AppDefaultsEnv {
    /// The current process environment.
    pub fn from_process() -> Self {
        AppDefaultsEnv {
            home: std::env::var("HOME").unwrap_or_default(),
            xdg_config_home: env_nonempty("XDG_CONFIG_HOME"),
            xdg_data_home: env_nonempty("XDG_DATA_HOME"),
            xdg_current_desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            desktop_session: std::env::var("DESKTOP_SESSION").unwrap_or_default(),
            has_wayland: env_nonempty("WAYLAND_DISPLAY").is_some(),
        }
    }
}

/// Path joining (`os.path.join` for our cases).
fn join(a: &str, b: &str) -> String {
    format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
}

// ============================================================================
// Profile generation outcome
// ============================================================================

/// What came out of `generate_profile` — port of its `(ok, message)` pair, but with
/// a DISTINGUISHABLE skip: the original returned "skipped" and "written" as one and
/// the same `True`, and session login reported creating a file that never
/// appeared (see item 2 in the module header).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerateOutcome {
    /// The file was written; the field is the full path.
    Wrote(String),
    /// The desktop is not installed — no file needed.
    Skipped(String),
    /// Did not work: not a single application or a write error.
    Failed(String),
}

impl GenerateOutcome {
    /// The original's `(ok, message)` pair — for places where the distinction is not needed.
    pub fn ok_msg(&self) -> (bool, String) {
        match self {
            GenerateOutcome::Wrote(p) => (true, p.clone()),
            GenerateOutcome::Skipped(m) => (true, m.clone()),
            GenerateOutcome::Failed(m) => (false, m.clone()),
        }
    }
}

// ============================================================================
// DEAppSwitcher
// ============================================================================

/// Name of the defaults autostart record in `<config>/autostart`.
pub const AUTOSTART_FILE: &str = "rexon-appdefaults.desktop";

/// Port of the `DEAppSwitcher` class — switching default applications when
/// the desktop changes.
///
/// Unlike the original, nobody creates the `~/.config/rexon` directory here:
/// the Python constructor did it on every object creation, while the
/// class itself never once wrote into that directory.
pub struct DeAppSwitcher<'a, IO: AppDefaultsIo + ?Sized> {
    io: &'a IO,
    env: AppDefaultsEnv,
    cfg_dir: String,
}

impl<'a, IO: AppDefaultsIo + ?Sized> DeAppSwitcher<'a, IO> {
    pub fn new(io: &'a IO, env: AppDefaultsEnv) -> Self {
        let cfg_dir = utils::config_home(env.xdg_config_home.as_deref(), &env.home);
        DeAppSwitcher { io, env, cfg_dir }
    }

    /// Create user copies of autostart records that belong
    /// only in their own desktop ([`ad::CONFINED_AUTOSTARTS`]). The person's own
    /// (a copy with the same name) is not touched; neither is a package the person installed
    /// themselves (the `pacman -Qqd` check: whether it is installed as a dependency).
    pub fn confine_desktop_autostarts(&self, logs: &mut Vec<String>) {
        const SYSTEM_DIR: &str = "/etc/xdg/autostart";
        let user_dir = join(&self.cfg_dir, "autostart");
        for entry in ad::CONFINED_AUTOSTARTS {
            let system_path = join(SYSTEM_DIR, entry.file);
            let Some(system_text) = self.io.read_text(&system_path) else {
                continue;
            };
            let user_path = join(&user_dir, entry.file);
            let user_text = self.io.read_text(&user_path);
            if !ad::needs_confined_autostart(&system_text, user_text.as_deref()) {
                continue;
            }
            if let Some(pkg) = entry.only_if_dependency {
                let argv: Vec<String> = vec!["pacman".into(), "-Qqd".into(), pkg.into()];
                if !self.io.which("pacman") || !self.io.run(&argv).is_some_and(|r| r.ok) {
                    continue;
                }
            }
            if let Err(e) = self.io.makedirs(&user_dir) {
                logs.push(format!("⚠️ {e}"));
                continue;
            }
            let text = ad::confined_autostart_text(&system_text, entry.only_show_in);
            match self.io.write_text(&user_path, &text) {
                // TRANSLATORS: %1 is an autostart file name, %2 a list of desktop names.
                Ok(()) => logs.push(trf(
                    "🔒 Autostart %1 now runs only in %2: in other desktops it took over the keyboard layouts",
                    &[&entry.file, &entry.only_show_in.trim_end_matches(';').replace(';', ", ")],
                )),
                Err(e) => logs.push(format!("⚠️ {e}")),
            }
        }
    }

    /// The directory of the session's user configs (`$XDG_CONFIG_HOME`).
    pub fn config_dir(&self) -> &str {
        &self.cfg_dir
    }

    fn expand(&self, path: &str) -> String {
        utils::expand_user_xdg(
            path,
            &self.env.home,
            self.env.xdg_config_home.as_deref(),
            self.env.xdg_data_home.as_deref(),
        )
    }

    /// `_desktop_exists` — whether the `.desktop` is in at least one of the directories.
    pub fn desktop_exists(&self, desktop_id: &str) -> bool {
        DESKTOP_DIRS
            .iter()
            .any(|d| self.io.exists(&join(&self.expand(d), desktop_id)))
    }

    // ── 1. Generating the per-DE mimeapps.list ──────────────────────────

    /// Port of `generate_profile`: the core builds the outcome, here — the write.
    pub fn generate_profile(&self, de_id: &str) -> GenerateOutcome {
        let outcome = ad::generate_profile(de_id, &|p| self.desktop_exists(p));
        match outcome {
            ProfileOutcome::Skip { message } => GenerateOutcome::Skipped(message),
            ProfileOutcome::NoApps { message } => GenerateOutcome::Failed(message),
            ProfileOutcome::Write { filename, content, .. } => {
                let filepath = join(&self.cfg_dir, &filename);
                match self.io.write_text(&filepath, &content) {
                    Ok(()) => GenerateOutcome::Wrote(filepath),
                    Err(e) => {
                        GenerateOutcome::Failed(trf("Couldn't write %1: %2", &[&filepath, &e]))
                    }
                }
            }
        }
    }

    /// Port of `generate_all_profiles`: `{de_id: message}` + log lines.
    pub fn generate_all_profiles(
        &self,
        installed_des: Option<&[String]>,
        logs: &mut Vec<String>,
    ) -> Vec<(String, String)> {
        let targets: Vec<String> = match installed_des {
            Some(list) => list.to_vec(),
            None => self.installed_des(),
        };
        let mut results = Vec::new();
        for de_id in targets {
            let (ok, msg) = self.generate_profile(&de_id).ok_msg();
            let status = if ok { "✅" } else { "⚠️" };
            logs.push(format!("  {status} {de_id}: {msg}"));
            results.push((de_id, msg));
        }
        results
    }

    // ── 2. DE-specific settings (terminal and so on) ─────────────────────

    /// The desktop's own terminal list (`<desktop>-xdg-terminals.list`): it
    /// overrides the shared one for `xdg-terminal-exec`, so a desktop with a native
    /// terminal gets it here (the decision is
    /// [`ad::native_terminals_list_plan`]). The choice the person made for
    /// this desktop in Rexon is not touched and is returned: it takes priority over the native
    /// terminal in all the desktop's other places too.
    pub fn ensure_terminals_list(
        &self,
        de_id: &str,
        desktop: &str,
        logs: &mut Vec<String>,
    ) -> Option<String> {
        let path_of = |name: &str| join(&self.cfg_dir, name);
        let existing = ad::terminals_list_filename(desktop)
            .and_then(|name| self.io.read_text(&path_of(&name)));
        match ad::native_terminals_list_plan(de_id, desktop, existing.as_deref(), &|p| {
            self.desktop_exists(p)
        }) {
            ad::TerminalsListPlan::Write { filename, terminal, content } => {
                let path = path_of(&filename);
                match self.io.write_text(&path, &content) {
                    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
                    Ok(()) if existing.is_some() => logs.push(trf(
                        "✅ Fixed: terminal → %1",
                        &[&crate::defaults::terminal_exec_name(&terminal)],
                    )),
                    Ok(()) => logs.push(trf("📝 Created %1", &[&filename])),
                    Err(e) => logs.push(format!("⚠️ {}", trf("Couldn't write %1: %2", &[&path, &e]))),
                }
                None
            }
            ad::TerminalsListPlan::KeepChosen { terminal, .. } => {
                logs.push(trf("✅ Terminal: %1", &[&crate::defaults::terminal_exec_name(&terminal)]));
                Some(terminal)
            }
            ad::TerminalsListPlan::UpToDate { filename } => {
                logs.push(trf("✅ %1 already exists", &[&filename]));
                None
            }
            ad::TerminalsListPlan::Nothing => None,
        }
    }

    /// Port of `apply_de_specific`: the core builds the plan (argv + the
    /// `helpers.rc` text + logs), here — running commands and writing.
    pub fn apply_de_specific(&self, de_id: &str) -> Vec<String> {
        self.apply_de_specific_with_terminal(de_id, None)
    }

    /// [`Self::apply_de_specific`] with the terminal the person chose for
    /// this desktop (see [`ad::apply_de_specific_with_terminal`]).
    pub fn apply_de_specific_with_terminal(
        &self,
        de_id: &str,
        chosen_terminal: Option<&str>,
    ) -> Vec<String> {
        // XFCE is read with THE SAME path expansion it is written with below: otherwise
        // with a non-standard `$XDG_CONFIG_HOME` new lines would be mixed into the old file
        // from `~/.config`, and the result would land in another
        // directory — the setting would silently not arrive.
        let helpers_path = self.expand("~/.config/xfce4/helpers.rc");
        let xfce_existing = if de_id == "xfce" && self.io.exists(&helpers_path) {
            self.io.read_text(&helpers_path)
        } else {
            None
        };

        let plan = ad::apply_de_specific_with_terminal(
            de_id,
            chosen_terminal,
            &|p| self.desktop_exists(p),
            &|c| self.io.which(c),
            xfce_existing.as_deref(),
            &crate::defaults::xfce_system_helper_exists,
        );
        for cmd in &plan.commands {
            self.io.run(cmd);
        }
        let mut logs = plan.logs;
        if let Some(text) = plan.helpers_rc.as_deref() {
            let dir = self.expand("~/.config/xfce4");
            let written = self
                .io
                .makedirs(&dir)
                .and_then(|()| self.io.write_text(&helpers_path, text));
            if let Err(e) = written {
                // The original hid this in the debug log and returned the logs
                // as if the setting had been applied (rule #13).
                logs.push(trf("⚠️ XFCE: couldn't write %1: %2", &[&helpers_path, &e]));
            }
        }
        // In-place edits of foreign configs (LXQt `lxqt-qtxdg.conf`): the same
        // steps of the shared terminal table as in the "Settings" tab.
        for step in &plan.file_steps {
            // Our own exo helper (XFCE, a chosen terminal without a system
            // helper): the directory and the file — with the same path expansion.
            match step {
                crate::defaults::TermStep::MakeDirs { path } => {
                    let path = self.expand(path);
                    if let Err(e) = self.io.makedirs(&path) {
                        logs.push(format!("⚠️ {}", trf("Couldn't write %1: %2", &[&path, &e])));
                    }
                    continue;
                }
                crate::defaults::TermStep::WriteFile { path, content } => {
                    let path = self.expand(path);
                    if let Err(e) = self.io.write_text(&path, content) {
                        logs.push(format!("⚠️ {}", trf("Couldn't write %1: %2", &[&path, &e])));
                    }
                    continue;
                }
                _ => {}
            }
            if let crate::defaults::TermStep::SetIni { path, section, key, value } = step {
                let path = self.expand(path);
                let existing = self.io.read_text(&path);
                let text = crate::defaults::set_ini_value(existing.as_deref(), section, key, value);
                let dir = Path::new(&path).parent().map(|d| d.to_string_lossy().into_owned());
                let written = match dir {
                    Some(d) => self.io.makedirs(&d),
                    None => Ok(()),
                }
                .and_then(|()| self.io.write_text(&path, &text));
                if let Err(e) = written {
                    logs.push(format!("⚠️ {}", trf("Couldn't write %1: %2", &[&path, &e])));
                }
            }
        }
        logs
    }

    // ── 3. Full application for the current desktop ──────────────────────

    /// Port of `apply_for_current_de` — what is called at session login.
    pub fn apply_for_current_de(&self) -> (bool, Vec<String>) {
        let Some(de_id) = ad::detect_current_de(
            &self.env.xdg_current_desktop,
            &self.env.desktop_session,
            self.env.has_wayland,
        ) else {
            return (false, vec![tr("⚠️ Couldn't detect the current desktop")]);
        };

        // TRANSLATORS: %1 is a desktop environment id (kde, gnome…).
        let mut logs = vec![trf("🖥️ Current desktop: %1", &[&de_id])];

        // 1. Make sure the per-DE mimeapps.list exists. EVERY desktop has a name
        //    (an unknown one gives `<id>-mimeapps.list`), so unlike the original
        //    there is never a "no name" branch here.
        let filename = ad::resolve_mimeapps_filename(&de_id);
        let filepath = join(&self.cfg_dir, &filename);
        if self.io.exists(&filepath) {
            logs.push(trf("✅ %1 already exists", &[&filename]));
        } else {
            match self.generate_profile(&de_id) {
                GenerateOutcome::Wrote(_) => logs.push(trf("📝 Created %1", &[&filename])),
                GenerateOutcome::Skipped(m) => logs.push(format!("ℹ️ {m}")),
                GenerateOutcome::Failed(m) => logs.push(format!("⚠️ {m}")),
            }
        }

        // 2. The desktop's own terminal list: `xdg-terminal-exec` reads it
        //    before the shared one (see `ad::native_terminals_list_plan`).
        let desktop = self.env.xdg_current_desktop.clone();
        let chosen = self.ensure_terminals_list(&de_id, &desktop, &mut logs);

        // 3. DE-specific settings — with the person's choice, if there is one.
        logs.extend(self.apply_de_specific_with_terminal(&de_id, chosen.as_deref()));

        // 4. The "Show in folder" slot. It comes AFTER mimeapps deliberately: the question
        //    is not decided there, and the order makes this visible in the log —
        //    first "defaults written", then "and the name on the bus is held by this one".
        logs.extend(self.ensure_filemanager1(&de_id));

        // 5. Other desktops' foreign autostart — only in their own desktop
        //    (`ad::CONFINED_AUTOSTARTS`): otherwise, for example, fcitx5 from Deepin
        //    hijacks keyboard layouts in MATE.
        self.confine_desktop_autostarts(&mut logs);

        // 6. Our own autostart record from earlier versions — without the
        //    gnome-session phase (`ad::heal_autostart_text`).
        self.heal_autostart();

        (true, logs)
    }

    /// Remove from the existing defaults autostart record the phase line
    /// that gnome-session 49+ does not execute. If there is no record, nothing is
    /// created: autostart turned off by the person stays off.
    /// Silently: a line in the login log would tell the person nothing.
    pub fn heal_autostart(&self) -> bool {
        let path = join(&join(&self.cfg_dir, "autostart"), AUTOSTART_FILE);
        let Some(text) = self.io.read_text(&path) else {
            return false;
        };
        match ad::heal_autostart_text(&text) {
            Some(fixed) => self.io.write_text(&path, &fixed).is_ok(),
            None => false,
        }
    }

    // ── 4. When switching the desktop through Rexon ──────────────────────

    /// Port of `on_switch_de`: the target desktop's profile + its settings +
    /// autostart.
    pub fn on_switch_de(&self, target_de: &str, logs: &mut Vec<String>) -> bool {
        logs.push(trf("🔄 Switching default apps to %1…", &[&target_de]));

        let (ok, msg) = self.generate_profile(target_de).ok_msg();
        let status = if ok { "✅" } else { "⚠️" };
        logs.push(format!("  {status} {msg}"));

        // The target desktop's terminal list — by the Rexon identifier
        // (`ad::terminals_list_filename` knows it too).
        let mut term_logs = Vec::new();
        let chosen = self.ensure_terminals_list(target_de, target_de, &mut term_logs);
        term_logs.extend(self.apply_de_specific_with_terminal(target_de, chosen.as_deref()));
        for line in term_logs {
            logs.push(format!("  {line}"));
        }
        // The "Show in folder" slot is part of the same defaults (rule #12: one
        // place decides, not "fixed at login, but not on switching").
        for line in self.ensure_filemanager1(target_de) {
            logs.push(format!("  {line}"));
        }

        let (_, notes) = self.install_autostart();
        logs.extend(notes);
        ok
    }

    // ── 5. Autostart ─────────────────────────────────────────────────────

    /// Port of `install_autostart`: a `.desktop` in the session autostart.
    ///
    /// What to launch with is decided by the product entry point ([`ad::autostart_exec_for`]):
    /// the Rust entry — the window binary; the Python entry (rollback) — `rexon` from `PATH`,
    /// otherwise the running binary ITSELF. There is no `/usr/local/lib` path here
    /// at all — see item 1 in the module header.
    pub fn install_autostart(&self) -> (bool, Vec<String>) {
        let mut notes = Vec::new();
        let entry = self.io.launch_entry();
        let exec_path = match entry {
            crate::installer::LaunchEntry::MainWindow(_) => ad::autostart_exec_for(&entry, None, None),
            crate::installer::LaunchEntry::Python => ad::autostart_exec_for(
                &entry,
                self.io.which_path(crate::installer::PYTHON_ENTRY_CMD).as_deref(),
                self.io.self_exe().as_deref(),
            ),
        };
        let Some(exec_path) = exec_path else {
            notes.push(tr(
                "⚠️ Default apps autostart not set up: couldn't find how to launch Rexon \
                 (no `rexon` in PATH and its own binary path is unknown)",
            ));
            return (false, notes);
        };
        let autostart_dir = join(&self.cfg_dir, "autostart");
        let autostart_path = join(&autostart_dir, AUTOSTART_FILE);
        let content = ad::autostart_desktop_content(&exec_path);
        let (content, refusal) = self.io.login_lines(&content);
        let written = self
            .io
            .makedirs(&autostart_dir)
            .and_then(|()| self.io.write_text(&autostart_path, &content));
        match written {
            Ok(()) => match refusal {
                // The record was written, but it will not be executed in this session.
                Some(msg) => {
                    notes.push(format!("⚠️ {msg}"));
                    (false, notes)
                }
                None => {
                    notes.push(trf("✅ Autostart set up: %1", &[&autostart_path]));
                    (true, notes)
                }
            },
            Err(e) => {
                notes.push(trf("⚠️ Autostart not set up: %1: %2", &[&autostart_path, &e]));
                (false, notes)
            }
        }
    }

    // ── 6. Information ───────────────────────────────────────────────────

    /// Port of `get_de_apps_info` — `[(category, .desktop | None)]`.
    pub fn de_apps_info(&self, de_id: &str) -> Vec<(String, Option<String>)> {
        ad::de_apps_info(de_id, &|p| self.desktop_exists(p))
    }

    /// Port of `get_installed_des`.
    pub fn installed_des(&self) -> Vec<String> {
        ad::installed_des(&|p| self.desktop_exists(p))
    }

    // ── 7. Default file manager ──────────────────────────────────────────

    /// The directory of user D-Bus activation files.
    fn user_dbus_services(&self) -> String {
        self.expand("~/.local/share/dbus-1/services")
    }

    /// Port of `_override_filemanager1`: shadow the D-Bus activation of
    /// `org.freedesktop.FileManager1` with a user copy.
    ///
    /// "Show in folder" goes NOT through mimeapps but through this name on the bus;
    /// on Arch the system file is owned by the nautilus package, and without an override
    /// GNOME Files intercepts such calls on any desktop.
    ///
    /// ⚠️ An override alone is NOT ENOUGH: it decides whom to START by name, and has no
    /// effect at all while a live process holds the name. The second half
    /// is handled by [`Self::reclaim_filemanager1`].
    pub fn override_filemanager1(&self, binary: &str, notes: &mut Vec<String>) -> bool {
        let mut vendor_txt: Option<String> = None;
        for name in self.io.list_dir(VENDOR_DBUS_SERVICES) {
            if !name.ends_with(".service") {
                continue;
            }
            let Some(txt) = self.io.read_text(&join(VENDOR_DBUS_SERVICES, &name)) else {
                continue;
            };
            if ad::fm1_matches(&txt, binary) {
                vendor_txt = Some(txt);
                break;
            }
        }
        let Some(vendor_txt) = vendor_txt else {
            notes.push(trf(
                "FileManager1: %1 has no vendor service file — not installing an override",
                &[&binary],
            ));
            return false;
        };

        let dir = self.user_dbus_services();
        let target = join(&dir, &format!("{}.service", ad::FM1_NAME));
        if let Err(e) = self
            .io
            .makedirs(&dir)
            .and_then(|()| self.io.write_text(&target, &vendor_txt))
        {
            notes.push(trf("FileManager1: couldn't write %1: %2", &[&target, &e]));
            return false;
        }

        self.io.run(&ad::busctl_reload_cmd());
        true
    }

    /// Who holds the name `org.freedesktop.FileManager1` RIGHT NOW — and whether it
    /// has windows.
    ///
    /// One question to the bus (`busctl status`) returns everything needed at once: `PID`,
    /// `Comm`, `Exe` and `CommandLine`. The old parse took ONE PID from there and
    /// fetched the name from `/proc/<pid>/comm` — those reads remain as a fallback for
    /// the terse output of an old bus.
    fn fm1_owner(&self) -> Option<ad::Fm1Owner> {
        let res = self.io.run(&ad::busctl_status_cmd(ad::FM1_NAME))?;
        if !res.ok {
            return None;
        }
        let mut facts = ad::busctl_owner(&res.stdout)?;
        if facts.comm.is_empty() && facts.exe.is_empty() {
            facts.comm = self.io.proc_comm(facts.pid).unwrap_or_default();
        }
        let has_windows = self.io.window_pids().map(|pids| pids.contains(&facts.pid));
        Some(ad::Fm1Owner { facts, has_windows })
    }

    /// Remove the manager from the saved KDE session so that at the next login
    /// it does not come up as a daemon again and take the name back.
    ///
    /// No file (not KDE) — silently do nothing: the decision about the contents
    /// is made by the core ([`ad::ksmserver_forget`]), here only reading and
    /// writing.
    fn forget_in_kde_session(&self, short: &str, notes: &mut Vec<String>) {
        let path = join(&self.cfg_dir, ad::KSMSERVERRC_NAME);
        let Some(text) = self.io.read_text(&path) else {
            return;
        };
        let Some(updated) = ad::ksmserver_forget(&text, short) else {
            return;
        };
        match self.io.write_text(&path, &updated) {
            Ok(()) => notes.push(format!(
                "  {}",
                trf("✅ %1 removed from the saved KDE session — it won't come back at the next login", &[&short])
            )),
            Err(e) => notes.push(format!("  {}", trf("⚠️ couldn't rewrite %1: %2", &[&path, &e]))),
        }
    }

    /// Say out loud who got the name after the fix.
    fn confirm_filemanager1(&self, desired_short: &str, notes: &mut Vec<String>) {
        match ad::fm1_verdict(self.fm1_owner().as_ref(), desired_short) {
            ad::Fm1Verdict::Free => notes.push(format!(
                "  {}",
                trf("✅ the name is free — “Show in Folder” will open %1", &[&desired_short])
            )),
            ad::Fm1Verdict::Owned { short, .. } => {
                notes.push(format!("  {}", trf("✅ the name passed to %1", &[&short])))
            }
            ad::Fm1Verdict::Busy { short, pid }
            | ad::Fm1Verdict::Evict { short, pid, .. } => notes.push(format!(
                "  {}",
                trf(
                    "⚠️ %1 (pid %2) still holds the name: until you log out, “Show in Folder” will open it",
                    &[&short, &pid]
                )
            )),
            ad::Fm1Verdict::Unknown { process, pid } => notes.push(format!(
                "  {}",
                trf("⚠️ %1 (pid %2) holds the name", &[&process, &pid])
            )),
        }
    }

    /// Return the `org.freedesktop.FileManager1` slot to the assigned manager.
    ///
    /// ⛔ ONLY a foreign daemon is killed, and ONLY the process whose PID the
    /// bus named: no searching by name, no `pkill`. An owner with windows is not touched
    /// at all — the person has open work there.
    ///
    /// Returns `true` if the slot had to be taken back.
    pub fn reclaim_filemanager1(
        &self,
        desired_short: &str,
        notes: &mut Vec<String>,
    ) -> bool {
        match ad::fm1_verdict(self.fm1_owner().as_ref(), desired_short) {
            ad::Fm1Verdict::Free => {
                notes.push(tr(
                    "✅ “Show in Folder”: the name org.freedesktop.FileManager1 is free — the chosen file manager will get it",
                ));
                false
            }
            ad::Fm1Verdict::Owned { short, .. } => {
                notes.push(trf(
                    "✅ “Show in Folder”: %1 holds the name — it's the chosen default",
                    &[&short],
                ));
                false
            }
            ad::Fm1Verdict::Unknown { process, pid } => {
                notes.push(trf(
                    "ℹ️ “Show in Folder”: %1 (pid %2) holds the name — it isn't a file manager, leaving it alone",
                    &[&process, &pid],
                ));
                false
            }
            ad::Fm1Verdict::Busy { short, pid } => {
                crate::log_info!("appdefaults", "FileManager1 держит {short} (pid {pid}) с открытыми окнами — не гасим");
                notes.push(trf(
                    // TRANSLATORS: %1 is a file manager (caja, thunar…). Rexon didn't close it because it has open windows.
                    "⚠️ “Show in Folder” is opened by %1 right now. Close its windows and run this again, or log out and back in",
                    &[&short],
                ));
                false
            }
            ad::Fm1Verdict::Evict { short, pid, quit } => {
                notes.push(trf(
                    "⚠️ %1 (pid %2, service mode) took over “Show in Folder” instead of %3 — taking it back",
                    &[&short, &pid, &desired_short],
                ));
                let mut stopped = false;
                if !quit.is_empty() {
                    stopped = self.io.run(&quit).map(|r| r.ok).unwrap_or(false);
                    if stopped {
                        notes.push(format!(
                            "  {}",
                            trf("✅ %1 quit normally: %2", &[&short, &quit.join(" ")])
                        ));
                    } else {
                        notes.push(format!(
                            "  {}",
                            trf("⚠️ normal quit didn't work (%1), stopping the process itself", &[&quit.join(" ")])
                        ));
                    }
                }
                if !stopped {
                    self.io.terminate(pid);
                    notes.push(format!(
                        "  {}",
                        trf("✅ %1 (pid %2) was sent the stop signal", &[&short, &pid])
                    ));
                }
                self.forget_in_kde_session(&short, notes);
                self.io.run(&ad::busctl_reload_cmd());
                self.confirm_filemanager1(desired_short, notes);
                true
            }
        }
    }

    /// Short name of the file manager Rexon considers the default
    /// for `inode/directory`: first what is actually written in mime, otherwise
    /// the desktop's native manager.
    pub fn desired_file_manager(&self, de_id: &str) -> String {
        if let Some(res) = self.io.run(&ad::xdg_mime_query_cmd()) {
            if res.ok {
                let desktop = res.stdout.trim().lines().next().unwrap_or("").trim();
                if desktop.ends_with(".desktop") {
                    return ad::fm_short_name(desktop, desktop);
                }
            }
        }
        ad::default_file_manager_for_de(Some(de_id), &|c| self.io.which(c))
    }

    /// The full "Show in folder" check for a desktop: our D-Bus activation
    /// override is in place, and the name on the bus belongs to the manager
    /// assigned as the default.
    ///
    /// Called at every session login: correct `mimeapps.list` is NOT ENOUGH for this
    /// button (why — see the section about the FileManager1 slot in
    /// [`crate::appdefaults`]).
    pub fn ensure_filemanager1(&self, de_id: &str) -> Vec<String> {
        let mut notes = Vec::new();
        let desired = self.desired_file_manager(de_id);
        self.override_filemanager1(&desired, &mut notes);
        self.reclaim_filemanager1(&desired, &mut notes);
        notes
    }

    /// Port of `set_file_manager_default`: `fm_id` — a short name from the settings
    /// ("dolphin", "nemo") or a ready `.desktop` id.
    pub fn set_file_manager_default(
        &self,
        fm_id: &str,
        de: Option<&str>,
    ) -> (bool, Vec<String>) {
        let mut notes = Vec::new();
        let Some(desktop) = ad::fm_default_target(fm_id, &|p| self.desktop_exists(p))
        else {
            notes.push(trf(
                "⚠️ File manager %1 isn't installed: its .desktop file wasn't found",
                &[&fm_id],
            ));
            return (false, notes);
        };
        let res = self.io.run(&ad::xdg_mime_default_cmd(&desktop));
        if !res.map(|r| r.ok).unwrap_or(false) {
            notes.push(trf("⚠️ xdg-mime default %1 failed", &[&desktop]));
            return (false, notes);
        }
        let short = ad::fm_short_name(fm_id, &desktop);
        self.override_filemanager1(&short, &mut notes);
        self.reclaim_filemanager1(&short, &mut notes);

        // Check: what was actually written.
        let chk = self.io.run(&ad::xdg_mime_query_cmd());
        let applied = chk
            .map(|r| r.ok && r.stdout.contains(&desktop))
            .unwrap_or(false);
        notes.push(format!(
            "set_file_manager_default({fm_id}, de={}) → {desktop}, applied={applied}",
            de.unwrap_or("—")
        ));
        (applied, notes)
    }
}

// ============================================================================
// The whole `--apply-defaults` route
// ============================================================================

/// What the route needs to know about the machine for the completeness check. Obtained
/// from outside ([`ComplianceEnv::from_live_machine`]), so that checks can run
/// the route on fake values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplianceEnv {
    pub pkg_family: PkgFamily,
    pub pkg_helper_available: bool,
    pub stage_helper_available: bool,
}

impl ComplianceEnv {
    pub fn from_live_machine() -> Self {
        Self::probe(
            DistroDetector::detect_live().family_live(),
            utils::which("pkexec", path_env().as_deref()).is_some(),
            &system_plan::is_exec_file,
            &|p| Path::new(p).exists(),
        )
    }

    /// The same diagnostics on supplied probes: `is_exec` picks the helper
    /// layout (a `/usr/lib` package or a manual `/usr/local/lib` install),
    /// `exists` answers about files. The checks run BOTH layouts without touching
    /// the live machine.
    pub fn probe(
        pkg_family: PkgFamily,
        has_pkexec: bool,
        is_exec: &dyn Fn(&str) -> bool,
        exists: &dyn Fn(&str) -> bool,
    ) -> Self {
        ComplianceEnv {
            pkg_family,
            pkg_helper_available: helper_available(
                system_plan::PKG_HELPER_NAME,
                system_plan::PKG_POLICY_PATH,
                has_pkexec,
                is_exec,
                exists,
            ),
            stage_helper_available: helper_available(
                system_plan::STAGE_HELPER_NAME,
                system_plan::STAGE_POLICY_PATH,
                has_pkexec,
                is_exec,
                exists,
            ),
        }
    }
}

/// Whether the privileged helper and its polkit policy are deployed — port of
/// `system._helper_available` (one question for all Rexon helpers).
///
/// ⚠️ The helper is asked for BY NAME, and the path is chosen by a probe
/// ([`system_plan::pick_helper_path`]): there are two layouts — a distribution package
/// puts helpers in `/usr/lib`, a manual install in `/usr/local/lib`.
/// A hard-coded `/usr/local/lib` constant made the diagnostics of the
/// `--apply-defaults` route say "no helpers" on a machine with the package, where
/// they exist. The policy is the same for both layouts — `/usr/share/polkit-1`.
///
/// Probes come from outside: the decision stays checkable on both layouts.
fn helper_available(
    name: &str,
    policy_path: &str,
    has_pkexec: bool,
    is_exec: &dyn Fn(&str) -> bool,
    exists: &dyn Fn(&str) -> bool,
) -> bool {
    has_pkexec && exists(&system_plan::pick_helper_path(name, is_exec)) && exists(policy_path)
}

/// The whole `--apply-defaults` route — port of the `rexon/app.py:625` branch.
///
/// The step order is the same as in the original, and it matters: first the
/// defaults are applied (their result decides the return code), then comes the completeness
/// DIAGNOSTICS. The check is called with `allow_privileged = false`: at
/// login Rexon does not ask for a password and does not go online — what is missing
/// is installed by the person with a button in the settings.
///
/// `out` — the line sink (in the product `println!`): lines go in exactly the
/// order the original printed them.
pub fn apply_defaults_route<IO, C>(
    io: &IO,
    env: &AppDefaultsEnv,
    compliance_io: &mut C,
    compliance_env: &ComplianceEnv,
    out: &mut dyn FnMut(&str),
) -> i32
where
    IO: AppDefaultsIo + ?Sized,
    C: ComplianceIo,
{
    let switcher = DeAppSwitcher::new(io, env.clone());
    let (ok, logs) = switcher.apply_for_current_de();
    for line in &logs {
        out(line);
    }

    let mut check_logs: Vec<String> = Vec::new();
    {
        let mut check = comp::Compliance::with_xdg(
            compliance_io,
            &env.home,
            env.xdg_config_home.as_deref(),
            compliance_env.pkg_family,
            false, // login: diagnostics only
            compliance_env.pkg_helper_available,
            compliance_env.stage_helper_available,
        );
        check.run_for_current_de(&env.xdg_current_desktop, &mut check_logs);
    }
    for line in &check_logs {
        out(line);
    }

    if ok {
        0
    } else {
        1
    }
}

/// The same route on the LIVE machine: real executors, its own environment.
pub fn apply_defaults_route_live(out: &mut dyn FnMut(&str)) -> i32 {
    let io = LiveAppDefaultsIo;
    let env = AppDefaultsEnv::from_process();
    let mut cio = LiveComplianceIo;
    let cenv = ComplianceEnv::from_live_machine();
    apply_defaults_route(&io, &env, &mut cio, &cenv, out)
}

// ============================================================================
// Live executors
// ============================================================================

fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// `safe_run(argv, capture_output=True)` in Rust: the core's single launch point.
/// `None` — the command did not start or did not fit into the timeout.
fn live_run(argv: &[String], timeout_secs: Option<u64>) -> Option<(bool, String)> {
    if argv.is_empty() {
        return None;
    }
    let spec = RunSpec {
        argv: argv.to_vec(),
        output: RunOutput::Capture,
        timeout_secs,
        ..RunSpec::default()
    };
    let report = utils::run(&spec, path_env().as_deref());
    match report.outcome {
        RunOutcome::Ok | RunOutcome::Failed => Some((
            report.returncode == Some(0),
            utils::decode_text(&report.stdout),
        )),
        RunOutcome::Timeout | RunOutcome::Error => None,
    }
}

/// File text with invalid bytes replaced; `None` — could not read.
fn read_text_lossy(path: &str) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

/// Writing a whole file through the core's shared door, creating the directory.
///
/// The directory is created here, not by the caller: the original in
/// `generate_profile` wrote to `~/.config` assuming the directory exists, and
/// on a fresh `$XDG_CONFIG_HOME` (our own case on the test bench) the write failed.
///
/// `mimeapps.list`, `kdeglobals` and the autostart line go here — the buyer's
/// files, not ours. Half of a `mimeapps.list` means links, pictures and
/// documents that stopped opening; the reference's mercy of "broken reads as
/// empty" does not apply here.
fn write_text_live(path: &str, content: &str) -> Result<(), String> {
    if let Some(dir) = Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::utils::atomic_write_str_err(path, content)
}

/// The real route executor: the file system, subprocesses, signals.
pub struct LiveAppDefaultsIo;

impl AppDefaultsIo for LiveAppDefaultsIo {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn which(&self, name: &str) -> bool {
        // Port of `safe_check_command`: for names with synonyms (`kwriteconfig6` →
        // `kwriteconfig5`) the answer covers ALL variants, otherwise the check and
        // the subsequent launch diverge.
        let path = path_env();
        if utils::tool_has_variants(name) {
            return utils::tool_argv(name, &[], path.as_deref()).is_some();
        }
        utils::lookup_tool(&[name], &[], path.as_deref()).is_some()
    }

    fn which_path(&self, name: &str) -> Option<String> {
        utils::which(name, path_env().as_deref())
    }

    fn read_text(&self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        write_text_live(path, content)
    }

    fn makedirs(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        // Names are sorted: Python's `glob` returns the file system's
        // order, and "the first matching vendor file" would otherwise depend on the
        // machine (the same argument as in the neighboring executors).
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn run(&self, argv: &[String]) -> Option<RunOut> {
        live_run(argv, None).map(|(ok, stdout)| RunOut { ok, stdout })
    }

    fn proc_comm(&self, pid: i64) -> Option<String> {
        read_text_lossy(&format!("/proc/{pid}/comm")).map(|s| s.trim().to_string())
    }

    fn terminate(&self, pid: i64) {
        if pid <= 0 {
            return; // 0 and negative ones are a process GROUP, not our case
        }
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    fn window_pids(&self) -> Option<Vec<i64>> {
        // Only X11 can list windows TOGETHER with PIDs (`wmctrl -lp`
        // reads `_NET_CLIENT_LIST` + `_NET_WM_PID`). Wayland has no such
        // protocol at all, and lying "no windows" is not allowed — it would lead to
        // killing a manager with the person's work open. We answer an honest
        // "don't know", and the decision is made by the owner's command line.
        if env_nonempty("WAYLAND_DISPLAY").is_some() || env_nonempty("DISPLAY").is_none() {
            return None;
        }
        let argv = vec!["wmctrl".to_string(), "-lp".to_string()];
        let (ok, stdout) = live_run(&argv, Some(5))?;
        if !ok {
            return None;
        }
        // wmctrl -lp columns: id, desktop, PID, host, title.
        Some(
            stdout
                .lines()
                .filter_map(|l| l.split_whitespace().nth(2)?.parse::<i64>().ok())
                .collect(),
        )
    }

    fn self_exe(&self) -> Option<String> {
        std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    fn launch_entry(&self) -> crate::installer::LaunchEntry {
        crate::installer::launch_entry_live()
    }

    fn login_lines(&self, desktop: &str) -> (String, Option<String>) {
        use crate::compositor_autostart as ca;
        ca::mirror_desktop_live(ca::ID_APPDEFAULTS, desktop)
    }
}

/// The real executor of the completeness check — port of
/// `_LiveComplianceIo` from `rexon/compliance.py`.
///
/// Each method is exactly one side call of the original; on error the same
/// safe default is returned (`safe_run` → `None`, `OSError` → `None`/`False`).
/// The call order is set by the core ([`comp::Compliance`]).
pub struct LiveComplianceIo;

/// A unique name in the temporary directory (`tempfile` without the tempfile crate).
fn tmp_name(prefix: &str, suffix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}{}_{stamp}_{n}{suffix}", std::process::id())
}

impl ComplianceIo for LiveComplianceIo {
    fn run(&mut self, argv: &[String], timeout: Option<u64>) -> Option<RunResult> {
        live_run(argv, timeout).map(|(ok, stdout)| RunResult { ok, stdout })
    }

    fn which(&mut self, name: &str) -> bool {
        utils::which(name, path_env().as_deref()).is_some()
    }

    fn env(&mut self, name: &str) -> String {
        std::env::var(name).unwrap_or_default()
    }

    fn exists(&mut self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn is_file(&mut self, path: &str) -> bool {
        Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        Path::new(path).is_dir()
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        read_text_lossy(path)
    }

    fn scandir(&mut self, path: &str) -> Option<Vec<ScanEntry>> {
        let entries = std::fs::read_dir(path).ok()?;
        let mut out: Vec<ScanEntry> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                Some(ScanEntry {
                    is_dir: e.path().is_dir(),
                    name,
                })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Some(out)
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn makedirs(&mut self, path: &str) {
        let _ = std::fs::create_dir_all(path);
    }

    fn write_text(&mut self, path: &str, content: &str) {
        let _ = write_text_live(path, content);
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let _ = std::fs::rename(src, dst);
    }

    fn realpath(&mut self, path: &str) -> String {
        std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }

    fn mkdtemp(&mut self, prefix: &str, dir: Option<&str>) -> String {
        use std::os::unix::fs::PermissionsExt;
        let base = match dir {
            Some(d) if !d.is_empty() => std::path::PathBuf::from(d),
            _ => std::env::temp_dir(),
        };
        let mut last = base.join(prefix);
        for _ in 0..64 {
            last = base.join(tmp_name(prefix, ""));
            match std::fs::create_dir_all(&last) {
                Ok(()) => {
                    let _ = std::fs::set_permissions(
                        &last,
                        std::fs::Permissions::from_mode(0o700),
                    );
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn mkstemp(&mut self, prefix: &str, suffix: &str) -> String {
        use std::os::unix::fs::OpenOptionsExt;
        let base = std::env::temp_dir();
        let mut last = base.join(tmp_name(prefix, suffix));
        for _ in 0..64 {
            last = base.join(tmp_name(prefix, suffix));
            let opened = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&last);
            match opened {
                Ok(_) => break,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => break,
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn chmod(&mut self, path: &str, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }

    fn remove_file(&mut self, path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn remove_tree(&mut self, path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn copy_file(&mut self, src: &str, dst: &str) {
        let _ = std::fs::copy(src, dst);
    }
}
