//! The executing layer of `rexon/system.py` as PURE "command plans": the
//! continuation of the system.py port (the first part is `system.rs`).
//!
//! NOTHING is executed here: each function builds, from its inputs, a
//! sequence of steps (`PlanStep`): the command, arguments, whether root is
//! needed, timeout, environment (LANG=C), description. The executor (live
//! Python through the bridge for now, rexon-gui later) simply runs the plan.
//! The parity reference is the RECORDED safe_run/subprocess.run calls of live
//! Python (argv/timeout/env) on the same inputs: the plan must match 1-to-1
//! (see `system_plan_parity.rs`).
//!
//! Ported:
//!   * command tables per `PkgFamily` (11 families): `install_cmd` /
//!     `remove_cmd` / `query_cmd` / `update_db_cmd` / `pkg_manager_name`
//!     (+ RuntimeError texts for UNKNOWN, as in Python);
//!   * `LOCK_PATHS` + the `WaitPkgLock` step primitive (poll 2 s, timeout
//!     300 s: snap-pac post-hooks hold the lock after a neighbouring transaction);
//!   * `install_argv`: the ONLY answer to "which command installs these
//!     packages": the flag table plus the choice "named org.rexon.pkg path or
//!     raw pkexec" (and NixOS without root). It is asked by `install_plan`,
//!     by installing video dependencies (`wallpaper_engine`) and by compliance
//!     repair (`compliance`): the last two kept THEIR OWN tables and both had
//!     drifted from the engine;
//!   * the plans `update_db_plan` / `install_plan` / `remove_plan` (the full
//!     remove pipeline: group expansion → installed query → forbidden set →
//!     transitive closure → removal command) / `install_batch_plan` (legacy
//!     `pkexec bash <tmp_sh>` with the script and a wrapper with the
//!     fs.protected_regular chown pitfall, the rexon-de-helper path, SteamOS
//!     steamos-readonly, NixOS/immutable early exits, Gentoo overlay, the AUR
//!     tail without root);
//!   * DE orchestration: `dm_enable_step` (package/unit pairs, `--force` for
//!     Plasma Login Manager), `remove_de_decision` (2-level protection against
//!     removing the active DE, the force flag: architecture rule #13),
//!     `switch_de_guard`, `logout_cmds` (choosing logout commands by environment);
//!   * the polkit layer: generating the TEXTS of the privileged helper scripts
//!     (rexon-de-helper / rexon-pkg-helper / rexon-stage-helper), policy XML,
//!     passwordless org.rexon.* rules, `privileged_pkg_cmd` /
//!     `privileged_stage_cmd` (named actions vs raw pkexec) and
//!     `de_helper_supports`. The texts are checked against Python text for text.
//!   * package version in the repositories: `clean_pkg_version`,
//!     `valid_pkg_name`, `repo_version_query` (command + binary + LANG=C +
//!     10 s timeout), `parse_repo_version` (Version/Candidate fields by family);
//!   * an installed package: two more INDEPENDENT questions to the engine (not
//!     "is it installed" and not "what is in the repositories"):
//!     `installed_version_query` + `parse_installed_version` (the version of
//!     what is on the machine) and `package_files_query` +
//!     `package_files_output` + `package_file_paths` (the package's file list
//!     and its parsing, the only one in the whole project),
//!     `installed_packages_query` + `parse_installed_packages` (list
//!     EVERYTHING installed: a question without a package argument),
//!     `installed_meta_query` (`pacman -Qi`, raw material for field parsing)
//!     and `repo_target_exists` (whether a target is in the repositories: by
//!     the outcome of the same query as the version);
//!   * waiting for the package manager lock: `lock_wait_step`, a pure loop
//!     step (free / tell the person / wait / gave up), shared by package
//!     installs and the look rebuild;
//!   * rollback of heavy Plasma forks: `filter_plasma_forks`,
//!     `fork_stock_candidates` + `fork_stock_target` (stock from live
//!     provides/replaces, checked for existence in the repositories),
//!     `plasma_fork_split`, `plasma_fork_recovery_command`,
//!     `plasma_fork_unresolved_message`, `plasma_fork_failure_message`,
//!     `plasma_fork_reset_plan` (one pkexec transaction `-Rns` + `-S`, or a
//!     `Blocked` refusal if there is nothing to restore), `plasma_fork_preview`
//!     (what will be done, without a transaction), `plasma_fork_reset_outcome`
//!     (parsing the pkexec outcome) and `TargetExistsMemo` (probe memory);
//!   * Plymouth: `plymouth_theme_from_link`, `plymouth_theme_from_output`,
//!     `plymouth_theme_candidates` + `plymouth_installed_themes` (what counts
//!     as an installed theme), `plymouth_theme_file`, `plymouth_package_default`,
//!     `target_plymouth_theme` (distro branding → package default → fallbacks),
//!     `plymouth_reset_command`, `plymouth_reset_plan` (choosing
//!     plymouth-set-default-theme vs update-alternatives),
//!     `plymouth_reset_decision` + `plymouth_reset_outcome` and the answer texts;
//!   * SDDM: `sddm_conf_read_order`, `sddm_conf_entries`,
//!     `sddm_kde_settings_path`, `sddm_files_to_remove`,
//!     `sddm_current_theme_from`,
//!     `sddm_distro_default_theme`, `sddm_reset_plan`, `sddm_reset_command`,
//!     `sddm_reset_outcome`,
//!     message texts (`sddm_reset_success_message`,
//!     `pkexec_denied_message`);
//!   * login screen detection: unit tables (`KNOWN_DM_UNITS`: 17 DMs, DDM
//!     included since 15.11.2026, `LOGIN_MANAGER_PROBE_UNITS`,
//!     `DM_ENABLED_STATES`), `dm_unit_from_symlink`, `active_login_manager_from`,
//!     `display_manager_present_from` (`systemctl is-enabled` itself via a callback).
//!   * live install progress: `parse_pacman_line`: a pacman output line into
//!     a stage and a ready localized message (steps, hooks, downloads,
//!     results, sections, errors, "already installed" from `--needed`).
//!   * install result and progress: `batch_main_result` (explanations about a
//!     pkexec refusal and a missing polkit agent), `batch_aur_intro` /
//!     `batch_aur_result`, `install_de_fraction` (bar share by stage),
//!     `pick_aur_helper`, `pkg_lock_timeout_message`, `gentoo_overlay_for`,
//!     `extra_packages_texts`;
//!   * the login screen foolproof guard: `SDDM_PKG_FOR_FAMILY` + `ensure_dm_plan`
//!     (what to install, with what, what to enable with and what to say);
//!   * MATE post-install: "Common tasks" captions by language, the snap path
//!     map, the gschema override and both privileged scripts, package lists
//!     beyond the distribution, result texts.
//!
//! NOT ported (the boundary is execution when connected to live Python):
//!   actually running commands/pkexec and reading their output, the install
//!   log tail thread (the line parsing itself is `parse_pacman_line`), the
//!   polkit agent probe (a `gdbus` call to polkit), the trips outside of the
//!   MATE post-install (git clone, copying files, gsettings),
//!   `ensure_gentoo_overlay` (the step itself is in the plan:
//!   `EnsureGentooOverlay`), `active_login_manager`, the presence of the
//!   privileged helpers (passed as a parameter), the install_de/switch_de
//!   orchestration around Python modules (PlasmaProtector,
//!   SystemDefaultsProtector, DEAppSwitcher, RexonBridge, BackupManager).

use std::collections::HashMap;

use serde::Serialize;

use crate::ext_manager::shlex_quote;
use crate::i18n::{fill, n_, ntr, tr, trc, trf};
use crate::system::{
    de_entry_packages_for, expand_groups, qi_field, DistroDetector, PkgFamily,
};

// ============================================================================
// Plan steps
// ============================================================================

/// A command for the executor: what Python passes to `safe_run`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cmd {
    pub argv: Vec<String>,
    /// None → timeout BY COMMAND CLASS (`utils::default_timeout_secs`): query
    /// 30 s, install from repositories 30 min, build from source 4 h. We set our
    /// own number only where the class cannot be seen from argv: above all
    /// behind `pkexec bash <script>`.
    pub timeout_secs: Option<u64>,
    /// Add LANG=C, LC_ALL=C on top of the environment (parsing steps: a Russian
    /// locale breaks parsing pacman output).
    pub lang_c: bool,
}

impl Cmd {
    fn new(argv: Vec<String>, timeout_secs: Option<u64>, lang_c: bool) -> Self {
        Cmd { argv, timeout_secs, lang_c }
    }
}

/// Machine identifier of a [`PlanStep::Run`] step: by it the executor learns
/// WHAT kind of run this is, without looking at the caption for the person.
///
/// ⚠️ WHY A SEPARATE FIELD. The executor used to tell steps apart by
/// comparing with the Russian `description` string (the "install packages
/// (one pkexec)" one and its neighbours). Because of this, renaming a caption
/// (a purely cosmetic change, exactly the one the plain language rule
/// encourages) SILENTLY broke the desktop install: the main privileged step
/// lost the live progress from the log file and the result check, while the
/// plan stayed correct and the parity checks noticed nothing. The caption is
/// now only for showing to the person and can be renamed freely; branching
/// goes by `id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RunStepId {
    /// Package database update (`update_db`, and the same step inside the nix branch).
    #[serde(rename = "pkg.update_db")]
    PkgUpdateDb,
    /// A single package install (`install`, without the batch wrapper).
    #[serde(rename = "pkg.install")]
    PkgInstall,
    /// The MAIN privileged step of the batch install: `pkexec` over the wrapper
    /// script (the regular path and SteamOS) or rexon-de-helper. By it the
    /// executor shows live progress from the log file and checks the result.
    #[serde(rename = "pkg.main")]
    PkgMain,
    /// The main batch install step on Nix (no pkexec and no log file).
    #[serde(rename = "pkg.nix")]
    PkgNix,
    /// The AUR install tail via a helper (yay/paru).
    #[serde(rename = "pkg.aur")]
    PkgAur,
    /// Package removal.
    #[serde(rename = "pkg.remove")]
    PkgRemove,
    /// Probe "what is in the group" (group expansion on removal).
    #[serde(rename = "pkg.query_group")]
    PkgQueryGroup,
    /// Probe "is the package installed?".
    #[serde(rename = "pkg.query")]
    PkgQuery,
    /// Probe "who needs the package" while gathering the forbidden set.
    #[serde(rename = "pkg.required_by")]
    PkgRequiredBy,
    /// Probe "who needs the package" on a transitive closure pass.
    #[serde(rename = "pkg.closure")]
    PkgClosure,
    /// A repeated "who needs the package" probe for the note on a skipped one.
    #[serde(rename = "pkg.skip_note")]
    PkgSkipNote,
    /// Installing Rexon itself into the system (one password).
    #[serde(rename = "app.install")]
    AppInstall,
    /// Removing Rexon itself from the system.
    #[serde(rename = "app.uninstall")]
    AppUninstall,
}

impl RunStepId {
    /// String form of the identifier: the same one that goes into the plan for the executor.
    pub fn as_str(self) -> &'static str {
        match self {
            RunStepId::PkgUpdateDb => "pkg.update_db",
            RunStepId::PkgInstall => "pkg.install",
            RunStepId::PkgMain => "pkg.main",
            RunStepId::PkgNix => "pkg.nix",
            RunStepId::PkgAur => "pkg.aur",
            RunStepId::PkgRemove => "pkg.remove",
            RunStepId::PkgQueryGroup => "pkg.query_group",
            RunStepId::PkgQuery => "pkg.query",
            RunStepId::PkgRequiredBy => "pkg.required_by",
            RunStepId::PkgClosure => "pkg.closure",
            RunStepId::PkgSkipNote => "pkg.skip_note",
            RunStepId::AppInstall => "app.install",
            RunStepId::AppUninstall => "app.uninstall",
        }
    }
}

/// All `Run` step identifiers: for the executor, to check the plan against
/// its own parsing and not stay silent about an unknown step.
pub const RUN_STEP_IDS: &[RunStepId] = &[
    RunStepId::PkgUpdateDb,
    RunStepId::PkgInstall,
    RunStepId::PkgMain,
    RunStepId::PkgNix,
    RunStepId::PkgAur,
    RunStepId::PkgRemove,
    RunStepId::PkgQueryGroup,
    RunStepId::PkgQuery,
    RunStepId::PkgRequiredBy,
    RunStepId::PkgClosure,
    RunStepId::PkgSkipNote,
    RunStepId::AppInstall,
    RunStepId::AppUninstall,
];

/// A plan step. Execution happens when connected to live Python.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "step")]
pub enum PlanStep {
    /// Run a command (safe_run). `needs_root` = argv starts with pkexec. `id` is
    /// the machine identifier for the executor, `description` the caption for
    /// the person (change freely, NEVER branch on it).
    Run { id: RunStepId, cmd: Cmd, needs_root: bool, description: String },
    /// Wait for the package manager lock file to be released (poll 2 s; an empty
    /// `paths`, for a family without a lock, means "free right away").
    WaitPkgLock { paths: Vec<String>, timeout_secs: u64, poll_secs: u64 },
    /// Write a file (a generated script/log) with permissions `mode`.
    WriteFile { path: String, content: String, mode: u32 },
    /// Delete temporary files (errors are ignored).
    RemoveFiles { paths: Vec<String> },
    /// Enable the Gentoo overlay (`ensure_gentoo_overlay`: executed when
    /// connected to live Python).
    EnsureGentooOverlay { overlay: String },
}

/// Cutting off standard input for EVERY script under which the package
/// manager runs (the legacy path wrapper, the SteamOS wrapper, rexon-de-helper).
///
/// ⚠️ Introduced after experience: `pacman -S <group>` without `--noconfirm`
/// and with a live but silent stdin hangs on "Enter a selection
/// (default=all):" forever: checked in a sandbox (`fakeroot pacman -r …`,
/// stdin = an unclosed pipe: the question is printed, then silence until the
/// timeout kill). With `--noconfirm` pacman answers itself and does not read
/// input, but one cannot rely on the flag never getting lost in eleven
/// command tables. The Rexon UI has no terminal, there is nobody to hang
/// there and nobody to notice: the person stares at frozen install progress
/// for half an hour.
const STDIN_GUARD: &str = "exec </dev/null\n";

fn run_step(id: RunStepId, argv: Vec<String>, timeout: Option<u64>,
            lang_c: bool, description: &str) -> PlanStep {
    let needs_root = argv.first().map(|a| a == "pkexec").unwrap_or(false);
    PlanStep::Run {
        id,
        cmd: Cmd::new(argv, timeout, lang_c),
        needs_root,
        description: description.to_string(),
    }
}

// ============================================================================
// Command tables per family: port of _install_cmd/_remove_cmd/_query_cmd/
// _update_db_cmd/get_pkg_manager_name
// ============================================================================

fn unknown_family_error(verb: &str, tail: &str) -> String {
    // The RuntimeError text in Python: an f-string "package manager not
    // detected ({self.family}). {Verb} … by hand: {…}". Reachable only for
    // UNKNOWN (the other families are in the tables); an f-string of (str, Enum)
    // gives "PkgFamily.UNKNOWN": checked by a golden reference.
    trf(
        // TRANSLATORS: %1 is an instruction like “Install the packages
        // manually:”, %2 the package names. PkgFamily.UNKNOWN stays as is.
        "The package manager wasn't detected (PkgFamily.UNKNOWN). %1 %2",
        &[&verb, &tail],
    )
}

/// Name check before assembling ANY package manager command.
///
/// ⛔ It sits in the common command table, not only before the privileged
/// helper: the raw fallback of the same plan (`pkexec apt-get install …`)
/// would install a smuggled LOCAL FILE as root exactly as the helper did
/// before the fix of 25.09.2026. The rule is [`is_safe_pkg_arg`].
fn check_pkg_names(family: PkgFamily, packages: &[String]) -> Result<(), String> {
    match packages.iter().find(|p| !is_safe_pkg_arg(family, p)) {
        Some(bad) => Err(bad_pkg_name_error(bad)),
        None => Ok(()),
    }
}

/// Package install command: port of `PackageManager._install_cmd`.
pub fn install_cmd(family: PkgFamily, packages: &[String]) -> Result<Vec<String>, String> {
    check_pkg_names(family, packages)?;
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-S", "--needed", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "install", "-y", "--no-install-recommends"],
        PkgFamily::Fedora => &["dnf", "install", "-y"],
        PkgFamily::Suse => &["zypper", "install", "-y", "--no-recommends"],
        PkgFamily::Gentoo => &["emerge", "--ask=n", "--quiet"],
        PkgFamily::Alpine => &["apk", "add", "--no-progress"],
        PkgFamily::Void => &["xbps-install", "-y"],
        PkgFamily::Nix => &["nix-env", "-iA"],
        PkgFamily::Solus => &["eopkg", "it", "-y"],
        PkgFamily::Clear => &["swupd", "bundle-add"],
        PkgFamily::Alt => &["apt-get", "install", "-y"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Install the packages manually:"),
                &packages.join(" "),
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    if family == PkgFamily::Nix {
        cmd.extend(packages.iter().map(|p| format!("nixpkgs.{p}")));
    } else {
        cmd.extend(packages.iter().cloned());
    }
    Ok(cmd)
}

/// Package removal command: port of `PackageManager._remove_cmd`.
pub fn remove_cmd(family: PkgFamily, packages: &[String]) -> Result<Vec<String>, String> {
    check_pkg_names(family, packages)?;
    // Vital packages are not removed by any path, raw `pkexec` included.
    if let Some(v) = packages.iter().find(|p| is_vital_package(p)) {
        return Err(vital_remove_error(v));
    }
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Rns", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "remove", "-y", "--purge"],
        PkgFamily::Fedora => &["dnf", "remove", "-y"],
        PkgFamily::Suse => &["zypper", "remove", "-y"],
        PkgFamily::Gentoo => &["emerge", "--unmerge"],
        PkgFamily::Alpine => &["apk", "del"],
        PkgFamily::Void => &["xbps-remove", "-y"],
        PkgFamily::Nix => &["nix-env", "-e"],
        PkgFamily::Solus => &["eopkg", "rm", "-y"],
        PkgFamily::Clear => &["swupd", "bundle-remove"],
        PkgFamily::Alt => &["apt-get", "remove", "-y"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Remove the packages manually:"),
                &packages.join(" "),
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    cmd.extend(packages.iter().cloned());
    Ok(cmd)
}

/// Whether a package is installed: parsing the OUTCOME of the [`query_cmd`] command.
///
/// `None` = the command could not be run at all (`safe_run` returned None: no
/// such package manager or a timeout); `Some(false)`: it exited non-zero.
/// Nobody reads the command output: only the exit code gives the answer, so
/// garbage in stdout affects nothing.
///
/// The only place where this conclusion is drawn. The project kept the same
/// line `r is not None and r.returncode == 0` in FOUR copies:
/// `PackageManager.is_installed`, Plymouth's `_is_pkg_installed`,
/// `ComplianceCheck._pkg_state` and the "beyond the distribution" filter
/// (`_ensure_extra_packages`).
pub fn pkg_installed_from_run(ok: Option<bool>) -> bool {
    ok == Some(true)
}

/// The same by exit code: the form for the Python shim, which passes
/// `r.returncode` (or `None` if `safe_run` returned None).
pub fn pkg_installed_from_rc(returncode: Option<i32>) -> bool {
    pkg_installed_from_run(returncode.map(|c| c == 0))
}

/// Installed check command: port of `PackageManager._query_cmd`.
pub fn query_cmd(family: PkgFamily, package: &str) -> Result<Vec<String>, String> {
    let head: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Q"],
        PkgFamily::Debian => &["dpkg", "-s"],
        PkgFamily::Fedora => &["rpm", "-q"],
        PkgFamily::Suse => &["rpm", "-q"],
        PkgFamily::Gentoo => &["equery", "list"],
        PkgFamily::Alpine => &["apk", "info", "-e"],
        PkgFamily::Void => &["xbps-query"],
        PkgFamily::Nix => &["nix-env", "-q"],
        PkgFamily::Solus => &["eopkg", "info"],
        PkgFamily::Clear => &["swupd", "bundle-info"],
        PkgFamily::Alt => &["rpm", "-q"],
        PkgFamily::Unknown => {
            return Err(unknown_family_error(
                &tr("Check the package manually:"), package,
            ))
        }
    };
    let mut cmd: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    cmd.push(package.to_string());
    Ok(cmd)
}

/// Package database update command: port of `PackageManager._update_db_cmd`
/// (None for UNKNOWN: Python's `cmds.get` → None → "nothing to do").
pub fn update_db_cmd(family: PkgFamily) -> Option<Vec<String>> {
    let cmd: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Sy", "--noconfirm"],
        PkgFamily::Debian => &["apt-get", "update", "-q"],
        PkgFamily::Fedora => &["dnf", "makecache", "-q"],
        PkgFamily::Suse => &["zypper", "refresh", "-q"],
        PkgFamily::Gentoo => &["emerge", "--sync"],
        PkgFamily::Alpine => &["apk", "update"],
        PkgFamily::Void => &["xbps-install", "-S"],
        PkgFamily::Nix => &["nix-channel", "--update"],
        PkgFamily::Solus => &["eopkg", "ur"],
        PkgFamily::Clear => &["swupd", "update", "--download"],
        PkgFamily::Alt => &["apt-get", "update", "-q"],
        PkgFamily::Unknown => return None,
    };
    Some(cmd.iter().map(|s| s.to_string()).collect())
}

/// Whether a repository name fits the [`sync_repo_cmd`] question.
///
/// An empty name and a leading `-` are cut off first: the name goes to the
/// manager as a positional argument, and `--root` instead of a name would go
/// as an option. Then a narrow allowlist: Gentoo repository names consist of
/// letters, digits, `_`, `-` and a dot (`guru`, `cosmic-overlay`,
/// `sarahmiaoverlay`); there is never a space, newline or `/` in them.
fn valid_repo_name(repo: &str) -> bool {
    !repo.is_empty()
        && !repo.starts_with('-')
        && repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The "sync ONE repository" command: the package engine's ninth independent
/// question.
///
/// It does not reduce to any earlier one. [`update_db_cmd`] updates
/// EVERYTHING: `emerge --sync` pulls both the portage tree and every enabled
/// overlay, while the subject of this question is one named repository, just
/// enabled and therefore still empty. It is asked by `ensure_gentoo_overlay`
/// (`rexon/system.py`), where the command used to be hardcoded.
///
/// The answer is a FULL command, `pkexec` included, unlike [`update_db_cmd`],
/// which returns a bare argv and leaves the "named path or raw" choice to
/// [`update_db_plan`]. There is nothing to choose from here: rexon-pkg-helper
/// knows four operations ([`PKG_HELPER_OPS`]), and this is not among them:
/// its `update` syncs everything and takes no positional arguments at all.
/// Adding a fifth operation would mean changing a file already deployed on
/// other people's machines for the sake of one family; so there is one path,
/// raw `pkexec`, exactly as before.
///
/// `None` for all other families, and this is not laziness: a repository
/// enabled SEPARATELY from the distribution set exists only on Gentoo: that
/// is what an overlay is, and only it can be empty right after enabling.
/// * pacman has no per-repository update at all: `-Sy` updates all
///   databases at once, the command takes no repository name.
/// * apt is the same: `apt-get update` reads the whole `sources.list`. It can
///   only be narrowed by overriding `Dir::Etc::sourcelist`, i.e. slipping in
///   another settings file, and that is not an "update this one" operation.
/// * dnf (`--repo`) and zypper (`refresh <alias>`) have the syntax, but there
///   is nobody to ask them this question: no Rexon path enables repositories
///   on these families (neither `add-apt-repository`, nor `dnf config-manager
///   --add-repo`, nor `zypper ar` occur in the tree), so there is nowhere to
///   take a repository name from. A privileged command we never ran and
///   nobody asks for is worse than an honest "I do not know".
pub fn sync_repo_cmd(family: PkgFamily, repo: &str) -> Option<Vec<String>> {
    if family != PkgFamily::Gentoo || !valid_repo_name(repo) {
        return None;
    }
    Some(vec![
        "pkexec".to_string(),
        "emerge".to_string(),
        "--sync".to_string(),
        "--repo".to_string(),
        repo.to_string(),
    ])
}

/// Package manager name: port of `get_pkg_manager_name`.
pub fn pkg_manager_name(family: PkgFamily, aur_helper: Option<&str>) -> String {
    match family {
        PkgFamily::Arch => match aur_helper {
            Some(h) => format!("pacman + {h}"),
            None => "pacman".to_string(),
        },
        PkgFamily::Debian => "apt".to_string(),
        PkgFamily::Fedora => "dnf".to_string(),
        PkgFamily::Suse => "zypper".to_string(),
        PkgFamily::Gentoo => "emerge".to_string(),
        PkgFamily::Alpine => "apk".to_string(),
        PkgFamily::Void => "xbps".to_string(),
        PkgFamily::Nix => "nix-env".to_string(),
        PkgFamily::Solus => "eopkg".to_string(),
        PkgFamily::Clear => "swupd".to_string(),
        PkgFamily::Alt => "apt-get (APT-RPM)".to_string(),
        PkgFamily::Unknown => "unknown".to_string(),
    }
}

/// Package manager lock files: port of `_LOCK_PATHS`. Held both by the
/// transaction and by post-hooks (snap-pac → snapper → btrfs snapshot).
pub fn lock_paths(family: PkgFamily) -> &'static [&'static str] {
    match family {
        PkgFamily::Arch => &["/var/lib/pacman/db.lck"],
        PkgFamily::Debian => &["/var/lib/dpkg/lock-frontend", "/var/lib/dpkg/lock"],
        PkgFamily::Fedora => &["/var/lib/rpm/.rpm.lock"],
        PkgFamily::Suse => &["/var/run/zypp.pid"],
        PkgFamily::Gentoo => &["/var/lock/portage"],
        PkgFamily::Void => &["/var/lock/xbps"],
        PkgFamily::Alpine => &["/var/lib/apk/db.lock"],
        PkgFamily::Solus => &["/var/lib/eopkg/lock"],
        _ => &[],
    }
}

/// The default pacman database directory: the one pacman itself takes when
/// `/etc/pacman.conf` has no `DBPath`.
pub const PACMAN_DEFAULT_DBPATH: &str = "/var/lib/pacman/";

/// The lock name inside the pacman database directory.
pub const PACMAN_LOCK_NAME: &str = "db.lck";

/// The pacman lock from the `pacman-conf DBPath` output. Pure.
///
/// WHY. pacman's database directory is configurable (`DBPath` in
/// `/etc/pacman.conf`), and the lock lives in it. A hardcoded
/// `/var/lib/pacman/db.lck` with a custom `DBPath` looked in the wrong place:
/// a busy database looked free, and the install failed with "database busy"
/// instead of waiting.
///
/// `conf_stdout` is the stdout of `pacman-conf DBPath`; `None`: no command,
/// it failed or timed out. Only one non-empty line with an absolute path
/// without control characters is accepted; otherwise (empty, an error
/// message, several lines) the fallback [`PACMAN_DEFAULT_DBPATH`].
pub fn pacman_lock_path(conf_stdout: Option<&str>) -> String {
    let dbpath = conf_stdout
        .and_then(|out| {
            let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
            let first = lines.next()?;
            if lines.next().is_some() {
                return None;
            }
            Some(first)
        })
        .filter(|p| p.starts_with('/') && !p.chars().any(char::is_control))
        .unwrap_or(PACMAN_DEFAULT_DBPATH);
    if dbpath.ends_with('/') {
        format!("{dbpath}{PACMAN_LOCK_NAME}")
    } else {
        format!("{dbpath}/{PACMAN_LOCK_NAME}")
    }
}

/// Package manager lock files on the LIVE machine.
///
/// Like [`lock_paths`], but the pacman lock comes from the real `DBPath`
/// (`pacman-conf DBPath`, parsed by [`pacman_lock_path`]). The pacman-conf
/// answer is cached per process: the setting does not change while running,
/// and the lock is waited for on every install.
pub fn lock_paths_live(family: PkgFamily) -> Vec<String> {
    match family {
        PkgFamily::Arch => vec![pacman_lock_path_live().to_string()],
        _ => lock_paths(family).iter().map(|s| s.to_string()).collect(),
    }
}

fn pacman_lock_path_live() -> &'static str {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let report = crate::utils::run(
            &crate::utils::RunSpec {
                argv: vec!["pacman-conf".to_string(), "DBPath".to_string()],
                output: crate::utils::RunOutput::Capture,
                timeout_secs: Some(10),
                lang_c: true,
                env: Vec::new(),
                stderr_to_stdout: false,
            },
            None,
        );
        let stdout = (report.outcome == crate::utils::RunOutcome::Ok)
            .then(|| String::from_utf8_lossy(&report.stdout).into_owned());
        pacman_lock_path(stdout.as_deref())
    })
}

/// Lock wait step: port of `_wait_for_pkg_lock` (poll 2 s, at most 300 s).
fn wait_lock_step(family: PkgFamily) -> PlanStep {
    PlanStep::WaitPkgLock {
        paths: lock_paths(family).iter().map(|s| s.to_string()).collect(),
        timeout_secs: 300,
        poll_secs: 2,
    }
}

// ============================================================================
// update_db / install: port of PackageManager.update_db / install
// ============================================================================

/// Families where update_db runs as root (all but NIX/UNKNOWN): like the
/// `privileged` tuple in Python.
fn update_db_privileged(family: PkgFamily) -> bool {
    !matches!(family, PkgFamily::Nix | PkgFamily::Unknown)
}

/// The `update_db` plan: an empty plan = "nothing to do, success".
pub fn update_db_plan(family: PkgFamily, pkg_helper_available: bool) -> Vec<PlanStep> {
    let Some(cmd) = update_db_cmd(family) else {
        return Vec::new();
    };
    let full = if update_db_privileged(family) {
        match privileged_pkg_cmd(family, "update", &[], pkg_helper_available) {
            Some(named) => named,
            None => {
                let mut v = vec!["pkexec".to_string()];
                v.extend(cmd);
                v
            }
        }
    } else {
        cmd
    };
    // 120 s was not enough on a slow mirror: a database update is network work.
    vec![run_step(RunStepId::PkgUpdateDb, full, Some(600), false,
              "Обновление базы пакетов")]
}

/// The privileged install command: the named path via rexon-pkg-helper,
/// otherwise raw `pkexec` over [`install_cmd`].
///
/// One choice for all places where Rexon installs packages. A handwritten
/// copy of this "otherwise" is exactly how a THIRD Gentoo command variant
/// appeared in `ensure_gentoo_overlay`: `emerge --ask=n` without `--quiet`,
/// diverging both from the [`install_cmd`] table and from the helper's own
/// built-in template.
fn privileged_install_argv(
    family: PkgFamily,
    packages: &[String],
    base: &[String],
    helper_available: bool,
) -> Vec<String> {
    if let Some(named) = privileged_pkg_cmd(family, "install", packages, helper_available) {
        return named;
    }
    let mut argv = vec!["pkexec".to_string()];
    argv.extend(base.iter().cloned());
    argv
}

/// Choosing the install path on top of an already assembled [`install_cmd`]:
/// NixOS installs packages into the USER profile (`nix-env -iA`) and does
/// not ask for root: `pkexec` is not needed there and would be an outright
/// error; the other ten families go privileged.
fn choose_install_argv(
    family: PkgFamily,
    packages: &[String],
    base: &[String],
    helper_available: bool,
) -> Vec<String> {
    if family == PkgFamily::Nix {
        return base.to_vec();
    }
    privileged_install_argv(family, packages, base, helper_available)
}

/// The engine's **only** answer to "which command installs these packages":
/// from the flag table to the choice "named path or raw pkexec".
///
/// A separate entry point next to [`install_plan`] is needed by places that
/// need one command, not a plan with steps, a timeout and a sudo hint:
/// installing the wallpaper engine's video dependencies
/// ([`crate::wallpaper_engine::install_deps_argv`]) and compliance repair
/// ([`crate::compliance::pkg_install_arch`]). Both used to keep THEIR OWN
/// tables and both had drifted from the engine.
///
/// `None`: there is no command and one must not be invented:
/// * the package list is empty: "install nothing" cannot be expressed as a
///   command, and `pacman -S --needed --noconfirm` without a single name
///   would return a usage error as root;
/// * the `UNKNOWN` family: the manager was not recognised, and the caller
///   tells the person what to install by hand.
pub fn install_argv(
    family: PkgFamily,
    packages: &[String],
    helper_available: bool,
) -> Option<Vec<String>> {
    if packages.is_empty() {
        return None;
    }
    let base = install_cmd(family, packages).ok()?;
    Some(choose_install_argv(family, packages, &base, helper_available))
}

/// The `install` plan + a sudo hint on a pkexec refusal.
pub struct InstallPlan {
    pub steps: Vec<PlanStep>,
    /// An early exit without steps: (ok, message).
    pub early_result: Option<(bool, String)>,
    /// The "administrator password not confirmed…" error text (Python shows it on
    /// returncode 126 / "Not authorized").
    pub sudo_hint: String,
}

/// Port of `PackageManager.install`.
pub fn install_plan(
    family: PkgFamily,
    packages: &[String],
    aur: bool,
    aur_helper: Option<&str>,
    pkg_helper_available: bool,
) -> Result<InstallPlan, String> {
    if packages.is_empty() {
        return Ok(InstallPlan {
            steps: Vec::new(),
            early_result: Some((true, tr("No packages to install"))),
            sudo_hint: String::new(),
        });
    }
    let base_cmd = install_cmd(family, packages)?;
    let aur_argv = if aur && family == PkgFamily::Arch {
        aur_helper.map(|helper| crate::aur_install::install_argv(helper, packages, false))
    } else {
        None
    };
    let argv = match aur_argv {
        Some(v) => v,
        // The same choice `install_argv` gives out: there must not be a second copy
        // of "pkexec or the named path, and Nix without root".
        None => choose_install_argv(family, packages, &base_cmd, pkg_helper_available),
    };
    let sudo_hint = trf(
        "The administrator password wasn't confirmed.\nYou can install from a terminal: sudo %1",
        &[&base_cmd.join(" ")],
    );
    Ok(InstallPlan {
        steps: vec![run_step(RunStepId::PkgInstall, argv, None, false,
                             "Установка пакетов")],
        early_result: None,
        sudo_hint,
    })
}

// ============================================================================
// remove: port of PackageManager.remove (the full pipeline as a plan)
// ============================================================================

/// The `remove` plan. Probe steps (`phase` in the description: qg / query /
/// required-by / closure / skip-note) go in Python's order; the closure phase
/// order in Python is nondeterministic (set iteration): the parity test sorts
/// this segment on both sides.
pub struct RemovePlan {
    pub steps: Vec<PlanStep>,
    /// The result if there are no executing steps (wait+remove): (ok, message).
    pub result_without_exec: Option<(bool, String)>,
    pub safe: Vec<String>,
    pub skipped: Vec<String>,
    /// Annotations of skipped packages: "pkg (required by: a, b)" / "(needed by
    /// another skipped package)".
    pub skipped_annotated: Vec<String>,
    /// Addition to stdout on success (a "\n\nSkipped…" note or "").
    pub success_note: String,
    /// Error text on a pkexec refusal.
    pub sudo_hint: String,
}

/// Port of `PackageManager.remove`. The pacman output is passed as closures
/// (as in `system.rs`): `qg`/`qi` are stdout on rc==0, `query_installed` is
/// whether the package is installed (rc==0 of the query command).
pub fn remove_plan(
    family: PkgFamily,
    packages: &[String],
    qg: &dyn Fn(&str) -> Option<String>,
    qi: &dyn Fn(&str) -> Option<String>,
    query_installed: &dyn Fn(&str) -> bool,
    pkg_helper_available: bool,
) -> Result<RemovePlan, String> {
    let mut plan = RemovePlan {
        steps: Vec::new(),
        result_without_exec: None,
        safe: Vec::new(),
        skipped: Vec::new(),
        skipped_annotated: Vec::new(),
        success_note: String::new(),
        sudo_hint: String::new(),
    };
    if packages.is_empty() {
        plan.result_without_exec = Some((true, String::new()));
        return Ok(plan);
    }

    // 1. Expand groups (a "qg" step per input package). The command is set by
    //    the package engine: it also decides which family has the "what is in
    //    the group" question at all; where there is none, there is no step.
    for p in packages {
        if let Some(q) = package_group_query(family, p) {
            plan.steps.push(run_step(
                RunStepId::PkgQueryGroup,
                q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "qg",
            ));
        }
    }
    // Vital packages (group members too) silently drop out of the set: neither
    // the helper nor the raw path (`remove_cmd`) would remove them.
    let expanded: Vec<String> = expand_groups(family, packages, qg)
        .into_iter()
        .filter(|p| !is_vital_package(p))
        .collect();

    // 2. Filter out the not installed ones (a query command for each).
    let mut installed: Vec<String> = Vec::new();
    for p in &expanded {
        plan.steps.push(run_step(RunStepId::PkgQuery, query_cmd(family, p)?,
                                 None, false, "query"));
        if query_installed(p) {
            installed.push(p.clone());
        }
    }
    if installed.is_empty() {
        plan.result_without_exec =
            Some((true, tr("No installed packages to remove")));
        return Ok(plan);
    }

    // 3. Forbidden set + transitive closure (-Qi steps; the same logic as
    //    system::filter_removable_transitive, but recording plan steps).
    let candidate: std::collections::HashSet<String> =
        installed.iter().cloned().collect();
    let mut forbidden: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if family == PkgFamily::Arch {
        for p in &installed {
            if let Some(q) = installed_meta_query(family, p) {
                plan.steps.push(run_step(
                    RunStepId::PkgRequiredBy,
                    q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "required-by",
                ));
            }
            let outside: Vec<String> = qi_field(family, p, "Required By", qi)
                .into_iter()
                .filter(|d| !candidate.contains(d))
                .collect();
            if !outside.is_empty() {
                forbidden.insert(p.clone());
            }
        }
        // Closure: a snapshot of forbidden on each pass (list(forbidden) in
        // Python); the pass order here is by installed (deterministic).
        let mut changed = true;
        while changed {
            changed = false;
            let snapshot: Vec<String> = installed
                .iter()
                .filter(|p| forbidden.contains(*p))
                .cloned()
                .collect();
            for p in snapshot {
                if let Some(q) = installed_meta_query(family, &p) {
                    plan.steps.push(run_step(
                        RunStepId::PkgClosure,
                        q.cmd.argv, q.cmd.timeout_secs, q.cmd.lang_c, "closure",
                    ));
                }
                let deps: std::collections::HashSet<String> =
                    qi_field(family, &p, "Depends On", qi).into_iter().collect();
                for d in deps.intersection(&candidate) {
                    if !forbidden.contains(d) {
                        forbidden.insert(d.clone());
                        changed = true;
                    }
                }
            }
        }
    }
    plan.safe = installed
        .iter()
        .filter(|p| !forbidden.contains(*p))
        .cloned()
        .collect();
    plan.skipped = installed
        .iter()
        .filter(|p| forbidden.contains(*p))
        .cloned()
        .collect();

    // Skipped annotations (repeated -Qi "Required By", as in Python).
    for p in &plan.skipped {
        plan.steps.push(run_step(
            RunStepId::PkgSkipNote,
            vec!["pacman".into(), "-Qi".into(), p.clone()],
            None, true, "skip-note",
        ));
        let outsiders: Vec<String> = qi_field(family, p, "Required By", qi)
            .into_iter()
            .filter(|d| !candidate.contains(d))
            .collect();
        if !outsiders.is_empty() {
            let head: Vec<&str> =
                outsiders.iter().take(3).map(String::as_str).collect();
            plan.skipped_annotated
                .push(trf("%1 (required by: %2)", &[&p, &head.join(", ")]));
        } else {
            plan.skipped_annotated
                .push(trf("%1 (needed by another skipped package)", &[&p]));
        }
    }

    if plan.safe.is_empty() {
        let bullets: Vec<String> = plan
            .skipped_annotated
            .iter()
            .map(|s| format!("  • {s}"))
            .collect();
        plan.result_without_exec = Some((
            false,
            trf(
                "All of this desktop's packages are needed by other installed programs:\n%1",
                &[&bullets.join("\n")],
            ),
        ));
        return Ok(plan);
    }

    // 4. Waiting for the lock + the removal command (timeout 600).
    plan.steps.push(wait_lock_step(family));
    let base_cmd = remove_cmd(family, &plan.safe)?;
    let argv = if family == PkgFamily::Nix {
        base_cmd.clone()
    } else {
        match privileged_pkg_cmd(family, "remove", &plan.safe, pkg_helper_available)
        {
            Some(named) => named,
            None => {
                let mut v = vec!["pkexec".to_string()];
                v.extend(base_cmd.iter().cloned());
                v
            }
        }
    };
    plan.steps.push(run_step(RunStepId::PkgRemove, argv,
                             Some(crate::utils::DOWNLOAD_TIMEOUT_SECS),
                             false, "remove"));

    if !plan.skipped_annotated.is_empty() {
        let bullets: Vec<String> = plan
            .skipped_annotated
            .iter()
            .map(|s| format!("  • {s}"))
            .collect();
        plan.success_note = format!(
            "\n\n{}",
            trf("Skipped (needed by other desktops):\n%1", &[&bullets.join("\n")])
        );
    }
    // ⭐ Fix: the refusal hint on REMOVAL said "Install from a terminal".
    plan.sudo_hint = trf(
        "The administrator password wasn't confirmed.\nYou can remove from a terminal: sudo %1",
        &[&base_cmd.join(" ")],
    );
    Ok(plan)
}

// ============================================================================
// install_batch: port of PackageManager.install_batch (plan + script texts)
// ============================================================================

/// NixOS: snippets for configuration.nix: port of `_NIXOS_DE_SNIPPETS`.
pub const NIXOS_DE_SNIPPETS: &[(&str, &str)] = &[
    ("kde", "services.desktopManager.plasma6.enable = true;\nservices.displayManager.sddm.enable = true;"),
    ("gnome", "services.xserver.desktopManager.gnome.enable = true;\nservices.xserver.displayManager.gdm.enable = true;"),
    ("xfce", "services.xserver.desktopManager.xfce.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("cinnamon-mint", "services.xserver.desktopManager.cinnamon.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("mate", "services.xserver.desktopManager.mate.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    ("budgie", "services.xserver.desktopManager.budgie.enable = true;\nservices.displayManager.lightdm.enable = true;"),
    // The nixpkgs module `nixos/modules/services/desktop-managers/pantheon.nix`
    // (`config.services.desktopManager.pantheon`) enables the Pantheon greeter
    // itself, but requires lightdm and warns if it is off.
    ("pantheon", "services.xserver.enable = true;\nservices.xserver.displayManager.lightdm.enable = true;\nservices.desktopManager.pantheon.enable = true;"),
    ("cosmic", "services.desktopManager.cosmic.enable = true;\nservices.displayManager.cosmic-greeter.enable = true;"),
    ("hyprland", "programs.hyprland.enable = true;"),
    ("lxqt", "services.xserver.desktopManager.lxqt.enable = true;\nservices.displayManager.sddm.enable = true;"),
];

/// Notes to the [`NIXOS_DE_SNIPPETS`] snippets: a `# + …` comment at the end
/// of a snippet. It is a hint for the person, so it comes from the
/// translation catalog, while the snippet itself (configuration.nix code) is
/// not translated.
pub const NIXOS_DE_NOTES: &[(&str, &str)] = &[
    ("hyprland", n_("set up waybar, wofi, kitty, etc.")),
];

/// The configuration.nix snippet for a desktop together with the note in the
/// UI language; `None`: the desktop is not installed on NixOS via Rexon.
pub fn nixos_de_snippet(de: &str) -> Option<String> {
    let snippet = table_get(NIXOS_DE_SNIPPETS, de)?;
    Some(match table_get(NIXOS_DE_NOTES, de) {
        Some(note) => format!("{snippet}\n# + {}", tr(note)),
        None => snippet.to_string(),
    })
}

/// An `echo '…'` line of the install script: the script output goes to the
/// window log, so the text comes from the translation catalog, and a single
/// quote in the translation is escaped for the shell.
fn echo_line(text: &str) -> String {
    format!("echo '{}'", text.replace('\'', "'\\''"))
}

/// Which DEs need an overlay on Gentoo: port of `_GENTOO_OVERLAYS`.
pub const GENTOO_OVERLAYS: &[(&str, &str)] = &[
    ("budgie", "sarahmiaoverlay"),
    ("cosmic", "cosmic-overlay"),
];

/// The package providing the `eselect repository` subcommand: without it
/// there is nothing to enable an overlay with. The name used to live
/// hardcoded in `ensure_gentoo_overlay` (`rexon/system.py`), and its place is
/// here, next to the overlay table, where the rest of the data about them lives.
pub const GENTOO_OVERLAY_TOOL_PACKAGE: &str = "app-eselect/eselect-repository";

fn table_get<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// The `install_batch` plan + the texts of the generated scripts.
pub struct BatchPlan {
    pub steps: Vec<PlanStep>,
    /// An early exit (empty / NixOS with de_id / immutable): (ok, message).
    pub early_result: Option<(bool, String)>,
    /// The main packages were installed via rexon-de-helper (the DM is already
    /// enabled in the same authorization session: install_de does not do a 2nd pkexec).
    pub used_de_helper: bool,
    /// The final message on success.
    pub final_message: String,
}

/// Port of `PackageManager.install_batch`. The tmp file paths are passed as
/// parameters (the executor creates them via mkstemp), so argv and the
/// script texts are deterministic and comparable with Python.
#[allow(clippy::too_many_arguments)]
pub fn install_batch_plan(
    family: PkgFamily,
    detector: &DistroDetector,
    packages: &[String],
    aur_packages: &[String],
    aur_helper: Option<&str>,
    update_first: bool,
    de_id: Option<&str>,
    de_helper_available: bool,
    tmp_sh: &str,
    tmp_log: &str,
) -> Result<BatchPlan, String> {
    install_batch_plan_with_deps(
        family, detector, packages, &[], &[], aur_packages, aur_helper, update_first, de_id,
        de_helper_available, tmp_sh, tmp_log,
    )
}

/// An install script line: mark `deps` as installed as dependencies
/// (`pacman -D --asdeps`), so that removing the desktop (`-Rns`) removes
/// them. Arch only; empty: no line.
pub fn mark_deps_line(family: PkgFamily, deps: &[String]) -> Result<Option<String>, String> {
    if family != PkgFamily::Arch || deps.is_empty() {
        return Ok(None);
    }
    check_pkg_names(family, deps)?;
    Ok(Some(format!("pacman -D --asdeps {}", deps.join(" "))))
}

/// [`install_batch_plan`] and `deps_only`: packages from `packages` that the
/// transaction installs only as dependencies of the set (a DKMS module and
/// headers for the installed kernels, [`InstallPreview::deps_only`]). They
/// are marked in the same script, after a successful install, without a
/// second password. `replace`: installed packages the person agreed to
/// replace ([`conflict_swap_approved`]): the same transaction removes them
/// ([`install_cmd_replacing`]), and it goes through the wrapper, not the
/// helper: its named action has no replacement consent.
#[allow(clippy::too_many_arguments)]
pub fn install_batch_plan_with_deps(
    family: PkgFamily,
    detector: &DistroDetector,
    packages: &[String],
    deps_only: &[String],
    replace: &[String],
    aur_packages: &[String],
    aur_helper: Option<&str>,
    update_first: bool,
    de_id: Option<&str>,
    de_helper_available: bool,
    tmp_sh: &str,
    tmp_log: &str,
) -> Result<BatchPlan, String> {
    let deps_only: Vec<String> = deps_only.iter().filter(|d| packages.contains(d)).cloned().collect();
    let mark_deps = mark_deps_line(family, &deps_only)?;
    let mut plan = BatchPlan {
        steps: Vec::new(),
        early_result: None,
        used_de_helper: false,
        final_message: tr("Installation complete"),
    };
    if packages.is_empty() && aur_packages.is_empty() {
        plan.early_result = Some((true, tr("No packages to install")));
        return Ok(plan);
    }

    // ── NixOS: a DE is installed via configuration.nix, not via nix-env ──
    if family == PkgFamily::Nix {
        if let Some(de) = de_id {
            let msg = match nixos_de_snippet(de) {
                Some(snippet) => trf(
                    // TRANSLATORS: %1 is a configuration.nix snippet.
                    "NixOS uses a declarative model.\n\
                     Add this to /etc/nixos/configuration.nix:\n\n\
                     %1\n\n\
                     Then run:\n  sudo nixos-rebuild switch",
                    &[&snippet],
                ),
                None => trf(
                    "Rexon can't set up the '%1' desktop on NixOS.\n\
                     Set it up yourself in /etc/nixos/configuration.nix.",
                    &[&de],
                ),
            };
            plan.early_result = Some((false, msg));
            return Ok(plan);
        }
    }

    // ── SteamOS: install via steamos-readonly ──
    if detector.is_steamos() {
        let update = update_db_cmd(family);
        let install = install_cmd_replacing(family, packages, replace)?;

        let mut script_lines: Vec<String> = vec![
            "#!/bin/bash".into(),
            "set -e".into(),
            // pacman output is parsed by the install progress parser
            // (`parse_pacman_line`): "warning:", "error:", "is up to date -- skipping".
            // As on the regular path below: otherwise on a Russian or German system the
            // parser finds nothing. `LC_ALL=C` also silences `LANGUAGE`, which the Rexon
            // window sets to its language: gettext does not read it under the C locale.
            "export LC_ALL=C LANG=C".into(),
            String::new(),
            "# ── Step 1: disabling read-only ──".into(),
            echo_line(&tr("🔓 Turning off steamos-readonly…")),
            "steamos-readonly disable".into(),
            String::new(),
            "# ── Step 2: initializing pacman keys ──".into(),
            echo_line(&tr("🔑 Setting up the pacman keys…")),
            "pacman-key --init 2>/dev/null || true".into(),
            "pacman-key --populate archlinux 2>/dev/null || true".into(),
            "pacman-key --populate holo 2>/dev/null || true".into(),
            String::new(),
        ];
        if update_first {
            if let Some(upd) = &update {
                script_lines.push("# ── Step 3: updating the package database ──".into());
                script_lines.push(upd.join(" "));
                script_lines.push(String::new());
            }
        }
        let install_line = install.join(" ").replace(
            "--noconfirm",
            "--noconfirm --noprogressbar --color never",
        );
        script_lines.extend([
            "# ── Step 4: installing packages ──".to_string(),
            // TRANSLATORS: %1 is a number of packages.
            echo_line(&trf("📦 Installing packages: %1…", &[&packages.len()])),
            install_line,
        ]);
        script_lines.extend(mark_deps.clone());
        script_lines.extend([
            String::new(),
            "# ── Step 5: enabling read-only again ──".to_string(),
            echo_line(&tr("🔒 Turning steamos-readonly back on…")),
            "steamos-readonly enable".to_string(),
            String::new(),
            echo_line(&format!("✅ {}", tr("Installation on SteamOS complete"))),
        ]);
        let script_body = script_lines.join("\n") + "\n";

        // chown at the start: fs.protected_regular=1 blocks root from truncating
        // files owned by a regular user in /tmp (sticky bit). chmod 0o666 alone
        // does not help: the kernel looks at the owner, not the permissions.
        let wrapper = format!(
            "#!/bin/bash\n\
             {STDIN_GUARD}\
             chown root:root {tmp_log} 2>/dev/null\n\
             > {tmp_log}\n\
             (\n\
             {script_body}\
             ) >> {tmp_log} 2>&1\n\
             EXIT_CODE=$?\n\
             echo \"__REXON_EXIT__$EXIT_CODE\" >> {tmp_log}\n\
             # Always turn readonly back on on error\n\
             if [ $EXIT_CODE -ne 0 ]; then\n\
             \x20 steamos-readonly enable 2>/dev/null || true\n\
             fi\n\
             # Give the ownership back, so the user can delete the tmp files\n\
             chown $PKEXEC_UID:$PKEXEC_UID {tmp_log} {tmp_sh} 2>/dev/null\n\
             exit $EXIT_CODE\n"
        );
        plan.steps.push(PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: wrapper,
            mode: 0o755,
        });
        plan.steps.push(PlanStep::WriteFile {
            path: tmp_log.to_string(),
            content: String::new(),
            mode: 0o666,
        });
        plan.steps.push(wait_lock_step(family));
        plan.steps.push(run_step(
            RunStepId::PkgMain,
            vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
            Some(crate::utils::DOWNLOAD_TIMEOUT_SECS), false, "SteamOS: установка пакетов",
        ));
        plan.steps.push(PlanStep::RemoveFiles {
            paths: vec![tmp_sh.to_string(), tmp_log.to_string()],
        });
        // AUR on SteamOS: right away, without waiting for the lock (as in Python).
        if !aur_packages.is_empty() {
            if let Some(helper) = aur_helper {
                let argv = crate::aur_install::install_argv(helper, aur_packages, false);
                plan.steps.push(run_step(RunStepId::PkgAur, argv, None, false,
                                         "Установка AUR"));
            }
        }
        plan.final_message = tr("Installation on SteamOS complete");
        return Ok(plan);
    }

    // ── Immutable (ostree) distributions ──
    if detector.is_immutable() && detector.distro_id() != "nixos" {
        let distro_id = detector.distro_id();
        let pkg_str = packages.join(" ");
        let variant_ostree = detector
            .info
            .get("VARIANT_ID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase()
            .contains("ostree");
        let msg = if matches!(distro_id.as_str(),
                              "silverblue" | "kinoite" | "sericea" | "onyx")
            || detector.name().to_lowercase().contains("atomic")
            || variant_ostree
        {
            trf(
                // TRANSLATORS: %1 is the distribution name, %2 package names.
                "%1 is an immutable distribution.\n\
                 The regular dnf doesn't work here. Use:\n\n  \
                 rpm-ostree install %2\n  \
                 systemctl reboot\n\n\
                 Or install the desktop with Fedora Toolbox / Distrobox.",
                &[&detector.pretty_name(), &pkg_str],
            )
        } else if distro_id == "bazzite" {
            trf(
                "Bazzite is an immutable (ostree) distribution.\n\
                 To install packages:\n\n  \
                 rpm-ostree install %1\n  \
                 systemctl reboot\n\n\
                 Or use Distrobox for an isolated environment.",
                &[&pkg_str],
            )
        } else {
            trf(
                "%1 is an immutable distribution.\n\
                 Installing packages with the regular package manager may not work.\n\
                 Use Flatpak, Distrobox or your distribution's own tools.",
                &[&detector.pretty_name()],
            )
        };
        plan.early_result = Some((false, msg));
        return Ok(plan);
    }

    // ── 0. Gentoo: enable the overlay if needed ──
    if family == PkgFamily::Gentoo {
        if let Some(de) = de_id {
            if let Some(overlay) = table_get(GENTOO_OVERLAYS, de) {
                plan.steps.push(PlanStep::EnsureGentooOverlay {
                    overlay: overlay.to_string(),
                });
            }
        }
    }

    // ── 1. Main packages (one pkexec) ──
    if !packages.is_empty() {
        let update = update_db_cmd(family);
        let install = install_cmd_replacing(family, packages, replace)?;

        if family == PkgFamily::Nix {
            // Nix does not use pkexec.
            if update_first {
                if let Some(upd) = update {
                    plan.steps.push(run_step(RunStepId::PkgUpdateDb, upd,
                                             None, false,
                                             "Обновление базы пакетов"));
                }
            }
            // nix can build from source: the command class knows that.
            plan.steps.push(run_step(RunStepId::PkgNix, install, None, false,
                                     "Установка пакетов (nix-env)"));
        } else {
            // Part B: the named rexon-de-helper action (auth_admin_keep: a confirmed
            // password holds for a few minutes and only for the Rexon window);
            // otherwise legacy `pkexec bash <tmp_sh>` (a password every time).
            // The helper installs the set from ITS OWN table, not the one passed: if
            // the set was adjusted (a group member dropped, a DKMS module added for the
            // installed kernel, the Deepin on Treeland variant), the wrapper installs.
            let use_de_helper = de_id
                .map(|de| de_helper_available
                     && de_helper_supports(de, family.as_str())
                     && de_helper_covers(de, family, packages)
                     && mark_deps.is_none()
                     && replace.is_empty())
                .unwrap_or(false);
            plan.used_de_helper = use_de_helper;

            if !use_de_helper {
                // One bash script → one pkexec (the legacy path). LC_ALL=C: stable English
                // pacman output for the progress parser.
                let mut script_lines: Vec<String> = vec![
                    "#!/bin/bash".into(),
                    "set -e".into(),
                    "export LC_ALL=C LANG=C".into(),
                ];
                if update_first {
                    if let Some(upd) = &update {
                        script_lines.push(upd.join(" "));
                    }
                }
                let install_line = install.join(" ").replace(
                    "--noconfirm",
                    "--noconfirm --noprogressbar --color never",
                );
                script_lines.push(install_line);
                script_lines.extend(mark_deps.clone());
                let script_body = script_lines.join("\n") + "\n";

                // The wrapper: all pacman output → the log file; the fs.protected_regular
                // chown pitfall (root:root at the start, PKEXEC_UID at the end).
                let wrapper = format!(
                    "#!/bin/bash\n\
                     {STDIN_GUARD}\
                     chown root:root {tmp_log} 2>/dev/null\n\
                     > {tmp_log}\n\
                     (\n\
                     {script_body}\
                     ) >> {tmp_log} 2>&1\n\
                     EXIT_CODE=$?\n\
                     echo \"__REXON_EXIT__$EXIT_CODE\" >> {tmp_log}\n\
                     # Give the ownership back, so the user can delete the tmp files\n\
                     chown $PKEXEC_UID:$PKEXEC_UID {tmp_log} {tmp_sh} 2>/dev/null\n\
                     exit $EXIT_CODE\n"
                );
                plan.steps.push(PlanStep::WriteFile {
                    path: tmp_sh.to_string(),
                    content: wrapper,
                    mode: 0o755,
                });
            }
            // Log: clear it and give root access (fs.protected_regular=2).
            plan.steps.push(PlanStep::WriteFile {
                path: tmp_log.to_string(),
                content: String::new(),
                mode: 0o666,
            });
            plan.steps.push(wait_lock_step(family));
            let pkexec_argv = if use_de_helper {
                let mut argv = vec!["pkexec".to_string(), de_helper_path().to_string()];
                // Helper message language (decision of 30.09.2026).
                argv.extend(crate::system_l10n::lang_args(de_helper_path()));
                argv.extend([
                    de_id.unwrap_or_default().to_string(),
                    family.as_str().to_string(), tmp_log.to_string(),
                ]);
                argv
            } else {
                vec!["pkexec".to_string(), "bash".to_string(),
                     tmp_sh.to_string()]
            };
            // Behind `pkexec bash <script>` is the install of a whole desktop: 600 s
            // was not always enough, and this is downloading, not a query.
            plan.steps.push(run_step(RunStepId::PkgMain, pkexec_argv,
                                     Some(crate::utils::DOWNLOAD_TIMEOUT_SECS),
                                     false, "Установка пакетов (один pkexec)"));
            let mut cleanup = Vec::new();
            if !use_de_helper {
                cleanup.push(tmp_sh.to_string());
            }
            cleanup.push(tmp_log.to_string());
            plan.steps.push(PlanStep::RemoveFiles { paths: cleanup });
        }
    }

    // ── 2. AUR packages (via yay/paru, without pkexec) ──
    if !aur_packages.is_empty() {
        if let Some(helper) = aur_helper {
            // A snap-pac post-hook of the finished pacman -S may hold the lock.
            plan.steps.push(wait_lock_step(family));
            // Privileges via the polkit window, the helper's questions answered in
            // advance: the window has no terminal (`aur_install::install_argv`).
            let argv = crate::aur_install::install_argv(helper, aur_packages, false);
            // None = by command class: the AUR helper BUILDS from source, and that is
            // measured in hours, not a half-minute default. Exactly here on 2026-09-01
            // the MATE desktop install died: nine packages were killed by a 30 s
            // timeout, and the person read "not installed".
            plan.steps.push(run_step(RunStepId::PkgAur, argv, None, false,
                                         "Установка AUR"));
        }
    }
    Ok(plan)
}

// ============================================================================
// DE orchestration: dm-enable / remove_de / switch_de / logout
// ============================================================================

/// (PACKAGE name, UNIT name) pairs of display managers: port of `dm_pairs`
/// from `install_de`. Usually the same; Plasma Login Manager (Arch, Plasma
/// 6.6+) is the exception: the package plasma-login-manager, the unit
/// plasmalogin. Order = priority.
pub const DM_PAIRS: &[(&str, &str)] = &[
    ("plasma-login-manager", "plasmalogin"),
    ("cosmic-greeter", "cosmic-greeter"),
    ("lightdm", "lightdm"),
    ("gdm", "gdm"),
    ("sddm", "sddm"),
    // Debian: the gdm3 package and unit (GNOME is installed with it there).
    ("gdm3", "gdm3"),
    ("lxdm", "lxdm"),
];

/// DM units requiring `systemctl enable --force` (they do not Provide
/// display-manager.service): port of `_DM_FORCE_ENABLE`.
pub const DM_FORCE_ENABLE: &[&str] = &["plasmalogin"];

/// UNIT NAME → PACKAGE NAME mapping: port of `_DM_PKG_FOR_UNIT`.
pub const DM_PKG_FOR_UNIT: &[(&str, &str)] = &[("plasmalogin", "plasma-login-manager")];

/// The step enabling the display manager after a DE install: port of the dm
/// branch of `install_de`. None if no DM was found in the package list, OR
/// the main packages went via rexon-de-helper (it already handled the login
/// screen in the same authorization session), OR an enabled login screen
/// already exists in the system. Returns (unit name, command with timeout 30).
///
/// ⚠️ `dm_present` (the `display_manager_present_from` answer) is exactly the
/// "do not touch someone else's login screen" rule: replacing a configured
/// DM is noticeable to the person and is not done along with a desktop
/// install. Our own DM may be installed only where there is no login screen
/// at all, otherwise there will be no graphical login after a reboot. One
/// rule for all paths: here, in rexon-de-helper and in `ensure_dm_plan`.
pub fn dm_enable_step(
    family: PkgFamily,
    installed_pkgs: &[String],
    used_de_helper: bool,
    pkg_helper_available: bool,
    dm_present: bool,
) -> Option<(String, Cmd)> {
    if used_de_helper || dm_present {
        return None;
    }
    for (dm_pkg, dm_unit) in DM_PAIRS {
        if installed_pkgs.iter().any(|p| p == dm_pkg) {
            let unit = (*dm_unit).to_string();
            let argv = match privileged_pkg_cmd(
                family, "enable-dm",
                std::slice::from_ref(&unit), pkg_helper_available,
            ) {
                Some(named) => named,
                None => {
                    // --force: PLM does not Provide display-manager.service.
                    let mut raw = vec![
                        "pkexec".to_string(), "systemctl".to_string(),
                        "enable".to_string(),
                    ];
                    if DM_FORCE_ENABLE.contains(dm_unit) {
                        raw.push("--force".to_string());
                    }
                    raw.push(format!("{dm_unit}.service"));
                    raw
                }
            };
            return Some((unit, Cmd::new(argv, Some(30), false)));
        }
    }
    None
}

/// The `remove_de` decision: 2-level protection against removing the active
/// DE (architecture rule #13: the backend refuses with force=false, the UI
/// must additionally lock the button).
pub enum RemoveDeDecision {
    /// Refusal: the error text.
    Refused(String),
    /// Removal allowed: the full package list (main + AUR, otherwise AUR
    /// extensions block the removal of the main ones through dependencies).
    Proceed(Vec<String>),
}

/// Port of the guard part of `remove_de`.
pub fn remove_de_decision(
    de_id: &str,
    display_name: &str,
    current_de: &str,
    force: bool,
    packages: &[String],
    aur: &[String],
) -> RemoveDeDecision {
    if !force && de_id == current_de {
        // A polite text without technical words ("DE", "logout"): previously it
        // bossed the user around with "switch" (fixed 13.09.2026).
        return RemoveDeDecision::Refused(trf(
            "You can't remove your current desktop (%1): after you log out \
             there would be nothing to log in to. Switch to another installed \
             desktop first.",
            &[&display_name],
        ));
    }
    if packages.is_empty() && aur.is_empty() {
        return RemoveDeDecision::Refused(
            tr("No packages for this distribution"),
        );
    }
    let mut all = packages.to_vec();
    all.extend(aur.iter().cloned());
    RemoveDeDecision::Proceed(all)
}

/// Guard part of `switch_de`: None means switching is allowed, Some is the error text.
pub fn switch_de_guard(
    current_de_name: &str,
    target_name: &str,
    target_display: &str,
) -> Option<String> {
    if current_de_name == "unknown" {
        return Some(tr("Couldn't detect your current desktop"));
    }
    if current_de_name == target_name {
        return Some(trf("Already in use: %1", &[&target_display]));
    }
    None
}

/// Session end commands: port of `logout_session` (chosen by environment;
/// the executor tries them in order until the first success, timeout 5 s each).
pub fn logout_cmds(
    xdg_current_desktop: &str,
    desktop_session: &str,
    xdg_session_id: &str,
) -> Vec<Cmd> {
    let de = xdg_current_desktop.to_lowercase();
    let session_env = desktop_session.to_lowercase();
    let mut cmds: Vec<Vec<&str>> = Vec::new();
    if de.contains("kde") || de.contains("plasma") || session_env.contains("plasma") {
        cmds.extend([
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.LogoutPrompt", "/LogoutPrompt",
                 "org.kde.LogoutPrompt.promptLogout"],
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.Shutdown", "/Shutdown",
                 "org.kde.Shutdown.logout"],
            vec!["dbus-send", "--session", "--type=method_call",
                 "--dest=org.kde.ksmserver", "/KSMServer",
                 "org.kde.KSMServerInterface.logout",
                 "int32:0", "int32:3", "int32:0"],
            vec!["qdbus6", "org.kde.Shutdown", "/Shutdown", "logout"],
        ]);
    } else if de.contains("xfce") || session_env.contains("xfce") {
        cmds.push(vec!["xfce4-session-logout", "--logout"]);
    } else if de.contains("mate") || session_env.contains("mate") {
        cmds.push(vec!["mate-session-save", "--logout"]);
    } else if de.contains("cinnamon") || session_env.contains("cinnamon") {
        cmds.push(vec!["cinnamon-session-quit", "--logout", "--no-prompt"]);
    } else if de.split(':').any(|t| t == "dde" || t == "deepin")
        || session_env.contains("deepin")
    {
        // Deepin v23/v25: logout without a question: the dde-session session
        // manager's `Logout` (`RequestLogout` would show a choice window).
        cmds.push(vec!["dbus-send", "--session", "--type=method_call",
                       "--dest=org.deepin.dde.SessionManager1",
                       "/org/deepin/dde/SessionManager1",
                       "org.deepin.dde.SessionManager1.Logout"]);
    } else if de.contains("gnome")
        || session_env.contains("gnome")
        // The Pantheon session is run by gnome-session (`Exec=gnome-session
        // --session=pantheon`), and it is what logs out.
        || de.contains("pantheon")
        || session_env.contains("pantheon")
    {
        cmds.push(vec!["gnome-session-quit", "--logout", "--no-prompt"]);
    } else if de.contains("cosmic") {
        cmds.push(vec!["cosmic-session", "--logout"]);
    } else if de.contains("hyprland") {
        cmds.push(vec!["hyprctl", "dispatch", "exit"]);
    } else if de.contains("lxqt") || session_env.contains("lxqt") {
        cmds.push(vec!["lxqt-leave", "--logout"]);
    }
    let mut out: Vec<Cmd> = cmds
        .into_iter()
        .map(|argv| Cmd::new(argv.into_iter().map(str::to_string).collect(),
                             Some(5), false))
        .collect();
    if !xdg_session_id.is_empty() {
        out.push(Cmd::new(
            vec!["loginctl".into(), "terminate-session".into(),
                 xdg_session_id.to_string()],
            Some(5), false,
        ));
    }
    out
}

// ============================================================================
// Polkit layer: named org.rexon.* actions, helper script and policy texts
// ============================================================================

pub const DE_INSTALL_ACTION: &str = "org.rexon.de-install";
pub const DE_HELPER_PATH: &str = "/usr/local/lib/rexon-de-helper";
pub const DE_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.de-install.policy";
pub const REXON_POLKIT_RULES_PATH: &str =
    "/etc/polkit-1/rules.d/49-rexon-nopasswd.rules";
pub const PKG_ACTION: &str = "org.rexon.pkg";
pub const PKG_HELPER_PATH: &str = "/usr/local/lib/rexon-pkg-helper";
pub const PKG_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.pkg.policy";
pub const STAGE_ACTION: &str = "org.rexon.stage";
pub const STAGE_HELPER_PATH: &str = "/usr/local/lib/rexon-stage-helper";
pub const STAGE_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.stage.policy";

// ── Window privilege session ("one password per launch", option "B", item B11) ──
//
// The [`crate::privilege`] helper action: the window brings it up with one
// `pkexec` per session. ⛔ `auth_admin` WITHOUT `_keep`: the password is held
// by the helper itself (it lives while the window is open), not by polkit:
// polkit's five-minute window would hand a second session to any Rexon
// process without a password. The action ⛔ is not in
// [`PASSWORDLESS_ACTIONS`], ⛔ is not in anyone else's `imply` and ⛔ does not
// `imply` anything itself: there is no silent chain to the session from
// any side. Guarded by `session_action_tests`.
pub const SESSION_ACTION: &str = "org.rexon.session";
/// Session helper name: the same as the client's ([`crate::privilege::client::HELPER_NAME`]).
pub const SESSION_HELPER_NAME: &str = crate::privilege::client::HELPER_NAME;
/// Session helper path for a MANUAL install (the package puts it into
/// [`HELPER_DIR_PACKAGE`]; the client searches [`HELPER_DIRS`] and checks
/// ownership itself).
pub const SESSION_HELPER_PATH: &str = "/usr/local/lib/rexon-priv-helper";
pub const SESSION_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.session.policy";

// ── Where the helpers physically live: ONE truth for two layouts ─────
//
// There are two layouts, and both are live:
//   * the distribution package puts the helpers into [`HELPER_DIR_PACKAGE`]
//     (`/usr/lib`): Arch rules forbid a package to write into `/usr/local`,
//     and `pacman -R` would not remove files outside its database;
//   * the manual install (`installer_exec`) puts them into
//     [`HELPER_DIR_SYSTEM`] (`/usr/local`), as before.
//
// ⚠️ The defect this closes: the path was nailed to `/usr/local/lib` in one
// line, the package had no helpers at all, and for a person with the
// package EVERY system operation asked for the password via raw `pkexec`
// (the "one password per session" option never kicked in).
//
// There is one truth: the helper name + a list of directories. The
// `*_HELPER_PATH` constants below remain the MANUAL install paths: the
// installer puts files by them and writes its policy's `exec.path` by them.
// Where to look at run time is answered by [`de_helper_path`],
// [`pkg_helper_path`] and [`stage_helper_path`].

/// Name of the desktop install helper.
pub const DE_HELPER_NAME: &str = "rexon-de-helper";
/// Name of the package manager operations helper.
pub const PKG_HELPER_NAME: &str = "rexon-pkg-helper";
/// Name of the helper placing files into `/usr/share`.
pub const STAGE_HELPER_NAME: &str = "rexon-stage-helper";

/// Helper directory of the distribution package.
pub const HELPER_DIR_PACKAGE: &str = "/usr/lib";
/// Helper directory of a manual system install.
pub const HELPER_DIR_SYSTEM: &str = "/usr/local/lib";

/// Helper directories in descending preference: the package before the
/// manual install: its files were placed by the package manager, and its
/// policy is in `/usr/share/polkit-1/actions`.
pub const HELPER_DIRS: [&str; 2] = [HELPER_DIR_PACKAGE, HELPER_DIR_SYSTEM];

/// Full helper path in the given directory.
pub fn helper_path_in(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// All places where the helper may be, in descending preference.
pub fn helper_candidates(name: &str) -> Vec<String> {
    HELPER_DIRS.iter().map(|d| helper_path_in(d, name)).collect()
}

/// Choose the helper path by probing the file system: the first executable
/// candidate, and if there is none, the manual install path (so that the
/// path in messages and the log stays defined).
///
/// The probe comes from outside: the decision stays pure and testable.
pub fn pick_helper_path(name: &str, is_exec: &dyn Fn(&str) -> bool) -> String {
    helper_candidates(name)
        .into_iter()
        .find(|p| is_exec(p))
        .unwrap_or_else(|| helper_path_in(HELPER_DIR_SYSTEM, name))
}

/// The live answer for this machine, computed once per process.
fn helper_path_live(slot: &'static std::sync::OnceLock<String>, name: &str) -> &'static str {
    slot.get_or_init(|| pick_helper_path(name, &|p| is_exec_file(p)))
}

/// An existing file with the exec bit. Public, so that the "helper is
/// deployed" check asks exactly the same as the path choice.
pub fn is_exec_file(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Where the desktop install helper lives on this machine.
pub fn de_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, DE_HELPER_NAME)
}

/// Where the package manager helper lives on this machine.
pub fn pkg_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, PKG_HELPER_NAME)
}

/// Where the file placing helper lives on this machine.
pub fn stage_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    helper_path_live(&SLOT, STAGE_HELPER_NAME)
}

/// Rewrite the helper path in a reference text to the one THIS machine
/// chooses.
///
/// ⚠️ Why. The helpers have two directories, and both are legitimate
/// ([`HELPER_DIRS`]): the distribution package puts them into `/usr/lib`, the
/// manual install into `/usr/local/lib`, and [`pick_helper_path`] chooses by
/// probing the file system. The golden check references were taken from live
/// Python on a machine without the package and hold `/usr/local/lib`. It was
/// enough to install our own package, and 16 checks turned red at once
/// (18.09.2026), not on a defect but on the machine state: green before the
/// install, red after. Exactly such a mine must not exist: a check that
/// depends on the live machine must either not depend on it or be marked
/// `isolated-checks` (the agreement in `rust/.cargo/config.toml`).
///
/// What the pin checks does not change: "we go through a NAMED helper, not
/// raw `pkexec`, and with exactly these arguments". A raw fallback
/// (`pkexec pacman -S …`) and any change to the argument set still fail the
/// check: ONLY the helper directory is substituted, and only with the second
/// legitimate one. On a machine without the package the substitution is the
/// identity: the live answer is `/usr/local/lib`.
pub fn retarget_helper_paths(text: &str) -> String {
    let mut out = text.to_string();
    for (name, live) in [
        (DE_HELPER_NAME, de_helper_path()),
        (PKG_HELPER_NAME, pkg_helper_path()),
        (STAGE_HELPER_NAME, stage_helper_path()),
    ] {
        let manual = helper_path_in(HELPER_DIR_SYSTEM, name);
        if manual != live {
            out = out.replace(&manual, live);
        }
        out = insert_live_lang_arg(&out, live);
    }
    out
}

/// Insert, after the helper path in a reference `pkexec` command (a JSON
/// array `"pkexec", "<helper>", …`), the language argument THIS machine would
/// pass with the current UI language ([`crate::system_l10n::lang_args`]).
///
/// ⚠️ Why. `--lang=<code>` goes to the helper only if the language is not
/// English AND the installed helper file carries the "I understand `--lang`"
/// mark. The references were taken where there was no such helper, and it
/// was enough to install our own package with new helpers for the command
/// checks to turn red on the machine state, not on a defect (04.10.2026; the
/// same class as with the helper directory above). The pin still catches a
/// raw `pkexec` and any change to the other arguments; ONLY what depends on
/// the machine is substituted.
///
/// The current language is used: the check must pin it (`i18n_pin`) BEFORE
/// parsing the reference.
fn insert_live_lang_arg(text: &str, live: &str) -> String {
    let args = crate::system_l10n::lang_args(live);
    if args.is_empty() {
        return text.to_string();
    }
    let needle = format!("\"{live}\",");
    let extra: String = args.iter().map(|a| format!(" \"{a}\",")).collect();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(&needle) {
        let (head, tail) = rest.split_at(at + needle.len());
        out.push_str(head);
        if head[..at].trim_end().ends_with("\"pkexec\",") {
            out.push_str(&extra);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Families for which the DE helper can install from official repos without the AUR.
pub const DE_HELPER_FAMILIES: &[&str] = &[
    "arch", "debian", "fedora", "suse", "alpine", "void", "solus", "clear", "alt",
];

/// DE helper install command per family ({PKGS} is substituted by the helper itself).
const DE_HELPER_PKGCMD: &[(&str, &str)] = &[
    ("arch", "pacman -S --needed --noconfirm --noprogressbar --color never {PKGS}"),
    ("debian", "apt-get install -y --no-install-recommends {PKGS}"),
    ("fedora", "dnf install -y {PKGS}"),
    ("suse", "zypper install -y --no-recommends {PKGS}"),
    ("alpine", "apk add --no-progress {PKGS}"),
    ("void", "xbps-install -y {PKGS}"),
    ("solus", "eopkg it -y {PKGS}"),
    ("clear", "swupd bundle-add {PKGS}"),
    ("alt", "apt-get install -y {PKGS}"),
];

/// DE helper update command per family.
const DE_HELPER_UPDCMD: &[(&str, &str)] = &[
    ("arch", "pacman -Sy --noconfirm --noprogressbar --color never"),
    ("debian", "apt-get update -q"),
    ("fedora", "dnf makecache -q"),
    ("suse", "zypper refresh -q"),
    ("alpine", "apk update"),
    ("void", "xbps-install -S"),
    ("solus", "eopkg ur"),
    ("clear", "swupd update --download"),
    ("alt", "apt-get update -q"),
];

/// DMs the helpers can enable: port of `_DE_HELPER_DISPLAY_MANAGERS`.
pub const DE_HELPER_DISPLAY_MANAGERS: &[&str] = &[
    "plasmalogin", "cosmic-greeter", "lightdm", "gdm", "sddm",
];

/// rexon-pkg-helper operations: port of `_PKG_HELPER_OPS`.
///
/// `build-deps` and `install-built` are the rebuild steps of the Arch recipe
/// ([`crate::repo_rebuild`]); it builds their argv itself, [`privileged_pkg_cmd`]
/// does not assemble them.
pub const PKG_HELPER_OPS: &[&str] = &[
    "install", "remove", "update", "enable-dm", "switch-dm", "build-deps", "install-built",
];

/// rexon-pkg-helper families (NIX deliberately excluded): port of
/// `_PKG_HELPER_FAMILIES`.
pub const PKG_HELPER_FAMILIES: &[&str] = &[
    "arch", "debian", "fedora", "suse", "gentoo",
    "alpine", "void", "solus", "clear", "alt",
];

const PKG_HELPER_INSTALL: &[(&str, &str)] = &[
    ("arch", "pacman -S --needed --noconfirm {PKGS}"),
    ("debian", "apt-get install -y --no-install-recommends {PKGS}"),
    ("fedora", "dnf install -y {PKGS}"),
    ("suse", "zypper install -y --no-recommends {PKGS}"),
    ("gentoo", "emerge --ask=n --quiet {PKGS}"),
    ("alpine", "apk add --no-progress {PKGS}"),
    ("void", "xbps-install -y {PKGS}"),
    ("solus", "eopkg it -y {PKGS}"),
    ("clear", "swupd bundle-add {PKGS}"),
    ("alt", "apt-get install -y {PKGS}"),
];
const PKG_HELPER_REMOVE: &[(&str, &str)] = &[
    ("arch", "pacman -Rns --noconfirm {PKGS}"),
    ("debian", "apt-get remove -y --purge {PKGS}"),
    ("fedora", "dnf remove -y {PKGS}"),
    ("suse", "zypper remove -y {PKGS}"),
    ("gentoo", "emerge --unmerge {PKGS}"),
    ("alpine", "apk del {PKGS}"),
    ("void", "xbps-remove -y {PKGS}"),
    ("solus", "eopkg rm -y {PKGS}"),
    ("clear", "swupd bundle-remove {PKGS}"),
    ("alt", "apt-get remove -y {PKGS}"),
];
const PKG_HELPER_UPDATE: &[(&str, &str)] = &[
    ("arch", "pacman -Sy --noconfirm"),
    ("debian", "apt-get update -q"),
    ("fedora", "dnf makecache -q"),
    ("suse", "zypper refresh -q"),
    ("gentoo", "emerge --sync --quiet"),
    ("alpine", "apk update"),
    ("void", "xbps-install -S"),
    ("solus", "eopkg ur"),
    ("clear", "swupd update --download"),
    ("alt", "apt-get update -q"),
];

/// File staging categories: port of `_STAGE_CATEGORIES`.
pub const STAGE_CATEGORIES: &[&str] = &[
    "gdm-bg", "lightdm-bg", "cosmic-greeter", "login-bg", "lightdm-webkit", "mate-layouts",
];

/// Destination root directories of the dir categories: port of `_STAGE_DIR_ROOT`.
///
/// ⚠️ Rule #16: the webkit greeter theme path was wrong EVEN on Arch: the
/// `lightdm-webkit2-greeter` package puts themes into
/// `/usr/share/lightdm-webkit`, a `lightdm-webkit2-greeter` directory exists
/// nowhere. The theme went into a nonexistent directory, and the user got
/// "installed". The greeter's successor (`web-greeter`) keeps themes in its
/// own place, so there are two roots and the one that really exists in the
/// system is chosen.
pub const STAGE_DIR_ROOT: &[(&str, &str)] = &[
    ("lightdm-webkit", "/usr/share/lightdm-webkit/themes"),
    ("mate-layouts", "/usr/share/mate-panel/layouts"),
];

/// The second webkit greeter themes root: the `web-greeter` successor.
pub const WEB_GREETER_ROOT: &str = "/usr/share/web-greeter/themes";

/// All 8 Ubuntu MATE layouts: port of `MATE_LAYOUTS`.
pub const MATE_LAYOUTS: &[&str] = &[
    "familiar", "redmond", "cupertino", "mutiny",
    "pantheon", "contemporary", "netbook", "traditional",
];

// ── A package name is a NAME, not a path: ONE check for all paths ───
//
// ⛔ HOLE CLOSED ON 25.09.2026. The two former checks (`is_safe_pkg_name`
// here and `is_safe_helper_arg` below) allowed the `/` character in a
// package name:
//
//     s.chars().all(|c| c.is_ascii_alphanumeric()
//                       || matches!(c, '@'|'.'|'_'|'+'|':'|'/'|'-'))
//
// and the privileged helper assembled the command via `eval`. Together this
// meant: `pkexec rexon-pkg-helper debian install /tmp/evil.deb`: apt-get
// (and likewise dnf, zypper) installs a LOCAL FILE, and its install scripts
// run AS ROOT. The same `/` also slipped into the raw fallback
// (`pkexec apt-get install …`), so the check sits in the command tables
// themselves ([`install_cmd`], [`remove_cmd`]), not only before the helper.
//
// There is one truth here too: the package name atom. A path cannot be
// composed into it in any way, and the place where a path is REALLY needed
// (placing look files) is a separate `org.rexon.stage` action with its own
// category and root check, not an exception in the common name.

/// Package name atom: starts with a letter or digit, followed only by
/// `A-Za-z0-9._+:-`. No `/` (a path), no leading `-` (swapping a package for
/// a flag), no leading dot, no `..`, no package file suffix.
fn is_pkg_atom(s: &str) -> bool {
    match s.chars().next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    !s.contains("..")
        && !looks_like_package_file(s)
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | ':' | '-')
        })
}

/// A package or archive file suffix. Needed SEPARATELY from the `/` ban:
/// `dnf install evil.rpm` and `zypper install evil.rpm` take the file from
/// the CURRENT directory, and the helper's current directory is set by the
/// calling process (pkexec does not change the working directory): such an
/// argument has no slash at all.
fn looks_like_package_file(s: &str) -> bool {
    const EXT: &[&str] = &[
        ".deb", ".rpm", ".apk", ".eopkg", ".xbps", ".tar", ".tgz", ".txz",
        ".zst", ".xz", ".gz", ".bz2", ".lz4", ".snap", ".flatpakref",
    ];
    let low = s.to_ascii_lowercase();
    EXT.iter().any(|e| low.ends_with(e))
}

/// A valid package name for a privileged operation, adjusted for the family,
/// and in exactly two ways:
///   * Fedora: a leading `@` is a dnf group (`@gnome-desktop`), and it is the
///     only legitimate `@` in the whole project;
///   * Gentoo: a Portage atom `category/name`: EXACTLY one `/`, both halves
///     atoms themselves. No leading `/` (an absolute path), no `@` (sets like
///     `@world`, with which `emerge --unmerge` would wipe the system).
/// The other families need `/` nowhere, and it is forbidden.
pub fn is_safe_pkg_arg(family: PkgFamily, s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let body = if family == PkgFamily::Fedora {
        s.strip_prefix('@').unwrap_or(s)
    } else {
        s
    };
    if family == PkgFamily::Gentoo {
        if let Some((category, name)) = body.split_once('/') {
            return !name.contains('/') && is_pkg_atom(category) && is_pkg_atom(name);
        }
    }
    is_pkg_atom(body)
}

/// Refusal text for removing a vital package ([`is_vital_package`]): one for
/// all removal paths.
pub fn vital_remove_error(name: &str) -> String {
    trf("Rexon does not remove %1: the system needs it to work", &[&name])
}

/// Refusal text for a name that is not a name: one for all paths, so that the
/// person sees the same reason both in the plan and in the log.
fn bad_pkg_name_error(name: &str) -> String {
    trf(
        "Invalid package name: %1. A package name is expected, not a file path.",
        &[&name],
    )
}

/// Vital packages: the helper will never remove them.
///
/// ⛔ The second half of the same hole: `remove` checked the name ONLY for
/// characters, while it runs as root and with "do not ask". One line
/// `pkexec rexon-pkg-helper arch remove linux` was enough to leave the
/// machine unbootable, and `… remove polkit` to take away the confirmation
/// mechanism itself. None of these names is in any desktop of the Rexon
/// database (the check
/// `вечный_список_не_пересекается_с_рабочими_столами`),
/// so the ban does not get in the way of a single honest scenario.
pub const PKG_HELPER_VITAL: &[&str] = &[
    "base", "base-files", "filesystem", "glibc", "gcc-libs", "bash",
    "coreutils", "util-linux", "systemd", "systemd-libs",
    "systemd-sysvcompat", "dbus", "dbus-broker",
    "linux", "linux-lts", "linux-zen", "linux-hardened", "linux-firmware",
    "kernel", "kernel-core", "grub", "limine", "systemd-boot",
    "pacman", "apt", "dpkg", "rpm", "dnf", "yum", "zypper", "apk-tools",
    "xbps", "eopkg", "swupd-client", "portage", "emerge",
    "polkit", "polkit-qt5", "sudo", "openssl", "zlib", "rexon",
];

/// An addition to [`PKG_HELPER_VITAL`]: families that cannot be listed by
/// exact names: kernels (`linux-cachyos`, `linux-image-…`, `kernel-modules`),
/// video drivers (`nvidia-…`, `mesa`, `vulkan-…`, `xf86-video-…`), firmware
/// and microcode, initramfs builders.
///
/// ⛔ The hole this closed (28.10.2026): the permanent list knew only exact
/// names (`linux`, `linux-lts`, `linux-zen`, `linux-hardened`), and
/// `rexon-pkg-helper arch remove linux-cachyos` (as well as `nvidia-utils`,
/// `mesa`, `amd-ucode`, `linux-image-amd64`) went through. No name from the
/// desktop tables falls in here (the check `вечное_не_задевает_столы`), so
/// the addition does not get in the way of desktop removal.
pub const VITAL_PREFIXES: &[&str] = &[
    "linux-", "kernel-", "gentoo-kernel", "gentoo-sources", "systemd-", "glibc-",
    "grub-", "sudo-", "opendoas", "polkit-qt", "pacman-", "nvidia", "lib32-nvidia",
    "mesa", "lib32-mesa", "vulkan-", "lib32-vulkan-", "xf86-video-", "firmware-",
    "mkinitcpio", "dracut", "booster",
];
/// Name suffixes of vital packages: firmware, microcode, DKMS modules.
pub const VITAL_SUFFIXES: &[&str] = &["-firmware", "-ucode", "-dkms"];

/// The name for matching against the permanent list: without the dnf group
/// (`@`) and without the Gentoo category (`sys-kernel/gentoo-kernel` →
/// `gentoo-kernel`), like `vital_key` in `rexon-pkg-helper`.
pub fn vital_key(name: &str) -> &str {
    let n = name.strip_prefix('@').unwrap_or(name);
    n.rsplit('/').next().unwrap_or(n)
}

/// A vital package: from [`PKG_HELPER_VITAL`] or from the kernel, driver,
/// firmware and boot families ([`VITAL_PREFIXES`], [`VITAL_SUFFIXES`]). ONE
/// rule for all removal paths: the package helper (the same list in its bash
/// text), the removal plan [`remove_plan`], [`remove_cmd`], removing packages
/// of «Distro looks» and the checks of helper "B".
pub fn is_vital_package(name: &str) -> bool {
    let key = vital_key(name);
    PKG_HELPER_VITAL.contains(&key)
        || VITAL_PREFIXES.iter().any(|p| key.starts_with(p))
        || VITAL_SUFFIXES.iter().any(|s| key.ends_with(s))
}

/// The same rule as a `case` pattern for the bash helper: exact names,
/// `prefix*`, `*suffix`. The names come from core constants (letters,
/// digits, `-`), none of them contains a pattern character (checked in tests).
fn vital_case_pattern() -> String {
    PKG_HELPER_VITAL
        .iter()
        .map(|v| v.to_string())
        .chain(VITAL_PREFIXES.iter().map(|p| format!("{p}*")))
        .chain(VITAL_SUFFIXES.iter().map(|s| format!("*{s}")))
        .collect::<Vec<_>>()
        .join("|")
}

/// Whitelist of the package helper's `install` operation for a family: all
/// core tables by which Rexon installs packages through the helper: desktops
/// (main and AUR), login screens ([`DM_PAIRS`], [`SDDM_PKG_FOR_FAMILY`]),
/// compliance repair, wallpaper video dependencies, yt-dlp, the Gentoo
/// overlay tool, shells, Conky widget programs and the 7z unpacker
/// (`utils::TOOL_PACKAGES`), Conky, parts of the MATE layouts, theme icons
/// and cursors, «Distro looks» images. Embedded into the helper text
/// ([`pkg_helper_script`]): the root helper does not install a name outside
/// the list.
///
/// ⛔ The hole this closed (28.10.2026): `install` accepted any package name,
/// and any session process with the administrator password for 5 minutes
/// (`auth_admin_keep`) could install anything from the enabled repositories,
/// with install scripts running as root.
///
/// ⚠️ A new place that installs packages through the helper must take the
/// names from a table and add that table here, otherwise the helper refuses.
/// The check `белый_список_покрывает_все_таблицы` keeps the inventory.
pub fn pkg_helper_install_allowlist(family: PkgFamily) -> Vec<String> {
    let fam = family.as_str();
    let mut out: Vec<String> = Vec::new();
    let mut add = |p: &str| {
        if is_safe_pkg_arg(family, p) && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    };
    for (de_id, _) in crate::system::DE_DATABASE {
        for p in crate::system::desktop_packages(de_id, family) {
            add(&p);
        }
    }
    for (p, _) in DM_PAIRS {
        add(p);
    }
    if let Some(p) = table_get(SDDM_PKG_FOR_FAMILY, fam) {
        add(p);
    }
    for list in [
        crate::compliance::AYATANA_PACKAGES,
        crate::compliance::MATE_AUR_PACKAGES,
        crate::compliance::FIX_PACKAGES,
    ] {
        for p in list {
            add(p);
        }
    }
    for session in ["wayland", "x11"] {
        for p in crate::wallpaper_engine::install_deps_packages(fam, session) {
            add(&p);
        }
    }
    add(crate::youtube_wallpaper::YTDLP_PACKAGE);
    if family == PkgFamily::Gentoo {
        add(GENTOO_OVERLAY_TOOL_PACKAGE);
    }
    for p in crate::shell_bundle::all_packages(family) {
        add(p);
    }
    for p in crate::utils::tool_packages_for(fam) {
        add(p);
    }
    if let Some(p) = crate::de_apply::conky_package(fam) {
        add(p);
    }
    for p in crate::mate_panel::all_part_packages(fam) {
        add(p);
    }
    for p in crate::themes::known_set_packages(fam) {
        add(p);
    }
    for p in crate::distro_looks::builtin_package_names(family) {
        add(&p);
    }
    out
}

/// `case "$family"` branches of the install whitelist for the helper text.
fn install_allow_cases() -> String {
    PKG_HELPER_FAMILIES
        .iter()
        .filter_map(|f| PkgFamily::from_value(f))
        .map(|fam| {
            format!(
                "        {}) list=\"{}\" ;;",
                fam.as_str(),
                pkg_helper_install_allowlist(fam).join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Desktop packages for protecting the current desktop in the helper:
/// `"<de>|<family>") echo …`: main and AUR by the core table.
fn desktop_protect_cases() -> String {
    let mut lines: Vec<String> = Vec::new();
    for (de_id, _) in crate::system::DE_DATABASE {
        if !is_safe_de_id(de_id) {
            continue;
        }
        for f in PKG_HELPER_FAMILIES {
            let Some(fam) = PkgFamily::from_value(f) else { continue };
            let pkgs: Vec<String> = crate::system::desktop_packages(de_id, fam)
                .into_iter()
                .filter(|p| is_safe_pkg_arg(fam, p))
                .collect();
            if !pkgs.is_empty() {
                lines.push(format!("        \"{de_id}|{f}\") echo {} ;;", pkgs.join(" ")));
            }
        }
    }
    lines.join("\n")
}

/// The desktop by the logind session's `Desktop=`: the same table as
/// [`crate::system::detect_current_de`]: the exact token, then a substring.
fn desktop_token_cases() -> (String, String) {
    let table = crate::system::XDG_TOKEN_TO_DE;
    let exact = table
        .iter()
        .map(|(k, de)| format!("            {k}) printf '%s' {de}; return 0 ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    let substr = table
        .iter()
        .map(|(k, de)| format!("        *{k}*) printf '%s' {de}; return 0 ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    (exact, substr)
}

/// Login screen package by unit for the helper (`family|unit`): Debian uses
/// [`DM_DEBIAN_PKG_FOR_UNIT`], the others [`DM_PKG_FOR_UNIT`].
fn dm_protect_map() -> String {
    DM_DEBIAN_PKG_FOR_UNIT
        .iter()
        .map(|(u, p)| format!("        debian\\|{u}) printf '%s\\n' {p} ;;"))
        .chain(DM_PKG_FOR_UNIT.iter().map(|(u, p)| format!("        *\\|{u}) printf '%s\\n' {p} ;;")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_safe_de_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Actions that, with the person's CONSENT, go without a repeated password
/// (the [`passwordless_rules_js`] rule, installed by the Rexon system install
/// only when consent is given).
///
/// ⭐⭐ Fixed 15.09.2026: the old rule (`rexon_polkit_rules_js`, removed on
/// 25.09.2026 together with its generator, see below) silently gave away the
/// WHOLE `org.rexon.*` family, including `org.rexon.pkg`: installing and
/// REMOVING any packages by name. Any session process could have done this
/// without a password (`pkexec rexon-pkg-helper arch remove linux`). Now one
/// action is allowed that takes no arbitrary input:
///   * `org.rexon.de-install`: installing a desktop only from the core table
///     (the package set is built into the helper, a package name is not accepted).
///
/// ⛔ 28.09.2026 (Wehrwolfmann's decision on the "one password" plan): removed
/// from the rule were `org.rexon.system-install` (SDDM and Plymouth themes)
/// and `org.rexon.stage` (login screen backgrounds and themes, MATE layouts).
/// Both take CONTENT from the user's directory and put it into `/usr/share`,
/// and a Plymouth theme is loaded as root at boot: plymouth turns the
/// `ModuleName` line into a module path without checking `../`, and a theme
/// with a `.so` inside would be code running as root, without a password,
/// from any session process. The theme helper now rejects this, but
/// "without password" will not come back for these actions: after moving to
/// "one password per launch" the rule is removed entirely.
///
/// `org.rexon.pkg` is NEVER included here and asks for the password as usual
/// (`auth_admin_keep`: five minutes per window process). It is also the
/// "umbrella" ([`PKG_IMPLIES`]): the `imply` direction is only from it to
/// the others.
///
/// ⛔ On 25.09.2026 the old rule's generator was removed completely. It had
/// stayed as dead code (Rust did not call it, Python wrote a file that the
/// core immediately removed with `rm -f` when there was no consent), but it
/// lay there as a ready mine: one call, and `org.rexon.pkg` goes without a
/// password. It must not be restored in any form: the "without password"
/// rule knows ONLY the [`PASSWORDLESS_ACTIONS`] list.
pub const PASSWORDLESS_ACTIONS: &[&str] = &[DE_INSTALL_ACTION];

/// Administrator groups the distribution's polkit allows administrator
/// actions to (`addAdminRule` / `AdminIdentities`).
///
/// ⭐ Fixed 15.09.2026: the rule knew only `wheel`, while on Debian and
/// Ubuntu administrators are in the `sudo` group (on old Ubuntu also
/// `admin`), on Solus also `sudo`. There the rule never fired. An unknown
/// family gets `wheel`: asking for the password once more is safer than
/// guessing someone else's group.
pub fn polkit_admin_groups(family: PkgFamily) -> &'static [&'static str] {
    match family {
        PkgFamily::Debian => &["sudo", "admin"],
        PkgFamily::Solus => &["sudo"],
        _ => &["wheel"],
    }
}

/// The polkit "without repeated password" rule for [`PASSWORDLESS_ACTIONS`]:
/// an administrator (by the family's [`polkit_admin_groups`]) in an active
/// local session. Placed at [`REXON_POLKIT_RULES_PATH`], the same path as
/// the old one, so removing Rexon removes it.
pub fn passwordless_rules_js(family: PkgFamily) -> String {
    let actions = PASSWORDLESS_ACTIONS
        .iter()
        .map(|a| format!("\"{a}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let groups = polkit_admin_groups(family)
        .iter()
        .map(|g| format!("subject.isInGroup(\"{g}\")"))
        .collect::<Vec<_>>()
        .join(" || ");
    format!(
        "// Rexon — no repeated password only for the listed actions\n\
         // (an administrator in an active local session). Installed with consent\n\
         // when installing Rexon. Installing and removing packages is not included.\n\
         var rexonActions = [{actions}];\n\
         polkit.addRule(function(action, subject) {{\n\
         \x20   if (rexonActions.indexOf(action.id) >= 0 &&\n\
         \x20       subject.active && subject.local &&\n\
         \x20       ({groups})) {{\n\
         \x20       return polkit.Result.YES;\n\
         \x20   }}\n\
         }});\n"
    )
}

/// bash case table `de_id|family → packages`: port of `_de_helper_pkg_table`.
/// THE SINGLE SOURCE OF TRUTH is DE_DATABASE (see system.rs), the helper does
/// not "drift" from the main database. Names are safe (allowlist) →
/// shlex.quote in Python is a no-op.
pub fn de_helper_pkg_table() -> String {
    let mut lines: Vec<String> = Vec::new();
    for (de_id, packages) in crate::system::de_database_packages() {
        if !is_safe_de_id(de_id) || !de_helper_allowed(de_id) {
            continue;
        }
        for (fam, pkgs) in packages {
            let fam_v = fam.as_str();
            if !DE_HELPER_FAMILIES.contains(&fam_v) {
                continue;
            }
            let clean: Vec<&str> = pkgs
                .iter()
                .filter(|p| is_safe_pkg_arg(*fam, p))
                .copied()
                .collect();
            if clean.is_empty() {
                continue;
            }
            lines.push(format!(
                "        \"{de_id}|{fam_v}\") echo {} ;;",
                clean.join(" ")
            ));
        }
    }
    lines.join("\n")
}

/// Desktops that are not in the helper table at all. Deepin: the `deepin`
/// group pulls in `deepin-session` with `deepin-kwin`, while Rexon installs
/// Deepin only on Treeland: as a set without them, via the wrapper
/// (`crate::deepin_treeland`). The root helper must not be able to install
/// the whole group.
fn de_helper_allowed(de_id: &str) -> bool {
    de_id != crate::deepin_treeland::DE_ID
}

/// Whether the DE helper can install the de_id|family pair: port of `_de_helper_supports`.
pub fn de_helper_supports(de_id: &str, family: &str) -> bool {
    if !DE_HELPER_FAMILIES.contains(&family) || !de_helper_allowed(de_id) {
        return false;
    }
    if !is_safe_de_id(de_id) {
        return false;
    }
    let fam = match PkgFamily::from_value(family) {
        Some(f) => f,
        None => return false,
    };
    match de_entry_packages_for(de_id, family) {
        Some(pkgs) => pkgs.iter().any(|p| is_safe_pkg_arg(fam, p)),
        None => false,
    }
}

/// Whether the helper will install everything from `packages`: it takes the
/// desktop packages from the built-in table ([`de_helper_pkg_table`]), not
/// from the arguments. A set of names from that table (all or part, as it
/// always was) it covers; a name not from the table (a DKMS module, an
/// expanded group member, an AUR package from a repository) the helper would
/// silently not install.
pub fn de_helper_covers(de_id: &str, family: PkgFamily, packages: &[String]) -> bool {
    let Some(table) = de_entry_packages_for(de_id, family.as_str()) else {
        return false;
    };
    packages
        .iter()
        .all(|p| table.iter().any(|t| *t == p.as_str() && is_safe_pkg_arg(family, t)))
}

/// Body of rexon-de-helper: port of `_de_helper_script` (template + substitutions).
pub fn de_helper_script() -> String {
    let pkg_table = de_helper_pkg_table();
    // Commands as ARRAYS of arguments: `eval` was removed from the helper
    // entirely (25.09.2026, together with the hole in rexon-pkg-helper).
    let fam_cases = DE_HELPER_FAMILIES
        .iter()
        .map(|f| {
            let pkg = fam_cmd_case(DE_HELPER_PKGCMD, f, "    ");
            let upd = fam_cmd_case(DE_HELPER_UPDCMD, f, "    ");
            format!(
                "{}\n{}",
                pkg.replace("cmd=(", "PKG_CMD=("),
                upd.replace("cmd=(", "UPD_CMD=("),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let dm_cases = DE_HELPER_DISPLAY_MANAGERS
        .iter()
        .map(|dm| format!("\"{dm}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let dm_pkgmap = DM_PKG_FOR_UNIT
        .iter()
        .map(|(unit, pkg)| format!("                {unit}) printf '%s' '{pkg}' ;;"))
        .collect::<Vec<_>>()
        .join("\n");
    let dm_force = DM_FORCE_ENABLE
        .iter()
        .map(|dm| format!("\"{dm}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    let dm_list = DE_HELPER_DISPLAY_MANAGERS.join(" ");
    // The list of known login screens and "enabled" states is the same one
    // Rexon uses to answer "is there a login screen?": the helper must see a
    // foreign DM in exactly the same way, otherwise it would silently replace it.
    let dm_known = KNOWN_DM_UNITS.join(" ");
    let dm_states = DM_ENABLED_STATES.join("|");
    let text = DE_HELPER_SCRIPT_TMPL
        .replace("__PKG_TABLE__", &pkg_table)
        .replace("__FAM_CASES__", &fam_cases)
        .replace("__DM_LIST__", &dm_list)
        .replace("__DM_PKGMAP__", &dm_pkgmap)
        .replace("__DM_FORCE__", &dm_force)
        .replace("__DM_CASES__", &dm_cases)
        .replace("__DM_KNOWN__", &dm_known)
        .replace("__DM_STATES__", &dm_states)
        .replace("__DM_SYMLINK__", DM_SYMLINK);
    crate::system_l10n::localize_script(DE_HELPER_NAME, &text)
}

/// The rexon-de-helper template (text for text with Python's `_de_helper_script`).
const DE_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon DE helper — privileged install/repair of a DE.
# Run EXCLUSIVELY through pkexec (polkit action org.rexon.de-install,
# auth_admin_keep: a confirmed password is kept for a few minutes for the
# Rexon window). Accepts only strictly validated de_id + family (+ opt.
# log_path under /tmp). Package names come from the EMBEDDED table. Does NOT run arbitrary code.
set -euo pipefail

# A stable C locale: pacman/apt/… output is always in English, so that
# the progress parser (_RE_SKIP «is up to date -- skipping» etc.)
# catches skips/progress on ANY system locale (otherwise on a Russian one
# pacman writes «не устарел -- пропускается» and «✓ уже установлено»
# does not fire).
export LC_ALL=C LANG=C

# ⚠️ No keyboard input. The package manager asks itself questions
# (group members, provider choice) and with --noconfirm answers them by
# default without reading stdin. But if one day --noconfirm gets lost in
# some branch while stdin turns out alive (the terminal Rexon was started from,
# or an unclosed pipe), the question will HANG — silently, up to the
# half-hour timeout. We cut stdin off once here: then the worst that can happen
# is a quick refusal instead of waiting forever.
exec </dev/null

__L10N__

[ "$#" -ge 2 ] && [ "$#" -le 3 ] \
    || die de_args
de_id="$1"
family="$2"
log_path="${3:-}"

# de_id / family — a strict format (a character allowlist). The real
# validity check of the pair is the table below (unknown pair → die).
printf '%s' "$de_id"  | grep -Eq '^[A-Za-z0-9._-]+$' || die bad_de_id "$de_id"
printf '%s' "$family" | grep -Eq '^[a-z]+$'          || die bad_family_arg "$family"

# log_path (if given): the install progress log for the Rexon window. Rexon
# creates the file in advance (mkstemp), and it belongs to the CALLER. So it is
# written by a process WITH THE CALLER'S RIGHTS (setpriv), not by root: whatever
# file turns up at this path by the time of writing, root does not write there
# and does not change the owner.
#
# ⛔ The hole this closed (28.09.2026). Before: realpath, then
# `chown root` + writing as root + at the end `chown $PKEXEC_UID` — by the path
# string, minutes into the install. A directory in the path belongs to the user,
# and it could be replaced with a link (/tmp/a/b → /etc) at any moment before
# the install ended: at the end root handed the user, say, /etc/shadow.
# The org.rexon.de-install action stood in the «passwordless» rule at the time —
# a silent root for any session process.
#
# the exit marker __REXON_EXIT__ is filtered by Rexon's tail parser.
if [ -n "$log_path" ]; then
    case "$log_path" in
        /tmp/?*|/var/tmp/?*) : ;;
        *) die log_outside_tmp "$log_path" ;;
    esac
    log_uid="${PKEXEC_UID:-}"
    printf '%s' "$log_uid" | grep -Eq '^[0-9]+$' || die not_via_pkexec
    log_gid="$(getent passwd "$log_uid" 2>/dev/null | cut -d: -f4)"
    printf '%s' "$log_gid" | grep -Eq '^[0-9]+$' || die no_user_group "$log_uid"
    if command -v setpriv >/dev/null 2>&1; then
        exec > >(exec setpriv --reuid="$log_uid" --regid="$log_gid" --clear-groups \
                     -- tee -a -- "$log_path" >/dev/null) 2>&1
        log_pid=$!
        trap 'ec=$?; echo "__REXON_EXIT__$ec"; exec >&- 2>&-; wait "$log_pid" 2>/dev/null || true; exit "$ec"' EXIT
    else
        # Without setpriv we keep no log at all: writing someone else's file as root is not allowed.
        rx_say no_setpriv >&2
    fi
fi

# Packages for the de_id|family pair — ONLY from the embedded table (allowlist).
pkgs="$(
  case "$de_id|$family" in
__PKG_TABLE__
    *) die unknown_de_pair "$de_id|$family" ;;
  esac
)"
[ -n "$pkgs" ] || die empty_pkg_list "$de_id|$family"

# Package manager commands per family (--needed = restore/repair:
# what is installed is skipped).
case "$family" in
__FAM_CASES__
    *) die unsupported_family "$family" ;;
esac

# ⛔ No `eval`: commands are ARRAYS of arguments, the package list
# is split into words EXACTLY once and is never handed to the shell to parse.
# The former `eval "${PKG_CMD/'{PKGS}'/$pkgs}"` kept the door open: had a name
# with a metacharacter got into the table, it would be a command, not a package.

# 1) Update the DB (otherwise --needed may fail on a stale DB).
"${UPD_CMD[@]}" || die db_update_failed

# 2) Install the DE packages (names — only from the embedded table above).
read -r -a pkg_list <<<"$pkgs"
[ "${#pkg_list[@]}" -gt 0 ] || die empty_pkg_list "$de_id|$family"
"${PKG_CMD[@]}" "${pkg_list[@]}"

# 3) The login screen in THE SAME authorization session (no 2nd password). Only
# with systemd. The DM name is taken ONLY from the embedded list AND must
# be present in $pkgs of this DE.
#
# ⚠️ We do NOT touch someone else's login screen. Replacing the login screen is
# an intervention in a system-wide entity that the person notices, and doing it
# silently, «along the way» with a desktop install, is not allowed: the person may
# have plasmalogin, gdm, greetd. We enable this desktop's DM ONLY when the system
# has no enabled login screen at all (otherwise after a reboot there is no graphical login).
#
# ⚠️ And in any outcome we tell the truth: did it — what was done, could not — why.
# Previously a systemctl error went to /dev/null, and «enabled» was printed
# unconditionally — the output reported success where nothing happened.
if command -v systemctl >/dev/null 2>&1; then
    # What is set now: the target of the display-manager.service symlink, and if
    # there is no symlink — the first enabled unit from the known list.
    cur_dm=""
    if [ -L "__DM_SYMLINK__" ]; then
        cur_target="$(readlink -f "__DM_SYMLINK__" 2>/dev/null || true)"
        cur_dm="${cur_target##*/}"
        cur_dm="${cur_dm%.service}"
        [ -n "$cur_dm" ] || cur_dm="display-manager"
    else
        for known in __DM_KNOWN__; do
            state="$(systemctl is-enabled "$known.service" 2>/dev/null || true)"
            case "$state" in
                __DM_STATES__) cur_dm="$known"; break ;;
            esac
        done
    fi

    if [ -n "$cur_dm" ]; then
        rx_say dm_already "$cur_dm"
    else
        # Mapping UNIT-NAME → PACKAGE-NAME (the names usually match; the exception is
        # Plasma Login Manager: unit plasmalogin, package plasma-login-manager).
        pkg_for_dm() {
            case "$1" in
__DM_PKGMAP__
                *) printf '%s' "$1" ;;
            esac
        }
        dm_done=""
        for dm in __DM_LIST__; do
            case "$dm" in
                __DM_CASES__) : ;;
                *) continue ;;
            esac
            dm_pkg="$(pkg_for_dm "$dm")"
            # the dm PACKAGE must be in the package list of THIS DE (exact word).
            printf ' %s ' "$pkgs" | grep -q " $dm_pkg " || continue
            # And the unit is really installed (the unit file exists).
            systemctl cat "$dm.service" >/dev/null 2>&1 || continue
            # --force for a DM that does not Provide display-manager.service (PLM).
            force=""
            case "$dm" in
                __DM_FORCE__) force="--force" ;;
            esac
            # shellcheck disable=SC2086
            if err="$(systemctl enable $force "$dm.service" 2>&1)"; then
                rx_say dm_enabled "$dm"
            else
                if [ -n "$err" ]; then
                    rx_say dm_enable_failed "$dm" "$err"
                else
                    rx_say dm_enable_failed_plain "$dm"
                fi
            fi
            dm_done="$dm"
            break
        done
        [ -n "$dm_done" ] || rx_say dm_none
    fi
fi
exit 0
"#;

// Password window captions (polkit): all catalog languages at once
// (`xml:lang`), polkit picks by the session locale itself ([`crate::system_l10n`]).
// TRANSLATORS: Name of the system action in the password dialog (polkit).
const DE_POLICY_DESCRIPTION: &str = n_("Install or repair a desktop environment");
// TRANSLATORS: Text of the system password dialog (polkit).
const DE_POLICY_MESSAGE: &str = n_("Authentication is required to install a desktop environment");
// TRANSLATORS: Name of the system action in the password dialog (polkit).
const PKG_POLICY_DESCRIPTION: &str = n_("Install, remove or update packages");
// TRANSLATORS: Text of the system password dialog (polkit).
const PKG_POLICY_MESSAGE: &str = n_("Authentication is required to manage packages");
// TRANSLATORS: Name of the system action in the password dialog (polkit): login screen background, greeter theme or MATE panel layout.
const STAGE_POLICY_DESCRIPTION: &str = n_("Install a system login-screen or panel layout");
// TRANSLATORS: Name of the system action in the password dialog (polkit): one password lets Rexon make system changes until its window is closed.
const SESSION_POLICY_DESCRIPTION: &str = n_("Allow Rexon system changes while its window is open");
// TRANSLATORS: Text of the system password dialog (polkit): one password for all system changes until the Rexon window is closed.
const SESSION_POLICY_MESSAGE: &str = n_("Authentication is required to let Rexon make system changes until its window is closed");

/// XML of the DE_INSTALL_ACTION polkit policy: port of `_de_policy_xml`.
///
/// The manual install path: the installer puts the helper there and
/// annotates it with the same path. The package prints its own policy
/// [`de_policy_xml_for`]: its helper lives in [`HELPER_DIR_PACKAGE`].
pub fn de_policy_xml() -> String {
    de_policy_xml_for(DE_HELPER_PATH)
}

/// XML of the same policy for a helper at the given path.
///
/// `exec.path` is a contract with polkit: if it does not match what is
/// really run, pkexec will not pick our action (a password every time).
pub fn de_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{DE_INSTALL_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", DE_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", DE_POLICY_MESSAGE),
    )
}

/// `case` branches "os-release ID → family" for the helper: ONE truth with
/// the core ([`crate::system::ID_MAP`]).
///
/// Why generate instead of writing by hand: the helper must recognise the
/// machine THE SAME way Rexon does. A second list would silently diverge,
/// and on some derivative distribution the helper would refuse where the
/// engine is sure everything is supported.
fn id_map_cases() -> String {
    let mut by_family: Vec<(&str, Vec<&str>)> = Vec::new();
    for (id, fam) in crate::system::ID_MAP {
        let fam = fam.as_str();
        match by_family.iter_mut().find(|(f, _)| *f == fam) {
            Some((_, ids)) => ids.push(id),
            None => by_family.push((fam, vec![id])),
        }
    }
    by_family
        .into_iter()
        .map(|(fam, ids)| {
            format!("        {}) printf '%s' {fam}; return 0 ;;", ids.join("|"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `case` branches "family → package manager binary": the tail of the same
/// detection as the core's (`DistroDetector.family`): a distribution that is
/// not in [`crate::system::ID_MAP`] is recognised by the installed manager.
const HELPER_BIN_PROBE: &[(&str, &str)] = &[
    ("pacman", "arch"),
    ("dnf", "fedora"),
    ("yum", "fedora"),
    ("zypper", "suse"),
    ("emerge", "gentoo"),
    ("apk", "alpine"),
    ("xbps-install", "void"),
    ("eopkg", "solus"),
    ("swupd", "clear"),
];

/// One `case` branch: the family command as an ARRAY of arguments, without a
/// shell and without `eval`. `{PKGS}` from the table is dropped: the helper
/// adds the packages itself, as separate arguments.
fn fam_cmd_case(table: &[(&str, &str)], fam: &str, indent: &str) -> String {
    let tmpl = table_get(table, fam).expect("family в таблице");
    let args: Vec<&str> = tmpl
        .split_whitespace()
        .filter(|t| *t != "{PKGS}")
        .collect();
    for a in &args {
        // A bash array is not a shell, but a word with a metacharacter would still
        // mean the command table has stopped being a table.
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '=' | '.' | '_' | '/' | ':')),
            "слово команды {a} требует кавычек — таблица команд сломана"
        );
    }
    format!("{indent}{fam}) cmd=({}) ;;", args.join(" "))
}

/// Body of rexon-pkg-helper.
///
/// ⛔ A HOLE WAS CLOSED HERE ON 25.09.2026 (details in the script's own header).
/// Three rules that no edit may weaken:
///   1. the script determines the package manager family ITSELF from the
///      machine ([`id_map_cases`], [`HELPER_BIN_PROBE`]), it does not take it
///      from an argument;
///   2. a package name is a NAME, not a path ([`is_safe_pkg_arg`]): `/` only
///      in a Gentoo atom, a package file is not accepted even without `/`;
///   3. the command is assembled as an ARRAY of arguments: there is no `eval`
///      in the helper.
pub fn pkg_helper_script() -> String {
    let fam_install = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_INSTALL, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let fam_remove = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_REMOVE, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let fam_update = PKG_HELPER_FAMILIES
        .iter()
        .map(|f| fam_cmd_case(PKG_HELPER_UPDATE, f, "        "))
        .collect::<Vec<_>>()
        .join("\n");
    let bin_probe = HELPER_BIN_PROBE
        .iter()
        .map(|(bin, fam)| {
            format!("    if have {bin}; then printf '%s' {fam}; return 0; fi")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let fam_list = PKG_HELPER_FAMILIES.join("|");
    let dm_cases = DE_HELPER_DISPLAY_MANAGERS.join("|");
    let (desktop_exact, desktop_substr) = desktop_token_cases();
    let dm_force = DM_FORCE_ENABLE.join("|");
    let text = PKG_HELPER_SCRIPT_TMPL
        .replace("__ID_CASES__", &id_map_cases())
        .replace("__BIN_PROBE__", &bin_probe)
        .replace("__FAM_LIST__", &fam_list)
        .replace("__FAM_INSTALL__", &fam_install)
        .replace("__FAM_REMOVE__", &fam_remove)
        .replace("__FAM_UPDATE__", &fam_update)
        .replace("__VITAL__", &PKG_HELPER_VITAL.join(" "))
        .replace("__VITAL_CASE__", &vital_case_pattern())
        .replace("__DM_KNOWN__", &KNOWN_DM_UNITS.join(" "))
        .replace("__DM_STATES__", &DM_ENABLED_STATES.join("|"))
        .replace("__DM_PROTECT_MAP__", &dm_protect_map())
        .replace("__DESKTOP_EXACT__", &desktop_exact)
        .replace("__DESKTOP_SUBSTR__", &desktop_substr)
        .replace("__DESKTOP_PKGS__", &desktop_protect_cases())
        .replace("__INSTALL_ALLOW__", &install_allow_cases())
        .replace("__DM_FORCE__", &dm_force)
        .replace("__DM_CASES__", &dm_cases)
        .replace("__DM_SWITCH_CASES__", &dm_switch_units().join("|"))
        .replace("__DM_SWITCH_PAM__", &dm_switch_pam_cases())
        .replace("__DM_DEBIAN_PKG__", &dm_debian_pkg_cases())
        .replace("__DM_FALLBACK_SCRIPT_PATH__", DM_FALLBACK_SCRIPT_PATH)
        .replace("__DM_FALLBACK_SCRIPT__", DM_FALLBACK_SCRIPT.trim_end())
        .replace("__DM_FALLBACK_UNIT__", &dm_fallback_unit().trim_end())
        .replace("__DM_FALLBACK_SERVICE__", DM_FALLBACK_SERVICE)
        .replace("__DM_FALLBACK_DROPIN__", DM_FALLBACK_DROPIN)
        .replace("__DM_FALLBACK_DIR__", DM_FALLBACK_DIR)
        .replace("__PREVIOUS_DM_FILE__", PREVIOUS_DM_FILE)
        .replace("__LIGHTDM_GREETER_CONF__", LIGHTDM_GREETER_CONF)
        .replace("__PKG_HELPER_NAME__", PKG_HELPER_NAME)
        .replace("__REBUILD_REC_DIR__", REBUILD_REC_DIR)
        .replace("__DM_SYMLINK__", DM_SYMLINK);
    crate::system_l10n::localize_script(PKG_HELPER_NAME, &text)
}

/// Package helper records "what build-deps installed" (one file per caller,
/// root 0700/0600; `/run` is cleared on reboot).
pub const REBUILD_REC_DIR: &str = "/run/rexon-rebuild";

// ── switch-dm: the safety net and files on disk ──────────────────────────

/// The Rexon directory in `/etc`: the console hint and the safety net data.
pub const DM_FALLBACK_DIR: &str = "/etc/rexon";
/// The hint "which login screen it was and how to restore it": read from a
/// text console with `cat` (Wehrwolfmann's decision of 20.10.2026).
pub const PREVIOUS_DM_FILE: &str = "/etc/rexon/previous-display-manager";
/// The fallback service script.
pub const DM_FALLBACK_SCRIPT_PATH: &str = "/etc/rexon/dm-fallback";
/// The fallback service: called by `OnFailure=` of the new login screen.
pub const DM_FALLBACK_SERVICE: &str = "rexon-dm-fallback.service";
/// The new login screen's drop-in with `OnFailure=`.
pub const DM_FALLBACK_DROPIN: &str = "50-rexon-fallback.conf";
/// The LightDM greeter chosen by the person.
pub const LIGHTDM_GREETER_CONF: &str = "/etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf";

/// Login screens the helper can make the default ([`DmSpec::switchable`]).
pub fn dm_switch_units() -> Vec<&'static str> {
    LOGIN_MANAGER_SPECS.iter().filter(|s| s.switchable).map(|s| s.unit).collect()
}

fn dm_switch_pam_cases() -> String {
    LOGIN_MANAGER_SPECS
        .iter()
        .filter(|s| s.switchable)
        .map(|s| format!("        {}) pam={} ;;", s.unit, s.pam))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Login screen package on Debian (the debconf question and
/// `dpkg-reconfigure`): where the unit name does not match the package.
pub const DM_DEBIAN_PKG_FOR_UNIT: &[(&str, &str)] =
    &[("gdm", "gdm3"), ("plasmalogin", "plasma-login-manager")];

fn dm_debian_pkg_cases() -> String {
    DM_DEBIAN_PKG_FOR_UNIT
        .iter()
        .map(|(u, p)| format!("            {u}) printf '%s' {p} ;;"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fallback service unit. `StartLimitIntervalSec=0`: `OnFailure=` calls
/// it on every crash of the login screen, and without this it would hit the
/// start limit itself (5 in 10 s) before the last, decisive call.
pub fn dm_fallback_unit() -> String {
    format!(
        "[Unit]\n\
         Description=Rexon: bring back the previous login screen\n\
         DefaultDependencies=no\n\
         After=sysinit.target\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart={DM_FALLBACK_SCRIPT_PATH}\n"
    )
}

/// The fallback service script: started by systemd through `OnFailure=` of
/// the new login screen when it crashed at boot (including after
/// `start-limit-hit`, like Plasma Login Manager on 08.10.2026).
///
/// systemd calls `OnFailure=` also on a crash after which it restarts the
/// unit itself (measured on an archlinux bench on 20.10.2026): such calls are
/// skipped: the fallback happens only when the unit is finally `failed`.
///
/// Loop protection: the `dm-fallback.done` mark: the fallback is done once;
/// the drop-in is removed from the new login screen, and the previous one
/// never had it, so if the previous one crashes too, nothing more runs: the
/// text console and the hint in [`PREVIOUS_DM_FILE`] remain. It does not fire
/// when the machine is shutting down or if another login screen has already
/// been made the default. The data comes from `dm-fallback.conf`, the values
/// are checked once more; nothing from the file is executed.
pub const DM_FALLBACK_SCRIPT: &str = r#"#!/bin/bash
# Rexon: bring back the previous login screen when the new one failed to
# start (systemd OnFailure= of the new login screen). Runs once.
set -u
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
conf=/etc/rexon/dm-fallback.conf
stamp=/etc/rexon/dm-fallback.done
link=/etc/systemd/system/display-manager.service
val() { sed -n "s/^$1=//p" "$conf" 2>/dev/null | head -n1; }
name_ok() { printf '%s' "$1" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$'; }
[ -r "$conf" ] || exit 0
[ ! -e "$stamp" ] || exit 0
old="$(val old)"; new="$(val new)"; new_real="$(val new_real)"
name_ok "$old" && name_ok "$new" || exit 0
case "$(systemctl is-system-running 2>/dev/null || true)" in
    stopping) exit 0 ;;
esac
[ "$(readlink -f "$link" 2>/dev/null || true)" = "$new_real" ] || exit 0
# systemd triggers OnFailure= on every failed run, also when it is about to
# restart the unit itself. Wait until it gives up (start-limit-hit or no
# more restarts): the unit is then "failed" and this runs again.
state="$(systemctl show -p ActiveState --value "$new.service" 2>/dev/null || true)"
sub="$(systemctl show -p SubState --value "$new.service" 2>/dev/null || true)"
if [ "$state" != failed ] || [ "$sub" = auto-restart ] || [ "$sub" = auto-restart-queued ]; then
    echo "rexon-dm-fallback: $new is $state/$sub, waiting for systemd to give up"
    exit 0
fi
date > "$stamp"
echo "rexon-dm-fallback: $new failed to start, bringing back $old"
if ! systemctl enable --force "$old.service"; then
    echo "rexon-dm-fallback: could not enable $old; see /etc/rexon/previous-display-manager"
    exit 1
fi
rm -f "/etc/systemd/system/$new.service.d/50-rexon-fallback.conf"
rmdir "/etc/systemd/system/$new.service.d" 2>/dev/null || true
old_x11="$(val old_x11)"
if [ -n "$old_x11" ] && [ -f /etc/X11/default-display-manager ]; then
    printf '%s\n' "$old_x11" > /etc/X11/default-display-manager
fi
old_pkg="$(val old_pkg)"
if [ -n "$old_pkg" ] && name_ok "$old_pkg" && command -v debconf-communicate >/dev/null 2>&1; then
    printf 'set shared/default-x-display-manager %s\n' "$old_pkg" | debconf-communicate >/dev/null 2>&1 || true
fi
if [ "$(val greeter_written)" = 1 ]; then
    if [ -f /etc/rexon/dm-fallback.greeter-prev ]; then
        cp -p /etc/rexon/dm-fallback.greeter-prev /etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf
    else
        rm -f /etc/lightdm/lightdm.conf.d/50-rexon-greeter.conf
    fi
fi
systemctl daemon-reload
systemctl reset-failed "$new.service" display-manager.service 2>/dev/null || true
systemctl start --no-block display-manager.service
"#;

const PKG_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon pkg helper — privileged package manager operations.
#
# Run EXCLUSIVELY through pkexec (polkit action org.rexon.pkg).
# Accepts an operation and package NAMES. Commands come from EMBEDDED tables,
# the helper determines the package manager family ITSELF from THIS machine.
# No shell, no `eval`, no paths: arbitrary code is not run.
# The only file the helper accepts is a package built by the user's
# Rexon rebuild (the install-built operation, only from the caller's
# rebuild directory; see its section below).
#
# ⛔ THE HOLE THIS CLOSED (25.09.2026)
# The former helper (1) took the family from an ARGUMENT, (2) allowed the
# character '/' in package names (allowlist '^[A-Za-z0-9@._+:/-]+$') and (3) built
# the command through eval. Any session process could call
#     pkexec rexon-pkg-helper debian install /tmp/evil.deb
# and apt-get (as well as dnf, zypper) would install a LOCAL FILE whose install
# scripts run AS ROOT. The family did not even have to match
# the machine — any installed manager was enough.
# The second half of the hole — 'remove': the name was checked by nothing but
# its characters, and 'remove linux' took down the kernel.
set -euo pipefail
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
# Neither keyboard input (a manager question would hang silently until the timeout)
# nor someone else's working directory: pkexec does NOT change it, and 'dnf install evil.rpm'
# looks for the file exactly there.
exec </dev/null
cd /

__L10N__
have() { command -v "$1" >/dev/null 2>&1; }

# ── Package manager family: ONLY from this machine ───────────────────
# The order is the same as in the core (DistroDetector.family): ID from os-release →
# ID_LIKE → presence of binaries. os-release is not executed and is not fed
# to the shell — sed extracts the value.
osr_field() {
    local f v
    for f in /etc/os-release /usr/lib/os-release; do
        [ -r "$f" ] || continue
        v="$(sed -n "s/^$1=//p" "$f" | head -n1)"
        v="${v%\"}"; v="${v#\"}"; v="${v%\'}"; v="${v#\'}"
        printf '%s' "$v" | tr 'A-Z' 'a-z'
        return 0
    done
}

family_of_id() {
    case "$1" in
__ID_CASES__
        *) return 1 ;;
    esac
}

detect_family() {
    local id like
    id="$(osr_field ID)"
    if [ -n "$id" ] && family_of_id "$id"; then return 0; fi
    for like in $(osr_field ID_LIKE); do
        if family_of_id "$like"; then return 0; fi
    done
    # APT-RPM (ALT): apt-get is there, dpkg is not.
    if have apt-get; then
        if have dpkg; then printf '%s' debian; return 0; fi
        if have rpm; then printf '%s' alt; return 0; fi
        printf '%s' debian; return 0
    fi
__BIN_PROBE__
    return 1
}

# ── Arguments ────────────────────────────────────────────────────────
# argv is as before (family op [pkg...]) — the engine calls with it — but the FIRST
# argument no longer decides anything: the command is chosen only by the family
# determined above. A mismatch is not a refusal but a journal entry: slipping
# someone else's package manager through it is no longer possible.
[ "$#" -ge 2 ] || die pkg_args
claimed="$1"; op="$2"; shift 2

family="$(detect_family || true)"
[ -n "$family" ] || die family_unknown
case "$family" in
    __FAM_LIST__) : ;;
    *) die unsupported_family "$family" ;;
esac
if [ "$claimed" != "$family" ]; then
    rx_say family_mismatch "$claimed" "$family" >&2
fi

case "$op" in
    install|remove|update|enable-dm|switch-dm|build-deps|install-built) : ;;
    *) die bad_op "$op" ;;
esac

# ── Rebuild from the Arch recipe: two narrow operations ──────────────
# The Rexon rebuild (repo_rebuild) builds a package AS THE USER and
# calls the helper twice, under the same polkit action as installing
# everything else — the password is asked once:
#   build-deps dep...                         what is missing for the build,
#       pacman -S --needed --asdeps; a line is a name and possibly
#       a version constraint ('cmake>=3.20'), as 'pacman -T' gives it;
#   install-built plain|replace file [pkg...] the built file, pacman -U
#       (replace — '--ask=4': replace a conflicting installed one), then
#       'pacman -Rns' of the build pkgs; failing to remove them is not an error.
# ⛔ install-built may remove ONLY what the previous build-deps of the same
# caller installed: build-deps records the new packages
# (the 'pacman -Qq' difference before and after) in its own root file under __REBUILD_REC_DIR__,
# install-built removes the intersection of the passed list with the record, and only
# packages marked «dependency» ('pacman -Qqd'); the record is erased after
# the install. No record — nothing is removed: otherwise install-built could be
# used to remove any system package except the permanent list.
# ⛔ file is NOT «any file»: only from the caller's rebuild directory
# (~/.cache/rexon/rebuild/<subdirectory>, the home directory comes from passwd by
# PKEXEC_UID, not from the environment), a regular file of the caller, not a link, a
# makepkg file name (*.pkg.tar[.compression]). pacman gets ITS OWN copy in a
# 0700 root directory, taken from the checked file (cp without dereferencing
# links refuses if the file was replaced during the copy) — a replacement
# after the check changes nothing. This way the helper installs neither a system
# file, nor an old version from the pacman cache, nor someone else's file.
if [ "$op" = build-deps ] || [ "$op" = install-built ]; then
    [ "$family" = arch ] || die rebuild_arch_only "$op"
fi
built=""; built_mode=""
if [ "$op" = install-built ]; then
    [ "$#" -ge 2 ] || die built_args
    built_mode="$1"; built="$2"; shift 2
    case "$built_mode" in
        plain|replace) : ;;
        *) die built_args ;;
    esac
fi

# ── A package name is a NAME, not a path ─────────────────────────────
# '/' is forbidden everywhere except the Gentoo atom «category/name»; a leading '-' is
# a package swapped for a flag; a leading dot and '..' are always a path; a package
# file tail ('.deb', '.rpm', …) is a file even without '/'; '@' is legitimate only as
# a dnf group on Fedora (the Portage set '@world' in 'emerge --unmerge' would wipe
# the system). The rule is the same as the core's is_safe_pkg_arg.
atom_ok() {
    printf '%s' "$1" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._+:-]*$'
}

pkg_ok() {
    local a="$1" ctg nm
    case "$a" in
        *..*) return 1 ;;
        *.deb|*.rpm|*.apk|*.eopkg|*.xbps|*.tar|*.tgz|*.txz|*.zst|*.xz|*.gz|*.bz2|*.lz4|*.snap|*.flatpakref)
            return 1 ;;
    esac
    [ "$family" != fedora ] || a="${a#@}"
    case "$a" in
        */*)
            [ "$family" = gentoo ] || return 1
            ctg="${a%%/*}"; nm="${a#*/}"
            case "$nm" in */*) return 1 ;; esac
            atom_ok "$ctg" && atom_ok "$nm" ;;
        *) atom_ok "$a" ;;
    esac
}

# A dependency line for build-deps: a name (the same rule) and possibly
# a version constraint.
dep_ok() {
    local nm="${1%%[<>=]*}" ver
    ver="${1#"$nm"}"
    pkg_ok "$nm" || return 1
    [ -z "$ver" ] || printf '%s' "$ver" | grep -Eq '^(>=|<=|=|<|>)[A-Za-z0-9][A-Za-z0-9._+:~-]*$'
}

for a in "$@"; do
    case "$a" in
        -*) die arg_dash "$a" ;;
    esac
    if [ "$op" = build-deps ]; then
        dep_ok "$a" || die not_a_name "$a"
    else
        pkg_ok "$a" || die not_a_name "$a"
    fi
done

# ── Removal: we never remove anything vital ──────────────────────────
# 'remove' runs as root and with «do not ask»: 'remove linux' would leave the
# machine unbootable, 'remove polkit' — without the confirmation mechanism. None
# of these names is in any desktop of the Rexon database, so the list
# does not get in the way of an honest desktop removal.
# ⛔ 28.10.2026: not only exact names but families too — kernels
# ('linux-cachyos', 'linux-image-…'), drivers ('nvidia-…', 'mesa'),
# firmware and microcode ('*-firmware', '*-ucode'), initramfs builders.
# The pattern is the same rule as the core's (is_vital_package); no eval.
is_vital() {
    local k="${1#@}"
    k="${k##*/}"
    case "$k" in
        __VITAL_CASE__) return 0 ;;
    esac
    return 1
}
if [ "$op" = "remove" ]; then
    for a in "$@"; do
        if is_vital "$a"; then
            die vital_pkg "$a"
        fi
    done
fi
# install-built: the former exact list (build ones like 'linux-headers'
# are removed legitimately; their bounds are in the install-built section).
VITAL="__VITAL__"
if [ "$op" = install-built ]; then
    for a in "$@"; do
        for v in $VITAL; do
            if [ "$a" = "$v" ] || [ "${a##*/}" = "$v" ]; then
                die vital_pkg "$a"
            fi
        done
    done
fi

# ── Removal: the desktop in use and its login screen are not removed ─
# The helper does not trust the window: it learns what to keep itself, as root.
#   1. The active login screen: the target of the display-manager.service link (no
#      link — the first enabled unit from the known list); what is kept is the
#      unit name, its package by the core's table and the owner of the unit file per
#      the package manager. For LightDM — also the greeters
#      ('lightdm-*greeter*'): lightdm does not depend on them.
#   2. logind graphical sessions (Type x11/wayland/mir, Class user — of any
#      user, not only the caller): the desktop — by the session's 'Desktop='
#      (XDG_SESSION_DESKTOP) with the same table as the core's; the desktop packages — from
#      the core's table, Arch groups are expanded ('pacman -Qqg').
# Limits: without systemd/elogind ('loginctl') item 2 is silent; a session without
# 'Desktop=' (startx, some logins through greetd) does not name a desktop —
# only the login screen is kept.
dm_unit_now() {
    local link="__DM_SYMLINK__" tgt known state
    if [ -L "$link" ]; then
        tgt="$(readlink -f "$link" 2>/dev/null || true)"
        if [ -n "$tgt" ] && [ -e "$tgt" ]; then
            printf '%s\n%s\n' "${tgt##*/}" "$tgt"
            return 0
        fi
    fi
    have systemctl || return 0
    for known in __DM_KNOWN__; do
        [ "$known" != display-manager ] || continue
        state="$(systemctl is-enabled "$known.service" 2>/dev/null || true)"
        case "$state" in
            __DM_STATES__)
                tgt="$(systemctl show -p FragmentPath --value "$known.service" 2>/dev/null || true)"
                printf '%s\n%s\n' "$known.service" "$tgt"
                return 0 ;;
        esac
    done
}

owner_of() {
    [ -n "$1" ] && [ -e "$1" ] || return 0
    case "$family" in
        arch) pacman -Qqo -- "$1" 2>/dev/null || true ;;
        debian) dpkg-query -S -- "$1" 2>/dev/null | sed -n 's/^\([^:,]*\)[:,].*/\1/p' | head -n1 || true ;;
        fedora|suse|alt|clear) rpm -qf --qf '%{NAME}\n' -- "$1" 2>/dev/null || true ;;
        alpine) apk info -Wq -- "$1" 2>/dev/null || true ;;
        void) xbps-query -o -- "$1" 2>/dev/null | sed -n 's/^\([^:]*\)-[^-:]*:.*/\1/p' | head -n1 || true ;;
    esac
}

dm_pkg_of_unit() {
    case "$family|$1" in
__DM_PROTECT_MAP__
        *) printf '%s\n' "$1" ;;
    esac
}

de_of_desktop() {
    local d t
    d="$(printf '%s' "$1" | tr 'A-Z' 'a-z')"
    [ -n "$d" ] || return 1
    # Tokens through read, not through word splitting: not a single pattern expansion.
    while IFS= read -r t; do
        case "$t" in
__DESKTOP_EXACT__
        esac
    done < <(printf '%s\n' "$d" | tr ':' '\n')
    case "$d" in
__DESKTOP_SUBSTR__
    esac
    return 1
}

de_pkgs() {
    case "$1" in
__DESKTOP_PKGS__
    esac
}

held_packages() {
    local unit file s ty cl de p
    { read -r unit; read -r file; } < <(dm_unit_now; printf '\n\n')
    unit="${unit%.service}"
    if [ -n "$unit" ]; then
        printf '%s\n' "$unit"
        dm_pkg_of_unit "$unit"
        owner_of "$file"
    fi
    have loginctl || return 0
    for s in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}'); do
        case "$s" in *[!A-Za-z0-9._-]*|'') continue ;; esac
        ty="$(loginctl show-session "$s" -p Type --value 2>/dev/null || true)"
        case "$ty" in x11|wayland|mir) : ;; *) continue ;; esac
        cl="$(loginctl show-session "$s" -p Class --value 2>/dev/null || true)"
        [ "$cl" = user ] || continue
        de="$(de_of_desktop "$(loginctl show-session "$s" -p Desktop --value 2>/dev/null || true)" || true)"
        [ -n "$de" ] || continue
        for p in $(de_pkgs "$de|$family"); do
            printf '%s\n' "$p"
            if [ "$family" = arch ]; then
                pacman -Qqg -- "$p" 2>/dev/null || true
            fi
        done
    done
}

if [ "$op" = "remove" ]; then
    held="$(held_packages | sed 's|.*/||' | LC_ALL=C sort -u || true)"
    for a in "$@"; do
        b="${a##*/}"
        # a here-string, not a pipeline (pipefail and SIGPIPE).
        if grep -Fxq -- "$b" <<<"$held"; then
            die current_de_pkg "$a"
        fi
        if grep -Fxq lightdm <<<"$held"; then
            case "$b" in lightdm-*greeter*) die current_de_pkg "$a" ;; esac
        fi
    done
fi

# ── Install: only packages from the core's tables ────────────────────
# The allowlist is embedded when the helper is generated: desktops (main and AUR),
# login screens, compliance repair, wallpaper video dependencies, yt-dlp,
# shells, Conky widget programs and the 7z unpacker, parts of the MATE
# layouts, theme icons and cursors, «Distro looks» images. A name outside the list
# the helper does not install as root, whatever the caller sends.
install_allowed() {
    local list=""
    case "$family" in
__INSTALL_ALLOW__
    esac
    case " $list " in
        *" $1 "*) return 0 ;;
    esac
    return 1
}
if [ "$op" = install ]; then
    for a in "$@"; do
        install_allowed "$a" || die not_in_tables "$a"
    done
fi

# The command is an argument array; it runs directly, without a shell.
cmd=()
run_cmd() {
    [ "${#cmd[@]}" -gt 0 ] || die no_cmd "$family"
    have "${cmd[0]}" || die no_pm "${cmd[0]}"
    "${cmd[@]}" "$@"
}

uid_ok() {
    [ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec
    case "$PKEXEC_UID" in
        *[!0-9]*) die not_via_pkexec ;;
    esac
}
REC_DIR="__REBUILD_REC_DIR__"

if [ "$op" = build-deps ]; then
    [ "$#" -ge 1 ] || die op_needs_pkg "$op"
    uid_ok
    have pacman || die no_pm pacman
    before="$(pacman -Qq 2>/dev/null | LC_ALL=C sort -u || true)"
    rc=0
    pacman -S --needed --noconfirm --asdeps -- "$@" || rc=$?
    after="$(pacman -Qq 2>/dev/null | LC_ALL=C sort -u || true)"
    mkdir -p -- "$REC_DIR"
    chmod 0700 -- "$REC_DIR"
    rm -f -- "$REC_DIR/$PKEXEC_UID"
    if [ -n "$before" ] && [ -n "$after" ]; then
        ( umask 077
          LC_ALL=C comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after") > "$REC_DIR/$PKEXEC_UID" )
    fi
    exit "$rc"
fi

if [ "$op" = install-built ]; then
    uid_ok
    have pacman || die no_pm pacman
    name="${built##*/}"
    case "$name" in
        .*|-*) die built_bad_name "$built" ;;
        *.pkg.tar|*.pkg.tar.zst|*.pkg.tar.xz|*.pkg.tar.gz|*.pkg.tar.bz2|*.pkg.tar.lz4|*.pkg.tar.lzo|*.pkg.tar.lrz|*.pkg.tar.lz|*.pkg.tar.Z) : ;;
        *) die built_bad_name "$built" ;;
    esac
    printf '%s' "$name" | grep -Eq '^[A-Za-z0-9@_+][A-Za-z0-9@._+:-]*$' || die built_bad_name "$built"
    case "$built" in
        /?*/*) : ;;
        *) die built_outside "$built" ;;
    esac
    home="$(getent passwd "$PKEXEC_UID" | cut -d: -f6 || true)"
    case "$home" in
        /?*) : ;;
        *) die built_no_home "$PKEXEC_UID" ;;
    esac
    # The rebuild root is the caller's own; ~/.cache may be a link
    # to another disk, so the real paths are compared.
    rebuild_root="$(cd -P -- "$home/.cache/rexon/rebuild" 2>/dev/null && pwd -P)" \
        || die built_outside "$built"
    [ "$(stat -c %u -- "$rebuild_root")" = "$PKEXEC_UID" ] || die built_outside "$built"
    # First ENTER the file's directory: from then on a name without '/' counts from it,
    # and replacing part of the path with a link changes nothing anymore.
    cd -P -- "${built%/*}" 2>/dev/null || die built_unreachable "$built"
    here="$(pwd -P)"
    case "$here" in
        "$rebuild_root"/?*) : ;;
        *) die built_outside "$built" ;;
    esac
    [ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die built_not_callers "$built"
    # Preliminary — so cp does not hang on a FIFO; the copy check decides.
    { [ -f "./$name" ] && [ ! -L "./$name" ]; } || die built_not_file "$built"
    work="$(mktemp -d /var/tmp/rexon-pkg-helper.XXXXXXXX)" || die no_work_dir
    trap 'rm -rf -- "$work"' EXIT
    cp -P --preserve=ownership -- "./$name" "$work/$name" 2>/dev/null || die built_copy_failed "$built"
    cd /
    { [ -f "$work/$name" ] && [ ! -L "$work/$name" ]; } || die built_not_file "$built"
    [ "$(stat -c %u -- "$work/$name")" = "$PKEXEC_UID" ] || die built_not_callers "$built"
    chown 0:0 -- "$work/$name"
    chmod 0600 -- "$work/$name"
    rc=0
    if [ "$built_mode" = replace ]; then
        pacman -U --noconfirm --ask=4 -- "$work/$name" || rc=$?
    else
        pacman -U --noconfirm -- "$work/$name" || rc=$?
    fi
    [ "$rc" -eq 0 ] || exit "$rc"
    rec="$REC_DIR/$PKEXEC_UID"
    drop=()
    if [ "$#" -gt 0 ]; then
        ours=""; asdeps=""
        if [ -f "$rec" ] && [ ! -L "$rec" ]; then
            ours="$(cat -- "$rec")"
            asdeps="$(pacman -Qqd 2>/dev/null || true)"
        fi
        for a in "$@"; do
            # a here-string, not a pipeline: with pipefail an early exit of grep -q
            # could take down printf (SIGPIPE) and «not find» what was found.
            if grep -Fxq -- "$a" <<<"$ours" && grep -Fxq -- "$a" <<<"$asdeps"; then
                drop+=("$a")
            else
                rx_say build_only_not_ours "$a"
            fi
        done
    fi
    rm -f -- "$rec"
    if [ "${#drop[@]}" -gt 0 ]; then
        pacman -Rns --noconfirm -- "${drop[@]}" || rx_say build_only_kept
    fi
    exit 0
fi

if [ "$op" = "update" ]; then
    [ "$#" -eq 0 ] || die update_no_pkgs
    case "$family" in
__FAM_UPDATE__
    esac
    run_cmd
    exit $?
fi

if [ "$op" = "enable-dm" ]; then
    [ "$#" -eq 1 ] || die enable_dm_one
    dm="$1"
    case "$dm" in
        __DM_CASES__) : ;;
        *) die bad_dm "$dm" ;;
    esac
    have systemctl || die no_systemctl
    # --force for a DM that does not Provide display-manager.service (Plasma Login
    # Manager: otherwise enable refuses because of an alias conflict).
    cmd=(systemctl enable)
    case "$dm" in
        __DM_FORCE__) cmd+=(--force) ;;
    esac
    cmd+=("$dm.service")
    "${cmd[@]}"
    exit $?
fi

# ── switch-dm: «Make default» for a login screen ─────────────────────
# systemd only. The helper ITSELF determines the previous login screen and the selection
# mechanism, ITSELF repeats the critical checks (an hour may have passed between probing
# the interface and the password) and verifies the result; on a mismatch — a rollback.
# ⛔ Only enable: no starting or stopping of services in a live session —
# the new login screen appears after a reboot. display-manager.service is
# started only by the rollback service, and only at boot,
# when the new login screen has failed.
if [ "$op" = "switch-dm" ]; then
    [ "$#" -ge 1 ] && [ "$#" -le 2 ] || die switch_dm_args
    new="$1"; greeter="${2:-}"
    case "$new" in
        __DM_SWITCH_CASES__) : ;;
        *) die bad_dm "$new" ;;
    esac
    have systemctl || die no_systemctl
    [ -d /run/systemd/system ] || die no_systemd
    DM_LINK="__DM_SYMLINK__"
    GREETER_CONF="__LIGHTDM_GREETER_CONF__"
    REXON_ETC="__DM_FALLBACK_DIR__"

    # The selection mechanism — per this machine (the same decision as dm_switch_backend).
    backend=systemd
    for p in "$DM_LINK" /usr/lib/systemd/system/display-manager.service; do
        if [ -f "$p" ] && [ "$(basename "$(readlink -f "$p")")" = display-manager.service ]; then
            die dm_backend_unsupported
        fi
    done
    if [ "$family" = debian ] && [ -f /etc/X11/default-display-manager ]; then
        backend=debian
    fi

    unit_path() {
        local d
        for d in /etc/systemd/system /usr/local/lib/systemd/system /usr/lib/systemd/system /lib/systemd/system; do
            if [ -e "$d/$1.service" ] || [ -L "$d/$1.service" ]; then
                printf '%s' "$d/$1.service"; return 0
            fi
        done
        return 1
    }
    unit_file="$(unit_path "$new" || true)"
    [ -n "$unit_file" ] || die dm_no_unit "$new"
    [ "$(readlink -f "$unit_file")" != /dev/null ] || die dm_masked "$new"
    unit_real="$(readlink -f "$unit_file")"

    # The program from ExecStart (the systemd prefixes -@:+! are stripped).
    exec_line="$(sed -n 's/^[[:space:]]*ExecStart=//p' "$unit_real" | tail -n1)"
    exec_bin="$(printf '%s\n' "$exec_line" | sed 's/^[-@:+!]*//' | awk '{print $1}')"
    [ -n "$exec_bin" ] || die dm_no_program "$new"
    have "$exec_bin" || die dm_no_program "$exec_bin"
    bin_path="$(command -v "$exec_bin")"

    pam=""
    case "$new" in
__DM_SWITCH_PAM__
    esac
    if [ -n "$pam" ] && [ ! -f "/etc/pam.d/$pam" ] && [ ! -f "/usr/lib/pam.d/$pam" ]; then
        die dm_no_pam "$pam"
    fi

    # greetd and everything that goes through it (cosmic-greeter): the greeter — from
    # ITS OWN config. A text login (agreety, a bare shell) does not qualify.
    if [ "$(basename "$exec_bin")" = greetd ]; then
        cfg="$(printf '%s\n' "$exec_line" | sed -n 's/.*\(--config\|-c\)[= ]\([^ ]*\).*/\2/p')"
        [ -n "$cfg" ] || cfg=/etc/greetd/config.toml
        gcmd="$(sed -n '/^\[default_session\]/,/^\[/{s/^[[:space:]]*command[[:space:]]*=[[:space:]]*//p}' "$cfg" 2>/dev/null | head -n1)"
        gcmd="${gcmd#\"}"; gcmd="${gcmd%\"}"; gcmd="${gcmd#\'}"; gcmd="${gcmd%\'}"
        gprog=""
        set -f
        for w in $gcmd; do
            case "$w" in
                env) continue ;;
                /*) gprog="$w"; break ;;
                *=*) continue ;;
                *) gprog="$w"; break ;;
            esac
        done
        set +f
        case "$(basename "${gprog:-agreety}")" in
            agreety|login|sh|bash|dash|zsh|fish) die dm_text_login ;;
        esac
        have "$gprog" || die dm_no_greeter_prog "$gprog"
    fi

    if [ -n "$greeter" ]; then
        [ "$new" = lightdm ] || die greeter_not_lightdm
        printf '%s' "$greeter" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$' || die bad_greeter "$greeter"
        gfile="/usr/share/xgreeters/$greeter.desktop"
        [ -f "$gfile" ] || die dm_no_greeter_prog "$greeter"
        gexec="$(sed -n 's/^Exec=//p' "$gfile" | head -n1 | awk '{print $1}')"
        { [ -n "$gexec" ] && have "$gexec"; } || die dm_no_greeter_prog "$greeter"
    fi
    if [ "$new" = lightdm ]; then
        ls /usr/share/xgreeters/*.desktop >/dev/null 2>&1 || die lightdm_no_greeter
        { have Xorg || [ -x /usr/lib/Xorg ]; } || die dm_no_xorg
    fi

    # The previous login screen — only by a live link.
    old=""; old_real=""
    if [ -L "$DM_LINK" ] && [ -e "$DM_LINK" ]; then
        old_real="$(readlink -f "$DM_LINK")"
        old="$(basename "$old_real" .service)"
        printf '%s' "$old" | grep -Eq '^[A-Za-z0-9][A-Za-z0-9._-]*$' || { old=""; old_real=""; }
    fi
    if [ -n "$old_real" ] && [ "$old_real" = "$unit_real" ]; then
        rx_say dm_already_default "$new"
        exit 0
    fi

    debian_pkg() {
        case "$1" in
__DM_DEBIAN_PKG__
            *) printf '%s' "$1" ;;
        esac
    }
    old_x11=""; old_pkg=""
    if [ "$backend" = debian ]; then
        old_x11="$(head -n1 /etc/X11/default-display-manager 2>/dev/null || true)"
        [ -z "$old" ] || old_pkg="$(debian_pkg "$old")"
    fi

    # The previous safety net is removed: it belonged to the past switch.
    for f in /etc/systemd/system/*.service.d/__DM_FALLBACK_DROPIN__; do
        [ -e "$f" ] || continue
        rm -f "$f"
        rmdir "$(dirname "$f")" 2>/dev/null || true
    done
    rm -f "$REXON_ETC/dm-fallback.done" "$REXON_ETC/dm-fallback.conf" "$REXON_ETC/dm-fallback.greeter-prev"

    greeter_written=0
    if [ -n "$greeter" ]; then
        mkdir -p "$(dirname "$GREETER_CONF")" "$REXON_ETC"
        [ ! -f "$GREETER_CONF" ] || cp -p "$GREETER_CONF" "$REXON_ETC/dm-fallback.greeter-prev"
        printf '[Seat:*]\ngreeter-session=%s\n' "$greeter" > "$GREETER_CONF"
        greeter_written=1
    fi

    debconf_set() {
        if have debconf-communicate; then
            printf 'set shared/default-x-display-manager %s\n' "$1" | debconf-communicate >/dev/null 2>&1 || true
        fi
    }
    rollback() {
        if [ -n "$old" ]; then
            systemctl enable --force "$old.service" >/dev/null 2>&1 || true
        fi
        if [ "$backend" = debian ] && [ -n "$old_x11" ]; then
            printf '%s\n' "$old_x11" > /etc/X11/default-display-manager || true
            [ -z "$old_pkg" ] || debconf_set "$old_pkg"
        fi
        if [ "$greeter_written" = 1 ]; then
            if [ -f "$REXON_ETC/dm-fallback.greeter-prev" ]; then
                cp -p "$REXON_ETC/dm-fallback.greeter-prev" "$GREETER_CONF" || true
            else
                rm -f "$GREETER_CONF"
            fi
        fi
    }

    if [ "$backend" = debian ]; then
        new_pkg="$(debian_pkg "$new")"
        debconf_set "$new_pkg"
        printf '%s\n' "$bin_path" > /etc/X11/default-display-manager
        if have dpkg-reconfigure; then
            # policy-rc.d: the package's script must not start the service in
            # a live session.
            own_policy=0
            if [ ! -e /usr/sbin/policy-rc.d ]; then
                printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
                chmod 0755 /usr/sbin/policy-rc.d
                own_policy=1
            fi
            DEBIAN_FRONTEND=noninteractive dpkg-reconfigure "$new_pkg" >/dev/null 2>&1 || true
            [ "$own_policy" = 0 ] || rm -f /usr/sbin/policy-rc.d
        fi
    fi

    if ! out="$(systemctl enable --force "$new.service" 2>&1)"; then
        rollback
        die dm_switch_failed "$new" "$out"
    fi

    # The «after» check: the link, the unit state, on Debian — the selection file.
    ok=1
    [ "$(readlink -f "$DM_LINK" 2>/dev/null || true)" = "$unit_real" ] || ok=0
    case "$(systemctl is-enabled "$new.service" 2>/dev/null || true)" in
        enabled|enabled-runtime|alias) : ;;
        *) ok=0 ;;
    esac
    if [ "$backend" = debian ] && [ "$(head -n1 /etc/X11/default-display-manager 2>/dev/null || true)" != "$bin_path" ]; then
        ok=0
    fi
    if [ "$ok" = 0 ]; then
        rollback
        die dm_switch_mismatch "$new"
    fi

    # Otherwise the previous one with WantedBy= (a home-made unit) would come up second.
    if [ -n "$old" ] && grep -Eq '^[[:space:]]*WantedBy=' "$old_real" 2>/dev/null; then
        systemctl disable "$old.service" >/dev/null 2>&1 || true
    fi

    # Safety net: the new login screen failed at boot (including
    # start-limit-hit) — the rollback service brings back the previous one. Only when there is
    # something to go back to.
    mkdir -p "$REXON_ETC"
    if [ -n "$old" ]; then
        {
            printf 'old=%s\n' "$old"
            printf 'old_real=%s\n' "$old_real"
            printf 'new=%s\n' "$new"
            printf 'new_real=%s\n' "$unit_real"
            printf 'old_x11=%s\n' "$old_x11"
            printf 'old_pkg=%s\n' "$old_pkg"
            printf 'greeter_written=%s\n' "$greeter_written"
        } > "$REXON_ETC/dm-fallback.conf"
        cat > "__DM_FALLBACK_SCRIPT_PATH__" <<'REXON_DM_FALLBACK'
__DM_FALLBACK_SCRIPT__
REXON_DM_FALLBACK
        chmod 0755 "__DM_FALLBACK_SCRIPT_PATH__"
        cat > "/etc/systemd/system/__DM_FALLBACK_SERVICE__" <<'REXON_DM_FALLBACK_UNIT'
__DM_FALLBACK_UNIT__
REXON_DM_FALLBACK_UNIT
        mkdir -p "/etc/systemd/system/$new.service.d"
        printf '[Unit]\nOnFailure=__DM_FALLBACK_SERVICE__\n' > "/etc/systemd/system/$new.service.d/__DM_FALLBACK_DROPIN__"
        if [ "$backend" = debian ]; then
            back="sudo dpkg-reconfigure $old_pkg && sudo systemctl enable --force $old.service && sudo reboot"
        else
            back="sudo systemctl enable --force $old.service && sudo reboot"
        fi
        # Lines for the person — without the helper name at the start.
        line() { printf '%s\n' "${1#__PKG_HELPER_NAME__: }"; }
        {
            line "$(rx_say prev_dm_header "$old" "$new")"
            line "$(rx_say prev_dm_back "$old")"
            printf '    %s\n' "$back"
            line "$(rx_say prev_dm_auto "$new" "$old")"
        } > "__PREVIOUS_DM_FILE__"
        rx_say dm_switched "$new" "$old"
    else
        rm -f "__PREVIOUS_DM_FILE__"
        rx_say dm_switched_first "$new"
    fi
    exit 0
fi

# install / remove — at least one package is needed.
[ "$#" -ge 1 ] || die op_needs_pkg "$op"
case "$op" in
    install)
        case "$family" in
__FAM_INSTALL__
        esac ;;
    remove)
        case "$family" in
__FAM_REMOVE__
        esac ;;
esac
run_cmd "$@"
exit $?
"#;

/// The polkit policy XML of PKG_ACTION — a port of `_pkg_policy_xml` (the
/// manual install path; the package prints [`pkg_policy_xml_for`]).
pub fn pkg_policy_xml() -> String {
    pkg_policy_xml_for(PKG_HELPER_PATH)
}

/// Actions covered by a confirmed `org.rexon.pkg` — the polkit annotation
/// `org.freedesktop.policykit.imply` (the «umbrella»).
///
/// How it works (polkit, `check_authorization_sync`): when asked for
/// authorization of action X, polkit also looks at the actions whose `imply`
/// lists X and checks them WITHOUT a password window — both an implicit
/// authorization and a temporary one (`auth_admin_keep`) for the same process.
/// So the password entered in the Rexon window for `org.rexon.pkg` covers for
/// five minutes the desktop install, the login screen file layout and the
/// SDDM/Plymouth themes as well — one window instead of four.
///
/// There is only one direction, and it was chosen on purpose: the umbrella is
/// held by the strongest action (installing and removing packages by name),
/// and it is NEVER «passwordless» ([`PASSWORDLESS_ACTIONS`]). The reverse
/// (`imply` on an action from the «passwordless» rule) would give packages
/// without a password to any session process — a hole closed on 25.09.2026.
/// The guard is `imply_umbrella_never_hangs_on_passwordless_action`.
pub const PKG_IMPLIES: &[&str] = &[
    DE_INSTALL_ACTION,
    STAGE_ACTION,
    crate::installer::SYSTEM_INSTALL_ACTION,
];

/// XML of the same policy for a helper located at the given path.
pub fn pkg_policy_xml_for(helper_path: &str) -> String {
    let implies = PKG_IMPLIES.join(" ");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{PKG_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.imply\">\
         {implies}</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", PKG_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", PKG_POLICY_MESSAGE),
    )
}

/// The body of rexon-stage-helper — a port of `_stage_helper_script`.
pub fn stage_helper_script() -> String {
    let cat_list = STAGE_CATEGORIES.join("|");
    let webkit_root = table_get(STAGE_DIR_ROOT, "lightdm-webkit").unwrap();
    let layouts_root = table_get(STAGE_DIR_ROOT, "mate-layouts").unwrap();
    let text = STAGE_HELPER_SCRIPT_TMPL
        .replace("__CAT_LIST__", &cat_list)
        .replace("__WEBKIT_ROOT__", webkit_root)
        .replace("__WEB_GREETER_ROOT__", WEB_GREETER_ROOT)
        .replace("__LAYOUTS_ROOT__", layouts_root);
    crate::system_l10n::localize_script(STAGE_HELPER_NAME, &text)
}

const STAGE_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon stage helper — privileged file-staging into /usr/share.
# Run EXCLUSIVELY through pkexec (polkit action org.rexon.stage,
# auth_admin_keep: a confirmed password is kept for a few minutes for the
# Rexon window; not in the «passwordless» rule since 28.09.2026). Copies a
# VALIDATED staged directory to a FIXED path by category. Does NOT
# accept an arbitrary dest. Does NOT run arbitrary code.
set -euo pipefail
export LC_ALL=C LANG=C

__L10N__

[ "$#" -eq 3 ] || die stage_args
category="$1"; name="$2"; staged="$3"

# 1) category — a strict allowlist.
case "$category" in
    __CAT_LIST__) : ;;
    *) die bad_category "$category" ;;
esac

# 2) name — only safe characters; no / .. and no leading '-'/'.'/emptiness.
case "$name" in
    ""|-*|.*|*/*|*..*) die bad_name "$name" ;;
esac
printf '%s' "$name" | grep -Eq '^[A-Za-z0-9._-]+$' \
    || die bad_name_chars "$name"

# 3) staged — the caller's directory under /tmp|/var/tmp (Rexon creates it with
#    mkdtemp). First we ENTER it: from then on we work with that very directory,
#    and replacing part of the path with a link changes nothing anymore. The place and
#    the owner are checked at the entry, the copy of «.» goes into our own 0700 directory and from
#    then on we work ONLY with it — the user will not touch it. Before: realpath,
#    then cp by the path string — a window to replace the path and carry someone else's
#    directory as root into the world-readable /usr/share.
[ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec
[ -d "$staged" ] || die staged_not_dir "$staged"
cd -P -- "$staged" 2>/dev/null || die staged_unreachable "$staged"
here="$(pwd -P)"
case "$here" in
    /tmp/?*|/var/tmp/?*) : ;;
    *) die staged_outside_tmp "$here" ;;
esac
[ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die staged_not_callers "$here"
work="$(mktemp -d /var/tmp/rexon-stage-helper.XXXXXXXX)" || die no_work_dir
trap 'rm -rf -- "$work"' EXIT
real_staged="$work/staged"
cp -R -P -- . "$real_staged" || die staged_copy_failed
cd /
odd="$(find "$real_staged" -mindepth 1 ! -type f ! -type d -print -quit)"
[ -z "$odd" ] || die staged_odd "${odd#"$real_staged"/}"

# ── Handling by category ─────────────────────────────────────────────
case "$category" in
    gdm-bg|lightdm-bg|cosmic-greeter|login-bg)
        # staged holds EXACTLY one image file. The file name is taken from the
        # directory, the extension is normalized (only [A-Za-z0-9]).
        img="$(find "$real_staged" -maxdepth 1 -type f \
            \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' \
               -o -iname '*.webp' -o -iname '*.bmp' \) | head -n1)"
        [ -n "$img" ] || die no_image
        ext="${img##*.}"
        printf '%s' "$ext" | grep -Eq '^[A-Za-z0-9]+$' || ext="png"
        dest_bg="/usr/share/backgrounds/rexon-login.$ext"
        install -d /usr/share/backgrounds
        # Remove past rexon-login.* (the extension may have changed).
        rm -f /usr/share/backgrounds/rexon-login.* 2>/dev/null || true
        cp -f "$img" "$dest_bg"
        chmod 644 "$dest_bg"
        case "$category" in
            gdm-bg)
                install -d /etc/dconf/db/gdm.d
                cat > /etc/dconf/db/gdm.d/02-rexon-login <<REXON_EOF
[org/gnome/desktop/background]
picture-uri='file://$dest_bg'
picture-uri-dark='file://$dest_bg'
picture-options='zoom'
REXON_EOF
                dconf update 2>/dev/null || true
                ;;
            lightdm-bg)
                # slick-greeter.
                if [ -f /etc/lightdm/slick-greeter.conf ]; then
                    sed -i "s|^background=.*|background=$dest_bg|" \
                        /etc/lightdm/slick-greeter.conf
                elif [ -d /etc/lightdm ]; then
                    cat >> /etc/lightdm/slick-greeter.conf <<REXON_EOF
[Greeter]
background=$dest_bg
REXON_EOF
                fi
                # gtk-greeter.
                if [ -f /etc/lightdm/lightdm-gtk-greeter.conf ]; then
                    sed -i "s|^background=.*|background=$dest_bg|" \
                        /etc/lightdm/lightdm-gtk-greeter.conf
                fi
                ;;
            cosmic-greeter|login-bg)
                : # только фон скопирован (login-bg — фон берёт учётная запись)
                ;;
        esac
        rx_say bg_installed "$dest_bg"
        ;;
    lightdm-webkit)
        # The root is chosen by fact: the one that really exists in the system
        # (both paths are fixed right here — an arbitrary dest is still
        # not accepted).
        if [ -d "__WEBKIT_ROOT__" ]; then
            root="__WEBKIT_ROOT__"
        elif [ -d "__WEB_GREETER_ROOT__" ]; then
            root="__WEB_GREETER_ROOT__"
        else
            die no_greeter_dir
        fi
        dest="$root/$name"
        install -d "$dest"
        cp -rT "$real_staged" "$dest"
        chmod -R a+rX "$dest"
        rx_say webkit_theme "$name"
        ;;
    mate-layouts)
        # staged = a directory with .layout/.dock (+ an opt. sed adaptation of snap paths
        # is done by Rexon BEFORE staging, the helper only copies). The files go
        # straight into the layouts root (without a <name> subdirectory). name here is
        # a technical marker ('rexon'), already validated above.
        dst="__LAYOUTS_ROOT__"
        install -d "$dst"
        # Copy ONLY .layout/.dock (against «extra files»).
        found=0
        for f in "$real_staged"/*.layout "$real_staged"/*.dock; do
            [ -e "$f" ] || continue
            cp -f "$f" "$dst/"
            found=1
        done
        [ "$found" = "1" ] || die no_layouts
        chmod -R a+rX "$dst"
        rx_say layouts_installed
        ;;
esac
exit 0
"#;

/// XML of the STAGE_ACTION polkit policy: port of `_stage_policy_xml` (the
/// manual install path; the package prints [`stage_policy_xml_for`]).
pub fn stage_policy_xml() -> String {
    stage_policy_xml_for(STAGE_HELPER_PATH)
}

/// XML of the same policy for a helper at the given path.
pub fn stage_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{STAGE_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", STAGE_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", crate::installer::POLICY_MESSAGE_SYSTEM_THEME),
    )
}

/// XML of the [`SESSION_ACTION`] polkit policy (the manual install path; the
/// package prints [`session_policy_xml_for`]).
pub fn session_policy_xml() -> String {
    session_policy_xml_for(SESSION_HELPER_PATH)
}

/// XML of the privilege session policy for a helper at the given path.
///
/// ⛔ `allow_active` is `auth_admin`, not `auth_admin_keep`: the helper holds
/// one password per window session. There is deliberately no `imply` annotation.
pub fn session_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{SESSION_ACTION}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">\
         {helper_path}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">\
         true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        description = crate::system_l10n::polkit_elem("    ", "description", SESSION_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", SESSION_POLICY_MESSAGE),
    )
}

/// pkexec command of a package manager operation: port of `privileged_pkg_cmd`.
/// None → the calling code falls back to raw `pkexec <pm> …` (graceful
/// fallback; the helper's presence is a parameter, files are not checked).
pub fn privileged_pkg_cmd(
    family: PkgFamily,
    op: &str,
    packages: &[String],
    helper_available: bool,
) -> Option<Vec<String>> {
    let fam = family.as_str();
    if !PKG_HELPER_OPS.contains(&op) || matches!(op, "build-deps" | "install-built") {
        return None;
    }
    if !PKG_HELPER_FAMILIES.contains(&fam) {
        return None;
    }
    if !helper_available {
        return None;
    }
    let mut args = vec![pkg_helper_path().to_string()];
    // Helper message language (decision of 30.09.2026), right after its path.
    args.extend(crate::system_l10n::lang_args(pkg_helper_path()));
    args.extend([fam.to_string(), op.to_string()]);
    if matches!(op, "install" | "remove" | "enable-dm") {
        if packages.is_empty() {
            return None;
        }
        for p in packages {
            // ⛔ A refusal here is NOT a fallback to the raw path: the raw path is
            // closed for such names too: [`install_cmd`] and [`remove_cmd`] return an
            // error before the plan reaches `pkexec`.
            if p.starts_with('-') || !is_safe_pkg_arg(family, p) {
                return None;
            }
        }
        if op == "enable-dm"
            && (packages.len() != 1
                || !DE_HELPER_DISPLAY_MANAGERS
                    .contains(&packages[0].as_str()))
        {
            return None;
        }
        args.extend(packages.iter().cloned());
    } else if op == "switch-dm" {
        // A unit from the supported list; a greeter only for LightDM and only a
        // file name (the helper checks its existence itself).
        let (unit, greeter) = match packages {
            [u] => (u, None),
            [u, g] => (u, Some(g)),
            _ => return None,
        };
        if !dm_switch_units().contains(&unit.as_str()) {
            return None;
        }
        if let Some(g) = greeter {
            if unit != "lightdm" || !greeter_name_ok(g) {
                return None;
            }
        }
        args.extend(packages.iter().cloned());
    } else if op == "update" && !packages.is_empty() {
        return None;
    }
    let mut cmd = vec!["pkexec".to_string()];
    cmd.extend(args);
    Some(cmd)
}

/// pkexec command of file staging: port of `privileged_stage_cmd`. Python's
/// file checks (`os.path.isdir`, `os.path.realpath`) are moved into the
/// parameters `staged_is_dir` / `staged_realpath` (None = does not exist).
pub fn privileged_stage_cmd(
    category: &str,
    name: &str,
    staged: &str,
    staged_is_dir: bool,
    staged_realpath: Option<&str>,
    helper_available: bool,
) -> Option<Vec<String>> {
    if !STAGE_CATEGORIES.contains(&category) {
        return None;
    }
    if name.is_empty() || name.starts_with(['-', '.', '/']) {
        return None;
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        || name.contains("..")
    {
        return None;
    }
    if staged.is_empty() || !staged_is_dir {
        return None;
    }
    let real = staged_realpath?;
    if !(real.starts_with("/tmp/") || real.starts_with("/var/tmp/")) {
        return None;
    }
    if !helper_available {
        return None;
    }
    let mut cmd = vec!["pkexec".to_string(), stage_helper_path().to_string()];
    // Helper message language (decision of 30.09.2026), right after its path.
    cmd.extend(crate::system_l10n::lang_args(stage_helper_path()));
    cmd.extend([category.to_string(), name.to_string(), staged.to_string()]);
    Some(cmd)
}

// ============================================================================
// Package version in the repositories: port of `_clean_pkg_version`/`query_repo_version`
// ============================================================================

/// Port of `_clean_pkg_version`: strip the epoch (`1:50.3` → `50.3`) and the
/// pkgrel/debian revision (`6.7.3-1.1` → `6.7.3`). The user expects the
/// upstream version, not service counters of the update order.
pub fn clean_pkg_version(raw: &str) -> String {
    let mut v = raw.trim();
    if let Some((_, rest)) = v.split_once(':') {
        v = rest;
    }
    if let Some((head, _)) = v.rsplit_once('-') {
        v = head;
    }
    v.trim().to_string()
}

/// Port of the package name validation in `query_repo_version`
/// (`re.fullmatch(r"[A-Za-z0-9@._+-]+")`): protection against argument
/// injection even without a shell.
pub fn valid_pkg_name(pkg: &str) -> bool {
    !pkg.is_empty()
        && pkg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

/// Query for a package version in the repositories: the pure part of `query_repo_version`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoVersionQuery {
    /// The binary whose presence the original checks with `shutil.which` BEFORE
    /// running (no binary → None, the command is not run).
    pub binary: String,
    pub cmd: Cmd,
    /// In the original `apt-cache policy` does NOT check the returncode (no
    /// package → empty stdout → None anyway), for the others `returncode != 0` = None.
    pub check_returncode: bool,
}

/// The command for a package version for the family. None: a family without
/// a query (gentoo/void/nix/alpine/solus/clear/unknown: the UI simply hides
/// the line) or the package name failed validation.
pub fn repo_version_query(family: PkgFamily, package: &str) -> Option<RepoVersionQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv, check_returncode): (&str, Vec<&str>, bool) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Si", pkg], true),
        PkgFamily::Debian | PkgFamily::Alt => {
            ("apt-cache", vec!["apt-cache", "policy", pkg], false)
        }
        PkgFamily::Fedora => ("dnf", vec!["dnf", "info", pkg], true),
        PkgFamily::Suse => {
            ("zypper", vec!["zypper", "--non-interactive", "info", pkg], true)
        }
        _ => return None,
    };
    Some(RepoVersionQuery {
        binary: binary.to_string(),
        // LANG=C is mandatory: in a Russian locale the managers print a translated
        // field name instead of "Version", and the field parser finds nothing.
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), Some(10), true),
        check_returncode,
    })
}

/// Parsing the version query output. None: no field / an empty value /
/// `(none)` from apt.
pub fn parse_repo_version(family: PkgFamily, stdout: &str) -> Option<String> {
    let non_empty = |v: String| if v.is_empty() { None } else { Some(v) };
    match family {
        PkgFamily::Arch => {
            for line in stdout.lines() {
                // The original does not trim the line: `pacman -Si` prints fields from
                // column zero.
                if line.starts_with("Version") {
                    // In the original a line without ':' raises IndexError, the outer `except`
                    // returns None: here the same None, without a panic.
                    let (_, rest) = line.split_once(':')?;
                    return non_empty(clean_pkg_version(rest));
                }
            }
            None
        }
        PkgFamily::Debian | PkgFamily::Alt => {
            for line in stdout.lines() {
                if let Some(rest) = line.trim().strip_prefix("Candidate:") {
                    let cand = rest.trim();
                    if cand.is_empty() || cand == "(none)" {
                        return None;
                    }
                    return non_empty(clean_pkg_version(cand));
                }
            }
            None
        }
        PkgFamily::Fedora | PkgFamily::Suse => {
            for line in stdout.lines() {
                let s = line.trim();
                if s.starts_with("Version") && s.contains(':') {
                    let (_, rest) = s.split_once(':')?;
                    return non_empty(clean_pkg_version(rest));
                }
            }
            None
        }
        _ => None,
    }
}

/// Whether such a target is in the enabled repositories: the answer by the
/// OUTCOME of the same command as [`repo_version_query`]: this question does
/// not need a second trip to the manager.
///
/// `returncode` = `None` when the command could not be run (no manager, a
/// timeout): then the answer is `None` too. `None` and `Some(false)` are
/// DIFFERENT: on `false` the target is rejected and the fork rollback does
/// not run, on `None` the decision stays as before, otherwise on non-Arch we
/// would refuse for no reason.
///
/// The exit code cannot be read for everyone: `apt-cache policy` answers
/// zero even for a nonexistent package (which is why [`RepoVersionQuery`] has
/// the `check_returncode` field), so for Debian/ALT the answer is given by
/// the presence of a version in the output. Families without a query at all
/// answer `None`: "I do not know".
pub fn repo_target_exists(family: PkgFamily, returncode: Option<i32>, stdout: &str) -> Option<bool> {
    let rc = returncode?;
    match family {
        PkgFamily::Arch | PkgFamily::Fedora | PkgFamily::Suse => Some(rc == 0),
        PkgFamily::Debian | PkgFamily::Alt => {
            Some(parse_repo_version(family, stdout).is_some())
        }
        _ => None,
    }
}

// ============================================================================
// Installed package: VERSION and FILE LIST
// ============================================================================
//
// These are two INDEPENDENT questions to the package engine, and neither
// reduces to the existing ones:
//   * "is it installed": [`query_cmd`] + [`pkg_installed_from_run`], the
//     answer is only the exit code, the output is not read at all;
//   * "which version is in the REPOSITORIES": [`repo_version_query`] +
//     [`parse_repo_version`], asked of the network/repository cache.
// Here it is "what is ON THE MACHINE": the installed package's version and
// its file list. Both questions used to be asked with a hardcoded `pacman`
// bypassing the engine (`_pacman_version` / `_pacman_file_list` in
// `rexon/system.py`).

/// A query to the package manager about an installed package.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstalledPkgQuery {
    /// The binary whose presence is checked BEFORE running.
    pub binary: String,
    pub cmd: Cmd,
}

/// How the version appears in the command output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionShape {
    /// `<name> <version>`: how `pacman -Q` prints it.
    NameThenVersion,
    /// The version alone: how `dpkg-query -W -f` and `rpm -q --qf` print it.
    Bare,
}

/// Output form of the [`installed_version_query`] command for the family.
fn installed_version_shape(family: PkgFamily) -> VersionShape {
    match family {
        PkgFamily::Arch => VersionShape::NameThenVersion,
        _ => VersionShape::Bare,
    }
}

/// The command for the version of an INSTALLED package.
///
/// `None`: nothing to ask with, and we will not invent a command: for gentoo,
/// alpine, void, nix, solus and clear the answer format was not checked by
/// hand, and silently misparsing foreign output is worse than honestly
/// answering "I do not know" (the same trick as [`repo_version_query`], which
/// also returns None for these families). The old code asked a hardcoded
/// `pacman` and answered None on all non-Arch anyway.
pub fn installed_version_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Q", pkg]),
        PkgFamily::Debian => (
            "dpkg-query",
            vec!["dpkg-query", "-W", "-f=${Version}", pkg],
        ),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => (
            "rpm",
            vec!["rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", pkg],
        ),
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        // Timeout by command class (query, 30 s), as the old safe_run had without an
        // explicit number. LANG=C: in a Russian locale the managers print localized
        // fields and errors, and the parser does not find them.
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// The installed package version from the [`installed_version_query`] output.
///
/// `ok`: the command exited with zero (did not start at all → false). An
/// empty output and an output without a version give None: "I do not know",
/// not an empty string.
pub fn parse_installed_version(family: PkgFamily, ok: bool, stdout: &str) -> Option<String> {
    if !ok {
        return None;
    }
    let value = match installed_version_shape(family) {
        VersionShape::NameThenVersion => stdout.split_whitespace().nth(1)?.to_string(),
        VersionShape::Bare => stdout.trim().to_string(),
    };
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// The command for the installed package's file list. `None`: a family whose
/// answer format was not checked (see [`installed_version_query`]).
pub fn package_files_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if !valid_pkg_name(pkg) {
        return None;
    }
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Ql", pkg]),
        PkgFamily::Debian => ("dpkg", vec!["dpkg", "-L", pkg]),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => ("rpm", vec!["rpm", "-ql", pkg]),
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// Raw output of [`package_files_query`] or None.
///
/// ⚠️ An empty output with a zero code is NOT an error: that is how a
/// metapackage with no files of its own answers (checked on a live machine:
/// `pacman -Ql base` prints zero bytes with code 0). The old Python returned
/// `""` here (`r.stdout or ""`): the same thing, and that is right: "no
/// files" and "could not ask" must differ.
pub fn package_files_output(ok: bool, stdout: &str) -> Option<String> {
    if ok {
        Some(stdout.to_string())
    } else {
        None
    }
}

/// Package file paths from the [`package_files_query`] output.
///
/// The only parser of this output in the whole project. There are two forms:
/// `pacman -Ql` prints `<package> <path>`, while `dpkg -L` and `rpm -ql`
/// print only the path. A common filter for both: we take absolute paths and
/// drop directories (they end with "/"): this also filters out service
/// headers like "package diverts others to:".
pub fn package_file_paths(family: PkgFamily, output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| match family {
            PkgFamily::Arch => line.split_once(' ').map(|(_pkg, path)| path.trim()),
            _ => Some(line.trim()),
        })
        .filter(|path| path.starts_with('/') && !path.ends_with('/'))
        .map(str::to_string)
        .collect()
}

/// The "list EVERYTHING installed" command: a question WITHOUT a package argument.
///
/// The engine's fourth independent question: `query_cmd` asks about one
/// named package, while here the name is not known in advance. The project
/// kept this question as two copies of a hardcoded `pacman -Qq` bypassing
/// the engine, in `detect_plasma_forks` (`rexon/system.py`) and in
/// `aur_client.installed_set`; both now come here.
///
/// `None`: a family whose answer format was not checked by hand (the same
/// trick as [`installed_version_query`]): inventing a command and silently
/// parsing foreign output is worse than honestly answering "I do not know".
pub fn installed_packages_query(family: PkgFamily) -> Option<InstalledPkgQuery> {
    let (binary, argv): (&str, Vec<&str>) = match family {
        PkgFamily::Arch => ("pacman", vec!["pacman", "-Qq"]),
        PkgFamily::Debian => (
            "dpkg-query",
            vec!["dpkg-query", "-W", "-f=${Package}\n"],
        ),
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => {
            ("rpm", vec!["rpm", "-qa", "--qf", "%{NAME}\n"])
        }
        _ => return None,
    };
    Some(InstalledPkgQuery {
        binary: binary.to_string(),
        cmd: Cmd::new(argv.into_iter().map(String::from).collect(), None, true),
    })
}

/// Installed package names from the [`installed_packages_query`] output.
///
/// `ok`: the command exited with zero (did not start at all → false). One
/// parser for all families: the commands above print one name per line, and
/// `split` on whitespace also survives extra empty lines. The order is kept:
/// it is set by those who need the answer ([`filter_plasma_forks`] sorts itself).
pub fn parse_installed_packages(ok: bool, stdout: &str) -> Vec<String> {
    if !ok {
        return Vec::new();
    }
    stdout.split_whitespace().map(str::to_string).collect()
}

/// The command for the METADATA of an installed package (`pacman -Qi`): raw
/// material for [`fork_stock_candidates`] and other readers of the
/// `Provides`/`Replaces` fields.
///
/// Arch only, and this is not laziness: the field parser
/// ([`crate::system::parse_qi_field`]) is written for the `pacman -Qi`
/// format; `dpkg -s` and `rpm -qi` name the fields differently, and giving
/// them to the same parser would silently produce empty lists. On other
/// families the honest answer is `None`.
pub fn installed_meta_query(family: PkgFamily, package: &str) -> Option<InstalledPkgQuery> {
    let pkg = package.trim();
    if family != PkgFamily::Arch || !valid_pkg_name(pkg) {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        cmd: Cmd::new(
            vec!["pacman".into(), "-Qi".into(), pkg.to_string()],
            None,
            true,
        ),
    })
}

/// The command for the members of a package GROUP: the engine's sixth
/// independent question.
///
/// It does not reduce to any of the five earlier ones: `query_cmd` and
/// [`installed_version_query`] ask about a NAMED package,
/// [`installed_packages_query`] about everything at once, while here the
/// subject is a group, and the answer is the list of its members. The project
/// kept this question as a hardcoded `pacman -Qg` bypassing the engine in two
/// places: the `PackageManager._pacman_query_stdout` callback in
/// `rexon/system.py` and the "qg" step of the removal plan right here in
/// [`remove_plan`]; both copies now come here.
///
/// Arch only, and this is not laziness. Far from every family has the notion
/// of a "package group": Debian has none at all (a metapackage is an
/// ordinary package, already answered by [`query_cmd`]), and dnf and zypper
/// call groups differently (`dnf group info`, `zypper info --type pattern`),
/// ask with other subcommands and print a completely different format, not
/// checked by hand. Inventing a command and silently feeding foreign output
/// to a parser written for "group member" lines would quietly give empty
/// lists instead of an answer. The honest answer of these families is
/// `None`, and on it the asker leaves the name as is: there it is an
/// ordinary package.
pub fn package_group_query(family: PkgFamily, group: &str) -> Option<InstalledPkgQuery> {
    let name = group.trim();
    if family != PkgFamily::Arch || !valid_pkg_name(name) {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        cmd: Cmd::new(
            vec!["pacman".into(), "-Qg".into(), name.to_string()],
            None,
            true,
        ),
    })
}

// ============================================================================
// Install preview: what will REALLY be installed (pacman -Sg / -Sp)
// ============================================================================
//
// WHY. The desktop card counted the NAMES from the core database: for
// Pantheon that is "pantheon lightdm lightdm-gtk-greeter", "3 packages to
// install", although `pantheon` is a group of 63 packages, and with
// dependencies more than 80 get installed on a clean system. And the install
// failed entirely when ONE optional group member in the repository could not
// be installed (08.10.2026: switchboard-plug-online-accounts was waiting for
// a rebuild against the new evolution-data-server): the person only learned
// about it from the pacman log after the password.
//
// Both troubles are cured by one question without root: `pacman -Sp
// --needed` prints exactly what the transaction will install (groups
// expanded, dependencies taken, installed ones skipped), and on an
// unresolvable dependency it names the culprit. Arch only: the other
// families have no such unprivileged "trial" transaction or its output was
// not checked by hand: there the honest answer is `None`, and the card keeps
// the old name count.

/// How many times to drop uninstallable group members and ask again. Each
/// round drops all found ones at once; more than three rounds would mean a
/// chain of breakages the card could not describe anyway.
const PREVIEW_MAX_ROUNDS: usize = 4;

/// Result of the install preview of a package set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct InstallPreview {
    /// The set's groups and the number of their members (without repeats across repositories).
    pub groups: Vec<(String, usize)>,
    /// What to give the package manager. The same as the original set while
    /// there is nothing to drop; otherwise groups are expanded into members
    /// without [`Self::skipped`].
    pub targets: Vec<String>,
    /// The set's packages after expanding groups (what the person "gets" as the
    /// desktop, without dependencies and without [`Self::skipped`]).
    pub members: Vec<String>,
    /// What the transaction installs ANEW: set members and their dependencies
    /// that are not in the system. Empty: everything is in place.
    pub new_packages: Vec<String>,
    /// What the transaction UPDATES: packages from its plan that are already
    /// installed: a new build of them came out in the repository (`--needed`
    /// skips only the same version). This does not count as missing: whether a
    /// package is installed is decided by the installed list
    /// ([`installed_packages_query`]), not by the transaction plan.
    pub updates: Vec<String>,
    /// Installed packages of the set whose OWN build is newer than the
    /// repository (built by hand or from the AUR, with a patch the repository
    /// does not have yet). `pacman -S --needed` skips only the same version and
    /// would ROLL such a package BACK to the repository one, so it is dropped
    /// from the transaction and not called an update. Live case of 08.11.2026:
    /// our Caja 1.28.0-6.100 with a transparent background for live wallpapers
    /// versus 1.28.0-6.1.
    pub kept_local: Vec<String>,
    /// Optional group members that cannot be installed now (an unresolvable
    /// dependency or not in the repository): dropped from the install.
    pub skipped: Vec<String>,
    /// Packages without which the install will not go through: named in the set
    /// directly, or the breakage is deeper, in the dependencies. Non-empty: the
    /// install is not possible.
    pub blocked: Vec<String>,
    /// What of the incoming conflicts with the installed (`Conflicts With` from
    /// either side). Non-empty: the install is not possible: pacman with
    /// `--noconfirm` refuses to remove what is installed, and Rexon does not
    /// take it upon itself to remove someone else's desktop ([`conflict_explanation`]).
    pub conflicts: Vec<PkgConflict>,
    /// Optional group members dropped from the install because they pull in a
    /// package that conflicts with an installed one (the Pantheon login screen
    /// pulls in mutter49, while mutter 51 is held by GNOME and Budgie). The
    /// desktop is installed without them, and the person is told what will be
    /// missing and why ([`conflict_skip_explanation`]). Mandatory parts (the
    /// desktop session and everything it pulls in) never get here: then it is
    /// [`Self::conflicts`].
    pub conflict_skipped: Vec<ConflictSkip>,
    /// Packages from [`Self::targets`] that the transaction installs ANEW only as
    /// dependencies of the set: a DKMS module and headers for the installed
    /// kernels ([`kernel_module_extras`]). Installed marked `--asdeps`, so that
    /// removing the desktop removes them; already installed ones are not included.
    pub deps_only: Vec<String>,
    /// What of the newly installed takes effect only after a reboot: the kernel
    /// and kernel modules (DKMS or prebuilt for a kernel): [`restart_packages`].
    /// Empty: logging out is enough.
    pub restart_packages: Vec<String>,
}

/// A package conflict: what is being installed, and which installed package
/// it cannot coexist with.
///
/// Besides the names, whose packages these are: the person is told "Pantheon
/// cannot go next to Budgie", not "mutter49 conflicts with mutter". Empty
/// fields mean no information (parsing a failed transaction): then the
/// desktop is guessed by the key packages table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PkgConflict {
    pub new_package: String,
    pub installed: String,
    /// Versions from `pacman -Si`/`-Qi` (as pacman writes them), empty: unknown.
    pub new_version: String,
    pub installed_version: String,
    /// It is one program in different versions (`mutter49` and `mutter`): then
    /// the explanation talks about versions, not about a "package conflict".
    pub same_program: bool,
    /// Desktops `new_package` is installed for (by the set's `Depends On`).
    pub new_desktops: Vec<String>,
    /// Desktops that hold the installed package (by `Required By`).
    pub installed_desktops: Vec<String>,
    /// Who directly requires the installed package (`Required By`).
    pub required_by: Vec<String>,
    /// The "installed" one is actually also being installed in this same
    /// transaction (on a machine without mutter the `pantheon` group pulls in
    /// both mutter from gala and mutter49 from the login screen): `-Sp` does not
    /// catch this, and the real transaction fails. Then
    /// [`Self::installed_desktops`] are the set's desktops that need it.
    pub installed_is_new: bool,
}

/// A group member that is not installed because of a conflict
/// ([`InstallPreview::conflict_skipped`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConflictSkip {
    /// The dropped group member (`lightdm-pantheon-greeter`).
    pub package: String,
    /// The desktop it belongs to (`Pantheon`); empty: cannot be found out.
    pub desktop: String,
    /// The conflict it was dropped for (`mutter49` versus `mutter`).
    pub conflict: PkgConflict,
}

impl InstallPreview {
    /// The install will go through (possibly without [`Self::skipped`]).
    pub fn installable(&self) -> bool {
        self.blocked.is_empty() && self.conflicts.is_empty()
    }

    /// Set packages not in the system (installed anew).
    pub fn missing_members(&self) -> Vec<String> {
        self.members
            .iter()
            .filter(|m| self.new_packages.contains(m))
            .cloned()
            .collect()
    }

    /// How many set packages are already installed.
    pub fn installed_members(&self) -> usize {
        self.members.len() - self.missing_members().len()
    }

    /// How many dependencies the transaction adds on top of the set.
    pub fn dependency_count(&self) -> usize {
        self.new_packages.len() - self.missing_members().len()
    }
}

/// The question about group members in the REPOSITORIES (`pacman -Sg
/// names…`). pacman rejects non-groups with a stderr line and code 1, but
/// still prints the groups from the same call, so the exit code does not
/// matter to the parser.
pub fn sync_groups_cmd(family: PkgFamily, names: &[String]) -> Option<Cmd> {
    if family != PkgFamily::Arch || names.is_empty() || !names.iter().all(|n| valid_pkg_name(n)) {
        return None;
    }
    let mut argv: Vec<String> = vec!["pacman".into(), "-Sg".into()];
    argv.extend(names.iter().cloned());
    Some(Cmd::new(argv, None, true))
}

/// A trial transaction without root: `pacman -Sp --needed` prints, one name
/// per line, everything that will be installed, and touches nothing.
pub fn sync_preview_cmd(family: PkgFamily, targets: &[String]) -> Option<Cmd> {
    if family != PkgFamily::Arch || targets.is_empty() || !targets.iter().all(|n| valid_pkg_name(n)) {
        return None;
    }
    let mut argv: Vec<String> = vec![
        "pacman".into(),
        "-Sp".into(),
        "--needed".into(),
        "--noconfirm".into(),
        "--print-format".into(),
        "%n".into(),
    ];
    argv.extend(targets.iter().cloned());
    Some(Cmd::new(argv, None, true))
}

/// Groups from the `pacman -Sg` output: "group member" per line, members
/// without repeats (the same group comes from several repositories), in
/// order of first appearance.
pub fn parse_sync_groups(stdout: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.split_whitespace();
        let (Some(group), Some(member), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !valid_pkg_name(group) || !valid_pkg_name(member) {
            continue;
        }
        let slot = match out.iter().position(|(g, _)| g == group) {
            Some(i) => i,
            None => {
                out.push((group.to_string(), Vec::new()));
                out.len() - 1
            }
        };
        if !out[slot].1.iter().any(|m| m == member) {
            out[slot].1.push(member.to_string());
        }
    }
    out
}

/// Culprits of a trial transaction refusal (output under LANG=C): the
/// package whose dependency is unresolvable ("unable to satisfy dependency
/// 'x' required by PACKAGE"), and a name not in the repositories ("target
/// not found: NAME").
pub fn parse_preview_culprits(stderr: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in stderr.lines() {
        let name = if let Some((_, tail)) = line.split_once("required by ") {
            tail.trim()
        } else if let Some((_, tail)) = line.split_once("target not found: ") {
            tail.trim()
        } else {
            continue;
        };
        if valid_pkg_name(name) && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// A command answer for the preview: (code, stdout, stderr); `None`: the
/// command did not answer at all.
pub type PreviewAnswer = Option<(i32, String, String)>;

/// What installing the `packages` set will install, and whether it can start.
///
/// `run` executes the command (a live run or a fake answer in a check).
/// `None`: could not ask (not Arch, no pacman, the command is silent): then
/// the caller keeps the old behaviour, inventing nothing.
///
/// An uninstallable GROUP member is dropped: a group is a set "to taste",
/// and a desktop without one settings module is better than no desktop. A
/// package named in the set directly (session, login screen) and a breakage
/// in the dependencies cannot be dropped: then [`InstallPreview::blocked`].
pub fn install_preview(
    family: PkgFamily,
    packages: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Option<InstallPreview> {
    if family != PkgFamily::Arch || packages.is_empty() {
        return None;
    }
    let groups_cmd = sync_groups_cmd(family, packages)?;
    let (_, groups_out, _) = run(&groups_cmd)?;
    let groups: Vec<(String, Vec<String>)> = parse_sync_groups(&groups_out)
        .into_iter()
        .filter(|(g, _)| packages.iter().any(|p| p == g))
        .collect();
    // The set after expanding groups: direct names plus group members.
    let mut expanded: Vec<String> = Vec::new();
    for p in packages {
        match groups.iter().find(|(g, _)| g == p) {
            Some((_, members)) => {
                for m in members {
                    if !expanded.contains(m) {
                        expanded.push(m.clone());
                    }
                }
            }
            None => {
                if !expanded.contains(p) {
                    expanded.push(p.clone());
                }
            }
        }
    }
    let is_group_member = |name: &str| {
        !packages.iter().any(|p| p == name)
            && groups.iter().any(|(_, members)| members.iter().any(|m| m == name))
    };

    let mut preview = InstallPreview {
        groups: groups.iter().map(|(g, m)| (g.clone(), m.len())).collect(),
        targets: packages.to_vec(),
        members: expanded.clone(),
        ..InstallPreview::default()
    };
    // Kernel modules for the installed kernels ([`kernel_module_extras`]): added
    // to the transaction once, survive dropping group members and are not part
    // of the "desktop packages" (`members`): they are dependencies.
    let mut kernel_extras: Vec<String> = Vec::new();
    for _ in 0..=PREVIEW_MAX_ROUNDS {
        let cmd = sync_preview_cmd(family, &preview.targets)?;
        let (rc, stdout, stderr) = run(&cmd)?;
        if rc == 0 {
            let planned: Vec<String> = stdout
                .lines()
                .map(str::trim)
                .filter(|l| valid_pkg_name(l))
                .map(str::to_string)
                .collect();
            let installed = installed_names(family, run);
            if kernel_extras.is_empty() {
                let extras = kernel_module_extras(family, &planned, installed.as_deref(), run);
                if !extras.is_empty() {
                    for e in &extras {
                        if !preview.targets.contains(e) {
                            preview.targets.push(e.clone());
                        }
                    }
                    kernel_extras = extras;
                    continue;
                }
            }
            let (updates, new_packages) = split_installed(planned, installed.clone());
            // An own build newer than the repository: it got into the plan only because
            // the version did not match: drop it and recompute the plan.
            let newer_local = locally_newer(&updates, run);
            if newer_local.iter().any(|p| !preview.kept_local.contains(p)) {
                for p in newer_local {
                    if !preview.kept_local.contains(&p) {
                        preview.kept_local.push(p);
                    }
                }
                rebuild_targets(&mut preview, &expanded, &kernel_extras);
                if preview.targets.is_empty() {
                    return Some(preview);
                }
                continue;
            }
            // ⚠️ `-Sp` does NOT check conflicts with installed packages (measured
            // 19.10.2026: Deepin with KWin installed gives code 0), while the real
            // transaction fails on them. We ask `Conflicts With` ourselves.
            let mut new_info: Vec<PkgInfo> = Vec::new();
            if let Some(installed) = &installed {
                let (conflicts, info) = package_conflicts(family, &new_packages, installed, run);
                // A conflict pulled in only by an optional group member (a login screen, an
                // app) does not hold the desktop: the member is dropped, and the plan is
                // recomputed without it. A mandatory one means a refusal.
                let drops = conflict_drops(&conflicts, &expanded, packages, &is_group_member, &info);
                if !drops.is_empty() {
                    for d in drops {
                        if !preview.conflict_skipped.iter().any(|s| s.package == d.package) {
                            preview.members.retain(|m| *m != d.package);
                            preview.conflict_skipped.push(d);
                        }
                    }
                    rebuild_targets(&mut preview, &expanded, &kernel_extras);
                    if preview.targets.is_empty() {
                        return Some(preview);
                    }
                    continue;
                }
                preview.conflicts = conflicts;
                new_info = info;
            }
            preview.restart_packages = restart_packages(&new_packages, &new_info);
            preview.deps_only = kernel_extras.iter().filter(|e| new_packages.contains(e)).cloned().collect();
            preview.new_packages = new_packages;
            preview.updates = updates;
            return Some(preview);
        }
        // ⚠️ pacman writes the "error: …" line to stderr, and the ":: unable to
        // satisfy dependency … required by …" lines themselves to stdout (measured
        // 08.10.2026): both streams are parsed.
        let culprits = parse_preview_culprits(&format!("{stdout}\n{stderr}"));
        if culprits.is_empty() {
            // A refusal the parser does not understand (database locked, no network to
            // the mirror, etc.): claim nothing.
            return None;
        }
        let (droppable, hard): (Vec<String>, Vec<String>) =
            culprits.into_iter().partition(|c| is_group_member(c));
        if !hard.is_empty() {
            preview.blocked = hard;
            preview.skipped.extend(droppable);
            return Some(preview);
        }
        preview.skipped.extend(droppable);
        rebuild_targets(&mut preview, &expanded, &[]);
        preview.members = preview.targets.clone();
        for e in &kernel_extras {
            if !preview.targets.contains(e) {
                preview.targets.push(e.clone());
            }
        }
    }
    None
}

/// Transaction targets anew: the set after expanding groups without what was
/// dropped (uninstallable, own build newer, conflicting optional) plus kernel
/// modules for the installed kernels.
fn rebuild_targets(preview: &mut InstallPreview, expanded: &[String], kernel_extras: &[String]) {
    preview.targets = expanded
        .iter()
        .filter(|m| {
            !preview.skipped.contains(m)
                && !preview.kept_local.contains(m)
                && !preview.conflict_skipped.iter().any(|s| &s.package == *m)
        })
        .cloned()
        .collect();
    for e in kernel_extras {
        if !preview.targets.contains(e) {
            preview.targets.push(e.clone());
        }
    }
}

/// Which group members to drop to resolve the `conflicts` conflicts.
///
/// The mandatory core of a desktop: packages named in the set directly, the
/// desktops' key packages (the session from the desktop database,
/// `pantheon-session`, the window manager `gala`, the
/// [`CONFLICT_DESKTOP_PACKAGES`] table) and everything they pull in by
/// `Depends On`. A conflicting package inside the core: there is nothing to
/// resolve the conflict with, the member is not dropped (a refusal,
/// [`conflict_explanation`]). Otherwise group members outside the core that
/// pull it in are dropped: the Pantheon login screen
/// (`lightdm-pantheon-greeter` → `mutter49`) with mutter 51 installed.
/// `info` is the `pacman -Si` information about the incoming packages;
/// relations are taken only from it.
fn conflict_drops(
    conflicts: &[PkgConflict],
    expanded: &[String],
    named: &[String],
    is_group_member: &dyn Fn(&str) -> bool,
    info: &[PkgInfo],
) -> Vec<ConflictSkip> {
    if conflicts.is_empty() {
        return Vec::new();
    }
    // Direct dependencies by `Depends On`, taking `Provides` into account.
    let deps = |name: &str| -> Vec<&str> {
        let Some(i) = info.iter().find(|i| i.name == name) else { return Vec::new() };
        info.iter()
            .filter(|o| i.depends.iter().any(|d| *d == o.name || o.provides.contains(d)))
            .map(|o| o.name.as_str())
            .collect()
    };
    let closure = |roots: Vec<&str>| -> std::collections::HashSet<String> {
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut stack: Vec<String> = roots.into_iter().map(str::to_string).collect();
        while let Some(n) = stack.pop() {
            if seen.len() > 4000 || !seen.insert(n.clone()) {
                continue;
            }
            stack.extend(deps(&n).into_iter().map(str::to_string));
        }
        seen
    };
    let essential = |name: &str| {
        named.iter().any(|n| n == name)
            || CONFLICT_DESKTOP_PACKAGES.iter().any(|(p, _)| *p == name)
            || crate::system::DE_DATABASE.iter().any(|(_, e)| e.check_package == name)
    };
    let core = closure(expanded.iter().map(String::as_str).filter(|m| essential(m) || !is_group_member(m)).collect());
    let mut out: Vec<ConflictSkip> = Vec::new();
    for c in conflicts {
        // A conflict inside the set is declared by either side: the one outside the
        // core can be dropped, and explained from its side.
        let c = if !core.contains(&c.new_package) {
            c.clone()
        } else if c.installed_is_new && !core.contains(&c.installed) {
            PkgConflict {
                new_package: c.installed.clone(),
                installed: c.new_package.clone(),
                new_version: c.installed_version.clone(),
                installed_version: c.new_version.clone(),
                new_desktops: c.installed_desktops.clone(),
                installed_desktops: c.new_desktops.clone(),
                ..c.clone()
            }
        } else {
            return Vec::new();
        };
        let pullers: Vec<&String> = expanded
            .iter()
            .filter(|m| is_group_member(m) && !core.contains(m.as_str()))
            .filter(|m| closure(vec![m.as_str()]).contains(&c.new_package))
            .collect();
        if pullers.is_empty() {
            // Nothing optional pulls in the conflicting package: nothing to remove, the
            // refusal stays.
            return Vec::new();
        }
        for m in pullers {
            if out.iter().any(|s| &s.package == m) {
                continue;
            }
            let desktop = own_desktop(m, info.iter().find(|i| &i.name == m))
                .map(str::to_string)
                .or_else(|| c.new_desktops.first().cloned())
                .unwrap_or_default();
            out.push(ConflictSkip { package: m.clone(), desktop, conflict: c.clone() });
        }
    }
    out
}

// ── Kernel modules for the installed kernels ──────────────────────────────
//
// Some Arch packages depend on a virtual kernel module
// (`DEEPIN-ANYTHING-MODULE`, `VHBA-MODULE`, `NVIDIA-MODULE`), and it has two
// providers: a prebuilt module for the stock kernel (`deepin-anything-arch`,
// `Depends On: linux`) and a DKMS variant. pacman with `--noconfirm` takes
// the first, and on a machine with another kernel (CachyOS: `linux-cachyos`)
// installs an extra `linux` kernel that then hangs around in the boot loader
// (20.10.2026, Deepin). So: if the transaction pulls in `linux` and it is
// not among the installed kernels, each package that needs exactly `linux`
// is replaced by the DKMS variant, and headers for EACH installed kernel go
// with it (otherwise DKMS cannot build).

/// The stock Arch kernel: prebuilt modules depend on it.
const ARCH_KERNEL: &str = "linux";

/// Prebuilt modules whose DKMS variant is not named by the rule "name-dkms" /
/// "name without -arch, plus -dkms".
const DKMS_ALTERNATIVES: &[(&str, &str)] = &[("virtualbox-host-modules-arch", "virtualbox-host-dkms")];

/// Candidates for the DKMS replacement of the prebuilt module `name`, in
/// order of preference; which of them are in the repositories is up to the caller.
pub fn dkms_candidates(name: &str) -> Vec<String> {
    let mut out: Vec<String> = DKMS_ALTERNATIVES
        .iter()
        .filter(|(prebuilt, _)| *prebuilt == name)
        .map(|(_, dkms)| dkms.to_string())
        .collect();
    if let Some(stem) = name.strip_suffix("-arch") {
        out.push(format!("{stem}-dkms"));
    }
    out.push(format!("{name}-dkms"));
    out.dedup();
    out
}

/// Whether a replacement is needed: the transaction pulls in the stock kernel
/// while others are installed. If the kernels cannot be found out (a
/// container without a kernel, `find` is silent), we do not touch it: there
/// is nothing to build a DKMS module for.
pub fn needs_dkms_modules(planned: &[String], kernels: &[String]) -> bool {
    planned.iter().any(|p| p == ARCH_KERNEL) && !kernels.is_empty() && !kernels.iter().any(|k| k == ARCH_KERNEL)
}

/// Prebuilt modules from the plan: packages depending on the stock kernel.
pub fn prebuilt_module_packages(planned_info: &[PkgInfo]) -> Vec<String> {
    planned_info
        .iter()
        .filter(|i| i.name != ARCH_KERNEL && i.depends.iter().any(|d| d == ARCH_KERNEL))
        .map(|i| i.name.clone())
        .collect()
}

/// Headers of installed kernels that are not there yet (`linux-cachyos` →
/// `linux-cachyos-headers`).
pub fn missing_kernel_headers(kernels: &[String], installed: Option<&[String]>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in kernels {
        let h = format!("{k}-headers");
        if !installed.is_some_and(|i| i.contains(&h)) && !out.contains(&h) {
            out.push(h);
        }
    }
    out
}

/// Installed kernels: the packages owning `/usr/lib/modules/*/pkgbase` (that
/// is how mkinitcpio and kernel-install recognise them). Empty: could not
/// find out.
fn installed_kernels(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Vec<String> {
    let find = Cmd::new(
        names_argv(&["find", "/usr/lib/modules", "-mindepth", "2", "-maxdepth", "2", "-name", "pkgbase"]),
        None,
        true,
    );
    let Some((_, files, _)) = run(&find) else {
        return Vec::new();
    };
    let files: Vec<String> = files
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("/usr/lib/modules/") && !l.contains(char::is_whitespace))
        .map(str::to_string)
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    let mut argv = names_argv(&["pacman", "-Qqo"]);
    argv.extend(files);
    // A foreign file (a kernel installed bypassing pacman) is rejected by pacman
    // in stderr with code 1, but it still prints the owners of the rest.
    let Some((_, owners, _)) = run(&Cmd::new(argv, None, true)) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for o in owners.lines().map(str::trim).filter(|o| valid_pkg_name(o)) {
        if !out.iter().any(|k| k == o) {
            out.push(o.to_string());
        }
    }
    out
}

fn names_argv(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// What to add to the `planned` transaction so that kernel modules fit the
/// installed kernels instead of pulling in the stock `linux`: DKMS variants
/// and headers. Empty: nothing to change (or nothing to replace with: then
/// as it was).
pub fn kernel_module_extras(
    family: PkgFamily,
    planned: &[String],
    installed: Option<&[String]>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Vec<String> {
    if family != PkgFamily::Arch || !planned.iter().any(|p| p == ARCH_KERNEL) {
        return Vec::new();
    }
    let kernels = installed_kernels(run);
    if !needs_dkms_modules(planned, &kernels) || !planned.iter().all(|n| valid_pkg_name(n)) {
        return Vec::new();
    }
    let sync_info = |names: &[String]| -> Vec<PkgInfo> {
        let mut argv = names_argv(&["pacman", "-Si"]);
        argv.extend(names.iter().cloned());
        run(&Cmd::new(argv, Some(60), true))
            .map(|(_, stdout, _)| parse_pkg_info(&stdout))
            .unwrap_or_default()
    };
    let prebuilt = prebuilt_module_packages(&sync_info(planned));
    if prebuilt.is_empty() {
        return Vec::new();
    }
    let headers = missing_kernel_headers(&kernels, installed);
    let mut wanted: Vec<String> = prebuilt.iter().flat_map(|p| dkms_candidates(p)).collect();
    wanted.extend(headers.iter().cloned());
    let available: Vec<String> = sync_info(&wanted).into_iter().map(|i| i.name).collect();
    let mut out: Vec<String> = Vec::new();
    for p in &prebuilt {
        match dkms_candidates(p).into_iter().find(|c| available.contains(c)) {
            Some(c) if !out.contains(&c) => out.push(c),
            Some(_) => {}
            // Nothing to replace with: leave pacman its choice entirely.
            None => return Vec::new(),
        }
    }
    // There are no headers for the own kernel in the repositories (a kernel
    // from the AUR): DKMS will not build for it, but there will be no extra kernel either.
    out.extend(headers.into_iter().filter(|h| available.contains(h)));
    out
}

/// Names from the `Conflicts With` field per block of `pacman -Si` / `-Qi`
/// output (LANG=C): (package, what it conflicts with). Version constraints
/// (`foo<2`) are skipped: they cannot be decided without comparing versions,
/// and a false refusal is worse than a miss: the real transaction will say
/// its piece anyway, and [`parse_transaction_conflicts`] will explain it.
pub fn parse_info_conflicts(stdout: &str) -> Vec<(String, Vec<String>)> {
    parse_pkg_info(stdout)
        .into_iter()
        .filter(|i| !i.conflicts.is_empty())
        .map(|i| (i.name, i.conflicts))
        .collect()
}

/// Package information from a `pacman -Si` / `-Qi` block (LANG=C): what is
/// needed to name a conflict by desktop: the version, groups, what it
/// provides, what it depends on, who requires it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PkgInfo {
    pub name: String,
    pub version: String,
    pub groups: Vec<String>,
    /// `Provides` without versions (`libmutter-17.so=0-64` → `libmutter-17.so`).
    pub provides: Vec<String>,
    /// `Depends On` without version constraints.
    pub depends: Vec<String>,
    pub required_by: Vec<String>,
    /// `Conflicts With`; version-constrained ones (`foo<2`) skipped.
    pub conflicts: Vec<String>,
}

/// `pacman -Si` / `-Qi` blocks (LANG=C). One package from two repositories is
/// one entry: the version and relations from the first block, conflicts are merged.
pub fn parse_pkg_info(stdout: &str) -> Vec<PkgInfo> {
    fn names(values: &str, strip_version: bool) -> Vec<String> {
        values
            .split_whitespace()
            .filter(|v| *v != "None")
            .filter_map(|v| {
                if strip_version {
                    v.split(['<', '>', '=']).next()
                } else if v.contains(['<', '>', '=']) {
                    None
                } else {
                    Some(v)
                }
            })
            .filter(|v| valid_pkg_name(v))
            .map(str::to_string)
            .collect()
    }
    fn extend(have: &mut Vec<String>, more: Vec<String>) {
        for v in more {
            if !have.contains(&v) {
                have.push(v);
            }
        }
    }
    let mut out: Vec<PkgInfo> = Vec::new();
    let mut cur: Option<PkgInfo> = None;
    let mut field = String::new();
    let flush = |cur: &mut Option<PkgInfo>, out: &mut Vec<PkgInfo>| {
        let Some(info) = cur.take() else { return };
        if info.name.is_empty() {
            return;
        }
        match out.iter_mut().find(|i| i.name == info.name) {
            Some(have) => extend(&mut have.conflicts, info.conflicts),
            None => out.push(info),
        }
    };
    let apply = |cur: &mut Option<PkgInfo>, key: &str, value: &str| {
        let Some(info) = cur.as_mut() else { return };
        match key {
            "Version" if info.version.is_empty() => info.version = value.trim().to_string(),
            "Groups" => extend(&mut info.groups, names(value, false)),
            "Provides" => extend(&mut info.provides, names(value, true)),
            "Depends On" => extend(&mut info.depends, names(value, true)),
            "Required By" => extend(&mut info.required_by, names(value, false)),
            "Conflicts With" => extend(&mut info.conflicts, names(value, false)),
            _ => {}
        }
    };
    for line in stdout.lines() {
        if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            // A wrapped long field: the continuation of what was above.
            let key = field.clone();
            apply(&mut cur, &key, line);
            continue;
        }
        let Some((key, value)) = line.split_once(" : ") else {
            if line.trim().is_empty() {
                flush(&mut cur, &mut out);
                field.clear();
            }
            continue;
        };
        field = key.trim().to_string();
        if field == "Name" {
            flush(&mut cur, &mut out);
            cur = Some(PkgInfo { name: value.trim().to_string(), ..PkgInfo::default() });
            continue;
        }
        let key = field.clone();
        apply(&mut cur, &key, value);
    }
    flush(&mut cur, &mut out);
    out
}

/// Conflicts of what will be installed (`new_packages`) with the installed:
/// both ways: declared by the incoming packages (`pacman -Si`) and declared by
/// the installed ones (`pacman -Qi`). Arch only; if the command did not
/// answer, empty (as before: the check failed, we claim nothing).
/// Also returns the `pacman -Si` information about the incoming packages:
/// [`restart_packages`] uses it to decide whether a reboot is needed, without
/// a second query.
fn package_conflicts(
    family: PkgFamily,
    new_packages: &[String],
    installed: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> (Vec<PkgConflict>, Vec<PkgInfo>) {
    if family != PkgFamily::Arch || new_packages.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let installed: std::collections::HashSet<&str> = installed.iter().map(String::as_str).collect();
    let mut out: Vec<PkgConflict> = Vec::new();
    let mut add = |new_package: &str, inst: &str| {
        let c = PkgConflict {
            new_package: new_package.to_string(),
            installed: inst.to_string(),
            ..PkgConflict::default()
        };
        if new_package != inst && !out.contains(&c) {
            out.push(c);
        }
    };
    let mut new_info: Vec<PkgInfo> = Vec::new();
    if new_packages.iter().all(|n| valid_pkg_name(n)) {
        let mut argv: Vec<String> = vec!["pacman".into(), "-Si".into()];
        argv.extend(new_packages.iter().cloned());
        if let Some((_, stdout, _)) = run(&Cmd::new(argv, Some(60), true)) {
            new_info = parse_pkg_info(&stdout);
            for info in new_info.iter().filter(|i| new_packages.contains(&i.name)) {
                for c in info.conflicts.iter().filter(|c| installed.contains(c.as_str()) || new_packages.contains(c)) {
                    add(&info.name, c);
                }
            }
        }
    }
    let mut installed_info: Vec<PkgInfo> = Vec::new();
    if let Some((_, stdout, _)) = run(&Cmd::new(vec!["pacman".into(), "-Qi".into()], Some(60), true)) {
        installed_info = parse_pkg_info(&stdout);
        for info in &installed_info {
            for c in info.conflicts.iter().filter(|c| new_packages.contains(c)) {
                add(c, &info.name);
            }
        }
    }
    // Whose packages these are: the incoming one by the set's `Depends On` (who
    // in it pulls it in), the installed one by `Required By`.
    let find = |list: &[PkgInfo], name: &str| list.iter().position(|i| i.name == name);
    for c in &mut out {
        if let Some(i) = find(&new_info, &c.new_package) {
            c.new_version = new_info[i].version.clone();
        }
        c.installed_is_new = !installed.contains(c.installed.as_str());
        if c.installed_is_new {
            if let Some(i) = find(&new_info, &c.installed) {
                c.installed_version = new_info[i].version.clone();
            }
        } else if let Some(i) = find(&installed_info, &c.installed) {
            c.installed_version = installed_info[i].version.clone();
            c.required_by = installed_info[i].required_by.clone();
        }
        c.same_program = same_program(c, new_info.iter().find(|i| i.name == c.new_package));
        let new_parents = |n: &str| -> Vec<String> {
            let provides: Vec<&str> = new_info
                .iter()
                .find(|i| i.name == n)
                .map(|i| i.provides.iter().map(String::as_str).collect())
                .unwrap_or_default();
            new_info
                .iter()
                .filter(|i| i.depends.iter().any(|d| d == n || provides.contains(&d.as_str())))
                .map(|i| i.name.clone())
                .collect()
        };
        c.new_desktops = holding_desktops(&c.new_package, &new_info, &new_parents);
        c.installed_desktops = if c.installed_is_new {
            holding_desktops(&c.installed, &new_info, &new_parents)
        } else {
            holding_desktops(&c.installed, &installed_info, &|n: &str| {
                installed_info
                    .iter()
                    .find(|i| i.name == n)
                    .map(|i| i.required_by.clone())
                    .unwrap_or_default()
            })
        };
    }
    (out, new_info)
}

/// What of the newly installed (`new_packages`) requires a reboot: the kernel
/// itself and kernel modules: DKMS (`*-dkms`, a dependency on `dkms`) and
/// prebuilt ones for the stock kernel (a dependency on `linux`). A module is
/// built at install time but loaded at boot (Deepin: `deepin-anything-dkms`),
/// a new kernel all the more so. Everything else (the desktop itself, its
/// services, groups) is picked up by a new login. `new_info` is the
/// `pacman -Si` information about the incoming packages (may be empty: then
/// the names alone decide).
pub fn restart_packages(new_packages: &[String], new_info: &[PkgInfo]) -> Vec<String> {
    new_packages
        .iter()
        .filter(|name| {
            let info = new_info.iter().find(|i| &i.name == *name);
            name.as_str() == ARCH_KERNEL
                || name.ends_with("-dkms")
                || info.is_some_and(|i| i.depends.iter().any(|d| d == "dkms" || d == ARCH_KERNEL))
        })
        .cloned()
        .collect()
}

/// Whether it is one program in different versions: `mutter49` versus
/// `mutter` (the name without trailing digits), or the incoming package
/// provides the installed one's name.
fn same_program(c: &PkgConflict, new_info: Option<&PkgInfo>) -> bool {
    let bare = |s: &str| s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
    bare(&c.new_package) == c.installed
        || bare(&c.installed) == c.new_package
        || new_info.is_some_and(|i| i.provides.contains(&c.installed))
}

/// pacman groups by which a package is part of a desktop, on top of the
/// desktop names and their sets in the database (`system::get_packages`).
const DESKTOP_GROUPS: &[(&str, &str)] = &[("plasma", "kde")];

/// The desktop the package itself belongs to: a key package of the desktop
/// or a member of its group.
fn own_desktop(name: &str, info: Option<&PkgInfo>) -> Option<&'static str> {
    if let Some(d) = conflict_desktop(name) {
        return Some(d);
    }
    let groups = info.map(|i| i.groups.as_slice()).unwrap_or_default();
    for g in groups {
        let id = DESKTOP_GROUPS.iter().find(|(grp, _)| grp == g).map(|(_, id)| *id);
        for m in crate::system::DE_META {
            let hit = id == Some(m.id)
                || g == m.id
                || crate::system::get_packages(m.id, PkgFamily::Arch).iter().any(|p| p == g);
            if hit {
                return Some(m.display_name);
            }
        }
    }
    None
}

/// Desktops that hold a package: walking "who requires it" (`parents`) up to
/// the first package belonging to a desktop. Nobody requires it: the package
/// stands on its own, and the desktop is guessed by the package itself. It is
/// required, but not by desktops: empty: a table-based "mutter is GNOME"
/// would lie if mutter is held by Budgie.
fn holding_desktops(start: &str, infos: &[PkgInfo], parents: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let info = |n: &str| infos.iter().find(|i| i.name == n);
    let first = parents(start);
    if first.is_empty() {
        return own_desktop(start, info(start)).map(|d| vec![d.to_string()]).unwrap_or_default();
    }
    let mut found: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    seen.insert(start.to_string());
    let mut queue: std::collections::VecDeque<String> = first.into_iter().collect();
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.clone()) || seen.len() > 4000 {
            continue;
        }
        if let Some(d) = own_desktop(&n, info(&n)) {
            if !found.iter().any(|f| f == d) {
                found.push(d.to_string());
            }
            continue;
        }
        queue.extend(parents(&n));
    }
    found
}

/// Conflicts from the output of a REAL pacman transaction (LANG=C):
/// `:: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict…`.
/// The version (`-version-build`) is cut off.
pub fn parse_transaction_conflicts(text: &str) -> Vec<PkgConflict> {
    fn strip_version(s: &str) -> &str {
        let mut parts = s.rsplitn(3, '-');
        let (_rel, _ver, name) = (parts.next(), parts.next(), parts.next());
        name.unwrap_or(s)
    }
    let mut out: Vec<PkgConflict> = Vec::new();
    for line in text.lines() {
        let Some(head) = line.split(" are in conflict").next().filter(|_| line.contains(" are in conflict")) else {
            continue;
        };
        let head = head.trim().trim_start_matches("::").trim();
        let Some((a, b)) = head.split_once(" and ") else { continue };
        let c = PkgConflict {
            new_package: strip_version(a.trim()).to_string(),
            installed: strip_version(b.trim()).to_string(),
            ..PkgConflict::default()
        };
        if valid_pkg_name(&c.new_package) && valid_pkg_name(&c.installed) && !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Key desktop packages on Arch: by them a conflict is named by desktop, not
/// by package name. Only what the desktop cannot exist without (window
/// manager, shell, session): a desktop is not guessed by a library.
const CONFLICT_DESKTOP_PACKAGES: &[(&str, &str)] = &[
    ("kwin", "kde"),
    ("kwin-x11", "kde"),
    ("plasma-workspace", "kde"),
    ("plasma-desktop", "kde"),
    ("gnome-shell", "gnome"),
    ("mutter", "gnome"),
    ("cinnamon", "cinnamon-mint"),
    ("muffin", "cinnamon-mint"),
    ("xfwm4", "xfce"),
    ("xfce4-session", "xfce"),
    ("marco", "mate"),
    ("mate-session-manager", "mate"),
    ("budgie-desktop", "budgie"),
    ("magpie-wm", "budgie"),
    ("gala", "pantheon"),
    ("pantheon-session", "pantheon"),
    ("deepin-kwin", "deepin"),
    ("deepin-session", "deepin"),
    ("deepin-shell", "deepin"),
    ("cosmic-comp", "cosmic"),
    ("cosmic-session", "cosmic"),
    ("lxqt-session", "lxqt"),
    ("hyprland", "hyprland"),
    ("sway", "sway"),
    ("niri", "niri"),
    // river-classic and river 0.4+ exclude each other (`conflicts=('river')`):
    // both sides of the conflict are the River desktop.
    ("river-classic", "river"),
    ("river", "river"),
    ("wayfire", "wayfire"),
];

/// How to name a package to the person: a role (translated) or a proper name.
const CONFLICT_PACKAGE_ROLES: &[(&str, &str)] = &[("deepin-kwin", n_("the Deepin window manager"))];
const CONFLICT_PACKAGE_NAMES: &[(&str, &str)] = &[
    ("kwin", "KWin"),
    ("kwin-x11", "KWin"),
    ("mutter", "Mutter"),
    ("muffin", "Muffin"),
    ("marco", "Marco"),
    ("xfwm4", "Xfwm"),
    ("gala", "Gala"),
];

/// A conflict Rexon resolves by REPLACEMENT, only with the person's consent.
///
/// An ordinary conflict is someone else's desktop or program, and Rexon does
/// not take it upon itself to remove them ([`conflict_explanation`]). This is
/// different: the installed package is another branch of the same program
/// the desktop installs, and they cannot coexist. Then the card explains
/// what is installed and why it has to be replaced, and offers a replace
/// button ([`conflict_swaps`]); on «Yes» pacman removes the installed one and
/// installs the set in ONE transaction ([`install_cmd_replacing`]).
struct ConflictSwapRule {
    new_package: &'static str,
    installed: &'static str,
    /// How to name the installed package to the person (the version is appended to the name).
    installed_name: &'static str,
    /// Explanation on the card. %1 is the installed one with its version ("River 0.5").
    notice: &'static str,
    /// Addition to the «Replace?» question (empty: none). %1 as in `notice`.
    confirm_note: &'static str,
}

const CONFLICT_SWAPS: &[ConflictSwapRule] = &[
    // River 0.4+ from extra is the "non-monolithic" branch without its own
    // tiling (`system.rs`, the `river` entry), Rexon installs river-classic. The
    // packages declare `Conflicts With` on each other; river has no `Provides`,
    // and removing it breaks nothing as long as nobody requires it.
    ConflictSwapRule {
        new_package: "river-classic",
        installed: "river",
        installed_name: "River",
        notice: n_(
            "⛔ You have %1 installed — the new River branch, which has no built-in tiling. \
             Rexon installs the classic River branch (river-classic) with its own tiling, \
             and for that %1 has to be replaced.",
        ),
        confirm_note: n_(
            "Your ~/.config/river/init stays as it is. If it was written for %1, the classic \
             River will run it too; if River then starts without keys, rename that file, and \
             “Check / Repair” will put the Rexon starter one in its place.",
        ),
    },
];

fn swap_rule(c: &PkgConflict) -> Option<&'static ConflictSwapRule> {
    CONFLICT_SWAPS
        .iter()
        .find(|r| r.new_package == c.new_package && r.installed == c.installed)
}

/// A short version for the person: `1:0.5.0-1.1` → `0.5`.
fn short_version(v: &str) -> String {
    let v = v.split_once(':').map(|(_, rest)| rest).unwrap_or(v);
    let v = v.split('-').next().unwrap_or(v);
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// The installed package in a replacement, as a name with a version ("River
/// 0.5"); without a version, as the package name.
fn swap_label(c: &PkgConflict) -> String {
    match swap_rule(c) {
        Some(r) if !c.installed_version.is_empty() => {
            format!("{} {}", r.installed_name, short_version(&c.installed_version))
        }
        _ => c.installed.clone(),
    }
}

/// Probe conflicts that are resolved by a replacement with the person's
/// consent. Empty: a replacement will not help: there is a conflict not from
/// the replacements table, someone requires the installed package (pacman
/// would remove it too), or the set would not install even without the
/// conflict ([`InstallPreview::blocked`]).
pub fn conflict_swaps(p: &InstallPreview) -> Vec<PkgConflict> {
    let all = !p.conflicts.is_empty()
        && p.blocked.is_empty()
        && p.conflicts.iter().all(|c| swap_rule(c).is_some() && c.required_by.is_empty() && !c.installed_is_new);
    if all {
        p.conflicts.clone()
    } else {
        Vec::new()
    }
}

/// The replacement consent covers all probe conflicts: then the install goes
/// with replacement ([`install_cmd_replacing`]). `replace` is the installed
/// packages the person agreed to replace.
pub fn conflict_swap_approved(p: &InstallPreview, replace: &[String]) -> bool {
    let swaps = conflict_swaps(p);
    !swaps.is_empty() && swaps.iter().all(|c| replace.contains(&c.installed))
}

fn swap_labels(swaps: &[PkgConflict]) -> String {
    let mut labels: Vec<String> = Vec::new();
    for c in swaps {
        let l = swap_label(c);
        if !labels.contains(&l) {
            labels.push(l);
        }
    }
    labels.join(", ")
}

/// Caption of the replace button on the desktop card.
pub fn conflict_swap_button(swaps: &[PkgConflict]) -> String {
    // TRANSLATORS: %1 is the installed program being replaced (e.g. “River 0.5”).
    trf("🔁 Replace %1", &[&swap_labels(swaps)])
}

/// Explanation under the replace button.
pub fn conflict_swap_hint(swaps: &[PkgConflict]) -> String {
    let new: Vec<&str> = swaps.iter().map(|c| c.new_package.as_str()).collect();
    trf(
        // TRANSLATORS: %1 is the installed program (e.g. “River 0.5”), %2 the
        // package installed in its place.
        "Rexon will ask first. %1 is removed and %2 installed in the same pacman \
         transaction: if the installation doesn't go through, %1 stays.",
        &[&swap_labels(swaps), &new.join(", ")],
    )
}

/// Title of the replacement question and operation window.
pub fn conflict_swap_title(swaps: &[PkgConflict]) -> String {
    // TRANSLATORS: %1 is the installed program being replaced (e.g. “River 0.5”).
    trf("Replace %1", &[&swap_labels(swaps)])
}

/// The «Replace?» question: what will be removed, what will be installed and
/// what happens to the desktop `desktop`.
pub fn conflict_swap_confirm_text(swaps: &[PkgConflict], desktop: &str) -> String {
    let new: Vec<&str> = swaps.iter().map(|c| c.new_package.as_str()).collect();
    let installed: Vec<&str> = swaps.iter().map(|c| c.installed.as_str()).collect();
    let mut text = trf(
        // TRANSLATORS: %1 is the installed program (e.g. “River 0.5”), %2 its
        // package, %3 the package installed in its place, %4 the desktop.
        "%1 (package %2) will be removed, and %3 will be installed in its place; then \
         %4 is set up as usual. It all happens in one pacman transaction: if it doesn't \
         go through, %1 stays installed.\n\nReplace it?",
        &[&swap_labels(swaps), &installed.join(", "), &new.join(", "), &desktop],
    );
    let mut notes: Vec<String> = Vec::new();
    for c in swaps {
        if let Some(r) = swap_rule(c).filter(|r| !r.confirm_note.is_empty()) {
            let note = trf(r.confirm_note, &[&swap_label(c)]);
            if !notes.contains(&note) {
                notes.push(note);
            }
        }
    }
    for n in notes {
        text.push_str("\n\n");
        text.push_str(&n);
    }
    text
}

/// The install command replacing a conflicting package: the same `pacman -S`
/// as [`install_cmd`], plus `--ask=4`: a "yes" answer to pacman's question
/// "X and Y are in conflict. Remove Y?" (ALPM_QUESTION_CONFLICT_PKG = 1 << 2)
/// under `--noconfirm`. Removal and install go in one transaction: if the
/// set does not install, the installed one stays. Arch only; with an empty
/// `replace` exactly [`install_cmd`]. Which conflicts are resolved is decided
/// before the password ([`conflict_swap_approved`]).
pub fn install_cmd_replacing(family: PkgFamily, packages: &[String], replace: &[String]) -> Result<Vec<String>, String> {
    let mut cmd = install_cmd(family, packages)?;
    if replace.is_empty() {
        return Ok(cmd);
    }
    if family != PkgFamily::Arch {
        return Err(tr("Rexon can replace a conflicting package only with pacman."));
    }
    check_pkg_names(family, replace)?;
    let at = cmd.iter().position(|a| a == "--noconfirm").map(|i| i + 1).unwrap_or(cmd.len());
    cmd.insert(at, "--ask=4".to_string());
    Ok(cmd)
}

fn conflict_desktop(package: &str) -> Option<&'static str> {
    let id = CONFLICT_DESKTOP_PACKAGES.iter().find(|(p, _)| *p == package)?.1;
    crate::system::DE_META.iter().find(|m| m.id == id).map(|m| m.display_name)
}

/// A conflict explanation for the person: one for the refusal before the
/// password, the desktop card and parsing a failed transaction.
pub fn conflict_explanation(conflicts: &[PkgConflict]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for c in conflicts.iter().take(3) {
        // Another branch of the same program has its own explanation (replacement
        // with consent, [`conflict_swaps`]).
        if let Some(rule) = swap_rule(c) {
            let line = trf(rule.notice, &[&swap_label(c)]);
            if !lines.contains(&line) {
                lines.push(line);
            }
            continue;
        }
        let join = |v: &[String]| (!v.is_empty()).then(|| v.join(", "));
        let target = join(&c.new_desktops).or_else(|| conflict_desktop(&c.new_package).map(str::to_string));
        // Someone requires the installed package, but not a desktop: we do not
        // guess the desktop by the table (mutter is held by Budgie, not GNOME).
        let other = join(&c.installed_desktops).or_else(|| {
            c.required_by.is_empty().then(|| conflict_desktop(&c.installed).map(str::to_string)).flatten()
        });
        let line = match (target, other) {
            _ if c.installed_is_new => trf(
                // TRANSLATORS: %1 and %2 are two packages of the set being installed.
                "⛔ Installation won't work: the set needs both %1 and %2, and the two \
                 can't be installed at once.",
                &[&c.new_package, &c.installed],
            ),
            (Some(t), Some(o)) if t != o && c.same_program && !c.new_version.is_empty() && !c.installed_version.is_empty() => {
                let bare = |s: &str| s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
                let program = if bare(&c.new_package) == c.installed { c.installed.clone() } else { bare(&c.installed) };
                trf(
                    // TRANSLATORS: %1 is the desktop being installed, %2 the installed
                    // desktop, %3 a program both need (e.g. mutter), %4 the version %1
                    // needs, %5 the package of that version, %6 the version %2 needs.
                    "⛔ %1 can't be installed next to %2: %1 needs %3 %4 (%5), and %2 needs \
                     %3 %6; the two can't be installed at once. To install %1, you'd have to \
                     remove %2 — Rexon doesn't remove another desktop by itself.",
                    &[
                        &t,
                        &o,
                        &program,
                        &major_version(&c.new_version),
                        &c.new_package,
                        &major_version(&c.installed_version),
                    ],
                )
            }
            (Some(t), Some(o)) if t != o => {
                let new_label = match CONFLICT_PACKAGE_ROLES.iter().find(|(p, _)| *p == c.new_package) {
                    Some((_, role)) => format!("{} ({})", tr(role), c.new_package),
                    None => c.new_package.clone(),
                };
                let installed_label = CONFLICT_PACKAGE_NAMES
                    .iter()
                    .find(|(p, _)| *p == c.installed)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| c.installed.clone());
                trf(
                    // TRANSLATORS: %1 is the desktop being installed, %2 the installed
                    // desktop, %3 the package of %1 (role and name), %4 the package of %2.
                    "⛔ %1 can't be installed next to %2: %3 conflicts with %4. To install %1, \
                     you'd have to remove %2 — Rexon doesn't remove another desktop by itself.",
                    &[&t, &o, &new_label, &installed_label],
                )
            }
            _ if !c.required_by.is_empty() => trf(
                // TRANSLATORS: %1 is the package being installed, %2 an installed
                // package, %3 the installed packages that need %2 (comma-separated).
                "⛔ Installation won't work: %1 conflicts with the installed package %2 \
                 (needed by %3). pacman would have to remove %2 along with them, and Rexon \
                 doesn't do that by itself.",
                &[&c.new_package, &c.installed, &c.required_by.join(", ")],
            ),
            _ => trf(
                // TRANSLATORS: %1 is the package being installed, %2 an installed package.
                "⛔ Installation won't work: %1 conflicts with the installed package %2. \
                 pacman would have to remove %2, and Rexon doesn't do that by itself.",
                &[&c.new_package, &c.installed],
            ),
        };
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// What from the set is not installed because of a conflict and why
/// ([`InstallPreview::conflict_skipped`]): one line per dropped package.
/// The desktop is still installed: the text warns, it does not refuse.
pub fn conflict_skip_explanation(skips: &[ConflictSkip]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for s in skips.iter().take(3) {
        let c = &s.conflict;
        let login_screen = s.package.contains("greeter");
        let label = if login_screen && !s.desktop.is_empty() {
            // TRANSLATORS: %1 is a desktop (e.g. Pantheon). The phrase is %1 of
            // “Not installing %1: …” and its sister lines — write it in the case
            // and capitalization that fits the start of those lines in your
            // language (Russian: “Экран входа %1”).
            trf("the %1 login screen", &[&s.desktop])
        } else {
            s.package.clone()
        };
        let others = (!c.installed_desktops.is_empty()).then(|| c.installed_desktops.join(", "));
        let reason = match others {
            Some(o) if c.same_program && !c.new_version.is_empty() && !c.installed_version.is_empty() => trf(
                // TRANSLATORS: %1 is what is not installed (e.g. “the Pantheon
                // login screen”), %2 a program (e.g. Mutter), %3 the version %1
                // needs, %4 the installed desktops, %5 the version they need.
                "⚠️ Not installing %1: it needs %2 %3, and %4 needs %2 %5 — the two \
                 can't be installed at once.",
                &[
                    &label,
                    &conflict_program_name(c),
                    &major_version(&c.new_version),
                    &o,
                    &major_version(&c.installed_version),
                ],
            ),
            _ if c.installed_is_new => trf(
                // TRANSLATORS: %1 is what is not installed, %2 a package the rest
                // of the set needs.
                "⚠️ Not installing %1: it conflicts with %2, which the rest of the set needs.",
                &[&label, &c.installed],
            ),
            _ => trf(
                // TRANSLATORS: %1 is what is not installed, %2 an installed package.
                "⚠️ Not installing %1: it conflicts with the installed package %2.",
                &[&label, &c.installed],
            ),
        };
        let tail = if login_screen && !s.desktop.is_empty() {
            // TRANSLATORS: %1 is the desktop being installed.
            trf("You can sign in to %1 from the regular login screen; everything else will be installed.", &[&s.desktop])
        } else {
            tr("Everything else will be installed without it.")
        };
        let line = format!("{reason} {tail}");
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// The program whose two versions the conflict involves (`mutter49` versus
/// `mutter` → `Mutter`), as a name for the person.
fn conflict_program_name(c: &PkgConflict) -> String {
    let bare = |s: &str| s.trim_end_matches(|ch: char| ch.is_ascii_digit()).to_string();
    let program = if bare(&c.new_package) == c.installed { c.installed.clone() } else { bare(&c.installed) };
    CONFLICT_PACKAGE_NAMES
        .iter()
        .find(|(p, _)| *p == program)
        .map(|(_, n)| n.to_string())
        .unwrap_or(program)
}

/// The major part of a version for the person: `1:49.8-1.1` → `49`.
fn major_version(v: &str) -> String {
    let v = v.split_once(':').map(|(_, rest)| rest).unwrap_or(v);
    v.split(['.', '-', '+']).next().unwrap_or(v).to_string()
}

/// Names of installed packages via the same `run` as the trial transaction.
/// `None`: could not ask: then nobody is declared installed.
fn installed_names(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<Vec<String>> {
    let query = installed_packages_query(family)?;
    let (rc, stdout, _) = run(&query.cmd)?;
    Some(parse_installed_packages(rc == 0, &stdout)).filter(|names| !names.is_empty())
}

/// Which installed packages of the plan are NOT updated but would be rolled
/// back: `pacman -Qu` lists only those whose repository build is newer (the
/// version comparison is pacman's own), the other installed ones from the
/// plan are an own build that is newer or different. No answer: nothing is
/// dropped.
pub fn locally_newer(installed_planned: &[String], run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Vec<String> {
    if installed_planned.is_empty() {
        return Vec::new();
    }
    let cmd = Cmd::new(vec!["pacman".into(), "-Qu".into(), "-q".into()], None, true);
    let Some((rc, stdout, stderr)) = run(&cmd) else {
        return Vec::new();
    };
    // Code 1 without output and without an error means "no updates at all";
    // anything else (database locked, read error) is an answer not understood,
    // nothing is dropped.
    if rc != 0 && !(rc == 1 && stdout.trim().is_empty() && stderr.trim().is_empty()) {
        return Vec::new();
    }
    let upgradable: std::collections::HashSet<&str> = stdout
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    installed_planned
        .iter()
        .filter(|p| !upgradable.contains(p.as_str()))
        .cloned()
        .collect()
}

/// Split the transaction plan into (installed: updates, new). Without the
/// installed list everything stays new.
pub fn split_installed(planned: Vec<String>, installed: Option<Vec<String>>) -> (Vec<String>, Vec<String>) {
    let Some(installed) = installed else {
        return (Vec::new(), planned);
    };
    let installed: std::collections::HashSet<String> = installed.into_iter().collect();
    planned.into_iter().partition(|name| installed.contains(name))
}

/// Live preview: commands go through `utils::run` (LANG=C). Quietly, without
/// panics: a run error gives `None`.
pub fn install_preview_live(family: PkgFamily, packages: &[String]) -> Option<InstallPreview> {
    crate::utils::which("pacman", None)?;
    install_preview(family, packages, &|cmd: &Cmd| {
        let report = crate::utils::run(
            &crate::utils::RunSpec {
                argv: cmd.argv.clone(),
                output: crate::utils::RunOutput::Capture,
                timeout_secs: cmd.timeout_secs,
                lang_c: cmd.lang_c,
                ..Default::default()
            },
            None,
        );
        let rc = report.returncode?;
        Some((
            rc,
            crate::utils::decode_text(&report.stdout),
            crate::utils::decode_text(&report.stderr),
        ))
    })
}

/// Split the packages marked as AUR in the desktop database by FACT: what is
/// in this machine's enabled repositories, and what is only in the AUR.
///
/// The "AUR" mark in the table is a property of plain Arch, not of the
/// machine: on CachyOS the Mint themes and icons are in the `cachyos`
/// repository. Without this check, on a machine without yay/paru such
/// packages would silently drop out together with the AUR list, although
/// pacman would install them itself. The question is the same `pacman -Si`
/// as in [`repo_version_query`]; an "I do not know" answer (`None`, the
/// command did not answer) leaves the package in the AUR, as before.
///
/// Returns (from repositories, AUR only), the order is kept.
pub fn split_repo_available(
    family: PkgFamily,
    names: &[String],
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> (Vec<String>, Vec<String>) {
    let mut repo = Vec::new();
    let mut aur = Vec::new();
    for name in names {
        let in_repo = repo_version_query(family, name).and_then(|q| {
            let (rc, stdout, _) = run(&q.cmd)?;
            repo_target_exists(family, Some(rc), &stdout)
        });
        if in_repo == Some(true) {
            repo.push(name.clone());
        } else {
            aur.push(name.clone());
        }
    }
    (repo, aur)
}

/// The command for packages NOT FROM THE REPOSITORIES: the engine's seventh
/// independent question.
///
/// The subject of the question is the ORIGIN of what is already installed:
/// whether the package was built locally (AUR, own builds) or came from the
/// distribution repository. It does not reduce to any earlier question:
/// [`installed_packages_query`] lists everything installed, saying nothing
/// about the origin, and [`repo_version_query`] asks about a NAMED package
/// and goes to the repository database. It is asked by one reader: the
/// "System / AUR" theme classification in the «Appearance» tab
/// (`rexon/appearance/installed_scan.py`), the answer is parsed by
/// [`crate::installed_scan::parse_qm_output`], the retry policy on a busy
/// database is [`crate::installed_scan::foreign_pkgs_from_attempts`].
///
/// Arch only, and this is not laziness: the other families have NO notion of
/// a "package not from the repositories" in the same sense.
/// * Debian has none at all. `dpkg` only knows that a package is installed
///   and does not remember where it came from; and
///   `dpkg --print-foreign-architectures` is a false friend: "foreign" there
///   is about the ARCHITECTURE (i386 next to amd64), not the origin. The
///   closest answer comes from `aptitude search '~i!~ODebian'` and
///   `apt-forktracer`, but neither is in a base install.
/// * dnf has an answer, `dnf repoquery --extras`, but it prints full NEVRA
///   (`name-version-release.arch`), not "name version", and a parser written
///   for `pacman -Qm` would silently return names with the version inside.
/// * For zypper the closest is `zypper packages --orphaned`, and that is a
///   different question ("a package no longer in any repository"), and the
///   format is a table with borders.
/// Inventing a command and feeding someone else's output to someone else's
/// parser would quietly count AUR themes as "System", exactly what the AUR
/// exclusion was introduced against. The honest answer of these families is
/// `None`, and on it the asker does not show "System" at all.
pub fn foreign_packages_query(family: PkgFamily) -> Option<InstalledPkgQuery> {
    if family != PkgFamily::Arch {
        return None;
    }
    Some(InstalledPkgQuery {
        binary: "pacman".to_string(),
        // Timeout by command class (query, 30 s). LANG=C: a Russian locale
        // translates the busy database message, and the retry policy looks for
        // "lck"/"lock" in it: without LANG=C the retry would silently not happen.
        cmd: Cmd::new(vec!["pacman".into(), "-Qm".into()], None, true),
    })
}

/// How many paths go into ONE file owner question.
///
/// The number guards against ARG_MAX, not against the manager: hundreds of
/// short paths do not hit it, but on a machine with thousands of themes they
/// can. Cutting into batches is the job of whoever assembles argv, so the
/// number lives here, next to the command (it used to be the `_QO_BATCH`
/// constant of the executor, in `rexon/appearance/installed_scan.py`).
pub const FILE_OWNER_BATCH: usize = 200;

/// Timeout of ONE file owner question, seconds.
///
/// A number of its own here, not a class: the "query" class (30 s) is
/// calibrated for a question about ONE package, while this command answers
/// about two hundred paths at once. The number is taken from the original
/// (`subprocess.run(..., timeout=60)`); measured work on this machine is
/// about 0.12 s per batch.
pub const FILE_OWNER_TIMEOUT_SECS: u64 = 60;

/// Whether a path is fit for the file owner question.
///
/// Absolute: the manager would resolve a relative one from the current
/// directory, which we do not set. The same condition cuts off a leading
/// `-`, otherwise the path would go to the manager as an option. A newline
/// would split the answer line in two, and the parser would silently assign
/// the file to someone else's package; a NUL byte would not survive argv at all.
fn valid_owner_path(path: &str) -> bool {
    path.starts_with('/') && !path.contains('\n') && !path.contains('\0')
}

/// Commands for the file OWNER: the engine's eighth independent question.
///
/// The subject of the question is a PATH, not a package name, and the answer
/// goes the other way: [`package_files_query`] asks "which files does this
/// package have", here it is "whose file is this". It is asked by the same
/// single reader as [`foreign_packages_query`]; the answer is parsed by
/// [`crate::installed_scan::parse_qo_output`].
///
/// The answer is a LIST of commands: there may be thousands of paths, and
/// whoever assembles argv cuts them into batches of [`FILE_OWNER_BATCH`]. An
/// empty list means "no valid paths left" and there is nothing to run;
/// `None` is a family that does not understand this question, and these are
/// different things.
///
/// Arch only, and this is not laziness: the other families have a different
/// answer format, and there is one parser for the whole project. `dpkg -S`
/// prints "package: path", the reverse order, and answers several lines for
/// a symlink path; `rpm -qf` prints only the package NEVRA, with no path at
/// all, so the answer can only be matched to the question by line order.
/// Neither was checked by hand, and silently feeding them to a parser
/// written for "path is owned by package version" would give an empty owner
/// map, i.e. an empty "System" list instead of an answer. The honest answer
/// is `None`.
pub fn file_owner_queries(
    family: PkgFamily,
    paths: &[String],
) -> Option<Vec<InstalledPkgQuery>> {
    if family != PkgFamily::Arch {
        return None;
    }
    let good: Vec<&String> = paths.iter().filter(|p| valid_owner_path(p)).collect();
    Some(
        good.chunks(FILE_OWNER_BATCH)
            .map(|chunk| {
                let mut argv: Vec<String> = vec!["pacman".into(), "-Qo".into()];
                argv.extend(chunk.iter().map(|p| (*p).clone()));
                InstalledPkgQuery {
                    binary: "pacman".to_string(),
                    // LANG=C is mandatory: in a Russian locale the answer is translated (no
                    // "is owned by" in it), and the parser finds nothing.
                    cmd: Cmd::new(argv, Some(FILE_OWNER_TIMEOUT_SECS), true),
                }
            })
            .collect(),
    )
}

// ============================================================================
// Waiting for the package manager lock: a pure loop step
// ============================================================================

/// How often to recheck the lock (seconds). Not less: snap-pac hooks hold the
/// lock for about half a minute after the transaction ends, and polling more
/// often only heats the CPU.
pub const PKG_LOCK_POLL_SECS: u64 = 2;
/// How long to wait for the lock in total (seconds).
pub const PKG_LOCK_TIMEOUT_SECS: u64 = 300;
/// Stage under which the wait is shown in the progress window.
pub const PKG_LOCK_WAIT_STAGE: &str = "wait_lock";
/// What to tell the person while we wait for someone else's transaction.
pub fn pkg_lock_wait_message() -> String {
    tr("⏳ Waiting for the package manager to become free (another transaction \
        is still running its hooks)…")
}
/// The "lock is free" answer: families without a lock at all answer it too.
pub const PKG_LOCK_OK: &str = "ok";

/// What to do on the next round of waiting for the lock.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LockWaitStep {
    /// The lock is free: go on.
    Ready { message: String },
    /// Gave up waiting: return a refusal with this text.
    TimedOut { message: String },
    /// Tell the person we are waiting, and wait. Said ONCE.
    Notify {
        stage: String,
        message: String,
        poll_secs: u64,
    },
    /// Just wait.
    Wait { poll_secs: u64 },
}

/// A pure lock wait step, shared by package installs and the look rebuild
/// (the "one class, one engine" rule: there were two copies of the loop).
///
/// `locked`: whether at least one file from [`lock_paths`] exists (for a
/// family without a lock the caller passes false and gets `Ready` right
/// away); `elapsed_secs`: how long we have been waiting; `notified`: whether
/// the person was already told.
pub fn lock_wait_step(
    locked: bool,
    elapsed_secs: f64,
    timeout_secs: u64,
    notified: bool,
) -> LockWaitStep {
    if !locked {
        return LockWaitStep::Ready {
            message: PKG_LOCK_OK.to_string(),
        };
    }
    if elapsed_secs >= timeout_secs as f64 {
        return LockWaitStep::TimedOut {
            message: pkg_lock_timeout_message(timeout_secs),
        };
    }
    if !notified {
        return LockWaitStep::Notify {
            stage: PKG_LOCK_WAIT_STAGE.to_string(),
            message: pkg_lock_wait_message(),
            poll_secs: PKG_LOCK_POLL_SECS,
        };
    }
    LockWaitStep::Wait {
        poll_secs: PKG_LOCK_POLL_SECS,
    }
}

/// Side effects of waiting for the lock: look at the file system, sleep, tell
/// the person.
///
/// The core does not touch the file system and does not sleep by itself:
/// the same trick as `DaemonFs`: transport outside, order inside. Thanks to
/// this the wait loop is tested without a single real second of sleep.
pub trait LockWaitIo {
    /// Whether at least one of the lock files exists.
    fn any_locked(&mut self, paths: &[String]) -> bool;
    /// How many seconds have passed since the wait began.
    fn elapsed_secs(&mut self) -> f64;
    /// Sleep before the next round.
    fn sleep_secs(&mut self, secs: u64);
    /// Tell the person we are waiting for someone else's transaction. Called ONCE.
    fn notify(&mut self, stage: &str, message: &str);
}

/// Wait for the package manager lock to be released. → `(free, message)`.
///
/// WHY THIS IS HERE AND NOT IN PYTHON. The decision (`lock_wait_step`) has
/// lived in the core for a long time, but a shim ran the loop: `os.path.exists`
/// in a list comprehension and `time.sleep`. As long as that was so, no Rust
/// consumer (including the main window host) could wait for the lock itself
/// and ran into the boundary.
///
/// ⚠️ The wait is real: snap-pac holds `/var/lib/pacman/db.lck` for about
/// thirty seconds AFTER the transaction finished (a post-hook takes a btrfs
/// snapshot). An install started right away would fail with "database busy".
pub fn wait_for_pkg_lock(
    io: &mut dyn LockWaitIo,
    paths: &[String],
    timeout_secs: u64,
) -> (bool, String) {
    let mut notified = false;
    loop {
        // A family without a lock: `paths` is empty, the answer is "free" on the first round.
        let locked = !paths.is_empty() && io.any_locked(paths);
        match lock_wait_step(locked, io.elapsed_secs(), timeout_secs, notified) {
            LockWaitStep::Ready { message } => return (true, message),
            LockWaitStep::TimedOut { message } => return (false, message),
            LockWaitStep::Notify {
                stage,
                message,
                poll_secs,
            } => {
                notified = true;
                io.notify(&stage, &message);
                io.sleep_secs(poll_secs);
            }
            LockWaitStep::Wait { poll_secs } => io.sleep_secs(poll_secs),
        }
    }
}

/// Live side effects of the wait: the real file system, clock and sleep.
///
/// `notify` is where to tell the person; for a consumer without a progress
/// window it is an empty closure.
pub struct LiveLockWait<N: FnMut(&str, &str)> {
    start: std::time::Instant,
    notify: N,
}

impl<N: FnMut(&str, &str)> LiveLockWait<N> {
    /// The time count starts here.
    pub fn new(notify: N) -> Self {
        Self {
            start: std::time::Instant::now(),
            notify,
        }
    }
}

impl<N: FnMut(&str, &str)> LockWaitIo for LiveLockWait<N> {
    fn any_locked(&mut self, paths: &[String]) -> bool {
        paths.iter().any(|p| std::path::Path::new(p).exists())
    }

    fn elapsed_secs(&mut self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn sleep_secs(&mut self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn notify(&mut self, stage: &str, message: &str) {
        (self.notify)(stage, message);
    }
}

// ============================================================================
// Live install progress: reading a growing log
// ============================================================================
//
// WHY. The main install step runs under `pkexec` with captured output: until
// it ends, the person sees not a single line and cannot tell work from a
// hang. So the output goes to a file, and a separate thread reads it on the
// fly and brings stage lines up.
//
// ⚠️ THERE WERE TWO READERS, AND IT WAS THE SAME WORK WRITTEN TWICE: the
// install from repositories (`_tail_pkg_log`) and the AUR build
// (`_run_aur_with_progress`) each kept their own thread, their own buffer,
// their own tail parsing and their own poll interval. They differed in
// exactly one thing: what to do with the next LINE. Here there is one
// engine, and the difference is moved into [`LogTailKind`] (the "one class,
// one engine" rule).

/// Markers of AUR build lines worth showing the person: `makepkg` stage
/// headers (`==>`), helper messages (`::`) and transitions (` -> `).
/// Everything else is compiler noise of tens of thousands of lines.
pub const AUR_PROGRESS_MARKERS: &[&str] = &["==>", "::", " -> "];

/// The exit code service mark in the install log: it is for the script, not
/// for the person.
pub const LOG_EXIT_MARKER: &str = "__REXON_EXIT__";

/// Whose log we read: this decides what to do with a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTailKind {
    /// Install from repositories: a line is parsed into a stage and a message,
    /// an unparsed one is shown as is.
    Pacman,
    /// AUR build: only stage lines are shown, the rest is noise.
    Aur,
    /// AeroShell look rebuild: each non-empty builder line goes to the progress
    /// window as is (`aeroshell::rebuild_line`), as in Python.
    Build,
    /// Each non-empty line as is, for whoever parses the output themselves (AUR
    /// install stages, `aur_install::stage_of_line`: pacman's "loading
    /// packages...", "installing x..." lines carry no `Aur` marks).
    Raw,
}

impl LogTailKind {
    /// Name for the bridge to Python.
    pub fn as_str(self) -> &'static str {
        match self {
            LogTailKind::Pacman => "pacman",
            LogTailKind::Aur => "aur",
            LogTailKind::Build => "build",
            LogTailKind::Raw => "raw",
        }
    }

    /// How often to look into the file, milliseconds.
    pub fn poll_ms(self) -> u64 {
        match self {
            LogTailKind::Pacman => 100,
            LogTailKind::Aur | LogTailKind::Build | LogTailKind::Raw => 300,
        }
    }
}

/// Log line → `(stage, message)` or `None`: nothing to show.
pub fn log_tail_line(kind: LogTailKind, raw: &str) -> Option<(String, String)> {
    match kind {
        LogTailKind::Pacman => {
            let line = raw.trim_end();
            if line.is_empty() || line.starts_with(LOG_EXIT_MARKER) {
                return None;
            }
            Some(match parse_pacman_line(line) {
                Some(p) => (p.stage, p.message),
                None => ("raw".to_string(), line.to_string()),
            })
        }
        LogTailKind::Aur => crate::aur_install::progress_line(raw).map(|line| ("aur".to_string(), line)),
        LogTailKind::Build => crate::aeroshell::rebuild_line(raw)
            .map(|line| (crate::aeroshell::REBUILD_LINE_STAGE.to_string(), line)),
        LogTailKind::Raw => {
            let line = raw.trim_end();
            (!line.trim().is_empty()).then(|| ("raw".to_string(), line.to_string()))
        }
    }
}

/// Stage of the "already installed" line from `--needed` ([`parse_pacman_line`]).
pub const LOG_STAGE_SKIP: &str = "skip";

/// Live reader of a growing log. Stopped by [`LogTail::stop`].
pub struct LogTail {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<Vec<String>>>,
}

impl LogTail {
    /// Stop reading, read the rest and return the shown lines.
    pub fn stop(&mut self) -> Vec<String> {
        self.done.store(true, std::sync::atomic::Ordering::SeqCst);
        match self.handle.take() {
            Some(h) => h.join().unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

impl Drop for LogTail {
    /// A forgotten reader must not outlive the install: the thread is a daemon
    /// in spirit, but we do not need a dangling file descriptor.
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop();
        }
    }
}

/// Start reading a growing log. `emit` is called for each shown line
/// `(stage, message)`.
///
/// ⚠️ The file is read as BYTES and split on `\n`: reading text from an
/// arbitrary position is undefined behaviour, and the next chunk may end in
/// the middle of a multibyte character. A tail without a newline stays in the
/// buffer until the next round and is read at the very end.
pub fn tail_log(
    path: &str,
    kind: LogTailKind,
    mut emit: impl FnMut(&str, &str) + Send + 'static,
) -> LogTail {
    use std::io::{Read, Seek, SeekFrom};

    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = done.clone();
    let path = path.to_string();
    let handle = std::thread::spawn(move || {
        let mut shown: Vec<String> = Vec::new();
        let mut pos: u64 = 0;
        let mut buf: Vec<u8> = Vec::new();

        let read_new = |pos: &mut u64| -> Vec<u8> {
            let Ok(mut f) = std::fs::File::open(&path) else {
                return Vec::new();
            };
            if f.seek(SeekFrom::Start(*pos)).is_err() {
                return Vec::new();
            }
            let mut chunk = Vec::new();
            if f.read_to_end(&mut chunk).is_err() {
                return Vec::new();
            }
            *pos += chunk.len() as u64;
            chunk
        };

        let flush = |raw: &[u8], shown: &mut Vec<String>, emit: &mut dyn FnMut(&str, &str)| {
            for part in raw.split(|b| *b == b'\n') {
                let line = String::from_utf8_lossy(part);
                if let Some((stage, message)) = log_tail_line(kind, &line) {
                    // "✅ already installed: openbox" twice: the package came both as a group
                    // member and as a name in the desktop set: a repeat adds nothing to the log
                    // (any desktop, any install path).
                    if stage == LOG_STAGE_SKIP && shown.contains(&message) {
                        continue;
                    }
                    shown.push(message.clone());
                    emit(&stage, &message);
                }
            }
        };

        loop {
            let stopping = flag.load(std::sync::atomic::Ordering::SeqCst);
            buf.extend_from_slice(&read_new(&mut pos));
            if let Some(idx) = buf.iter().rposition(|b| *b == b'\n') {
                let rest = buf.split_off(idx + 1);
                flush(&buf, &mut shown, &mut emit);
                buf = rest;
            }
            if stopping {
                // The last round: a tail without a newline is shown too.
                flush(&buf, &mut shown, &mut emit);
                return shown;
            }
            std::thread::sleep(std::time::Duration::from_millis(kind.poll_ms()));
        }
    });
    LogTail {
        done,
        handle: Some(handle),
    }
}

// ============================================================================
// Heavy Plasma forks (AeroThemePlasma / AeroShell): detection and rollback to stock
// ============================================================================
//
// The forks REPLACE Plasma binaries/QML, so a theme reset does not remove
// them: only pacman cures it. The provider of a stock package cannot be
// `-R`'d (reverse dependencies hold it), a `pacman -S <stock>` in one
// transaction is needed.

/// Fork package name prefixes (covering the -git/-x11-git variants).
pub const PLASMA_FORK_PREFIXES: &[&str] = &["aerothemeplasma-", "aeroshell-"];
/// Known forks outside the prefixes.
pub const PLASMA_FORK_EXTRA: &[&str] = &["uac-polkit-agent-git"];
/// FALLBACK map "stock package ← provider fork prefixes". The main path
/// (`fork_stock_candidates`) takes names from pacman's live provides/replaces;
/// the map adds its candidate last, for when the fork's metadata is empty.
/// Any name from here still passes the repository existence check, so it
/// cannot go stale unnoticed.
pub const PLASMA_FORK_STOCK_REPLACEMENTS: &[(&str, &[&str])] = &[
    ("libplasma", &["aeroshell-libplasma"]),
    ("polkit-kde-agent", &["uac-polkit-agent"]),
];

fn is_plasma_fork_name(name: &str) -> bool {
    PLASMA_FORK_PREFIXES.iter().any(|p| name.starts_with(p))
        || PLASMA_FORK_EXTRA.contains(&name)
}

/// Port of `filter_plasma_forks`: pick the forks from a list of installed
/// package names. Sorted, without deduplication (like `sorted(generator)`).
pub fn filter_plasma_forks(package_names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = package_names
        .iter()
        .filter(|p| is_plasma_fork_name(p))
        .cloned()
        .collect();
    out.sort();
    out
}

/// Values of a `pacman -Qi <pkg>` field for the module-level fork rollback
/// functions: port of `_pacman_qi_field`/`_qi_field` (outside a manager
/// instance, so the family is fixed to Arch: elsewhere there is no pacman and
/// the callback returns None).
fn pacman_qi_field(
    package: &str,
    field: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    qi_field(PkgFamily::Arch, package, field, qi_output)
}

/// Candidate names for the stock replacement of a fork, in order of trust.
///
/// ⭐ The order is exactly this, and it is the key point of the whole fix:
///   1. `Provides`: what the fork replaces NOW. The fork build must keep this
///      field up to date, otherwise its own reverse dependencies would not
///      resolve, so the name here is live by construction.
///   2. `Replaces`: a list of HISTORICAL names: it accumulates over the years
///      and keeps dead names. This is exactly where the rollback broke:
///      AeroShell carries `replaces=(plasma-framework)` from Plasma 5 times,
///      KDE renamed the package to `libplasma` long ago, and
///      `pacman -S plasma-framework` failed with "target not found".
///   3. The fallback map, only if there was no metadata at all.
///
/// The fork itself and any names of its own family (`aeroshell-…`) are
/// filtered out: they cannot be stock. Duplicates are removed keeping order.
pub fn fork_stock_candidates(
    fork: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |name: String, out: &mut Vec<String>| {
        if name.is_empty() || name == fork || is_plasma_fork_name(&name) {
            return;
        }
        if !out.contains(&name) {
            out.push(name);
        }
    };
    for field in ["Provides", "Replaces"] {
        for name in pacman_qi_field(fork, field, qi_output) {
            push(name, &mut out);
        }
    }
    for (stock, prefixes) in PLASMA_FORK_STOCK_REPLACEMENTS {
        if prefixes.iter().any(|p| fork.starts_with(p)) {
            push((*stock).to_string(), &mut out);
        }
    }
    out
}

/// How the search for a stock package for a specific fork ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum ForkStockTarget {
    /// A standalone fork: there is no stock counterpart, it is removed by a direct `-Rns`.
    Standalone,
    /// Stock found AND exists in the enabled repositories.
    Found { package: String },
    /// The fork declares a replacement, but the repositories know NONE of the
    /// names. A rollback for such a fork is impossible: there would be nothing
    /// to restore.
    Unresolved { candidates: Vec<String> },
}

/// The stock package that replaces a fork on rollback.
///
/// `target_exists(name)`: whether such a target is in the repositories:
/// `Some(true)` it is, `Some(false)` it is not, `None` the check FAILED
/// (pacman unavailable, the database not synced). An uncheckable candidate is
/// not rejected: without pacman we would not run anything anyway, and the
/// user would rightly complain about a blind refusal. We reject only when
/// pacman answered clearly and the answer is negative for ALL names.
pub fn fork_stock_target(
    fork: &str,
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> ForkStockTarget {
    let candidates = fork_stock_candidates(fork, qi_output);
    if candidates.is_empty() {
        return ForkStockTarget::Standalone;
    }
    let mut unchecked: Option<String> = None;
    for name in &candidates {
        match target_exists(name) {
            Some(true) => return ForkStockTarget::Found { package: name.clone() },
            Some(false) => {}
            None => {
                if unchecked.is_none() {
                    unchecked = Some(name.clone());
                }
            }
        }
    }
    match unchecked {
        Some(package) => ForkStockTarget::Found { package },
        None => ForkStockTarget::Unresolved { candidates },
    }
}

/// A fork for which no stock was found, with the names it offered.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnresolvedFork {
    pub fork: String,
    pub candidates: Vec<String>,
}

/// Splitting a set of forks into rollback steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlasmaForkSplit {
    /// Standalone forks: a direct `pacman -Rns`.
    pub removable: Vec<String>,
    /// Stock packages for provider forks: `pacman -S` (a provider cannot be
    /// `-R`'d: reverse dependencies hold it, the transaction would abort).
    pub stock: Vec<String>,
    /// Forks for which no stock was found: rolling them back is forbidden.
    pub unresolved: Vec<UnresolvedFork>,
}

/// Port of `_plasma_fork_split`. The decision for EACH fork is taken from its
/// pacman metadata and checked against the repositories, so a new provider
/// fork (and the next KDE rename) is handled correctly without code changes.
pub fn plasma_fork_split(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkSplit {
    let mut out = PlasmaForkSplit {
        removable: Vec::new(),
        stock: Vec::new(),
        unresolved: Vec::new(),
    };
    for f in forks {
        match fork_stock_target(f, qi_output, target_exists) {
            ForkStockTarget::Found { package } => {
                if !out.stock.contains(&package) {
                    out.stock.push(package);
                }
            }
            ForkStockTarget::Standalone => {
                if !out.removable.contains(f) {
                    out.removable.push(f.clone());
                }
            }
            ForkStockTarget::Unresolved { candidates } => {
                if !out.unresolved.iter().any(|u| &u.fork == f) {
                    out.unresolved.push(UnresolvedFork {
                        fork: f.clone(),
                        candidates,
                    });
                }
            }
        }
    }
    out
}

/// Port of `plasma_fork_recovery_command`: a human-readable full rollback
/// command (for display in the UI/CLI). None: no forks or no actions. Only
/// existing targets go into the command: showing the user a `pacman -S` with
/// a nonexistent package is exactly the garbage they saw in the dialog.
pub fn plasma_fork_recovery_command(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> Option<String> {
    if forks.is_empty() {
        return None;
    }
    let split = plasma_fork_split(forks, qi_output, target_exists);
    let mut parts: Vec<String> = Vec::new();
    if !split.removable.is_empty() {
        parts.push(format!("sudo pacman -Rns {}", split.removable.join(" ")));
    }
    if !split.stock.is_empty() {
        parts.push(format!("sudo pacman -S {}", split.stock.join(" ")));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("  &&  "))
    }
}

/// Refusal explanation: why the rollback does not start and what the person
/// should do. Without a single word from pacman's output: it tells the user nothing.
pub fn plasma_fork_unresolved_message(unresolved: &[UnresolvedFork]) -> String {
    let mut s = format!(
        "{}\n\n",
        tr("The rollback didn't start: no stock package was found to replace the fork.")
    );
    for u in unresolved {
        let names = if u.candidates.is_empty() {
            tr("(the fork didn't name any package)")
        } else {
            u.candidates.join(", ")
        };
        s.push_str(&format!(
            "  • {}\n",
            trf("%1 — not in the repositories: %2", &[&u.fork, &names])
        ));
    }
    s.push('\n');
    s.push_str(&tr(
        "Usually this means KDE renamed the stock package, while the fork build \
         still uses the old name.\n\
         Nothing was removed — your system is untouched.\n\n\
         What to do: refresh the package database (System Update) and try \
         again. If the name still can't be found, the fork needs a fresh \
         rebuild — it will bring the correct stock name.",
    ));
    s
}

/// An explanation on top of pacman's raw output when the transaction did
/// fail. `remaining` is the command left to run (recomputed AFTER the failure:
/// some steps may have gone through).
pub fn plasma_fork_failure_message(raw: &str, remaining: Option<&str>) -> String {
    let raw = raw.trim();
    let mut s = format!("{}\n\n", tr("pacman didn't finish the rollback."));
    if !raw.is_empty() {
        s.push_str(&format!("{}\n\n", trf("What pacman said:\n%1", &[&raw])));
    }
    match remaining {
        Some(cmd) => s.push_str(&trf(
            "Still to run (you can do it by hand in a terminal):\n%1",
            &[&cmd],
        )),
        None => s.push_str(&tr(
            "No forks are left on the system — the rollback seems to have gone \
             through after all. Log out and log back in.",
        )),
    }
    s
}

/// The `remove_plasma_forks` decision without execution. All variants except
/// `Run` carry a READY answer for the person (`ok` + `message`): the executor
/// only has to pass it on as is, it has no texts of its own and no "is this
/// good or bad" parsing of its own.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum PlasmaForkReset {
    /// No forks at all: "already as it should be" (`ok = true`).
    NoForks { ok: bool, message: String },
    /// There are forks, but the split gave no steps: also `ok = true`.
    NoActions { ok: bool, message: String },
    /// ⛔ No stock was found for at least one fork: WE RUN NOTHING. `-Rns` is
    /// irreversible, and there is nothing to restore stock with: exactly how the
    /// owner ended up with a half-removed look. A refusal before pacman, entirely.
    Blocked {
        ok: bool,
        unresolved: Vec<UnresolvedFork>,
        message: String,
    },
    /// One `pkexec sh -c` (one password prompt). `shell` is the body WITHOUT the
    /// `set -e; ` prefix (as in PlymouthReset/SddmReset), `removable`/`stock` are
    /// for the log.
    Run {
        cmd: Cmd,
        shell: String,
        removable: Vec<String>,
        stock: Vec<String>,
    },
}

/// Port of the deciding part of `remove_plasma_forks`. The original computes
/// the recovery command for the pkexec refusal message LAZILY (a second pass
/// over pacman), so it is not here: call `plasma_fork_recovery_command` in
/// the error branch, as in the original.
///
/// The order of steps (`-Rns`, then `-S`) MUST NOT be changed: standalone
/// forks (`aeroshell-workspace-git`, `aeroshell-kwin-components-git`) depend
/// on the `aeroshell-libplasma` provider, and `pacman -S libplasma` as the
/// first step would abort with "breaks dependency". So the only protection
/// against "removed, and nothing to restore" is checking the targets BEFORE
/// the transaction: the `Blocked` branch.
pub fn plasma_fork_reset_plan(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkReset {
    if forks.is_empty() {
        return PlasmaForkReset::NoForks {
            ok: true,
            message: plasma_fork_none_message(),
        };
    }
    let split = plasma_fork_split(forks, qi_output, target_exists);
    if !split.unresolved.is_empty() {
        let message = plasma_fork_unresolved_message(&split.unresolved);
        return PlasmaForkReset::Blocked {
            ok: false,
            unresolved: split.unresolved,
            message,
        };
    }
    let mut steps: Vec<String> = Vec::new();
    if !split.removable.is_empty() {
        steps.push(format!(
            "pacman -Rns --noconfirm {}",
            split.removable.iter().map(|f| shlex_quote(f)).collect::<Vec<_>>().join(" ")
        ));
    }
    if !split.stock.is_empty() {
        steps.push(format!(
            "pacman -S --noconfirm --needed {}",
            split.stock.iter().map(|s| shlex_quote(s)).collect::<Vec<_>>().join(" ")
        ));
    }
    if steps.is_empty() {
        return PlasmaForkReset::NoActions {
            ok: true,
            message: plasma_fork_no_actions_message(),
        };
    }
    let shell = steps.join("; ");
    PlasmaForkReset::Run {
        // set -e: if -Rns failed, we do not install stock on top of a broken state.
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(600),
            false,
        ),
        shell,
        removable: split.removable,
        stock: split.stock,
    }
}

/// Memory of "is the target in the repositories" probe answers: port of
/// `_target_exists_probe`.
///
/// The same stock package shows up for several forks (`libplasma` is
/// declared both by `aeroshell-libplasma-git` and by the fallback map), and a
/// full rollback goes over the set of forks TWICE: as the plan and as the
/// human-readable command. Without memory `pacman -Si` would be called for
/// every repeat. The "ask or take from memory" decision is here; the trip
/// outside itself is passed as a callback.
#[derive(Debug, Default, Clone)]
pub struct TargetExistsMemo {
    seen: HashMap<String, Option<bool>>,
}

impl TargetExistsMemo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer by name: asks the probe ONLY about what it has not asked yet.
    /// `None` ("could not ask") is remembered too: there is nothing to re-ask,
    /// the answer will not change, and an extra pacman run costs seconds.
    pub fn answer(
        &mut self,
        package: &str,
        probe: &dyn Fn(&str) -> Option<bool>,
    ) -> Option<bool> {
        if let Some(known) = self.seen.get(package) {
            return *known;
        }
        let answer = probe(package);
        self.seen.insert(package.to_string(), answer);
        answer
    }

    /// How many DIFFERENT names have been asked (for run count checks).
    pub fn asked(&self) -> usize {
        self.seen.len()
    }
}

/// No forks in the system: the preview has nothing to roll back.
pub fn plasma_fork_nothing_message() -> String {
    tr("No Plasma forks found — nothing to roll back.")
}
/// There are forks, but the rollback command did not come together.
pub fn plasma_fork_no_command_message() -> String {
    tr("Couldn't build the rollback command.")
}
/// Idempotent answer of `remove_plasma_forks` when there are no forks at all.
pub fn plasma_fork_none_message() -> String {
    tr("No Plasma forks found")
}
/// There are forks, but not a single rollback step came out of them.
pub fn plasma_fork_no_actions_message() -> String {
    tr("Nothing to do")
}
/// What replaces the empty output of a successful transaction.
pub fn plasma_fork_reset_done_message() -> String {
    tr("Forks removed, stock packages restored.")
}

/// The answer of a fork operation to the person: whether it worked and what to say.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlasmaForkAnswer {
    pub ok: bool,
    pub message: String,
}

/// Port of `plasma_fork_reset_preview`: what will be done, WITHOUT a transaction.
///
/// `(true, command)`: a rollback is possible, and this is exactly what will
/// run; `(false, explanation)`: there is nothing to roll back with or
/// nothing to roll back, and the person is told why in plain language, not
/// with pacman's output.
///
/// The target existence probe is wrapped in [`TargetExistsMemo`]: the plan
/// and the human-readable command go over the forks twice, and without
/// memory `pacman -Si` would be asked twice for each name.
pub fn plasma_fork_preview(
    forks: &[String],
    qi_output: &dyn Fn(&str) -> Option<String>,
    target_exists: &dyn Fn(&str) -> Option<bool>,
) -> PlasmaForkAnswer {
    let nothing = || PlasmaForkAnswer {
        ok: false,
        message: plasma_fork_nothing_message(),
    };
    if forks.is_empty() {
        return nothing();
    }
    let memo = std::cell::RefCell::new(TargetExistsMemo::new());
    let cached = |pkg: &str| memo.borrow_mut().answer(pkg, target_exists);
    match plasma_fork_reset_plan(forks, qi_output, &cached) {
        PlasmaForkReset::Blocked { message, .. } => PlasmaForkAnswer { ok: false, message },
        PlasmaForkReset::NoForks { .. } | PlasmaForkReset::NoActions { .. } => nothing(),
        PlasmaForkReset::Run { .. } => {
            match plasma_fork_recovery_command(forks, qi_output, &cached) {
                Some(cmd) => PlasmaForkAnswer { ok: true, message: cmd },
                None => PlasmaForkAnswer {
                    ok: false,
                    message: plasma_fork_no_command_message(),
                },
            }
        }
    }
}

/// Port of the parsing part of `remove_plasma_forks`: how pkexec ended.
///
/// Nothing external here: Python passes the exit code and both streams, the
/// decision and the text are here. `returncode` = `None` when pkexec was not
/// found at all.
///
/// Both recovery commands are computed LAZILY and are DIFFERENT, as in the
/// original:
///   * `planned_command`: what we were going to run; needed only on an
///     authorization refusal, when the system is untouched;
///   * `remaining_command`: recomputed from the LIVE state after a failure:
///     some steps may have gone through, and the person needs the remainder,
///     not the original command.
///
/// ⭐ Fixed in the port: the original put `str(None)` into the hint and
/// showed the person "Run by hand in a terminal: None" if the rollback
/// command did not come together. Here its place is taken by the body of the
/// transaction itself, `sudo <shell>`, exactly what the shared
/// [`pkexec_failure_message`] does.
pub fn plasma_fork_reset_outcome(
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
    planned_command: &dyn Fn() -> Option<String>,
    remaining_command: &dyn Fn() -> Option<String>,
) -> PlasmaForkAnswer {
    let Some(rc) = returncode else {
        return PlasmaForkAnswer {
            ok: false,
            message: pkexec_missing_message(),
        };
    };
    if rc == 0 {
        // pacman's output is NOT trimmed: the original passes it as is.
        return PlasmaForkAnswer {
            ok: true,
            message: if stdout.is_empty() {
                plasma_fork_reset_done_message()
            } else {
                stdout.to_string()
            },
        };
    }
    let raw = if !stderr.is_empty() { stderr } else { stdout };
    let err = raw.trim().to_string();
    let message = if pkexec_auth_denied(rc, &err) {
        let manual = planned_command().unwrap_or_else(|| format!("sudo {shell}"));
        pkexec_denied_message(&manual)
    } else {
        plasma_fork_failure_message(&err, remaining_command().as_deref())
    };
    PlasmaForkAnswer { ok: false, message }
}

// ============================================================================
// Directories: common selection of entries
// ============================================================================

/// What we take from a directory listing and in what order. The input is a
/// raw list of `(name, is it fit)`: Python lists the directory and checks the
/// attribute (`os.listdir`, `os.path.isdir`), and the decision is here.
///
/// The order is set HERE and only here: directory traversal in the file
/// system is arbitrary, and both sides need a lexicographic one: SDDM merges
/// its `*.conf` that way (later ones override earlier ones), and the list of
/// Plymouth themes is shown to the person that way. The "fit" attribute
/// differs per format (for SDDM a name suffix, for Plymouth that it is a
/// subdirectory), so it is passed ready, and the selection and order stay common.
pub fn dir_entries_selected(entries: &[(String, bool)]) -> Vec<String> {
    let mut out: Vec<String> = entries
        .iter()
        .filter(|(_, keep)| *keep)
        .map(|(name, _)| name.clone())
        .collect();
    out.sort();
    out
}

// ============================================================================
// Plymouth (boot splash): detection, choosing the distro default, reset
// ============================================================================
//
// Plymouth is SYSTEM level (root, /etc + initramfs), a Plasma theme reset
// does not touch it. A reset rebuilds the initramfs → only from an explicit
// recovery step.

pub const PLYMOUTH_THEMES_DIR: &str = "/usr/share/plymouth/themes";
pub const PLYMOUTH_DEFAULTS: &str = "/usr/share/plymouth/plymouthd.defaults";
/// Distro branding theme → provider package. If the package is installed,
/// that is "as on the distro stock" and takes priority over the general
/// default of the plymouth package (bgrt, which shows the FIRMWARE BGRT logo).
pub const PLYMOUTH_DISTRO_BRANDING: &[(&str, &str)] =
    &[("cachyos-bootanimation", "cachyos-plymouth-bootanimation")];
/// Fallback candidates if neither the branding nor the package default fits.
pub const PLYMOUTH_FALLBACK_CANDIDATES: &[&str] = &["bgrt", "spinner"];

/// Port of `_plymouth_theme_from_default_link`: the theme name from the
/// `default.plymouth` symlink target (the path for distros without the
/// plymouth-set-default-theme binary, e.g. Ubuntu). `realpath_existing` is
/// the link's realpath if such a path exists (None = no link / broken target
/// / OSError).
pub fn plymouth_theme_from_link(realpath_existing: Option<&str>) -> Option<String> {
    let target = realpath_existing?;
    // basename(dirname(target)): the theme directory.
    let dir = target.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let name = dir.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Path to the theme description: `<themes dir>/<theme>/<theme>.plymouth`.
/// Exactly this file makes a subdirectory a theme: without it
/// `plymouth-set-default-theme` does not see the theme and
/// `update-alternatives` has nothing to point to. One join for the whole
/// module: the same string used to exist in three copies.
pub fn plymouth_theme_file(theme: &str) -> String {
    plymouth_theme_file_in(PLYMOUTH_THEMES_DIR, theme)
}

/// The same in a GIVEN themes directory. The directory is passed as an
/// argument so that the preview and tests do not go to the live
/// `/usr/share/plymouth/themes`: `list_plymouth_themes` has its own directory
/// (`_PLYMOUTH_THEMES_DIR`), and the file probe must go to THE SAME directory
/// that was listed. On the production path (`themes_dir` =
/// [`PLYMOUTH_THEMES_DIR`]) the answer is the same as before.
pub fn plymouth_theme_file_in(themes_dir: &str, theme: &str) -> String {
    let dir = themes_dir.trim_end_matches('/');
    if dir.is_empty() {
        return format!("{theme}/{theme}.plymouth");
    }
    format!("{dir}/{theme}/{theme}.plymouth")
}

/// The first half of `list_plymouth_themes`: which entries of the themes
/// directory are worth checking at all. The input is the raw listing as
/// `(name, is it a directory?)` pairs.
pub fn plymouth_theme_candidates(entries: &[(String, bool)]) -> Vec<String> {
    dir_entries_selected(entries)
}

/// The second half of `list_plymouth_themes`: installed themes from the
/// probes `(name, whether <theme>.plymouth exists)`.
///
/// ⚠️ Fixed in the port. The original returned ALL subdirectories of
/// `/usr/share/plymouth/themes` and claimed in a comment that this was "the
/// same result" as `plymouth-set-default-theme --list`. That is not true:
/// plymouth itself lists only directories with `<theme>.plymouth` inside. An
/// empty directory remains after the theme package is removed (with someone
/// else's file in it or just nothing), and such a "theme" passed into
/// [`target_plymouth_theme`] as installed: if it was called `bgrt` or
/// `spinner`, Rexon declared it the distribution's native default and reset
/// the splash to something nonexistent: `plymouth-set-default-theme bgrt -R`
/// rebuilt the initramfs for nothing. The same module did check the theme
/// file on the alternatives path ([`PlymouthReset::MissingThemeFile`]): the
/// check existed, but only in one branch of two. Now the list is honest for both.
pub fn plymouth_installed_themes(probes: &[(String, bool)]) -> Vec<String> {
    dir_entries_selected(probes)
}

/// Port of parsing the output of `plymouth-set-default-theme` without
/// arguments: it prints the current theme name. `None` in the answer: no name
/// (the binary did not start, crashed or stayed silent), the caller goes to
/// `default.plymouth`. `returncode` = `None` when `safe_run` returned None.
pub fn plymouth_theme_from_output(returncode: Option<i32>, stdout: &str) -> Option<String> {
    if returncode != Some(0) {
        return None;
    }
    let name = stdout.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Port of `_plymouth_package_default`: the `Theme=` line from
/// plymouthd.defaults (None: no file or an empty value).
pub fn plymouth_package_default(defaults_text: Option<&str>) -> Option<String> {
    let text = defaults_text?;
    for line in text.lines() {
        let line = line.trim();
        let bytes = line.as_bytes();
        if bytes.len() >= 6 && bytes[..6].eq_ignore_ascii_case(b"theme=") {
            // The first 6 bytes are ASCII, slicing at a character boundary is safe.
            let v = line[6..].trim();
            // The original returns from the FIRST matching line, even an empty one.
            return if v.is_empty() { None } else { Some(v.to_string()) };
        }
    }
    None
}

/// Port of `target_plymouth_theme`: the distribution's native default,
/// without hardcoding "cachyos only". `pkg_installed` and `package_default`
/// are called LAZILY in the original's order (branding → plymouthd.defaults →
/// fallbacks).
pub fn target_plymouth_theme(
    installed_themes: &[String],
    pkg_installed: &dyn Fn(&str) -> bool,
    package_default: &dyn Fn() -> Option<String>,
) -> Option<String> {
    if installed_themes.is_empty() {
        return None;
    }
    let installed = |t: &str| installed_themes.iter().any(|x| x == t);
    for (theme, pkg) in PLYMOUTH_DISTRO_BRANDING {
        if installed(theme) && pkg_installed(pkg) {
            return Some((*theme).to_string());
        }
    }
    if let Some(d) = package_default() {
        if installed(&d) {
            return Some(d);
        }
    }
    for cand in PLYMOUTH_FALLBACK_CANDIDATES {
        if installed(cand) {
            return Some((*cand).to_string());
        }
    }
    None
}

/// Port of `plymouth_reset_command`: a human-readable reset command (for
/// display). The method is chosen by the presence of the tool, not by family
/// (Ubuntu has no binary at all, the theme is switched only by alternatives).
pub fn plymouth_reset_command(target: &str, has_set_default_theme: bool) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    if has_set_default_theme {
        return Some(format!(
            "sudo plymouth-set-default-theme {} -R",
            shlex_quote(target)
        ));
    }
    let pl = plymouth_theme_file(target);
    Some(format!(
        "sudo update-alternatives --set default.plymouth {} && sudo update-initramfs -u",
        shlex_quote(&pl)
    ))
}

/// The `reset_plymouth_to_default` decision without execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum PlymouthReset {
    /// `(False, <"Could not determine the native Plymouth default">)`.
    NoTarget,
    /// Idempotence: the current theme is already the target.
    AlreadyDefault { target: String },
    /// The alternatives path, but the theme has no `<theme>.plymouth`.
    MissingThemeFile { target: String, path: String },
    /// Nothing to switch with: neither plymouth-set-default-theme nor
    /// update-alternatives.
    NoTool,
    /// One `pkexec sh -c "set -e; …"`; `shell` is the body for the error text.
    Run { cmd: Cmd, shell: String, target: String },
}

/// Port of the deciding part of `reset_plymouth_to_default`.
///
/// The original reads `current` ONLY after the target is determined: the
/// shim must keep this laziness (`None` while there is no target).
/// `theme_file_exists(path)` is `os.path.isfile` for the alternatives path.
pub fn plymouth_reset_plan(
    target: Option<&str>,
    current: Option<&str>,
    has_set_default_theme: bool,
    has_update_alternatives: bool,
    theme_file_exists: &dyn Fn(&str) -> bool,
) -> PlymouthReset {
    let target = match target {
        Some(t) if !t.is_empty() => t,
        _ => return PlymouthReset::NoTarget,
    };
    if current == Some(target) {
        return PlymouthReset::AlreadyDefault { target: target.to_string() };
    }
    let shell = if has_set_default_theme {
        format!("plymouth-set-default-theme {} -R", shlex_quote(target))
    } else if has_update_alternatives {
        let pl = plymouth_theme_file(target);
        if !theme_file_exists(&pl) {
            return PlymouthReset::MissingThemeFile {
                target: target.to_string(),
                path: pl,
            };
        }
        format!(
            "update-alternatives --set default.plymouth {} && update-initramfs -u",
            shlex_quote(&pl)
        )
    } else {
        return PlymouthReset::NoTool;
    };
    PlymouthReset::Run {
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(600),
            false,
        ),
        shell,
        target: target.to_string(),
    }
}

/// Could not determine the distribution's native default: nothing to reset to.
pub fn plymouth_no_target_message() -> String {
    tr("Couldn't determine the distribution's default Plymouth theme")
}
/// Neither `plymouth-set-default-theme` nor `update-alternatives` is in the system.
pub fn plymouth_no_tool_message() -> String {
    tr("Nothing to switch the boot splash with: neither \
        plymouth-set-default-theme nor update-alternatives was found")
}
/// Text that replaces the empty output of a failed pkexec.
pub fn plymouth_reset_failed_message() -> String {
    tr("Couldn't reset Plymouth")
}

/// Idempotent answer: the theme is already the one being reset to.
pub fn plymouth_already_default_message(target: &str) -> String {
    trf("Plymouth is already on the default theme “%1”", &[&target])
}

/// The alternatives path, but the theme description is not in the directory.
pub fn plymouth_missing_theme_file_message(target: &str) -> String {
    // TRANSLATORS: %1 is a theme name; “%1.plymouth” is its file name.
    trf("The “%1” theme has no %1.plymouth file", &[&target])
}

/// Success: the initramfs was rebuilt, it will be visible after a reboot.
pub fn plymouth_reset_success_message(target: &str) -> String {
    trf(
        "Plymouth reset to “%1” (initramfs rebuilt; takes effect after a restart).",
        &[&target],
    )
}

/// Result of `reset_plymouth_to_default`: `(ok, message)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlymouthResetOutcome {
    pub ok: bool,
    pub message: String,
}

/// The answer the caller gets WITHOUT running pkexec. `None` only for
/// [`PlymouthReset::Run`]: there the answer comes from
/// [`plymouth_reset_outcome`] by the exit code. These four texts used to
/// live as strings in Python.
pub fn plymouth_reset_precheck(plan: &PlymouthReset) -> Option<PlymouthResetOutcome> {
    let (ok, message) = match plan {
        PlymouthReset::NoTarget => (false, plymouth_no_target_message()),
        PlymouthReset::AlreadyDefault { target } => {
            (true, plymouth_already_default_message(target))
        }
        PlymouthReset::MissingThemeFile { target, .. } => {
            (false, plymouth_missing_theme_file_message(target))
        }
        PlymouthReset::NoTool => (false, plymouth_no_tool_message()),
        PlymouthReset::Run { .. } => return None,
    };
    Some(PlymouthResetOutcome { ok, message })
}

/// The reset plan together with a ready answer for outcomes where there is
/// no reason to go outside. Python decides nothing here: it either returns
/// `(ok, message)` or runs `cmd` and calls [`plymouth_reset_outcome`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlymouthResetDecision {
    #[serde(flatten)]
    pub plan: PlymouthReset,
    /// `None` = the decision requires a run (`kind` = `Run`).
    pub ok: Option<bool>,
    pub message: Option<String>,
}

/// [`plymouth_reset_plan`] together with the answer text: what goes into the bridge.
pub fn plymouth_reset_decision(
    target: Option<&str>,
    current: Option<&str>,
    has_set_default_theme: bool,
    has_update_alternatives: bool,
    theme_file_exists: &dyn Fn(&str) -> bool,
) -> PlymouthResetDecision {
    let plan = plymouth_reset_plan(
        target,
        current,
        has_set_default_theme,
        has_update_alternatives,
        theme_file_exists,
    );
    let pre = plymouth_reset_precheck(&plan);
    PlymouthResetDecision {
        plan,
        ok: pre.as_ref().map(|o| o.ok),
        message: pre.map(|o| o.message),
    }
}

/// Parsing the pkexec result for `reset_plymouth_to_default`. Nothing
/// external here: Python passes the exit code and both streams, the decision
/// and the text are here.
///
/// `returncode` = `None` when pkexec was not found (`safe_run` returned
/// `None`). `shell` is the command body from [`PlymouthReset::Run`], it also
/// goes into the "run it by hand" hint.
pub fn plymouth_reset_outcome(
    target: &str,
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> PlymouthResetOutcome {
    let Some(code) = returncode else {
        return PlymouthResetOutcome {
            ok: false,
            message: pkexec_missing_message(),
        };
    };
    if code == 0 {
        return PlymouthResetOutcome {
            ok: true,
            message: plymouth_reset_success_message(target),
        };
    }
    PlymouthResetOutcome {
        ok: false,
        message: pkexec_failure_message(
            code,
            stdout,
            stderr,
            &plymouth_reset_failed_message(),
            shell,
        ),
    }
}

// ============================================================================
// SDDM (login screen): detect-driven reset to the DISTRIBUTION STOCK
// ============================================================================
//
// Rexon never writes into other people's SDDM files: it puts its theme into
// its own override /etc/sddm.conf.d/zz-rexon-theme.conf. SDDM merges
// /etc/sddm.conf.d/*.conf on top of the distro defaults in
// /usr/lib/sddm/sddm.conf.d/, so "back to out of the box" = simply DELETE
// our override and the fork trace files: without hardcoding a theme and
// without overwriting kde_settings.conf.

pub const SDDM_CONF_D: &str = "/etc/sddm.conf.d";
pub const SDDM_DISTRO_CONF_D: &str = "/usr/lib/sddm/sddm.conf.d";
/// Overrides created by Rexon itself (always safe to delete).
pub const SDDM_REXON_FILES: &[&str] = &["zz-rexon-theme.conf"];
/// Fork theme tokens in the file name OR the override contents. The original
/// `_SDDM_FORK_RE` is an alternation of literals with `re.I`, which is
/// equivalent to a case-insensitive substring search (we do not pull our own
/// regex engine into the core).
pub const SDDM_FORK_TOKENS: &[&str] = &[
    "wackyideas", "sevenstart", "aeroshell", "aerothemeplasma", "se7enaero", "win7os",
];

fn has_sddm_fork_token(s: &str) -> bool {
    let low = s.to_lowercase();
    SDDM_FORK_TOKENS.iter().any(|t| low.contains(t))
}

/// Port of `_sddm_files_to_remove`. Input: (basename, contents). We delete
/// our own overrides by name + fork trace files (a token in the NAME or the
/// CONTENTS); kde_settings.conf (the KDE/distro file) and other user configs
/// are not touched.
pub fn sddm_files_to_remove(entries: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (name, content) in entries {
        let base = name.rsplit('/').next().unwrap_or(name).to_string();
        if SDDM_REXON_FILES.contains(&base.as_str()) {
            out.push(base);
            continue;
        }
        if has_sddm_fork_token(&base) || has_sddm_fork_token(content) {
            out.push(base);
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `[Theme] Current` from a config text. Port of `configparser(strict=False)`:
/// section names are case-sensitive, keys are not; with duplicate keys the
/// last one wins.
fn ini_theme_current(text: &str) -> Option<String> {
    let mut in_section = false;
    let mut found: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
            in_section = &t[1..t.len() - 1] == "Theme";
            continue;
        }
        if !in_section {
            continue;
        }
        let Some(pos) = t.find(['=', ':']) else {
            continue;
        };
        if t[..pos].trim().eq_ignore_ascii_case("current") {
            found = Some(t[pos + 1..].trim().to_string());
        }
    }
    found
}

/// SDDM theme from a set of configs: the common core of
/// `_sddm_read_current_theme`. The input must be sorted by name: SDDM merges
/// lexicographically, later files override. An empty value does NOT reset
/// what was found earlier.
pub fn sddm_current_theme_from(entries: &[(String, String)]) -> Option<String> {
    let mut result: Option<String> = None;
    for (name, content) in entries {
        if !name.ends_with(".conf") {
            continue;
        }
        if let Some(v) = ini_theme_current(content) {
            if !v.is_empty() {
                result = Some(v);
            }
        }
    }
    result
}

/// Port of `sddm_distro_default_theme`: the sddm package's distro defaults,
/// with `/etc/sddm.conf.d/kde_settings.conf` on top (it is installed by
/// KDE/the distro, it is "out of the box", NOT Rexon). None: could not be
/// determined.
pub fn sddm_distro_default_theme(
    distro_entries: &[(String, String)],
    kde_settings: Option<&str>,
) -> Option<String> {
    let mut result = sddm_current_theme_from(distro_entries);
    if let Some(text) = kde_settings {
        if let Some(v) = ini_theme_current(text) {
            if !v.is_empty() {
                result = Some(v);
            }
        }
    }
    result
}

/// Port of `sddm_reset_plan`: what we delete and which theme becomes the default.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SddmResetPlan {
    pub to_remove: Vec<String>,
    pub default_theme: Option<String>,
}

/// `conf_entries` is (name, contents) of the /etc/sddm.conf.d files in sort
/// order; the `.conf` filter is applied here (as in the original).
pub fn sddm_reset_plan(
    conf_entries: &[(String, String)],
    distro_entries: &[(String, String)],
    kde_settings: Option<&str>,
) -> SddmResetPlan {
    let conf: Vec<(String, String)> = conf_entries
        .iter()
        .filter(|(n, _)| n.ends_with(".conf"))
        .cloned()
        .collect();
    SddmResetPlan {
        to_remove: sddm_files_to_remove(&conf),
        default_theme: sddm_distro_default_theme(distro_entries, kde_settings),
    }
}

/// The `reset_sddm_to_default` decision without execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum SddmReset {
    /// Idempotent no-op: `(True, <"…already on the stock config">, [])`.
    AlreadyStock,
    /// One `pkexec sh -c "set -e; rm -f …"`; `shell` is the body for the error text.
    Run {
        cmd: Cmd,
        shell: String,
        removed: Vec<String>,
        default_theme: Option<String>,
    },
}

/// Port of the deciding part of `reset_sddm_to_default`. `conf_d` is the
/// directory the plan's basenames are joined to (by default [`SDDM_CONF_D`]).
pub fn sddm_reset_command(plan: &SddmResetPlan, conf_d: &str) -> SddmReset {
    if plan.to_remove.is_empty() {
        return SddmReset::AlreadyStock;
    }
    let quoted = plan
        .to_remove
        .iter()
        .map(|b| shlex_quote(&format!("{conf_d}/{b}")))
        .collect::<Vec<_>>()
        .join(" ");
    let shell = format!("rm -f {quoted}");
    SddmReset::Run {
        cmd: Cmd::new(
            vec![
                "pkexec".into(),
                "sh".into(),
                "-c".into(),
                format!("set -e; {shell}"),
            ],
            Some(120),
            false,
        ),
        shell,
        removed: plan.to_remove.clone(),
        default_theme: plan.default_theme.clone(),
    }
}

/// Success text of `reset_sddm_to_default` (compared text for text with Python).
pub fn sddm_reset_success_message(removed: &[String], default_theme: Option<&str>) -> String {
    let tail = match default_theme {
        Some(t) if !t.is_empty() => trf(
            "The login screen will go back to the distribution default “%1”.",
            &[&t],
        ),
        _ => tr("The login screen will go back to the distribution default."),
    };
    format!(
        "{} {tail}",
        trf("Removed SDDM overrides: %1.", &[&removed.join(", ")])
    )
}

/// Name of the KDE/distro file inside `/etc/sddm.conf.d`. It is put there by
/// the KDE install: it is "out of the box", NOT Rexon: it is read separately
/// from the common scan and never gets into the removal list.
pub const SDDM_KDE_SETTINGS_FILE: &str = "kde_settings.conf";

/// Path to `kde_settings.conf` inside the given directory: port of the
/// `os.path.join` from `_sddm_kde_settings_text`.
///
/// ⚠️ Fixed in the port: the original joined the path with a HARDCODED
/// `_SDDM_CONF_D` and ignored the `conf_d` argument that `sddm_reset_plan`
/// and `_sddm_read_current_theme` accept. Because of this a preview or a
/// test on a substituted directory still read the live machine's real
/// `/etc/sddm.conf.d/kde_settings.conf` and could declare as the distro
/// default a theme that is not in that directory. Now the directory is set
/// by the argument; on the production path (`conf_d` = [`SDDM_CONF_D`]) the
/// answer is the same as before.
pub fn sddm_kde_settings_path(conf_d: &str) -> String {
    let dir = conf_d.trim_end_matches('/');
    if dir.is_empty() {
        return SDDM_KDE_SETTINGS_FILE.to_string();
    }
    format!("{dir}/{SDDM_KDE_SETTINGS_FILE}")
}

/// Which files of the SDDM directory to read and in what order: the pure
/// part of `_sddm_conf_entries`. The input is the raw directory listing.
///
/// The order is mandatory: SDDM merges `*.conf` lexicographically, later
/// ones override earlier ones. Anything that is not `*.conf` does not count
/// as a settings directory entry. The original computed the same correctly
/// (`sorted(os.listdir(...))`): the rule moved here not as a fix, but to
/// live in one place with the parsing that depends on this order.
pub fn sddm_conf_read_order(names: &[String]) -> Vec<String> {
    let entries: Vec<(String, bool)> = names
        .iter()
        .map(|n| (n.clone(), n.ends_with(".conf")))
        .collect();
    dir_entries_selected(&entries)
}

/// Assembling (name, contents) pairs from the read results: the second pure
/// part of `_sddm_conf_entries`. `None` = the file could not be read: it
/// still stays in the list with EMPTY contents, because it can be recognised
/// by its NAME alone (a fork token), as in the original.
pub fn sddm_conf_entries(reads: &[(String, Option<String>)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = reads
        .iter()
        .filter(|(n, _)| n.ends_with(".conf"))
        .map(|(n, c)| (n.clone(), c.clone().unwrap_or_default()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

// ============================================================================
// The login screen's default session: an X11 session → its Wayland pair
// ============================================================================
//
// Foolproof guard: the login screen opens by itself (autologin) or offers
// first an X11 session that is leaving the system (Plasma (X11) with Plasma
// 6.8): the person gets stuck at login. Rexon moves ONLY the session keys
// to the pair from the table; it neither enables nor disables autologin,
// and does not touch other keys and lines of the files. A new pair is a new
// table entry.
//
// Where login screens keep the session (from their sources and man pages):
// * SDDM: `[Autologin] Session=` (sddm.conf(5): the files
//   `/usr/lib/sddm/sddm.conf.d/*.conf`, then `/etc/sddm.conf.d/*.conf` by
//   name, `/etc/sddm.conf` last; a later one overrides) and the last chosen
//   `[Last] Session=` in `state.conf` in the sddm user's home directory, the
//   full path of the session file; its greeter offers it first, and an empty
//   `[Autologin] Session=` with autologin takes the same one.
// * Plasma Login Manager: `[Autologin] Session=` (the daemon) and
//   `[Greeter] PreselectedSession=` (the greeter; overrides the last chosen
//   one). The main file `/etc/plasmalogin.conf` (written in full by the
//   «System Settings» module) overrides the directories
//   `/usr/lib/plasmalogin/plasmalogin.conf.d`, then `/etc/plasmalogin.conf.d`.
//   The greeter keeps the last chosen session itself (the plasmalogin
//   user's state file, per person): Rexon does not write it: even the
//   settings module goes there after dropping privileges to that user, and
//   the session there changes by itself at the first Wayland login.

/// An X11 session that has a Wayland pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSuccessor {
    /// X11 session file name without `.desktop` (`/usr/share/xsessions/<name>`).
    pub x11: &'static str,
    /// Pair file name in [`WAYLAND_SESSIONS_DIR`] without `.desktop`.
    pub wayland: &'static str,
    /// Names for the person, as in the login screen list (not translated).
    pub x11_name: &'static str,
    pub wayland_name: &'static str,
}

/// X11 sessions that are leaving systems, and their Wayland pairs.
pub const X11_SESSION_SUCCESSORS: &[SessionSuccessor] = &[SessionSuccessor {
    x11: "plasmax11",
    wayland: "plasma",
    x11_name: "Plasma (X11)",
    wayland_name: "Plasma (Wayland)",
}];

/// Wayland sessions directory (shared by SDDM and Plasma Login Manager).
pub const WAYLAND_SESSIONS_DIR: &str = "/usr/share/wayland-sessions";

/// SDDM's last chosen session (`[Last] Session=`).
pub const SDDM_STATE_FILE: &str = "/var/lib/sddm/state.conf";

/// Session key in a login screen settings group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionKey {
    pub group: &'static str,
    pub key: &'static str,
}

/// A source of login screen settings: `*.conf` directories in reading order,
/// then a file (a later one overrides), and the session keys in them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSource {
    pub dirs: &'static [&'static str],
    pub file: &'static str,
    pub keys: &'static [SessionKey],
}

/// All places where login screens keep the default session.
pub const LOGIN_SESSION_SOURCES: &[SessionSource] = &[
    SessionSource {
        dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D],
        file: "/etc/sddm.conf",
        keys: &[SessionKey { group: "Autologin", key: "Session" }],
    },
    SessionSource { dirs: &[], file: SDDM_STATE_FILE, keys: &[SessionKey { group: "Last", key: "Session" }] },
    SessionSource {
        dirs: &["/usr/lib/plasmalogin/plasmalogin.conf.d", "/etc/plasmalogin.conf.d"],
        file: "/etc/plasmalogin.conf",
        keys: &[
            SessionKey { group: "Autologin", key: "Session" },
            SessionKey { group: "Greeter", key: "PreselectedSession" },
        ],
    },
];

/// The pair for a `Session=` value (`plasmax11`, `plasmax11.desktop`, a full
/// path), by the session file name.
pub fn session_successor(value: &str) -> Option<&'static SessionSuccessor> {
    let name = crate::update_guard::session_file_name(value);
    X11_SESSION_SUCCESSORS.iter().find(|s| s.x11 == name)
}

/// The Wayland pair session file.
pub fn wayland_session_file(s: &SessionSuccessor) -> String {
    format!("{WAYLAND_SESSIONS_DIR}/{}.desktop", s.wayland)
}

/// Source files in reading order (`list` gives the names in a directory).
fn source_files(src: &SessionSource, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for dir in src.dirs {
        out.extend(sddm_conf_read_order(&list(dir)).iter().map(|n| format!("{dir}/{n}")));
    }
    out.push(src.file.to_string());
    out
}

/// Sessions the login screens open or offer by themselves: the resulting
/// value of each key of each source (empty ones dropped).
pub fn login_session_values(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for src in LOGIN_SESSION_SOURCES {
        let texts: Vec<String> = source_files(src, list).iter().filter_map(|p| read(p)).collect();
        for k in src.keys {
            out.extend(crate::login_rescue::ini_value(&texts, k.group, k.key));
        }
    }
    out.retain(|s| !s.trim().is_empty());
    out
}

/// An X11 session from the table that the login screen opens or offers by itself.
pub fn x11_login_default(values: &[String]) -> Option<&'static SessionSuccessor> {
    values.iter().find_map(|v| session_successor(v))
}

/// The new value in the same form as the old one: a full path as a full
/// path, with `.desktop` with it, a bare name as a name.
fn successor_value(old: &str, s: &SessionSuccessor) -> String {
    let old = old.trim();
    if old.contains('/') {
        wayland_session_file(s)
    } else if old.ends_with(".desktop") {
        format!("{}.desktop", s.wayland)
    } else {
        s.wayland.to_string()
    }
}

/// File text where the `keys` keys pointing to `s.x11` are moved to the
/// pair. Only the value of such lines changes: other lines, indents,
/// comments and line endings stay byte for byte. `None`: nothing to change.
pub fn retarget_session(text: &str, keys: &[SessionKey], s: &SessionSuccessor) -> Option<String> {
    rewrite_ini_values(
        text,
        keys,
        &|v| crate::update_guard::session_file_name(v) == s.x11,
        &|v| successor_value(v, s),
    )
}

/// Common edit of login screen settings: for `key=value` lines from `keys`
/// (the group is by the last `[…]` header) whose value matches `hit`, the
/// value is replaced with `new(old)`. Other lines, indents, comments and line
/// endings stay byte for byte. `None`: no line changed.
pub fn rewrite_ini_values(
    text: &str,
    keys: &[SessionKey],
    hit: &dyn Fn(&str) -> bool,
    new: &dyn Fn(&str) -> String,
) -> Option<String> {
    let mut group = String::new();
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let t = body.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            group = name.trim().to_string();
            out.push_str(line);
            continue;
        }
        let matched = !t.starts_with('#')
            && !t.starts_with(';')
            && body.split_once('=').is_some_and(|(k, v)| {
                keys.iter().any(|sk| sk.group == group && sk.key == k.trim()) && hit(v)
            });
        if !matched {
            out.push_str(line);
            continue;
        }
        let eq = body.find('=').unwrap_or(body.len());
        let (head, value) = body.split_at(eq + 1);
        let pad = &value[..value.len() - value.trim_start().len()];
        let replaced = new(value);
        if replaced == value.trim() {
            out.push_str(line);
            continue;
        }
        out.push_str(head);
        out.push_str(pad);
        out.push_str(&replaced);
        out.push_str(&line[body.len()..]);
        changed = true;
    }
    changed.then_some(out)
}

/// A file the edit is allowed to rewrite: settings in `/etc` and SDDM's last
/// session file. Package files (`/usr/lib/…`) are not: the next update would
/// bring them back.
pub fn session_file_writable(path: &str) -> bool {
    path.starts_with("/etc/") || path == SDDM_STATE_FILE
}

/// Edit of one file: the path and the new text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEdit {
    pub path: String,
    pub text: String,
}

/// The "make the pair the default session" decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSwitch {
    /// Rewrite these files: after them the login screen neither opens nor
    /// offers the X11 session by itself.
    Edits(Vec<SessionEdit>),
    /// Nothing to change: the X11 session is not the default anywhere.
    Nothing,
    /// The X11 session is set where Rexon does not edit (a package file): change
    /// nothing.
    Elsewhere,
}

/// What to rewrite so that the login screen stops opening `s.x11` by itself.
/// The result is checked with the same reading as the update watchdog's
/// ([`login_session_values`]) on top of the new texts.
pub fn plan_wayland_default(
    s: &SessionSuccessor,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let mut edits = Vec::new();
    for src in LOGIN_SESSION_SOURCES {
        for path in source_files(src, list) {
            if !session_file_writable(&path) {
                continue;
            }
            if let Some(text) = read(&path).and_then(|t| retarget_session(&t, src.keys, s)) {
                edits.push(SessionEdit { path, text });
            }
        }
    }
    let after = |p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let still = login_session_values(&after, list).iter().any(|v| crate::update_guard::session_file_name(v) == s.x11);
    if still {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

/// SDDM login screen theme: `[Theme] Current=` in the same settings and the
/// same reading order as the session: the package directory, the `/etc`
/// directory, then `/etc/sddm.conf`, which is read LAST and overrides all `*.conf`.
pub const SDDM_THEME_SOURCE: SessionSource = SessionSource {
    dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D],
    file: "/etc/sddm.conf",
    keys: &[SessionKey { group: "Theme", key: "Current" }],
};

/// Our own SDDM theme file: written only when no `/etc` file sets the theme
/// (`zz-`: after `kde_settings.conf` and others in reading order).
pub const SDDM_REXON_THEME_CONF: &str = "/etc/sddm.conf.d/zz-rexon-theme.conf";

/// Files SDDM takes the theme from, in its reading order (the last one wins):
/// the package directory, the `/etc` directory, then `/etc/sddm.conf`.
/// `list` gives the names in a directory. «Appearance» uses the same files
/// to learn the current login screen theme (`installed_scan::LiveScan::active_map`).
pub fn sddm_theme_files(list: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    source_files(&SDDM_THEME_SOURCE, list)
}

/// The theme SDDM shows with such settings (`None`: the built-in one).
pub fn sddm_effective_theme(read: &dyn Fn(&str) -> Option<String>, list: &dyn Fn(&str) -> Vec<String>) -> Option<String> {
    let texts: Vec<String> = source_files(&SDDM_THEME_SOURCE, list).iter().filter_map(|p| read(p)).collect();
    crate::login_rescue::ini_value(&texts, "Theme", "Current")
}

/// What to rewrite so that SDDM shows the theme `theme` (the «Apply» button
/// of an installed login screen theme).
///
/// ⭐ Defect of 30.10.2026: previously the helper wrote only our own
/// [`SDDM_REXON_THEME_CONF`]. But SDDM reads `/etc/sddm.conf` after all the
/// `*.conf`, and a `Current=` in it (as on CachyOS: `Current=breeze`)
/// silently overrode the choice: "Applied", yet the login screen was the old
/// one. Now the value is changed in EVERY `/etc` file where the key already
/// exists (and the KDE settings `kde_settings.conf` show the same theme),
/// and our own file is written only if the result is wrong without it. The
/// result is checked with the same reading as SDDM's. Package files
/// (`/usr/lib/…`) are not edited: `/etc` overrides them.
pub fn plan_sddm_theme(
    theme: &str,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let src = &SDDM_THEME_SOURCE;
    let mut edits: Vec<SessionEdit> = Vec::new();
    for path in source_files(src, list) {
        if !path.starts_with("/etc/") {
            continue;
        }
        if let Some(text) = read(&path).and_then(|t| rewrite_ini_values(&t, src.keys, &|_| true, &|_| theme.to_string())) {
            edits.push(SessionEdit { path, text });
        }
    }
    let effective = |edits: &[SessionEdit]| {
        let after = |p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
        let list_after = |d: &str| {
            let mut names = list(d);
            for e in edits {
                if let Some(n) = e.path.strip_prefix(&format!("{d}/")) {
                    if !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
            names
        };
        sddm_effective_theme(&after, &list_after)
    };
    if effective(&edits).as_deref() != Some(theme) && !edits.iter().any(|e| e.path == SDDM_REXON_THEME_CONF) {
        edits.push(SessionEdit {
            path: SDDM_REXON_THEME_CONF.to_string(),
            text: format!("[Theme]\nCurrent={theme}\n"),
        });
    }
    if effective(&edits).as_deref() != Some(theme) {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

// ── SDDM login screen: theme background and transferring the desktop look ──
//
// Parity with the KDE module «Login Screen (SDDM)» (sddm-kcm 6.8.2:
// `src/sddmkcm.cpp` `save`/`synchronizeSettings`/`resetSyncronizedSettings`,
// `sddmauthhelper.cpp`, `src/themesmodel.cpp`, `src/ui/main.qml`):
// * the theme's «Change background»: the picture is copied into the theme
//   directory, its name is the `[General] background=` key of the
//   `<ConfigFile>.user` file next to the theme settings file (`ConfigFile=`
//   in `metadata.desktop`), plus `type=image`. A theme without a settings
//   file does not accept a background (`supportsBackground`).
// * «Apply Plasma Settings»: the cursor theme and size (`[Theme]
//   CursorTheme/CursorSize`), the font (`[Theme] Font`), NumLock (`[General]
//   Numlock`), DPI (`[X11] ServerArguments=-dpi N`) go into
//   `/etc/sddm.conf.d/kde_settings.conf` (the same key from `/etc/sddm.conf`
//   is removed); the files `kdeglobals` (colours), `plasmarc` (the Plasma
//   theme), `kcminputrc`, `kwinoutputconfig.json` (monitors on Wayland) and
//   fontconfig go into `~sddm/.config`, the `~sddm/.cache` cache is wiped.
//   «Reset» removes the keys and files.
//
// Deliberate differences from the kcm:
// * the picture is stored under our own name [`SDDM_BACKGROUND_STEM`], only
//   PNG or JPEG (by signature); the previous one is deleted only if it is
//   ours (the kcm deletes the file by the key's previous value, even an
//   absolute path outside the theme);
// * «Theme background» restores the theme's own background (the kcm's
//   «Clear Image» sets `type=color`, a solid colour);
// * `-dpi N` is appended to the previous X server arguments instead of
//   replacing them (the kcm wipes `-nolisten tcp` from the package
//   settings); with `forceFontDPI=0` (no scale set) DPI is not touched,
//   while the kcm sends `-dpi 0`;
// * the key is written where SDDM will really read it (like
//   [`plan_sddm_theme`]), the result is checked with the same reading;
// * files in the `sddm` home directory are written as `sddm`, not root
//   (Plasma Login Manager does so since 6.7 after CVE-2026-25710);
// * fontconfig: only `fonts.conf` (as in Plasma Login Manager); the X11
//   monitor layout (`~/.local/share/kscreen`) is not transferred, and is
//   removed on «Reset».

/// Settings written by the KDE module «Login Screen (SDDM)».
pub const SDDM_KDE_SETTINGS_CONF: &str = "/etc/sddm.conf.d/kde_settings.conf";
/// Name of the background picture Rexon puts into the theme directory (the
/// extension by the file signature).
pub const SDDM_BACKGROUND_STEM: &str = "rexon-background";
/// Background picture size limit.
pub const SDDM_BACKGROUND_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Limit of one desktop settings file transferred to the login screen
/// (Plasma Login Manager uses the same 1 MiB per config).
pub const SDDM_LOOK_FILE_MAX_BYTES: u64 = 1024 * 1024;
/// The SDDM greeter's system account.
pub const SDDM_USER: &str = "sddm";
/// Desktop files transferred into `~sddm/.config` (the path is inside
/// `.config` of both the user and `sddm`). The helper accepts no others.
pub const SDDM_LOOK_FILES: &[&str] =
    &["kdeglobals", "plasmarc", "kcminputrc", "kwinoutputconfig.json", "fontconfig/fonts.conf"];
/// What «Reset» removes in the `sddm` home directory (besides the cache): the
/// transferred files and the kscreen monitor layout the kcm put there.
pub const SDDM_LOOK_RESET_PATHS: &[&str] = &[
    ".config/kdeglobals",
    ".config/plasmarc",
    ".config/kcminputrc",
    ".config/kwinoutputconfig.json",
    ".config/fontconfig/fonts.conf",
    ".local/share/kscreen",
];

/// SDDM settings keys touched by the look transfer (as in the kcm).
pub const SDDM_LOOK_KEYS: &[SessionKey] = &[
    SessionKey { group: "Theme", key: "CursorTheme" },
    SessionKey { group: "Theme", key: "CursorSize" },
    SessionKey { group: "Theme", key: "Font" },
    SessionKey { group: "X11", key: "ServerArguments" },
    SessionKey { group: "General", key: "Numlock" },
];

const SDDM_LOOK_SOURCE: SessionSource =
    SessionSource { dirs: &[SDDM_DISTRO_CONF_D, SDDM_CONF_D], file: "/etc/sddm.conf", keys: SDDM_LOOK_KEYS };

/// NumLock at login (SDDM's `[General] Numlock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SddmNumlock {
    On,
    Off,
    None,
}

impl SddmNumlock {
    /// SDDM key value.
    pub fn value(self) -> &'static str {
        match self {
            SddmNumlock::On => "on",
            SddmNumlock::Off => "off",
            SddmNumlock::None => "none",
        }
    }

    /// From Plasma's `kcminputrc [Keyboard] NumLock` (0: turn on, 1: turn off,
    /// 2: leave alone), as the kcm translates it.
    pub fn from_kcminput(v: &str) -> Option<Self> {
        match v.trim() {
            "0" => Some(SddmNumlock::On),
            "1" => Some(SddmNumlock::Off),
            "2" => Some(SddmNumlock::None),
            _ => None,
        }
    }
}

/// Desktop values for the login screen. `None` for the cursor means "remove"
/// (as the kcm does when Plasma has no key); for the font, NumLock and DPI,
/// "leave alone".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SddmLook {
    pub cursor_theme: Option<String>,
    pub cursor_size: Option<u32>,
    pub font: Option<String>,
    pub numlock: Option<SddmNumlock>,
    pub dpi: Option<u32>,
}

/// X server arguments with `-dpi N`: the previous `-dpi …` is removed, the
/// new one appended (`None`: only remove).
pub fn server_args_with_dpi(current: &str, dpi: Option<u32>) -> String {
    let words: Vec<&str> = current.split_whitespace().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if words[i] == "-dpi" {
            i += 2;
            continue;
        }
        out.push(words[i].to_string());
        i += 1;
    }
    if let Some(d) = dpi {
        out.push("-dpi".into());
        out.push(d.to_string());
    }
    out.join(" ")
}

/// What key `k` should become (SDDM's reading result `current`): `None`:
/// leave alone, `Some(None)`: remove from the `/etc` files, `Some(Some(v))`:
/// the value. `look: None` means «Reset».
fn sddm_look_want(k: &SessionKey, look: Option<&SddmLook>, current: Option<&str>) -> Option<Option<String>> {
    let nonempty = |s: String| (!s.trim().is_empty()).then_some(s);
    match (look, k.key) {
        (Some(l), "CursorTheme") => Some(l.cursor_theme.clone()),
        (Some(l), "CursorSize") => Some(l.cursor_size.map(|n| n.to_string())),
        (Some(l), "Font") => l.font.clone().map(Some),
        (Some(l), "Numlock") => l.numlock.map(|n| Some(n.value().to_string())),
        (Some(l), "ServerArguments") => l.dpi.map(|d| nonempty(server_args_with_dpi(current.unwrap_or(""), Some(d)))),
        (None, "ServerArguments") => current
            .filter(|c| c.split_whitespace().any(|w| w == "-dpi"))
            .map(|c| nonempty(server_args_with_dpi(c, None))),
        (None, _) => Some(None),
        _ => None,
    }
}

/// The text without the `key=…` lines from `keys` (the group by the last
/// header). A group that became empty because of this (not a single key,
/// only empty lines and the header) is removed entirely. `None`: there were
/// no such lines.
pub fn drop_ini_keys(text: &str, keys: &[SessionKey]) -> Option<String> {
    // Split into groups: (the header, or empty for the start of the file, the lines).
    let mut groups: Vec<(String, Vec<&str>)> = vec![(String::new(), Vec::new())];
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            groups.push((name.trim().to_string(), vec![line]));
        } else if let Some(g) = groups.last_mut() {
            g.1.push(line);
        }
    }
    let mut changed = false;
    let mut out = String::with_capacity(text.len());
    for (name, lines) in groups {
        let mut dropped = false;
        let kept: Vec<&str> = lines
            .into_iter()
            .filter(|line| {
                let t = line.trim();
                let hit = !t.starts_with('#')
                    && !t.starts_with(';')
                    && !t.starts_with('[')
                    && t.split_once('=').is_some_and(|(k, _)| keys.iter().any(|sk| sk.group == name && sk.key == k.trim()));
                dropped |= hit;
                !hit
            })
            .collect();
        changed |= dropped;
        let empty = kept.iter().all(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with('[')
        });
        if dropped && !name.is_empty() && empty {
            continue;
        }
        kept.iter().for_each(|l| out.push_str(l));
    }
    changed.then_some(out)
}

/// Write `[group] key=value` into the text (`None`: no file): the value is
/// edited in place; a new key goes after the group's last key (before the
/// empty lines preceding the next one); no group: appended at the end.
pub(crate) fn ini_put(existing: Option<&str>, group: &str, key: &str, value: &str) -> String {
    let text = existing.unwrap_or("");
    if text.trim().is_empty() {
        return format!("[{group}]\n{key}={value}\n");
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut cur = String::new();
    let mut last_in_group: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            cur = name.trim().to_string();
            if cur == group {
                last_in_group = Some(i);
            }
            continue;
        }
        if cur != group || t.is_empty() {
            continue;
        }
        if !t.starts_with('#') && !t.starts_with(';') && t.split_once('=').is_some_and(|(k, _)| k.trim() == key) {
            let mut out: String = lines[..i].concat();
            out.push_str(&format!("{key}={value}\n"));
            out.push_str(&lines[i + 1..].concat());
            return out;
        }
        last_in_group = Some(i);
    }
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 4);
    match last_in_group {
        Some(i) => {
            out.push_str(&lines[..=i].concat());
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("{key}={value}\n"));
            out.push_str(&lines[i + 1..].concat());
        }
        None => {
            out.push_str(text);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            if !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&format!("[{group}]\n{key}={value}\n"));
        }
    }
    out
}

/// What to rewrite so that the SDDM login screen gets the desktop values
/// `look` (`None` is «Reset»: the transfer keys are removed, and only `-dpi`
/// leaves the X server arguments). Like [`plan_sddm_theme`]: the `/etc` files
/// where the key is already set are edited; if SDDM's reading result is
/// wrong, [`SDDM_KDE_SETTINGS_CONF`]. Package files are not edited.
/// `Elsewhere`: the value is overridden where Rexon does not edit.
pub fn plan_sddm_look(
    look: Option<&SddmLook>,
    read: &dyn Fn(&str) -> Option<String>,
    list: &dyn Fn(&str) -> Vec<String>,
) -> SessionSwitch {
    let src = &SDDM_LOOK_SOURCE;
    let mut edits: Vec<SessionEdit> = Vec::new();
    let after = |edits: &[SessionEdit], p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let files_after = |edits: &[SessionEdit]| {
        let list_after = |d: &str| {
            let mut names = list(d);
            for e in edits {
                if let Some(n) = e.path.strip_prefix(&format!("{d}/")) {
                    if !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
            names
        };
        source_files(src, &list_after)
    };
    let effective = |edits: &[SessionEdit], k: &SessionKey| {
        let texts: Vec<String> = files_after(edits).iter().filter_map(|p| after(edits, p)).collect();
        crate::login_rescue::ini_value(&texts, k.group, k.key)
    };
    let put = |edits: &mut Vec<SessionEdit>, path: &str, text: String| match edits.iter_mut().find(|e| e.path == path) {
        Some(e) => e.text = text,
        None => edits.push(SessionEdit { path: path.to_string(), text }),
    };
    let mut wanted: Vec<(SessionKey, String)> = Vec::new();
    for k in SDDM_LOOK_KEYS {
        let current = effective(&edits, k);
        let Some(mut want) = sddm_look_want(k, look, current.as_deref()) else { continue };
        // «Reset»: if without the `/etc` lines the key already gives the right
        // thing (`-dpi` is gone, the package value remains), the lines are removed
        // rather than rewritten with a copy of the package value.
        if look.is_none() && want.is_some() {
            let mut trial = edits.clone();
            for path in files_after(&trial).into_iter().filter(|p| p.starts_with("/etc/")) {
                if let Some(n) = after(&trial, &path).and_then(|t| drop_ini_keys(&t, std::slice::from_ref(k))) {
                    put(&mut trial, &path, n);
                }
            }
            if effective(&trial, k) == want {
                want = None;
            }
        }
        for path in files_after(&edits).into_iter().filter(|p| p.starts_with("/etc/")) {
            let Some(text) = after(&edits, &path) else { continue };
            let new = match &want {
                None => drop_ini_keys(&text, std::slice::from_ref(k)),
                Some(v) => rewrite_ini_values(&text, std::slice::from_ref(k), &|_| true, &|_| v.clone()),
            };
            if let Some(n) = new {
                put(&mut edits, &path, n);
            }
        }
        if let Some(v) = want {
            if effective(&edits, k).as_deref() != Some(v.as_str()) {
                let base = after(&edits, SDDM_KDE_SETTINGS_CONF);
                put(&mut edits, SDDM_KDE_SETTINGS_CONF, ini_put(base.as_deref(), k.group, k.key, &v));
            }
            wanted.push((*k, v));
        }
    }
    if wanted.iter().any(|(k, v)| effective(&edits, k).as_deref() != Some(v.as_str())) {
        SessionSwitch::Elsewhere
    } else if edits.is_empty() {
        SessionSwitch::Nothing
    } else {
        SessionSwitch::Edits(edits)
    }
}

/// Name of the SDDM theme settings file (`[SddmGreeterTheme] ConfigFile=` in
/// `metadata.desktop`): only a simple name inside the theme directory.
pub fn sddm_theme_config_name(metadata: &str) -> Option<String> {
    let v = crate::login_rescue::ini_value(&[metadata.to_string()], "SddmGreeterTheme", "ConfigFile")?;
    let ok = !v.is_empty()
        && !v.contains('/')
        && !v.starts_with('.')
        && !v.starts_with('-')
        && !v.chars().any(|c| c.is_control());
    ok.then_some(v)
}

/// SDDM theme background as the kcm sees it (`ThemesModel::add`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SddmBackground {
    /// The theme accepts its own background: it has a settings file.
    pub supported: bool,
    /// Full path of the theme settings file (empty: none).
    pub config: String,
    /// Full path of the picture the theme will show (empty: not set).
    pub image: String,
    /// The background is set on top of the theme (`<ConfigFile>.user`).
    pub custom: bool,
    /// This background was put by Rexon ([`SDDM_BACKGROUND_STEM`]).
    pub ours: bool,
    /// «Show clock» (`showClock` from `.user` on top of the theme file; missing
    /// means yes, as in the kcm's `ThemesModel::add`).
    pub clock: bool,
    /// The theme greeter's Qt ([`sddm_theme_qt_version`]).
    pub qt: u32,
    /// This theme's greeter will show WebP: its Qt has the WebP module
    /// ([`qt_has_webp`]). Filled by the caller: it cannot be learned from the theme files.
    pub webp: bool,
    /// Rexon's own copy ([`kept_login_background`]); empty: none. Filled by the
    /// caller.
    pub kept: String,
    /// The `image` file is readable by this user. Filled by the caller.
    pub readable: bool,
}

/// A flag value as KConfig reads it (`readEntry(…, bool)`): `true`/`on`/
/// `yes`/`1` is yes, `false`/`off`/`no`/`0` is no, anything else is unset.
pub fn kconfig_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// The SDDM theme greeter's Qt: `[SddmGreeterTheme] QtVersion` (missing or
/// not a number means 5, as in SDDM 0.21's `ThemeMetadata`).
pub fn sddm_theme_qt_version(metadata: &str) -> u32 {
    crate::login_rescue::ini_value(&[metadata.to_string()], "SddmGreeterTheme", "QtVersion")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| *v >= 5)
        .unwrap_or(5)
}

/// The background of the theme in `theme_dir` by its files.
pub fn sddm_theme_background(theme_dir: &str, read: &dyn Fn(&str) -> Option<String>) -> SddmBackground {
    let dir = theme_dir.trim_end_matches('/');
    let metadata = read(&format!("{dir}/metadata.desktop"));
    let qt = metadata.as_deref().map(sddm_theme_qt_version).unwrap_or(5);
    let Some(name) = metadata.and_then(|m| sddm_theme_config_name(&m)) else {
        return SddmBackground { qt, ..SddmBackground::default() };
    };
    let config = format!("{dir}/{name}");
    let Some(base) = read(&config) else {
        return SddmBackground { qt, ..SddmBackground::default() };
    };
    let user = read(&format!("{config}.user"));
    let user_bg = user.as_ref().and_then(|u| crate::login_rescue::ini_value(&[u.clone()], "General", "background"));
    // As KConfig with cascading: the `.user` value is stronger than the theme file.
    let clock = user
        .as_ref()
        .and_then(|u| crate::login_rescue::ini_value(&[u.clone()], "General", "showClock"))
        .or_else(|| crate::login_rescue::ini_value(&[base.clone()], "General", "showClock"))
        .and_then(|v| kconfig_bool(&v))
        .unwrap_or(true);
    let bg = user_bg.clone().or_else(|| crate::login_rescue::ini_value(&[base], "General", "background")).unwrap_or_default();
    let image = if bg.is_empty() {
        String::new()
    } else if bg.starts_with('/') {
        bg
    } else {
        format!("{dir}/{bg}")
    };
    let custom = user_bg.as_deref().is_some_and(|b| !b.is_empty());
    let ours = user_bg.as_deref().is_some_and(|b| sddm_background_is_ours(b));
    SddmBackground { supported: true, config, image, custom, ours, clock, qt, ..SddmBackground::default() }
}

/// The file name is a background picture put by Rexon (`rexon-background.png`).
pub fn sddm_background_is_ours(name: &str) -> bool {
    name.strip_prefix(SDDM_BACKGROUND_STEM)
        .and_then(|r| r.strip_prefix('.'))
        .is_some_and(|ext| matches!(ext, "png" | "jpg" | "webp"))
}

/// New `<ConfigFile>.user` text with «Show clock» = `show` (as the kcm: the
/// `showClock` key is written explicitly even for "yes"). `None`: nothing to change.
pub fn sddm_clock_user_text(existing: Option<&str>, show: bool) -> Option<String> {
    let t = ini_put(existing, "General", "showClock", if show { "true" } else { "false" });
    (existing != Some(t.as_str())).then_some(t)
}

/// Where Qt `qt` keeps the WebP reader module (`imageformats/libqwebp.so`):
/// the Arch, Fedora/openSUSE and Debian/Ubuntu layouts.
pub fn qt_webp_plugin_paths(qt: u32) -> Vec<String> {
    let triplet = format!("{}-linux-gnu", std::env::consts::ARCH);
    let dirs: Vec<String> = if qt == 5 {
        vec![
            "/usr/lib/qt/plugins".into(),
            "/usr/lib64/qt5/plugins".into(),
            format!("/usr/lib/{triplet}/qt5/plugins"),
        ]
    } else {
        vec![
            format!("/usr/lib/qt{qt}/plugins"),
            format!("/usr/lib64/qt{qt}/plugins"),
            format!("/usr/lib/{triplet}/qt{qt}/plugins"),
        ]
    };
    dirs.into_iter().map(|d| format!("{d}/imageformats/libqwebp.so")).collect()
}

/// Whether a greeter on Qt `qt` shows a WebP picture: the module exists in
/// at least one of the layouts ([`qt_webp_plugin_paths`]).
pub fn qt_has_webp(qt: u32, exists: &dyn Fn(&str) -> bool) -> bool {
    qt_webp_plugin_paths(qt).iter().any(|p| exists(p))
}

/// [`qt_has_webp`] on the live machine.
pub fn qt_has_webp_live(qt: u32) -> bool {
    qt_has_webp(qt, &|p| std::path::Path::new(p).is_file())
}

/// New `<ConfigFile>.user` text: `image` is the picture name in the theme
/// directory (`background=`, `type=image`); `None` restores the theme
/// background (both keys are removed). `None` in the answer: nothing to change.
pub fn sddm_background_user_text(existing: Option<&str>, image: Option<&str>) -> Option<String> {
    match image {
        Some(name) => {
            let t = ini_put(existing, "General", "background", name);
            let t = ini_put(Some(&t), "General", "type", "image");
            (existing != Some(t.as_str())).then_some(t)
        }
        None => drop_ini_keys(
            existing?,
            &[SessionKey { group: "General", key: "background" }, SessionKey { group: "General", key: "type" }],
        ),
    }
}

/// Picture extension by signature: PNG or JPEG, what an SDDM greeter on Qt
/// shows without extra image modules.
pub fn login_image_ext(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if head.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else {
        None
    }
}

/// WebP signature: `RIFF`, size, `WEBP`.
pub fn is_webp(head: &[u8]) -> bool {
    head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP"
}

/// Like [`login_image_ext`], but `webp`: the greeter shows WebP too (its Qt
/// has the module, [`qt_has_webp`]).
pub fn login_image_ext_with(head: &[u8], webp: bool) -> Option<&'static str> {
    login_image_ext(head).or_else(|| (webp && is_webp(head)).then_some("webp"))
}

/// Refusal before the password: the file is not a picture the login screen
/// will show. `webp`: the greeter's Qt has the WebP module; `qt` is the Qt
/// version (for the hint about which module is missing).
pub fn login_image_refusal(head: &[u8], webp: bool, qt: u32) -> String {
    if is_webp(head) && !webp {
        // TRANSLATORS: %1 is a Qt major version, e.g. "6"; "WebP" and "Qt" are names, keep them as is.
        return trf(
            "The login screen can't show WebP pictures: the WebP image module of Qt %1 isn't installed \
             (package qt%1-imageformats or similar). Pick a PNG or JPEG file.",
            &[&qt.to_string()],
        );
    }
    if webp {
        tr("The login screen shows PNG, JPEG and WebP pictures only — pick another file.")
    } else {
        tr("The login screen shows PNG and JPEG pictures only — pick another file.")
    }
}

// ── Our own copy of the login screen background picture ─────────────────
//
// A picture set on the login screen through Rexon is also kept by Rexon:
// `<data>/rexon/login-background/<kind>.<extension>` plus a `<kind>.json`
// mark (which file was chosen and when). Why: the `plasmalogin` home is
// closed (0750): the window cannot read a copy there, neither for the
// preview nor for resending to the PLM helper; SDDM follows the same rule
// for uniformity. The kind is [`LOGIN_BG_SDDM`] or [`LOGIN_BG_PLM`].
// «Restore default background» removes the copy.

/// Background copy kind: the SDDM login screen.
pub const LOGIN_BG_SDDM: &str = "sddm";
/// Background copy kind: Plasma Login Manager.
pub const LOGIN_BG_PLM: &str = "plm";

/// Copies directory in the user's data root `data_home`.
pub fn login_background_keep_dir(data_home: &str) -> String {
    format!("{}/rexon/login-background", data_home.trim_end_matches('/'))
}

/// Copy mark: the file name in the directory, where the picture came from
/// and when (Unix seconds).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct KeptBackground {
    pub file: String,
    pub source: String,
    pub set_at: u64,
}

/// Text of the `<kind>.json` mark.
pub fn kept_background_marker(kept: &KeptBackground) -> String {
    serde_json::json!({"file": kept.file, "source": kept.source, "set_at": kept.set_at}).to_string() + "\n"
}

/// Copy of kind `kind` in directory `dir`: the picture path and the mark. No
/// mark, a file not ours by name (`<kind>.<png|jpg|webp>`) or missing: `None`.
pub fn kept_login_background(
    dir: &str,
    kind: &str,
    read: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
) -> Option<(String, KeptBackground)> {
    let marker: serde_json::Value = serde_json::from_str(&read(&format!("{dir}/{kind}.json"))?).ok()?;
    let file = marker.get("file")?.as_str()?.to_string();
    let ok = file.strip_prefix(kind).and_then(|r| r.strip_prefix('.')).is_some_and(|e| matches!(e, "png" | "jpg" | "webp"));
    let path = format!("{dir}/{file}");
    if !ok || !exists(&path) {
        return None;
    }
    let kept = KeptBackground {
        file,
        source: marker.get("source").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        set_at: marker.get("set_at").and_then(|v| v.as_u64()).unwrap_or(0),
    };
    Some((path, kept))
}

/// Put a copy of picture `source` (extension `ext` by signature) as kind
/// `kind` into directory `dir` with a mark; previous copies of this kind go.
pub fn keep_login_background_in(dir: &str, kind: &str, source: &str, ext: &str, now: u64) -> std::io::Result<String> {
    drop_login_background_in(dir, kind);
    std::fs::create_dir_all(dir)?;
    let file = format!("{kind}.{ext}");
    let path = format!("{dir}/{file}");
    std::fs::copy(source, &path)?;
    let marker = kept_background_marker(&KeptBackground { file, source: source.to_string(), set_at: now });
    std::fs::write(format!("{dir}/{kind}.json"), marker)?;
    Ok(path)
}

/// Remove the copy of kind `kind` (pictures of all extensions and the mark).
pub fn drop_login_background_in(dir: &str, kind: &str) {
    for name in ["png", "jpg", "webp", "json"].map(|e| format!("{dir}/{kind}.{e}")) {
        let _ = std::fs::remove_file(name);
    }
}

/// Copies directory on the live machine (`$XDG_DATA_HOME` or `~/.local/share`).
pub fn login_background_keep_dir_live() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
    login_background_keep_dir(&crate::utils::data_home(xdg.as_deref(), &home))
}

/// Path of our own copy of kind `kind` on the live machine.
pub fn kept_login_background_live(kind: &str) -> Option<String> {
    let dir = login_background_keep_dir_live();
    kept_login_background(&dir, kind, &|p| std::fs::read_to_string(p).ok(), &|p| std::path::Path::new(p).is_file())
        .map(|(p, _)| p)
}

/// After a successful background change: picture `image` means put a copy (a
/// copy failure does not cancel the change, it is only logged), `None` means
/// remove the copy.
pub fn sync_kept_login_background_live(kind: &str, image: Option<&str>, webp: bool) {
    use std::io::Read;
    let dir = login_background_keep_dir_live();
    let Some(src) = image else {
        drop_login_background_in(&dir, kind);
        return;
    };
    let mut head = [0u8; 16];
    let n = std::fs::File::open(src).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    let Some(ext) = login_image_ext_with(&head[..n], webp) else {
        return;
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Err(e) = keep_login_background_in(&dir, kind, src, ext, now) {
        crate::log_warn!("login", "копия фона экрана входа не сохранилась: {e}");
    }
}

/// Preview and caption of a custom background (set on top of the default or
/// the theme). `ours`: the picture was put by Rexon (`rexon-background.*`);
/// `current`: the path the setting points to; `kept`: Rexon's own copy;
/// `readable`: whether the file can be read. A foreign or unreadable one gets
/// an honest caption without a preview.
pub fn custom_background_preview(
    ours: bool,
    current: &str,
    kept: Option<&str>,
    readable: &dyn Fn(&str) -> bool,
) -> (String, String) {
    if ours {
        if let Some(k) = kept {
            return (k.to_string(), tr("Background: your picture"));
        }
        if readable(current) {
            return (current.to_string(), tr("Background: your picture"));
        }
        return (String::new(), tr("Background: your picture — no preview, Rexon has no copy of it"));
    }
    if readable(current) {
        (current.to_string(), tr("Background: set outside Rexon"))
    } else {
        (String::new(), tr("Background: set outside Rexon — no preview, the file can't be read"))
    }
}

/// Filter of the login screen background file dialog.
pub fn login_image_filter(webp: bool) -> String {
    if webp {
        // TRANSLATORS: file dialog filter; keep the patterns in parentheses as is.
        tr("Images (*.png *.jpg *.jpeg *.webp)")
    } else {
        // TRANSLATORS: file dialog filter; keep the patterns in parentheses as is.
        tr("Images (*.png *.jpg *.jpeg)")
    }
}

/// What from this desktop goes to the login screen: values, files (a name
/// from [`SDDM_LOOK_FILES`] → the user's path) and captions for the person:
/// what is transferred and what is not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopLook {
    pub look: SddmLook,
    pub files: Vec<(String, String)>,
    pub items: Vec<String>,
    pub missing: Vec<String>,
}

/// Cursor and font outside Plasma, as «Appearance» reads them for this
/// desktop (the font as the desktop's string: "Noto Sans 11" or a Qt entry).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtherDesktopLook {
    pub cursor_theme: Option<String>,
    pub cursor_size: Option<u32>,
    pub font: Option<String>,
}

fn look_item_cursor() -> String {
    tr("Cursor theme and size")
}
fn look_item_font() -> String {
    tr("Font")
}
fn look_item_rendering() -> String {
    tr("Font rendering")
}
fn look_item_colors() -> String {
    tr("Color scheme")
}
fn look_item_plasma_theme() -> String {
    tr("Plasma theme")
}
fn look_item_numlock() -> String {
    tr("NumLock preference")
}
fn look_item_dpi() -> String {
    tr("Scaling DPI")
}
fn look_item_screens() -> String {
    tr("Screen configuration (Wayland only)")
}

/// Desktop values and files for the login screen. `plasma`: a Plasma
/// session: then everything as in the kcm (`kcminputrc`, `kdeglobals`,
/// `kcmfonts` in `config_home`); otherwise the cursor and font come from
/// `other`, and of the files only Qt colours (`kdeglobals`) and
/// `fontconfig`, if present. `exists`: whether the user has the file.
pub fn desktop_greeter_look(
    plasma: bool,
    config_home: &str,
    read: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&str) -> bool,
    other: &OtherDesktopLook,
) -> DesktopLook {
    let cfg = config_home.trim_end_matches('/');
    let ini = |file: &str, group: &str, key: &str| {
        read(&format!("{cfg}/{file}")).and_then(|t| crate::themes::ini_key_value(&t, group, key))
    };
    let mut out = DesktopLook::default();
    if plasma {
        out.look.cursor_theme = ini("kcminputrc", "Mouse", "cursorTheme");
        out.look.cursor_size = ini("kcminputrc", "Mouse", "cursorSize").and_then(|v| crate::themes::parse_cursor_size(&v));
        out.look.font = ini("kdeglobals", "General", "font");
        out.look.numlock = ini("kcminputrc", "Keyboard", "NumLock").and_then(|v| SddmNumlock::from_kcminput(&v));
        out.look.dpi = ini("kcmfonts", "General", "forceFontDPI")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|d| (48..=480).contains(d));
    } else {
        out.look.cursor_theme = other.cursor_theme.clone();
        out.look.cursor_size = other.cursor_size;
        out.look.font = other.font.as_deref().and_then(|f| {
            let (family, size, face) = crate::qt_settings::parse_font_spec_face(f);
            (!family.is_empty())
                .then(|| crate::qt_settings::qt_font_spec_face(&family, if size > 0 { size } else { 10 }, face))
        });
    }
    let wanted: &[&str] = if plasma { SDDM_LOOK_FILES } else { &["kdeglobals", "fontconfig/fonts.conf"] };
    for name in wanted {
        let path = format!("{cfg}/{name}");
        if exists(&path) {
            out.files.push((name.to_string(), path));
        }
    }
    let has = |n: &str| out.files.iter().any(|(f, _)| f == n);
    let mut rows: Vec<(String, bool)> = vec![
        (look_item_colors(), has("kdeglobals")),
        (look_item_cursor(), out.look.cursor_theme.is_some()),
        (look_item_font(), out.look.font.is_some()),
        (look_item_rendering(), has("fontconfig/fonts.conf")),
    ];
    if plasma {
        rows.push((look_item_numlock(), out.look.numlock.is_some()));
        rows.push((look_item_plasma_theme(), has("plasmarc")));
        rows.push((look_item_dpi(), out.look.dpi.is_some()));
        rows.push((look_item_screens(), has("kwinoutputconfig.json")));
    } else {
        // Outside Plasma the desktop has this in no form the login screen understands.
        for item in [look_item_plasma_theme(), look_item_numlock(), look_item_dpi(), look_item_screens()] {
            rows.push((item, false));
        }
    }
    for (label, present) in rows {
        if present {
            out.items.push(label);
        } else {
            out.missing.push(label);
        }
    }
    out
}

/// The «Login screen background and look» section with SDDM: data for the window.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmGreeterView {
    /// The login screen is SDDM, the theme is known.
    pub visible: bool,
    pub title: String,
    /// «Theme: Breeze».
    pub theme: String,
    /// Can be changed (not SteamOS and not an immutable system).
    pub can_change: bool,
    /// Why it cannot (in grey, before the password).
    pub blocked: String,
    pub background: SddmBackgroundView,
    pub look: SddmLookView,
    pub clock: GreeterClockView,
    pub preview: GreeterPreviewView,
}

/// The «Show clock» checkbox (for SDDM the theme's `showClock`, for Plasma
/// Login Manager `[Greeter] ShowClock`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreeterClockView {
    pub supported: bool,
    pub checked: bool,
    pub label: String,
}

/// The live login screen preview button (the greeter's test mode).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreeterPreviewView {
    /// Can be shown; otherwise the button is locked and `note` says why.
    pub available: bool,
    pub label: String,
    /// Caption of the same button while the preview is open.
    pub close: String,
    pub tooltip: String,
    pub note: String,
}

/// Lifetime cap of the login screen preview: Rexon does not keep it longer
/// (there may be no way to leave the greeter's test mode by oneself).
pub const LOGIN_PREVIEW_MAX_SECS: u64 = 120;

/// Preview button: `plm` is Plasma Login Manager (exit is a fake login, it
/// has to be named); `problem` is why it cannot be shown.
pub fn greeter_preview_view(plm: bool, problem: Option<String>) -> GreeterPreviewView {
    let tooltip = if plm {
        tr("Shows the login screen full screen without logging you out; nothing is changed. To close it, \
            log in as any user with the password “mypassword” (Plasma Login Manager's test mode), or wait \
            two minutes. Its Sleep, Restart and Shut Down buttons work for real.")
    } else {
        tr("Shows the login screen with this theme full screen without logging you out; nothing is \
            changed. It closes by itself after two minutes, or switch back to Rexon (Alt+Tab) and click \
            “Close Preview”.")
    };
    GreeterPreviewView {
        available: problem.is_none(),
        label: tr("Preview"),
        close: tr("Close Preview"),
        tooltip,
        note: problem.unwrap_or_default(),
    }
}

/// The «Show clock» checkbox.
pub fn greeter_clock_view(supported: bool, checked: bool) -> GreeterClockView {
    GreeterClockView { supported, checked, label: tr("Show clock") }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmBackgroundView {
    pub supported: bool,
    /// Picture for the preview (`file:` is the window's business); empty: not set.
    pub image: String,
    /// Caption under the preview: "custom" or "theme background".
    pub state: String,
    /// Why the background cannot be changed (a theme without a settings file).
    pub note: String,
    pub choose: String,
    /// «Theme background» only when a custom one is set.
    pub reset: String,
    pub can_reset: bool,
    /// File dialog filter.
    pub filter: String,
    pub dialog_title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SddmLookView {
    pub apply: String,
    pub reset: String,
    pub confirm_title: String,
    pub confirm: String,
    pub reset_confirm: String,
}

/// The section decision from facts: `theme` is the SDDM theme (`None`: the
/// built-in one), `name` its title from `metadata.desktop`, `background` is
/// [`sddm_theme_background`], `look` is [`desktop_greeter_look`], `blocked`
/// is the reason system files are not changed (empty: allowed).
pub fn sddm_greeter_view(
    theme: Option<&str>,
    name: Option<&str>,
    background: &SddmBackground,
    look: &DesktopLook,
    plasma: bool,
    blocked: &str,
) -> SddmGreeterView {
    let Some(theme) = theme.filter(|t| !t.trim().is_empty()) else {
        return SddmGreeterView::default();
    };
    let label = name.filter(|n| !n.trim().is_empty()).unwrap_or(theme);
    let (image, state) = if !background.supported {
        (background.image.clone(), String::new())
    } else if background.custom {
        let kept = (!background.kept.is_empty()).then_some(background.kept.as_str());
        custom_background_preview(background.ours, &background.image, kept, &|_| background.readable)
    } else {
        (background.image.clone(), tr("Background: the theme's own"))
    };
    let list = |items: &[String]| items.iter().map(|i| format!("• {i}")).collect::<Vec<_>>().join("\n");
    let mut confirm = if plasma {
        tr("The login screen will reflect your customizations to the following Plasma settings:")
    } else {
        tr("The login screen will reflect these settings of your desktop:")
    };
    confirm.push('\n');
    confirm.push_str(&list(&look.items));
    if !look.missing.is_empty() {
        confirm.push_str("\n\n");
        confirm.push_str(&if plasma {
            tr("Not set on this desktop, so the login screen keeps its own:")
        } else {
            tr("Not taken from this desktop — the login screen keeps its own:")
        });
        confirm.push('\n');
        confirm.push_str(&list(&look.missing));
    }
    SddmGreeterView {
        visible: true,
        title: tr("Login screen background and look"),
        // TRANSLATORS: %1 is the name of a login screen theme, e.g. "Breeze".
        theme: trf("Theme: %1", &[&label]),
        can_change: blocked.is_empty(),
        blocked: blocked.to_string(),
        background: SddmBackgroundView {
            supported: background.supported,
            image,
            state,
            note: if background.supported {
                String::new()
            } else {
                tr("This theme doesn't let you change its background.")
            },
            choose: tr("Change Background…"),
            reset: tr("Use the Theme's Background"),
            can_reset: background.custom,
            filter: login_image_filter(background.webp),
            dialog_title: tr("Choose a Login Screen Background"),
        },
        clock: greeter_clock_view(background.supported, background.clock),
        preview: greeter_preview_view(false, None),
        look: SddmLookView {
            apply: if plasma { tr("Apply Plasma Settings…") } else { tr("Apply Desktop Settings…") },
            reset: tr("Reset to Default Settings…"),
            confirm_title: if plasma { tr("Apply Plasma Settings") } else { tr("Apply Desktop Settings") },
            confirm,
            reset_confirm: tr(
                "The login screen will go back to its own cursor, font, colors and other settings. \
                 Its theme and background stay as they are.",
            ),
        },
    }
}

/// The system font of desktop `de` as its storage string (outside Plasma).
fn live_desktop_font(de: &str, config_home: &str) -> Option<String> {
    use crate::qt_settings as qs;
    let raw = match qs::font_plan(de, "system").primary {
        qs::FontStore::Cmd(backend) => {
            let argv = qs::sound_read_argv(&backend, "kreadconfig6")?;
            let report = crate::utils::run(
                &crate::utils::RunSpec {
                    argv,
                    output: crate::utils::RunOutput::Capture,
                    timeout_secs: Some(5),
                    lang_c: true,
                    ..Default::default()
                },
                None,
            );
            if report.returncode != Some(0) {
                return None;
            }
            crate::utils::decode_text(&report.stdout)
        }
        qs::FontStore::File(file) => {
            let text = std::fs::read_to_string(format!("{}/{}", config_home.trim_end_matches('/'), file.rel_path())).ok()?;
            file.read_spec(&text)
        }
    };
    let v = raw.trim().trim_matches(['\'', '"']).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// [`desktop_greeter_look`] on the live machine for desktop `de` (`kde` is Plasma).
pub fn desktop_greeter_look_live(de: &str) -> DesktopLook {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    let cfg = crate::utils::config_home(xdg.as_deref(), &home);
    let plasma = de == "kde";
    let other = if plasma {
        OtherDesktopLook::default()
    } else {
        let cursor = |size| crate::appearance_installer::live_cursor_value(de, &home, xdg.as_deref(), size);
        OtherDesktopLook {
            cursor_theme: cursor(false),
            cursor_size: cursor(true).and_then(|v| crate::themes::parse_cursor_size(&v)),
            font: live_desktop_font(de, &cfg),
        }
    };
    desktop_greeter_look(
        plasma,
        &cfg,
        &|p| std::fs::read_to_string(p).ok(),
        &|p| std::path::Path::new(p).is_file(),
        &other,
    )
}

/// The theme SDDM shows now (`None`: the built-in one), with the same reading
/// as SDDM itself.
pub fn sddm_current_theme_live() -> Option<String> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let list = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    };
    sddm_effective_theme(&read, &list).filter(|t| !t.trim().is_empty())
}

/// [`sddm_greeter_view`] on the live machine: the SDDM theme with the same
/// reading as SDDM itself; on SteamOS it cannot be changed (the core's reason).
pub fn sddm_greeter_view_live(de: &str, look: &DesktopLook) -> SddmGreeterView {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let theme = sddm_current_theme_live();
    let dir = theme.as_deref().map(|t| format!("{SDDM_THEMES_DIR}/{t}")).unwrap_or_default();
    let name = theme.as_ref().and_then(|_| read(&format!("{dir}/metadata.desktop"))).and_then(|m| {
        crate::login_rescue::ini_value(&[m], "SddmGreeterTheme", "Name")
    });
    let mut background = if theme.is_some() { sddm_theme_background(&dir, &read) } else { SddmBackground::default() };
    background.webp = qt_has_webp_live(background.qt);
    background.readable = std::fs::File::open(&background.image).is_ok();
    if background.ours {
        background.kept = kept_login_background_live(LOGIN_BG_SDDM).unwrap_or_default();
    }
    let blocked = if crate::distro_compat::running_on_steamos() {
        crate::distro_compat::SteamOsBlock::LoginScreen.reason()
    } else {
        String::new()
    };
    let mut view = sddm_greeter_view(theme.as_deref(), name.as_deref(), &background, look, de == "kde", &blocked);
    if let Some(t) = theme.as_deref() {
        view.preview = greeter_preview_view(false, sddm_preview_argv_live(t).err());
    }
    view
}

/// Result of a privilege session operation for the person: `Ok(done)` on
/// code 0, otherwise the helper's ready refusal text or `failed`.
fn greeter_outcome(out: Option<crate::system_exec::RunOut>, done: String, failed: String) -> Result<String, String> {
    let Some(out) = out else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(done);
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or(failed))
}

/// Change the background of SDDM theme `theme` to picture `image` (`None`:
/// restore the theme background) through the window's privilege session
/// ([`crate::privilege::op::PrivOp::SddmBackground`]). Before the password,
/// the picture is checked by its first bytes (`head`): a certain refusal is
/// not worth a password. Everything else the root helper checks itself;
/// there is no raw path via `pkexec` (Z5).
/// `webp`/`qt`: whether the theme's greeter shows WebP and which Qt it is on
/// ([`qt_has_webp`]).
pub fn sddm_background_with(
    theme: &str,
    image: Option<&str>,
    webp: bool,
    qt: u32,
    head: &dyn Fn(&str) -> Option<Vec<u8>>,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    use crate::privilege::op::{LocalFile, PrivOp};
    if let Some(path) = image {
        let bytes = head(path).unwrap_or_default();
        if login_image_ext_with(&bytes, webp).is_none() {
            return Err(login_image_refusal(&bytes, webp, qt));
        }
    }
    let op = PrivOp::SddmBackground {
        theme: theme.to_string(),
        image: image.map(|p| LocalFile { slot: 0, label: p.to_string() }),
    };
    let files: Vec<String> = image.map(|p| vec![p.to_string()]).unwrap_or_default();
    let done = if image.is_some() {
        tr("Done. The new background will show the next time the login screen appears.")
    } else {
        tr("Done. The login screen shows the theme's own background again.")
    };
    greeter_outcome(run(&op, &files), done, tr("Couldn't change the login screen background."))
}

/// [`sddm_background_with`] on the live machine.
pub fn sddm_background_live(theme: &str, image: Option<&str>) -> Result<String, String> {
    use std::io::Read;
    let qt = std::fs::read_to_string(format!("{SDDM_THEMES_DIR}/{theme}/metadata.desktop"))
        .map(|m| sddm_theme_qt_version(&m))
        .unwrap_or(5);
    let webp = qt_has_webp_live(qt);
    let done = sddm_background_with(
        theme,
        image,
        webp,
        qt,
        &|p| {
            let mut buf = [0u8; 16];
            let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut buf)).ok()?;
            Some(buf[..n].to_vec())
        },
        &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}),
    )?;
    sync_kept_login_background_live(LOGIN_BG_SDDM, image, webp);
    Ok(done)
}

/// «Show clock» for SDDM theme `theme` (`showClock` in `<ConfigFile>.user`,
/// as the kcm) through the window's privilege session
/// ([`crate::privilege::op::PrivOp::SddmClock`]).
pub fn sddm_clock_with(
    theme: &str,
    show: bool,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    let op = crate::privilege::op::PrivOp::SddmClock { theme: theme.to_string(), show };
    let done = if show {
        tr("Done. The login screen will show the clock.")
    } else {
        tr("Done. The login screen won't show the clock.")
    };
    greeter_outcome(run(&op, &[]), done, tr("Couldn't change the login screen clock."))
}

/// [`sddm_clock_with`] on the live machine.
pub fn sddm_clock_live(theme: &str, show: bool) -> Result<String, String> {
    sddm_clock_with(theme, show, &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}))
}

// ── Live login screen preview ────────────────────────────────────────────
//
// Checked against the SDDM 0.21 sources (`src/greeter/GreeterApp.cpp`,
// `src/daemon/Greeter.cpp`, `src/common/ThemeMetadata.cpp`):
//   * the theme greeter is `sddm-greeter` for Qt 5 themes and
//     `sddm-greeter-qt6` for `[SddmGreeterTheme] QtVersion=6` (no key means
//     5); it lives next to `sddm` (`BIN_INSTALL_DIR`);
//   * test mode is `--test-mode --theme <theme dir>`: no daemon and no
//     login, as a regular user, a fullscreen window on every monitor; the
//     theme reads `theme.conf` and `theme.conf.user` itself;
//   * a closed window does not end the process
//     (`setQuitOnLastWindowClosed(false)`), so Rexon holds and kills the
//     preview ([`run_greeter_preview`]).
// The KDE 6.8 module (sddm-kcm) has no preview button: this goes beyond it.

/// Directory where SDDM keeps its programs.
pub const SDDM_BIN_DIR: &str = "/usr/bin";

/// SDDM greeter for themes on Qt `qt` (like `Greeter::greeterPathForQt`).
pub fn sddm_greeter_bin(qt: u32) -> String {
    if qt == 5 {
        format!("{SDDM_BIN_DIR}/sddm-greeter")
    } else {
        format!("{SDDM_BIN_DIR}/sddm-greeter-qt{qt}")
    }
}

/// The theme name is a simple directory name.
fn plain_theme_name(theme: &str) -> bool {
    !theme.is_empty()
        && theme != "."
        && theme != ".."
        && !theme.contains('/')
        && !theme.starts_with('-')
        && !theme.chars().any(|c| c.is_control())
}

/// Preview command of SDDM theme `theme` from [`SDDM_THEMES_DIR`]. `read` is
/// the file text, `executable` whether the program can be run. `Err`: why it
/// cannot be shown (for the person).
pub fn sddm_preview_argv(
    theme: &str,
    read: &dyn Fn(&str) -> Option<String>,
    executable: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let missing = || tr("This login screen theme isn't installed in the system folder, so there's nothing to preview.");
    if !plain_theme_name(theme) {
        return Err(missing());
    }
    let dir = format!("{SDDM_THEMES_DIR}/{theme}");
    let Some(meta) = read(&format!("{dir}/metadata.desktop")) else {
        return Err(missing());
    };
    let bin = sddm_greeter_bin(sddm_theme_qt_version(&meta));
    if !executable(&bin) {
        // TRANSLATORS: %1 is a program path, e.g. "/usr/bin/sddm-greeter".
        return Err(trf("Can't preview this theme: it needs %1, which isn't installed.", &[&bin]));
    }
    Ok(vec![bin, "--test-mode".into(), "--theme".into(), dir])
}

/// Whether file `path` can be run (a regular file with the exec bit).
pub fn is_executable_live(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// [`sddm_preview_argv`] on the live machine.
pub fn sddm_preview_argv_live(theme: &str) -> Result<Vec<String>, String> {
    sddm_preview_argv(theme, &|p| std::fs::read_to_string(p).ok(), &is_executable_live)
}

/// The preview failed; `lines` are the last lines the program said.
pub fn login_preview_failed(lines: &[String]) -> String {
    let head = tr("Couldn't show the login screen preview.");
    // A line longer than 300 characters is cut: the person needs the reason, not a dump.
    let tail: Vec<String> = lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| if l.chars().count() > 300 { format!("{}…", l.chars().take(300).collect::<String>()) } else { l.to_string() })
        .collect();
    if tail.is_empty() {
        head
    } else {
        format!("{head}\n{}", tail[tail.len().saturating_sub(5)..].join("\n"))
    }
}

/// Show the login screen preview `argv` and wait for the end: it closed by
/// itself, `stop` was raised (the «Close preview» button) or `max` passed.
/// If Rexon dies, the preview dies with it (`PR_SET_PDEATHSIG`; so call it
/// from a thread that lives until the end of the show). The program's output
/// is read all the time (otherwise a chatty greeter stalls on a full pipe),
/// the last lines go into the refusal. Being closed by us is not an error.
pub fn run_greeter_preview(argv: &[String], stop: &std::sync::atomic::AtomicBool, max: std::time::Duration) -> Result<(), String> {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    use std::sync::atomic::Ordering;
    let Some((bin, args)) = argv.split_first() else {
        return Err(login_preview_failed(&[]));
    };
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    // SAFETY: between fork and exec only the async-signal-safe prctl.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| login_preview_failed(&[e.to_string()]))?;
    let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::<String>::new()));
    let drain = child.stderr.take().map(|err| {
        let tail = tail.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                if let Ok(mut t) = tail.lock() {
                    if t.len() >= 20 {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            }
        })
    });
    let started = std::time::Instant::now();
    let mut ours = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) => {}
            Err(_) => break None,
        }
        if !ours && (stop.load(Ordering::SeqCst) || started.elapsed() >= max) {
            ours = true;
            // SAFETY: the process is our not yet reaped child, the pid is not taken by another.
            unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
            let soft = std::time::Instant::now();
            while soft.elapsed() < std::time::Duration::from_secs(3) {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    };
    if let Some(d) = drain {
        let _ = d.join();
    }
    if ours || status.is_some_and(|s| s.success()) {
        return Ok(());
    }
    let lines: Vec<String> = tail.lock().map(|t| t.iter().cloned().collect()).unwrap_or_default();
    Err(login_preview_failed(&lines))
}

/// Login screen preview command by the button's sign: `sddm:<theme>` is an
/// SDDM theme from a tile; empty means what the login screen will show now
/// (Plasma Login Manager or the current SDDM theme).
pub fn login_preview_argv_live(token: &str) -> Result<Vec<String>, String> {
    if let Some(theme) = token.strip_prefix("sddm:") {
        return sddm_preview_argv_live(theme);
    }
    if is_plasma_login(&crate::appearance_installer::active_login_manager()) {
        return crate::plasma_login::preview_argv(&is_executable_live);
    }
    match sddm_current_theme_live() {
        Some(theme) => sddm_preview_argv_live(&theme),
        None => Err(tr("SDDM uses its built-in theme here, so there's no theme to preview.")),
    }
}

/// Transfer the desktop look to the SDDM login screen (`look: Some`) or
/// «Reset» (`None`) through the window's privilege session
/// ([`crate::privilege::op::PrivOp::SddmLook`]). A file larger than
/// [`SDDM_LOOK_FILE_MAX_BYTES`] is not sent (`size` is its size).
pub fn sddm_look_with(
    look: Option<&DesktopLook>,
    size: &dyn Fn(&str) -> Option<u64>,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    use crate::privilege::op::{LocalFile, LookFile, PrivOp};
    let (op, files, done) = match look {
        Some(l) => {
            let sent: Vec<&(String, String)> = l
                .files
                .iter()
                .filter(|(_, p)| size(p).is_some_and(|s| s <= SDDM_LOOK_FILE_MAX_BYTES))
                .collect();
            let files: Vec<LookFile> = sent
                .iter()
                .enumerate()
                .map(|(i, (name, path))| LookFile { source: LocalFile { slot: i as u8, label: path.clone() }, name: name.clone() })
                .collect();
            let paths = sent.iter().map(|(_, p)| p.clone()).collect();
            (
                PrivOp::SddmLook { look: Some(l.look.clone()), files },
                paths,
                tr("Done. The login screen will use these settings the next time it appears."),
            )
        }
        None => (
            PrivOp::SddmLook { look: None, files: Vec::new() },
            Vec::new(),
            tr("Done. The login screen settings are back to default."),
        ),
    };
    greeter_outcome(run(&op, &files), done, tr("Couldn't change the login screen settings."))
}

/// [`sddm_look_with`] on the live machine.
pub fn sddm_look_live(look: Option<&DesktopLook>) -> Result<String, String> {
    sddm_look_with(
        look,
        &|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()),
        &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}),
    )
}

/// [`x11_login_default`] on the live machine, by what the window can read
/// (SDDM's last session is closed to the user; the root helper checks it
/// itself). A pair is offered only if its session is installed.
pub fn x11_login_default_live() -> Option<&'static SessionSuccessor> {
    let read = |p: &str| std::fs::read_to_string(p).ok();
    let list = |d: &str| -> Vec<String> {
        std::fs::read_dir(d)
            .map(|it| it.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    };
    x11_login_default(&login_session_values(&read, &list))
        .filter(|s| std::path::Path::new(&wayland_session_file(s)).is_file())
}

/// Start of the refusal key "the pair is not in the system" (+ the X11 session name).
pub const WAYLAND_SESSION_MISSING: &str = "refused.wayland-session-missing.";
/// Start of the refusal key "the session is set in the package file" (+ the X11 session name).
pub const SESSION_SET_ELSEWHERE: &str = "refused.session-set-elsewhere.";

// TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
const T_WAYLAND_DEFAULT_BUTTON: &str = n_("Make %1 the default session");
const T_WAYLAND_DEFAULT_CONFIRM: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen will start %2 instead of %1 — for automatic sign-in and as the session it offers first. \
     Only this setting changes: automatic sign-in stays on or off as it is now, and you can still pick another \
     session on the login screen.",
);
const T_WAYLAND_DEFAULT_BANNER: &str = n_(
    // TRANSLATORS: %1 and %2 are session names as shown on the login screen, e.g. "Plasma (X11)" and "Plasma (Wayland)".
    "The login screen starts %1 by default. This session is no longer supported and may stop working after an \
     update. Make %2 the default instead.",
);
// TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
const T_WAYLAND_DEFAULT_DONE: &str = n_("%1 is now the default session on the login screen.");
const T_WAYLAND_SESSION_MISSING: &str =
    // TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (Wayland)".
    n_("Rexon didn't change the login screen: %1 isn't installed. Nothing was changed.");
const T_SESSION_SET_ELSEWHERE: &str = n_(
    // TRANSLATORS: %1 is a session name as shown on the login screen, e.g. "Plasma (X11)".
    "Rexon didn't change the login screen: %1 is set as the default in a file of the system itself, which \
     Rexon doesn't edit. Nothing was changed.",
);
const T_WAYLAND_DEFAULT_FAILED: &str = n_("Rexon couldn't change the default session. Nothing was changed.");

/// "Default session" lines as templates (for checking translations).
pub const WAYLAND_DEFAULT_TEXTS: &[&str] = &[
    T_WAYLAND_DEFAULT_BUTTON,
    T_WAYLAND_DEFAULT_CONFIRM,
    T_WAYLAND_DEFAULT_BANNER,
    T_WAYLAND_DEFAULT_DONE,
    T_WAYLAND_SESSION_MISSING,
    T_SESSION_SET_ELSEWHERE,
    T_WAYLAND_DEFAULT_FAILED,
];

/// Button and confirmation dialog title.
pub fn wayland_default_button(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_BUTTON, &[&s.wayland_name])
}

/// Confirmation text.
pub fn wayland_default_confirm(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_CONFIRM, &[&s.x11_name, &s.wayland_name])
}

/// Banner in the login screen section.
pub fn wayland_default_banner(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_BANNER, &[&s.x11_name, &s.wayland_name])
}

/// Result: the pair became the default session.
pub fn wayland_default_done(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_DEFAULT_DONE, &[&s.wayland_name])
}

/// Helper refusal: the pair is not in the system.
pub fn wayland_session_missing_text(s: &SessionSuccessor) -> String {
    trf(T_WAYLAND_SESSION_MISSING, &[&s.wayland_name])
}

/// Helper refusal: the X11 session is set in the package file.
pub fn session_set_elsewhere_text(s: &SessionSuccessor) -> String {
    trf(T_SESSION_SET_ELSEWHERE, &[&s.x11_name])
}

/// General failure (the helper did not answer, writing failed).
pub fn wayland_default_failed() -> String {
    tr(T_WAYLAND_DEFAULT_FAILED)
}

/// The "default session is going away" banner for the login screen section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaylandDefaultOffer {
    /// X11 session name from the table: the window passes it back to
    /// `makeWaylandDefaultSession` (the helper checks it against the table itself).
    pub x11: String,
    pub text: String,
    pub button: String,
    pub confirm: String,
}

/// Banner captions for pair `s`.
pub fn wayland_default_offer(s: &SessionSuccessor) -> WaylandDefaultOffer {
    WaylandDefaultOffer {
        x11: s.x11.to_string(),
        text: wayland_default_banner(s),
        button: wayland_default_button(s),
        confirm: wayland_default_confirm(s),
    }
}

/// Make the pair the default session through the window's privilege session
/// ([`crate::privilege`]): that exactly the session key changes, that the
/// pair is installed and that after the edit the login screen does not open
/// the X11 session by itself is checked by the root helper; it does not
/// trust the window. There is deliberately no raw path via `pkexec`. `Ok` is
/// the result for the person, `Err` a ready refusal text.
pub fn make_wayland_default_with(
    s: &SessionSuccessor,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<String, String> {
    let op = crate::privilege::op::PrivOp::LoginSessionWayland { x11: s.x11.to_string() };
    let Some(out) = run(&op) else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(wayland_default_done(s));
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(wayland_default_failed))
}

/// [`make_wayland_default_with`] on the live machine.
pub fn make_wayland_default_live(s: &SessionSuccessor) -> Result<String, String> {
    make_wayland_default_with(s, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut |_| {}))
}

/// Idempotent answer of `reset_sddm_to_default` when there is nothing to delete.
pub fn sddm_already_stock_message() -> String {
    tr("The login screen (SDDM) already uses the stock configuration")
}
/// Text that replaces the empty output of a failed pkexec.
pub fn sddm_reset_failed_message() -> String {
    tr("Couldn't reset SDDM")
}
/// `safe_run` did not find pkexec: the command did not run at all.
///
/// ⭐ Fixed 15.09.2026: it used to say "pkexec not found": a technical word
/// and nothing about what to do. The package names in the second line are
/// not words but what has to be installed: on Arch, Fedora and openSUSE the
/// password prompt program is in the polkit package, on Debian and Ubuntu in
/// a separate pkexec.
pub fn pkexec_missing_message() -> String {
    tr("Couldn't ask for the administrator password: a component needed for \
        that is missing on this system. Nothing was changed.\n\
        Install the polkit package (pkexec on Debian and Ubuntu) and try again.")
}

/// A polkit authorization refusal is recognised the same way in all recovery
/// operations (Plasma forks, Plymouth, SDDM): either the agent text or code 126.
pub fn pkexec_auth_denied(returncode: i32, err: &str) -> bool {
    err.contains("Not authorized") || returncode == 126
}

/// What to show the person when pkexec returned NON-zero. Shared by all
/// recovery operations (Plymouth, SDDM): choose the stream, trim whitespace,
/// and replace an authorization refusal with a "run it by hand" hint.
///
/// Port of `(r.stderr or r.stdout or "<fallback text>").strip()`: an empty
/// string is falsy in Python, so an empty stderr gives way to stdout, and
/// both empty give way to the operation's fallback text.
pub fn pkexec_failure_message(
    returncode: i32,
    stdout: &str,
    stderr: &str,
    fallback: &str,
    shell: &str,
) -> String {
    let raw = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        fallback
    };
    let err = raw.trim().to_string();
    if pkexec_auth_denied(returncode, &err) {
        pkexec_denied_message(&format!("sudo {shell}"))
    } else {
        err
    }
}

/// Result of `reset_sddm_to_default`: `(ok, message, removed)`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SddmResetOutcome {
    pub ok: bool,
    pub message: String,
    pub removed: Vec<String>,
}

/// Parsing the pkexec result for `reset_sddm_to_default`. Nothing external
/// here: Python passes the exit code and both streams, the decision and the
/// text are here.
///
/// `returncode` = `None` when pkexec was not found (`safe_run` returned
/// `None`). `shell` is the command body from [`SddmReset::Run`], it also goes
/// into the "run it by hand" hint.
pub fn sddm_reset_outcome(
    removed: &[String],
    default_theme: Option<&str>,
    shell: &str,
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> SddmResetOutcome {
    let Some(code) = returncode else {
        return SddmResetOutcome {
            ok: false,
            message: pkexec_missing_message(),
            removed: Vec::new(),
        };
    };
    if code == 0 {
        return SddmResetOutcome {
            ok: true,
            message: sddm_reset_success_message(removed, default_theme),
            removed: removed.to_vec(),
        };
    }
    SddmResetOutcome {
        ok: false,
        message: pkexec_failure_message(
            code,
            stdout,
            stderr,
            &sddm_reset_failed_message(),
            shell,
        ),
        removed: Vec::new(),
    }
}

/// pkexec authorization refusal text: the same for all recovery operations
/// (Plasma forks/plymouth/SDDM): "not confirmed, it can be done by hand
/// <command>".
///
/// ⭐ Fixed 15.09.2026: it used to say "pkexec authorization rejected.":
/// technical words for the user and the wrong meaning: code 126 also comes
/// when the person closed the password window themselves, nobody "rejected"
/// anything. The first line is the same as for the install and removal
/// refusals (`install_plan`, `remove_plan`, `batch_main_result`). The
/// terminal command stays as is: it is not words but what has to be typed.
pub fn pkexec_denied_message(manual_cmd: &str) -> String {
    trf(
        "The administrator password wasn't confirmed.\nYou can do this by hand in a terminal:\n%1",
        &[&manual_cmd],
    )
}

// ============================================================================
// Login screen detection: port of `active_login_manager`/`_display_manager_present`
// ============================================================================

/// The standard systemd symlink of the active DM.
pub const DM_SYMLINK: &str = "/etc/systemd/system/display-manager.service";
/// A WIDE list of known DM units: port of `_KNOWN_DM_UNITS`. Needed for an
/// honest "is there a login screen" answer (the `ensure_display_manager`
/// foolproof guard: install SDDM ONLY when a DM is really missing).
pub const KNOWN_DM_UNITS: &[&str] = &[
    "display-manager", "plasmalogin", "sddm", "gdm", "gdm3", "lightdm",
    "lxdm", "greetd", "ly", "cosmic-greeter", "slim", "nodm", "xdm",
    "entrance", "emptty", "ddm", "tbsm",
];
/// Query order in `active_login_manager` (Plasma Login Manager first: its
/// early releases had `Provides: None`, the symlink might not exist at all;
/// on a live machine on 03.09.2026 the unit already carries
/// `Alias=display-manager.service`).
pub const LOGIN_MANAGER_PROBE_UNITS: &[&str] =
    &["plasmalogin", "sddm", "gdm", "lightdm", "cosmic-greeter"];
/// `systemctl is-enabled` states meaning "starts at boot".
pub const DM_ENABLED_STATES: &[&str] =
    &["enabled", "enabled-runtime", "alias", "indirect", "static"];
/// Default of `active_login_manager` when there is nothing to determine it with.
pub const DEFAULT_LOGIN_MANAGER: &str = "sddm";
/// Plasma Login Manager unit (Plasma 6.6+): a login screen WITHOUT themes: it
/// has one look, built into the binary, and it does not load SDDM QML themes.
pub const PLASMA_LOGIN_UNIT: &str = "plasmalogin";

/// Whether Plasma Login Manager draws the login screen, by the
/// [`active_login_manager_from`] answer. One decision for the product: it is
/// asked by the «Appearance» section (which shelf to show), by the active
/// SDDM theme mark, and by the last refusal before installing an SDDM theme.
pub fn is_plasma_login(active_unit: &str) -> bool {
    active_unit.trim() == PLASMA_LOGIN_UNIT
}

/// The unit from the display-manager.service symlink target: the basename
/// without `.service`. None: no target or an empty name. Target liveness is
/// not checked here: that is `dm_symlink_unit`'s job.
pub fn dm_unit_from_symlink(realpath_target: Option<&str>) -> Option<String> {
    let target = realpath_target?;
    let base = target.rsplit('/').next().unwrap_or(target);
    let base = base.strip_suffix(".service").unwrap_or(base);
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

/// Whether `systemctl is-enabled` output means "starts at boot".
pub fn dm_state_is_enabled(state: &str) -> bool {
    DM_ENABLED_STATES.contains(&state.trim())
}

/// The unit display-manager.service points to with a LIVE symlink.
///
/// `symlink_target` is the symlink realpath (None = no symlink),
/// `target_exists` is whether the unit file exists at that path.
///
/// ⚠️ Fixed in the port. A dangling symlink is the normal state after the
/// login screen package is removed: `systemctl enable sddm` puts a symlink
/// into /etc, and `pacman -R sddm` (including our `remove_de` for KDE and
/// LXQt, where sddm is in the package list) does not touch it. Python treated
/// such a symlink as a live DM (`islink or exists`): `_display_manager_present`
/// answered "present", the `ensure_display_manager` foolproof guard skipped
/// the install, and after a reboot the person was left without a graphical
/// login: exactly the trouble the guard exists for. A dangling symlink now
/// means "no symlink": the decision goes to the `systemctl is-enabled` query.
pub fn dm_symlink_unit(symlink_target: Option<&str>, target_exists: bool) -> Option<String> {
    if !target_exists {
        return None;
    }
    dm_unit_from_symlink(symlink_target)
}

/// Port of `active_login_manager`. `is_enabled(unit)` is the stdout of
/// `systemctl is-enabled <unit>.service` or None (the command gave no
/// answer); called LAZILY and only until the first enabled unit is found.
pub fn active_login_manager_from(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> String {
    login_manager_by(
        symlink_target,
        target_exists,
        has_systemctl,
        LOGIN_MANAGER_PROBE_UNITS.iter().copied(),
        &dm_state_is_enabled,
        is_enabled,
    )
    .unwrap_or_else(|| DEFAULT_LOGIN_MANAGER.to_string())
}

/// `systemctl is-enabled` states in which a login screen unit REALLY starts
/// at boot as display-manager.service. Narrower than [`DM_ENABLED_STATES`]:
/// `static` and `indirect` for a DM unit do not mean it will take the place
/// of display-manager.service, and they are not fit for the "default"
/// caption (the wide list stays for an honest "there is a login screen").
pub const DM_ACTIVE_STATES: &[&str] = &["enabled", "enabled-runtime", "alias"];

/// Whether `systemctl is-enabled` output means the unit is the default login screen.
pub fn dm_state_is_active(state: &str) -> bool {
    DM_ACTIVE_STATES.contains(&state.trim())
}

/// Query order for [`active_login_manager_opt`]: first the narrow list of
/// the old detection (the same priority), then the other known units:
/// otherwise an enabled `gdm3` on Debian without a symlink would stay "not found".
pub fn active_probe_units() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = LOGIN_MANAGER_PROBE_UNITS.to_vec();
    for unit in KNOWN_DM_UNITS {
        if *unit != "display-manager" && !out.contains(unit) {
            out.push(unit);
        }
    }
    out
}

/// Common core of active login screen detection: a live symlink, then a lazy
/// query of `units` until the first unit whose state `accept` accepts.
fn login_manager_by<'a>(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    units: impl IntoIterator<Item = &'a str>,
    accept: &dyn Fn(&str) -> bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    if let Some(unit) = dm_symlink_unit(symlink_target, target_exists) {
        return Some(unit);
    }
    if has_systemctl {
        for unit in units {
            if let Some(state) = is_enabled(unit) {
                if accept(&state) {
                    return Some(unit.to_string());
                }
            }
        }
    }
    None
}

/// The active login screen WITHOUT substitution: `None` means nothing to
/// determine it with (no live symlink and no known unit enabled, or no systemctl).
///
/// The "default now" caption needs exactly this answer: the old
/// [`active_login_manager_from`] answers `sddm` in that case, and the person
/// would see someone else's login screen as theirs. The old one stays for old
/// consumers (choosing the theme shelf, etc.) and behaves as before.
///
/// Differences from the old one: only [`DM_ACTIVE_STATES`] count as enabled,
/// and the wide list [`active_probe_units`] is queried.
pub fn active_login_manager_opt(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    login_manager_by(
        symlink_target,
        target_exists,
        has_systemctl,
        active_probe_units(),
        &dm_state_is_active,
        is_enabled,
    )
}

/// A login screen greeter the person chooses between: for LightDM it is the
/// file `/usr/share/xgreeters/<id>.desktop`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DmGreeter {
    /// File name without `.desktop`: what is written into `greeter-session=`.
    pub id: String,
    /// `Name=` from the file (empty: not set).
    pub name: String,
    /// The first word of `Exec=`: the greeter program.
    pub exec: String,
}

/// Whether a login screen is ready to become the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum DmReadiness {
    /// Everything needed is in place.
    Ready,
    /// It will work, but first the person chooses a greeter from the installed ones.
    NeedsGreeterChoice(Vec<DmGreeter>),
    /// Not possible: the reason for the person.
    Blocked(String),
    /// Rexon cannot switch this login screen (or this system).
    Unsupported,
}

/// One installed login screen: a row of the future list in the «Login
/// screen» section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginManagerInfo {
    /// Unit without `.service` (`plasmalogin`, `sddm`…).
    pub unit: String,
    /// Human name; empty: an unknown login screen.
    pub label: String,
    /// Program from the unit's `ExecStart` (as written in the unit).
    pub binary: Option<String>,
    /// The unit was found and its program is in place.
    pub present: bool,
    /// Default: display-manager.service points to it.
    pub active: bool,
    pub readiness: DmReadiness,
    /// Warnings that do not prevent switching (for the person).
    pub warnings: Vec<String>,
}

/// Port of `_display_manager_present`: an HONEST boolean "there is a login screen".
///
/// Without systemd (`has_systemctl == false`): true: auto-enable is
/// impossible there, and init itself manages the login screen, we must not
/// interfere (otherwise we would add a second DM).
///
/// The display-manager.service symlink counts ONLY with a live target (see
/// `dm_symlink_unit`): a dangling one after `pacman -R sddm` means "no login
/// screen", and the answer comes from querying the units.
pub fn display_manager_present_from(
    symlink_target: Option<&str>,
    target_exists: bool,
    has_systemctl: bool,
    is_enabled: &dyn Fn(&str) -> Option<String>,
) -> bool {
    if symlink_target.is_some() && target_exists {
        return true;
    }
    if !has_systemctl {
        return true;
    }
    for unit in KNOWN_DM_UNITS {
        if let Some(state) = is_enabled(unit) {
            if dm_state_is_enabled(&state) {
                return true;
            }
        }
    }
    false
}

// ── Installed login screens: units with Alias=display-manager.service ────

/// systemd unit directories in priority order: a file from `/etc` overrides
/// the package one with the same name. `/lib/systemd/system` is for old
/// Debian without the `/usr` merge (where the merge exists, it is a link and
/// same-named files are filtered out).
pub const DM_UNIT_DIRS: &[&str] = &[
    "/etc/systemd/system",
    "/usr/local/lib/systemd/system",
    "/usr/lib/systemd/system",
    "/lib/systemd/system",
];

/// The alias systemd starts the login screen by.
pub const DM_ALIAS: &str = "display-manager.service";

/// Unit name without `.service` by file path; `None`: not a `.service`.
pub fn unit_name_from_path(path: &str) -> Option<&str> {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(".service").filter(|n| !n.is_empty())
}

/// Whether a unit with this name can be a login screen in the list: the
/// alias itself, `@` templates and the obsolete `display-manager-legacy`
/// (openSUSE) cannot.
pub fn dm_unit_name_listable(name: &str) -> bool {
    !(name == "display-manager"
        || name.starts_with("display-manager-")
        || name.ends_with("-legacy")
        || name.contains('@'))
}

/// Values of a unit file section key with systemd rules: the key may appear
/// several times, the value is a space-separated list, an empty assignment
/// resets what was accumulated.
pub fn unit_values(text: &str, section: &str, key: &str) -> Vec<String> {
    let mut here = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') || s.starts_with(';') {
            continue;
        }
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == section;
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        if v.is_empty() {
            out.clear();
        } else {
            out.extend(v.split_whitespace().map(String::from));
        }
    }
    out
}

/// The unit's `ExecStart` as words, without systemd prefixes (`-`, `@`, `:`,
/// `+`, `!`) and quotes. `None`: no line. The last non-empty one wins.
pub fn unit_exec_start(text: &str) -> Option<Vec<String>> {
    let mut here = false;
    let mut found: Option<String> = None;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = &s[1..s.len() - 1] == "Service";
            continue;
        }
        if !here {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == "ExecStart" {
                let v = v.trim();
                found = (!v.is_empty()).then(|| v.to_string());
            }
        }
    }
    let line = found?;
    let line = line.trim_start_matches(['-', '@', ':', '+', '!']);
    let words: Vec<String> = line
        .split_whitespace()
        .map(|w| w.trim_matches(['"', '\'']).to_string())
        .filter(|w| !w.is_empty())
        .collect();
    (!words.is_empty()).then_some(words)
}

/// Installed login screens by unit files: those with
/// `Alias=display-manager.service` in `[Install]`. This also catches login
/// screens that are not in our tables.
///
/// `files` is `(path, text)` in directory priority order ([`DM_UNIT_DIRS`]):
/// among same-named ones the FIRST decides, as in systemd (an own file in
/// `/etc` overrides the package one). The alias, templates and `-legacy` are
/// filtered out. The answer order is the [`KNOWN_DM_UNITS`] order, unknown
/// ones after them alphabetically.
pub fn dm_units_from_unit_files(files: &[(String, String)]) -> Vec<String> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for (path, text) in files {
        let Some(name) = unit_name_from_path(path) else { continue };
        if seen.contains(&name) {
            continue;
        }
        seen.push(name);
        if !dm_unit_name_listable(name) {
            continue;
        }
        if unit_values(text, "Install", "Alias").iter().any(|a| a == DM_ALIAS) {
            out.push(name.to_string());
        }
    }
    let rank = |u: &str| KNOWN_DM_UNITS.iter().position(|k| *k == u).unwrap_or(usize::MAX);
    out.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    out
}

/// What a login screen needs for a working login: one row per unit. The
/// table is shared by the caption, the readiness checks and (later) the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DmSpec {
    /// Unit without `.service`.
    pub unit: &'static str,
    /// Human name (in Latin script: it is a program name).
    pub label: &'static str,
    /// Login screen program name (the directory differs between distributions:
    /// the path is taken from the unit's `ExecStart`).
    pub binary: &'static str,
    /// Package (the Arch name; Debian has its own names: `gdm3` as a separate row).
    pub package: &'static str,
    /// System user the greeter runs as; `None`: there is no own user or it is
    /// set by the config (greetd).
    pub user: Option<&'static str>,
    /// PAM service file (`/etc/pam.d` or `/usr/lib/pam.d`).
    pub pam: &'static str,
    /// The greeter always runs on X: it will not come up without Xorg. For SDDM
    /// it depends on `DisplayServer=`: the readiness check decides.
    pub needs_xorg: bool,
    /// Rexon can check readiness and offer «Make default». The others are only
    /// shown in the list.
    pub switchable: bool,
}

/// The login screen table. The order is the [`KNOWN_DM_UNITS`] order (without
/// the alias and tbsm, which has no graphical login of its own).
pub const LOGIN_MANAGER_SPECS: &[DmSpec] = &[
    DmSpec {
        unit: PLASMA_LOGIN_UNIT, label: "Plasma Login Manager", binary: "plasmalogin",
        package: "plasma-login-manager", user: Some("plasmalogin"), pam: "plasmalogin",
        needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "sddm", label: "SDDM", binary: "sddm", package: "sddm",
        user: Some("sddm"), pam: "sddm", needs_xorg: true, switchable: true,
    },
    DmSpec {
        unit: "gdm", label: "GDM", binary: "gdm", package: "gdm",
        user: Some("gdm"), pam: "gdm-password", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "gdm3", label: "GDM", binary: "gdm3", package: "gdm3",
        user: Some("Debian-gdm"), pam: "gdm-password", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "lightdm", label: "LightDM", binary: "lightdm", package: "lightdm",
        user: Some("lightdm"), pam: "lightdm", needs_xorg: true, switchable: true,
    },
    DmSpec {
        unit: "lxdm", label: "LXDM", binary: "lxdm", package: "lxdm",
        user: None, pam: "lxdm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "greetd", label: "greetd", binary: "greetd", package: "greetd",
        user: None, pam: "greetd", needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "ly", label: "Ly", binary: "ly", package: "ly",
        user: None, pam: "ly", needs_xorg: false, switchable: false,
    },
    DmSpec {
        unit: "cosmic-greeter", label: "COSMIC Greeter", binary: "cosmic-greeter",
        package: "cosmic-greeter", user: Some("cosmic-greeter"), pam: "greetd",
        needs_xorg: false, switchable: true,
    },
    DmSpec {
        unit: "slim", label: "SLiM", binary: "slim", package: "slim",
        user: None, pam: "slim", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "nodm", label: "nodm", binary: "nodm", package: "nodm",
        user: None, pam: "nodm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "xdm", label: "XDM", binary: "xdm", package: "xorg-xdm",
        user: None, pam: "xdm", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "entrance", label: "Entrance", binary: "entrance", package: "entrance",
        user: None, pam: "entrance", needs_xorg: true, switchable: false,
    },
    DmSpec {
        unit: "emptty", label: "emptty", binary: "emptty", package: "emptty",
        user: None, pam: "emptty", needs_xorg: false, switchable: false,
    },
    // Deepin's own login screen (an SDDM fork, the ddm 0.4 package): the login
    // window is drawn by Treeland itself (`treeland.service` as user `dde`), and
    // the `dde-seatd` seat service is mandatory next to it (the unit's
    // `Requires=`: checked by readiness, [`DmFacts::missing_units`]). Without it
    // Treeland 0.11 does not bring up the session (treeland#1656), see
    // [`DESKTOP_DM_RULES`]. Whether it starts other desktops was not checked on
    // a test bench: [`dm_warnings`] and the confirmation dialog say so honestly.
    DmSpec {
        unit: DDM_UNIT, label: "DDM", binary: "ddm", package: "ddm",
        user: Some("dde"), pam: "ddm", needs_xorg: false, switchable: true,
    },
];

/// The unit of Deepin's own login screen.
pub const DDM_UNIT: &str = "ddm";

// ── Which login screen which desktop needs ────────────────────────────────
//
// There is one login screen per computer, and it starts before the person
// chooses a desktop. Most desktops do not care which one it is; TWO
// desktops are tied to their own by name and lose the main thing without it
// (research of 15.11.2026):
//   * GNOME → GDM: gnome-shell enables the lock screen only if GDM answered
//     over D-Bus (`org.gnome.DisplayManager`); under another login screen
//     there is no locking at all: no Super+L, no idle lock, no lock before sleep;
//   * Deepin on Treeland → DDM: Treeland 0.11 creates the person's session
//     only on DDM messages (treeland#1656); under another login screen one
//     sees the background without a panel.
// KDE Plasma, Cinnamon, MATE and Xfce work the same under Plasma Login /
// SDDM / LightDM, and under GDM lose «Switch user» (GDM does not provide
// `org.freedesktop.DisplayManager`), Plasma also loses the KWallet login
// (GDM's PAM has no `pam_kwallet5`).

/// What a desktop requires from the login screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DmNeed {
    /// Exactly this login screen is needed (the units are its variants:
    /// `gdm`/`gdm3`); under another one the desktop loses the main thing.
    Required(&'static [&'static str]),
    /// Any will do, but under these login screens some features are lost.
    Avoid(&'static [&'static str]),
    /// Any will do.
    Any,
}

/// How bad it is for a desktop under a login screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DmHarm {
    /// Some features are lost (switching users, KWallet).
    Minor,
    /// The desktop has no screen lock or does not start at all.
    Critical,
}

/// A row of the "desktop → login screen" table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopDmRule {
    /// Desktop in the core database (as in `system::de_installed_binaries`).
    pub de_id: &'static str,
    /// Desktop name for the person (the product name, in Latin script).
    pub label: &'static str,
    /// The desktop is installed if ALL these programs are in place. For Deepin
    /// it is Treeland itself: regular Deepin on X11 (without Treeland) does not
    /// need DDM.
    pub binaries: &'static [&'static str],
    pub need: DmNeed,
    /// What is lost under the "wrong" login screen (an English catalog key,
    /// translated when shown); `%1` is the login screen name.
    pub harm_text: &'static str,
}

const GDM_UNITS: &[&str] = &["gdm", "gdm3"];
const DDM_UNITS: &[&str] = &[DDM_UNIT];

/// The requirements table. Desktops not listed here work under any login
/// screen ([`desktop_dm_need`] → [`DmNeed::Any`]).
pub const DESKTOP_DM_RULES: &[DesktopDmRule] = &[
    DesktopDmRule {
        de_id: "gnome", label: "GNOME", binaries: &["gnome-shell"], need: DmNeed::Required(GDM_UNITS),
        // TRANSLATORS: %1 is a login screen name (SDDM, Plasma Login Manager, …). Super+L is a key combination.
        harm_text: n_("With %1 as the login screen, GNOME can't lock the screen: Super+L, locking when idle and before sleep do nothing. GNOME needs GDM."),
    },
    DesktopDmRule {
        de_id: "deepin", label: "Deepin", binaries: &["treeland"], need: DmNeed::Required(DDM_UNITS),
        // TRANSLATORS: %1 is a login screen name. Treeland and DDM are program names.
        harm_text: n_("With %1 as the login screen, Deepin doesn't start: Treeland shows only the wallpaper, without the panel. Deepin needs its own login screen, DDM."),
    },
    DesktopDmRule {
        de_id: "kde", label: "KDE Plasma", binaries: &["plasmashell"], need: DmNeed::Avoid(GDM_UNITS),
        // TRANSLATORS: %1 is a login screen name (GDM). KWallet is a program name.
        harm_text: n_("With %1 as the login screen, KDE Plasma asks for the KWallet password separately after sign-in, and “Switch User” may not work."),
    },
    DesktopDmRule {
        de_id: "cinnamon-mint", label: "Cinnamon", binaries: &["cinnamon"], need: DmNeed::Avoid(GDM_UNITS),
        // TRANSLATORS: %1 is a login screen name (GDM).
        harm_text: n_("With %1 as the login screen, “Switch User” doesn't work in Cinnamon."),
    },
    DesktopDmRule {
        de_id: "mate", label: "MATE", binaries: &["mate-session"], need: DmNeed::Avoid(GDM_UNITS),
        // TRANSLATORS: %1 is a login screen name (GDM).
        harm_text: n_("With %1 as the login screen, “Switch User” doesn't work in MATE."),
    },
    DesktopDmRule {
        de_id: "xfce", label: "Xfce", binaries: &["xfce4-session"], need: DmNeed::Avoid(GDM_UNITS),
        // TRANSLATORS: %1 is a login screen name (GDM).
        harm_text: n_("With %1 as the login screen, “Switch User” doesn't work in Xfce."),
    },
];

/// Table row by desktop; `None`: the desktop does not care.
pub fn desktop_dm_rule(de_id: &str) -> Option<&'static DesktopDmRule> {
    let de_id = de_id.trim();
    DESKTOP_DM_RULES.iter().find(|r| r.de_id == de_id)
}

/// What a desktop requires from the login screen.
pub fn desktop_dm_need(de_id: &str) -> DmNeed {
    desktop_dm_rule(de_id).map_or(DmNeed::Any, |r| r.need)
}

/// How bad it is for a desktop under login screen `dm_unit`; `None`: it
/// loses nothing.
pub fn desktop_dm_harm(de_id: &str, dm_unit: &str) -> Option<DmHarm> {
    let dm = dm_unit.trim();
    match desktop_dm_need(de_id) {
        DmNeed::Required(units) if !units.contains(&dm) => Some(DmHarm::Critical),
        DmNeed::Avoid(units) if units.contains(&dm) => Some(DmHarm::Minor),
        _ => None,
    }
}

/// One "desktop vs login screen" compatibility problem for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DmCompatIssue {
    pub de_id: String,
    pub harm: DmHarm,
    pub text: String,
}

/// What is bad about login screen `dm_unit` for the installed desktops
/// `installed` (ids from the core database): first what the desktop cannot
/// work without, then minor things; inside, the table order. Decision only,
/// no system access.
pub fn dm_compat_issues(dm_unit: &str, installed: &[String]) -> Vec<DmCompatIssue> {
    let dm = dm_unit.trim();
    if dm.is_empty() {
        return Vec::new();
    }
    let name = dm_label(dm);
    let mut out: Vec<DmCompatIssue> = DESKTOP_DM_RULES
        .iter()
        .filter(|r| installed.iter().any(|d| d == r.de_id))
        .filter_map(|r| {
            let harm = desktop_dm_harm(r.de_id, dm)?;
            Some(DmCompatIssue { de_id: r.de_id.to_string(), harm, text: trf(r.harm_text, &[&name]) })
        })
        .collect();
    out.sort_by(|a, b| b.harm.cmp(&a.harm));
    out
}

/// DDM with other desktops has not been checked on a test bench yet: a note
/// under its row in the «Login manager» section and in the confirmation dialog.
pub fn ddm_untested_warning() -> String {
    tr("DDM is Deepin's own login screen. Rexon hasn't checked yet whether it starts other desktops. \
        If yours doesn't start, sign in to Deepin and make the previous login screen the default here again.")
}

/// Table row by unit; `None`: an unknown login screen.
pub fn login_manager_spec(unit: &str) -> Option<&'static DmSpec> {
    let unit = unit.trim();
    LOGIN_MANAGER_SPECS.iter().find(|s| s.unit == unit)
}

// ── How the login screen is chosen on this system ──────────────────────────

/// The login screen selection mechanism of the system: it decides whether
/// Rexon can offer «Make default» and which steps will be needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "init", rename_all = "snake_case")]
pub enum DmSwitchBackend {
    /// Only the display-manager.service link (Arch, Fedora, etc.).
    Systemd,
    /// systemd + `/etc/X11/default-display-manager` and debconf (Debian,
    /// Ubuntu, Mint): the file and the link must match.
    SystemdDebian,
    /// openSUSE: `display-manager.service` is a regular file, selection via
    /// `update-alternatives default-displaymanager`.
    SystemdSuseAlternatives,
    /// NixOS: the login screen is set in configuration.nix.
    NixosDeclarative,
    /// Without systemd: the login screen is managed by its own init (the name
    /// for the person; empty: not recognised).
    Unsupported(String),
}

impl DmSwitchBackend {
    /// Whether Rexon offers the «Make default» button. The first version
    /// (Wehrwolfmann's decision of 20.10.2026): only systemd systems of the Arch
    /// family, Fedora and Debian/Ubuntu; openSUSE after testing on a bench.
    pub fn switch_supported(&self) -> bool {
        matches!(self, DmSwitchBackend::Systemd | DmSwitchBackend::SystemdDebian)
    }
}

/// System probes for [`dm_switch_backend`]: gathered by the glue code, read only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmBackendProbe {
    /// systemd is PID 1: `/run/systemd/system` exists (systemd itself checks it
    /// this way). `systemctl` in PATH alone is not enough: on Artix it may exist
    /// without systemd.
    pub systemd_running: bool,
    /// `systemctl` is present.
    pub has_systemctl: bool,
    /// `/etc/X11/default-display-manager` is present.
    pub x11_default_dm_file: bool,
    /// `display-manager.service` in `/etc/systemd/system` or
    /// `/usr/lib/systemd/system` is a regular file, not a link (openSUSE).
    pub dm_service_is_regular_file: bool,
    /// The `default-displaymanager` alternative exists (`/etc/alternatives`).
    pub suse_dm_alternative: bool,
    /// A recognised init without systemd (`OpenRC`, `runit`, `s6`, `dinit`,
    /// `SysVinit`); empty: not recognised.
    pub other_init: String,
}

/// Login screen selection mechanism by distribution family and probes.
pub fn dm_switch_backend(family: PkgFamily, probe: &DmBackendProbe) -> DmSwitchBackend {
    if family == PkgFamily::Nix {
        return DmSwitchBackend::NixosDeclarative;
    }
    if !(probe.systemd_running && probe.has_systemctl) {
        return DmSwitchBackend::Unsupported(probe.other_init.clone());
    }
    match family {
        PkgFamily::Suse if probe.dm_service_is_regular_file && probe.suse_dm_alternative => {
            DmSwitchBackend::SystemdSuseAlternatives
        }
        PkgFamily::Debian if probe.x11_default_dm_file => DmSwitchBackend::SystemdDebian,
        _ => DmSwitchBackend::Systemd,
    }
}

/// Name of a non-systemd init by the signs found (order: from precise to
/// general). `has(path)` is whether a path exists, `which(name)` whether a program does.
pub fn other_init_name(has: &dyn Fn(&str) -> bool, which: &dyn Fn(&str) -> bool) -> String {
    let name = if has("/run/openrc") || which("openrc") || which("rc-update") {
        "OpenRC"
    } else if has("/run/runit") || has("/etc/runit") || which("runit") {
        "runit"
    } else if has("/run/s6-rc") || which("s6-rc") {
        "s6"
    } else if which("dinitctl") {
        "dinit"
    } else if has("/etc/inittab") {
        "SysVinit"
    } else {
        ""
    };
    name.to_string()
}

/// The command for manual switching on a system without systemd: shown
/// instead of the button. `None`: there is no ready command for this init.
/// These are not words for translation but what is typed in a terminal.
pub fn dm_manual_switch_command(init: &str, family: PkgFamily, unit: &str) -> Option<String> {
    let dm = unit.trim();
    if dm.is_empty() || !dm.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return None;
    }
    Some(match (init, family) {
        ("OpenRC", _) => format!(
            "sudo sed -i 's/^DISPLAYMANAGER=.*/DISPLAYMANAGER=\"{dm}\"/' /etc/conf.d/display-manager && sudo rc-update add display-manager default"
        ),
        ("runit", PkgFamily::Void) => format!("sudo ln -s /etc/sv/{dm} /var/service/"),
        ("runit", _) => format!("sudo ln -s /etc/runit/sv/{dm} /run/runit/service/"),
        ("dinit", _) => format!("sudo dinitctl enable {dm}"),
        ("SysVinit", PkgFamily::Debian) => format!("sudo dpkg-reconfigure {dm}"),
        _ => return None,
    })
}

/// Whether a unit file is worth reading for the login screen list: a known
/// name or the text mentions the alias. A decision for the glue code, so as
/// not to keep hundreds of foreign units in memory.
pub fn dm_unit_file_candidate(name: &str, text: &str) -> bool {
    dm_unit_name_listable(name) && (KNOWN_DM_UNITS.contains(&name) || text.contains(DM_ALIAS))
}

// ── Readiness: LightDM and its greeters ────────────────────────────────────

/// LightDM greeter directory: `<id>.desktop`, `greeter-session=<id>`.
pub const XGREETERS_DIR: &str = "/usr/share/xgreeters";
/// The built-in default of a LightDM built without distribution patches.
pub const LIGHTDM_UPSTREAM_DEFAULT_GREETER: &str = "example-gtk-gnome";
/// The greeter Rexon suggests first when the person has to choose: Slick,
/// which is the default in Linux Mint.
pub const LIGHTDM_SUGGESTED_GREETER: &str = "lightdm-slick-greeter";
/// Its program, in case the file is named differently.
const LIGHTDM_SUGGESTED_GREETER_EXEC: &str = "slick-greeter";
/// The Debian alternative name (`update-alternatives lightdm-greeter`): a
/// link to one of the real greeters, not shown as a separate item.
const LIGHTDM_DEBIAN_ALT_GREETER: &str = "lightdm-greeter";

/// `greeter-session` from the `lightdm --show-config` output (the `[Seat:*]`
/// section, old name `[SeatDefaults]`). Lines there look like
/// `A  key=value`: the letter marks the source file. `None`: not set.
pub fn lightdm_greeter_from_show_config(text: &str) -> Option<String> {
    let mut here = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            here = matches!(&s[1..s.len() - 1], "Seat:*" | "SeatDefaults");
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.split_whitespace().last() == Some("greeter-session") {
            let v = v.trim();
            found = (!v.is_empty()).then(|| v.to_string());
        }
    }
    found
}

/// The greeter LightDM takes when `greeter-session` is not set: built in at
/// build time and different between distributions. `None`: not confirmed
/// (then the person chooses).
pub fn lightdm_default_greeter(family: PkgFamily) -> Option<&'static str> {
    match family {
        PkgFamily::Arch => Some("lightdm-gtk-greeter"),
        PkgFamily::Debian => Some(LIGHTDM_DEBIAN_ALT_GREETER),
        PkgFamily::Gentoo | PkgFamily::Void | PkgFamily::Alpine => {
            Some(LIGHTDM_UPSTREAM_DEFAULT_GREETER)
        }
        _ => None,
    }
}

/// Installed LightDM greeters by the `XGREETERS_DIR/*.desktop` files:
/// `(path, text)`. Only those whose `Exec=` program exists are taken
/// (`exec_exists`); `Hidden=true` is filtered out. The Debian alternative
/// `lightdm-greeter` with the same program as a real one is not shown a
/// second time. Order by `id`.
pub fn xgreeters_from(
    files: &[(String, String)],
    exec_exists: &dyn Fn(&str) -> bool,
) -> Vec<DmGreeter> {
    let value = |text: &str, key: &str| {
        crate::de_apply::ini_value(text, "Desktop Entry", key).unwrap_or_default()
    };
    let mut all: Vec<DmGreeter> = Vec::new();
    for (path, text) in files {
        let base = path.rsplit('/').next().unwrap_or(path);
        let Some(id) = base.strip_suffix(".desktop").filter(|s| !s.is_empty()) else {
            continue;
        };
        if value(text, "Hidden").eq_ignore_ascii_case("true") {
            continue;
        }
        let exec = value(text, "Exec")
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if exec.is_empty() || !exec_exists(&exec) {
            continue;
        }
        all.push(DmGreeter { id: id.to_string(), name: value(text, "Name"), exec });
    }
    // The alternative goes last, so that on a program match the real file stays.
    all.sort_by(|a, b| {
        (a.id == LIGHTDM_DEBIAN_ALT_GREETER)
            .cmp(&(b.id == LIGHTDM_DEBIAN_ALT_GREETER))
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut out: Vec<DmGreeter> = Vec::new();
    for g in all {
        if g.id == LIGHTDM_DEBIAN_ALT_GREETER && out.iter().any(|o| o.exec == g.exec) {
            continue;
        }
        out.push(g);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Preselection in the greeter list: Slick if installed, otherwise the first.
pub fn suggested_greeter(greeters: &[DmGreeter]) -> Option<&DmGreeter> {
    greeters
        .iter()
        .find(|g| g.id == LIGHTDM_SUGGESTED_GREETER || g.exec == LIGHTDM_SUGGESTED_GREETER_EXEC)
        .or_else(|| greeters.first())
}

/// LightDM readiness by greeter.
///
/// * set (`greeter-session`) and installed: `Ready`;
/// * not set, the distribution default is installed and it is the only one:
///   `Ready`;
/// * set but not installed; not set and the default not confirmed; or
///   several greeters: `NeedsGreeterChoice` (preselection:
///   [`suggested_greeter`]): otherwise LightDM fails on a nonexistent greeter
///   or shows something other than the person expects;
/// * no greeters at all: `Blocked`.
pub fn lightdm_greeter_readiness(
    greeter_session: Option<&str>,
    default_greeter: Option<&str>,
    greeters: &[DmGreeter],
) -> DmReadiness {
    if greeters.is_empty() {
        return DmReadiness::Blocked(tr(
            "LightDM has no greeter installed, so it can't show a login screen.",
        ));
    }
    let installed = |id: &str| greeters.iter().any(|g| g.id == id);
    match greeter_session.map(str::trim).filter(|s| !s.is_empty()) {
        Some(set) if installed(set) => DmReadiness::Ready,
        Some(_) => DmReadiness::NeedsGreeterChoice(greeters.to_vec()),
        None => match default_greeter {
            Some(d) if installed(d) && greeters.len() == 1 => DmReadiness::Ready,
            _ => DmReadiness::NeedsGreeterChoice(greeters.to_vec()),
        },
    }
}

// ── Readiness: greetd ──────────────────────────────────────────────────────

/// greetd config if the unit did not pass its own (`--config`).
pub const GREETD_DEFAULT_CONFIG: &str = "/etc/greetd/config.toml";
/// greetd greeters that give only a text login (login into a shell, not
/// into a desktop): the built-in `agreety` and bare shells.
pub const GREETD_TEXT_GREETERS: &[&str] =
    &["agreety", "login", "sh", "bash", "dash", "zsh", "fish"];

/// What greetd runs as the greeter (`[default_session] command`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "program", rename_all = "snake_case")]
pub enum GreetdCommand {
    /// A graphical or TUI greeter that starts a desktop (`tuigreet`, `regreet`,
    /// `gtkgreet`, `nwg-hello`, wrappers like `cage`): the program is the first
    /// word of the command.
    Graphical(String),
    /// A text login straight into a shell (`agreety`).
    Text(String),
    /// No command.
    Missing,
}

/// greetd config path from the `ExecStart` words (`--config X`, `-c X`,
/// `--config=X`); otherwise [`GREETD_DEFAULT_CONFIG`].
pub fn greetd_config_path(exec_start: &[String]) -> String {
    let mut it = exec_start.iter().skip(1);
    while let Some(w) = it.next() {
        if let Some(v) = w.strip_prefix("--config=") {
            return v.to_string();
        }
        if w == "--config" || w == "-c" {
            if let Some(v) = it.next() {
                return v.clone();
            }
        }
    }
    GREETD_DEFAULT_CONFIG.to_string()
}

/// String value of a key in a section of greetd's TOML file (the simple form
/// `key = "value"`, enough for its configs).
pub fn toml_string_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut here = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if s.starts_with('[') && s.ends_with(']') {
            here = s[1..s.len() - 1].trim() == section;
            continue;
        }
        if !here {
            continue;
        }
        let Some((k, v)) = s.split_once('=') else { continue };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        let unq = if let Some(rest) = v.strip_prefix('"') {
            rest.split_once('"').map_or(rest, |(a, _)| a)
        } else if let Some(rest) = v.strip_prefix('\'') {
            rest.split_once('\'').map_or(rest, |(a, _)| a)
        } else {
            v.split('#').next().unwrap_or(v).trim()
        };
        found = Some(unq.to_string());
    }
    found.filter(|s| !s.trim().is_empty())
}

/// Parsing greetd's `[default_session] command`. `NAME=value` assignments
/// and an `env` wrapper before the program are skipped.
pub fn greetd_command_kind(config_toml: &str) -> GreetdCommand {
    let Some(cmd) = toml_string_value(config_toml, "default_session", "command") else {
        return GreetdCommand::Missing;
    };
    let program = cmd
        .split_whitespace()
        .find(|w| *w != "env" && !(w.contains('=') && !w.starts_with('/')))
        .unwrap_or_default()
        .to_string();
    if program.is_empty() {
        return GreetdCommand::Missing;
    }
    let base = program.rsplit('/').next().unwrap_or(&program);
    if GREETD_TEXT_GREETERS.contains(&base) {
        GreetdCommand::Text(program)
    } else {
        GreetdCommand::Graphical(program)
    }
}

/// The greetd greeter user (`[default_session] user`).
pub fn greetd_session_user(config_toml: &str) -> Option<String> {
    toml_string_value(config_toml, "default_session", "user")
}

// ── Readiness: SDDM ────────────────────────────────────────────────────────

/// Default SDDM theme directory (`[Theme] ThemeDir`).
pub const SDDM_THEMES_DIR: &str = "/usr/share/sddm/themes";
/// SDDM greeter for Qt 6 themes and for themes without `QtVersion=6` (Qt 5).
pub const SDDM_GREETER_QT6: &str = "sddm-greeter-qt6";
pub const SDDM_GREETER_QT5: &str = "sddm-greeter";

/// Key value across config files in reading order: the last non-empty one
/// wins (that is how SDDM merges its files).
pub fn ini_last_value(texts: &[&str], group: &str, key: &str) -> Option<String> {
    texts
        .iter()
        .filter_map(|t| crate::de_apply::ini_value(t, group, key))
        .last()
}

/// Whether SDDM needs Xorg: `[General] DisplayServer` (default `x11`).
pub fn sddm_uses_x11(confs: &[&str]) -> bool {
    !matches!(
        ini_last_value(confs, "General", "DisplayServer").as_deref(),
        Some("wayland")
    )
}

/// Compositor program for the SDDM greeter on Wayland
/// (`[Wayland] CompositorCommand`, default `weston`).
pub fn sddm_wayland_compositor(confs: &[&str]) -> String {
    ini_last_value(confs, "Wayland", "CompositorCommand")
        .and_then(|c| c.split_whitespace().next().map(String::from))
        .unwrap_or_else(|| "weston".to_string())
}

/// SDDM theme state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SddmThemeState {
    /// `Current=` is empty: the built-in fallback theme, a working option.
    Builtin,
    /// The theme is in place, a greeter for its Qt exists.
    Ready { theme: String },
    /// No theme directory or `metadata.desktop`: SDDM shows the fallback: not
    /// fatal, but the person should be warned.
    Missing { theme: String },
    /// The theme exists, but there is no greeter for its Qt version.
    NeedsGreeter { theme: String, greeter: String },
}

/// Whether the SDDM theme is ready. `metadata_text` is the theme's
/// `metadata.desktop` (`None`: no directory or file); `has_greeter(name)` is
/// whether the greeter program exists.
pub fn sddm_theme_ready(
    current: Option<&str>,
    metadata_text: Option<&str>,
    has_greeter: &dyn Fn(&str) -> bool,
) -> SddmThemeState {
    let Some(theme) = current.map(str::trim).filter(|t| !t.is_empty()) else {
        return SddmThemeState::Builtin;
    };
    let theme = theme.to_string();
    let Some(meta) = metadata_text else {
        return SddmThemeState::Missing { theme };
    };
    let qt6 = crate::de_apply::ini_value(meta, "SddmGreeterTheme", "QtVersion").as_deref()
        == Some("6");
    let greeter = if qt6 { SDDM_GREETER_QT6 } else { SDDM_GREETER_QT5 };
    if has_greeter(greeter) {
        SddmThemeState::Ready { theme }
    } else {
        SddmThemeState::NeedsGreeter { theme, greeter: greeter.to_string() }
    }
}

// ── Readiness: common checks ───────────────────────────────────────────────

/// Whether a `sysusers.d` file declares the user (`u name …`): then
/// `systemd-sysusers` creates it at boot, and its absence now is not a blocker.
pub fn sysusers_declares(text: &str, user: &str) -> bool {
    text.lines().any(|l| {
        let mut w = l.split_whitespace();
        matches!(w.next(), Some("u" | "u!")) && w.next() == Some(user)
    })
}

/// Whether the user is in the `/etc/passwd` text.
pub fn passwd_has_user(passwd: &str, user: &str) -> bool {
    passwd.lines().any(|l| l.split(':').next() == Some(user))
}

/// greetd data for the readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreetdFacts {
    pub command: GreetdCommand,
    /// Whether the program from `command` exists (for `Graphical`).
    pub program_exists: bool,
}

/// SDDM data for the readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SddmFacts {
    pub theme: SddmThemeState,
    /// Greeter on Wayland: `(compositor, whether it exists)`; `None`: on X.
    pub wayland_compositor: Option<(String, bool)>,
}

/// LightDM data for the readiness check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightdmFacts {
    pub greeter_session: Option<String>,
    pub default_greeter: Option<String>,
    pub greeters: Vec<DmGreeter>,
}

/// Pre-gathered answers about the system for [`dm_readiness`], all without root.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmFacts {
    /// Unit without `.service` (`sddm`, `ddm`…); empty: not set.
    pub unit: String,
    /// The program from the unit's `ExecStart` is in place.
    pub binary_exists: bool,
    /// Services from the unit's `Requires=` that are missing or masked (for DDM,
    /// `dde-seatd.service`): without them systemd will not start the login screen.
    pub missing_units: Vec<String>,
    /// The unit is masked (`/etc/systemd/system/<unit>.service → /dev/null`).
    pub masked: bool,
    /// Greeter user; `None`: nobody to check.
    pub user: Option<String>,
    /// The user exists or `systemd-sysusers` will create it.
    pub user_ok: bool,
    /// The service's PAM file (name) and whether it is in `/etc/pam.d` or `/usr/lib/pam.d`.
    pub pam: String,
    pub pam_exists: bool,
    /// The greeter is on X: Xorg is needed.
    pub needs_xorg: bool,
    pub xorg: bool,
    /// Number of sessions in `xsessions` and `wayland-sessions`.
    pub x_sessions: usize,
    pub wayland_sessions: usize,
    pub lightdm: Option<LightdmFacts>,
    pub greetd: Option<GreetdFacts>,
    pub sddm: Option<SddmFacts>,
}

/// Whether the login screen is ready to become the default: the first
/// refusal reason found for the person, a greeter choice (LightDM) or
/// `Ready`. `Unsupported`: Rexon does not offer the button on this system or
/// for this login screen.
///
/// The Qt binary health probe (`ldd -r`) is not included here yet.
pub fn dm_readiness(
    spec: Option<&DmSpec>,
    backend: &DmSwitchBackend,
    facts: &DmFacts,
) -> DmReadiness {
    let Some(spec) = spec.filter(|s| s.switchable) else {
        return DmReadiness::Unsupported;
    };
    if !backend.switch_supported() {
        return DmReadiness::Unsupported;
    }
    let blocked = DmReadiness::Blocked;
    if facts.masked {
        return blocked(tr("This login screen is switched off in the system (the service is masked)."));
    }
    if !facts.binary_exists {
        // TRANSLATORS: %1 is a program name, e.g. sddm.
        return blocked(trf("The login screen program %1 is missing.", &[&spec.binary]));
    }
    if let Some(unit) = facts.missing_units.first() {
        return blocked(trf(
            // TRANSLATORS: %1 is a system service name, e.g. dde-seatd.service.
            "The login screen needs the system service %1, and it's missing or switched off.",
            &[unit],
        ));
    }
    if !facts.pam_exists {
        // TRANSLATORS: %1 is a PAM service file name, e.g. sddm.
        return blocked(trf("The sign-in settings file (PAM) “%1” is missing.", &[&facts.pam]));
    }
    if let (Some(user), false) = (facts.user.as_deref(), facts.user_ok) {
        return blocked(trf(
            // TRANSLATORS: %1 is a system user name, e.g. sddm.
            "The system user “%1” that the login screen runs as doesn't exist.",
            &[&user],
        ));
    }
    if facts.x_sessions + facts.wayland_sessions == 0 {
        return blocked(tr("No desktop sessions are installed — there would be nothing to sign in to."));
    }
    if facts.needs_xorg && !facts.xorg {
        return blocked(tr("This login screen needs Xorg, and Xorg isn't installed."));
    }
    if let Some(g) = &facts.greetd {
        match &g.command {
            GreetdCommand::Text(_) | GreetdCommand::Missing => {
                return blocked(tr("greetd is set up for text sign-in — the desktop won't start."));
            }
            GreetdCommand::Graphical(program) if !g.program_exists => {
                return blocked(trf(
                    // TRANSLATORS: %1 is a program name, e.g. tuigreet.
                    "The greeter %1 that greetd is set up to start isn't installed.",
                    &[program],
                ));
            }
            GreetdCommand::Graphical(_) => {}
        }
    }
    if let Some(s) = &facts.sddm {
        if let SddmThemeState::NeedsGreeter { theme, greeter } = &s.theme {
            return blocked(trf(
                // TRANSLATORS: %1 is a theme name, %2 a program name (sddm-greeter-qt6).
                "The SDDM theme “%1” needs %2, which isn't installed.",
                &[theme, greeter],
            ));
        }
        if let Some((compositor, false)) = &s.wayland_compositor {
            return blocked(trf(
                // TRANSLATORS: %1 is a program name, e.g. weston or kwin_wayland.
                "SDDM is set to start on Wayland with %1, which isn't installed.",
                &[compositor],
            ));
        }
    }
    if let Some(l) = &facts.lightdm {
        return lightdm_greeter_readiness(
            l.greeter_session.as_deref(),
            l.default_greeter.as_deref(),
            &l.greeters,
        );
    }
    DmReadiness::Ready
}

/// Warnings that do not prevent switching.
pub fn dm_warnings(facts: &DmFacts) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(SddmFacts { theme: SddmThemeState::Missing { theme }, .. }) = &facts.sddm {
        out.push(trf(
            // TRANSLATORS: %1 is a theme name.
            "The SDDM theme “%1” isn't installed — SDDM will show its built-in look.",
            &[theme],
        ));
    }
    if facts.unit == DDM_UNIT {
        out.push(ddm_untested_warning());
    }
    out
}

/// The login screen on a system without systemd by the init selection files:
/// OpenRC: `DISPLAYMANAGER=` in `/etc/conf.d/display-manager`, Devuan: the
/// program path in `/etc/X11/default-display-manager`. The answer is the
/// unit from the table by program name; `None`: cannot be determined.
pub fn dm_from_init_files(conf_d: Option<&str>, x11_default: Option<&str>) -> Option<String> {
    let from_conf = conf_d.and_then(|t| {
        t.lines().rev().find_map(|l| {
            let v = l.trim().strip_prefix("DISPLAYMANAGER=")?;
            Some(v.trim().trim_matches(['"', '\'']).to_string())
        })
    });
    let from_x11 = x11_default.and_then(|t| {
        let line = t.lines().map(str::trim).find(|l| !l.is_empty())?;
        Some(line.rsplit('/').next().unwrap_or(line).to_string())
    });
    let name = from_conf.or(from_x11).filter(|n| !n.is_empty())?;
    LOGIN_MANAGER_SPECS
        .iter()
        .find(|s| s.binary == name || s.unit == name)
        .map(|s| s.unit.to_string())
        .or(Some(name))
}

impl DmReadiness {
    /// Greeter suggested by default when the person has to choose.
    pub fn suggested_greeter(&self) -> Option<&DmGreeter> {
        match self {
            DmReadiness::NeedsGreeterChoice(list) => suggested_greeter(list),
            _ => None,
        }
    }
}

impl LoginManagerInfo {
    /// Greeter suggested by default (see [`DmReadiness::suggested_greeter`]).
    pub fn suggested_greeter(&self) -> Option<&DmGreeter> {
        self.readiness.suggested_greeter()
    }
}

// ── «Make default»: command and texts ─────────────────────────────────────

/// LightDM greeter name: the file name without `.desktop`, without paths and flags.
pub fn greeter_name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

/// `pkexec rexon-pkg-helper … switch-dm <unit> [<greeter>]`. `None`: no
/// helper or the arguments failed the check: there is deliberately no raw
/// `pkexec systemctl`: it would not repeat the checks and would not roll back
/// an error (then the UI says [`login_manager_needs_helper`]).
pub fn switch_dm_cmd(
    family: PkgFamily,
    unit: &str,
    greeter: Option<&str>,
    helper_available: bool,
) -> Option<Vec<String>> {
    let mut args = vec![unit.to_string()];
    args.extend(greeter.map(String::from));
    privileged_pkg_cmd(family, "switch-dm", &args, helper_available)
}

/// The command that restores the previous login screen from a text console.
/// The same line the helper writes into [`PREVIOUS_DM_FILE`].
pub fn dm_rollback_command(old_unit: &str, backend: &DmSwitchBackend) -> Option<String> {
    let old = old_unit.trim();
    if !greeter_name_ok(old) {
        return None;
    }
    let enable = format!("sudo systemctl enable --force {old}.service && sudo reboot");
    match backend {
        DmSwitchBackend::Systemd => Some(enable),
        DmSwitchBackend::SystemdDebian => {
            let pkg = DM_DEBIAN_PKG_FOR_UNIT
                .iter()
                .find(|(u, _)| *u == old)
                .map_or(old, |(_, p)| *p);
            Some(format!("sudo dpkg-reconfigure {pkg} && {enable}"))
        }
        _ => None,
    }
}

fn dm_label(unit: &str) -> String {
    let label = crate::categories::login_manager_label(unit);
    if label.is_empty() { unit.trim().to_string() } else { label.to_string() }
}

/// Confirmation dialog title.
pub fn login_manager_switch_title(new_unit: &str) -> String {
    // TRANSLATORS: %1 is a login screen name (SDDM, LightDM, …).
    trf("Make %1 the login screen?", &[&dm_label(new_unit)])
}

/// Hint "how to restore the previous login screen from the console". `None`:
/// this mechanism has no command.
pub fn login_manager_rollback_hint(old_unit: &str, backend: &DmSwitchBackend) -> Option<String> {
    let cmd = dm_rollback_command(old_unit, backend)?;
    Some(trf(
        // TRANSLATORS: %1 is a terminal command, %2 a file path.
        "If there's no login screen after the restart: press Ctrl+Alt+F3, sign in with your name and password and run:\n%1\nThe same is written in %2.",
        &[&cmd, &PREVIOUS_DM_FILE],
    ))
}

/// Confirmation dialog text (before the password, rule #13: the condition is
/// named in advance): what becomes the default, which greeter, when it
/// appears and how to revert. `old_unit` is the current default (`None`: unknown).
pub fn login_manager_switch_confirm(
    old_unit: Option<&str>,
    new_unit: &str,
    greeter: Option<&DmGreeter>,
    backend: &DmSwitchBackend,
) -> String {
    let new = dm_label(new_unit);
    let mut parts = vec![trf(
        // TRANSLATORS: %1 is a login screen name.
        "%1 will appear after a restart. Your current session won't be interrupted.",
        &[&new],
    )];
    if let Some(g) = greeter {
        let name = if g.name.is_empty() { &g.id } else { &g.name };
        // TRANSLATORS: %1 is a LightDM greeter name (Slick Greeter, …).
        parts.push(trf("Greeter: %1.", &[name]));
    }
    if new_unit.trim() == DDM_UNIT {
        parts.push(ddm_untested_warning());
    }
    if let Some(old) = old_unit.map(str::trim).filter(|o| !o.is_empty()) {
        parts.push(trf(
            // TRANSLATORS: %1 is the new login screen name, %2 the current one.
            "If %1 doesn't start, Rexon brings back %2 by itself on that boot.",
            &[&new, &dm_label(old)],
        ));
        parts.extend(login_manager_rollback_hint(old, backend));
    }
    parts.join("\n\n")
}

/// Result "done".
pub fn login_manager_switch_done(new_unit: &str) -> String {
    // TRANSLATORS: %1 is a login screen name.
    trf("Done: %1 will be the login screen after a restart.", &[&dm_label(new_unit)])
}

/// Result "failed" with the reason from the helper.
pub fn login_manager_switch_failed(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        tr("Couldn't change the login screen.")
    } else {
        // TRANSLATORS: %1 is the reason reported by Rexon's helper.
        trf("Couldn't change the login screen: %1", &[&reason])
    }
}

/// No helper: nothing to switch with.
pub fn login_manager_needs_helper() -> String {
    tr("Changing the login screen needs the Rexon helper, and it isn't installed. Reinstall Rexon to add it.")
}

/// The helper's answer to `switch-dm`: `(done, text for the person)`. The
/// refusal reason is its last line without the helper name; code 126/127
/// means the password was not confirmed.
pub fn switch_dm_outcome(
    returncode: Option<i32>,
    stdout: &str,
    stderr: &str,
    new_unit: &str,
    manual_cmd: &str,
) -> (bool, String) {
    if returncode == Some(0) {
        return (true, login_manager_switch_done(new_unit));
    }
    if matches!(returncode, Some(126 | 127)) {
        return (false, pkexec_denied_message(manual_cmd));
    }
    let prefix = format!("{PKG_HELPER_NAME}: ");
    let reason = stderr
        .lines()
        .chain(stdout.lines())
        .filter(|l| !l.trim().is_empty())
        .last()
        .map(|l| l.strip_prefix(&prefix).unwrap_or(l).to_string())
        .unwrap_or_default();
    (false, login_manager_switch_failed(&reason))
}

/// An explanation instead of the button where Rexon does not switch the login
/// screen. `manual_cmd` is the command for this init ([`dm_manual_switch_command`]).
pub fn login_manager_unsupported_note(
    backend: &DmSwitchBackend,
    manual_cmd: Option<&str>,
) -> Option<String> {
    Some(match backend {
        DmSwitchBackend::Systemd | DmSwitchBackend::SystemdDebian => return None,
        DmSwitchBackend::NixosDeclarative => tr(
            "On NixOS the login screen is chosen in configuration.nix (services.displayManager), so Rexon doesn't change it.",
        ),
        DmSwitchBackend::SystemdSuseAlternatives => {
            tr("On openSUSE Rexon can't change the login screen yet.")
        }
        DmSwitchBackend::Unsupported(init) => {
            let init = if init.is_empty() { tr("its own init system") } else { init.clone() };
            match manual_cmd {
                Some(cmd) => trf(
                    // TRANSLATORS: %1 is an init system name (OpenRC, runit, …), %2 a terminal command.
                    "Here the login screen is switched by %1, and Rexon can't do it yet. In a terminal:\n%2",
                    &[&init, &cmd],
                ),
                // TRANSLATORS: %1 is an init system name (OpenRC, runit, …).
                None => trf("Here the login screen is switched by %1, and Rexon can't do it yet.", &[&init]),
            }
        }
    })
}

// ============================================================================
// Live parsing of pacman output (port of `PackageManager._parse_pacman_line`)
// ============================================================================
//
// The desktop install shows progress line by line: Python reads the log file
// tail, and the MEANING of a line (which step, which package, which message
// to show the person) is computed here. The meaning used to be computed by
// seven regexes in Python; they were ported as "letter by letter" parsing,
// because the core does without the regex crate.
//
// The parsing is pure: no files, no processes. The reference is the recorded
// answers of live Python on the same lines (`golden/system_plan_texts.json`,
// the `pacman_progress` key).
//
// ⚠️ Fixed in the port: the Python version had a `total_packages` parameter
// that no branch read. It was not ported: the calling code did not need it
// either.

/// A parsed line of pacman output: the stage, a ready message for the person
/// and the split fields (where present).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PacmanLine {
    pub stage: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
}

impl PacmanLine {
    fn plain(stage: &str, message: String) -> Self {
        PacmanLine {
            stage: stage.to_string(),
            message,
            current: None,
            total: None,
            action: None,
            package: None,
            filename: None,
            count: None,
        }
    }
}

/// Python's `\w`: a letter, digit or underscore (Unicode-aware).
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Case-insensitive "string starts with": returns the tail.
fn ci_strip_prefix<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let mut si = s.char_indices();
    for pc in prefix.chars() {
        let (_, sc) = si.next()?;
        if !sc.eq_ignore_ascii_case(&pc) {
            return None;
        }
    }
    Some(&s[si.next().map(|(i, _)| i).unwrap_or(s.len())..])
}

/// `\s+` at the start: the tail after at least one whitespace character.
fn strip_ws1(s: &str) -> Option<&str> {
    let t = s.trim_start();
    if t.len() == s.len() {
        None
    } else {
        Some(t)
    }
}

/// `(.+?)\.{3}\s*$`: the body up to the trailing ellipsis (non-empty).
fn strip_ellipsis(s: &str) -> Option<&str> {
    let body = s.trim_end().strip_suffix("...")?;
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// `^\((\d+)/(\d+)\)\s+`: the step counter and the rest of the line after it.
fn counter_prefix(line: &str) -> Option<(u64, u64, &str)> {
    let rest = line.strip_prefix('(')?;
    let close = rest.find(')')?;
    let (cur, total) = rest[..close].split_once('/')?;
    if cur.is_empty()
        || total.is_empty()
        || !cur.bytes().all(|b| b.is_ascii_digit())
        || !total.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((
        cur.parse().ok()?,
        total.parse().ok()?,
        strip_ws1(&rest[close + 1..])?,
    ))
}

/// Split "action \s+ name" exactly as the greedy `\s+` with backtracking did:
/// the name is everything after the spaces, and if nothing follows them we
/// back off by one whitespace character (it becomes the name).
fn split_after_action(rest: &str) -> Option<String> {
    let name = rest.trim_start();
    if name.len() == rest.len() {
        return None; // `\s+` requires at least one space
    }
    if !name.is_empty() {
        return Some(name.to_string());
    }
    // Only spaces: the last of them is the `(.+?)`.
    rest.chars().next_back().map(|c| c.to_string())
}

/// Action and package from the tail of a `(N/M) …` line, or None.
fn operation_action(payload: &str) -> Option<(String, String)> {
    for act in [
        "installing",
        "upgrading",
        "removing",
        "reinstalling",
        "downgrading",
    ] {
        if let Some(rest) = payload.strip_prefix(act) {
            if let Some(pkg) = split_after_action(rest) {
                return Some((act.to_string(), pkg));
            }
        }
    }
    // `checking \w[\w ]*` / `loading \w[\w ]*`: a greedy run of words and
    // spaces, backing off to where at least one more word remains.
    for act in ["checking ", "loading "] {
        let Some(rest) = payload.strip_prefix(act) else {
            continue;
        };
        let Some(first) = rest.chars().next() else {
            continue;
        };
        if !is_word_char(first) {
            continue;
        }
        let run: Vec<usize> = rest
            .char_indices()
            .take_while(|(_, c)| is_word_char(*c) || *c == ' ')
            .map(|(i, c)| i + c.len_utf8())
            .collect();
        for &end in run.iter().rev() {
            if let Some(pkg) = split_after_action(&rest[end..]) {
                return Some((
                    format!("{}{}", act, &rest[..end]),
                    pkg,
                ));
            }
        }
    }
    None
}

/// `^Packages\s+\((\d+)\)`: how many packages there are in the transaction.
fn packages_count(line: &str) -> Option<u64> {
    let rest = strip_ws1(ci_strip_prefix(line, "Packages")?)?;
    let inner = rest.strip_prefix('(')?;
    let close = inner.find(')')?;
    let digits = &inner[..close];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// `^<header>:\s+([\d.]+)\s+(\w+)`: the size and the unit.
fn total_size(line: &str, header: &str) -> Option<String> {
    let rest = strip_ws1(ci_strip_prefix(line, header)?)?;
    let num_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_digit() || *b == b'.')
        .count();
    if num_len == 0 {
        return None;
    }
    let unit_rest = strip_ws1(&rest[num_len..])?;
    let unit_len: usize = unit_rest
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .map(|(_, c)| c.len_utf8())
        .sum();
    if unit_len == 0 {
        return None;
    }
    Some(format!("{} {}", &rest[..num_len], &unit_rest[..unit_len]))
}

/// `^(\S+)\s+is up to date\s*--\s*skipping`: the name of a package pacman
/// skipped as already installed (`--needed`).
fn skip_package(body: &str) -> Option<&str> {
    let name_len = body.chars().take_while(|c| !c.is_whitespace()).count();
    let name_len: usize = body
        .chars()
        .take(name_len)
        .map(|c| c.len_utf8())
        .sum();
    if name_len == 0 {
        return None;
    }
    let rest = strip_ws1(&body[name_len..])?;
    let rest = ci_strip_prefix(rest, "is up to date")?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("--")?;
    let rest = rest.trim_start();
    ci_strip_prefix(rest, "skipping")?;
    Some(&body[..name_len])
}

// ── About re-sorting mirrors (decided 2026-09-01, we do NOT do it) ──────
//
// The temptation: after several mirror failures in a row, offer the person
// to reshuffle the list (`cachyos-rate-mirrors`, `rate-mirrors`,
// `reflector`; Wehrwolfmann's machine has all three). Rejected, and here is why.
//
// 1. There is nothing to fix. A mirror failure after which the transaction
//    ENDED in success is not a breakage but normal operation of the fallback
//    list: the 12 mirrors in cachyos-mirrorlist and 66 in mirrorlist are
//    there exactly for that. Calling the person to fix what fixed itself is
//    the worst kind of false alarm.
// 2. The cost of a mistake is asymmetric. The mirror list is system-wide,
//    root-owned, and the WHOLE system uses it, not just our install. A bad
//    re-sort breaks any future update, including our own. The gain is
//    minutes, the loss is a working package manager.
// 3. The files are not ours. `/etc/pacman.d/cachyos-mirrorlist` belongs to
//    the cachyos-mirrorlist package, `/etc/pacman.d/mirrorlist` to the
//    pacman-mirrorlist package. Rewriting them, we get a .pacnew on every
//    update and fight the distribution's own tools.
// 4. It is slow and needs the network. Re-sorting itself visits dozens of
//    mirrors; doing it in the middle of a desktop install means lengthening a
//    successful operation for a symptom that is already gone.
//
// What we do instead: tell the truth line by line (below) and do not paint
// survivable things red. If the download fails FOR REAL, the person sees the
// real pacman error and decides themselves: advice about mirrors fits
// there, not here.

/// The download failed FOR REAL: pacman announced that some files could not
/// be downloaded from any mirror, and did not start the transaction.
pub fn download_failed(output: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim().to_ascii_lowercase();
        line.contains("failed to retrieve some files")
            || line.starts_with("error: failed to commit transaction (failed to retrieve")
    })
}

/// What to tell the person when packages did not download: what happened,
/// that nothing is broken and what to do. The raw pacman text stays in the log.
pub fn download_failed_message() -> String {
    tr("Couldn't download the packages: the package servers didn't respond. \
        The internet connection may have dropped.\n\
        Nothing was changed — check the connection and try again.")
}

/// Result of a failed transaction without a parsed reason: only the error lines.
///
/// ⭐ Fixed 08.11.2026: previously the window got ALL of pacman's output: the
/// package summary, download progress and "warning: … is up to date --
/// skipping" under a red cross, although each line was already shown in the
/// log line by line and in human terms. If there are no `error:` lines, we
/// pass the output as is, so as not to lose the only explanation.
pub fn manager_error_lines(output: &str) -> String {
    let errors: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| line.to_ascii_lowercase().starts_with("error:"))
        .filter(|line| mirror_retrieve_failure(line["error:".len()..].trim()).is_none())
        .collect();
    if errors.is_empty() {
        output.trim().to_string()
    } else {
        errors.join("\n")
    }
}

/// `failed retrieving file '<file>' from <mirror> : <reason>`: the mirror name.
///
/// This is a SURVIVABLE failure: pacman takes the next mirror from the list
/// right away, and the install goes on. A real download failure is announced
/// by pacman on a SEPARATE line (`failed to retrieve some files` / `failed to
/// commit transaction`), so one can be told from the other line by line,
/// without waiting for the result.
fn mirror_retrieve_failure(body: &str) -> Option<&str> {
    let rest = ci_strip_prefix(body, "failed retrieving file")?;
    let rest = strip_ws1(rest)?;
    // The quoted file name is not needed by itself: without it identical
    // failures of one mirror collapse into one line.
    let rest = rest.strip_prefix('\'')?;
    let close = rest.find('\'')?;
    let rest = strip_ws1(&rest[close + 1..])?;
    let rest = ci_strip_prefix(rest, "from")?;
    let rest = strip_ws1(rest)?;
    // The tail "<mirror> : <reason>": the mirror name is up to the first space.
    let host_len: usize = rest
        .chars()
        .take_while(|c| !c.is_whitespace())
        .map(|c| c.len_utf8())
        .sum();
    if host_len == 0 {
        return None;
    }
    Some(&rest[..host_len])
}

/// `too many errors from <mirror>, skipping for the remainder of this
/// transaction`: the name of a mirror pacman crossed out until the end of the
/// transaction. Also survivable: a mirror list is a list for a reason.
fn mirror_skipped(body: &str) -> Option<&str> {
    let rest = ci_strip_prefix(body, "too many errors from")?;
    let rest = strip_ws1(rest)?;
    let comma = rest.find(',')?;
    let host = rest[..comma].trim_end();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    ci_strip_prefix(rest[comma + 1..].trim_start(), "skipping")?;
    Some(host)
}

/// `There are <N> <kind> ... <subject>:`: parsing the header of an
/// interactive pacman question. Returns (N, subject) for a question of the
/// requested kind.
///
/// There are TWO questions, and pacman asks both itself:
///   * `There are 24 members in group lxqt:`: which group members to install;
///   * `There are 2 providers available for libQt6Xdg.so=4-64:`: whose package
///     satisfies the dependency.
/// With `--noconfirm` (and we only call pacman that way) neither waits for
/// input: the question is printed and the default is taken. But in the log
/// it looked like a riddle: the person saw the question and not the answer.
fn there_are_question<'a>(line: &'a str, kind: &str) -> Option<(u64, &'a str)> {
    let rest = strip_ws1(ci_strip_prefix(line, "There are")?)?;
    let num_len: usize = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .map(|c| c.len_utf8())
        .sum();
    if num_len == 0 {
        return None;
    }
    let count: u64 = rest[..num_len].parse().ok()?;
    let rest = strip_ws1(&rest[num_len..])?;
    let rest = ci_strip_prefix(rest, kind)?;
    let rest = strip_ws1(rest)?;
    let subject = rest.trim_end().strip_suffix(':')?.trim_end();
    if subject.is_empty() {
        return None;
    }
    Some((count, subject))
}

/// Body of a `:: …` line that pacman answers itself under `--noconfirm` and
/// in which there is nothing for the person to read: the selection menu
/// header (`Repository extra`) and the transaction consent (`Proceed with
/// installation? [Y/n]`, `Proceed with download? [Y/n]`). Meaningful
/// questions (`Remove kwin? [y/N]`, `Import PGP key …? [Y/n]`, `Replace …?`)
/// are NOT included: their answer decides the outcome, and the person must
/// see the line.
fn is_self_answered_question(body: &str) -> bool {
    if let Some(rest) = ci_strip_prefix(body, "Repository").and_then(strip_ws1) {
        let name = rest.trim_end();
        return !name.is_empty() && !name.contains(char::is_whitespace);
    }
    let Some(rest) = ci_strip_prefix(body, "Proceed with").and_then(strip_ws1) else {
        return false;
    };
    let rest = rest.trim_end();
    (ci_strip_prefix(rest, "installation?").is_some() || ci_strip_prefix(rest, "download?").is_some())
        && (rest.ends_with("[Y/n]") || rest.ends_with("[y/N]"))
}

/// A pacman selection menu item: `1) gsettings-qt5` or on one line
/// `1) a  2) b  3) c` (leading spaces stripped): strictly alternating
/// "number)" and a package name: numbered text of an install script
/// ("1) Do this") does not match this and stays visible.
fn is_menu_item(line: &str) -> bool {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    !tokens.is_empty()
        && tokens.len() % 2 == 0
        && tokens.chunks(2).all(|pair| {
            let num = pair[0].strip_suffix(')').unwrap_or("");
            !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit())
                && pair[1].chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                && pair[1].chars().all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c))
        })
}

/// `re.sub(r'-[^-\s]+-[^-\s]+$', '', name)`: cut off "-<version>-<release>".
fn strip_version_suffix(name: &str) -> &str {
    let ok = |seg: &str| !seg.is_empty() && !seg.contains(|c: char| c == '-' || c.is_whitespace());
    let Some(last) = name.rfind('-') else {
        return name;
    };
    if !ok(&name[last + 1..]) {
        return name;
    }
    let Some(prev) = name[..last].rfind('-') else {
        return name;
    };
    if !ok(&name[prev + 1..last]) {
        return name;
    }
    &name[..prev]
}

/// Parse one line of pacman output: the stage + a message for the person.
/// None: the line means nothing for the progress display.
/// A pacman transaction service header (`:: Running pre-transaction
/// hooks...`, `:: Processing package changes...`): it is followed by progress
/// lines with their own meaning, and it tells the person nothing by itself.
fn is_transaction_header(body: &str) -> bool {
    let b = body.to_lowercase();
    [
        "running pre-transaction hooks",
        "running post-transaction hooks",
        "processing package changes",
        "synchronizing package databases",
        "checking keys in keyring",
        "checking package integrity",
        "loading package files",
        "checking for file conflicts",
        "checking available disk space",
    ]
    .iter()
    .any(|h| b.starts_with(h))
}

/// Package and hook output around the transaction that the window does not
/// need (see [`parse_pacman_line`]). `raw` is the line with its leading indent.
fn is_package_chatter(raw: &str) -> bool {
    let line = raw.trim();
    if line.starts_with("Optional dependencies for ") || line.starts_with("===>") {
        return true;
    }
    // `==> root: 1488` (snap-pac: the config and the snapshot number).
    if let Some(rest) = line.strip_prefix("==> ") {
        if let Some((cfg, num)) = rest.split_once(": ") {
            if !cfg.is_empty() && !cfg.contains(' ') && !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    let lower = line.to_lowercase();
    if lower.starts_with("performing snapper ") || lower.starts_with("waiting for limine-snapper-sync") {
        return true;
    }
    // An optional dependency line: indent, package name, then ": description"
    // and/or "[installed]"/"[pending]".
    if raw.starts_with("    ") {
        let head = line.split([':', ' ']).next().unwrap_or("");
        let name_ok = !head.is_empty()
            && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '@'));
        let rest = line[head.len()..].trim_start();
        return name_ok && (rest.is_empty() || rest.starts_with(':') || rest.starts_with('['));
    }
    false
}

pub fn parse_pacman_line(raw: &str) -> Option<PacmanLine> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }

    // (N/M) installing|upgrading|… <package>...
    if let Some((cur, total, rest)) = counter_prefix(line) {
        if let Some(payload) = strip_ellipsis(rest) {
            if let Some((action, pkg)) = operation_action(payload) {
                let check = action.starts_with("checking") || action.starts_with("loading");
                let label = match action.as_str() {
                    "installing" => trc("pacman action", "Installing"),
                    "upgrading" => trc("pacman action", "Upgrading"),
                    "removing" => trc("pacman action", "Removing"),
                    "reinstalling" => trc("pacman action", "Reinstalling"),
                    "downgrading" => trc("pacman action", "Downgrading"),
                    other => other.to_string(),
                };
                return Some(PacmanLine {
                    stage: (if check { "check" } else { "pkg_install" }).to_string(),
                    message: if check {
                        // What exactly pacman checks (keys, conflicts, space) is in English and not
                        // needed by the person; the counter is.
                        trf("🔍 (%1/%2) Checking the packages…", &[&cur, &total])
                    } else {
                        format!("📦 ({}/{}) {}: {}", cur, total, label, pkg)
                    },
                    current: Some(cur),
                    total: Some(total),
                    action: Some(action),
                    package: Some(pkg),
                    filename: None,
                    count: None,
                });
            }
        }
    }

    // downloading <file>...
    if let Some(rest) = ci_strip_prefix(line, "downloading") {
        if let Some(rest) = strip_ws1(rest) {
            if let Some(name) = strip_ellipsis(rest) {
                let mut ev = PacmanLine::plain(
                    "download",
                    trf("⬇️  Downloading: %1", &[&name]),
                );
                ev.filename = Some(name.to_string());
                return Some(ev);
            }
        }
    }

    // Packages (N)
    if let Some(count) = packages_count(line) {
        let mut ev = PacmanLine::plain("summary", trf("📋 Total packages: %1", &[&count]));
        ev.count = Some(count);
        return Some(ev);
    }

    // Total Download Size / Total Installed Size
    if let Some(size) = total_size(line, "Total Download Size:") {
        return Some(PacmanLine::plain("summary", trf("⬇️  Download size: %1", &[&size])));
    }
    if let Some(size) = total_size(line, "Total Installed Size:") {
        return Some(PacmanLine::plain("summary", trf("💾 Installed size: %1", &[&size])));
    }

    // :: A section header, but first the two interactive pacman questions that
    // it answers itself under `--noconfirm`.
    if let Some(rest) = line.strip_prefix("::") {
        if let Some(body) = strip_ws1(rest) {
            // "There are N members in group G:": we install the whole group.
            if let Some((count, group)) = there_are_question(body, "members in group") {
                let mut ev = PacmanLine::plain(
                    "prompt",
                    fill(
                        &ntr(
                            // TRANSLATORS: %1 is a package group name.
                            "🧩 Group %1 (%n package) — installing all of it, nothing to answer",
                            "🧩 Group %1 (%n packages) — installing all of it, nothing to answer",
                            count,
                        ),
                        &[&group],
                    ),
                );
                ev.count = Some(count);
                ev.package = Some(group.to_string());
                return Some(ev);
            }
            // "There are N providers available for X:": we take the provider from the
            // repository with the highest priority. This is NOT an accident of order:
            // pacman lists providers in the repository order of pacman.conf, and first
            // there is the one the person (or their distribution) declared primary.
            // Exactly this would be taken by `pacman -S` from a terminal, so we have no
            // right to decide for it.
            if let Some((count, subject)) =
                there_are_question(body, "providers available for")
            {
                let mut ev = PacmanLine::plain(
                    "prompt",
                    trf(
                        // TRANSLATORS: %1 is what is being provided, %2 how many
                        // packages can provide it.
                        "🔀 %1: %2 providers — taking the one from the first \
                         repository in pacman.conf priority order",
                        &[&subject, &count],
                    ),
                );
                ev.count = Some(count);
                ev.package = Some(subject.to_string());
                return Some(ev);
            }
            // The rest from the same questions that pacman answers itself under
            // `--noconfirm`: "Repository <name>" is the header of the providers and group
            // members menu, "Proceed with installation/download? [Y/n]" is the consent.
            // There is nothing here for the person to read: the answer was already named
            // a line above (or it is just "yes"). An empty message: see "Enter a …".
            if is_self_answered_question(body) {
                return Some(PacmanLine::plain("prompt", String::new()));
            }
            // Transaction service headers, not as an English "📌 …": the download gets
            // its own line, the rest the person does not need (package and hook
            // progress follows in its own lines). Live on 08.11.2026, «Check / Repair»
            // for MATE.
            if body.to_lowercase().starts_with("retrieving packages") {
                return Some(PacmanLine::plain("download", tr("⬇️  Downloading packages…")));
            }
            if is_transaction_header(body) {
                return Some(PacmanLine::plain("prompt", String::new()));
            }
            if !body.is_empty() {
                return Some(PacmanLine::plain("section", format!("📌 {}", body)));
            }
        }
    }

    // "Enter a selection (default=all):" / "Enter a number (default=1):": the
    // tail of an already explained question. There is nothing to show: the
    // answer was named a line above, and the question itself does not wait for input.
    //
    // ⚠️ Exactly an EMPTY message, not `None`: `None` from this parser means
    // "I do not know this line", and the executor shows it raw (the `raw`
    // stage). Caught by showing the whole log: the question was explained a
    // line above and immediately printed again in English.
    if let Some(rest) = ci_strip_prefix(line, "Enter a") {
        if let Some(rest) = strip_ws1(rest) {
            if (ci_strip_prefix(rest, "selection").is_some()
                || ci_strip_prefix(rest, "number").is_some())
                && rest.contains("default=")
            {
                return Some(PacmanLine::plain("prompt", String::new()));
            }
        }
    }
    // "   1) gsettings-qt5" / "   1) a  2) b  3) c": items of the same menu.
    if is_menu_item(line) {
        return Some(PacmanLine::plain("prompt", String::new()));
    }

    // error: / warning:
    if let Some(rest) = ci_strip_prefix(line, "error:") {
        if let Some(body) = strip_ws1(rest) {
            if !body.is_empty() {
                // ⚠️ A failure of ONE mirror is not our problem: pacman takes the next one
                // and completes the install. A red cross on such a line told the user
                // "everything broke" where everything is fine. The yardstick: the
                // transaction ended in success → no crosses in the log at all; a real
                // download failure is announced by pacman separately ("failed to retrieve
                // some files", "failed to commit transaction"), and it stays red.
                if let Some(host) = mirror_retrieve_failure(body) {
                    let mut ev = PacmanLine::plain(
                        "mirror",
                        trf("🌐 mirror didn't respond (%1), trying another one", &[&host]),
                    );
                    ev.package = Some(host.to_string());
                    return Some(ev);
                }
                return Some(PacmanLine::plain("error", format!("❌ {}", body)));
            }
        }
    }
    if let Some(rest) = ci_strip_prefix(line, "warning:") {
        if let Some(body) = strip_ws1(rest) {
            if !body.is_empty() {
                // "<pkg>-<ver> is up to date -- skipping" is not a problem but the normal
                // answer of `--needed` in «Check / Repair» mode: we show a friendly
                // "already installed". "too many errors from <mirror>, skipping…" is of the
                // same class as the download failure above: a mirror list is a list for a
                // reason. We show it calmly, with the same line.
                if let Some(host) = mirror_skipped(body) {
                    let mut ev = PacmanLine::plain(
                        "mirror",
                        trf(
                            "🌐 no longer asking mirror %1 for the rest of this installation",
                            &[&host],
                        ),
                    );
                    ev.package = Some(host.to_string());
                    return Some(ev);
                }
                if let Some(pkg) = skip_package(body) {
                    let base = strip_version_suffix(pkg);
                    let base = if base.is_empty() { pkg } else { base };
                    let mut ev = PacmanLine::plain(
                        "skip",
                        trf("✅ already installed: %1", &[&base]),
                    );
                    ev.package = Some(base.to_string());
                    return Some(ev);
                }
                return Some(PacmanLine::plain("warning", format!("⚠️  {}", body)));
            }
        }
    }

    // (N/M) <hook>... : pacman's post-transaction hooks
    if let Some((cur, total, rest)) = counter_prefix(line) {
        if let Some(hook) = strip_ellipsis(rest) {
            return Some(PacmanLine {
                stage: "hook".to_string(),
                // The hook name is an English phrase from someone else's package
                // ("Checking the local deepin-session-treeland build…", "Arming
                // ConditionNeedsUpdate"): the window gets the counter, the name goes to the log.
                message: {
                    crate::log_info!("pkg", "хук pacman ({cur}/{total}): {hook}");
                    trf("🔧 (%1/%2) Finishing the installation…", &[&cur, &total])
                },
                current: Some(cur),
                total: Some(total),
                action: None,
                package: None,
                filename: None,
                count: None,
            });
        }
    }

    // Transaction progress without a counter (output not to a terminal):
    // "installing x..." is said in the UI language; "checking keyring...",
    // "resolving dependencies..." and other checks are hidden (live on
    // 08.11.2026, lxqt-wayland-session).
    if let Some(payload) = strip_ellipsis(line) {
        if let Some((action, pkg)) = operation_action(payload) {
            if !(action.starts_with("checking") || action.starts_with("loading")) {
                let label = match action.as_str() {
                    "installing" => trc("pacman action", "Installing"),
                    "upgrading" => trc("pacman action", "Upgrading"),
                    "removing" => trc("pacman action", "Removing"),
                    "reinstalling" => trc("pacman action", "Reinstalling"),
                    "downgrading" => trc("pacman action", "Downgrading"),
                    other => other.to_string(),
                };
                let mut ev = PacmanLine::plain("pkg_install", format!("📦 {label}: {pkg}"));
                ev.action = Some(action);
                ev.package = Some(pkg);
                return Some(ev);
            }
        }
    }
    if line.starts_with("resolving ")
        || line.starts_with("looking for ")
        || (strip_ellipsis(line).is_some() && (line.starts_with("checking ") || line.starts_with("loading ")))
    {
        return Some(PacmanLine::plain("check", String::new()));
    }
    // "Optional dependencies for x" and its lines "    labwc [installed]",
    // "    river: description"; messages of package install scripts ("===> For
    // a working … please install …"); the chatter of snapper and limine hooks
    // ("Performing snapper pre snapshots…", "==> root: 1488", "Waiting for
    // limine-snapper-sync to finish"). The person does not need them and they
    // often mislead (asking to install what is already installed): only to the
    // Rexon log.
    if is_package_chatter(raw) {
        crate::log_info!("pkg", "{line}");
        return Some(PacmanLine::plain("chatter", String::new()));
    }
    // "there is nothing to do" (`--needed`, everything already installed): the
    // result says it itself in the UI language (`system_de_exec::main_already_installed`).
    if line.eq_ignore_ascii_case("there is nothing to do") {
        return Some(PacmanLine::plain("nothing", String::new()));
    }
    if line.to_lowercase().contains("retrieving packages") {
        return Some(PacmanLine::plain(
            "download",
            tr("⬇️  Downloading packages…"),
        ));
    }
    None
}

// ============================================================================
// Foolproof guard "there must be a login screen" (port of `ensure_display_manager`)
// ============================================================================
//
// Installing a tiling WM (niri/sway/Hyprland) does not pull in a display
// manager by itself: sddm was deliberately removed from their package lists,
// because the login screen is a system-wide entity, not part of a specific
// desktop. Without this guard the person is left without a graphical login
// after a reboot.
//
// The whole decision is here: whether there is anything to install, what to
// install with, what to enable with and what to say. Live probes (is there
// a DM, is there systemctl, is the privileged helper deployed) are passed as
// parameters, running commands is up to the executor.

/// sddm PACKAGE name by family. Gentoo: with the category. NIX is excluded
/// (a declarative build there, not the pkexec path), CLEAR does not ship sddm
/// as a separate bundle; on them the auto-install is softly skipped.
pub const SDDM_PKG_FOR_FAMILY: &[(&str, &str)] = &[
    ("arch", "sddm"),
    ("debian", "sddm"),
    ("fedora", "sddm"),
    ("suse", "sddm"),
    ("gentoo", "x11-misc/sddm"),
    ("alpine", "sddm"),
    ("void", "sddm"),
    ("solus", "sddm"),
    ("alt", "sddm"),
];

/// What to do with the login screen after a desktop install.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum EnsureDmPlan {
    /// An enabled login screen already exists: we do not interfere.
    Present { message: String, code: String },
    /// Nothing to install or nothing to install with: not systemd, no package
    /// name for the family, no install command. Not an error: the desktop install
    /// does not fail.
    Skipped { message: Option<String>, code: String },
    /// No login screen: install SDDM and enable its unit.
    Install {
        message: String,
        fraction: f64,
        package: String,
        install: Cmd,
        enable: Cmd,
        done_message: String,
        done_fraction: f64,
        code: String,
    },
}

/// Decision and commands of the login screen foolproof guard.
///
/// `dm_present` is the `display_manager_present_from` answer, `has_systemctl`
/// is the presence of systemd, `pkg_helper_available` is whether
/// rexon-pkg-helper is deployed (with it both commands go as named
/// org.rexon.* actions and do not ask for the password a second time).
pub fn ensure_dm_plan(
    family: PkgFamily,
    dm_present: bool,
    has_systemctl: bool,
    pkg_helper_available: bool,
) -> EnsureDmPlan {
    if dm_present {
        return EnsureDmPlan::Present {
            message: tr("⚙️ Login screen already set up — skipping"),
            code: "present".to_string(),
        };
    }
    if !has_systemctl {
        // Not systemd: init itself manages the login screen, we do not add a second DM.
        return EnsureDmPlan::Skipped { message: None, code: "skipped".to_string() };
    }
    let fam = family.as_str();
    let Some(pkg) = table_get(SDDM_PKG_FOR_FAMILY, fam) else {
        return EnsureDmPlan::Skipped {
            message: Some(tr(
                "⚙️ Login screen: automatic setup isn't available for this distribution",
            )),
            code: "skipped".to_string(),
        };
    };
    let pkgs = [pkg.to_string()];
    let install_argv = match privileged_pkg_cmd(family, "install", &pkgs, pkg_helper_available) {
        Some(argv) => argv,
        None => {
            // Raw pkexec by the family's common install table.
            let Some(tmpl) = table_get(PKG_HELPER_INSTALL, fam) else {
                return EnsureDmPlan::Skipped {
                    message: None,
                    code: "skipped".to_string(),
                };
            };
            let mut argv = vec!["pkexec".to_string()];
            argv.extend(
                tmpl.replace("{PKGS}", pkg)
                    .split_whitespace()
                    .map(str::to_string),
            );
            argv
        }
    };
    let sddm_unit = ["sddm".to_string()];
    let enable_argv =
        privileged_pkg_cmd(family, "enable-dm", &sddm_unit, pkg_helper_available)
            .unwrap_or_else(|| {
                vec![
                    "pkexec".to_string(),
                    "systemctl".to_string(),
                    "enable".to_string(),
                    "sddm.service".to_string(),
                ]
            });
    EnsureDmPlan::Install {
        message: tr("🖥️ No login screen — installing SDDM…"),
        fraction: 0.90,
        package: pkg.to_string(),
        install: Cmd::new(install_argv, Some(crate::utils::DOWNLOAD_TIMEOUT_SECS), false),
        enable: Cmd::new(enable_argv, Some(30), false),
        done_message: tr("✅ SDDM installed and enabled"),
        done_fraction: 0.92,
        code: "installed".to_string(),
    }
}

// ============================================================================
// Install odds and ends that lived in Python: AUR helper, progress shares, result texts
// ============================================================================

/// AUR helpers in order of preference: port of `_find_aur_helper`.
pub const AUR_HELPERS: &[&str] = &["yay", "paru", "trizen", "pikaur", "aurman"];

/// The first AUR helper found (Arch only), or None.
/// `has` is a live check for the binary (`shutil.which`).
pub fn pick_aur_helper(
    family: PkgFamily,
    has: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if family != PkgFamily::Arch {
        return None;
    }
    AUR_HELPERS
        .iter()
        .find(|h| has(h))
        .map(|h| h.to_string())
}

/// The Gentoo overlay this desktop needs (or None).
pub fn gentoo_overlay_for(de_id: &str) -> Option<&'static str> {
    table_get(GENTOO_OVERLAYS, de_id)
}

/// The command that installs the overlay management tool.
///
/// Assembled from two pieces of the engine: the name
/// [`GENTOO_OVERLAY_TOOL_PACKAGE`] and the [`install_cmd`] table; the choice
/// between the named and the raw path is shared ([`privileged_install_argv`]).
/// Previously all of this was rewritten by hand in `ensure_gentoo_overlay`,
/// and the copy had already diverged from the engine: it lost `--quiet`.
///
/// `None`: not Gentoo: there are no overlays there, nothing to install.
pub fn gentoo_overlay_tool_install_cmd(
    family: PkgFamily,
    helper_available: bool,
) -> Option<Vec<String>> {
    if family != PkgFamily::Gentoo {
        return None;
    }
    let packages = vec![GENTOO_OVERLAY_TOOL_PACKAGE.to_string()];
    let base = install_cmd(family, &packages).ok()?;
    Some(privileged_install_argv(family, &packages, &base, helper_available))
}

/// Refusal text when the package manager lock never got released.
pub fn pkg_lock_timeout_message(timeout_secs: u64) -> String {
    trf(
        // TRANSLATORS: %1 is a number of seconds.
        "The package manager lock wasn't released within %1 s. Another \
         transaction or a snapper snapshot may be stuck. Check with: pgrep -af \
         'pacman|yay|paru|snap-pac|snapper'",
        &[&timeout_secs],
    )
}

/// The first "(N/M)" counter in a text: port of `re.search(r"\((\d+)/(\d+)\)")`.
fn find_counter(text: &str) -> Option<(u64, u64)> {
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'(' {
            continue;
        }
        let rest = &text[i + 1..];
        let Some(close) = rest.find(')') else { continue };
        let Some((a, b2)) = rest[..close].split_once('/') else { continue };
        if a.is_empty()
            || b2.is_empty()
            || !a.bytes().all(|c| c.is_ascii_digit())
            || !b2.bytes().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        if let (Ok(cur), Ok(total)) = (a.parse(), b2.parse()) {
            return Some((cur, total));
        }
    }
    None
}

/// Desktop install progress share by stage and message.
///
/// Port of the `_pkg_cb` closure from `install_de`: package manager steps take
/// 5–65 % of the bar, post-transaction hooks 70–75 %, the AUR tail brings it
/// to 85 %. The "(N/M)" counter is taken from the ready message, the very one
/// `parse_pacman_line` assembled.
pub fn install_de_fraction(stage: &str, message: &str) -> Option<f64> {
    let ratio = || -> Option<f64> {
        let (cur, total) = find_counter(message)?;
        Some(cur as f64 / total.max(1) as f64)
    };
    match stage {
        "pkg_install" => Some(0.05 + 0.60 * ratio()?),
        "hook" => Some(0.70 + 0.05 * ratio()?),
        "done_main" => Some(0.70),
        "aur" => Some(0.75),
        "aur_done" | "aur_warn" => Some(0.85),
        _ => None,
    }
}

/// Result of the main batch install step: success with log lines, or a
/// refusal with an explanation for the person.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BatchMainOutcome {
    pub ok: bool,
    pub message: String,
    /// (stage, text) pairs for the progress callback.
    pub logs: Vec<(String, String)>,
}

/// Where the password window programs (polkit agents) live, per package file
/// lists: Arch (`polkit-gnome`, `polkit-kde-agent`, `mate-polkit`,
/// `lxqt-policykit`, `lxsession`, `xfce-polkit` from the AUR), Debian/Ubuntu
/// (`policykit-1-gnome`, `polkit-kde-agent-1`, `mate-polkit`), Fedora
/// (`polkit-gnome` up to 40, `polkit-kde`, `mate-polkit`, `lxqt-policykit`,
/// `xfce-polkit`) and openSUSE Tumbleweed. The hint names the first one found
/// on the machine.
pub const POLKIT_AGENT_PATHS: &[&str] = &[
    // polkit-gnome: Arch; Debian and Ubuntu; Fedora and openSUSE.
    "/usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1",
    "/usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1",
    "/usr/libexec/polkit-gnome-authentication-agent-1",
    // polkit-kde-agent: Arch; Debian and Ubuntu; Fedora; openSUSE.
    "/usr/lib/polkit-kde-authentication-agent-1",
    "/usr/lib/x86_64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
    "/usr/lib/aarch64-linux-gnu/libexec/polkit-kde-authentication-agent-1",
    "/usr/libexec/kf6/polkit-kde-authentication-agent-1",
    "/usr/libexec/polkit-kde-authentication-agent-1",
    // mate-polkit: Arch; Debian, Fedora, openSUSE.
    "/usr/lib/mate-polkit/polkit-mate-authentication-agent-1",
    "/usr/libexec/polkit-mate-authentication-agent-1",
    // lxqt-policykit: Arch, Debian, openSUSE; Fedora.
    "/usr/bin/lxqt-policykit-agent",
    "/usr/libexec/lxqt-policykit-agent",
    // xfce-polkit: AUR; Fedora.
    "/usr/lib/xfce-polkit/xfce-polkit",
    "/usr/libexec/xfce-polkit",
    // lxpolkit (LXDE): Arch, Debian.
    "/usr/bin/lxpolkit",
];

/// The login session polkit will attach the password window to: the
/// process's own session, and for a process outside a session the user's
/// graphical session (`auto` in logind; polkit itself chooses the same way).
pub fn login_session_argv() -> Vec<String> {
    ["loginctl", "show-session", "auto", "-p", "Id", "--value"].iter().map(|s| s.to_string()).collect()
}

/// Probe "is a password window (polkit agent) running for session `session_id`".
///
/// We ask polkit itself instead of looking for processes by name: there are
/// many agents (polkit-gnome, polkit-kde-agent, mate-polkit, xfce-polkit,
/// lxqt-policykit, lxpolkit), and the Noctalia and DankMaterialShell shells
/// keep the agent inside their quickshell process: it cannot be found by
/// name. But any agent registers with polkit per session, and polkit rejects
/// a second registration for the same session ("already exists"). That is
/// the answer. If there is no agent, the registration goes through and is
/// removed by polkit itself as soon as the `gdbus` process exits and leaves
/// the bus (the same way pkexec brings up its text agent for one command).
pub fn polkit_agent_probe_argv(session_id: &str) -> Vec<String> {
    vec![
        "gdbus".into(),
        "call".into(),
        "--system".into(),
        "--dest".into(),
        "org.freedesktop.PolicyKit1".into(),
        "--object-path".into(),
        "/org/freedesktop/PolicyKit1/Authority".into(),
        "--method".into(),
        "org.freedesktop.PolicyKit1.Authority.RegisterAuthenticationAgent".into(),
        format!("('unix-session', {{'session-id': <'{session_id}'>}})"),
        "C".into(),
        "/org/rexon/PolkitAgentProbe".into(),
    ]
}

/// Whether a session id is fit for the probe (logind gives `2`, `c1`).
pub fn login_session_id_ok(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Probe answer: `Some(true)`: an agent already exists, `Some(false)`: no
/// agent, `None`: could not ask (no `gdbus`, wrong session, etc.). polkit
/// does not translate the error text.
pub fn polkit_agent_probe_answer(rc: Option<i32>, stderr: &str) -> Option<bool> {
    match rc {
        Some(0) => Some(false),
        _ if stderr.contains("An authentication agent already exists") => Some(true),
        _ => None,
    }
}

/// Port of `_batch_main_result`: parsing the exit code of the main pkexec step.
///
/// `result` is None if the command could not even be started. The "is a
/// polkit agent running" check is called LAZILY, only in the authorization
/// refusal branch, exactly as in the original.
///
/// ⚠️ Fixed in the port: if the family is unknown, the original crashed
/// right in the error branch (`_install_cmd` raises RuntimeError instead of
/// the "install from a terminal" hint). Now instead of a crash the person
/// sees the same text as an explanation.
pub fn batch_main_result(
    result: Option<(i32, &str, &str)>,
    main_seen: bool,
    packages: &[String],
    steamos: bool,
    family: PkgFamily,
    polkit_agent_running: &dyn Fn() -> bool,
    installed_agent: &dyn Fn() -> Option<String>,
) -> BatchMainOutcome {
    let ok = |logs: Vec<(String, String)>| BatchMainOutcome {
        ok: true,
        message: String::new(),
        logs,
    };
    let fail = |message: String| BatchMainOutcome {
        ok: false,
        message,
        logs: Vec::new(),
    };
    if !main_seen || packages.is_empty() {
        return ok(Vec::new());
    }
    let Some((code, stderr, stdout)) = result else {
        return fail(tr("Command not found"));
    };
    if code != 0 {
        let err = if !stderr.is_empty() { stderr } else { stdout };
        // pkexec without a polkit agent (bare sway, niri and other tiling ones do
        // not start one): code 127 and its own phrase, no "Not authorized" words.
        let no_agent = ["No authentication agent found", "Агент аутентификации не найден"]
            .iter()
            .any(|n| err.contains(n));
        if err.contains("Not authorized") || code == 126 || no_agent {
            // The "install it by hand" hint: usually a ready command, and on an unknown
            // family an explanation of why there is no command.
            // ⚠️ Fixed in the port: the original built it BEFORE branching and on an
            // unknown family crashed with RuntimeError instead of any of the
            // explanations below, including cases where no command is needed at all.
            let hint = |lead: &str| match install_cmd(family, packages) {
                Ok(argv) => format!("{}\n  sudo {}", lead, argv.join(" ")),
                Err(e) => e,
            };
            if steamos {
                return fail(match install_cmd(family, packages) {
                    Ok(argv) => trf(
                        // TRANSLATORS: %1 is a package manager command line.
                        "The administrator password wasn't confirmed.\n\
                         You can install from a terminal:\n  \
                         sudo steamos-readonly disable\n  \
                         sudo %1\n  \
                         sudo steamos-readonly enable",
                        &[&argv.join(" ")],
                    ),
                    Err(e) => format!(
                        "{}\n{}",
                        tr("The administrator password wasn't confirmed."),
                        e
                    ),
                });
            }
            if no_agent || !polkit_agent_running() {
                let or_install = hint(&tr("Or install from a terminal:"));
                // The path is a terminal command, not a word for the person: without it
                // there would be nothing to run. We take the one found on this machine: the
                // same agent has its own path in each distribution ([`POLKIT_AGENT_PATHS`]).
                return fail(match installed_agent() {
                    Some(agent) => trf(
                        // TRANSLATORS: %1 is the path of the program to start, %2 a hint
                        // with a command to run in a terminal.
                        "The service that shows the password window isn't running.\n\n\
                         Start it yourself:\n  %1 &\n\n%2",
                        &[&agent, &or_install],
                    ),
                    None => trf(
                        // TRANSLATORS: %1 is a hint with a command to run in a terminal.
                        // polkit-gnome is a package name.
                        "The service that shows the password window isn't running.\n\n\
                         No such service is installed here. Install one for your desktop \
                         (for example, polkit-gnome) — after that Rexon will show here how \
                         to start it.\n\n%1",
                        &[&or_install],
                    ),
                });
            }
            return fail(trf(
                // TRANSLATORS: %1 is a hint with a command to run in a terminal.
                "The administrator password wasn't confirmed.\n\n\
                 Possible reasons:\n\
                 • the password window didn't appear;\n\
                 • the password was wrong;\n\
                 • the password window was closed.\n\n\
                 %1",
                &[&hint(&tr("Try installing from a terminal:"))],
            ));
        }
        // Conflict with an installed package (pacman with `--noconfirm` answered "N"
        // and removed nothing): explain instead of showing the raw English text.
        let both = format!("{stdout}\n{stderr}");
        let conflicts = parse_transaction_conflicts(&both);
        if !conflicts.is_empty() {
            return fail(conflict_explanation(&conflicts));
        }
        // No mirror served the file (network or DNS gone): pacman installed nothing.
        // To the person: what happened and what to do, not an English wall of text
        // (live case of 08.11.2026, «Check / Repair» for Budgie with the network down).
        if download_failed(&both) {
            return fail(download_failed_message());
        }
        return fail(if err.is_empty() {
            tr("Installation failed")
        } else {
            manager_error_lines(err)
        });
    }
    if steamos {
        return ok(vec![
            ("steamos_done".into(), tr("✅ Desktop installed on SteamOS")),
            (
                "steamos_warn".into(),
                tr("⚠️ A SteamOS update may remove these packages."),
            ),
            (
                "steamos_warn".into(),
                format!("   {}", tr("Tip: pin the packages with pacman -D --asexplicit")),
            ),
        ]);
    }
    ok(vec![(
        "done_main".into(),
        ntr(
            "✅ Main packages installed (%n package)",
            "✅ Main packages installed (%n packages)",
            packages.len() as u64,
        ),
    )])
}

/// Intro line of the install's AUR tail: port of the start of `_batch_run_aur`.
pub fn batch_aur_intro(steamos: bool, count: usize) -> (String, String) {
    (
        "aur".to_string(),
        if steamos {
            trf("🎨 Installing from the AUR (%1)…", &[&count])
        } else {
            ntr(
                "🎨 Installing %n package from the AUR…",
                "🎨 Installing %n packages from the AUR…",
                count as u64,
            )
        },
    )
}

/// Result of the AUR tail: what to say after trying to install packages from the AUR.
///
/// `failed` / `installed` are sorted by the live `is_installed` the executor
/// does; on SteamOS the original does not do this sorting at all, so the
/// lists are not needed there.
///
/// `timed_out`: the AUR helper did not fit into the allotted time. A timeout
/// refusal MUST name itself: "did not install" and "did not wait for the
/// build" are different news, and the person has to do different things
/// about them. On 2026-09-01 a user read "⚠️ AUR not installed (9 of 9)"
/// where in fact the build was killed after half a minute.
pub fn batch_aur_result(
    steamos: bool,
    succeeded: bool,
    total: usize,
    failed: &[String],
    installed: &[String],
    timed_out: bool,
) -> Vec<(String, String)> {
    if steamos {
        return vec![if succeeded {
            ("aur_done".into(), tr("✅ AUR packages installed"))
        } else if timed_out {
            ("aur_warn".into(), aur_timeout_message(total))
        } else {
            ("aur_warn".into(), tr("⚠️ The AUR packages didn't install"))
        }];
    }
    if succeeded {
        return vec![("aur_done".into(), tr("✅ AUR packages installed"))];
    }
    if timed_out {
        let mut out = vec![("aur_warn".to_string(), aur_timeout_message(total))];
        if !installed.is_empty() {
            out.push((
                "aur_warn".to_string(),
                format!("   {}", trf("✅ Built in time: %1", &[&installed.join(", ")])),
            ));
        }
        return out;
    }
    if failed.is_empty() {
        // yay sometimes returns a non-zero code after installing every single package.
        return vec![("aur_done".into(), tr("✅ AUR packages installed"))];
    }
    let mut out = vec![(
        "aur_warn".to_string(),
        trf(
            // TRANSLATORS: %1 of %2 packages failed; %3 lists them.
            "⚠️ Some AUR packages didn't install (%1 of %2): %3",
            &[&failed.len(), &total, &failed.join(", ")],
        ),
    )];
    if !installed.is_empty() {
        out.push((
            "aur_warn".to_string(),
            format!("   {}", fill(&trc("AUR packages", "✅ Installed: %1"), &[&installed.join(", ")])),
        ));
    }
    out
}

/// A timeout refusal as a separate text, so it cannot be confused with
/// "packages did not install". We tell the person right away how to continue by hand.
pub fn aur_timeout_message(total: usize) -> String {
    ntr(
        "⏳ The AUR build ran out of time (%n package) — this is NOT an \
         installation failure. To finish it yourself: yay -S --needed <packages>",
        "⏳ The AUR build ran out of time (%n packages) — this is NOT an \
         installation failure. To finish it yourself: yay -S --needed <packages>",
        total as u64,
    )
}

/// The progress window mark about the start of the package install: port of
/// two lines of the plan executor (`📦 Installing …` and its SteamOS variant).
///
/// ⭐ Fixed in the port: both showed the user a broken plural ("package(s)",
/// "1 packages") instead of a proper word form. The project already had
/// plural forms (the translation catalog, `ntr`), these two lines simply did
/// not use them.
pub fn pkg_install_start_message(packages: usize, steamos: bool) -> String {
    let n = packages as u64;
    if steamos {
        ntr(
            "📦 SteamOS: installing %n package…",
            "📦 SteamOS: installing %n packages…",
            n,
        )
    } else {
        ntr("📦 Installing %n package…", "📦 Installing %n packages…", n)
    }
}

// ============================================================================
// MATE post-install: data and texts (port of `_post_install_mate`)
// ============================================================================
//
// Bringing MATE to the Ubuntu MATE look is the only post-install hook in the
// project. It consists of trips outside (git clone, copying files,
// gsettings, pkexec), but everything WHAT exactly is copied and written is
// data: the "Common tasks" captions in different languages, the snap path
// map, the gschema override contents, the texts of both privileged scripts
// and the lists of packages not in the regular repositories. The data lives
// here, the trips with the executor.

/// The MATE Control Center "Common tasks" captions by UI language. The key is
/// matched by the prefix of the LANG variable, as in the original.
pub const MATE_LOCALE_ACTIONS: &[(&str, &str)] = &[
    (
        "ru",
        "['Изменить тему;mate-appearance-properties.desktop', \
'Предпочтительные приложения;mate-default-applications-properties.desktop', \
'Информация о системе MATE;mate-system-info.desktop']",
    ),
    (
        "de",
        "['Design ändern;mate-appearance-properties.desktop', \
'Bevorzugte Anwendungen;mate-default-applications-properties.desktop', \
'MATE-Systeminformation;mate-system-info.desktop']",
    ),
];

/// UI language → (locale key, cc-actions-list value), or None.
pub fn mate_locale_actions(lang: &str) -> Option<(&'static str, &'static str)> {
    MATE_LOCALE_ACTIONS
        .iter()
        .find(|(k, _)| lang.starts_with(k))
        .map(|(k, v)| (*k, *v))
}

/// Ubuntu snap build shortcuts → their usual names on Arch/CachyOS.
pub const MATE_SNAP_MAP: &[(&str, &str)] = &[
    ("firefox_firefox.desktop", "firefox.desktop"),
    ("thunderbird_thunderbird.desktop", "thunderbird.desktop"),
    ("snap-store_snap-store.desktop", "org.gnome.Software.desktop"),
    (
        "snap-store_ubuntu-mate-welcome.desktop",
        "org.gnome.Software.desktop",
    ),
    ("software-properties-drivers.desktop", "cachyos-hello.desktop"),
];

/// Rewrite snap paths in the panel layout text to the usual ones.
pub fn mate_layout_fix_snap_paths(text: &str) -> String {
    let mut out = text.to_string();
    for (snap, native) in MATE_SNAP_MAP {
        out = out.replace(
            &format!("/var/lib/snapd/desktop/applications/{}", snap),
            &format!("/usr/share/applications/{}", native),
        );
    }
    out
}

/// Where the Rexon gschema override goes.
pub const MATE_GSCHEMA_PATH: &str =
    "/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override";

// ── MATE defaults are built ONLY from what is really on disk ──
//
// WHY. The override set `Yaru-MATE-dark` as the default theme BEFORE the
// theme package was even tried, and did not check the result. The package
// `yaru-mate-gtk-theme` does not exist in the AUR at all, the install
// failed, and the desktop stayed configured for a nonexistent theme (defect
// of 2026-09-01). The class is wider than one theme: nothing we did not
// install may be written into the defaults.
//
// So each theme name is chosen from a preference chain against the LIST OF
// WHAT IS REALLY PRESENT (gathered by the executor by walking the theme
// directories), and each substitution is said out loud to the person.
//
// ⚠️ Marco has a separate chain, and this is not nitpicking: the
// `Yaru-MATE-dark` theme has no `metacity-1` directory: it is not a theme
// for the window manager at all, and the old override asked Marco for
// something that does not exist even after a successful install.

/// GTK look, in descending preference.
pub const MATE_GTK_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-MATE", "Yaru-dark", "Yaru",
    "BlackMATE", "Menta", "Adwaita-dark", "Adwaita",
];

/// Window frame look (Marco). Needs a `metacity-1` directory.
pub const MATE_MARCO_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-dark", "YaruOk", "YaruGreen",
    "BlackMATE", "Menta", "TraditionalOk", "Adwaita",
];

/// Icon set.
pub const MATE_ICON_THEME_CHAIN: &[&str] = &[
    "Yaru-MATE-dark", "Yaru-MATE", "Yaru", "menta", "mate", "Adwaita",
];

/// Mouse pointer.
pub const MATE_CURSOR_THEME_CHAIN: &[&str] = &["Yaru", "mate", "Adwaita"];

/// The first PRESENT name from a preference chain.
///
/// The SINGLE point answering "what of what we want do we really have".
/// Introduced instead of a scatter of `if os.path.isdir(...)`: a missed check
/// leaves a default pointing into the void, and the person sees a broken
/// desktop without a single word about the cause.
pub fn first_present<'a>(chain: &[&'a str], available: &[String]) -> Option<&'a str> {
    chain
        .iter()
        .find(|want| available.iter().any(|have| have == *want))
        .copied()
}

/// What can really be written into the MATE defaults: gathered by the
/// executor by walking directories AFTER the packages are installed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MateLookInput {
    /// Names of themes with a `gtk-3.0` directory.
    pub gtk_themes: Vec<String>,
    /// Names of themes with a `metacity-1` directory (Marco understands them).
    pub marco_themes: Vec<String>,
    pub icon_themes: Vec<String>,
    pub cursor_themes: Vec<String>,
    /// Whether the Ubuntu font family is installed.
    pub ubuntu_font: bool,
}

/// The verified MATE look + what had to be replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MateLook {
    pub gtk_theme: Option<String>,
    pub marco_theme: Option<String>,
    pub icon_theme: Option<String>,
    pub cursor_theme: Option<String>,
    pub ubuntu_font: bool,
    /// Lines for the person about each substitution and each omission.
    pub notes: Vec<String>,
}

/// Choose the look from what is present and explain each replacement.
pub fn mate_look(input: &MateLookInput) -> MateLook {
    let mut notes = Vec::new();
    let mut pick = |what: String, chain: &[&str], available: &[String]| -> Option<String> {
        let want = chain[0];
        match first_present(chain, available) {
            Some(got) if got == want => Some(got.to_string()),
            Some(got) => {
                notes.push(format!(
                    "  {}",
                    // TRANSLATORS: %1 is a part of the look (theme, icons…),
                    // %2 the wanted theme, %3 the one kept instead.
                    trf("⚠️ %1: “%2” isn't installed — keeping “%3”", &[&what, &want, &got])
                ));
                Some(got.to_string())
            }
            None => {
                notes.push(format!(
                    "  {}",
                    trf(
                        "⚠️ %1: “%2” isn't installed and there's no replacement \
                         either — keeping the MATE default",
                        &[&what, &want],
                    )
                ));
                None
            }
        }
    };
    let gtk_theme = pick(trc("MATE look part", "Theme"), MATE_GTK_THEME_CHAIN, &input.gtk_themes);
    let marco_theme = pick(
        trc("MATE look part", "Window borders"),
        MATE_MARCO_THEME_CHAIN,
        &input.marco_themes,
    );
    let icon_theme = pick(trc("MATE look part", "Icons"), MATE_ICON_THEME_CHAIN, &input.icon_themes);
    let cursor_theme = pick(
        trc("MATE look part", "Pointer"),
        MATE_CURSOR_THEME_CHAIN,
        &input.cursor_themes,
    );
    if !input.ubuntu_font {
        notes.push(format!(
            "  {}",
            tr("⚠️ Fonts: the Ubuntu font family isn't installed — keeping the MATE font")
        ));
    }
    MateLook {
        gtk_theme,
        marco_theme,
        icon_theme,
        cursor_theme,
        ubuntu_font: input.ubuntu_font,
        notes,
    }
}

/// MATE settings Rexon sets for the user: (schema, key, value), exactly
/// those that used to go into the system-wide schema override.
///
/// WHY FOR THE USER AND NOT THE SYSTEM. Previously these same values went into
/// `/usr/share/glib-2.0/schemas/90_rexon-mate-settings.gschema.override`, a
/// root file owned by no package. It affected ALL environments and all people
/// at the machine, could not be undone by switching desktops, and survived
/// the removal of Rexon itself: the person uninstalled the program, and its
/// trace stayed in the system forever. A desktop setting is about the user,
/// not the machine, so we write into the user's own layer (`gsettings`),
/// where any value is undone by the stock `gsettings reset`.
///
/// `look` is an ALREADY VERIFIED look: names of nonexistent themes do not
/// appear here at all.
pub fn mate_user_settings(look: &MateLook) -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut put = |schema: &str, key: &str, value: &str| {
        out.push((schema.to_string(), key.to_string(), value.to_string()));
    };
    if let Some(t) = &look.gtk_theme {
        put("org.mate.interface", "gtk-theme", t);
    }
    if let Some(t) = &look.icon_theme {
        put("org.mate.interface", "icon-theme", t);
    }
    if look.ubuntu_font {
        put("org.mate.interface", "font-name", "Ubuntu 11");
        put("org.mate.interface", "document-font-name", "Ubuntu 11");
        put("org.mate.interface", "monospace-font-name", "Ubuntu Mono 13");
    }
    put(
        "org.mate.interface",
        "gtk-decoration-layout",
        "close,minimize,maximize:",
    );
    if let Some(t) = &look.marco_theme {
        put("org.mate.Marco.general", "theme", t);
    }
    if look.ubuntu_font {
        put("org.mate.Marco.general", "titlebar-font", "Ubuntu Bold 11");
    }
    put("org.mate.Marco.general", "compositing-manager", "true");
    put("org.mate.Marco.general", "center-new-windows", "false");
    put("org.mate.Marco.general", "allow-tiling", "true");
    if let Some(t) = &look.cursor_theme {
        put("org.mate.peripherals-mouse", "cursor-theme", t);
        put("org.mate.peripherals-mouse", "cursor-size", "24");
    }
    put("org.mate.background", "show-desktop-icons", "true");
    put("org.mate.caja.desktop", "computer-icon-visible", "true");
    put("org.mate.caja.desktop", "home-icon-visible", "true");
    put("org.mate.caja.desktop", "trash-icon-visible", "true");
    put("org.mate.panel", "default-layout", "familiar");
    put(
        "org.mate.session",
        "required-components-list",
        "['windowmanager', 'panel', 'filemanager']",
    );
    put("org.mate.session.required-components", "windowmanager", "marco");
    put("org.mate.session.required-components", "panel", "mate-panel");
    put("org.mate.session.required-components", "filemanager", "caja");
    put("org.mate.NotificationDaemon", "theme", "slider");
    if look.ubuntu_font {
        put("org.mate.pluma", "editor-font", "Ubuntu Mono 13");
        put("org.mate.pluma", "use-default-font", "false");
        put(
            "org.mate.terminal.profile:/org/mate/terminal/profiles/default/",
            "font",
            "Ubuntu Mono 13",
        );
        put(
            "org.mate.terminal.profile:/org/mate/terminal/profiles/default/",
            "use-system-font",
            "false",
        );
    }
    out
}

/// argv of the `gsettings set` calls for `mate_user_settings`.
pub fn mate_user_settings_argv(look: &MateLook) -> Vec<Vec<String>> {
    mate_user_settings(look)
        .into_iter()
        .map(|(schema, key, value)| {
            vec!["gsettings".to_string(), "set".to_string(), schema, key, value]
        })
        .collect()
}

/// Privileged cleanup of a FOREIGN trace of past versions: the system-wide
/// schema override Rexon used to put before moving to the user layer.
///
/// It cannot be left even as harmless: the file belongs to no package, it
/// declares defaults for the whole machine and survives the product's removal.
pub fn mate_gschema_cleanup_script() -> String {
    format!(
        "#!/bin/bash\nset -e\nrm -f '{}'\n\
glib-compile-schemas /usr/share/glib-2.0/schemas/\n",
        MATE_GSCHEMA_PATH
    )
}

/// Raw (no named action) script that places panel files into the system
/// directory. All paths are escaped: file names come from a foreign
/// repository and must not be substituted into bash as is under root.
pub fn mate_layouts_stage_script(dst: &str, staged_files: &[String]) -> String {
    let qdst = shlex_quote(dst);
    let mut lines = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("install -d {}", qdst),
    ];
    for f in staged_files {
        lines.push(format!(
            "cp -f {} {}",
            shlex_quote(f),
            shlex_quote(&format!("{}/", dst))
        ));
    }
    lines.push(format!("chmod -R a+rX {}", qdst));
    format!("{}\n", lines.join("\n"))
}

/// Packages "beyond the distribution" that are not in the regular
/// repositories: (label for the error log, step title as a catalog `msgid`,
/// translated where shown, the package list).
pub const MATE_EXTRA_PACKAGES: &[(&str, &str, &[&str])] = &[
    // ⛔ The old names `yaru-mate-gtk-theme` / `yaru-mate-icon-theme` do NOT
    // exist, neither in the AUR nor in the repositories (checked via RPC
    // 2026-09-01, resultcount 0). The install failed every time, while the
    // override had already declared `Yaru-MATE-dark` the default theme. The real
    // providers of Ubuntu MATE themes in the AUR are `ubuntu-mate-themes`
    // (gives /usr/share/themes/Yaru-MATE, Yaru-MATE-dark, Yaru-MATE-light) and
    // `ubuntu-mate-icon-themes` (gives Yaru, Yaru-MATE-dark, Yaru-MATE-light).
    // The Yaru-MATE icons inherit from Humanity: without `humanity-icon-theme`
    // the set is almost empty.
    (
        "yaru-mate",
        n_("🎨 Checking the Ubuntu MATE themes (Yaru-MATE)…"),
        &["ubuntu-mate-themes", "ubuntu-mate-icon-themes", "humanity-icon-theme"],
    ),
    (
        "ayatana",
        n_("🔔 Checking the status icons in the panel (Ayatana)…"),
        &[
            "ayatana-indicator-application",
            "ayatana-indicator-session",
            "ayatana-indicator-datetime",
            "ayatana-indicator-sound",
            "ayatana-indicator-notifications",
        ],
    ),
    (
        "vala-panel-appmenu",
        n_("📋 Checking the global menu in the panel (vala-panel-appmenu)…"),
        // Not the common `vala-panel-appmenu`: it pulls in budgie-desktop and vala-panel.
        &["vala-panel-appmenu-mate", "vala-panel-appmenu-registrar"],
    ),
    // The MATE Menu (the Ubuntu MATE menu) — in place of the Brisk menu, which
    // Rexon no longer installs (Wehrwolfmann, 24.11.2026). A group of its own on
    // purpose: the AUR package has been failing to install (26.10.1 ships
    // `gschemas.compiled`, which conflicts with the file on disk), and in a shared
    // transaction its failure would take the other packages down with it. If it
    // fails, nothing is lost: the stock MATE panel uses the built-in menu, and
    // the layouts put the built-in menu button in its place (`mate_panel::prepare`).
    (
        "mate-menu",
        n_("📋 Checking the MATE Menu (mate-menu)…"),
        &["mate-menu"],
    ),
];

/// Log lines about the written look: they list what was really written, not
/// what we wanted to write.
pub fn mate_gschema_success_lines(look: &MateLook) -> Vec<String> {
    let mut out = vec![format!(
        "  {}",
        tr("✅ The MATE look was saved to your user settings:")
    )];
    let mut say = |label: &str, value: &Option<String>| {
        if let Some(v) = value {
            out.push(format!("     • {label}: {v}"));
        }
    };
    say("GTK", &look.gtk_theme);
    say(&trc("MATE look summary", "Icons"), &look.icon_theme);
    if let Some(c) = &look.cursor_theme {
        out.push(format!("     • {}", trf("Cursor: %1 (24px)", &[&c])));
    }
    if look.ubuntu_font {
        out.push(format!("     • {}", tr("Font: Ubuntu 11 / Ubuntu Mono 13")));
    }
    if let Some(m) = &look.marco_theme {
        out.push(format!("     • {}", trf("WM: Marco with compositing, theme %1", &[&m])));
    } else {
        out.push(format!("     • {}", tr("WM: Marco with compositing")));
    }
    out.push(format!("     • {}", tr("Default layout: Familiar")));
    out
}

/// Result of the MATE post-install: log lines and the result text.
pub fn mate_post_install_summary(errors: usize) -> (Vec<String>, String) {
    if errors > 0 {
        return (
            vec![format!(
                "\n{}",
                ntr(
                    "⚠️ MATE post-install: %n warning (not critical)",
                    "⚠️ MATE post-install: %n warnings (not critical)",
                    errors as u64,
                )
            )],
            ntr("Partly done (%n warning)", "Partly done (%n warnings)", errors as u64),
        );
    }
    (
        vec![
            format!("\n{}", tr("✅ MATE post-install complete — the full Ubuntu MATE!")),
            // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
            format!("   → {}", tr("8 layouts: mate-tweak → Panel")),
            "   → Familiar | Redmond | Cupertino | Mutiny".into(),
            "   → Pantheon | Contemporary | Netbook | Traditional".into(),
            format!("   → {}", tr("Yaru-MATE theme + Ubuntu fonts + Ayatana")),
            "   → CLI: mate-tweak --layout redmond".into(),
        ],
        tr("MATE post-install complete"),
    )
}

/// MATE post-install log line: the lock screen shows the desktop wallpaper
/// instead of the stock "stripes".
pub fn mate_lock_follows_desktop_line() -> String {
    tr("🔒 Lock screen: shows your desktop wallpaper")
}

// ============================================================================
// river post-install: the starter init
// ============================================================================
//
// river-classic without an executable `$XDG_CONFIG_HOME/river/init` starts
// WITHOUT A SINGLE key binding: no terminal, no exit, only Ctrl+Alt+F1…F12.
// river-classic puts its example init into /usr/share as a template and does
// not read it itself. So after the install Rexon puts its own starter init,
// but only if the file does not exist yet: other people's (and our own
// earlier) settings are never touched.

/// Where river looks for init: `$XDG_CONFIG_HOME/river/init`, without it
/// `$HOME/.config/river/init` (river(1), the CONFIGURATION section).
pub fn river_init_path(xdg_config_home: &str, home: &str) -> String {
    let base = if xdg_config_home.is_empty() {
        format!("{}/.config", home.trim_end_matches('/'))
    } else {
        xdg_config_home.trim_end_matches('/').to_string()
    };
    format!("{base}/river/init")
}

/// Permissions of the starter init: river runs it as a program.
pub const RIVER_INIT_MODE: u32 = 0o755;

/// The river starter init: keys like the neighbouring tiling ones
/// (Super+Enter: terminal, Super+D: app menu, Super+Q: close window,
/// Super+Shift+Esc: lock the screen, as in the Wayfire example), the waybar
/// panel, mako notifications, the polkit password window and the rivertile
/// layout. Locking on idle (5 minutes), before sleep and on
/// `loginctl lock-session` is handled by swayidle: the same [`SWAYIDLE_LOCK`]
/// line as for Wayfire. The programs are exactly those the `river` entry of
/// the desktop database installs.
///
/// river-classic does not set `XDG_CURRENT_DESKTOP` itself, but
/// xdg-desktop-portal picks the portal by it (wlr: `UseIn=…;river;…`) and
/// Rexon recognises the desktop by it. So the name also goes into the user
/// services' environment.
pub fn river_starter_init() -> &'static str {
    r#"#!/bin/sh
# Starter river setup written by Rexon. It is yours now: change it freely,
# Rexon never overwrites an existing init. Reference: riverctl(1), rivertile(1).

export XDG_CURRENT_DESKTOP=river
dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP=river 2>/dev/null

# Super+Enter: terminal, Super+D: applications, Super+Q: close the window,
# Super+Shift+E: log out, Super+Shift+Escape: lock the screen.
riverctl map normal Super Return spawn foot
riverctl map normal Super D spawn fuzzel
riverctl map normal Super Q close
riverctl map normal Super+Shift E exit
riverctl map normal Super+Shift Escape spawn 'swaylock -f'

riverctl map normal Super J focus-view next
riverctl map normal Super K focus-view previous
riverctl map normal Super+Shift J swap next
riverctl map normal Super+Shift K swap previous
riverctl map normal Super+Shift Return zoom
riverctl map normal Super H send-layout-cmd rivertile "main-ratio -0.05"
riverctl map normal Super L send-layout-cmd rivertile "main-ratio +0.05"
riverctl map normal Super Space toggle-float
riverctl map normal Super F toggle-fullscreen
riverctl map-pointer normal Super BTN_LEFT move-view
riverctl map-pointer normal Super BTN_RIGHT resize-view

# Super+1..9: show a tag, Super+Shift+1..9: move the window there,
# Super+0: show everything.
for i in 1 2 3 4 5 6 7 8 9; do
    tags=$((1 << (i - 1)))
    riverctl map normal Super "$i" set-focused-tags "$tags"
    riverctl map normal Super+Shift "$i" set-view-tags "$tags"
done
riverctl map normal Super 0 set-focused-tags $(((1 << 32) - 1))

for mode in normal locked; do
    riverctl map $mode None XF86AudioRaiseVolume spawn 'wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+'
    riverctl map $mode None XF86AudioLowerVolume spawn 'wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-'
    riverctl map $mode None XF86AudioMute spawn 'wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle'
done

riverctl set-repeat 50 300
riverctl background-color 0x1e1e2e
riverctl border-color-focused 0x89b4fa
riverctl border-color-unfocused 0x45475a

riverctl spawn waybar
riverctl spawn mako
# Lock the screen after 5 idle minutes and before sleep.
riverctl spawn "swayidle -w timeout 300 'swaylock -f' before-sleep 'swaylock -f' lock 'swaylock -f'"
for agent in /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1 \
             /usr/libexec/polkit-gnome-authentication-agent-1; do
    if [ -x "$agent" ]; then
        riverctl spawn "$agent"
        break
    fi
done

riverctl default-layout rivertile
rivertile -view-padding 6 -outer-padding 6 &
"#
}

/// Log line: the river starter init was written.
pub fn river_init_written(path: &str) -> String {
    // TRANSLATORS: %1 is a file path.
    trf("✅ River: starter settings written (%1) — Super+Enter opens a terminal, Super+D the app menu", &[&path])
}

/// Log line: a river init already existed and was left as is.
pub fn river_init_kept(path: &str) -> String {
    // TRANSLATORS: %1 is a file path.
    trf("ℹ️ River: your own settings are kept (%1)", &[&path])
}

/// Log line: writing the river starter init failed.
pub fn river_init_failed(path: &str, why: &str) -> String {
    // TRANSLATORS: %1 is a file path, %2 the reason.
    trf("⚠️ River: couldn't write the starter settings (%1): %2. Without them River starts with no keyboard shortcuts.", &[&path, &why])
}

#[cfg(test)]
mod river_init_tests {
    use super::*;

    #[test]
    fn init_path_follows_xdg_config_home() {
        assert_eq!(river_init_path("", "/home/t"), "/home/t/.config/river/init");
        assert_eq!(river_init_path("", "/home/t/"), "/home/t/.config/river/init");
        assert_eq!(river_init_path("/x/cfg/", "/home/t"), "/x/cfg/river/init");
    }

    /// The starter init calls only what the `river` entry of the desktop
    /// database installs (on Arch), and `rivertile`/`riverctl` from river-classic itself.
    #[test]
    fn starter_init_uses_only_installed_programs() {
        let init = river_starter_init();
        assert!(init.starts_with("#!/bin/sh\n"));
        let arch = crate::system::get_packages("river", PkgFamily::Arch);
        for program in ["foot", "fuzzel", "waybar", "mako", "polkit-gnome", "swaylock", "swayidle"] {
            assert!(init.contains(program.split('-').next().unwrap()), "{program}");
            assert!(arch.iter().any(|p| p == program), "{program} не ставится с River");
        }
        assert!(arch.iter().any(|p| p == "river-classic"));
        assert!(init.contains("riverctl default-layout rivertile"));
        assert!(init.contains("export XDG_CURRENT_DESKTOP=river"));
        // There is always a way to log out, otherwise there is no getting out of River.
        assert!(init.contains("Super+Shift E exit"));
        // Locking: by key and on idle, with the same command as for Wayfire.
        assert!(init.contains(&format!("Super+Shift Escape spawn '{LOCK_COMMAND}'\n")));
        assert!(init.contains(&format!("riverctl spawn \"{SWAYIDLE_LOCK}\"\n")));
    }
}

// ============================================================================
// Post-install: screen lock (Wayfire, LXQt)
// ============================================================================
//
// Someone else's config is edited only by APPENDING and only where the
// person has not set up locking yet: their own key, locker or swayidle stay
// as they are.

/// What to lock the screen with in wlroots sessions: swaylock goes to the
/// background once the screen is locked (`-f`): the caller (a binding,
/// swayidle, lxqt-leave) does not have to wait for unlocking, and exit code 0
/// means "locked".
pub const LOCK_COMMAND: &str = "swaylock -f";

/// Locking on idle (5 minutes), before sleep and on `loginctl lock-session`
/// (the `lock` event). `-w`: sleep waits until the screen is locked.
pub const SWAYIDLE_LOCK: &str =
    "swayidle -w timeout 300 'swaylock -f' before-sleep 'swaylock -f' lock 'swaylock -f'";

/// The Wayfire lock key: the same as in the `wayfire.ini` example from
/// Wayfire's own documentation.
pub const WAYFIRE_LOCK_BINDING: &str = "<super> <shift> KEY_ESC";

/// LXQt X11 locking: «Lock» and Ctrl+Alt+L reach xscreensaver, which
/// lxqt-session starts itself. An explicit command rather than the default
/// `xdg-screensaver lock`: that one picks the locker by whose name is owned
/// on the bus, and with mate-screensaver running it would go to it.
pub const LXQT_X11_LOCK_COMMAND: &str = "xscreensaver-command -lock";

/// Mark of appended lines in someone else's config.
const LOCK_HEADER: &str = "Screen lock (added by Rexon; change freely)";

fn ini_section_name(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

/// `(key, value)` of an INI line; comments and lines without `=` are `None`.
fn ini_pair(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with('#') || t.starts_with(';') {
        return None;
    }
    let (k, v) = t.split_once('=')?;
    Some((k.trim(), v.trim()))
}

/// `(key, value)` lines of the `[section]` section (all occurrences of the section).
fn ini_section_pairs<'a>(text: &'a str, section: &str) -> Vec<(&'a str, &'a str)> {
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(s) = ini_section_name(line) {
            inside = s == section;
            continue;
        }
        if inside {
            if let Some(pair) = ini_pair(line) {
                out.push(pair);
            }
        }
    }
    out
}

/// Insert lines at the start of the `[section]` section (the first
/// occurrence), or, if there is no such section, as a new section at the end.
fn ini_insert(text: &str, section: &str, new: &[String]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    match lines.iter().position(|l| ini_section_name(l) == Some(section)) {
        Some(h) => {
            lines.splice(h + 1..h + 1, new.iter().cloned());
        }
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.extend(new.iter().cloned());
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Some lock program in the command line.
fn mentions_locker(value: &str) -> bool {
    const LOCKERS: &[&str] = &["swaylock", "wf-locker", "gtklock", "waylock", "hyprlock", "loginctl lock-session"];
    LOCKERS.iter().any(|l| value.contains(l))
}

/// Screen lock in `wayfire.ini`: the Super+Shift+Esc key in `[command]` and
/// swayidle in `[autostart]`. Each part only if the person has not set it
/// themselves (their own lock key/command or their own swayidle and the
/// `idle` key). `None`: nothing to append, the file is not touched.
///
/// Without this, on a clean machine Wayfire has no locking at all: its
/// defaults (`command.xml`) have no bindings, and the swaylock example is
/// only in the documentation.
pub fn wayfire_lock_edit(ini: &str) -> Option<String> {
    let command = ini_section_pairs(ini, "command");
    let has_binding = command
        .iter()
        .any(|(k, v)| matches!(*k, "binding_lock" | "command_lock") || (k.starts_with("command_") && mentions_locker(v)));
    let autostart = ini_section_pairs(ini, "autostart");
    let has_idle = autostart.iter().any(|(k, v)| *k == "idle" || v.contains("swayidle"));
    let mut text = ini.to_string();
    if !has_binding {
        text = ini_insert(
            &text,
            "command",
            &[
                format!("# {LOCK_HEADER}"),
                format!("binding_lock = {WAYFIRE_LOCK_BINDING}"),
                format!("command_lock = {LOCK_COMMAND}"),
            ],
        );
    }
    if !has_idle {
        text = ini_insert(&text, "autostart", &[format!("# {LOCK_HEADER}"), format!("idle = {SWAYIDLE_LOCK}")]);
    }
    (text != ini).then_some(text)
}

/// Lock commands in `~/.config/lxqt/session.conf` (the `[General]` section,
/// where the «LXQt Session Settings» window writes them too): `lock_command`
/// for X11 and `lock_command_wayland` for Wayland. LXQt takes the file's
/// other keys from the system `session.conf`, so a file of only these lines
/// is valid. A command set by the person is not touched; `None`: nothing to write.
///
/// Without this «Lock» did not work in any session: in X11 it failed on
/// `xdg-screensaver lock` without xscreensaver, in Wayland
/// `lock_command_wayland` is empty, and LXQt silently did nothing.
pub fn lxqt_lock_edit(existing: Option<&str>) -> Option<String> {
    let text = existing.unwrap_or("");
    let general = ini_section_pairs(text, "General");
    let set = |key: &str| general.iter().any(|(k, v)| *k == key && !v.is_empty());
    let mut out = existing.map(str::to_string);
    for (key, value) in [("lock_command", LXQT_X11_LOCK_COMMAND), ("lock_command_wayland", LOCK_COMMAND)] {
        if !set(key) {
            out = Some(crate::defaults::set_ini_value(out.as_deref(), "[General]", key, value));
        }
    }
    out.filter(|t| Some(t.as_str()) != existing)
}

// ── Tiling desktops: sway, Hyprland, niri ──────────────────────────────────
//
// Same config the keyboard layout edits (`keyboard_layout::load_config`):
// the user's main file, or, when missing, `include` of the system config for
// sway and the package sample for Hyprland and niri. The user's own settings
// are looked for in included files too.

/// sway lock key — the same Super+Shift+Esc as on Wayfire and River (free
/// in `/etc/sway/config`). `Mod4`, not `$mod`: the user's variable may be
/// unset or mean something else.
pub const SWAY_LOCK_BINDING: &str = "Mod4+Shift+Escape";

/// Hyprland's locker and idle daemon.
pub const HYPR_LOCK_COMMAND: &str = "hyprlock";
pub const HYPR_IDLE_COMMAND: &str = "hypridle";

/// Idle daemons: any of them in the config means idle locking is the user's own.
const IDLE_DAEMONS: &[&str] = &["swayidle", "hypridle"];

fn mentions_idle(code: &str) -> bool {
    IDLE_DAEMONS.iter().any(|d| code.contains(d))
}

/// Live (non-comment) lines of all texts.
fn live_lines<'a>(texts: &[&'a str], code: fn(&str) -> &str) -> Vec<&'a str> {
    texts.iter().flat_map(|t| t.lines()).map(code).filter(|c| !c.is_empty()).collect()
}

/// A key combination as a set: `Mod4+Shift+Escape` → {escape, mod4, shift};
/// `$mod`, `$mainMod`, `SUPER` are the same Mod4.
fn key_combo(keys: &str, seps: &[char]) -> Vec<String> {
    let mut v: Vec<String> = keys
        .split(|c: char| seps.contains(&c))
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty())
        .map(|k| if matches!(k.as_str(), "$mod" | "$mainmod" | "super" | "mainmod" | "win") { "mod4".to_string() } else { k })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn is_super_shift_escape(combo: &[String]) -> bool {
    combo == ["escape", "mod4", "shift"]
}

/// Screen lock in the sway config: the Super+Shift+Esc key and swayidle
/// (5 minutes idle, before sleep, `loginctl lock-session`). Each part only
/// if the user hasn't set it: the key — if there is neither a binding to a
/// locker nor an own Super+Shift+Esc; swayidle — if it isn't in the config
/// and `idle_elsewhere` (an own systemd unit) is false. `None` — nothing to
/// add.
///
/// Without this sway doesn't lock at all on a clean machine:
/// `/etc/sway/config` has no binding, and swayidle is only in a commented
/// out example.
pub fn sway_lock_edit(main: &str, included: &[String], idle_elsewhere: bool) -> Option<String> {
    let mut all: Vec<&str> = vec![main];
    all.extend(included.iter().map(String::as_str));
    let lines = live_lines(&all, crate::keyboard_layout::hash_code);
    let has_binding = lines.iter().any(|l| {
        let mut words = l.split_whitespace();
        let Some(first) = words.next() else { return false };
        if first != "bindsym" && first != "bindcode" {
            return false;
        }
        let combo = words.find(|w| !w.starts_with("--")).map(|k| key_combo(k, &['+'])).unwrap_or_default();
        mentions_locker(l) || is_super_shift_escape(&combo)
    });
    let has_idle = idle_elsewhere || lines.iter().any(|l| mentions_idle(l));
    let mut block = Vec::new();
    if !has_binding {
        block.push(format!("bindsym {SWAY_LOCK_BINDING} exec {LOCK_COMMAND}"));
    }
    if !has_idle {
        block.push(format!("exec {SWAYIDLE_LOCK}"));
    }
    if block.is_empty() {
        return None;
    }
    let snippet = format!("# {LOCK_HEADER}\n{}\n", block.join("\n"));
    Some(crate::keyboard_layout::append(main, &snippet))
}

/// What to add to the Hyprland config for the lock. `idle` — whether the
/// hypridle autostart was added: then `hypridle.conf` ([`hypridle_conf`])
/// is needed next to it unless the user has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyprLockEdit {
    pub text: String,
    pub idle: bool,
}

/// Screen lock in the Hyprland config (`hyprland.conf`, or `hyprland.lua`
/// with `lua`): Super+Shift+Esc → hyprlock and hypridle started at login.
/// Same "user's own" rules as [`sway_lock_edit`]; `None` — nothing to add.
///
/// The Hyprland sample config has neither a hyprlock binding nor a
/// hypridle autostart: the packages are installed, yet the screen locks
/// neither by key nor when idle.
pub fn hypr_lock_edit(lua: bool, main: &str, included: &[String], idle_elsewhere: bool) -> Option<HyprLockEdit> {
    let mut all: Vec<&str> = vec![main];
    all.extend(included.iter().map(String::as_str));
    let code = if lua { crate::keyboard_layout::lua_code } else { crate::keyboard_layout::hash_code };
    let lines = live_lines(&all, code);
    let has_binding = lines.iter().any(|l| {
        if lua {
            if !l.contains("hl.bind") {
                return false;
            }
            let low = l.to_ascii_lowercase();
            mentions_locker(l) || (low.contains("escape") && low.contains("shift") && (low.contains("super") || low.contains("mainmod")))
        } else {
            let Some((k, v)) = l.split_once('=') else { return false };
            if !k.trim().starts_with("bind") {
                return false;
            }
            let mut fields = v.split(',');
            let mods = fields.next().unwrap_or("");
            let key = fields.next().unwrap_or("");
            let mut combo = key_combo(mods, &[' ', '_', '+', '&']);
            combo.push(key.trim().to_ascii_lowercase());
            combo.sort();
            combo.dedup();
            mentions_locker(v) || is_super_shift_escape(&combo)
        }
    });
    let has_idle = idle_elsewhere || lines.iter().any(|l| mentions_idle(l));
    let mut block = Vec::new();
    if !has_binding {
        block.push(if lua {
            format!("hl.bind(\"SUPER + SHIFT + Escape\", hl.dsp.exec_cmd(\"{HYPR_LOCK_COMMAND}\"))")
        } else {
            format!("bind = SUPER SHIFT, Escape, exec, {HYPR_LOCK_COMMAND}")
        });
    }
    if !has_idle {
        block.push(if lua {
            format!("hl.on(\"hyprland.start\", function()\n    hl.exec_cmd(\"{HYPR_IDLE_COMMAND}\")\nend)")
        } else {
            format!("exec-once = {HYPR_IDLE_COMMAND}")
        });
    }
    if block.is_empty() {
        return None;
    }
    let mark = if lua { "--" } else { "#" };
    let snippet = format!("{mark} {LOCK_HEADER}\n{}\n", block.join("\n"));
    Some(HyprLockEdit { text: crate::keyboard_layout::append(main, &snippet), idle: !has_idle })
}

/// Minimal `hypridle.conf`: lock after 5 minutes idle, before sleep and on
/// `loginctl lock-session` (the `lock` event, which `before_sleep_cmd` also
/// sends). Without a file hypridle doesn't know what to do. The screen comes
/// back with the first key press after sleep: the `dpms` lines of the
/// hypridle sample depend on the Hyprland version, so they are left out.
pub fn hypridle_conf() -> String {
    format!(
        "# {LOCK_HEADER}\n\
general {{\n    lock_cmd = pidof {HYPR_LOCK_COMMAND} || {HYPR_LOCK_COMMAND}\n    before_sleep_cmd = loginctl lock-session\n}}\n\n\
listener {{\n    timeout = 300\n    on-timeout = loginctl lock-session\n}}\n"
    )
}

// ── Lock screen look: swaylock and hyprlock ────────────────────────────────
//
// Without a config swaylock shows a plain white screen (its default color is
// `ffffff`) and the ring appears only while typing; hyprlock shows a blurred
// screenshot. Rexon writes its own look only when there is no config at all:
// neither the user's nor the distribution's (`/etc`), so a chosen look is
// never replaced. The background is the dark color of the `steam_dark` skin:
// on the tiling desktops Rexon does not keep the path of the current
// wallpaper (swaybg/swww hold it only while running), so there is no picture
// to show.

/// Mark of a lock look file written by Rexon.
const LOOK_HEADER: &str = "Lock screen look (written by Rexon; change freely)";

/// A `steam_dark` skin color as bare `rrggbb` (the form both lockers take).
/// The source is [`crate::ui_base::APPEARANCE_STEAM_COLORS`].
fn skin_hex(token: &str) -> &'static str {
    crate::ui_base::APPEARANCE_STEAM_COLORS
        .iter()
        .find(|(k, _)| *k == token)
        .map(|(_, v)| v.trim_start_matches('#'))
        .unwrap_or("000000")
}

/// Every file swaylock reads its options from, in its own search order
/// (swaylock(1), FILES): `~/.swaylock/config`, `$XDG_CONFIG_HOME/swaylock/config`,
/// `/etc/swaylock/config`. Any of them present — the look is someone's choice.
pub fn swaylock_config_paths(home: &str, config_home: &str) -> Vec<String> {
    vec![
        format!("{}/.swaylock/config", home.trim_end_matches('/')),
        format!("{}/swaylock/config", config_home.trim_end_matches('/')),
        "/etc/swaylock/config".to_string(),
    ]
}

/// Every file hyprlock may take as its config: `$XDG_CONFIG_HOME/hypr/hyprlock.conf`
/// and the system one in `XDG_CONFIG_DIRS` (`/etc/xdg/hypr/hyprlock.conf`).
pub fn hyprlock_config_paths(config_home: &str) -> Vec<String> {
    vec![
        format!("{}/hypr/hyprlock.conf", config_home.trim_end_matches('/')),
        "/etc/xdg/hypr/hyprlock.conf".to_string(),
    ]
}

/// `~/.config/swaylock/config`: the dark skin background and a ring that is
/// visible all the time (`indicator-idle-visible`), so the locked screen
/// clearly asks for the password. Lines are swaylock's long options without
/// `--`; only options plain swaylock has had since 1.5 are used (an unknown
/// option makes swaylock refuse to start, and the screen would not lock).
pub fn swaylock_config() -> String {
    let bg = skin_hex("bg_primary");
    let inside = skin_hex("bg_secondary");
    let ring = skin_hex("border");
    let accent = skin_hex("accent");
    let ok = skin_hex("accent_green");
    // `author_self` is the skin's red.
    let wrong = skin_hex("author_self");
    let caps = skin_hex("rating_star");
    let text = skin_hex("text_primary");
    format!(
        "# {LOOK_HEADER}\n\
# Options are swaylock's long options without \"--\", see swaylock(1).\n\
color={bg}\n\
indicator-idle-visible\n\
indicator-radius=100\n\
indicator-thickness=10\n\
show-failed-attempts\n\
ignore-empty-password\n\
inside-color={inside}\n\
inside-clear-color={inside}\n\
inside-caps-lock-color={inside}\n\
inside-ver-color={inside}\n\
inside-wrong-color={inside}\n\
ring-color={ring}\n\
ring-clear-color={accent}\n\
ring-caps-lock-color={caps}\n\
ring-ver-color={ok}\n\
ring-wrong-color={wrong}\n\
key-hl-color={accent}\n\
bs-hl-color={wrong}\n\
caps-lock-key-hl-color={caps}\n\
caps-lock-bs-hl-color={wrong}\n\
line-uses-inside\n\
separator-color=00000000\n\
text-color={text}\n\
text-clear-color={text}\n\
text-caps-lock-color={text}\n\
text-ver-color={text}\n\
text-wrong-color={text}\n"
    )
}

/// `~/.config/hypr/hyprlock.conf`: the dark skin background, the clock and
/// the date above a password field, as in the hyprlock example config. The
/// placeholder is in the person's language at install time.
pub fn hyprlock_conf() -> String {
    let bg = skin_hex("bg_primary");
    let inside = skin_hex("bg_secondary");
    let ring = skin_hex("border");
    let accent = skin_hex("accent");
    let wrong = skin_hex("author_self");
    let caps = skin_hex("rating_star");
    let text = skin_hex("text_primary");
    // TRANSLATORS: placeholder in the password field of the lock screen (hyprlock).
    // `#` starts a comment in hyprlock.conf, a newline ends the value.
    let placeholder: String = tr("Password…").chars().filter(|c| *c != '#' && *c != '\n').collect();
    format!(
        "# {LOOK_HEADER}\n\
# Reference: https://wiki.hypr.land/Hypr-Ecosystem/hyprlock/\n\
general {{\n    hide_cursor = true\n}}\n\n\
background {{\n    monitor =\n    color = rgb({bg})\n}}\n\n\
label {{\n    monitor =\n    text = $TIME\n    color = rgb({text})\n    font_size = 72\n    position = 0, 160\n    halign = center\n    valign = center\n}}\n\n\
label {{\n    monitor =\n    text = cmd[update:60000] date +\"%A, %d %B\"\n    color = rgb({accent})\n    font_size = 20\n    position = 0, 80\n    halign = center\n    valign = center\n}}\n\n\
input-field {{\n    monitor =\n    size = 320, 56\n    outline_thickness = 3\n    rounding = 8\n    dots_size = 0.25\n    dots_spacing = 0.3\n\
    outer_color = rgb({ring})\n    inner_color = rgb({inside})\n    font_color = rgb({text})\n    check_color = rgb({accent})\n\
    fail_color = rgb({wrong})\n    capslock_color = rgb({caps})\n    fade_on_empty = false\n    placeholder_text = <i>{placeholder}</i>\n\
    position = 0, -40\n    halign = center\n    valign = center\n}}\n"
    )
}

/// Log line: the lock screen look was written.
pub fn lock_look_written(locker: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a lock program (swaylock, hyprlock), %2 a file path.
    trf("🎨 %1: the lock screen got a dark background and a visible password field (%2) — change it freely", &[&locker, &path])
}

/// swayidle autostart for niri (`spawn-at-startup` takes one argument per
/// word; swayidle passes its lock commands to `sh -c` itself).
fn niri_swayidle_node() -> String {
    let args = ["swayidle", "-w", "timeout", "300", LOCK_COMMAND, "before-sleep", LOCK_COMMAND, "lock", LOCK_COMMAND];
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
    format!("spawn-at-startup {}", quoted.join(" "))
}

/// Idle locking in niri's `config.kdl`. The key (Super+Alt+L → swaylock) is
/// already in the niri sample; only swayidle is missing, so it is added
/// unless the user has one (in the config, in included files, as a systemd
/// unit). A disabled `/-…` node doesn't count. `None` — nothing to add.
pub fn niri_idle_edit(main: &str, included: &[String], idle_elsewhere: bool) -> Option<String> {
    if idle_elsewhere {
        return None;
    }
    let live = |t: &str| {
        let mut in_block = false;
        t.lines().any(|l| {
            let c = crate::keyboard_layout::kdl_strip(l, &mut in_block);
            let c = c.trim();
            !c.starts_with("/-") && mentions_idle(c)
        })
    };
    if live(main) || included.iter().any(|t| live(t)) {
        return None;
    }
    let snippet = format!("// {LOCK_HEADER}\n{}\n", niri_swayidle_node());
    Some(crate::keyboard_layout::append(main, &snippet))
}

/// User systemd units through which the user may have enabled the idle
/// daemon (`systemctl --user enable swayidle`/`hypridle`): then a second
/// start from the config isn't needed. `config_home` — `XDG_CONFIG_HOME`.
pub fn idle_service_paths(config_home: &str, daemon: &str) -> Vec<String> {
    let base = format!("{}/systemd/user", config_home.trim_end_matches('/'));
    let mut v = vec![format!("{base}/{daemon}.service")];
    for target in ["graphical-session", "default", "sway-session", "hyprland-session", "niri"] {
        v.push(format!("{base}/{target}.target.wants/{daemon}.service"));
    }
    v
}

/// Log line: lock programs aren't installed — nothing to set up with.
pub fn lock_setup_no_program(desktop: &str, programs: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Hyprland…), %2 program names (“hyprlock, hypridle”).
    trf("ℹ️ %1: screen lock programs are missing (%2) — your distribution may not have them; the screen lock wasn't set up", &[&desktop, &programs])
}

/// Log line: the desktop has no config yet, nowhere to add to.
pub fn lock_setup_no_config(desktop: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Hyprland, niri…), %2 a file path.
    trf("ℹ️ %1: there is no settings file yet (%2). Log in once, then run “Check / Repair” and Rexon will set up the screen lock.", &[&desktop, &path])
}

/// Log line: screen lock configured.
pub fn lock_setup_written(desktop: &str, keys: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a desktop (Wayfire, LXQt…), %2 a key combination,
    // %3 a file path.
    trf("🔒 %1: screen lock set up (%3) — lock it with %2", &[&desktop, &keys, &path])
}

/// Log line: the person already set up locking themselves.
pub fn lock_setup_kept(desktop: &str, path: &str) -> String {
    // TRANSLATORS: %1 is a desktop, %2 a file path.
    trf("ℹ️ %1: the screen lock is already set up in your settings (%2) — left as is", &[&desktop, &path])
}

/// Log line: writing the lock setting failed.
pub fn lock_setup_failed(desktop: &str, path: &str, why: &str) -> String {
    // TRANSLATORS: %1 is a desktop, %2 a file path, %3 the reason.
    trf("⚠️ %1: couldn't set up the screen lock in %2: %3. Locking the screen may not work.", &[&desktop, &path, &why])
}

#[cfg(test)]
mod lock_setup_tests {
    use super::*;

    /// The lock look files: the skin's dark background instead of swaylock's
    /// white screen and hyprlock's blurred screenshot, a visible password
    /// field, only options both lockers know.
    #[test]
    fn lock_look_is_dark_skin_with_visible_input() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let sway = swaylock_config();
        assert!(sway.starts_with(&format!("# {LOOK_HEADER}\n")), "{sway}");
        assert!(sway.contains("\ncolor=1b2838\n"), "{sway}");
        assert!(sway.contains("\nindicator-idle-visible\n") && sway.contains("\nkey-hl-color=66c0f4\n"), "{sway}");
        for line in sway.lines().filter(|l| !l.starts_with('#')) {
            // Every line is a bare long option, values are rrggbb[aa].
            let (opt, value) = line.split_once('=').unwrap_or((line, ""));
            assert!(!opt.starts_with('-') && opt.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{line}");
            assert!(value.is_empty() || value.parse::<u32>().is_ok() || (matches!(value.len(), 6 | 8) && value.chars().all(|c| c.is_ascii_hexdigit())), "{line}");
        }

        let hypr = hyprlock_conf();
        assert!(hypr.contains("background {\n    monitor =\n    color = rgb(1b2838)\n}"), "{hypr}");
        assert!(!hypr.contains("screenshot") && !hypr.contains("blur"), "{hypr}");
        assert!(hypr.contains("text = $TIME") && hypr.contains("input-field {") && hypr.contains("fade_on_empty = false"), "{hypr}");
        assert!(hypr.contains("placeholder_text = <i>Password…</i>"), "{hypr}");
        // Braces balance: every block is closed.
        assert_eq!(hypr.matches('{').count(), hypr.matches('}').count(), "{hypr}");
    }

    #[test]
    fn lock_look_config_paths_follow_lockers_search_order() {
        assert_eq!(
            swaylock_config_paths("/home/u/", "/home/u/.config"),
            ["/home/u/.swaylock/config", "/home/u/.config/swaylock/config", "/etc/swaylock/config"]
        );
        assert_eq!(hyprlock_config_paths("/x/cfg/"), ["/x/cfg/hypr/hyprlock.conf", "/etc/xdg/hypr/hyprlock.conf"]);
    }

    #[test]
    fn wayfire_gets_lock_key_and_idle_once() {
        let out = wayfire_lock_edit("").unwrap();
        assert!(out.contains(&format!("[command]\n# {LOCK_HEADER}\n")), "{out}");
        assert!(out.contains("binding_lock = <super> <shift> KEY_ESC\ncommand_lock = swaylock -f\n"), "{out}");
        assert!(out.contains(&format!("[autostart]\n# {LOCK_HEADER}\nidle = {SWAYIDLE_LOCK}\n")), "{out}");
        // A second run changes nothing.
        assert_eq!(wayfire_lock_edit(&out), None);

        // Own sections: the lines go into them, what was there stays intact.
        let ini = "[input]\nxkb_layout = us,ru\n\n[command]\nbinding_terminal = <super> KEY_ENTER\ncommand_terminal = foot\n";
        let out = wayfire_lock_edit(ini).unwrap();
        assert!(out.starts_with(&format!("[input]\nxkb_layout = us,ru\n\n[command]\n# {LOCK_HEADER}\n")), "{out}");
        assert!(out.contains("command_terminal = foot\n"), "{out}");
        assert_eq!(out.matches("[command]").count(), 1, "{out}");
    }

    #[test]
    fn wayfire_own_lock_is_kept() {
        // The example from the Wayfire documentation: both the key and swayidle are already there.
        let ini = "[autostart]\nidle = swayidle before-sleep swaylock\n\n[command]\nbinding_lock = <super> <shift> KEY_ESC\ncommand_lock = swaylock\n";
        assert_eq!(wayfire_lock_edit(ini), None);
        // Own locker under another name: the key is not added, swayidle is.
        let ini = "[command]\nbinding_mylock = <super> KEY_L\ncommand_mylock = wf-locker\n";
        let out = wayfire_lock_edit(ini).unwrap();
        assert!(!out.contains("binding_lock"), "{out}");
        assert!(out.contains("idle = swayidle"), "{out}");
        // A commented-out example is not a setting.
        let out = wayfire_lock_edit("[command]\n# command_lock = swaylock\n").unwrap();
        assert!(out.contains("command_lock = swaylock -f"), "{out}");
    }

    #[test]
    fn sway_gets_lock_key_and_idle_once() {
        let out = sway_lock_edit("include /etc/sway/config\n", &[], false).unwrap();
        assert_eq!(
            out,
            format!("include /etc/sway/config\n\n# {LOCK_HEADER}\nbindsym Mod4+Shift+Escape exec swaylock -f\nexec {SWAYIDLE_LOCK}\n")
        );
        assert_eq!(sway_lock_edit(&out, &[], false), None);
        // A commented out example from /etc/sway/config is not a setting.
        let system = "# exec swayidle -w \\\n#   timeout 300 'swaylock -f -c 000000'\nbindsym $mod+Shift+e exec swaynag\n".to_string();
        assert!(sway_lock_edit("include /etc/sway/config\n", &[system], false).unwrap().contains("exec swayidle"));
        // Own locker on another key — no key is added.
        let out = sway_lock_edit("bindsym --to-code $mod+l exec gtklock\n", &[], false).unwrap();
        assert!(!out.contains("bindsym Mod4") && out.contains("exec swayidle"), "{out}");
        // Super+Shift+Esc taken by something else — not overridden.
        let out = sway_lock_edit("bindsym $mod+Shift+Escape exec foot\n", &[], false).unwrap();
        assert!(!out.contains("bindsym Mod4"), "{out}");
        // swayidle in an included file or as a unit — the user's own.
        let inc = vec!["exec_always swayidle -w before-sleep gtklock\n".to_string()];
        let out = sway_lock_edit("x\n", &inc, false).unwrap();
        assert!(!out.contains("exec swayidle"), "{out}");
        assert!(!sway_lock_edit("x\n", &[], true).unwrap().contains("exec swayidle"));
        assert_eq!(sway_lock_edit("bindsym Mod4+x exec swaylock\n", &[], true), None);
    }

    #[test]
    fn hyprland_conf_and_lua_get_lock_once() {
        let edit = hypr_lock_edit(false, "bind = $mainMod, Q, exec, kitty\n", &[], false).unwrap();
        assert!(edit.idle);
        assert!(edit.text.ends_with(&format!("# {LOCK_HEADER}\nbind = SUPER SHIFT, Escape, exec, hyprlock\nexec-once = hypridle\n")), "{}", edit.text);
        assert_eq!(hypr_lock_edit(false, &edit.text, &[], false), None);
        // Own hyprlock on Super+L and hypridle as a unit — nothing to add.
        assert_eq!(hypr_lock_edit(false, "bind = $mainMod, L, exec, hyprlock\n", &[], true), None);
        // Super+Shift+Esc taken — no key added, hypridle is.
        let edit = hypr_lock_edit(false, "bind = SUPER_SHIFT, escape, exec, foot\n", &[], false).unwrap();
        assert!(!edit.text.contains("hyprlock") && edit.text.contains("exec-once = hypridle"), "{}", edit.text);
        // Commented out — not the user's own.
        assert!(hypr_lock_edit(false, "# exec-once = hypridle\n", &[], false).unwrap().idle);
        // hypridle in an included file.
        let edit = hypr_lock_edit(false, "source = ~/.config/hypr/auto.conf\n", &["exec-once = hypridle\n".into()], false).unwrap();
        assert!(!edit.idle && edit.text.contains("bind = SUPER SHIFT, Escape"), "{}", edit.text);

        let lua = "local mainMod = \"SUPER\"\n-- hl.exec_cmd(\"hypridle\")\n";
        let edit = hypr_lock_edit(true, lua, &[], false).unwrap();
        assert!(edit.idle);
        assert!(edit.text.ends_with(&format!(
            "-- {LOCK_HEADER}\nhl.bind(\"SUPER + SHIFT + Escape\", hl.dsp.exec_cmd(\"hyprlock\"))\nhl.on(\"hyprland.start\", function()\n    hl.exec_cmd(\"hypridle\")\nend)\n"
        )), "{}", edit.text);
        assert_eq!(hypr_lock_edit(true, &edit.text, &[], false), None);
        let own = "hl.bind(mainMod .. \" + L\", hl.dsp.exec_cmd(\"hyprlock\"))\nhl.on(\"hyprland.start\", function() hl.exec_cmd(\"hypridle\") end)\n";
        assert_eq!(hypr_lock_edit(true, own, &[], false), None);

        let conf = hypridle_conf();
        assert!(conf.contains("lock_cmd = pidof hyprlock || hyprlock\n") && conf.contains("timeout = 300\n"), "{conf}");
    }

    #[test]
    fn niri_gets_swayidle_once() {
        let kdl = "spawn-at-startup \"waybar\"\n";
        let out = niri_idle_edit(kdl, &[], false).unwrap();
        assert_eq!(
            out,
            format!("{kdl}\n// {LOCK_HEADER}\nspawn-at-startup \"swayidle\" \"-w\" \"timeout\" \"300\" \"swaylock -f\" \"before-sleep\" \"swaylock -f\" \"lock\" \"swaylock -f\"\n")
        );
        assert_eq!(niri_idle_edit(&out, &[], false), None);
        // A disabled node and a comment don't count; an included file and a
        // unit do.
        assert!(niri_idle_edit("/-spawn-at-startup \"swayidle\"\n// swayidle\n", &[], false).is_some());
        assert_eq!(niri_idle_edit(kdl, &["spawn-sh-at-startup \"swayidle -w\"\n".into()], false), None);
        assert_eq!(niri_idle_edit(kdl, &[], true), None);
    }

    #[test]
    fn idle_services_are_looked_up_in_config_home() {
        let v = idle_service_paths("/home/t/.config/", "swayidle");
        assert!(v.contains(&"/home/t/.config/systemd/user/swayidle.service".to_string()), "{v:?}");
        assert!(v.contains(&"/home/t/.config/systemd/user/graphical-session.target.wants/swayidle.service".to_string()), "{v:?}");
    }

    #[test]
    fn lxqt_gets_both_lock_commands_once() {
        let out = lxqt_lock_edit(None).unwrap();
        assert!(out.contains("[General]\n"), "{out}");
        assert!(out.contains("lock_command=xscreensaver-command -lock\n"), "{out}");
        assert!(out.contains("lock_command_wayland=swaylock -f\n"), "{out}");
        assert_eq!(lxqt_lock_edit(Some(&out)), None);

        // Own X11 command intact, the missing Wayland one is appended; neighbouring
        // keys of the file are not touched.
        let mine = "[General]\nwindow_manager=openbox\nlock_command=i3lock\n\n[Mouse]\ncursor_size=24\n";
        let out = lxqt_lock_edit(Some(mine)).unwrap();
        assert!(out.contains("lock_command=i3lock\n") && !out.contains("xscreensaver"), "{out}");
        assert!(out.contains("lock_command_wayland=swaylock -f\n"), "{out}");
        assert!(out.contains("window_manager=openbox\n") && out.ends_with("[Mouse]\ncursor_size=24\n"), "{out}");
        // An empty value is not a setting (the LXQt settings window writes it like that).
        let out = lxqt_lock_edit(Some("[General]\nlock_command=\n")).unwrap();
        assert!(out.contains("lock_command=xscreensaver-command -lock\n"), "{out}");
        assert!(!out.contains("lock_command=\n"), "{out}");
    }
}

/// Texts and the command of the "install packages beyond the distribution"
/// step (port of `_ensure_extra_packages`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtraPkgTexts {
    /// Not Arch: there is nowhere else to build these packages: we say so honestly and leave.
    pub not_arch: String,
    pub already: String,
    pub no_helper: String,
    pub no_helper_error: String,
    pub installing: String,
    pub done: String,
    pub failed: String,
    pub failed_error: String,
    /// A separate text for when the build did not fit into the time limit.
    pub failed_timeout: String,
    pub failed_timeout_error: String,
    /// AUR helper command (empty if there is no helper).
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

/// Gather everything the install-extras step may say and run.
pub fn extra_packages_texts(
    tag: &str,
    missing: &[String],
    helper: Option<&str>,
) -> ExtraPkgTexts {
    let list = missing.join(", ");
    let mut argv = Vec::new();
    if let Some(h) = helper {
        argv = crate::aur_install::install_argv(h, missing, false);
    }
    ExtraPkgTexts {
        not_arch: format!(
            "  {}",
            tr("ℹ️ These add-ons are only built for Arch/CachyOS for now — \
                skipping this step on this system")
        ),
        already: format!("  {}", tr("✅ Already installed")),
        no_helper: format!(
            "  {}",
            trf("⚠️ No AUR helper found — install these yourself: %1", &[&list])
        ),
        no_helper_error: format!("no aur helper for {}", tag),
        installing: format!("  {}", trf("📥 Installing: %1…", &[&list])),
        done: format!("  {}", trc("add-on install step", "✅ Installed")),
        failed: format!("  {}", tr("⚠️ Couldn't install")),
        failed_error: format!("{} install failed", tag),
        failed_timeout: format!(
            "  {}",
            trf(
                "⏳ The build ran out of time — this is NOT an installation \
                 failure. To finish it yourself: yay -S --needed %1",
                &[&list],
            )
        ),
        failed_timeout_error: format!("{} install timed out", tag),
        argv,
        // Building from source is the `build` class: 1800 s was already not enough
        // for an average set, and the time here is set by the machine, not by us.
        timeout_secs: crate::utils::BUILD_TIMEOUT_SECS,
    }
}

#[cfg(test)]
mod helper_path_tests {
    use super::*;

    /// The manual install constants must be the same as what [`helper_path_in`]
    /// joins from the name and directory: otherwise the truth about the path
    /// splits again, and the policy diverges from argv.
    #[test]
    fn константы_ручной_установки_склеиваются_из_имени() {
        assert_eq!(DE_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, DE_HELPER_NAME));
        assert_eq!(PKG_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, PKG_HELPER_NAME));
        assert_eq!(
            STAGE_HELPER_PATH,
            helper_path_in(HELPER_DIR_SYSTEM, STAGE_HELPER_NAME)
        );
    }

    /// The package is preferred over the manual install, and without files the
    /// answer is the manual install path (a defined one, not empty).
    #[test]
    fn выбор_пути_помощника() {
        let pkg = helper_path_in(HELPER_DIR_PACKAGE, PKG_HELPER_NAME);
        assert_eq!(
            helper_candidates(PKG_HELPER_NAME),
            vec![pkg.clone(), PKG_HELPER_PATH.to_string()]
        );
        assert_eq!(pick_helper_path(PKG_HELPER_NAME, &|_| true), pkg);
        assert_eq!(
            pick_helper_path(PKG_HELPER_NAME, &|p| p == PKG_HELPER_PATH),
            PKG_HELPER_PATH
        );
        assert_eq!(pick_helper_path(PKG_HELPER_NAME, &|_| false), PKG_HELPER_PATH);
    }
}


#[cfg(test)]
mod no_polkit_agent_tests {
    use super::*;

    /// No polkit agent (bare sway and niri do not start one themselves): pkexec
    /// answers with code 127 and "No authentication agent found", without the
    /// words "Not authorized". Previously the person got the raw pkexec line
    /// instead of an explanation that there is no password window and how to
    /// bring one up. A `pgrep` check does not help here: the `polkitd` pattern
    /// matches the system service, which always runs.
    #[test]
    fn missing_agent_is_explained() {
        let pkgs = vec!["noctalia".to_string()];
        for err in [
            "Error executing command as another user: No authentication agent found.\n",
            "Ошибка выполнения команды от имени другого пользователя: Агент аутентификации не найден.\n",
        ] {
            let out =
                batch_main_result(Some((127, err, "")), true, &pkgs, false, PkgFamily::Arch, &|| true, &|| None);
            assert!(!out.ok);
            assert!(
                out.message.starts_with("The service that shows the password window isn't running."),
                "{}",
                out.message
            );
            assert!(out.message.contains("sudo pacman"), "{}", out.message);
        }
    }
}

// ============================================================================
// Biting the privileged helper: package name versus path
// ============================================================================
//
// ⛔ These checks stand on the site of a real hole (25.09.2026): the
// `org.rexon.pkg` helper accepted `/` in a package name, took the package
// manager family from an argument and assembled the command via `eval`. The
// line `pkexec rexon-pkg-helper debian install /tmp/evil.deb` gave execution
// of foreign code AS ROOT on any machine with apt-get/dnf/zypper. Weakening
// any of the checks below means opening it again.

#[cfg(test)]
mod pkg_arg_security_tests {
    use super::*;

    /// Everything that could smuggle in a path or a command instead of a package name.
    const ATTACKS: &[&str] = &[
        "/tmp/evil.deb",
        "/tmp/evil.rpm",
        "/tmp/evil",
        "./evil.deb",
        "evil.deb",
        "evil.rpm",
        "evil.pkg.tar.zst",
        "../../etc/passwd",
        "..",
        "../mate",
        "mate/../../tmp/evil",
        "a/b/c",
        ".hidden",
        "-Rns",
        "--root=/tmp",
        "mate;rm -rf /",
        "mate&&id",
        "mate|id",
        "$(id)",
        "`id`",
        "${PATH}",
        "mate linux",
        "mate\nlinux",
        "mate\tlinux",
        "'mate'",
        "\"mate\"",
        "",
    ];

    /// Portage sets: `emerge --unmerge @world` would wipe the whole system. A
    /// separate list because a leading `@` is valid in EXACTLY one place: a dnf
    /// group on Fedora (`@gnome-desktop`).
    const SET_ATTACKS: &[&str] = &["@world", "@system", "@installed", "@profile"];

    /// A slash in the middle of a name: a relative path. Valid in EXACTLY
    /// Portage: it is its `category/name` atom (`x11-misc/sddm`), and there it is
    /// checked half by half.
    const SLASH_ATTACKS: &[&str] = &["a/b", "usr/bin/id", "tmp/evil"];

    /// No family accepts a path, a package file, a flag or a metacharacter:
    /// neither in the allowlist, nor in the command table, nor in the privileged
    /// helper's argv.
    ///
    /// The command tables are checked separately from the helper on purpose: a
    /// helper refusal would mean "fall back to raw `pkexec apt-get install …`",
    /// and that is the same hole without the helper.
    #[test]
    fn путь_вместо_имени_пакета_отвергнут() {
        let families = [
            PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora,
            PkgFamily::Suse, PkgFamily::Gentoo, PkgFamily::Alpine,
            PkgFamily::Void, PkgFamily::Solus, PkgFamily::Clear,
            PkgFamily::Alt, PkgFamily::Nix,
        ];
        for family in families {
            for bad in ATTACKS {
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: allowlist пропустил {bad:?}", family.as_str()
                );
                assert!(
                    install_cmd(family, &pkgs).is_err(),
                    "{}: install_cmd собрал команду на {bad:?}", family.as_str()
                );
                assert!(
                    remove_cmd(family, &pkgs).is_err(),
                    "{}: remove_cmd собрал команду на {bad:?}", family.as_str()
                );
                assert!(
                    privileged_pkg_cmd(family, "install", &pkgs, true).is_none(),
                    "{}: помощник позван с {bad:?}", family.as_str()
                );
                assert!(
                    privileged_pkg_cmd(family, "remove", &pkgs, true).is_none(),
                    "{}: помощник позван с {bad:?}", family.as_str()
                );
                // An honest name next to a planted one does not launder it.
                let mixed = vec!["mate".to_string(), (*bad).to_string()];
                assert!(install_cmd(family, &mixed).is_err(), "{bad:?} в хвосте");
            }
            for bad in SLASH_ATTACKS {
                if family == PkgFamily::Gentoo {
                    continue; // a Portage atom is the only valid '/'
                }
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: относительный путь {bad:?} принят за имя", family.as_str()
                );
                assert!(install_cmd(family, &pkgs).is_err(), "{bad:?} в установке");
                assert!(
                    privileged_pkg_cmd(family, "install", &pkgs, true).is_none(),
                    "{}: помощник позван с путём {bad:?}", family.as_str()
                );
            }
            for bad in SET_ATTACKS {
                if family == PkgFamily::Fedora {
                    continue; // in dnf `@name` is a package group, and only that
                }
                let pkgs = vec![(*bad).to_string()];
                assert!(
                    !is_safe_pkg_arg(family, bad),
                    "{}: набор {bad:?} принят за имя пакета", family.as_str()
                );
                assert!(remove_cmd(family, &pkgs).is_err(), "{bad:?} в удалении");
                assert!(
                    privileged_pkg_cmd(family, "remove", &pkgs, true).is_none(),
                    "{}: помощник позван с набором {bad:?}", family.as_str()
                );
            }
        }
    }

    /// The other half: honest names pass AS THEY WERE: the check is not about
    /// strictness but about the product staying functional.
    #[test]
    fn честные_имена_пакетов_проходят() {
        let cases: &[(PkgFamily, &[&str])] = &[
            (PkgFamily::Arch, &[
                "plasma-desktop", "qt6-base", "python-pyside6", "xorg-server",
                "lib32-mesa", "ttf-dejavu", "gcc-libs", "libstdc++5", "g++",
                "sddm", "mate", "mate-extra", "cinnamon-translations",
            ]),
            (PkgFamily::Debian, &[
                "plasma-desktop", "qt6-base-dev", "python3-pyside6",
                "libstdc++6", "libc6:i386", "xfce4", "lightdm",
            ]),
            (PkgFamily::Fedora, &[
                "@gnome-desktop", "@kde-desktop-environment", "qt6-qtbase",
                "lightdm-gtk-greeter",
            ]),
            (PkgFamily::Gentoo, &[
                "gnome-extra/cinnamon", "dev-lang/python:3.12",
                "x11-misc/sddm", "kde-plasma/plasma-meta", "mate-base/mate",
            ]),
            (PkgFamily::Suse, &["plasma6-desktop", "libQt6Core6"]),
            (PkgFamily::Alt, &["plasma5-desktop", "libstdc++6"]),
        ];
        for (family, names) in cases {
            for n in *names {
                assert!(
                    is_safe_pkg_arg(*family, n),
                    "{}: честное имя {n:?} отвергнуто", family.as_str()
                );
            }
            let pkgs: Vec<String> = names.iter().map(|s| (*s).to_string()).collect();
            let cmd = install_cmd(*family, &pkgs)
                .unwrap_or_else(|e| panic!("{}: {e}", family.as_str()));
            for n in *names {
                assert!(
                    cmd.iter().any(|a| a == n),
                    "{}: имя {n:?} не попало в команду целиком", family.as_str()
                );
            }
            // Vital packages (`gcc-libs`, `lib32-mesa`) are not removed by the raw
            // removal path; the rest is.
            let removable: Vec<String> =
                pkgs.iter().filter(|p| !is_vital_package(p)).cloned().collect();
            remove_cmd(*family, &removable).expect("удаление честных имён");
            if removable.len() != pkgs.len() {
                assert!(remove_cmd(*family, &pkgs).is_err(), "вечное в сыром удалении");
            }
            if PKG_HELPER_FAMILIES.contains(&family.as_str()) {
                let argv = privileged_pkg_cmd(*family, "install", &pkgs, true)
                    .expect("помощник берёт честные имена");
                for n in *names {
                    assert!(
                        argv.iter().any(|a| a == n),
                        "{}: помощник потерял {n:?}", family.as_str()
                    );
                }
            }
        }
    }

    /// Rexon desktop packages and the permanent list do not intersect: so the
    /// ban on removing vital packages does not break a single honest scenario
    /// (the only caller of `remove` is desktop removal).
    #[test]
    fn вечный_список_не_пересекается_с_рабочими_столами() {
        for (de_id, packages) in crate::system::de_database_packages() {
            for (_fam, pkgs) in packages {
                for p in *pkgs {
                    assert!(
                        !PKG_HELPER_VITAL.contains(p),
                        "{de_id}: {p} и в столе, и в вечном списке"
                    );
                }
            }
        }
        for dm in DE_HELPER_DISPLAY_MANAGERS {
            assert!(!PKG_HELPER_VITAL.contains(dm), "экран входа {dm} в вечном списке");
        }
    }

    /// Helper text: no `eval`, no family from an argument, no old allowlist with `/`.
    #[test]
    fn помощники_без_eval_и_без_семейства_из_аргумента() {
        let pkg = pkg_helper_script();
        let de = de_helper_script();
        // We look at the CODE, not the comments: both `eval` and the old allowlist
        // are mentioned in the helper headers precisely so that they are not
        // brought back.
        let code = |s: &str| -> String {
            s.lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for (name, s) in [("pkg", &pkg), ("de", &de)] {
            let body = code(s);
            assert!(
                !body.contains("eval"),
                "{name}-helper снова собирает команду через eval"
            );
            assert!(
                !body.contains("[A-Za-z0-9@._+:/-]"),
                "{name}-helper вернул allowlist с '/' в имени пакета"
            );
        }
        // The helper determines the family ITSELF, by the same table as the core.
        assert!(pkg.contains("detect_family"), "нет детекции семейства");
        assert!(
            pkg.contains("family=\"$(detect_family"),
            "семейство снова берётся не из детекции"
        );
        assert!(
            pkg.contains("claimed=\"$1\""),
            "argv помощника изменился без нужды"
        );
        for (id, fam) in crate::system::ID_MAP {
            assert!(pkg.contains(id), "ID {id} потерян в детекции помощника");
            assert!(pkg.contains(fam.as_str()), "семейство {} потеряно", fam.as_str());
        }
        // The command is an array of arguments, not a string for the shell.
        for f in PKG_HELPER_FAMILIES {
            assert!(
                pkg.contains(&format!("{f}) cmd=(")),
                "команда {f} не массивом"
            );
        }
        assert!(de.contains("PKG_CMD=("), "de-helper: команда не массивом");
        assert!(de.contains("UPD_CMD=("), "de-helper: обновление не массивом");
        // The package name is checked by the same rule as in the core.
        assert!(
            pkg.contains("'^[A-Za-z0-9][A-Za-z0-9._+:-]*$'"),
            "в помощнике нет проверки атома имени"
        );
        assert!(pkg.contains("*.deb|*.rpm"), "помощник берёт файл пакета");
        for v in PKG_HELPER_VITAL {
            assert!(pkg.contains(v), "вечный список потерял {v}");
        }
    }

    /// ⭐ A LIVE BITE: the helper is run by the system bash and must REFUSE
    /// every attempt to smuggle in a path. root is neither needed nor involved
    /// here: the refusal happens before any package manager call, and the honest
    /// run goes to fake binaries from its own directory.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn помощник_живьём_отказывает_на_пути() {
        use std::io::Write;
        use std::process::Command;

        let dir = std::env::temp_dir()
            .join(format!("rexon_pkg_helper_stand_{}", std::process::id()));
        let stub = dir.join("stub");
        std::fs::create_dir_all(&stub).expect("каталог стенда");
        // Fake package managers: they print argv and do nothing. The real manager
        // is not run even once in this check.
        for bin in ["pacman", "apt-get", "dpkg", "rpm", "dnf", "yum", "zypper",
                    "emerge", "apk", "xbps-install", "eopkg", "swupd",
                    "systemctl"] {
            let p = stub.join(bin);
            let mut f = std::fs::File::create(&p).expect("подставной бинарь");
            writeln!(f, "#!/bin/bash\necho \"STUB $(basename \"$0\") $*\"")
                .expect("тело подставного бинаря");
            drop(f);
            set_exec(&p);
        }
        // Utilities the helper uses to parse os-release and names.
        for tool in ["sed", "grep", "head", "tr", "basename", "bash", "cat"] {
            for root in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
                let from = std::path::Path::new(root).join(tool);
                if from.exists() {
                    let _ = std::os::unix::fs::symlink(&from, stub.join(tool));
                    break;
                }
            }
        }
        // A copy of the helper with ONE edit: the bench PATH instead of the system
        // one. The edit must apply, otherwise the check would silently measure the
        // wrong thing. The same line is in the login screen fallback script
        // (`DM_FALLBACK_SCRIPT`): the helper embeds it whole and on `switch-dm`
        // writes it to `DM_FALLBACK_SCRIPT_PATH`. Only the helper's OWN line is
        // replaced, the first one, before the embedded script; the fallback keeps
        // the system PATH.
        let src = pkg_helper_script();
        let want = "export PATH=/usr/sbin:/usr/bin:/sbin:/bin";
        assert_eq!(PKG_HELPER_SCRIPT_TMPL.matches(want).count(), 1, "строка PATH помощника изменилась");
        assert_eq!(DM_FALLBACK_SCRIPT.matches(want).count(), 1, "строка PATH сценария отката изменилась");
        assert_eq!(src.matches(want).count(), 2, "строка PATH встречается не там, где ждали");
        let own = src.find(want).expect("строка PATH помощника");
        let embedded = src.find(DM_FALLBACK_SCRIPT.trim_end()).expect("встроенный сценарий отката");
        assert!(own < embedded, "первая строка PATH — не помощника, а сценария отката");
        let script = src.replacen(want, &format!("export PATH={}", stub.display()), 1);
        let path = dir.join("rexon-pkg-helper");
        std::fs::write(&path, &script).expect("запись помощника");
        set_exec(&path);

        let run = |args: &[&str]| -> (i32, String, String) {
            let out = Command::new("bash")
                .arg(&path)
                .args(args)
                .output()
                .expect("запуск помощника");
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).to_string(),
                String::from_utf8_lossy(&out.stderr).to_string(),
            )
        };

        // 1. A path, a package file, directory traversal, metacharacters, a flag:
        //    refusal 2, and not a single STUB in the output (the manager did not run).
        for (op, arg) in [
            ("install", "/tmp/evil.deb"),
            ("install", "/tmp/evil.rpm"),
            ("install", "./evil.deb"),
            ("install", "evil.deb"),
            ("install", "../../etc/passwd"),
            ("install", "mate/../../tmp/evil"),
            ("install", "mate;rm -rf /"),
            ("install", "$(id)"),
            ("install", "`id`"),
            ("install", "mate linux"),
            ("install", "-Rns"),
            ("install", "@world"),
            ("remove", "/tmp/evil.deb"),
            ("remove", "linux"),
            ("remove", "systemd"),
            ("remove", "polkit"),
            ("remove", "pacman"),
        ] {
            for claimed in ["arch", "debian", "fedora", "suse", "gentoo"] {
                let (code, out, err) = run(&[claimed, op, arg]);
                assert_eq!(
                    code, 2,
                    "{claimed} {op} {arg:?}: помощник не отказал ({err})"
                );
                assert!(
                    !out.contains("STUB"),
                    "{claimed} {op} {arg:?}: менеджер пакетов всё-таки позван: {out}"
                );
                assert!(
                    err.contains("rexon-pkg-helper:"),
                    "{claimed} {op} {arg:?}: отказ без объяснения"
                );
            }
        }

        // 2. Honest names (from the core's tables: the install whitelist since
        //    28.10.2026) pass and go out as SEPARATE arguments.
        let (code, out, err) =
            run(&["arch", "install", "konsole", "sddm", "mpv"]);
        if err.contains("unsupported family") {
            // NixOS and unknown machines: the helper honestly refuses, and that is
            // exactly what should happen: there is no argv to compare then.
            assert_eq!(code, 2);
        } else {
            assert_eq!(code, 0, "честные имена отвергнуты: {err}");
            assert!(out.starts_with("STUB "), "менеджер не позван: {out}");
            assert!(
                out.trim_end()
                    .ends_with(" konsole sddm mpv"),
                "имена попали в команду не отдельными аргументами: {out}"
            );
        }

        // 3. Lying about the family is no longer possible: the one determined from
        //    the machine is chosen, not the one the caller named.
        let (_, out_true, _) = run(&["arch", "update"]);
        let (_, out_lie, _) = run(&["debian", "update"]);
        assert_eq!(
            out_true, out_lie,
            "семейство из аргумента всё ещё меняет команду"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn set_exec(p: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(p).expect("права").permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(p, perm).expect("бит запуска");
    }
}

// ============================================================================
// polkit "umbrella": one password window for four helpers (M1, 28.09.2026)
// ============================================================================

#[cfg(test)]
mod imply_umbrella_tests {
    use super::*;
    use crate::installer::{system_policy_xml, system_policy_xml_for, SYSTEM_INSTALL_ACTION};

    const IMPLY_KEY: &str = "org.freedesktop.policykit.imply";

    /// All Rexon policies: (action, manual install XML, package XML).
    fn policies() -> Vec<(&'static str, String, String)> {
        let pkg_dir = |name| helper_path_in(HELPER_DIR_PACKAGE, name);
        vec![
            (DE_INSTALL_ACTION, de_policy_xml(), de_policy_xml_for(&pkg_dir(DE_HELPER_NAME))),
            (PKG_ACTION, pkg_policy_xml(), pkg_policy_xml_for(&pkg_dir(PKG_HELPER_NAME))),
            (STAGE_ACTION, stage_policy_xml(), stage_policy_xml_for(&pkg_dir(STAGE_HELPER_NAME))),
            (
                SYSTEM_INSTALL_ACTION,
                system_policy_xml(),
                system_policy_xml_for(&pkg_dir(crate::installer::SYSTEM_HELPER_NAME)),
            ),
        ]
    }

    /// `org.rexon.pkg` covers the three other actions, both in the package and
    /// in the manual install.
    #[test]
    fn pkg_policy_carries_the_umbrella() {
        let want = format!(
            "<annotate key=\"{IMPLY_KEY}\">{DE_INSTALL_ACTION} {STAGE_ACTION} \
             {SYSTEM_INSTALL_ACTION}</annotate>"
        );
        for (action, manual, package) in policies() {
            if action == PKG_ACTION {
                assert!(manual.contains(&want), "{manual}");
                assert!(package.contains(&want), "{package}");
            }
        }
        assert!(!PKG_IMPLIES.contains(&PKG_ACTION));
    }

    /// ⛔ The `imply` direction is one-way: from the action that is never
    /// "without password" to the others. An `imply` on an action from the
    /// "without password" rule would make everything it covers password-free
    /// too, for any process of the session. So exactly one policy has the
    /// annotation, and that policy is not in the rule.
    #[test]
    fn imply_umbrella_never_hangs_on_passwordless_action() {
        assert!(!PASSWORDLESS_ACTIONS.contains(&PKG_ACTION));
        for (action, manual, package) in policies() {
            let carries = manual.contains(IMPLY_KEY) || package.contains(IMPLY_KEY);
            assert_eq!(carries, action == PKG_ACTION, "imply у {action}");
            if carries {
                assert!(
                    !PASSWORDLESS_ACTIONS.contains(&action),
                    "{action} с imply стоит в правиле «без пароля»"
                );
            }
        }
    }

    /// Wehrwolfmann's decision of 28.09.2026: login and boot screen themes and
    /// the login screen background were removed from the "without password" rule.
    #[test]
    fn passwordless_rule_no_longer_covers_login_and_boot_screens() {
        assert_eq!(PASSWORDLESS_ACTIONS, &[DE_INSTALL_ACTION]);
        for family in [PkgFamily::Arch, PkgFamily::Debian] {
            let rule = passwordless_rules_js(family);
            for gone in [SYSTEM_INSTALL_ACTION, STAGE_ACTION, PKG_ACTION] {
                assert!(!rule.contains(&format!("\"{gone}\"")), "{gone} в правиле:\n{rule}");
            }
        }
    }
}

// ============================================================================
// The org.rexon.session privilege session action (B11)
// ============================================================================

#[cfg(test)]
mod session_action_tests {
    use super::*;
    use crate::installer::{system_policy_xml, SYSTEM_INSTALL_ACTION};

    const IMPLY_KEY: &str = "org.freedesktop.policykit.imply";

    /// Value of the `<tag>…</tag>` element (first occurrence).
    fn elem<'a>(xml: &'a str, tag: &str) -> &'a str {
        let open = format!("<{tag}>");
        let at = xml.find(&open).unwrap_or_else(|| panic!("нет <{tag}>")) + open.len();
        &xml[at..at + xml[at..].find('<').unwrap()]
    }

    /// ⛔ A password for EVERY session start: `auth_admin` in all three fields,
    /// without `_keep` (the helper holds it, not polkit), the path is where the
    /// helper lives, a graphical password window.
    #[test]
    fn session_policy_asks_password_every_time_and_points_to_helper() {
        let pkg = helper_path_in(HELPER_DIR_PACKAGE, SESSION_HELPER_NAME);
        assert_eq!(pkg, "/usr/lib/rexon-priv-helper");
        assert_eq!(SESSION_HELPER_PATH, helper_path_in(HELPER_DIR_SYSTEM, SESSION_HELPER_NAME));
        assert_eq!(SESSION_POLICY_PATH, format!("/usr/share/polkit-1/actions/{SESSION_ACTION}.policy"));
        for (path, xml) in [(SESSION_HELPER_PATH.to_string(), session_policy_xml()), (pkg.clone(), session_policy_xml_for(&pkg))] {
            assert!(xml.contains(&format!("<action id=\"{SESSION_ACTION}\">")), "{xml}");
            for tag in ["allow_any", "allow_inactive", "allow_active"] {
                assert_eq!(elem(&xml, tag), "auth_admin", "{tag}:\n{xml}");
            }
            assert!(!xml.contains("auth_admin_keep"), "{xml}");
            assert!(!xml.contains(">yes<"), "{xml}");
            assert!(xml.contains(&format!(
                "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
            )));
            assert!(xml.contains("<annotate key=\"org.freedesktop.policykit.exec.allow_gui\">true</annotate>"));
            assert!(xml.contains("<description>") && xml.contains("<message>"));
            assert_eq!(xml.matches("<action ").count(), 1);
        }
    }

    /// ⛔ Not a single silent road to the session: the action is not in the
    /// "without password" rule, nobody's `imply` covers it, and it `imply`s nothing itself.
    #[test]
    fn session_action_is_not_passwordless_and_not_implied() {
        assert!(!PASSWORDLESS_ACTIONS.contains(&SESSION_ACTION));
        assert!(!PKG_IMPLIES.contains(&SESSION_ACTION));
        for family in [PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Unknown] {
            let rule = passwordless_rules_js(family);
            assert!(!rule.contains(SESSION_ACTION), "{SESSION_ACTION} в правиле:\n{rule}");
        }
        for xml in [de_policy_xml(), pkg_policy_xml(), stage_policy_xml(), system_policy_xml()] {
            assert!(!xml.contains(SESSION_ACTION), "чужая политика упоминает {SESSION_ACTION}:\n{xml}");
        }
        assert!(!session_policy_xml().contains(IMPLY_KEY));
        // And the other way: the session does not pull in the old actions.
        let own = session_policy_xml();
        for other in [DE_INSTALL_ACTION, PKG_ACTION, STAGE_ACTION, SYSTEM_INSTALL_ACTION] {
            assert!(!own.contains(other), "{other} в политике сеанса");
        }
    }

    /// Password window captions are translated into the catalog languages by
    /// the same mechanism as the other policies (`xml:lang`).
    #[test]
    fn session_policy_texts_are_translated() {
        let xml = session_policy_xml();
        for code in ["ru", "de"] {
            if crate::system_l10n::languages().contains(&code) {
                assert!(xml.contains(&format!("<description xml:lang=\"{code}\">")), "{code}:\n{xml}");
                assert!(xml.contains(&format!("<message xml:lang=\"{code}\">")), "{code}:\n{xml}");
            }
        }
    }
}

// ============================================================================
// The desktop install helper's log is not written by root (28.09.2026)
// ============================================================================

#[cfg(test)]
mod de_helper_log_tests {
    use super::*;
    use std::process::Command;

    /// The helper up to package selection, then a mark and code 3. Run as a
    /// regular user with a fake `setpriv` (without root it is only needed as
    /// "run the rest").
    fn run_log_part(tag: &str, log_path: &str) -> (i32, String) {
        let full = de_helper_script();
        let cut = full.find("# Packages for the de_id|family pair").expect("раздел пакетов в помощнике");
        let script = format!("{}echo REXON_BODY\nexit 3\n", &full[..cut]);
        let dir = std::env::temp_dir().join(format!("rexon-de-log-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stub = dir.join("setpriv");
        std::fs::write(&stub, "#!/bin/bash\nwhile [ \"$1\" != \"--\" ]; do shift; done; shift; exec \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let uid = String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .arg("rexon-de-helper")
            .args(["plasma", "arch", log_path])
            .env("PATH", path)
            .env("PKEXEC_UID", uid.trim())
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn журнал_пишет_вызвавший_а_не_root() {
        let log = format!("/tmp/rexon-de-log-test-{}.log", std::process::id());
        std::fs::write(&log, "").unwrap();
        let (code, err) = run_log_part("own", &log);
        let text = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_file(&log);
        assert_eq!(code, 3, "{err}");
        assert!(text.contains("REXON_BODY"), "{text}");
        assert!(text.trim_end().ends_with("__REXON_EXIT__3"), "{text}");
    }

    #[test]
    fn журнал_вне_tmp_отвергнут() {
        let (code, err) = run_log_part("etc", "/etc/shadow");
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("outside /tmp"), "{err}");
    }

    /// ⛔ No chown and no writes into a user file as root: the file owner by a
    /// path string is something the user has time to swap.
    #[test]
    fn помощник_стола_не_меняет_владельца_и_не_пишет_журнал_от_root() {
        let text = de_helper_script();
        for line in text.lines().map(str::trim_start).filter(|l| !l.starts_with('#')) {
            assert!(!line.contains("chown"), "chown в помощнике установки стола: {line}");
        }
        assert!(!text.contains("exec >>"), "запись журнала от root");
        assert!(text.contains("setpriv --reuid=\"$log_uid\""));
    }
}

// ============================================================================
// The layout helper takes its own copy of the directory (28.09.2026)
// ============================================================================

#[cfg(test)]
mod stage_helper_copy_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn dir(tag: &str) -> PathBuf {
        let d = PathBuf::from("/tmp").join(format!("rexon-stage-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn uid() -> String {
        String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    }

    /// The helper up to placing by category, then a mark. As a regular user:
    /// nothing is written into the system.
    fn run(staged: &Path, pkexec_uid: &str) -> (bool, String) {
        let full = stage_helper_script();
        let cut = full.find("# ── Handling by category").expect("раздел раскладки");
        let script = format!(
            "{}[ -f \"$real_staged/bg.png\" ] && echo REXON_VALIDATED\nexit 0\n",
            &full[..cut]
        );
        let out = Command::new("bash")
            .arg("-c")
            .arg(script)
            .arg("rexon-stage-helper")
            .args(["gdm-bg", "rexon", &staged.to_string_lossy()])
            .env("PKEXEC_UID", pkexec_uid)
            .output()
            .unwrap();
        let ok = out.status.success() && String::from_utf8_lossy(&out.stdout).contains("REXON_VALIDATED");
        (ok, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    #[test]
    fn свой_каталог_проходит_копией() {
        let d = dir("good");
        std::fs::write(d.join("bg.png"), b"\x89PNG").unwrap();
        let (ok, err) = run(&d, &uid());
        assert!(ok, "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ссылка_чужой_и_внешний_каталог_отвергнуты() {
        let d = dir("bad");
        std::fs::write(d.join("bg.png"), b"\x89PNG").unwrap();
        symlink("/etc/shadow", d.join("shadow.png")).unwrap();
        let (ok, err) = run(&d, &uid());
        assert!(!ok && err.contains("link"), "{err}");
        std::fs::remove_file(d.join("shadow.png")).unwrap();
        let (ok, err) = run(&d, "0");
        assert!(!ok && err.contains("does not belong to the caller"), "{err}");
        let link = PathBuf::from("/tmp").join(format!("rexon-stage-test-lnk-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink("/usr", &link).unwrap();
        let (ok, err) = run(&link, &uid());
        assert!(!ok && err.contains("outside /tmp"), "{err}");
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_dir_all(&d);
    }
}

// ============================================================================
// Package helper: rebuild steps (build-deps, install-built), 28.10.2026
// ============================================================================

/// The whole helper, as a regular user, with fake `pacman`, `getent` and
/// `chown` (the real pacman is never called), its own os-release
/// (`ID=arch`) and its own work directory instead of `/var/tmp`.
#[cfg(test)]
mod pkg_helper_rebuild_tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct Stand {
        root: PathBuf,
        home: PathBuf,
        script: PathBuf,
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn uid() -> String {
        String::from_utf8(Command::new("id").arg("-u").output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string()
    }

    fn exec(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn stand(tag: &str) -> Stand {
        let root = std::env::temp_dir().join(format!("rexon-pkg-rebuild-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let stub = root.join("stub");
        let home = root.join("home");
        let work = root.join("work");
        for d in [&stub, &home.join(".cache/rexon/rebuild/foo"), &work] {
            std::fs::create_dir_all(d).unwrap();
        }
        // pacman: prints argv; for `-U`, the contents and permissions of the file it
        // got (it must be the helper's copy, not the user's file). The package
        // database is two bench files: everything installed and "dependencies".
        // `-S` appends what it installed to both (like `--asdeps`).
        exec(
            &stub.join("pacman"),
            "#!/bin/bash\n\
             case \"$1\" in -Qq) cat \"$FAKE_DB\"; exit 0 ;; -Qqd) cat \"$FAKE_DEPS\"; exit 0 ;; esac\n\
             echo \"STUB pacman $*\"\n\
             if [ \"$1\" = -S ]; then while [ \"$1\" != -- ]; do shift; done; shift; \
             for a in \"$@\"; do n=\"${a%%[<>=]*}\"; echo \"$n\" >> \"$FAKE_DB\"; echo \"$n\" >> \"$FAKE_DEPS\"; done; \
             exit \"${FAKE_S_RC:-0}\"; fi\n\
             if [ \"$1\" = -U ]; then f=\"${@: -1}\"; echo \"GOT $(cat \"$f\") $(stat -c %a \"$f\")\"; \
             exit \"${FAKE_U_RC:-0}\"; fi\n\
             if [ \"$1\" = -Rns ]; then exit \"${FAKE_R_RC:-0}\"; fi\nexit 0\n",
        );
        std::fs::write(root.join("db"), "bash\nfirefox\ngimp\nlinux\n").unwrap();
        std::fs::write(root.join("deps"), "firefox\n").unwrap();
        exec(&stub.join("getent"), "#!/bin/bash\necho \"u:x:$3:$3::$FAKE_HOME:/bin/bash\"\n");
        exec(&stub.join("chown"), "#!/bin/bash\nexit 0\n");
        let osr = root.join("os-release");
        std::fs::write(&osr, "ID=arch\n").unwrap();

        let mut text = pkg_helper_script();
        for (from, to) in [
            ("export PATH=/usr/sbin:/usr/bin:/sbin:/bin", format!("export PATH={}:/usr/bin:/bin", stub.display())),
            ("for f in /etc/os-release /usr/lib/os-release; do", format!("for f in {}; do", osr.display())),
            ("mktemp -d /var/tmp/rexon-pkg-helper.XXXXXXXX", format!("mktemp -d {}/h.XXXXXXXX", work.display())),
        ] {
            assert!(text.contains(from), "строка помощника изменилась: {from}");
            text = text.replacen(from, &to, 1);
        }
        // The build-deps record directory everywhere (it is also in the header comment).
        assert!(text.contains(&format!("REC_DIR=\"{REBUILD_REC_DIR}\"")));
        text = text.replace(REBUILD_REC_DIR, &root.join("rec").display().to_string());
        let script = root.join("rexon-pkg-helper");
        exec(&script, &text);
        Stand { root, home, script }
    }

    impl Stand {
        fn rebuild(&self) -> PathBuf {
            self.home.join(".cache/rexon/rebuild/foo")
        }

        fn run_env(&self, args: &[&str], pkexec_uid: Option<&str>, env: &[(&str, &str)]) -> (i32, String, String) {
            let mut cmd = Command::new("bash");
            cmd.arg(&self.script)
                .args(args)
                .env("FAKE_HOME", &self.home)
                .env("FAKE_DB", self.root.join("db"))
                .env("FAKE_DEPS", self.root.join("deps"))
                .env_remove("PKEXEC_UID");
            if let Some(u) = pkexec_uid {
                cmd.env("PKEXEC_UID", u);
            }
            for (k, v) in env {
                cmd.env(k, v);
            }
            let out = cmd.output().unwrap();
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        }

        fn run(&self, args: &[&str]) -> (i32, String, String) {
            self.run_env(args, Some(&uid()), &[])
        }

        fn record(&self) -> Option<String> {
            std::fs::read_to_string(self.root.join("rec").join(uid())).ok()
        }

        fn work_is_clean(&self) -> bool {
            std::fs::read_dir(self.root.join("work")).unwrap().next().is_none()
        }
    }

    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn свой_собранный_файл_ставится_копией_и_сборочные_снимаются() {
        let st = stand("ok");
        let file = st.rebuild().join("foo-1:2.0-1-x86_64.pkg.tar.zst");
        std::fs::write(&file, "PKG").unwrap();
        let f = file.to_string_lossy().into_owned();
        let (code, _, err) = st.run(&["arch", "build-deps", "vala", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(st.record().as_deref(), Some("base-devel\nvala\n"), "запись build-deps");
        let (code, out, err) = st.run(&["arch", "install-built", "plain", &f, "vala", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        let u = out.lines().find(|l| l.starts_with("STUB pacman -U")).expect("pacman -U");
        assert!(u.starts_with("STUB pacman -U --noconfirm -- "), "{u}");
        assert!(!u.contains("--ask") && !u.contains(&f), "pacman получил файл пользователя: {u}");
        assert!(out.contains("GOT PKG 600"), "{out}");
        assert!(out.contains("STUB pacman -Rns --noconfirm -- vala base-devel"), "{out}");
        assert!(st.work_is_clean(), "своя копия не убрана");
        assert_eq!(st.record(), None, "запись build-deps не стёрта");

        let (code, out, _) = st.run(&["arch", "install-built", "replace", &f]);
        assert_eq!(code, 0);
        assert!(out.contains("STUB pacman -U --noconfirm --ask=4 -- "), "{out}");
        assert!(!out.contains("-Rns"), "{out}");

        // Build deps not removed: not an error, but said out loud.
        st.run(&["arch", "build-deps", "gperf"]);
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &f, "gperf"], Some(&uid()), &[("FAKE_R_RC", "1")]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("build-only packages kept"), "{out}");
        // Did not install: pacman's code, build deps are not touched.
        let (code, out, _) = st.run_env(&["arch", "install-built", "plain", &f, "vala"], Some(&uid()), &[("FAKE_U_RC", "1")]);
        assert_eq!(code, 1);
        assert!(!out.contains("-Rns"), "{out}");
    }

    /// Nothing but its own makepkg file from its own rebuild directory: each
    /// refusal is code 2 and not a single pacman call.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn чужой_внешний_ссылка_и_не_пакет_отвергнуты() {
        let st = stand("bad");
        let good = st.rebuild().join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&good, "PKG").unwrap();
        // Outside the rebuild directory.
        let outside = st.root.join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&outside, "EVIL").unwrap();
        // A link inside the directory to a file outside.
        let link = st.rebuild().join("bar-1-1-x86_64.pkg.tar.zst");
        symlink(&outside, &link).unwrap();
        // A directory link inside the rebuild root leading outside.
        let evil_dir = st.root.join("evil");
        std::fs::create_dir_all(&evil_dir).unwrap();
        std::fs::write(evil_dir.join("foo-1-1-x86_64.pkg.tar.zst"), "EVIL").unwrap();
        symlink(&evil_dir, st.home.join(".cache/rexon/rebuild/evil")).unwrap();
        // Not a package file by name.
        for name in ["foo.tar.gz", "foo.sh", "foo.pkg.tar.zst.sig", ".foo-1-1-x86_64.pkg.tar.zst"] {
            std::fs::write(st.rebuild().join(name), "EVIL").unwrap();
        }
        // A directory with a package name.
        std::fs::create_dir_all(st.rebuild().join("dir-1-1-x86_64.pkg.tar.zst")).unwrap();

        let p = |path: &Path| path.to_string_lossy().into_owned();
        let rb = p(&st.rebuild());
        let cases: Vec<(Vec<String>, &str)> = vec![
            (vec![p(&outside)], "outside"),
            (vec![p(&link)], "link"),
            (vec![format!("{}/evil/foo-1-1-x86_64.pkg.tar.zst", p(&st.home.join(".cache/rexon/rebuild")))], "outside"),
            (vec![format!("{rb}/foo.tar.gz")], "makepkg"),
            (vec![format!("{rb}/foo.sh")], "makepkg"),
            (vec![format!("{rb}/foo.pkg.tar.zst.sig")], "makepkg"),
            (vec![format!("{rb}/.foo-1-1-x86_64.pkg.tar.zst")], "makepkg"),
            (vec![format!("{rb}/dir-1-1-x86_64.pkg.tar.zst")], "not a regular file"),
            (vec!["foo-1-1-x86_64.pkg.tar.zst".into()], "outside"),
            (vec!["/usr/pacman-7.0.0-1-x86_64.pkg.tar.zst".into()], "outside"),
            (vec![format!("{rb}/../foo/foo-1-1-x86_64.pkg.tar.zst"), "linux".into()], "vital"),
            (vec![p(&good), "/tmp/x.pkg.tar.zst".into()], "not a path"),
            (vec![p(&good), "-dd".into()], "'-'"),
        ];
        for (rest, why) in &cases {
            let mut args = vec!["arch", "install-built", "plain"];
            args.extend(rest.iter().map(String::as_str));
            let (code, out, err) = st.run(&args);
            assert_eq!(code, 2, "{rest:?}: не отказал ({err})");
            assert!(!out.contains("STUB"), "{rest:?}: pacman позван: {out}");
            assert!(err.contains(why), "{rest:?}: ждали «{why}»: {err}");
        }
        // Mode of exactly two words only.
        let (code, out, _) = st.run(&["arch", "install-built", "--ask=4", &p(&good)]);
        assert_eq!(code, 2);
        assert!(!out.contains("STUB"));
        // Not via pkexec and a foreign caller (the directory is not theirs).
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &p(&good)], None, &[]);
        assert!(code == 2 && !out.contains("STUB") && err.contains("pkexec"), "{err}");
        let other = (uid().parse::<u64>().unwrap() + 1).to_string();
        let (code, out, err) = st.run_env(&["arch", "install-built", "plain", &p(&good)], Some(&other), &[]);
        assert!(code == 2 && !out.contains("STUB") && err.contains("outside"), "{err}");
        assert!(st.work_is_clean());
    }

    /// ⛔ install-built cannot remove an unrelated package: only what build-deps
    /// installed for THIS caller, and only with the "dependency" mark. Without a
    /// record nothing is removed.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn посторонний_пакет_не_снимается() {
        let st = stand("foreign");
        let file = st.rebuild().join("foo-1-1-x86_64.pkg.tar.zst");
        std::fs::write(&file, "PKG").unwrap();
        let f = file.to_string_lossy().into_owned();
        // No record at all: firefox (installed, even as a "dependency") stays.
        let (code, out, err) = st.run(&["arch", "install-built", "plain", &f, "firefox", "gimp"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("STUB pacman -U"), "{out}");
        assert!(!out.contains("-Rns"), "снято без записи: {out}");
        assert!(out.contains("firefox was not installed for this build"), "{out}");
        // There is a record, but it only has vala: unrelated ones are not removed.
        st.run(&["arch", "build-deps", "vala"]);
        let (code, out, _) = st.run(&["arch", "install-built", "plain", &f, "vala", "firefox", "gimp"]);
        assert_eq!(code, 0);
        assert!(out.contains("STUB pacman -Rns --noconfirm -- vala\n"), "{out}");
        // Installed by build-deps, but the "dependency" mark was removed: it stays.
        st.run(&["arch", "build-deps", "meson"]);
        std::fs::write(st.root.join("deps"), "firefox\n").unwrap();
        let (_, out, _) = st.run(&["arch", "install-built", "plain", &f, "meson"]);
        assert!(!out.contains("-Rns"), "{out}");
        // Someone else's record (a different caller) does not count.
        st.run(&["arch", "build-deps", "ninja"]);
        let rec = st.root.join("rec");
        std::fs::rename(rec.join(uid()), rec.join("999999")).unwrap();
        let (_, out, _) = st.run(&["arch", "install-built", "plain", &f, "ninja"]);
        assert!(!out.contains("-Rns"), "{out}");
    }

    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn сборочные_зависимости_только_по_именам() {
        let st = stand("deps");
        let (code, out, err) = st.run(&["arch", "build-deps", "cmake>=3.20", "vala", "libfoo.so=1-64", "base-devel"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            out.trim(),
            "STUB pacman -S --needed --noconfirm --asdeps -- cmake>=3.20 vala libfoo.so=1-64 base-devel"
        );
        for bad in ["/tmp/x.pkg.tar.zst", "x.pkg.tar.zst", "-dd", "cmake>=$(id)", "cmake>=", "a>=1 b", "../x", ">=1"] {
            let (code, out, _) = st.run(&["arch", "build-deps", bad]);
            assert_eq!(code, 2, "{bad}");
            assert!(!out.contains("STUB"), "{bad}: {out}");
        }
        let (code, out, _) = st.run(&["arch", "build-deps"]);
        assert!(code == 2 && !out.contains("STUB"));
    }

    /// Not Arch: both operations refuse right away.
    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
    fn шаги_пересборки_только_на_arch() {
        let st = stand("deb");
        std::fs::write(st.root.join("os-release"), "ID=debian\n").unwrap();
        for args in [["debian", "build-deps", "vala"], ["arch", "install-built", "plain"]] {
            let (code, out, err) = st.run(&args);
            assert_eq!(code, 2, "{args:?}");
            assert!(!out.contains("STUB"));
            assert!(err.contains("only with pacman"), "{err}");
        }
    }
}

#[cfg(test)]
mod pacman_lock_path_tests {
    use super::*;

    const DEFAULT_LOCK: &str = "/var/lib/pacman/db.lck";

    #[test]
    fn usual_dbpath() {
        assert_eq!(pacman_lock_path(Some("/var/lib/pacman/\n")), DEFAULT_LOCK);
    }

    #[test]
    fn own_dbpath() {
        assert_eq!(pacman_lock_path(Some("/srv/pacdb/\n")), "/srv/pacdb/db.lck");
    }

    #[test]
    fn dbpath_without_trailing_slash() {
        assert_eq!(pacman_lock_path(Some("/srv/pacdb\n")), "/srv/pacdb/db.lck");
        assert_eq!(pacman_lock_path(Some("  /srv/pacdb  ")), "/srv/pacdb/db.lck");
    }

    #[test]
    fn empty_or_missing_output_falls_back() {
        assert_eq!(pacman_lock_path(None), DEFAULT_LOCK);
        assert_eq!(pacman_lock_path(Some("")), DEFAULT_LOCK);
        assert_eq!(pacman_lock_path(Some("\n  \n")), DEFAULT_LOCK);
    }

    #[test]
    fn garbage_falls_back() {
        for junk in [
            "error: config file /etc/pacman.conf could not be read: No such file\n",
            "var/lib/pacman/\n",
            "/a/\n/b/\n",
            "/srv/pa\u{1b}[0mcdb/\n",
            "DBPath = /srv/pacdb/\n",
        ] {
            assert_eq!(pacman_lock_path(Some(junk)), DEFAULT_LOCK, "{junk:?}");
        }
    }

    #[test]
    fn other_families_keep_static_paths() {
        for f in [PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse, PkgFamily::Unknown] {
            let want: Vec<String> = lock_paths(f).iter().map(|s| s.to_string()).collect();
            assert_eq!(lock_paths_live(f), want, "{}", f.as_str());
        }
    }
}

#[cfg(test)]
mod install_preview_tests {
    use super::*;

    /// How pacman splits a refusal across streams (measured 08.10.2026):
    /// "error:" goes to stderr, the list of unresolvable dependencies to stdout.
    const ERR_ONLINE_ACCOUNTS: &str = "error: failed to prepare transaction (could not satisfy dependencies)\n";
    const OUT_ONLINE_ACCOUNTS: &str = ":: unable to satisfy dependency 'libcamel-1.2.so=67-64' required by switchboard-plug-online-accounts\n\
        :: unable to satisfy dependency 'libedataserver-1.2.so=27-64' required by switchboard-plug-online-accounts\n";

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// Fake pacman with the behaviour recorded on 08.10.2026: the `pantheon`
    /// group (a repeat from two repositories), a broken group member.
    fn pantheon(cmd: &Cmd) -> PreviewAnswer {
        let a = &cmd.argv;
        if a[1] == "-Sg" {
            return Some((
                1,
                "pantheon gala\npantheon switchboard-plug-online-accounts\npantheon wingpanel\n\
                 pantheon gala\npantheon wingpanel\n"
                    .into(),
                "error: package group 'lightdm' was not found\n".into(),
            ));
        }
        if a[1] == "-Qq" {
            return Some((0, "glibc\n".into(), String::new()));
        }
        if a[1] == "-Si" || a[1] == "-Qi" {
            // Conflict check: this set has none.
            return Some((0, String::new(), String::new()));
        }
        assert_eq!(&a[..6], &names(&["pacman", "-Sp", "--needed", "--noconfirm", "--print-format", "%n"])[..]);
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C");
        if a.iter().any(|x| x == "pantheon" || x == "switchboard-plug-online-accounts") {
            return Some((1, OUT_ONLINE_ACCOUNTS.into(), ERR_ONLINE_ACCOUNTS.into()));
        }
        Some((0, "gala\nwingpanel\nmutter\nlightdm\n".into(), String::new()))
    }

    fn conflict_pairs(c: &[PkgConflict]) -> Vec<(&str, &str)> {
        c.iter().map(|c| (c.new_package.as_str(), c.installed.as_str())).collect()
    }

    /// Recorded 19.10.2026 (CachyOS, KDE Plasma 6.8): `LC_ALL=C pacman -Si
    /// deepin-kwin`, a block from cachyos-extra-znver4 and a block from extra.
    const SI_DEEPIN_KWIN: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : deepin-kwin\n\
        Version         : 6.0.8-12.1\n\
        Provides        : None\n\
        Depends On      : qt6-base  kwayland  deepin-qt6platform-plugins\n\
        Conflicts With  : kwin  kwin-x11\n\
        Replaces        : None\n\
        \n\
        Repository      : extra\n\
        Name            : deepin-kwin\n\
        Version         : 6.0.8-12\n\
        Conflicts With  : kwin  kwin-x11\n\
        \n\
        Repository      : extra\n\
        Name            : deepin-session-shell\n\
        Conflicts With  : deepin-session-ui<5\n\
        \n\
        Repository      : extra\n\
        Name            : lightdm\n\
        Conflicts With  : None\n";

    /// Installing Deepin on a machine with KDE Plasma (the owner's live case of
    /// 19.10.2026): the `-Sp` trial transaction does NOT see conflicts (code 0),
    /// while the real one fails on "deepin-kwin and kwin are in conflict". The
    /// conflict is found in advance via `Conflicts With` of what will be
    /// installed, and installing is not allowed. A version constraint
    /// (`deepin-session-ui<5`) does not count.
    fn deepin_on_kde(cmd: &Cmd) -> PreviewAnswer {
        let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
        match a.as_slice() {
            ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
            ["pacman", "-Sp", ..] => Some((
                0,
                "deepin-kwin\ndeepin-session-shell\nlightdm\n".into(),
                String::new(),
            )),
            ["pacman", "-Qq"] => Some((0, "glibc\nkwin\nplasma-workspace\ndeepin-session-ui\n".into(), String::new())),
            ["pacman", "-Si", rest @ ..] => {
                assert_eq!(rest, ["deepin-kwin", "deepin-session-shell", "lightdm"]);
                Some((0, SI_DEEPIN_KWIN.into(), String::new()))
            }
            ["pacman", "-Qi"] => Some((
                0,
                "Name            : kwin\nConflicts With  : None\n\n\
                 Name            : glibc\nConflicts With  : None\n"
                    .into(),
                String::new(),
            )),
            other => panic!("неожиданная команда {other:?}"),
        }
    }

    #[test]
    fn conflict_with_installed_package_blocks_install() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_on_kde).unwrap();
        assert_eq!(
            conflict_pairs(&p.conflicts),
            vec![("deepin-kwin", "kwin")]
        );
        assert!(!p.installable());
        assert!(p.blocked.is_empty());
    }

    /// The reverse side: the conflict is declared by an INSTALLED package
    /// (`tuned-ppd` versus `power-profiles-daemon`, which COSMIC expects).
    #[test]
    fn conflict_declared_by_installed_package_blocks_install() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
                ["pacman", "-Sp", ..] => Some((0, "power-profiles-daemon\ncosmic-comp\n".into(), String::new())),
                ["pacman", "-Qq"] => Some((0, "glibc\ntuned-ppd\n".into(), String::new())),
                ["pacman", "-Si", ..] => Some((
                    0,
                    "Name            : power-profiles-daemon\nConflicts With  : None\n\n\
                     Name            : cosmic-comp\nConflicts With  : None\n"
                        .into(),
                    String::new(),
                )),
                ["pacman", "-Qi"] => Some((
                    0,
                    "Name            : tuned-ppd\nVersion         : 2.26.0-1\n\
                     Conflicts With  : power-profiles-daemon\n\n\
                     Name            : glibc\nConflicts With  : None\n"
                        .into(),
                    String::new(),
                )),
                other => panic!("неожиданная команда {other:?}"),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["cosmic-comp", "power-profiles-daemon"]), &run).unwrap();
        assert_eq!(
            conflict_pairs(&p.conflicts),
            vec![("power-profiles-daemon", "tuned-ppd")]
        );
        assert!(!p.installable());
    }

    /// Recorded 20.10.2026 (CachyOS, MATE; Budgie installed next to it):
    /// `LC_ALL=C pacman -Si mutter49 gala lightdm` and `pacman -Qi`: mutter 51
    /// on the machine is held by budgie-desktop, while Pantheon (gala) needs
    /// mutter49, which declares `Conflicts With: mutter`.
    const SI_PANTHEON_MUTTER49: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : mutter49\n\
        Version         : 49.8-1.1\n\
        Groups          : None\n\
        Provides        : libmutter-17.so=0-64\n\
        Depends On      : at-spi2-core  cairo  glib2\n\
        \x20                 gtk4  xorg-xwayland\n\
        Conflicts With  : mutter\n\
        \n\
        Repository      : extra\n\
        Name            : mutter49\n\
        Version         : 49.8-1\n\
        Conflicts With  : mutter\n\
        \n\
        Repository      : cachyos-extra-znver4\n\
        Name            : gala\n\
        Version         : 8.6.2-1.1\n\
        Groups          : pantheon\n\
        Depends On      : at-spi2-core  cairo  dconf  libgee  mutter49  pango\n\
        Conflicts With  : None\n\
        \n\
        Repository      : extra\n\
        Name            : lightdm\n\
        Conflicts With  : None\n";
    const QI_BUDGIE_MUTTER: &str = "Name            : glibc\n\
        Version         : 2.42-1\n\
        Required By     : at-spi2-core  bash  budgie-desktop  mutter\n\
        Conflicts With  : None\n\
        \n\
        Name            : mutter\n\
        Version         : 51.0-3\n\
        Groups          : None\n\
        Provides        : libmutter-51.so=0-64  mutter\n\
        Required By     : budgie-desktop\n\
        Conflicts With  : None\n\
        \n\
        Name            : budgie-desktop\n\
        Version         : 10.10.1-1\n\
        Groups          : budgie\n\
        Required By     : None\n\
        Conflicts With  : None\n";

    fn pantheon_on_budgie(cmd: &Cmd) -> PreviewAnswer {
        let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
        assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
        match a.as_slice() {
            ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
            ["pacman", "-Sp", ..] => Some((0, "mutter49\ngala\nlightdm\n".into(), String::new())),
            ["pacman", "-Qq"] => Some((0, "glibc\nmutter\nbudgie-desktop\n".into(), String::new())),
            ["pacman", "-Si", ..] => Some((0, SI_PANTHEON_MUTTER49.into(), String::new())),
            ["pacman", "-Qi"] => Some((0, QI_BUDGIE_MUTTER.into(), String::new())),
            other => panic!("неожиданная команда {other:?}"),
        }
    }

    /// The conflict is named by DESKTOPS, not packages: mutter is installed
    /// because budgie-desktop requires it (Required By), and mutter49 is pulled
    /// in by gala from the pantheon group (Depends On). A table-based "mutter is
    /// GNOME" would be a lie here: there is no GNOME on the machine.
    #[test]
    fn conflict_names_desktops_through_required_by() {
        let p = install_preview(PkgFamily::Arch, &names(&["gala", "lightdm"]), &pantheon_on_budgie).unwrap();
        assert!(!p.installable());
        assert_eq!(p.conflicts.len(), 1, "{:#?}", p.conflicts);
        let c = &p.conflicts[0];
        assert_eq!((c.new_package.as_str(), c.installed.as_str()), ("mutter49", "mutter"));
        assert_eq!(c.new_desktops, names(&["Pantheon"]));
        assert_eq!(c.installed_desktops, names(&["Budgie"]));
        assert_eq!(c.new_version, "49.8-1.1");
        assert_eq!(c.installed_version, "51.0-3");
        assert!(c.same_program);
    }

    /// Recorded 24.10.2026 (CachyOS): `LC_ALL=C pacman -Si river-classic` and
    /// `pacman -Qi river` with River 0.5 from extra installed.
    const SI_RIVER_CLASSIC: &str = "Repository      : cachyos-extra-znver4\n\
        Name            : river-classic\n\
        Version         : 0.3.17-2.1\n\
        Provides        : wayland-compositor\n\
        Depends On      : glibc  libevdev  libinput  libxkbcommon  mesa  pixman  sh  wayland  wlroots0.20  xorg-xwayland\n\
        Conflicts With  : river\n\
        Replaces        : None\n\
        \n\
        Repository      : extra\n\
        Name            : waybar\n\
        Conflicts With  : None\n";
    const QI_RIVER_05: &str = "Name            : river\n\
        Version         : 0.5.0-1.1\n\
        Provides        : None\n\
        Required By     : None\n\
        Conflicts With  : river-classic\n\
        \n\
        Name            : glibc\n\
        Conflicts With  : None\n";

    /// Probe of the River set on a machine with River 0.5; `required_by` is who
    /// requires the installed river (empty: nobody).
    fn river_over_05(required_by: &'static str) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((1, String::new(), String::new())),
                ["pacman", "-Sp", ..] => Some((0, "river-classic\nwaybar\n".into(), String::new())),
                ["pacman", "-Qq"] => Some((0, "glibc\nriver\n".into(), String::new())),
                ["pacman", "-Si", ..] => Some((0, SI_RIVER_CLASSIC.into(), String::new())),
                ["pacman", "-Qi"] => Some((
                    0,
                    QI_RIVER_05.replace("Required By     : None", &format!("Required By     : {required_by}")),
                    String::new(),
                )),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    /// River 0.5 is installed: the set is not installed silently (a refusal with
    /// a clear text about the River branches), but the conflict is resolved by a
    /// replacement with consent.
    #[test]
    fn river_05_conflict_is_explained_and_offered_for_replacement() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let p = install_preview(PkgFamily::Arch, &names(&["river-classic", "waybar"]), &river_over_05("None")).unwrap();
        assert_eq!(conflict_pairs(&p.conflicts), vec![("river-classic", "river")]);
        assert!(!p.installable());
        let text = conflict_explanation(&p.conflicts);
        assert!(text.contains("River 0.5") && text.contains("river-classic"), "{text}");
        assert!(!text.contains("conflicts with the installed package"), "общий текст не нужен: {text}");

        let swaps = conflict_swaps(&p);
        assert_eq!(conflict_pairs(&swaps), vec![("river-classic", "river")]);
        assert_eq!(conflict_swap_button(&swaps), "🔁 Replace River 0.5");
        let confirm = conflict_swap_confirm_text(&swaps, "River");
        assert!(confirm.contains("River 0.5 (package river)") && confirm.contains("~/.config/river/init"), "{confirm}");

        // Without consent there is no replacement; consent to a different name neither.
        assert!(!conflict_swap_approved(&p, &[]));
        assert!(!conflict_swap_approved(&p, &names(&["kwin"])));
        assert!(conflict_swap_approved(&p, &names(&["river"])));
    }

    /// Someone requires the installed river: pacman would remove it too, so we
    /// do not offer a replacement, the refusal stays.
    #[test]
    fn river_05_needed_by_another_package_is_not_offered_for_replacement() {
        let p = install_preview(PkgFamily::Arch, &names(&["river-classic", "waybar"]), &river_over_05("kwm")).unwrap();
        assert!(!p.installable());
        assert!(conflict_swaps(&p).is_empty());
        assert!(!conflict_swap_approved(&p, &names(&["river"])));
    }

    /// Another desktop is not resolved by replacement: Deepin next to KDE Plasma.
    #[test]
    fn foreign_desktop_conflict_is_never_offered_for_replacement() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_on_kde).unwrap();
        assert!(conflict_swaps(&p).is_empty());
        assert!(!conflict_swap_approved(&p, &names(&["kwin"])));
    }

    #[test]
    fn short_version_keeps_major_and_minor() {
        assert_eq!(short_version("0.5.0-1.1"), "0.5");
        assert_eq!(short_version("1:0.5.2-3"), "0.5");
        assert_eq!(short_version("7-1"), "7");
    }

    /// The replacement is the same `pacman -S` transaction with a "yes" answer to
    /// the conflict question; Arch only, names are checked as for the install.
    #[test]
    fn install_cmd_replacing_answers_the_conflict_question() {
        let pkgs = names(&["river-classic", "waybar"]);
        assert_eq!(install_cmd_replacing(PkgFamily::Arch, &pkgs, &[]).unwrap(), install_cmd(PkgFamily::Arch, &pkgs).unwrap());
        assert_eq!(
            install_cmd_replacing(PkgFamily::Arch, &pkgs, &names(&["river"])).unwrap(),
            names(&["pacman", "-S", "--needed", "--noconfirm", "--ask=4", "river-classic", "waybar"])
        );
        assert!(install_cmd_replacing(PkgFamily::Fedora, &pkgs, &names(&["river"])).is_err());
        assert!(install_cmd_replacing(PkgFamily::Arch, &pkgs, &names(&["/tmp/x.pkg.tar.zst"])).is_err());
    }

    /// With a replacement the set goes through the wrapper (the helper has no
    /// consent), and its script installs with `--ask=4`.
    #[test]
    fn batch_plan_with_replacement_skips_the_helper() {
        let det = DistroDetector::from_os_release_files(Some("ID=cachyos\nNAME=CachyOS\n"), None);
        let table = names(crate::system::de_entry_packages_for("river", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &names(&["river"]), &[], None, true, Some("river"), true,
            "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(!plan.used_de_helper);
        let script = script_of(&plan, "/tmp/a.sh");
        assert!(script.contains("pacman -S --needed --noconfirm --noprogressbar --color never --ask=4 river-classic"), "{script}");
    }

    /// The owner of the installed package is not a desktop: we name who requires it.
    #[test]
    fn conflict_names_requiring_packages_when_no_desktop_holds_it() {
        let qi = QI_BUDGIE_MUTTER
            .replace("Required By     : budgie-desktop\n", "Required By     : some-app\n")
            .replace("Name            : budgie-desktop", "Name            : some-app")
            .replace("Groups          : budgie\n", "Groups          : None\n");
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Qi" {
                return Some((0, qi.clone(), String::new()));
            }
            pantheon_on_budgie(cmd)
        };
        let p = install_preview(PkgFamily::Arch, &names(&["gala", "lightdm"]), &run).unwrap();
        let c = &p.conflicts[0];
        assert!(c.installed_desktops.is_empty(), "{c:#?}");
        assert_eq!(c.required_by, names(&["some-app"]));
    }

    /// Output of the owner's real transaction of 19.10.2026 (`--noconfirm`
    /// answers "N", nothing is removed).
    #[test]
    fn transaction_conflict_is_parsed() {
        let out = ":: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict. Remove kwin? [y/N] \n\
                   error: unresolvable package conflicts detected\n\
                   error: failed to prepare transaction (conflicting dependencies)\n\
                   :: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict\n";
        assert_eq!(
            parse_transaction_conflicts(out),
            vec![PkgConflict { new_package: "deepin-kwin".into(), installed: "kwin".into(), ..PkgConflict::default() }]
        );
        // Conflict via "provides" and an epoch in the version.
        assert_eq!(
            parse_transaction_conflicts(":: pipewire-pulse-1:1.4.9-1 and pulseaudio-17.0-3 are in conflict (pulseaudio). Remove pulseaudio? [y/N]"),
            vec![PkgConflict { new_package: "pipewire-pulse".into(), installed: "pulseaudio".into(), ..PkgConflict::default() }]
        );
        assert!(parse_transaction_conflicts("error: target not found: foo\n").is_empty());
    }

    #[test]
    fn broken_group_member_is_skipped_and_group_expanded() {
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon).unwrap();
        assert_eq!(p.groups, vec![("pantheon".to_string(), 3)]);
        assert_eq!(p.skipped, names(&["switchboard-plug-online-accounts"]));
        assert!(p.installable());
        assert_eq!(p.targets, names(&["gala", "wingpanel", "lightdm"]));
        assert_eq!(p.new_packages.len(), 4);
        assert_eq!(p.missing_members(), names(&["gala", "wingpanel", "lightdm"]));
        assert_eq!(p.dependency_count(), 1);
        assert_eq!(p.installed_members(), 0);
    }

    /// Recorded 15.11.2026 (CachyOS): `LC_ALL=C pacman -Si` of the members of
    /// the `pantheon` group (trimmed to what is needed). gala 8.6.2-4 moved to
    /// mutter 51; of the whole group only `lightdm-pantheon-greeter` pulls in mutter49.
    fn si_pantheon_block(name: &str, gala_on_mutter49: bool) -> Option<String> {
        let block = match name {
            "pantheon-session" => "Name            : pantheon-session\nVersion         : 8.1.0-1\nGroups          : pantheon\n\
                 Depends On      : gala  gnome-session  wingpanel\nConflicts With  : None\n"
                .to_string(),
            "gala" => format!(
                "Name            : gala\nVersion         : 8.6.2-4.1\nGroups          : pantheon\n\
                 Depends On      : glib2  {}  granite7\nConflicts With  : None\n",
                if gala_on_mutter49 { "mutter49" } else { "mutter" }
            ),
            "wingpanel" => format!(
                "Name            : wingpanel\nVersion         : 8.0.3-1\nGroups          : pantheon\n\
                 Depends On      : gala  {}\nConflicts With  : None\n",
                if gala_on_mutter49 { "mutter49" } else { "mutter" }
            ),
            "lightdm-pantheon-greeter" => "Name            : lightdm-pantheon-greeter\nVersion         : 8.1.2-2.1\n\
                 Groups          : pantheon\nDepends On      : lightdm  mutter49  granite\nConflicts With  : None\n"
                .to_string(),
            "mutter49" => "Repository      : cachyos-extra-znver4\nName            : mutter49\nVersion         : 49.8-1.1\n\
                 Groups          : None\nProvides        : libmutter-17.so=0-64\nDepends On      : glib2  gtk4\n\
                 Conflicts With  : mutter\n"
                .to_string(),
            "mutter" => "Name            : mutter\nVersion         : 51.1-1\nProvides        : libmutter-51.so=0-64\n\
                 Depends On      : glib2  gtk4\nConflicts With  : None\n"
                .to_string(),
            "lightdm" | "gnome-session" | "granite7" | "granite" => {
                format!("Name            : {name}\nVersion         : 1.0-1\nConflicts With  : None\n")
            }
            _ => return None,
        };
        Some(block)
    }

    /// Fake pacman: the `pantheon` group of four members; `with_mutter`: mutter 51
    /// is already installed (held by GNOME and Budgie).
    fn pantheon_real(with_mutter: bool, gala_on_mutter49: bool) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
            let installed: Vec<&str> =
                if with_mutter { vec!["glibc", "mutter", "gnome-shell", "budgie-desktop"] } else { vec!["glibc"] };
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((
                    1,
                    "pantheon gala\npantheon lightdm-pantheon-greeter\npantheon pantheon-session\npantheon wingpanel\n".into(),
                    "error: package group 'lightdm' was not found\n".into(),
                )),
                ["pacman", "-Sp", ..] => {
                    let targets = &cmd.argv[6..];
                    let wants = |n: &str| targets.iter().any(|t| t == n || t == "pantheon");
                    let mut plan: Vec<&str> = Vec::new();
                    for n in ["gala", "wingpanel", "pantheon-session", "lightdm-pantheon-greeter"] {
                        if wants(n) {
                            plan.push(n);
                        }
                    }
                    plan.extend(["gnome-session", "granite7", "lightdm"]);
                    let needs49 = wants("lightdm-pantheon-greeter") || (gala_on_mutter49 && wants("gala"));
                    if needs49 {
                        plan.push("mutter49");
                        plan.push("granite");
                    }
                    if !with_mutter && !gala_on_mutter49 && plan.iter().any(|n| ["gala", "wingpanel"].contains(n)) {
                        plan.push("mutter");
                    }
                    Some((0, plan.iter().map(|n| format!("{n}\n")).collect(), String::new()))
                }
                ["pacman", "-Qq"] => Some((0, installed.iter().map(|n| format!("{n}\n")).collect(), String::new())),
                ["pacman", "-Si", rest @ ..] => Some((
                    0,
                    rest.iter().filter_map(|n| si_pantheon_block(n, gala_on_mutter49)).collect::<Vec<_>>().join("\n"),
                    String::new(),
                )),
                ["pacman", "-Qi"] => Some((
                    0,
                    if with_mutter {
                        "Name            : mutter\nVersion         : 51.1-1\nRequired By     : gnome-shell  budgie-desktop\n\
                         Conflicts With  : None\n\n\
                         Name            : gnome-shell\nVersion         : 51.1-1\nRequired By     : None\nConflicts With  : None\n\n\
                         Name            : budgie-desktop\nVersion         : 10.10.1-1\nGroups          : budgie\n\
                         Required By     : None\nConflicts With  : None\n"
                            .into()
                    } else {
                        "Name            : glibc\nConflicts With  : None\n".into()
                    },
                    String::new(),
                )),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    /// A live defect of 15.11.2026: Pantheon with GNOME and Budgie installed was
    /// blocked entirely, although only the Pantheon login screen pulls in
    /// mutter49. Now the desktop is installed without it, and the person is told
    /// what will be missing.
    #[test]
    fn optional_member_conflict_is_dropped_not_blocking() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon_real(true, false)).unwrap();
        assert!(p.installable(), "{p:#?}");
        assert!(p.conflicts.is_empty());
        assert!(!p.targets.iter().any(|t| t == "lightdm-pantheon-greeter"), "{:?}", p.targets);
        assert!(p.targets.iter().any(|t| t == "gala") && p.targets.iter().any(|t| t == "pantheon-session"));
        assert!(!p.members.iter().any(|t| t == "lightdm-pantheon-greeter"));
        assert!(!p.new_packages.iter().any(|t| t == "mutter49"), "{:?}", p.new_packages);
        assert_eq!(p.conflict_skipped.len(), 1, "{:#?}", p.conflict_skipped);
        let s = &p.conflict_skipped[0];
        assert_eq!(s.package, "lightdm-pantheon-greeter");
        assert_eq!(s.desktop, "Pantheon");
        assert_eq!((s.conflict.new_package.as_str(), s.conflict.installed.as_str()), ("mutter49", "mutter"));
        assert_eq!(s.conflict.installed_desktops, names(&["GNOME", "Budgie"]));
        assert_eq!(
            conflict_skip_explanation(&p.conflict_skipped),
            "⚠️ Not installing the Pantheon login screen: it needs Mutter 49, and GNOME, Budgie needs \
             Mutter 51 — the two can't be installed at once. You can sign in to Pantheon from the \
             regular login screen; everything else will be installed."
        );
        assert_eq!(crate::qt_desktops::packages_notice(&p), conflict_skip_explanation(&p.conflict_skipped));
    }

    /// On a machine without mutter the same group pulls in BOTH mutter (gala)
    /// AND mutter49 (the login screen): `-Sp` stays silent, and the real
    /// transaction would fail on "mutter49 and mutter are in conflict". A
    /// conflict inside the set is caught the same way and resolved by the same
    /// dropping of the login screen.
    #[test]
    fn conflict_inside_the_set_drops_optional_member() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon_real(false, false)).unwrap();
        assert!(p.installable(), "{p:#?}");
        assert_eq!(p.conflict_skipped.len(), 1, "{:#?}", p.conflict_skipped);
        let s = &p.conflict_skipped[0];
        assert_eq!(s.package, "lightdm-pantheon-greeter");
        assert!(s.conflict.installed_is_new);
        assert_eq!(s.conflict.installed_desktops, names(&["Pantheon"]));
        assert!(p.new_packages.iter().any(|t| t == "mutter"));
        assert!(!p.new_packages.iter().any(|t| t == "mutter49"));
        assert!(conflict_skip_explanation(&p.conflict_skipped).starts_with(
            "⚠️ Not installing the Pantheon login screen: it needs Mutter 49, and Pantheon needs Mutter 51"
        ));
    }

    /// Without a conflict the whole group is installed, the login screen too
    /// (the whole desktop on mutter49, no mutter on the machine: that is how it
    /// was before gala 8.6.2-4).
    #[test]
    fn group_without_conflict_installs_whole() {
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon_real(false, true)).unwrap();
        assert!(p.installable(), "{p:#?}");
        assert!(p.conflict_skipped.is_empty());
        assert!(p.conflicts.is_empty());
        assert_eq!(p.targets, names(&["pantheon", "lightdm"]));
        assert!(p.members.iter().any(|t| t == "lightdm-pantheon-greeter"));
        assert!(p.new_packages.iter().any(|t| t == "mutter49"));
        assert!(crate::qt_desktops::packages_notice(&p).is_empty());
    }

    /// A conflict in a mandatory member (the desktop's window manager on
    /// mutter49, as before gala 8.6.2-4) is the old honest refusal: it cannot be dropped.
    #[test]
    fn essential_member_conflict_still_blocks() {
        let p = install_preview(PkgFamily::Arch, &names(&["pantheon", "lightdm"]), &pantheon_real(true, true)).unwrap();
        assert!(!p.installable(), "{p:#?}");
        assert!(p.conflict_skipped.is_empty(), "{:#?}", p.conflict_skipped);
        assert_eq!(conflict_pairs(&p.conflicts), vec![("mutter49", "mutter")]);
        assert!(!p.targets.is_empty());
    }

    #[test]
    fn broken_named_package_blocks() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((1, String::new(), String::new()));
            }
            Some((1, String::new(), "error: target not found: lightdm\n".into()))
        };
        let p = install_preview(PkgFamily::Arch, &names(&["lightdm"]), &run).unwrap();
        assert!(!p.installable());
        assert_eq!(p.blocked, names(&["lightdm"]));
    }

    #[test]
    fn nothing_new_when_all_installed() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((0, "cosmic a\ncosmic b\n".into(), String::new()));
            }
            Some((0, String::new(), "warning: a is up to date -- skipping\n".into()))
        };
        let p = install_preview(PkgFamily::Arch, &names(&["cosmic"]), &run).unwrap();
        assert!(p.new_packages.is_empty());
        assert_eq!(p.members, names(&["a", "b"]));
        assert_eq!(p.installed_members(), 2);
    }

    /// 19.10.2026, MATE: `pacman -Sp --needed` also prints INSTALLED packages
    /// that have a new build (mate-applets 1.28.1-3.1 with 1.28.1-2.1
    /// installed), and the card said "2 packages missing". Whether a package is
    /// installed is decided by the installed list, not by the transaction plan:
    /// such packages are updates, not missing ones.
    #[test]
    fn outdated_installed_members_are_updates_not_missing() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            match cmd.argv[1].as_str() {
                "-Sg" => Some((0, "mate mate-applets\nmate mate-panel\nmate mate-netbook\n".into(), String::new())),
                "-Qq" => Some((0, "mate-applets\nmate-netbook\nglibc\n".into(), String::new())),
                _ => Some((0, "mate-applets\nmate-netbook\nmate-panel\nlibmatepanel\n".into(), String::new())),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["mate"]), &run).unwrap();
        assert_eq!(p.new_packages, names(&["mate-panel", "libmatepanel"]));
        assert_eq!(p.updates, names(&["mate-applets", "mate-netbook"]));
        assert_eq!(p.missing_members(), names(&["mate-panel"]));
        assert_eq!(p.installed_members(), 2);
        assert_eq!(p.dependency_count(), 1);
    }

    /// Own build newer than the repository (live case of 08.11.2026: Caja
    /// 1.28.0-6.100 with a transparent background versus 1.28.0-6.1):
    /// `pacman -Qu` does not name it, so `-S --needed` would ROLL IT BACK. It is
    /// dropped from the transaction, the group is expanded into members without
    /// it, and it is not named as an update.
    #[test]
    fn own_build_newer_than_repo_is_kept_not_downgraded() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            let has = |n: &str| cmd.argv.iter().any(|a| a == n);
            match cmd.argv[1].as_str() {
                "-Sg" => Some((0, "mate caja\nmate mate-panel\nmate mate-applets\n".into(), String::new())),
                "-Qq" => Some((0, "caja\nmate-applets\nglibc\n".into(), String::new())),
                "-Qu" => Some((0, "mate-applets\n".into(), String::new())),
                // The first pass goes over the group: our own Caja gets into the plan too;
                // after expanding the group without it, only the others.
                _ if has("mate") || has("caja") => Some((0, "caja\nmate-applets\nmate-panel\n".into(), String::new())),
                _ => Some((0, "mate-applets\nmate-panel\n".into(), String::new())),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["mate"]), &run).unwrap();
        assert_eq!(p.kept_local, names(&["caja"]));
        assert!(!p.targets.contains(&"caja".to_string()), "в транзакцию не идёт: {:?}", p.targets);
        assert_eq!(p.updates, names(&["mate-applets"]));
        assert_eq!(p.new_packages, names(&["mate-panel"]));
    }

    /// `pacman -Qu` did not answer: nothing is dropped (the old behaviour).
    #[test]
    fn locally_newer_without_answer_keeps_nothing() {
        let none = |_: &Cmd| -> PreviewAnswer { None };
        assert!(locally_newer(&names(&["caja"]), &none).is_empty());
        let nothing = |_: &Cmd| -> PreviewAnswer { Some((1, String::new(), String::new())) };
        assert_eq!(locally_newer(&names(&["caja"]), &nothing), names(&["caja"]));
        let broken = |_: &Cmd| -> PreviewAnswer { Some((1, String::new(), "error: db".into())) };
        assert!(locally_newer(&names(&["caja"]), &broken).is_empty());
    }

    /// The installed list was not obtained: the old behaviour: everything the
    /// transaction installs counts as new (we invent nothing).
    #[test]
    fn without_installed_list_everything_counts_as_new() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            match cmd.argv[1].as_str() {
                "-Sg" => Some((1, String::new(), String::new())),
                "-Qq" => None,
                _ => Some((0, "a\nb\n".into(), String::new())),
            }
        };
        let p = install_preview(PkgFamily::Arch, &names(&["a"]), &run).unwrap();
        assert_eq!(p.new_packages, names(&["a", "b"]));
        assert!(p.updates.is_empty());
    }

    #[test]
    fn unknown_failure_and_other_families_say_nothing() {
        let run = |cmd: &Cmd| -> PreviewAnswer {
            if cmd.argv[1] == "-Sg" {
                return Some((1, String::new(), String::new()));
            }
            Some((1, String::new(), "error: failed to init transaction (unable to lock database)\n".into()))
        };
        assert!(install_preview(PkgFamily::Arch, &names(&["kde"]), &run).is_none());
        assert!(install_preview(PkgFamily::Debian, &names(&["kde"]), &run).is_none());
        assert!(install_preview(PkgFamily::Arch, &names(&["bad name"]), &run).is_none());
    }

    // ── Kernel modules for the installed kernels (Deepin on CachyOS, 20.10.2026) ──

    #[test]
    fn dkms_modules_only_without_the_arch_kernel() {
        let plan = names(&["deepin-file-manager", "deepin-anything", "deepin-anything-arch", "linux"]);
        // The stock kernel is installed: the prebuilt module works.
        assert!(!needs_dkms_modules(&plan, &names(&["linux"])));
        assert!(!needs_dkms_modules(&plan, &names(&["linux", "linux-cachyos"])));
        // Only other kernels: the stock one is not let into the transaction.
        assert!(needs_dkms_modules(&plan, &names(&["linux-cachyos"])));
        assert!(needs_dkms_modules(&plan, &names(&["linux-cachyos", "linux-cachyos-lts"])));
        assert!(needs_dkms_modules(&plan, &names(&["linux-lts"])));
        // Kernels unknown: as before.
        assert!(!needs_dkms_modules(&plan, &[]));
        // `linux` is not pulled in: nothing to change.
        assert!(!needs_dkms_modules(&names(&["deepin-anything-dkms", "dkms"]), &names(&["linux-cachyos"])));
    }

    #[test]
    fn dkms_names_and_headers() {
        assert_eq!(dkms_candidates("deepin-anything-arch"), names(&["deepin-anything-dkms", "deepin-anything-arch-dkms"]));
        assert_eq!(dkms_candidates("vhba-module"), names(&["vhba-module-dkms"]));
        assert_eq!(dkms_candidates("virtualbox-host-modules-arch")[0], "virtualbox-host-dkms");
        let installed = names(&["linux-cachyos", "linux-cachyos-headers", "linux-cachyos-lts"]);
        assert_eq!(
            missing_kernel_headers(&names(&["linux-cachyos", "linux-cachyos-lts"]), Some(&installed)),
            names(&["linux-cachyos-lts-headers"])
        );
        assert_eq!(missing_kernel_headers(&names(&["linux-lts"]), None), names(&["linux-lts-headers"]));
        let info = parse_pkg_info(
            "Name            : deepin-anything-arch\nDepends On      : linux\n\n\
             Name            : deepin-anything\nDepends On      : DEEPIN-ANYTHING-MODULE  qt6-base\n\n\
             Name            : linux\nDepends On      : coreutils  kmod\n",
        );
        assert_eq!(prebuilt_module_packages(&info), names(&["deepin-anything-arch"]));
    }

    /// Fake pacman of the owner's machine: kernels `kernels` (+ `-headers` from
    /// `headers_installed`), the `deepin` set pulls in `deepin-anything-arch` and
    /// `linux` while `deepin-anything-dkms` is not among the targets.
    fn deepin_machine(kernels: &'static [&'static str], installed_extra: &'static [&'static str]) -> impl Fn(&Cmd) -> PreviewAnswer {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Sg", ..] => Some((0, "deepin deepin-file-manager\ndeepin deepin-session\n".into(), String::new())),
                ["pacman", "-Qq"] => {
                    let mut list: Vec<&str> = vec!["glibc"];
                    list.extend(kernels);
                    list.extend(installed_extra);
                    Some((0, list.join("\n") + "\n", String::new()))
                }
                ["find", "/usr/lib/modules", ..] => Some((
                    0,
                    kernels.iter().enumerate().map(|(i, _)| format!("/usr/lib/modules/{i}.0-x/pkgbase\n")).collect(),
                    String::new(),
                )),
                ["pacman", "-Qqo", files @ ..] => {
                    assert_eq!(files.len(), kernels.len());
                    Some((0, kernels.iter().map(|k| format!("{k}\n")).collect(), String::new()))
                }
                ["pacman", "-Sp", ..] => {
                    let mut out = "deepin-file-manager\ndeepin-session\ndeepin-anything\n".to_string();
                    if a.contains(&"deepin-anything-dkms") {
                        out.push_str("deepin-anything-dkms\ndkms\n");
                        for t in a.iter().filter(|t| t.ends_with("-headers")) {
                            out.push_str(&format!("{t}\n"));
                        }
                    } else {
                        out.push_str("deepin-anything-arch\n");
                        if !kernels.contains(&"linux") {
                            out.push_str("linux\n");
                        }
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Si", rest @ ..] => {
                    let mut out = String::new();
                    for n in rest {
                        let deps = match *n {
                            "deepin-anything-arch" => "linux",
                            "deepin-anything-dkms" => "dkms",
                            "deepin-anything-arch-dkms" => continue,
                            _ => "glibc",
                        };
                        out.push_str(&format!("Name            : {n}\nDepends On      : {deps}\n\n"));
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Qi"] => Some((0, String::new(), String::new())),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    #[test]
    fn deepin_on_cachyos_takes_dkms_not_the_arch_kernel() {
        let run = deepin_machine(&["linux-cachyos", "linux-cachyos-lts"], &["linux-cachyos-headers"]);
        let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &run).unwrap();
        assert_eq!(p.targets, names(&["deepin", "lightdm", "deepin-anything-dkms", "linux-cachyos-lts-headers"]));
        assert!(!p.new_packages.iter().any(|n| n == "linux" || n == "deepin-anything-arch"), "{:?}", p.new_packages);
        assert!(p.new_packages.iter().any(|n| n == "deepin-anything-dkms"));
        // These are dependencies, not desktop packages; already installed headers
        // (`linux-cachyos-headers`) do not get the mark.
        assert!(!p.members.iter().any(|n| n == "deepin-anything-dkms"));
        assert_eq!(p.deps_only, names(&["deepin-anything-dkms", "linux-cachyos-lts-headers"]));
        // A kernel module loads at boot: a reboot is needed after the install; the
        // headers and `dkms` itself do not require one.
        assert_eq!(p.restart_packages, names(&["deepin-anything-dkms"]));
    }

    #[test]
    fn deepin_with_the_arch_kernel_keeps_the_prebuilt_module() {
        for kernels in [&["linux"][..], &["linux", "linux-cachyos"][..]] {
            let kernels: &'static [&'static str] = Box::leak(kernels.to_vec().into_boxed_slice());
            let p = install_preview(PkgFamily::Arch, &names(&["deepin", "lightdm"]), &deepin_machine(kernels, &[])).unwrap();
            assert_eq!(p.targets, names(&["deepin", "lightdm"]));
            assert!(p.new_packages.iter().any(|n| n == "deepin-anything-arch"));
            assert!(p.deps_only.is_empty());
            // A prebuilt module for the stock kernel: also a reboot.
            assert_eq!(p.restart_packages, names(&["deepin-anything-arch"]));
        }
    }

    /// Only a kernel and kernel modules require a reboot; the desktop, its
    /// services and the login screen do not. The dependencies from `pacman -Si`
    /// decide, and without them the names.
    #[test]
    fn restart_packages_are_kernels_and_kernel_modules() {
        let info = |name: &str, deps: &[&str]| PkgInfo {
            name: name.to_string(),
            depends: names(deps),
            ..PkgInfo::default()
        };
        let new = names(&["river", "foot", "wlroots0.19", "lightdm", "nvidia", "zfs-dkms", "linux", "linux-headers"]);
        let infos = [
            info("river", &["wlroots0.19", "libxkbcommon"]),
            info("foot", &["fcft"]),
            info("lightdm", &["systemd"]),
            info("nvidia", &["linux", "nvidia-utils"]),
            info("zfs-dkms", &["dkms"]),
            info("linux-headers", &["pahole"]),
        ];
        assert_eq!(restart_packages(&new, &infos), names(&["nvidia", "zfs-dkms", "linux"]));
        // No information: the names decide: `*-dkms` and the kernel itself.
        assert_eq!(restart_packages(&new, &[]), names(&["zfs-dkms", "linux"]));
        // An ordinary desktop (River 24.10.2026): logging out is enough.
        assert!(restart_packages(&names(&["river", "foot"]), &infos).is_empty());
    }

    #[test]
    fn deepin_on_arch_lts_only_gets_lts_headers() {
        let p = install_preview(PkgFamily::Arch, &names(&["deepin"]), &deepin_machine(&["linux-lts"], &[])).unwrap();
        assert_eq!(p.targets, names(&["deepin", "deepin-anything-dkms", "linux-lts-headers"]));
        assert!(!p.new_packages.iter().any(|n| n == "linux"));
    }

    #[test]
    fn de_helper_installs_only_its_own_table() {
        let table = names(crate::system::de_entry_packages_for("pantheon", "arch").unwrap());
        assert!(de_helper_covers("pantheon", PkgFamily::Arch, &table));
        let mut more = table.clone();
        more.push("vhba-module-dkms".into());
        assert!(!de_helper_covers("pantheon", PkgFamily::Arch, &more));
        // The helper installs its part of the table as always (Python parity,
        // the arch-de-helper-kde case).
        assert!(de_helper_covers("pantheon", PkgFamily::Arch, &table[1..]));
        assert!(!de_helper_covers("pantheon", PkgFamily::Arch, &names(&["gala"])));
    }

    /// The root helper does not install the `deepin` group (with deepin-session
    /// and deepin-kwin): Deepin only on Treeland, via the wrapper.
    #[test]
    fn de_helper_has_no_deepin() {
        assert!(!de_helper_supports("deepin", "arch"));
        assert!(!de_helper_pkg_table().contains("\"deepin|"));
        assert!(de_helper_pkg_table().contains("\"pantheon|arch\")"));
    }

    fn script_of(plan: &BatchPlan, path: &str) -> String {
        plan.steps
            .iter()
            .find_map(|s| match s {
                PlanStep::WriteFile { path: p, content, .. } if p == path => Some(content.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn kernel_module_deps_are_marked_in_the_same_script() {
        let det = DistroDetector::from_os_release_files(Some("ID=cachyos\nNAME=CachyOS\n"), None);
        let pkgs = names(&["deepin", "lightdm", "deepin-anything-dkms", "linux-cachyos-lts-headers"]);
        // A foreign name in the mark (not from the set) is dropped.
        let deps = names(&["deepin-anything-dkms", "linux-cachyos-lts-headers", "linux"]);
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &pkgs, &deps, &[], &[], None, true, Some("deepin"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        // A set not from the helper's table: the wrapper, one pkexec.
        assert!(!plan.used_de_helper);
        let script = script_of(&plan, "/tmp/a.sh");
        let install = script.find("pacman -S --needed").expect(&script);
        let mark = script.find("pacman -D --asdeps deepin-anything-dkms linux-cachyos-lts-headers\n").expect(&script);
        assert!(mark > install, "{script}");
        assert!(script.contains("set -e"), "пометка только после успешной установки");
        let pkexecs = plan.steps.iter().filter(|s| matches!(s, PlanStep::Run { needs_root: true, .. })).count();
        assert_eq!(pkexecs, 1);

        // Without a mark, as before: a set from the table goes through the helper.
        let table = names(crate::system::de_entry_packages_for("pantheon", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &[], &[], None, true, Some("pantheon"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(plan.used_de_helper);
        // Deepin never via the helper, even as a set from the table.
        let table = names(crate::system::de_entry_packages_for("deepin", "arch").unwrap());
        let plan = install_batch_plan_with_deps(
            PkgFamily::Arch, &det, &table, &[], &[], &[], None, true, Some("deepin"), true, "/tmp/a.sh", "/tmp/a.log",
        )
        .unwrap();
        assert!(!plan.used_de_helper);
        assert_eq!(mark_deps_line(PkgFamily::Debian, &deps).unwrap(), None);
    }
}

#[cfg(test)]
mod wayland_default_session_tests {
    use super::*;
    use std::collections::HashMap;

    fn fs(files: &[(&str, &str)]) -> HashMap<String, String> {
        files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
    }

    fn plan_on(files: &HashMap<String, String>) -> SessionSwitch {
        let read = |p: &str| files.get(p).cloned();
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        plan_wayland_default(&X11_SESSION_SUCCESSORS[0], &read, &list)
    }

    #[test]
    fn только_значение_ключа_сеанса_и_в_прежнем_виде() {
        let s = &X11_SESSION_SUCCESSORS[0];
        let keys = LOGIN_SESSION_SOURCES[2].keys;
        // Bare name, with `.desktop`, a full path; foreign keys and groups, the
        // same key in a foreign group, a comment are not touched.
        let text = "# Session=plasmax11\n[Autologin]\nUser=me\nSession=plasmax11\n[Greeter]\nPreselectedSession =  plasmax11.desktop\n\
                    Session=plasmax11\n[Other]\nSession=plasmax11\n[Autologin][seat1]\nSession=plasmax11";
        assert_eq!(
            retarget_session(text, keys, s).unwrap(),
            "# Session=plasmax11\n[Autologin]\nUser=me\nSession=plasma\n[Greeter]\nPreselectedSession =  plasma.desktop\n\
             Session=plasmax11\n[Other]\nSession=plasmax11\n[Autologin][seat1]\nSession=plasmax11"
        );
        let sddm_state = "[Last]\nSession=/usr/share/xsessions/plasmax11.desktop\n";
        assert_eq!(
            retarget_session(sddm_state, LOGIN_SESSION_SOURCES[1].keys, s).unwrap(),
            "[Last]\nSession=/usr/share/wayland-sessions/plasma.desktop\n"
        );
        // Already Wayland or another X11 session: nothing to change.
        assert_eq!(retarget_session("[Autologin]\nSession=plasma\n", keys, s), None);
        assert_eq!(retarget_session("[Autologin]\nSession=xfce\n", keys, s), None);
    }

    fn theme_on(theme: &str, files: &HashMap<String, String>) -> SessionSwitch {
        let read = |p: &str| files.get(p).cloned();
        let list = |d: &str| {
            files.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>()
        };
        plan_sddm_theme(theme, &read, &list)
    }

    #[test]
    fn тема_sddm_перекрытая_etc_sddm_conf_меняется_и_там() {
        // As on CachyOS: the theme in `/etc/sddm.conf` (read last) and in the KDE
        // settings. Both are edited, no own file is needed, the rest byte for byte.
        let files = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=breeze\nCursorTheme=breeze_cursors\n"),
            ("/etc/sddm.conf", "[General]\nX=1\n\n[Theme]\nCurrent=breeze\nCursorSize=32\n"),
        ]);
        let SessionSwitch::Edits(e) = theme_on("sugar-dark", &files) else { panic!() };
        assert_eq!(
            e,
            vec![
                SessionEdit {
                    path: "/etc/sddm.conf.d/kde_settings.conf".into(),
                    text: "[Autologin]\nSession=plasma\n\n[Theme]\nCurrent=sugar-dark\nCursorTheme=breeze_cursors\n".into(),
                },
                SessionEdit { path: "/etc/sddm.conf".into(), text: "[General]\nX=1\n\n[Theme]\nCurrent=sugar-dark\nCursorSize=32\n".into() },
            ]
        );
        // The result is by the same reading as SDDM's.
        let mut after = files.clone();
        for x in e {
            after.insert(x.path, x.text);
        }
        let read = |p: &str| after.get(p).cloned();
        let list = |d: &str| after.keys().filter_map(|p| p.strip_prefix(&format!("{d}/")).map(str::to_string)).collect::<Vec<_>>();
        assert_eq!(sddm_effective_theme(&read, &list).as_deref(), Some("sugar-dark"));
    }

    #[test]
    fn тема_sddm_без_ключа_в_etc_свой_файл() {
        // Not set anywhere: an own file.
        assert_eq!(
            theme_on("t", &fs(&[])),
            SessionSwitch::Edits(vec![SessionEdit { path: SDDM_REXON_THEME_CONF.into(), text: "[Theme]\nCurrent=t\n".into() }])
        );
        // Only in the package file: we do not touch it, we override it with ours.
        let SessionSwitch::Edits(e) = theme_on("t", &fs(&[("/usr/lib/sddm/sddm.conf.d/default.conf", "[Theme]\nCurrent=\n")])) else {
            panic!()
        };
        assert_eq!(e.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), [SDDM_REXON_THEME_CONF]);
        // The key in a foreign group and in a comment is not a theme.
        let SessionSwitch::Edits(e) = theme_on("t", &fs(&[("/etc/sddm.conf", "# Current=x\n[General]\nCurrent=x\n")])) else {
            panic!()
        };
        assert_eq!(e.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), [SDDM_REXON_THEME_CONF]);
    }

    #[test]
    fn тема_sddm_уже_выбрана_или_свой_файл_правится_на_месте() {
        assert_eq!(theme_on("t", &fs(&[("/etc/sddm.conf", "[Theme]\nCurrent=t\n")])), SessionSwitch::Nothing);
        // The previous own file is edited, no second one appears; an empty
        // `Current=` in `/etc/sddm.conf` (the built-in theme) is a value too.
        let SessionSwitch::Edits(e) = theme_on(
            "new",
            &fs(&[(SDDM_REXON_THEME_CONF, "[Theme]\nCurrent=old\n"), ("/etc/sddm.conf", "[Theme]\nCurrent=\n")]),
        ) else {
            panic!()
        };
        assert_eq!(
            e,
            vec![
                SessionEdit { path: SDDM_REXON_THEME_CONF.into(), text: "[Theme]\nCurrent=new\n".into() },
                SessionEdit { path: "/etc/sddm.conf".into(), text: "[Theme]\nCurrent=new\n".into() },
            ]
        );
    }

    #[test]
    fn правит_только_etc_и_состояние_sddm_и_сверяет_итог() {
        // Empty: nothing to change.
        assert_eq!(plan_on(&fs(&[])), SessionSwitch::Nothing);
        // `/etc` and SDDM's last session are edited; the package file without X11
        // is not touched.
        let SessionSwitch::Edits(e) = plan_on(&fs(&[
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasmax11.desktop\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            (SDDM_STATE_FILE, "[Last]\nSession=/usr/share/xsessions/plasmax11.desktop\n"),
            ("/etc/plasmalogin.conf.d/10.conf", "[Greeter]\nPreselectedSession=plasmax11.desktop\n"),
        ])) else {
            panic!()
        };
        let paths: Vec<&str> = e.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/etc/sddm.conf.d/kde_settings.conf", "/etc/sddm.conf", SDDM_STATE_FILE, "/etc/plasmalogin.conf.d/10.conf"]
        );
        // X11 is in the package file, and `/etc` does not override it: change nothing.
        assert_eq!(
            plan_on(&fs(&[
                ("/usr/lib/plasmalogin/plasmalogin.conf.d/x.conf", "[Autologin]\nSession=plasmax11\n"),
                ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ])),
            SessionSwitch::Elsewhere
        );
        // Overridden by a later `/etc` file: then that one is edited, and the result is clean.
        assert!(matches!(
            plan_on(&fs(&[
                ("/usr/lib/sddm/sddm.conf.d/x.conf", "[Autologin]\nSession=plasmax11\n"),
                ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ])),
            SessionSwitch::Edits(e) if e.len() == 1 && e[0].path == "/etc/sddm.conf"
        ));
    }

    #[test]
    fn операция_уходит_помощнику_с_именем_из_таблицы() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let s = &X11_SESSION_SUCCESSORS[0];
        let out = |rc: i32, stderr: String| crate::system_exec::RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr,
        };
        let mut seen = Vec::new();
        let ok = make_wayland_default_with(s, &mut |op| {
            seen.push(op.clone());
            Some(out(0, String::new()))
        });
        assert_eq!(ok.as_deref(), Ok("Plasma (Wayland) is now the default session on the login screen."));
        assert_eq!(seen, [crate::privilege::op::PrivOp::LoginSessionWayland { x11: "plasmax11".into() }]);
        let refused = make_wayland_default_with(s, &mut |_| {
            Some(out(1, crate::privilege::client::key_text("refused.wayland-session-missing.plasmax11")))
        });
        assert_eq!(
            refused,
            Err("Rexon didn't change the login screen: Plasma (Wayland) isn't installed. Nothing was changed.".into())
        );
        assert!(make_wayland_default_with(s, &mut |_| None).is_err());
    }

    #[test]
    fn сеанс_экрана_входа_по_умолчанию_опознаётся() {
        let found = |v: &[&str]| x11_login_default(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).map(|s| s.x11);
        assert_eq!(found(&["plasma", "/usr/share/xsessions/plasmax11.desktop"]), Some("plasmax11"));
        assert_eq!(found(&["plasma.desktop", "xfce"]), None);
        // Each pair is different, and the X11 session does not match its own pair.
        for s in X11_SESSION_SUCCESSORS {
            assert_ne!(s.x11, s.wayland);
        }
    }
}

#[cfg(test)]
mod pacman_self_answered_tests {
    use super::*;

    fn hidden(line: &str) -> bool {
        parse_pacman_line(line).is_some_and(|p| p.stage == "prompt" && p.message.is_empty())
    }

    /// Everything pacman asks and answers itself under `--noconfirm` is an empty
    /// message of the `prompt` stage (it does not go to the person's log). The
    /// lines were taken from a live Deepin install (the build-deps log, 02.11.2026).
    #[test]
    fn self_answered_menu_and_consent_are_hidden() {
        for line in [
            ":: Proceed with installation? [Y/n] ",
            ":: Proceed with download? [Y/n]",
            ":: Repository cachyos-extra-znver4",
            ":: Repository extra",
            "   1) gsettings-qt5",
            "   2) gsettings-qt5",
            "   1) lxqt-about  2) lxqt-admin  3) lxqt-config",
            "Enter a number (default=1): ",
        ] {
            assert!(hidden(line), "{line}");
        }
        // The question header stays, with the answer that was chosen.
        let head = parse_pacman_line(":: There are 2 providers available for gsettings-qt:").unwrap();
        assert_eq!(head.stage, "prompt");
        assert!(!head.message.is_empty());
    }

    /// Questions whose answer decides the outcome, and ordinary text, are not hidden.
    #[test]
    fn meaningful_lines_stay_visible() {
        for line in [
            ":: deepin-kwin-6.0.8-12.1 and kwin-6.8.0-1.1 are in conflict. Remove kwin? [y/N]",
            ":: Import PGP key ABCD? [Y/n]",
            ":: Retrieving packages...",
            ":: Repository extra is broken",
            "1) Do this first",
            "1) Do this now",
            "(1/3) installing kwin...",
        ] {
            assert!(!hidden(line), "{line}");
        }
    }
}
