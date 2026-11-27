//! The shared plan for installing, applying and removing — and its single
//! executor.
//!
//! WHY. Rexon had three kinds of steps and three executors:
//!   * [`ExtStep`] (`ext_manager`) — installing and removing `.rxn`: files,
//!     staging under `/tmp`, the system part via pkexec, notes for the person;
//!     executor — `ext_exec::exec_plan_noted`;
//!   * `appearance_installer::InstallStep` — installing a product from the
//!     "Appearance" store: the same file steps, only poorer (a subset of
//!     [`ExtStep`]); executor — `exec_install_steps`. Since step 2 the store has
//!     no type of its own: its planners build [`ExtStep`];
//!   * [`ApplyStep`] (`themes`) — "Apply" of a section's theme: commands with an
//!     `ok` flag, ini writes, the compositor config, the Deepin D-Bus check;
//!     executor — `exec_apply_plan_report`.
//!
//! The `.rxn` v5 format installs a package as ONE plan: "put the files" and
//! "enable". Here is the first step of the merge: one step type [`Step`] (the
//! file layer — [`ExtStep`], which fully covers the store's steps; the enabling
//! layer — [`ApplyStep`]) and one executor [`exec`]. The old executors became
//! thin wrappers over it; their plans did not change.
//!
//! The difference between the paths that is kept for now is whether the plan
//! has a system part ([`Ctx::privileged`]). Copying is the same for all paths
//! (step 2): see [`copy_tree`] and [`copy_file`]; running commands is also one —
//! [`Ctx::run`], [`Ctx::have`], [`Ctx::spawn`], the live machine — [`Ctx::live`].
//! Each path assembles the error text itself from [`Failure`] — the way it did
//! before.
//!
//! Rollback on a failure in the middle of an installation (step 4) is also one:
//! [`Undo`] — it is written by copying and the other file steps, and executed by
//! [`exec`] (or by an installation that has its own steps after the plan —
//! [`exec_undoable`]).

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::ext_manager::ExtStep;
use crate::ext_exec::PrivOutcome;
use crate::themes::{ApplyStep, CursorReport};

/// A step of the shared plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Put, remove, run: the file and system layer.
    File(ExtStep),
    /// Enable: an apply step with an `ok` flag (see [`ApplyStep`]).
    Apply(ApplyStep),
}

impl From<ExtStep> for Step {
    fn from(step: ExtStep) -> Self {
        Step::File(step)
    }
}

impl From<ApplyStep> for Step {
    fn from(step: ApplyStep) -> Self {
        Step::Apply(step)
    }
}

/// How a failed step ended. The caller assembles the text for the person from
/// it — each path has its own (see the wrappers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The text is already ready (the system part, SteamOS, the theme check before
    /// the password).
    Text(String),
    MakeDirs { path: String, err: String },
    Copy { src: String, dest: String, err: String },
    Remove { path: String, err: String },
    Symlink { link: String, err: String },
    Write { path: String, err: String },
    Unpack { archive: String, err: String },
    Prepare { source: String, err: String },
}

/// What is external to the plan run.
pub struct Ctx<'a> {
    /// An installation plan (`true`) or a removal — the system part on SteamOS
    /// depends on this.
    pub install: bool,
    /// We are on SteamOS: the system partition is closed.
    pub steamos: bool,
    /// Executor of system steps (pkexec). `None` — the plan never has a system part
    /// (the store): a `Run` with `pkexec` goes as an ordinary command.
    pub privileged: Option<&'a dyn Fn(&ExtStep) -> PrivOutcome>,
    /// The ONLY way the plan's commands are run with waiting: both the file steps
    /// (`Run`, `RunNoted`) and the enabling steps ([`ApplyStep`] — commands,
    /// probes, the Deepin check, the compositor config check). argv, the step's
    /// timeout (`None` — by command class, [`crate::utils::run_duration`]) → a
    /// result with output. Live — [`run_live`].
    pub run: &'a dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
    /// Whether a program exists — by the same lookup rule as [`Self::run`]
    /// ([`have_live`]).
    pub have: &'a dyn Fn(&str) -> bool,
    /// Launch without waiting ([`ExtStep::Spawn`]: `conky`, restarting the shell).
    /// Live — [`spawn_live`]; in checks — launch nothing.
    pub spawn: &'a dyn Fn(&[String]),
    /// Whether a Treeland session is running — the reason for stopping at
    /// [`ApplyStep::DdeTreelandGate`] ([`crate::dde::refusal`]). The field name is
    /// the former one: live calls pass [`crate::dde::treeland_session_live`] here.
    pub wayland: bool,
    /// The plan wrote variables to `environment.d` — mirror them into sessions that
    /// do not read this directory; the writes are under the run's rollback. Live —
    /// [`sync_env_live`]; in checks — nothing: the live sync writes into the
    /// compositor configs of the real profile.
    pub sync_env: &'a dyn Fn(&mut Undo),
}

impl<'a> Ctx<'a> {
    /// The live machine: commands — [`run_live`], program lookup — [`have_live`],
    /// launch without waiting — [`spawn_live`].
    pub fn live(
        install: bool,
        steamos: bool,
        privileged: Option<&'a dyn Fn(&ExtStep) -> PrivOutcome>,
        treeland: bool,
    ) -> Self {
        Ctx { install, steamos, privileged, run: &run_live, have: &have_live, spawn: &spawn_live, wayland: treeland, sync_env: &sync_env_live }
    }
}

/// Mirror the `environment.d` variables into the sessions of the live machine
/// ([`crate::compositor_autostart::sync_session_env_live`]) under the `undo`
/// rollback: session files (`labwc/environment`, `.xprofile`, the Hyprland
/// config) — as a copy before writing, created ones — as a mark. Previously this
/// write bypassed the rollback, and "revert to how it was" after a cursor change
/// left a block of variables in `~/.config/xfce4/labwc/environment` (found on
/// 25.10.2026).
pub fn sync_env_live(undo: &mut Undo) {
    let paths: Vec<std::path::PathBuf> =
        crate::compositor_autostart::session_env_paths_live().into_iter().map(Into::into).collect();
    let missing = undo.guard_external(&paths);
    let _ = crate::compositor_autostart::sync_session_env_live();
    undo.after_external(missing);
}

/// Synchronize nothing (checks, substitute contexts).
pub fn no_sync_env(_: &mut Undo) {}

/// The process's `$PATH` — one program lookup rule for all plan steps.
fn path_env() -> Option<String> {
    std::env::var("PATH").ok()
}

/// Run a plan command and wait — through the product's single launch point
/// ([`crate::utils::run`]): binary synonyms (`kwriteconfig6` → `kwriteconfig5`,
/// `qdbus` → `qdbus6`, directories outside PATH), a timeout by command class if
/// the step did not set its own, the launch log; the output is collected.
///
/// The appearance store has no system executor ([`Ctx::privileged`] = `None`):
/// its step of the themes and layouts helper (`pkexec <helper> …`) first goes to
/// the window's privilege session
/// ([`crate::ext_exec::run_system_argv_session`]); if there is no session — the
/// former command. A helper's refusal is not repeated by it (Z5).
pub fn run_live(argv: &[String], timeout_secs: Option<u64>) -> crate::utils::RunReport {
    if let Some(report) = crate::ext_exec::run_system_argv_session(argv, timeout_secs) {
        return report;
    }
    crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs,
            ..Default::default()
        },
        path_env().as_deref(),
    )
}

/// Whether program `binary` exists — with the same synonym substitution as in
/// [`run_live`]: the probe and the launch do not disagree.
pub fn have_live(binary: &str) -> bool {
    let path = path_env();
    let argv = crate::utils::resolved_argv(&[binary.to_string()], path.as_deref());
    argv.first().is_some_and(|b| crate::utils::which(b, path.as_deref()).is_some())
}

/// Launch a program and do not wait (stdin/stdout/stderr — to nowhere).
pub fn spawn_live(argv: &[String]) {
    if let Some(program) = argv.first() {
        let _ = Command::new(program)
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// Result of the run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// What in the plan was not done and why ([`ExtStep::Notice`], a failed
    /// [`ExtStep::RunNoted`]).
    pub notes: Vec<String>,
    /// Result of the enabling layer (the `ok` flag and the cursor details).
    pub apply: CursorReport,
}

/// Execute the shared plan.
///
/// A file step with an error stops the plan (`Err`). Enabling steps do not fail:
/// they keep the `ok` flag ([`Report::apply`]); the Treeland stop ends the plan
/// entirely. `pre_ran` — probes executed while the plan was being built: their
/// `Run` steps of the same name are skipped in turn.
///
/// ROLLBACK (step 4 of merging the installers). An installation plan
/// (`ctx.install`) on an error removes everything THIS run put and restores what
/// was set aside ([`Undo`]); on success the set-aside copies are erased. One
/// rule for `.rxn` and the store. Removal plans (`ctx.install == false`) are not
/// rolled back: they have nothing to restore except what the person asked to
/// remove.
pub fn exec(steps: &[Step], pre_ran: &mut Vec<Vec<String>>, ctx: &Ctx) -> Result<Report, Failure> {
    let mut undo = if ctx.install { Undo::new() } else { Undo::off() };
    let outcome = exec_undoable(steps, pre_ran, ctx, &mut undo);
    undo.finish(outcome.is_ok());
    outcome
}

/// The same as [`exec`], but the rollback accumulates in the caller's `undo` and
/// is NOT executed: an installation that has its own steps after the plan (a
/// copy of the `.rxn` package, writing state) decides itself —
/// [`Undo::finish`]. The exception is the desktop's previous values
/// ([`Undo::keeping_values`]): on failure they are restored right here.
pub fn exec_undoable(
    steps: &[Step],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    undo: &mut Undo,
) -> Result<Report, Failure> {
    let mut notes = Vec::new();
    let mut apply = crate::appearance_installer::ApplyRun::new(ctx.wayland, ctx.run, ctx.have, ctx.sync_env);

    // Refusal of the whole installation (all the main parts are skipped — something
    // foreign is in place): the answer is the reason before the first step, nothing
    // is touched.
    if let Some(text) = steps.iter().find_map(|s| match s {
        Step::File(ExtStep::Refuse { text }) => Some(text),
        _ => None,
    }) {
        return Err(Failure::Text(text.clone()));
    }

    if ctx.privileged.is_some() {
        if ctx.steamos && ctx.install {
            let first = steps.iter().find_map(|s| match s {
                Step::File(f) if is_privileged(f) => Some(f),
                _ => None,
            });
            if let Some(step) = first {
                return Err(Failure::Text(steamos_block_of(step).reason()));
            }
        }
    }

    // The last stage (`Stage`) before a system step — the system step will carry
    // its contents into the system.
    let mut last_stage: Option<&str> = None;
    let run = run_steps(steps, pre_ran, ctx, &mut notes, &mut apply, undo, &mut last_stage);
    if let Err(f) = run {
        // Files created by commands after `Guard` are also under the rollback.
        apply.settle(undo);
        // Desktop values the run managed to change go back right away: the
        // command needs a launch, and the file rollback does not have one.
        let _ = restore_values(&undo.take_values(), ctx.run);
        return Err(f);
    }
    Ok(Report { notes, apply: apply.finish(undo) })
}

/// The plan's steps in order — the body of [`exec_undoable`].
fn run_steps<'s>(
    steps: &'s [Step],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    notes: &mut Vec<String>,
    apply: &mut crate::appearance_installer::ApplyRun,
    undo: &mut Undo,
    last_stage: &mut Option<&'s str>,
) -> Result<(), Failure> {
    for step in steps {
        let file = match step {
            Step::Apply(a) => {
                if !apply.step(a, undo)? {
                    break;
                }
                continue;
            }
            Step::File(f) => f,
        };
        if let Some(privileged) = ctx.privileged {
            if is_privileged(file) {
                if ctx.steamos {
                    continue; // removal on SteamOS: we do not touch the system part
                }
                if ctx.install {
                    if let Some(problem) = system_theme_problem(file, *last_stage) {
                        return Err(Failure::Text(problem.message()));
                    }
                }
                if let Some(text) = crate::ext_exec::privileged_failure_text(&privileged(file)) {
                    return Err(Failure::Text(text));
                }
                continue;
            }
        }
        exec_file_step(file, pre_ran, ctx, notes, undo)?;
        if let ExtStep::Stage { staged, .. } = file {
            *last_stage = Some(staged.as_str());
        }
    }
    Ok(())
}

fn io(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Text of a failed step for the person (lower-case first letter: it comes after
/// the caller's introduction).
pub fn failure_text(f: &Failure) -> String {
    use crate::i18n::trf;
    match f {
        Failure::Text(text) => text.clone(),
        Failure::MakeDirs { path, err } => trf("couldn't create the folder %1: %2", &[path, err]),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        Failure::Copy { src, dest, err } => trf("couldn't copy %1 → %2: %3", &[src, dest, err]),
        Failure::Remove { path, err } => trf("couldn't remove %1: %2", &[path, err]),
        Failure::Symlink { link, err } => trf("couldn't create the link %1: %2", &[link, err]),
        Failure::Write { path, err } => trf("couldn't write the file %1: %2", &[path, err]),
        Failure::Unpack { archive, err } => trf("couldn't unpack %1: %2", &[archive, err]),
        Failure::Prepare { source, err } => trf("couldn't prepare %1: %2", &[source, err]),
    }
}

fn exec_file_step(
    step: &ExtStep,
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &Ctx,
    notes: &mut Vec<String>,
    undo: &mut Undo,
) -> Result<(), Failure> {
    match step {
        ExtStep::MakeDirs { path } => make_dirs(Path::new(path), undo, false)
            .map_err(|e| Failure::MakeDirs { path: path.clone(), err: io(e) })?,
        ExtStep::CopyTree { src, dest } => copy_tree(Path::new(src), Path::new(dest), undo)
            .map_err(|e| Failure::Copy { src: src.clone(), dest: dest.clone(), err: io(e) })?,
        ExtStep::CopyFile { src, dest } => copy_file_undo(Path::new(src), Path::new(dest), undo)
            .map_err(|e| Failure::Copy { src: src.clone(), dest: dest.clone(), err: io(e) })?,
        ExtStep::RemoveTree { path, safe, ignore_errors } => {
            if *safe {
                // The same bounds as `safe_rmtree`: a symlink — itself, a directory —
                // only under the home or the temporary directory; otherwise — nothing.
                let p = Path::new(path);
                if !(crate::ext_utils::safe_rmtree_removes(p) && undo.set_aside(p)) {
                    crate::ext_utils::safe_rmtree(p);
                }
            } else {
                // `remove_dir_all` removes a directory and a symlink; a file is an error.
                let p = Path::new(path);
                let tree_or_link = std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() || m.file_type().is_symlink());
                if !(tree_or_link && undo.set_aside(p)) {
                    let r = std::fs::remove_dir_all(path);
                    if !*ignore_errors {
                        r.map_err(|e| Failure::Remove { path: path.clone(), err: io(e) })?;
                    }
                }
            }
        }
        ExtStep::RemoveFile { path } => {
            let p = Path::new(path);
            let non_dir = std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir());
            if !(non_dir && undo.set_aside(p)) {
                std::fs::remove_file(path).map_err(|e| Failure::Remove { path: path.clone(), err: io(e) })?
            }
        }
        ExtStep::Symlink { target, link } => {
            std::os::unix::fs::symlink(target, link).map_err(|e| Failure::Symlink { link: link.clone(), err: io(e) })?;
            undo.created(Path::new(link));
        }
        ExtStep::WriteFile { path, content } => {
            let p = Path::new(path);
            if let Some(parent) = p.parent() {
                let _ = make_dirs(parent, undo, false);
            }
            let fresh = std::fs::symlink_metadata(p).is_err();
            if !fresh {
                undo.keep_copy(p);
            }
            std::fs::write(path, content).map_err(|e| Failure::Write { path: path.clone(), err: io(e) })?;
            if fresh {
                undo.created(p);
            }
        }
        ExtStep::ExtractZip { archive, dest } => {
            let d = Path::new(dest);
            let unpack = |into: &Path| {
                crate::ext_utils::safe_zip_extract(Path::new(archive), into)
                    .map_err(|e| Failure::Unpack { archive: archive.clone(), err: io(e) })
            };
            if undo.enabled && d.is_dir() {
                // On top of an existing directory: unpack next to it and move with
                // the shared copying — that way the rollback knows every file put and
                // replaced.
                let side = undo.side_path(d, "unpack");
                let r = unpack(&side).and_then(|()| {
                    copy_tree(&side, d, undo)
                        .map_err(|e| Failure::Unpack { archive: archive.clone(), err: io(e) })
                });
                let _ = std::fs::remove_dir_all(&side);
                r?
            } else {
                make_dirs(d, undo, true).map_err(|e| Failure::MakeDirs { path: dest.clone(), err: io(e) })?;
                unpack(d)?
            }
        }
        ExtStep::Stage { staged, sources } => {
            // `_staged_under_tmp`: directory contents go to the staged root,
            // files under their own name.
            make_dirs(Path::new(staged), undo, false)
                .map_err(|e| Failure::MakeDirs { path: staged.clone(), err: io(e) })?;
            for p in sources {
                let prepare = |e: std::io::Error| Failure::Prepare { source: p.clone(), err: io(e) };
                let src = Path::new(p);
                if src.is_dir() {
                    for name in sorted_names(src) {
                        let from = src.join(&name);
                        let to = Path::new(staged).join(&name);
                        let r = if from.is_dir() { copy_tree(&from, &to, undo) } else { copy_file_undo(&from, &to, undo) };
                        r.map_err(prepare)?;
                    }
                } else {
                    let to = match src.file_name() {
                        Some(name) => Path::new(staged).join(name),
                        None => Path::new(staged).to_path_buf(),
                    };
                    copy_file_undo(src, &to, undo).map_err(prepare)?;
                }
            }
        }
        // Without a system executor the wrapper script has nowhere to come
        // from; with one it was executed above together with the other
        // system steps.
        ExtStep::PkexecScript { .. } => {}
        ExtStep::Run { argv, timeout_secs } => {
            if pre_ran.first().map(|a| a == argv).unwrap_or(false) {
                pre_ran.remove(0); // already executed as a probe while building the plan
                return Ok(());
            }
            let _ = undo.run_kept(ctx.run, argv, *timeout_secs);
        }
        ExtStep::RunNoted { argv, timeout_secs, failed } => {
            if undo.run_kept(ctx.run, argv, *timeout_secs).returncode != Some(0) {
                notes.push(failed.clone());
            }
        }
        ExtStep::Notice { text } => notes.push(text.clone()),
        ExtStep::Refuse { text } => return Err(Failure::Text(text.clone())),
        ExtStep::Spawn { argv } => {
            if argv.first().map(|a| a == "conky").unwrap_or(false) {
                // the original: pkill → time.sleep(0.5) → Popen
                std::thread::sleep(Duration::from_millis(500));
            }
            (ctx.spawn)(argv);
        }
    }
    Ok(())
}

/// Names in a directory alphabetically (the order does not depend on the file
/// system); names that are not UTF-8 are skipped — as in the `ext_exec` snapshot.
fn sorted_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// ============================================================================
// The plan's system part
// ============================================================================

/// Whether a step is privileged: it writes into the system via pkexec.
pub(crate) fn is_privileged(step: &ExtStep) -> bool {
    match step {
        ExtStep::PkexecScript { .. } => true,
        ExtStep::Run { argv, .. } => argv.first().map(String::as_str) == Some("pkexec"),
        _ => false,
    }
}

/// The helper's arguments after `pkexec <helper>` without the optional
/// `--lang=…`: the category is the first word.
///
/// ⭐ Fix of 09.10.2026: previously the category was taken as the third word of
/// argv, and with a Russian interface `--lang=ru` stands there — the theme check
/// before the password dialog silently did not trigger, and on SteamOS the
/// refusal reason came out generic.
pub(crate) fn helper_args(argv: &[String]) -> &[String] {
    let rest = argv.get(2..).unwrap_or(&[]);
    match rest.first() {
        Some(a) if a.starts_with("--lang=") => &rest[1..],
        _ => rest,
    }
}

/// What exactly a privileged step writes — for the reason on SteamOS. The label
/// is taken from the step itself: for helpers it is the category as the third
/// word of argv (`pkexec <helper> sddm …`), for wrapper scripts — their prefix
/// (`rexon_sddm`, `rexon_plymouth`, `rexon_gdm-bg`…).
pub(crate) fn steamos_block_of(step: &ExtStep) -> crate::distro_compat::SteamOsBlock {
    use crate::distro_compat::SteamOsBlock;
    let tag = match step {
        ExtStep::PkexecScript { prefix, .. } => prefix.as_str(),
        ExtStep::Run { argv, .. } => helper_args(argv).first().map(String::as_str).unwrap_or(""),
        _ => "",
    };
    if tag.contains("plymouth") {
        SteamOsBlock::BootSplash
    } else if ["sddm", "gdm", "lightdm", "greeter", "login"].iter().any(|k| tag.contains(k)) {
        SteamOsBlock::LoginScreen
    } else {
        SteamOsBlock::SystemPart
    }
}

/// The login or boot screen theme that system step `step` is about to carry into
/// `/usr/share`: a check BEFORE the password dialog
/// ([`crate::installer::system_theme_problem`]).
///
/// The contents are the last stage step (`Stage`) before the system one
/// (`last_stage`): both the helper and the fallback script copy it (the latter
/// takes the source directory, and the stage is its copy, made just now). On the
/// fallback path (a raw `pkexec bash`, no helper) this check is the only one:
/// code running as root via `ModuleName=../…` and a `.so` in a Plymouth theme do
/// not get through there either.
fn system_theme_problem(
    step: &ExtStep,
    last_stage: Option<&str>,
) -> Option<crate::installer::SystemThemeProblem> {
    let (kind, theme_id) = match step {
        ExtStep::Run { argv, .. } => {
            let args = helper_args(argv);
            (args.first()?.as_str(), args.get(1).map(String::as_str))
        }
        ExtStep::PkexecScript { prefix, .. } => (prefix.strip_prefix("rexon_")?, None),
        _ => return None,
    };
    if kind != "sddm" && kind != "plymouth" {
        return None;
    }
    crate::installer::system_theme_problem(kind, Path::new(last_stage?), theme_id)
}

// ============================================================================
// Copying
// ============================================================================

/// Move tree `src` into `dest` — one behavior for all installation paths (step 2
/// of the merge; previously `.rxn` and the store behaved differently).
///
/// * A symbolic link is RECREATED as a link with the same target (icon sets like
///   Buuf consist almost entirely of them, and following them would bloat the
///   theme many times over; besides, following a link leading outside would copy
///   a foreign file). A dangling link is moved too: a relative target
///   (`../hicolor/…`) dangles under the temporary unpacking directory but may be
///   alive at the install location. If a dangling link could not be recreated,
///   it is skipped — not a reason to fail the whole installation; a live link
///   that could not be recreated is an error.
/// * A file or link already lying in place of a link is replaced (a repeated
///   installation on top); a directory is not.
/// * A regular file is not written THROUGH a link left in its place: the link is
///   removed, and the file is put in its place (otherwise the copy would go
///   where the link points — outside the theme).
///
/// With writing to the `undo` rollback ([`Undo::off`] — without it): a directory
/// that did not exist is created entirely (nothing inside it is recorded one by
/// one); into an existing directory — one by one, and what is replaced is set
/// aside.
///
/// Names are walked in order (by comparing the bytes of the name): the file
/// system order differs from machine to machine, and with it — what managed to
/// land before a failure.
pub(crate) fn copy_tree(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    if undo.enabled && std::fs::symlink_metadata(dest).is_err() {
        make_dirs(dest, undo, true)?;
        return copy_tree(src, dest, &mut Undo::off());
    }
    std::fs::create_dir_all(dest)?;
    let mut entries: Vec<std::fs::DirEntry> = std::fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let md = std::fs::symlink_metadata(&from)?;
        if md.file_type().is_symlink() {
            let target = std::fs::read_link(&from)?;
            undo.clear_non_dir(&to);
            match std::os::unix::fs::symlink(&target, &to) {
                Ok(()) => undo.created(&to),
                Err(e) => {
                    if from.exists() {
                        return Err(e);
                    }
                }
            }
        } else if md.is_dir() {
            copy_tree(&from, &to, undo)?;
        } else {
            copy_regular(&from, &to, undo)?;
        }
    }
    Ok(())
}

/// A copy of file `src` strictly into file `dest` — one behavior for all paths:
/// the destination directory is created, a link in place of `dest` is removed
/// (the copy is not written through it). `dest` being an existing directory is
/// an error: the plan must name the file explicitly (previously for `.rxn` a
/// directory meant "put inside", while for the store it meant an error, and the
/// same step meant different things).
pub(crate) fn copy_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    copy_file_undo(src, dest, &mut Undo::off())
}

/// [`copy_file`] with writing to the `undo` rollback.
pub(crate) fn copy_file_undo(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    if let Some(parent) = dest.parent() {
        make_dirs(parent, undo, false)?;
    }
    copy_regular(src, dest, undo)
}

fn copy_regular(src: &Path, dest: &Path, undo: &mut Undo) -> std::io::Result<()> {
    let before = std::fs::symlink_metadata(dest).ok();
    match &before {
        // A file or link in place: with a rollback — set aside (it will come
        // back on failure); without one the link is removed and the file is
        // overwritten.
        Some(m) if !m.is_dir() => {
            if !undo.set_aside(dest) && m.file_type().is_symlink() {
                std::fs::remove_file(dest)?;
            }
        }
        _ => {}
    }
    std::fs::copy(src, dest)?;
    if before.is_none() {
        undo.created(dest);
    }
    Ok(())
}

/// Create directory `path` with missing parents. With a rollback each one
/// created is recorded: parents are removed only if empty; `path` itself with
/// `tree` — entirely (everything inside was put by this run).
pub(crate) fn make_dirs(path: &Path, undo: &mut Undo, tree: bool) -> std::io::Result<()> {
    if !undo.enabled {
        return std::fs::create_dir_all(path);
    }
    let mut missing = Vec::new();
    let mut cur = Some(path);
    while let Some(p) = cur {
        if p.as_os_str().is_empty() || std::fs::symlink_metadata(p).is_ok() {
            break;
        }
        missing.push(p.to_path_buf());
        cur = p.parent();
    }
    for dir in missing.iter().rev() {
        match std::fs::create_dir(dir) {
            Ok(()) => {
                let whole = tree && dir.as_path() == path;
                undo.done.push(if whole { Done::CreatedTree(dir.clone()) } else { Done::Created(dir.clone()) });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && dir.is_dir() => {}
            Err(e) => return Err(e),
        }
    }
    if path.is_dir() {
        Ok(())
    } else {
        // The same answer `create_dir_all` would give (a file in place of a directory).
        std::fs::create_dir_all(path)
    }
}

// ============================================================================
// Installation rollback
// ============================================================================

/// Rollback of one run of an installation plan: what the run put and what it
/// set aside.
///
/// It is recorded during copying rather than derived from the plan in advance
/// ([`crate::install_journal::placed_from_steps`] serves the journal): into an
/// existing directory (a foreign one or a base one like `~/.local/share/fonts`)
/// the plan puts files one by one, and on failure exactly those must be removed
/// — and only those, without touching what lay there before the installation.
///
/// * What the run created (a file, a link, a directory) is removed; a directory
///   created for a copy as a whole is removed as a whole; a parent one only if
///   empty.
/// * What is being replaced (a file, a link, the previous copy of a theme under
///   `RemoveTree`) is set aside by renaming next to it
///   (`.<name>.rexon-undo-<pid>-<n>`, the same file system) and put back on
///   failure. A file that `WriteFile` writes in place — as a copy. The snapshot
///   is needed: a repeated installation of one's own theme first removes the
///   previous copy, and without the snapshot a failure would leave the person
///   with no theme at all; the same for the configs the plan writes.
/// * Success — what was set aside is erased ([`Undo::finish`]).
///
/// File writes of the enabling layer ([`ApplyStep`]: ini, our files, the
/// compositor config) go under the same rollback since step 5a
/// ([`Undo::write_text`], [`Undo::remove_file`]): a write failure in the middle
/// of an "Apply" click restores everything the click managed to write.
///
/// Not rolled back: the system part (pkexec — it can be removed only by the root
/// helper) and the plan's other commands (icon caches, `fc-cache`).
///
/// Desktop values stored not in a file but by a service (`gsettings`/`dconf`,
/// `xfconf-query`) are captured by a rollback with [`Undo::keeping_values`]
/// BEFORE the command that changes them ([`Undo::run_kept`]): the previous value
/// or "the key was not set" goes into a [`ValueBefore`] — as a restoring
/// command. A KDE file written by `kwriteconfig6` is captured there too, as an
/// ordinary copy. A failure in the middle of the plan restores them right away
/// ([`exec_undoable`]); success hands them to the "how it was" snapshot
/// ([`Undo::take_values`]).
#[derive(Debug, Default)]
pub struct Undo {
    enabled: bool,
    done: Vec<Done>,
    seq: u32,
    /// Previous desktop values in write order; `None` — they are not captured.
    values: Option<Vec<ValueBefore>>,
    /// Paths that did not exist before the run and are waiting for a "created" mark
    /// after a foreign write ([`Self::guard_external`] → [`Self::after_external`]).
    /// Their "how it was" is "it did not exist": no intermediate copy is taken from
    /// them.
    pending: Vec<std::path::PathBuf>,
}

/// A path touched by the run ([`Undo::keep_into`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touched {
    pub path: std::path::PathBuf,
    /// A copy of the previous contents; `None` — the path was created by the run.
    pub before: Option<std::path::PathBuf>,
    /// A directory created by the run.
    pub dir: bool,
}

#[derive(Debug)]
enum Done {
    /// Created by the run: a file, a link or a directory (removed only if empty).
    Created(std::path::PathBuf),
    /// A directory created by the run for a copy as a whole.
    CreatedTree(std::path::PathBuf),
    /// The previous contents of `path` are set aside in `backup`.
    Saved { path: std::path::PathBuf, backup: std::path::PathBuf },
}

impl Undo {
    /// A rollback that records.
    pub fn new() -> Self {
        Undo { enabled: true, ..Default::default() }
    }

    /// Without a rollback: nothing is recorded or set aside (removal, copying
    /// outside a plan).
    pub fn off() -> Self {
        Undo::default()
    }

    /// Also capture the previous desktop values (`gsettings`/`dconf`, `xfconf`)
    /// before the commands that change them — for "Apply" clicks that have a "Revert
    /// to how it was". Without a rollback nothing is captured.
    pub fn keeping_values(mut self) -> Self {
        if self.enabled {
            self.values = Some(Vec::new());
        }
        self
    }

    /// Run a plan command via `run`; if it changes a desktop value, capture the
    /// previous one before it (once per run for a key: the first one is the "how it
    /// was"). If the command failed, the value did not change, and what was
    /// captured is forgotten.
    pub(crate) fn run_kept(
        &mut self,
        run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport,
        argv: &[String],
        timeout_secs: Option<u64>,
    ) -> crate::utils::RunReport {
        let mut fresh = false;
        if let Some(values) = self.values.as_mut() {
            if let Some(target) = value_target(argv) {
                if !values.iter().any(|v| v.key == target.key) {
                    match value_before(&target, run) {
                        Some(v) => {
                            values.push(v);
                            fresh = true;
                        }
                        None => crate::log_warn!("install", "{}: прежнее значение не прочитано — вернуть его будет нечем", target.key),
                    }
                }
            }
        }
        // A KDE file that `kwriteconfig6` will rewrite — as a copy before the
        // command (what it created — as a mark after): restored as a file.
        let guarded = match (&self.values, kconfig_written_file(argv, &crate::utils::config_home_env())) {
            (Some(_), Some(file)) => Some(self.guard_external(&[file])),
            _ => None,
        };
        let report = run(argv, timeout_secs);
        if let Some(missing) = guarded {
            self.after_external(missing);
        }
        if fresh && report.returncode != Some(0) {
            if let Some(values) = self.values.as_mut() {
                values.pop();
            }
        }
        report
    }

    /// The captured previous desktop values (in write order); the rollback no
    /// longer has them.
    pub fn take_values(&mut self) -> Vec<ValueBefore> {
        self.values.as_mut().map(std::mem::take).unwrap_or_default()
    }

    /// Path `path` was created by the run.
    fn created(&mut self, path: &Path) {
        if self.enabled {
            self.done.push(Done::Created(path.to_path_buf()));
        }
    }

    /// Paths `paths` are about to be rewritten by a foreign write (not through this
    /// rollback): take a copy of existing files, and for missing ones remember the
    /// first missing ancestor — after the write [`Self::after_external`] will mark
    /// what was created.
    pub(crate) fn guard_external(&mut self, paths: &[std::path::PathBuf]) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
        let mut missing = Vec::new();
        for p in paths {
            if self.pending.contains(p) {
                continue; // already waiting for a mark: "how it was" is "it did not exist"
            }
            match std::fs::symlink_metadata(p) {
                Ok(m) if m.is_dir() => {}
                Ok(_) => self.keep_copy(p),
                Err(_) => {
                    let mut top = p.clone();
                    while let Some(parent) = top.parent() {
                        if parent.as_os_str().is_empty() || std::fs::symlink_metadata(parent).is_ok() {
                            break;
                        }
                        top = parent.to_path_buf();
                    }
                    if self.enabled {
                        self.pending.push(p.clone());
                    }
                    missing.push((p.clone(), top));
                }
            }
        }
        missing
    }

    /// The foreign write went through: mark what it created (directories from the
    /// ancestor down, then the file) — the rollback and the "how it was" snapshot
    /// will remove them.
    pub(crate) fn after_external(&mut self, missing: Vec<(std::path::PathBuf, std::path::PathBuf)>) {
        let mut seen: Vec<std::path::PathBuf> = Vec::new();
        for (path, top) in missing {
            self.pending.retain(|p| *p != path);
            let mut chain: Vec<std::path::PathBuf> = Vec::new();
            for a in path.ancestors() {
                chain.push(a.to_path_buf());
                if a == top {
                    break;
                }
            }
            for p in chain.into_iter().rev() {
                if std::fs::symlink_metadata(&p).is_ok() && !seen.contains(&p) {
                    self.created(&p);
                    seen.push(p);
                }
            }
        }
    }

    /// A free name next to `path` for a service copy.
    fn side_path(&mut self, path: &Path, what: &str) -> std::path::PathBuf {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = path.parent().unwrap_or(Path::new("."));
        loop {
            self.seq += 1;
            let side = dir.join(format!(".{name}.rexon-{what}-{}-{}", std::process::id(), self.seq));
            if std::fs::symlink_metadata(&side).is_err() {
                return side;
            }
        }
    }

    /// Set aside what lies at `path` (a file, a link, a directory) instead of
    /// deleting it. `false` — the rollback is off or renaming failed: the caller
    /// acts as without a rollback.
    fn set_aside(&mut self, path: &Path) -> bool {
        if !self.enabled {
            return false;
        }
        // There is no point in setting aside what this same run created: it
        // did not exist before the run, on failure the rollback itself removes
        // it, and a set-aside copy would become a false "how it was" (see
        // [`Self::keep_copy`]).
        if self.made_by_run(path) {
            return false;
        }
        let backup = self.side_path(path, "undo");
        match std::fs::rename(path, &backup) {
            Ok(()) => {
                self.done.push(Done::Saved { path: path.to_path_buf(), backup });
                true
            }
            Err(e) => {
                crate::log_warn!("install", "{}: не отложено для отката — {e}", path.display());
                false
            }
        }
    }

    /// Remove the file or link at `path`: with a rollback — set it aside, without
    /// one — remove it; a directory is not touched.
    fn clear_non_dir(&mut self, path: &Path) {
        if let Ok(md) = std::fs::symlink_metadata(path) {
            if !md.is_dir() && !self.set_aside(path) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    /// Create directory `path` with missing parents under the rollback (parents are
    /// removed on failure only if empty).
    pub(crate) fn make_dirs(&mut self, path: &Path) -> std::io::Result<()> {
        make_dirs(path, self, false)
    }

    /// Write `content` into file `path` under the rollback — the shared path of the
    /// enabling layer's file writes ("Apply", step 5a): the directory is created,
    /// the previous contents are captured as a copy (through a link — a copy of its
    /// target, the link itself stays), a created file is removed on failure.
    /// `atomic` — writing by renaming ([`crate::utils::atomic_write`]), otherwise in
    /// place.
    pub(crate) fn write_text(&mut self, path: &Path, content: &str, atomic: bool) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                self.make_dirs(parent)?;
            }
        }
        let fresh = match std::fs::symlink_metadata(path) {
            Err(_) => true,
            // A dangling link: the write will replace it with a file — we set the
            // link itself aside.
            Ok(m) if m.file_type().is_symlink() && !path.exists() => !self.set_aside(path),
            Ok(_) => {
                self.keep_copy(path);
                false
            }
        };
        if atomic {
            crate::utils::atomic_write(&path.to_string_lossy(), content)?;
        } else {
            std::fs::write(path, content)?;
        }
        if fresh {
            self.created(path);
        }
        Ok(())
    }

    /// Remove file or link `path` under the rollback (set aside, it will come back
    /// on failure). No file is not an error.
    pub(crate) fn remove_file(&mut self, path: &Path) -> std::io::Result<()> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => Err(std::io::Error::other("на месте файла каталог")),
            Ok(_) => {
                if self.set_aside(path) {
                    Ok(())
                } else {
                    std::fs::remove_file(path)
                }
            }
        }
    }

    /// File `path` will be rewritten in place: keep a copy (on the file system where
    /// it lies; through a link — a copy of its target).
    fn keep_copy(&mut self, path: &Path) {
        if !self.enabled {
            return;
        }
        let Ok(real) = std::fs::canonicalize(path) else { return };
        if !real.is_file() {
            return;
        }
        // Created by this same run (the file itself, a copy directory above it,
        // or a path waiting for a mark after a foreign write): it did not exist
        // before the run, and an intermediate copy would become a false "how it
        // was" — "Revert to how it was" would resurrect a file the person never
        // had.
        if self.made_by_run(path) || self.made_by_run(&real) {
            return;
        }
        let backup = self.side_path(&real, "undo");
        match std::fs::copy(&real, &backup) {
            Ok(_) => self.done.push(Done::Saved { path: real, backup }),
            Err(e) => crate::log_warn!("install", "{}: копия для отката не снята — {e}", real.display()),
        }
    }

    /// `path` was created by this run: marked as created (itself or a copy directory
    /// above it) or waiting for a mark after a foreign write.
    fn made_by_run(&self, path: &Path) -> bool {
        self.pending.iter().any(|p| p == path)
            || self.done.iter().any(|d| match d {
                Done::Created(p) => p == path,
                Done::CreatedTree(p) => path.starts_with(p),
                Done::Saved { .. } => false,
            })
    }

    /// Result of the run: success — erase what was set aside; failure — remove what
    /// was put and restore what was set aside, in reverse order.
    pub fn finish(self, ok: bool) {
        if ok {
            self.commit()
        } else {
            self.rollback()
        }
    }

    /// Success while keeping "how it was" (step 5a, "Apply"): the set-aside copies
    /// are not erased but moved into the snapshot directory `dir` (names — numbers
    /// starting from `first`). Returns every touched path in run order, once each:
    /// the first appearance of a path is the truth about what was before the run
    /// (later copies of the same path are intermediate and are erased).
    pub fn keep_into(self, dir: &Path, first: usize) -> Vec<Touched> {
        let mut out: Vec<Touched> = Vec::new();
        let mut n = first;
        for d in self.done {
            let (path, backup, is_dir) = match d {
                Done::Created(p) => {
                    let is_dir = std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
                    (p, None, is_dir)
                }
                Done::CreatedTree(p) => (p, None, true),
                Done::Saved { path, backup } => (path, Some(backup), false),
            };
            if out.iter().any(|t| t.path == path) {
                if let Some(b) = backup {
                    remove_any(&b);
                }
                continue;
            }
            let before = backup.map(|b| {
                let to = dir.join(format!("{n:04}"));
                n += 1;
                let moved = std::fs::create_dir_all(dir).is_ok()
                    && (std::fs::rename(&b, &to).is_ok()
                        || (b.is_file() && std::fs::copy(&b, &to).is_ok() && std::fs::remove_file(&b).is_ok()));
                if moved {
                    to
                } else {
                    // Did not move — the copy stays next to the file.
                    crate::log_warn!("install", "{}: копия «что было» не перенесена в снимок", b.display());
                    b
                }
            });
            out.push(Touched { path, before, dir: is_dir });
        }
        out
    }

    fn commit(self) {
        for d in self.done {
            if let Done::Saved { backup, .. } = d {
                remove_any(&backup);
            }
        }
    }

    fn rollback(self) {
        for d in self.done.into_iter().rev() {
            match d {
                Done::Created(p) => match std::fs::symlink_metadata(&p) {
                    Ok(m) if m.is_dir() => {
                        let _ = std::fs::remove_dir(&p); // not empty — so it is not only ours
                    }
                    Ok(_) => {
                        let _ = std::fs::remove_file(&p);
                    }
                    Err(_) => {}
                },
                Done::CreatedTree(p) => remove_any(&p),
                Done::Saved { path, backup } => {
                    // In place is what this run put (its records have already been
                    // removed above); the directory is not empty — not ours, what was set
                    // aside stays next to it instead of overwriting someone else's.
                    let free = match std::fs::symlink_metadata(&path) {
                        Ok(m) if m.is_dir() => std::fs::remove_dir(&path).is_ok(),
                        Ok(_) => std::fs::remove_file(&path).is_ok(),
                        Err(_) => true,
                    };
                    let back = free && std::fs::rename(&backup, &path).is_ok();
                    if !back {
                        crate::log_warn!(
                            "install",
                            "{}: прежнее не вернулось на место, оно лежит в {}",
                            path.display(),
                            backup.display()
                        );
                    }
                }
            }
        }
    }
}

/// Remove a path entirely: a directory with its contents, a file and a link by
/// themselves.
fn remove_any(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

// ============================================================================
// Desktop values under the "how it was" snapshot
// ============================================================================

/// The previous value of a desktop key that a plan command overwrote: how to
/// restore it. If the key was not set, the restore resets it (`reset`, `-r`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueBefore {
    /// Which key (`gsettings org.gnome.desktop.interface gtk-theme`,
    /// `dconf /org/x/y`, `xfconf xsettings /Net/ThemeName`).
    pub key: String,
    /// The restoring command.
    pub restore: Vec<String>,
}

/// The key the command changes, and how to read its current value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueTarget {
    key: String,
    kind: TargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    /// `base` — `[env …] gsettings [--schemadir D]`; `assigns` — the `env`
    /// variables in front of it, `tool` — `gsettings [--schemadir D]` (reading the
    /// schema default is assembled from them).
    Gsettings { base: Vec<String>, assigns: Vec<String>, tool: Vec<String>, schema: String, key: String },
    /// `base` — `[env …] dconf`.
    Dconf { base: Vec<String>, key: String },
    /// `base` — `[env …] xfconf-query -c CH -p PROP`; `create` — `-n -t T` of the
    /// command (one value of one type): how to create the property anew.
    Xfconf { base: Vec<String>, create: Vec<String> },
}

/// The command's program under an `env VAR=…` prefix: `(prefix, variables,
/// program name, its arguments)`. Options of `env` itself are not parsed.
fn split_env(argv: &[String]) -> Option<(&[String], &[String], &str, &[String])> {
    let mut i = 0;
    if argv.first().map(String::as_str) == Some("env") {
        i = 1;
        while argv.get(i).is_some_and(|a| a.contains('=') && !a.starts_with('-')) {
            i += 1;
        }
        if argv.get(i).is_some_and(|a| a.starts_with('-')) {
            return None;
        }
    }
    let program = argv.get(i)?;
    let name = Path::new(program).file_name()?.to_str()?;
    let assigns = if i > 0 { &argv[1..i] } else { &argv[..0] };
    Some((&argv[..=i], assigns, name, &argv[i + 1..]))
}

/// Whether the command changes a desktop value — and which one:
/// `gsettings set|reset`, `dconf write|reset` (one key), `xfconf-query … -s|-r`
/// (one property). Anything else (`reset -f`, `-R`, `bash -c` scripts) is not
/// parsed — there is nothing to capture it with.
fn value_target(argv: &[String]) -> Option<ValueTarget> {
    let (head, assigns, name, args) = split_env(argv)?;
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match name {
        "gsettings" => {
            let mut tool = vec![head[head.len() - 1].clone()];
            let mut rest = &a[..];
            if rest.first() == Some(&"--schemadir") {
                tool.extend(rest.iter().take(2).map(|s| s.to_string()));
                rest = rest.get(2..)?;
            }
            let mut base = head[..head.len() - 1].to_vec();
            base.extend(tool.iter().cloned());
            let (schema, key) = match rest {
                ["set", schema, key, _] | ["reset", schema, key] => (*schema, *key),
                _ => return None,
            };
            Some(ValueTarget {
                key: format!("gsettings {schema} {key}"),
                kind: TargetKind::Gsettings { base, assigns: assigns.to_vec(), tool, schema: schema.into(), key: key.into() },
            })
        }
        "dconf" => {
            let key = match &a[..] {
                ["write", key, _] | ["reset", key] => *key,
                _ => return None,
            };
            if !key.starts_with('/') || key.ends_with('/') {
                return None;
            }
            Some(ValueTarget { key: format!("dconf {key}"), kind: TargetKind::Dconf { base: head.to_vec(), key: key.into() } })
        }
        "xfconf-query" => {
            let (mut channel, mut prop) = (None, None);
            let (mut sets, mut reset, mut create) = (0usize, false, false);
            let mut types: Vec<&str> = Vec::new();
            let mut i = 0;
            while i < a.len() {
                let (opt, inline) = match a[i].split_once('=') {
                    Some((o, v)) if o.starts_with("--") => (o, Some(v)),
                    _ => (a[i], None),
                };
                let takes = matches!(opt, "-c" | "--channel" | "-p" | "--property" | "-s" | "--set" | "-t" | "--type");
                let value = if takes {
                    match inline {
                        Some(v) => Some(v),
                        None => {
                            i += 1;
                            Some(*a.get(i)?)
                        }
                    }
                } else {
                    None
                };
                match opt {
                    "-c" | "--channel" => channel = value,
                    "-p" | "--property" => prop = value,
                    "-s" | "--set" => sets += 1,
                    "-t" | "--type" => types.extend(value),
                    "-n" | "--create" => create = true,
                    "-r" | "--reset" => reset = true,
                    // An array, a whole branch, a toggle — we do not capture.
                    _ => return None,
                }
                i += 1;
            }
            let (channel, prop) = (channel?, prop?);
            if sets == 0 && !reset {
                return None;
            }
            let mut base = head.to_vec();
            base.extend(["-c", channel, "-p", prop].map(String::from));
            let create = if create && sets == 1 && types.len() == 1 {
                vec!["-n".to_string(), "-t".to_string(), types[0].to_string()]
            } else {
                Vec::new()
            };
            Some(ValueTarget { key: format!("xfconf {channel} {prop}"), kind: TargetKind::Xfconf { base, create } })
        }
        _ => None,
    }
}

/// The file `kwriteconfig6`/`kwriteconfig5` will rewrite: `--file` (a name — in
/// the `config_home` config directory, a path — as is), without it —
/// `kdeglobals`. `None` — the command is not that one.
fn kconfig_written_file(argv: &[String], config_home: &str) -> Option<std::path::PathBuf> {
    let (_, _, name, args) = split_env(argv)?;
    if !matches!(name, "kwriteconfig6" | "kwriteconfig5") {
        return None;
    }
    let mut file = "kdeglobals";
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--file" => {
                file = args.get(i + 1)?.as_str();
                i += 1;
            }
            a => {
                if let Some(f) = a.strip_prefix("--file=") {
                    file = f;
                }
            }
        }
        i += 1;
    }
    if file.is_empty() {
        return None;
    }
    Some(if file.starts_with('/') { file.into() } else { Path::new(config_home).join(file) })
}

/// The desktop key the command changes (as [`ValueBefore::key`]) — so that our
/// own restoring commands do not restore a second time what the snapshot will
/// restore.
pub fn value_key_of(argv: &[String]) -> Option<String> {
    value_target(argv).map(|t| t.key)
}

/// The command only reads a desktop value (`gsettings get`, `dconf read`,
/// `xfconf-query` without writing) — that is how "how it was" is captured
/// ([`Undo::run_kept`]). A failure of such a read is not a reason for the click
/// to fail.
pub fn is_value_read(argv: &[String]) -> bool {
    let Some((_, _, name, args)) = split_env(argv) else { return false };
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match name {
        "gsettings" => {
            let rest = if a.first() == Some(&"--schemadir") { a.get(2..).unwrap_or(&[]) } else { &a[..] };
            rest.first() == Some(&"get")
        }
        "dconf" => a.first() == Some(&"read"),
        "xfconf-query" => !a.iter().any(|o| {
            matches!(*o, "-s" | "--set" | "-r" | "--reset" | "-n" | "--create" | "-T" | "--toggle" | "-l" | "--list")
                || o.starts_with("--set=")
        }),
        _ => false,
    }
}

/// The read answer without a trailing newline.
fn read_text(r: &crate::utils::RunReport) -> String {
    crate::utils::decode_text(&r.stdout).trim_end_matches(['\n', '\r']).to_string()
}

/// Read the current value of `target` and assemble a command that will restore
/// it. `None` — reading failed (there will be nothing to restore with).
fn value_before(target: &ValueTarget, run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport) -> Option<ValueBefore> {
    let with = |base: &[String], tail: &[&str]| -> Vec<String> {
        base.iter().cloned().chain(tail.iter().map(|s| s.to_string())).collect()
    };
    let restore = match &target.kind {
        TargetKind::Gsettings { base, assigns, tool, schema, key } => {
            let now = run(&with(base, &["get", schema, key]), Some(5));
            if now.returncode != Some(0) {
                return None;
            }
            let now = read_text(&now);
            // The schema default (with the distribution's overrides) — bypassing
            // written values: if it matches, we restore by resetting. gsettings has
            // no question "is the key set", and the desktop cannot tell a reset to
            // the same value apart anyway.
            let mut default = vec!["env".to_string()];
            default.extend(assigns.iter().filter(|a| !a.starts_with("GSETTINGS_BACKEND=")).cloned());
            default.push("GSETTINGS_BACKEND=memory".into());
            default.extend(tool.iter().cloned());
            default.extend(["get", schema, key].map(|s| s.to_string()));
            let d = run(&default, Some(5));
            if d.returncode == Some(0) && read_text(&d) == now {
                with(base, &["reset", schema, key])
            } else {
                with(base, &["set", schema, key, &now])
            }
        }
        TargetKind::Dconf { base, key } => {
            let now = run(&with(base, &["read", key]), Some(5));
            if now.returncode != Some(0) {
                return None;
            }
            let now = read_text(&now);
            if now.trim().is_empty() {
                with(base, &["reset", key])
            } else {
                with(base, &["write", key, &now])
            }
        }
        TargetKind::Xfconf { base, create } => {
            let now = run(base, Some(5));
            if now.returncode != Some(0) {
                // There is no property. (If there is no service either, the write
                // itself will fail too, and what was captured will be forgotten: see
                // [`Undo::run_kept`].)
                with(base, &["-r"])
            } else {
                let now = read_text(&now);
                if now.contains('\n') {
                    return None; // an array: "Value is an array with N items…"
                }
                let mut out = base.clone();
                out.extend(create.iter().cloned());
                out.extend(["-s".to_string(), now]);
                out
            }
        }
    };
    Some(ValueBefore { key: target.key.clone(), restore })
}

/// Restore the previous desktop values — in reverse write order. Returns the
/// keys that could not be restored.
pub fn restore_values(values: &[ValueBefore], run: &dyn Fn(&[String], Option<u64>) -> crate::utils::RunReport) -> Vec<String> {
    let mut failed = Vec::new();
    for v in values.iter().rev() {
        if v.restore.is_empty() || run(&v.restore, Some(10)).returncode != Some(0) {
            failed.push(v.key.clone());
        }
    }
    failed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-install-plan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    fn no_run(_: &[String], _: Option<u64>) -> crate::utils::RunReport {
        crate::utils::RunReport::default()
    }

    fn nothing(_: &str) -> bool {
        false
    }

    fn no_spawn(_: &[String]) {}

    fn ctx() -> Ctx<'static> {
        Ctx { install: true, steamos: false, privileged: None, run: &no_run, have: &nothing, spawn: &no_spawn, wayland: false, sync_env: &no_sync_env }
    }

    /// One plan — files and enabling: files are put, the enabling layer keeps `ok`,
    /// notes are collected.
    #[test]
    fn mixed_plan_puts_files_and_keeps_ok_flag() {
        let dir = scratch("mixed");
        let file = dir.join("theme/index.theme");
        let steps = vec![
            Step::from(ExtStep::WriteFile { path: s(&file), content: "[Icon Theme]\n".into() }),
            Step::from(ExtStep::Notice { text: "часть пропущена".into() }),
            Step::from(ApplyStep::ForceOk),
        ];
        let report = exec(&steps, &mut Vec::new(), &ctx()).expect("план исполнен");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[Icon Theme]\n");
        assert_eq!(report.notes, vec!["часть пропущена".to_string()]);
        assert!(report.apply.ok);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An error of a file step stops the plan and says which step and where.
    #[test]
    fn file_failure_stops_plan() {
        let dir = scratch("fail");
        let missing = s(&dir.join("нет-такого"));
        let after = dir.join("after");
        let steps = vec![
            Step::from(ExtStep::RemoveFile { path: missing.clone() }),
            Step::from(ExtStep::MakeDirs { path: s(&after) }),
        ];
        let r = exec(&steps, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Remove { ref path, .. }) if *path == missing), "{r:?}");
        assert!(!after.exists(), "шаг после ошибки исполнен");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file copy is strictly file to file: the destination directory is created,
    /// an existing directory in place of the file is an error, a link in place of
    /// the file is removed rather than written through.
    #[test]
    fn copy_file_is_file_to_file() {
        let dir = scratch("copy");
        let src = dir.join("a.txt");
        std::fs::write(&src, "a").unwrap();
        let into = dir.join("into");
        std::fs::create_dir_all(&into).unwrap();
        let step = [Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&into) })];
        let r = exec(&step, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Copy { .. })), "{r:?}");
        assert!(!into.join("a.txt").exists());

        let outside = dir.join("outside.txt");
        std::fs::write(&outside, "чужой").unwrap();
        let dest = dir.join("new/sub/b.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&outside, &dest).unwrap();
        let step = [Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&dest) })];
        exec(&step, &mut Vec::new(), &ctx()).expect("файл в файл");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "a");
        assert!(!std::fs::symlink_metadata(&dest).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "чужой", "копия прошла сквозь ссылку");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tree: links are moved as links (dangling ones too), a new link goes on top
    /// of an old one.
    #[test]
    fn copy_tree_keeps_links() {
        let dir = scratch("tree");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub/f.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink("sub/f.svg", src.join("alias.svg")).unwrap();
        std::os::unix::fs::symlink("../нет-такого", src.join("dangling.svg")).unwrap();
        let dest = dir.join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        std::os::unix::fs::symlink("старая", dest.join("alias.svg")).unwrap();
        let step = [Step::from(ExtStep::CopyTree { src: s(&src), dest: s(&dest) })];
        exec(&step, &mut Vec::new(), &ctx()).expect("дерево перенесено");
        assert_eq!(std::fs::read_link(dest.join("alias.svg")).unwrap(), Path::new("sub/f.svg"));
        assert_eq!(std::fs::read_link(dest.join("dangling.svg")).unwrap(), Path::new("../нет-такого"));
        assert_eq!(std::fs::read_to_string(dest.join("alias.svg")).unwrap(), "<svg/>");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Commands and launching without waiting go ONLY through the context: both
    /// file steps and enabling steps; the fake launches nothing real (on 25.10.2026
    /// a test with a fake launched `cinnamon --replace` on the live screen — this
    /// path is closed here).
    #[test]
    fn commands_and_spawn_go_through_ctx() {
        use crate::themes::Cmd;
        let ran = std::cell::RefCell::new(Vec::<(Vec<String>, Option<u64>)>::new());
        let spawned = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], t: Option<u64>| {
            ran.borrow_mut().push((argv.to_vec(), t));
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let spawn = |argv: &[String]| spawned.borrow_mut().push(argv.to_vec());
        let have = |b: &str| b == "есть-такая";
        let c = Ctx { run: &run, spawn: &spawn, have: &have, ..ctx() };
        let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let steps = vec![
            Step::from(ExtStep::Run { argv: v(&["кеш"]), timeout_secs: None }),
            Step::from(ExtStep::RunNoted { argv: v(&["заметка"]), timeout_secs: Some(7), failed: "нет".into() }),
            Step::from(ExtStep::Spawn { argv: v(&["cinnamon", "--replace"]) }),
            Step::from(ApplyStep::Primary(Cmd { argv: v(&["включить"]), timeout: Some(5) })),
            Step::from(ApplyStep::Live(Cmd { argv: v(&["нет-такой"]), timeout: None })),
            Step::from(ApplyStep::Live(Cmd { argv: v(&["есть-такая"]), timeout: None })),
        ];
        let report = exec(&steps, &mut Vec::new(), &c).expect("план исполнен");
        assert!(report.apply.ok);
        assert_eq!(report.apply.live, Some(true));
        assert!(report.notes.is_empty());
        assert_eq!(
            *ran.borrow(),
            vec![
                (v(&["кеш"]), None),
                (v(&["заметка"]), Some(7)),
                (v(&["включить"]), Some(5)),
                (v(&["есть-такая"]), None),
            ]
        );
        assert_eq!(*spawned.borrow(), vec![v(&["cinnamon", "--replace"])]);
    }

    /// Without a system executor (the store) the SteamOS check does not trigger, and
    /// `pkexec` goes as an ordinary command.
    #[test]
    fn plan_without_privileged_runner_runs_pkexec_as_command() {
        let seen = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], _: Option<u64>| {
            seen.borrow_mut().push(argv.to_vec());
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let c = Ctx { steamos: true, run: &run, ..ctx() };
        let argv = vec!["pkexec".to_string(), "true".to_string()];
        let steps = [Step::from(ExtStep::Run { argv: argv.clone(), timeout_secs: Some(5) })];
        exec(&steps, &mut Vec::new(), &c).expect("план исполнен");
        assert_eq!(*seen.borrow(), vec![argv]);
    }

    /// Everything under `root` byte for byte: path, kind, file contents or link target.
    fn snapshot(root: &Path) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let md = std::fs::symlink_metadata(&p).unwrap();
                let what = if md.file_type().is_symlink() {
                    format!("link → {}", std::fs::read_link(&p).unwrap().display())
                } else if md.is_dir() {
                    stack.push(p.clone());
                    "dir".to_string()
                } else {
                    format!("file {:?}", std::fs::read(&p).unwrap())
                };
                out.push((rel, what));
            }
        }
        out.sort();
        out
    }

    /// A failure in the middle of an installation: everything the run managed to put
    /// is removed, everything replaced (a file, a link, the previous copy of a
    /// theme, a config rewritten in place) is restored — the profile is byte for
    /// byte as before the installation.
    #[test]
    fn failed_install_rolls_back_to_byte_identical_profile() {
        let dir = scratch("rollback");
        let src = dir.join("src");
        std::fs::create_dir_all(src.join("Theme/sub")).unwrap();
        std::fs::write(src.join("Theme/index.theme"), "новый").unwrap();
        std::fs::write(src.join("Theme/sub/a.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink("sub/a.svg", src.join("Theme/alias.svg")).unwrap();
        std::fs::write(src.join("one.colors"), "новая схема").unwrap();

        let home = dir.join("home");
        // A foreign directory on the copy's path — with a foreign file and a file of
        // the same name.
        std::fs::create_dir_all(home.join("icons/Foreign")).unwrap();
        std::fs::write(home.join("icons/Foreign/index.theme"), "чужой").unwrap();
        std::fs::write(home.join("icons/Foreign/own.txt"), "чужой свой").unwrap();
        // The previous copy of one's own theme (a repeated installation removes it).
        std::fs::create_dir_all(home.join("themes/Mine/gtk-3.0")).unwrap();
        std::fs::write(home.join("themes/Mine/gtk-3.0/gtk.css"), "старая").unwrap();
        // A file and a link the plan will replace, and a config it will rewrite.
        std::fs::create_dir_all(home.join("schemes")).unwrap();
        std::fs::write(home.join("schemes/one.colors"), "старая схема").unwrap();
        std::os::unix::fs::symlink("/нет/такого", home.join("schemes/two.colors")).unwrap();
        std::fs::write(home.join("app.conf"), "[a]\nkey=old\n").unwrap();
        let before = snapshot(&home);

        let h = |p: &str| s(&home.join(p));
        let steps: Vec<Step> = vec![
            ExtStep::MakeDirs { path: h("new/base") },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("icons/Foreign") },
            ExtStep::RemoveTree { path: h("themes/Mine"), safe: false, ignore_errors: true },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("themes/Mine") },
            ExtStep::CopyTree { src: s(&src.join("Theme")), dest: h("new/base/Fresh") },
            ExtStep::CopyFile { src: s(&src.join("one.colors")), dest: h("schemes/one.colors") },
            ExtStep::CopyFile { src: s(&src.join("one.colors")), dest: h("schemes/two.colors") },
            ExtStep::WriteFile { path: h("app.conf"), content: "[a]\nkey=new\n".into() },
            ExtStep::WriteFile { path: h("deep/new/rexon.conf"), content: "x".into() },
            ExtStep::Symlink { target: "themes/Mine".into(), link: h("link") },
            ExtStep::RemoveFile { path: h("нет-такого") },
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let r = exec(&steps, &mut Vec::new(), &ctx());
        assert!(matches!(r, Err(Failure::Remove { .. })), "{r:?}");
        assert_eq!(snapshot(&home), before, "сбой оставил профиль не таким, как до установки");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Step 5a: "Apply" — the enabling layer's writes under the same rollback. A
    /// write failure in the middle of a click: all the click's configs are byte for
    /// byte as before (an ini edited in place, an ini through a link, our new file, a
    /// removed file, a key removed from `settings.ini`); the steps after the failure
    /// are not executed.
    #[test]
    fn failed_apply_press_rolls_back_config_writes() {
        use crate::themes::Cmd;
        let dir = scratch("press");
        let home = dir.join("home");
        let cfg = home.join(".config");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-theme-name=Old\ngtk-cursor-theme-name=Old\n").unwrap();
        std::fs::write(cfg.join("kdeglobals"), "[General]\nfoo=1\n[Icons]\nTheme=Old\n").unwrap();
        // The person's config, laid out as a link from their store.
        std::fs::create_dir_all(dir.join("store")).unwrap();
        std::fs::write(dir.join("store/lxqt.conf"), "[General]\nicon_theme=Old\n").unwrap();
        std::fs::create_dir_all(cfg.join("lxqt")).unwrap();
        std::os::unix::fs::symlink(dir.join("store/lxqt.conf"), cfg.join("lxqt/lxqt.conf")).unwrap();
        std::fs::write(cfg.join("ours.conf"), "наш прежний").unwrap();
        // A file in place of a directory: writing under it will fail.
        std::fs::write(cfg.join("gtk-4.0"), "не каталог").unwrap();
        let before = (snapshot(&home), snapshot(&dir.join("store")));

        let c = |p: &str| s(&cfg.join(p));
        let ran = std::cell::RefCell::new(Vec::<Vec<String>>::new());
        let run = |argv: &[String], _: Option<u64>| {
            ran.borrow_mut().push(argv.to_vec());
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let ctx = Ctx { run: &run, ..ctx() };
        let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let steps: Vec<Step> = vec![
            ApplyStep::Primary(Cmd { argv: v(&["включить"]), timeout: None }),
            ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::GtkIniUnsetIfOk { path: c("gtk-3.0/settings.ini"), key: "gtk-cursor-theme-name".into() },
            ApplyStep::IniSetIfOk { path: c("kdeglobals"), section: "[Icons]".into(), key: "Theme".into(), value: "New".into() },
            ApplyStep::IniSetIfOk { path: c("lxqt/lxqt.conf"), section: "[General]".into(), key: "icon_theme".into(), value: "New".into() },
            ApplyStep::WriteIfOk { dir: c("rexon-new/sub"), path: c("rexon-new/sub/cursor.conf"), content: "x".into() },
            ApplyStep::WriteSetsOk { dir: c(""), path: c("ours.conf"), content: "наш новый".into() },
            ApplyStep::RemoveIfOk { path: c("kdeglobals") },
            ApplyStep::GtkIni { path: c("gtk-4.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::IfOk(Cmd { argv: v(&["после-сбоя"]), timeout: None }),
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let r = exec(&steps, &mut Vec::new(), &ctx);
        assert!(matches!(r, Err(Failure::Write { ref path, .. }) if path.ends_with("gtk-4.0/settings.ini")), "{r:?}");
        assert_eq!((snapshot(&home), snapshot(&dir.join("store"))), before, "сбой посреди «Применить» оставил конфиги не прежними");
        assert_eq!(*ran.borrow(), vec![v(&["включить"])], "шаг после сбоя исполнен");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Step 5a: a successful click writes the configs and leaves no service copies;
    /// a link to the person's config stays a link.
    #[test]
    fn apply_press_writes_and_leaves_no_undo_copies() {
        let dir = scratch("press-ok");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
        std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-theme-name=Old\n").unwrap();
        std::fs::write(dir.join("real.conf"), "[General]\nicon_theme=Old\n").unwrap();
        std::os::unix::fs::symlink(dir.join("real.conf"), cfg.join("lxqt.conf")).unwrap();
        let c = |p: &str| s(&cfg.join(p));
        let steps: Vec<Step> = vec![
            ApplyStep::ForceOk,
            ApplyStep::GtkIni { path: c("gtk-3.0/settings.ini"), key: "gtk-theme-name".into(), value: "New".into() },
            ApplyStep::IniSetIfOk { path: c("lxqt.conf"), section: "[General]".into(), key: "icon_theme".into(), value: "New".into() },
            ApplyStep::WriteSetsOk { dir: c("new"), path: c("new/ours.conf"), content: "наш".into() },
        ]
        .into_iter()
        .map(Step::from)
        .collect();
        let report = exec(&steps, &mut Vec::new(), &ctx()).expect("нажатие удалось");
        assert!(report.apply.ok);
        assert!(std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap().contains("gtk-theme-name=New"));
        assert!(std::fs::symlink_metadata(cfg.join("lxqt.conf")).unwrap().file_type().is_symlink(), "ссылка стала файлом");
        assert!(std::fs::read_to_string(dir.join("real.conf")).unwrap().contains("icon_theme=New"));
        let names: Vec<String> = snapshot(&cfg).into_iter().map(|(p, _)| p).collect();
        assert_eq!(names, ["gtk-3.0", "gtk-3.0/settings.ini", "lxqt.conf", "new", "new/ours.conf"], "остались служебные копии");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A failure inside a foreign directory (closed for writing): the foreign stays
    /// intact, our own is removed, and so is what landed before the failure.
    #[test]
    fn failure_inside_foreign_dir_keeps_foreign() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("foreign");
        let src = dir.join("src/Theme");
        std::fs::create_dir_all(src.join("xfwm4")).unwrap();
        std::fs::write(src.join("index.theme"), "наш").unwrap();
        std::fs::write(src.join("xfwm4/themerc"), "наш").unwrap();
        let home = dir.join("home");
        let foreign = home.join("themes/Theme");
        std::fs::create_dir_all(foreign.join("xfwm4")).unwrap();
        std::fs::write(foreign.join("index.theme"), "чужой").unwrap();
        std::fs::write(foreign.join("xfwm4/keep"), "чужой").unwrap();
        std::fs::set_permissions(foreign.join("xfwm4"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let before = snapshot(&home);
        let steps = [
            Step::from(ExtStep::CopyTree { src: s(&dir.join("src/Theme")), dest: s(&home.join("icons/Fresh")) }),
            Step::from(ExtStep::CopyTree { src: s(&src), dest: s(&foreign) }),
        ];
        let r = exec(&steps, &mut Vec::new(), &ctx());
        let after = snapshot(&home);
        std::fs::set_permissions(foreign.join("xfwm4"), std::fs::Permissions::from_mode(0o755)).unwrap();
        if std::fs::write(foreign.join("xfwm4/probe"), "").is_ok() && r.is_ok() {
            // Write permissions do not apply under root — there will be no failure.
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        assert!(matches!(r, Err(Failure::Copy { .. })), "{r:?}");
        assert_eq!(after, before, "сбой в чужом каталоге задел чужое или оставил своё");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Success: the copies set aside for the rollback do not remain next to the
    /// files; removal (`install = false`) is not rolled back.
    #[test]
    fn success_leaves_no_undo_copies_and_removal_is_not_rolled_back() {
        let dir = scratch("commit");
        let src = dir.join("a.txt");
        std::fs::write(&src, "новый").unwrap();
        let dest = dir.join("home/a.txt");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, "старый").unwrap();
        std::fs::create_dir_all(dir.join("home/Old")).unwrap();
        let steps = [
            Step::from(ExtStep::RemoveTree { path: s(&dir.join("home/Old")), safe: false, ignore_errors: false }),
            Step::from(ExtStep::CopyFile { src: s(&src), dest: s(&dest) }),
        ];
        exec(&steps, &mut Vec::new(), &ctx()).expect("план исполнен");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "новый");
        let names: Vec<String> = snapshot(&dir.join("home")).into_iter().map(|(p, _)| p).collect();
        assert_eq!(names, vec!["a.txt".to_string()], "остались служебные копии");

        std::fs::create_dir_all(dir.join("home/Gone")).unwrap();
        let removal = [
            Step::from(ExtStep::RemoveTree { path: s(&dir.join("home/Gone")), safe: false, ignore_errors: false }),
            Step::from(ExtStep::RemoveFile { path: s(&dir.join("home/нет-такого")) }),
        ];
        let c = Ctx { install: false, ..ctx() };
        assert!(exec(&removal, &mut Vec::new(), &c).is_err());
        assert!(!dir.join("home/Gone").exists(), "удаление откатилось");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A substitute desktop: `gsettings` stores (set by the person on top of schema
    /// defaults), `dconf` and `xfconf` in memory, and a launch log. It answers the
    /// same command forms as the real programs; the real ones are not launched.
    #[derive(Default)]
    pub(crate) struct FakeDesk {
        pub defaults: std::collections::BTreeMap<String, String>,
        pub gsettings: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub dconf: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub xfconf: std::cell::RefCell<std::collections::BTreeMap<String, String>>,
        pub ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl FakeDesk {
        pub(crate) fn run(&self, argv: &[String]) -> crate::utils::RunReport {
            self.ran.borrow_mut().push(argv.to_vec());
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            let answer = |ok: bool, out: String| crate::utils::RunReport {
                returncode: Some(if ok { 0 } else { 1 }),
                stdout: out.into_bytes(),
                ..Default::default()
            };
            let id = |s: &str, k: &str| format!("{s} {k}");
            match &a[..] {
                ["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", s, k] => match self.defaults.get(&id(s, k)) {
                    Some(v) => answer(true, format!("{v}\n")),
                    None => answer(false, String::new()),
                },
                ["gsettings", "get", s, k] => match self.defaults.get(&id(s, k)) {
                    Some(d) => answer(true, format!("{}\n", self.gsettings.borrow().get(&id(s, k)).unwrap_or(d))),
                    None => answer(false, String::new()),
                },
                ["gsettings", "set", s, k, v] if self.defaults.contains_key(&id(s, k)) => {
                    self.gsettings.borrow_mut().insert(id(s, k), v.to_string());
                    answer(true, String::new())
                }
                ["gsettings", "reset", s, k] if self.defaults.contains_key(&id(s, k)) => {
                    self.gsettings.borrow_mut().remove(&id(s, k));
                    answer(true, String::new())
                }
                ["dconf", "read", k] => answer(true, self.dconf.borrow().get(*k).map(|v| format!("{v}\n")).unwrap_or_default()),
                ["dconf", "write", k, v] => {
                    self.dconf.borrow_mut().insert(k.to_string(), v.to_string());
                    answer(true, String::new())
                }
                ["dconf", "reset", k] => {
                    self.dconf.borrow_mut().remove(*k);
                    answer(true, String::new())
                }
                ["xfconf-query", "-c", c, "-p", p, rest @ ..] => {
                    let key = id(c, p);
                    let exists = self.xfconf.borrow().contains_key(&key);
                    match rest {
                        [] => answer(exists, self.xfconf.borrow().get(&key).map(|v| format!("{v}\n")).unwrap_or_default()),
                        ["-r"] => {
                            self.xfconf.borrow_mut().remove(&key);
                            answer(true, String::new())
                        }
                        ["-s", v] if exists => {
                            self.xfconf.borrow_mut().insert(key, v.to_string());
                            answer(true, String::new())
                        }
                        ["-n", "-t", _, "-s", v] => {
                            self.xfconf.borrow_mut().insert(key, v.to_string());
                            answer(true, String::new())
                        }
                        _ => answer(false, String::new()),
                    }
                }
                _ => answer(false, String::new()),
            }
        }

        /// Everything the desktop stores — for the "as it was" check.
        pub(crate) fn state(&self) -> String {
            format!("{:?} {:?} {:?}", self.gsettings.borrow(), self.dconf.borrow(), self.xfconf.borrow())
        }
    }

    pub(crate) fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|x| x.to_string()).collect()
    }

    /// Parsing commands that change desktop values: which key and how it is read;
    /// what we do not parse we do not touch.
    #[test]
    fn value_targets_of_commands() {
        let key = |a: &[&str]| value_target(&v(a)).map(|t| t.key);
        assert_eq!(key(&["gsettings", "set", "org.a", "k", "'v'"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["/usr/bin/gsettings", "reset", "org.a", "k"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["env", "X=1", "gsettings", "--schemadir", "/d", "set", "org.a", "k", "1"]).as_deref(), Some("gsettings org.a k"));
        assert_eq!(key(&["gsettings", "get", "org.a", "k"]), None);
        assert_eq!(key(&["gsettings", "reset-recursively", "org.a"]), None);
        assert_eq!(key(&["dconf", "write", "/org/a/k", "'v'"]).as_deref(), Some("dconf /org/a/k"));
        assert_eq!(key(&["dconf", "reset", "-f", "/org/a/"]), None, "ветку целиком не снимаем");
        assert_eq!(key(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "X"]).as_deref(), Some("xfconf xsettings /Net/ThemeName"));
        assert_eq!(key(&["xfconf-query", "--channel=xfwm4", "--property", "/general/theme", "--set=X"]).as_deref(), Some("xfconf xfwm4 /general/theme"));
        assert_eq!(key(&["xfconf-query", "-c", "c", "-p", "/p", "-r", "-R"]), None, "ветку целиком не снимаем");
        assert_eq!(key(&["xfconf-query", "-c", "c", "-p", "/p"]), None, "чтение не запись");
        assert_eq!(key(&["bash", "-c", "gsettings set org.a k v"]), None);
        assert_eq!(key(&["env", "-i", "gsettings", "set", "org.a", "k", "v"]), None);
        // Reading the schema default — with the same variables and schema directory.
        let desk = FakeDesk::default();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let t = value_target(&v(&["env", "X=1", "gsettings", "--schemadir", "/d", "set", "org.a", "k", "1"])).unwrap();
        let _ = value_before(&t, &run);
        assert_eq!(
            desk.ran.borrow().clone(),
            [
                v(&["env", "X=1", "gsettings", "--schemadir", "/d", "get", "org.a", "k"]),
            ],
            "чтение не удалось — умолчание не спрашивается"
        );
        for read in [
            v(&["gsettings", "get", "org.a", "k"]),
            v(&["env", "GSETTINGS_BACKEND=memory", "gsettings", "get", "org.a", "k"]),
            v(&["dconf", "read", "/org/a/k"]),
            v(&["xfconf-query", "-c", "c", "-p", "/p"]),
        ] {
            assert!(is_value_read(&read), "{read:?}");
        }
        for write in [v(&["gsettings", "set", "org.a", "k", "v"]), v(&["xfconf-query", "-c", "c", "-p", "/p", "-s", "v"]), v(&["dconf", "write", "/a", "1"])] {
            assert!(!is_value_read(&write), "{write:?}");
        }
    }

    /// A file step failure in the middle of a click: desktop values that managed to
    /// change are restored right away (together with the files), nothing captured
    /// remains.
    #[test]
    fn failed_press_puts_values_back() {
        use crate::themes::Cmd;
        let dir = scratch("values-fail");
        let desk = FakeDesk {
            defaults: [("org.a gtk-theme".to_string(), "'Adwaita'".to_string())].into(),
            ..Default::default()
        };
        desk.gsettings.borrow_mut().insert("org.a gtk-theme".into(), "'Old'".into());
        desk.xfconf.borrow_mut().insert("xsettings /Net/ThemeName".into(), "Old".into());
        let before = desk.state();
        let run = |a: &[String], _: Option<u64>| desk.run(a);
        let c = Ctx { run: &run, ..ctx() };
        let blocker = dir.join("файл");
        std::fs::write(&blocker, "").unwrap();
        let steps = vec![
            Step::from(ApplyStep::Primary(Cmd { argv: v(&["gsettings", "set", "org.a", "gtk-theme", "'New'"]), timeout: None })),
            Step::from(ExtStep::Run { argv: v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName", "-s", "New"]), timeout_secs: None }),
            Step::from(ExtStep::Run { argv: v(&["xfconf-query", "-c", "xsettings", "-p", "/Net/IconThemeName", "-n", "-t", "string", "-s", "New"]), timeout_secs: None }),
            // Writing under a file as if under a directory — a failure.
            Step::from(ExtStep::WriteFile { path: s(&blocker.join("x.ini")), content: "x".into() }),
        ];
        let mut undo = Undo::new().keeping_values();
        assert!(exec_undoable(&steps, &mut Vec::new(), &c, &mut undo).is_err());
        assert_eq!(desk.state(), before, "значения стола не вернулись после сбоя");
        assert!(undo.take_values().is_empty(), "снятое осталось в откате");
        undo.finish(false);
        // Without `keeping_values` there are no reads: other plans go as before.
        desk.ran.borrow_mut().clear();
        let mut plain = Undo::new();
        let _ = exec_undoable(&steps[..1], &mut Vec::new(), &c, &mut plain);
        assert_eq!(desk.ran.borrow().clone(), [v(&["gsettings", "set", "org.a", "gtk-theme", "'New'"])]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `kwriteconfig6`: the file it will rewrite is captured as a copy before the
    /// command — a failure in the middle of a click restores it, success puts the
    /// copy into the "how it was" snapshot. A name without a path — in the config
    /// directory, without `--file` — `kdeglobals`.
    #[test]
    fn kwriteconfig_file_is_kept() {
        let cfg = "/h/.config";
        let f = |a: &[&str]| kconfig_written_file(&v(a), cfg);
        assert_eq!(f(&["kwriteconfig6", "--file", "kwinrc", "--group", "G", "--key", "K", "v"]), Some("/h/.config/kwinrc".into()));
        assert_eq!(f(&["kwriteconfig5", "--group", "Icons", "--key", "Theme", "x"]), Some("/h/.config/kdeglobals".into()));
        assert_eq!(f(&["env", "A=1", "kwriteconfig6", "--file=/abs/rc", "--key", "K", "v"]), Some("/abs/rc".into()));
        assert_eq!(f(&["kreadconfig6", "--file", "kwinrc", "--key", "K"]), None);

        let dir = scratch("kconfig");
        let rc = dir.join("kdeglobals");
        std::fs::write(&rc, "[Icons]\nTheme=Old\n").unwrap();
        let fresh = dir.join("sub/kcminputrc");
        let run = |a: &[String], _: Option<u64>| {
            // A kwriteconfig6 fake: writes the file from `--file`.
            if a[0] == "kwriteconfig6" {
                let p = Path::new(&a[2]);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "[Icons]\nTheme=New\n").unwrap();
            }
            crate::utils::RunReport { returncode: Some(0), ..Default::default() }
        };
        let c = Ctx { run: &run, ..ctx() };
        let blocker = dir.join("файл");
        std::fs::write(&blocker, "").unwrap();
        let steps = vec![
            Step::from(ExtStep::Run { argv: v(&["kwriteconfig6", "--file", &s(&rc), "--group", "Icons", "--key", "Theme", "New"]), timeout_secs: None }),
            Step::from(ExtStep::Run { argv: v(&["kwriteconfig6", "--file", &s(&fresh), "--key", "K", "v"]), timeout_secs: None }),
        ];
        // A failure after the commands: the file is as before, what was created is removed.
        let mut failing = steps.clone();
        failing.push(Step::from(ExtStep::WriteFile { path: s(&blocker.join("x")), content: "x".into() }));
        let mut undo = Undo::new().keeping_values();
        assert!(exec_undoable(&failing, &mut Vec::new(), &c, &mut undo).is_err());
        undo.finish(false);
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "[Icons]\nTheme=Old\n");
        assert!(!fresh.exists() && !dir.join("sub").exists(), "заведённое командой осталось");
        // Success: the copy goes into the snapshot, what was created — as a mark.
        let mut undo = Undo::new().keeping_values();
        exec_undoable(&steps, &mut Vec::new(), &c, &mut undo).unwrap();
        let touched = undo.keep_into(&dir.join("snap"), 1);
        let paths: Vec<&Path> = touched.iter().map(|t| t.path.as_path()).collect();
        assert_eq!(paths, [std::fs::canonicalize(&rc).unwrap().as_path(), dir.join("sub").as_path(), fresh.as_path()]);
        assert_eq!(std::fs::read_to_string(touched[0].before.as_ref().unwrap()).unwrap(), "[Icons]\nTheme=Old\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
