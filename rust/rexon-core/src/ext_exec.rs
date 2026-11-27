//! The extension manager's executor on the live file system.
//!
//! WHY. The extension planner has long been in Rust — `ext_manager.rs` can
//! DECIDE everything: check a .rxn, derive the id, build an install/uninstall plan for
//! appearance with [`ExtStep`] steps. But it cannot DO anything: its input is
//! an `FsSnap` snapshot, there is no real file system in it at all. The executor
//! of this plan was until now Python (`_exec_plan` in
//! `rexon/extensions/manager.py`), and because of that the side-effects object
//! of the «Extensions» tab had to be kept on PySide6.
//!
//! The executor is written here: the same side calls Python made
//! (`shutil`/`os`/`subprocess`/`pkexec`), but in Rust. The split is the same and
//! it is fundamental — **the planner decides, the executor does**:
//!   * what to copy, what to write, what to run — [`ext_manager`];
//!   * the FS snapshot for the planner — [`plan_snapshot`] (port of `_plan_snapshot`);
//!   * running the steps — [`exec_plan`] (port of `_exec_plan`);
//!   * whole pipelines — [`install_from_file`], [`uninstall`], [`toggle`].
//!
//! ⭐ IMPORTERS OF FOREIGN FORMATS ARE CONNECTED (07.09.2026). The old boundary —
//! "I understand only `.rxn`" — is lifted: the live executor of the `ImpIo` trait moved
//! from the `rexon-py` bridge into the core ([`crate::imp_live`]), and [`install_from_file`]
//! again does what Python did: `.themepack`/`.deskthemepack` and
//! Windows themes (`.theme`/`.msstyles`/`.wmv`/`.dream`/`.gadget`) are first
//! converted to `.rxn` in a temporary directory, and the result is what gets installed.
//! Otherwise the tab would deceive with its own file chooser filter
//! (`qt_extensions::FILE_DIALOG_FILTER` offers Windows themes in so many
//! words).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::constants;
use crate::ext_manager::{self, ExtEnv, ExtInfo, ExtStep, FsSnap, TmpAlloc};
use crate::ext_utils;
use crate::extension::Extension;
use crate::i18n::{tr, trf};
use crate::qt_extensions;

// ============================================================================
// Odds and ends
// ============================================================================

/// `$HOME` — the same one Python substitutes (`os.path.expanduser("~")`).
fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// An executor error is a ready string for the person: the tab's status
/// line shows it, as it showed `str(e)` from Python.
pub type ExecResult<T> = Result<T, String>;

fn err<T>(text: impl Into<String>) -> ExecResult<T> {
    Err(text.into())
}

// ============================================================================
// File system snapshots for the planner
// ============================================================================

/// The snapshot collector: the same four layers as Python's (`_plan_snapshot`).
#[derive(Default)]
struct SnapBuilder {
    dirs: Map<String, Value>,
    files: Map<String, Value>,
    links: Vec<Value>,
    pre_dirs: Vec<Value>,
    pre_files: Vec<Value>,
    /// Files the shared log recorded for the package (`FsSnap::owns_file`).
    owned_files: Vec<Value>,
    /// `[package file, file in place]` pairs that are equal byte for byte
    /// (`FsSnap::same_bytes`).
    same_files: Vec<Value>,
}

impl SnapBuilder {
    fn into_value(self) -> Value {
        let mut out = Map::new();
        out.insert("dirs".into(), Value::Object(self.dirs));
        out.insert("files".into(), Value::Object(self.files));
        out.insert("links".into(), Value::Array(self.links));
        out.insert("pre_dirs".into(), Value::Array(self.pre_dirs));
        out.insert("pre_files".into(), Value::Array(self.pre_files));
        out.insert("owned_files".into(), Value::Array(self.owned_files));
        out.insert("same_files".into(), Value::Array(self.same_files));
        Value::Object(out)
    }

    /// Single files of the package that already lie in the destination directories
    /// under the same name: which of them are byte-for-byte the package's files
    /// (the owner rule for files, `ext_manager::put_owned_file`).
    /// Only regular files (not links) of the same size are compared.
    fn same_files_in(&mut self, ext_dir: &Path, bases: &[PathBuf]) {
        let prefix = format!("{}/", s(ext_dir));
        let mut by_name: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        for path in self.files.keys() {
            if let Some(rel) = path.strip_prefix(&prefix) {
                let name = rel.rsplit('/').next().unwrap_or(rel);
                by_name.entry(name.to_string()).or_default().push(path.clone());
            }
        }
        for base in bases {
            for name in Self::names_of(base) {
                let Some(srcs) = by_name.get(&name) else { continue };
                let dest = base.join(&name);
                let Ok(md) = std::fs::symlink_metadata(&dest) else { continue };
                if !md.file_type().is_file() {
                    continue;
                }
                let mut dest_bytes: Option<Vec<u8>> = None;
                for src in srcs {
                    if std::fs::metadata(src).map(|m| m.len()).ok() != Some(md.len()) {
                        continue;
                    }
                    if dest_bytes.is_none() {
                        dest_bytes = std::fs::read(&dest).ok();
                    }
                    if dest_bytes.is_some() && std::fs::read(src).ok() == dest_bytes {
                        self.same_files.push(Value::Array(vec![Value::String(src.clone()), Value::String(s(&dest))]));
                    }
                }
            }
        }
    }

    fn file(&mut self, path: &Path, size: u64, text: Option<String>) {
        let mut m = Map::new();
        m.insert("size".into(), Value::from(size));
        m.insert(
            "text".into(),
            match text {
                Some(t) => Value::String(t),
                None => Value::Null,
            },
        );
        self.files.insert(s(path), Value::Object(m));
    }

    /// Directory names in reading order. `read_dir` returns them in FS order, and
    /// Python's `os.scandir` in its own; the order affects choosing "the first
    /// matching" file in the plan, so the names are sorted — this way the storefront and
    /// the plan behave the same from machine to machine (the same argument as for
    /// [`qt_extensions::installed_in`]).
    fn names_of(dir: &Path) -> Vec<String> {
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

    /// A deep traversal (port of `deep`): the whole tree, with the texts of
    /// `metadata.json`/`metadata.desktop` — from them the planner gets
    /// the plasmoid's plugin-id.
    ///
    /// `observed_only = true` — the records go only into the observations layer: this is what
    /// the original saw AFTER its own steps, and it must not become decisions about "did it exist
    /// before us".
    fn deep(&mut self, root: &Path, observed_only: bool) {
        let key = s(root);
        if self.dirs.contains_key(&key) && observed_only {
            return;
        }
        let names = Self::names_of(root);
        self.dirs.insert(
            key,
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        if !observed_only {
            self.pre_dirs.push(Value::String(s(root)));
        }
        for name in names {
            let path = root.join(&name);
            let link = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if link {
                self.links.push(Value::String(s(&path)));
            }
            let is_dir = path.is_dir();
            if is_dir {
                if link {
                    // like os.walk(followlinks=False): the directory is in the classification,
                    // we do not go inside
                    self.dirs.entry(s(&path)).or_insert_with(|| Value::Array(Vec::new()));
                    if !observed_only {
                        self.pre_dirs.push(Value::String(s(&path)));
                    }
                } else {
                    self.deep(&path, observed_only);
                }
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let text = if name == "metadata.json" || name == "metadata.desktop" {
                std::fs::read_to_string(&path).ok()
            } else {
                None
            };
            self.file(&path, size, text);
            if !observed_only {
                self.pre_files.push(Value::String(s(&path)));
            }
        }
    }

    /// A file with its text (small settings files: LightDM).
    fn text_file(&mut self, path: &Path) {
        if self.files.contains_key(&s(path)) || !path.is_file() {
            return;
        }
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        self.file(path, size, std::fs::read_to_string(path).ok());
        self.pre_files.push(Value::String(s(path)));
    }

    /// A directory of settings files with texts — only names with `suffix`
    /// (`.conf`, `.desktop`); links are read by target (on Debian the greeter
    /// description is a link to the real one).
    fn texts_of(&mut self, dir: &Path, suffix: &str) {
        if self.dirs.contains_key(&s(dir)) || !dir.is_dir() {
            return;
        }
        let names: Vec<String> = Self::names_of(dir).into_iter().filter(|n| n.ends_with(suffix)).collect();
        self.dirs.insert(s(dir), Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()));
        self.pre_dirs.push(Value::String(s(dir)));
        for name in names {
            self.text_file(&dir.join(name));
        }
    }

    /// A shallow traversal of the target directory (port of `shallow`): presence,
    /// classification and symlinks of the children — without recursion. This is enough for
    /// the planner's `exists`/`isdir` decisions, while traversing the whole `~/.local/share/icons`
    /// would mean reading tens of thousands of files on every installation.
    fn shallow(&mut self, base: &Path) {
        if self.dirs.contains_key(&s(base)) || !base.is_dir() {
            return;
        }
        let names = Self::names_of(base);
        self.dirs.insert(
            s(base),
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        self.pre_dirs.push(Value::String(s(base)));
        for name in names {
            let path = base.join(&name);
            if std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)
            {
                self.links.push(Value::String(s(&path)));
            }
            if path.is_dir() {
                self.dirs.entry(s(&path)).or_insert_with(|| Value::Array(Vec::new()));
                self.pre_dirs.push(Value::String(s(&path)));
            } else if path.is_file() {
                self.pre_files.push(Value::String(s(&path)));
            }
        }
    }

    /// The tree of `src_root` re-rooted into `dest_root` — into the observations
    /// (port of `project`). This way the planner sees the contents of ITS OWN
    /// copying: conky looks for the config in the target directory already.
    fn project(&mut self, src_root: &Path, dest_root: &Path) {
        let names = Self::names_of(src_root);
        self.dirs.insert(
            s(dest_root),
            Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        for name in names {
            let src = src_root.join(&name);
            let dest = dest_root.join(&name);
            if src.is_dir() {
                if std::fs::symlink_metadata(&src)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                {
                    self.dirs.entry(s(&dest)).or_insert_with(|| Value::Array(Vec::new()));
                } else {
                    self.project(&src, &dest);
                }
                continue;
            }
            let size = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
            self.file(&dest, size, None);
        }
    }
}

/// A tree snapshot for the `.rxn` validation pipeline (port of `_snap_tree`).
///
/// The planner reads the unpacked archive as an OBSERVATIONS layer (`obs_is_file`) —
/// it appeared during the run, not before it. So the pre-layers stay
/// empty: `FsSnap::from_json` without them would consider everything pre-state.
pub fn snap_tree(root: &Path) -> Value {
    let mut b = SnapBuilder::default();
    b.deep(root, true);
    b.into_value()
}

/// Directories where Rexon installs appearance — the same ones the planner sees.
///
/// ⚠️ They are taken as TWO sets deliberately. The planner builds a path as
/// `{HOME}/{suffix}` (`ExtEnv::install_path`), while the product table does it
/// XDG-aware (`constants::install_path`). On a machine with
/// `XDG_DATA_HOME` set these are different directories, and the snapshot must cover both: otherwise
/// the "already exists" decision would be made on an empty place.
fn target_dirs(home: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for t in constants::valid_types() {
        if let Some(suffix) = constants::install_path_suffix(t) {
            out.push(PathBuf::from(format!("{home}/{suffix}")));
        }
        if let Some(path) = constants::install_path(t) {
            out.push(path);
        }
    }
    out.push(PathBuf::from(format!("{home}/.themes")));
    out.push(PathBuf::from(crate::utils::expand_user(
        "~/.config/autostart",
        home,
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
    )));
    out.sort();
    out.dedup();
    out
}

/// The `.plasmoid` archive the plan will choose in the directory `dir`: the first by
/// name order, unless a directory with a plasmoid description came before it
/// (then the directory is installed, the archive is not needed).
fn first_plasmoid_archive(dir: &Path) -> Option<PathBuf> {
    for name in SnapBuilder::names_of(dir) {
        let full = dir.join(&name);
        if name.ends_with(".plasmoid") {
            return Some(full);
        }
        if full.is_dir() && (full.join("metadata.json").is_file() || full.join("metadata.desktop").is_file()) {
            return None;
        }
    }
    None
}

/// Directories where the plan looks for the package's plasmoid: `data/` for the plasmoid type,
/// the `theme-plasmoid` part subdirectory for a bundle.
fn plasmoid_dirs(ext: &Extension) -> Vec<PathBuf> {
    match ext.ext_type() {
        "theme-plasmoid" => vec![ext.dir.join("data")],
        "theme-bundle" => ext
            .manifest
            .get("bundle")
            .and_then(Value::as_object)
            .and_then(|m| m.get("theme-plasmoid"))
            .and_then(Value::as_str)
            .map(|rel| vec![ext.dir.join(rel)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The FS snapshot for generating the install/uninstall plan (port of `_plan_snapshot`).
pub fn plan_snapshot(ext: &Extension, tmp_base: &Path, install: bool) -> Value {
    let home = home();
    let mut b = SnapBuilder::default();
    b.deep(&ext.dir, false);
    for base in target_dirs(&home) {
        b.shallow(&base);
    }
    // An existing theme directory (the main copy, `~/.themes/<theme>` for
    // frames, bundle subfolders and Conky) is touched by the plan only by the owner
    // mark — its text is needed both by installation (update our own, do not touch
    // foreign) and by removal. For some types the directory name comes from the metadata
    // (uuid, KPlugin.Id), so the mark is read for every base directory.
    for base in target_dirs(&home) {
        for name in SnapBuilder::names_of(&base) {
            b.text_file(&base.join(&name).join(ext_manager::OWNER_MARK));
        }
    }
    // Single files (color schemes, Nemo actions, bundle fonts)
    // carry no mark — whether the file in place is ours, the plan learns from the shared log
    // (recorded for this package) or by a byte-for-byte match.
    let journal = crate::install_journal::view_at(&rexon_config_dir());
    let key = crate::install_journal::key_of(crate::install_journal::Source::Rxn, ext.ext_id());
    if let Some(entry) = journal.items.get(&key) {
        for p in &entry.placed {
            if matches!(p.what, Some(crate::install_journal::What::File) | None) {
                b.owned_files.push(Value::String(p.path.clone()));
            }
        }
    }
    b.same_files_in(&ext.dir, &target_dirs(&home));
    // .plasmoid: the plasmoid id is inside the archive. The snapshot unpacks it
    // in advance (`ext_manager::PLASMOID_PROBE_DIR`) so that the plan decides "a foreign
    // plasmoid with the same id is in place — do not touch" BEFORE starting kpackagetool, and
    // removal takes away the directory of the real id.
    for dir in plasmoid_dirs(ext) {
        if let Some(file) = first_plasmoid_archive(&dir) {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let probe = tmp_base.join(ext_manager::PLASMOID_PROBE_DIR).join(name);
            if std::fs::create_dir_all(&probe).is_ok() && ext_utils::safe_zip_extract(&file, &probe).is_ok() {
                b.deep(&probe, true);
            }
        }
    }
    if !install {
        // Removing a login screen or splash theme: the plan looks at whether
        // it lies in the system directory (the root helper removes it).
        for base in ["/usr/share/sddm/themes", crate::system_plan::PLYMOUTH_THEMES_DIR] {
            b.shallow(Path::new(base));
        }
    }

    if install {
        // The LightDM login screen: which greeter is installed, the plan decides by
        // the LightDM settings files and the greeter descriptions — their texts are needed.
        for dir in ext_manager::LIGHTDM_CONF_DIRS {
            b.texts_of(Path::new(dir), ".conf");
        }
        b.texts_of(Path::new(ext_manager::XGREETERS_DIR), ".desktop");
        b.text_file(Path::new(ext_manager::LIGHTDM_CONF));

        // conky: the planner looks for the config in the target directory AFTER copying
        // — we project the source (dest after rmtree+copytree equals it one to one).
        let conky_dest = PathBuf::from(crate::utils::expand_user(
            "~/.config/conky",
            &home,
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        ))
        .join(ext.ext_id());
        match ext.ext_type() {
            "theme-conky" => b.project(&ext.dir.join("data"), &conky_dest),
            "theme-bundle" => {
                if let Some(bundle) = ext.manifest.get("bundle").and_then(Value::as_object) {
                    for (sub_type, sub_rel) in bundle {
                        if sub_type == "theme-conky" {
                            if let Some(rel) = sub_rel.as_str() {
                                let sub_dir = ext.dir.join(rel);
                                if sub_dir.is_dir() {
                                    b.project(&sub_dir, &conky_dest);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // .plasmoid: the planner's manual branch (no kpackagetool or it did not
        // work) reads the tree of the UNPACKED archive. We unpack in advance
        // into the path the plan's first mkdtemp will give — otherwise the branch
        // would see emptiness.
        if ext.ext_type() == "theme-plasmoid" {
            if let Some(file) = first_plasmoid_archive(&ext.dir.join("data")) {
                let tmp0 = tmp_base.join("rexon_plasmoid_0");
                if std::fs::create_dir_all(&tmp0).is_ok()
                    && ext_utils::safe_zip_extract(&file, &tmp0).is_ok()
                {
                    b.deep(&tmp0, true);
                }
            }
        }
    }

    b.into_value()
}

// ============================================================================
// The plan: the planner decides
// ============================================================================

/// A ready plan together with what has already been executed while building it.
pub struct Plan {
    pub steps: Vec<ExtStep>,
    /// Probes EXECUTED by the `run_ok` callback right during generation — the executor
    /// will skip their same-named steps in turn.
    pub pre_ran: Vec<Vec<String>>,
    /// The plan's working directory under `/tmp`; removed at the end of the run.
    pub tmp_base: PathBuf,
    /// An install plan (`true`) or an uninstall one: this decides what to do with
    /// the system part on SteamOS — see [`exec_plan`].
    pub install: bool,
}

/// `tempfile.mkdtemp(prefix=…, dir=…)` without external crates: the name from the time and
/// a counter, creation — `create_dir` (fails if the name is taken — so the
/// directory is ours and ours only).
pub(crate) fn mkdtemp(dir: &Path, prefix: &str) -> ExecResult<PathBuf> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    for attempt in 0..64u64 {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{prefix}{}_{}_{}", std::process::id(), stamp, n + attempt));
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return err(trf("couldn't create a temporary folder: %1", &[&e])),
        }
    }
    err(tr("couldn't create a temporary folder: all names are taken"))
}

/// Build an install or uninstall plan (port of `_plan_steps`).
///
/// ⚠️ The `run_ok` callback RUNS the probes already here, in the same order in which
/// the original called them (`kpackagetool -u`/`-i`, `systemctl is-active`) — otherwise
/// the planner will not know which branch to choose. The executor then skips their steps
/// by the `pre_ran` list.
pub fn make_plan(ext: &Extension, install: bool) -> ExecResult<Plan> {
    // dir="/tmp" is mandatory: the plan's staged paths must pass the stage-helper's realpath
    // check (like the original's `mkdtemp(dir="/tmp")`).
    let tmp_base = mkdtemp(Path::new("/tmp"), "rexon_plan_")?;
    let built = build_plan(ext, install, &tmp_base);
    match built {
        Ok((steps, pre_ran)) => Ok(Plan { steps, pre_ran, tmp_base, install }),
        Err(e) => {
            // We clean up the temporary directory after ourselves and pass the error up:
            // either there is a plan or the installation must not start.
            let _ = std::fs::remove_dir_all(&tmp_base);
            Err(e)
        }
    }
}

fn build_plan(
    ext: &Extension,
    install: bool,
    tmp_base: &Path,
) -> ExecResult<(Vec<ExtStep>, Vec<Vec<String>>)> {
    let home = home();
    let snap_value = plan_snapshot(ext, tmp_base, install);
    let fs = FsSnap::from_json(&snap_value);
    let tmp = TmpAlloc::new(&s(tmp_base));

    let pre_ran = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let which = |name: &str| crate::utils::which(name, std::env::var("PATH").ok().as_deref());
    let run_ok = |argv: &[String]| -> bool {
        let argv = argv.to_vec();
        if argv.is_empty() {
            return false;
        }
        // kpackagetool -i: the original runs -u right before -i —
        // we execute the pair in the original order.
        if argv.len() == 3 && argv[1] == "-i" {
            let u_argv = vec![argv[0].clone(), "-u".to_string(), argv[2].clone()];
            pre_ran.borrow_mut().push(u_argv.clone());
            let _ = run_capture(&u_argv, 30);
        }
        let timeout = if Path::new(&argv[0])
            .file_name()
            .map(|n| n == "systemctl")
            .unwrap_or(false)
        {
            5
        } else {
            30
        };
        pre_ran.borrow_mut().push(argv.clone());
        run_capture(&argv, timeout) == Some(0)
    };

    // The helper is looked for in both legitimate directories (a package — `/usr/lib`,
    // a manual install — `/usr/local/lib`), like the other helpers.
    let system_helper_path = crate::installer::system_helper_path();
    let system_helper_exists = crate::system_plan::is_exec_file(system_helper_path);
    let family = if crate::system::DistroDetector::detect_live().family_live()
        == crate::system::PkgFamily::Debian
    {
        "debian"
    } else {
        "arch"
    };
    let env = ExtEnv {
        home: &home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: &std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
        xdg_session_type: &std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
        wayland_display: std::env::var_os("WAYLAND_DISPLAY").is_some(),
        stage_helper_available: helper_available(
            crate::system_plan::stage_helper_path(),
            crate::system_plan::STAGE_POLICY_PATH,
        ),
        system_helper_path,
        system_helper_exists,
        helper_family: family,
        tmp: &tmp,
    };
    let info = ext_manager::ExtInfo { dir: &s(&ext.dir), manifest: &ext.manifest };
    let steps = if install {
        ext_manager::safe_install_plan(&env, &info)
    } else {
        ext_manager::safe_uninstall_plan(&env, &info)
    };
    Ok((steps, pre_ran.into_inner()))
}

/// Whether the helper is deployed (port of `_helper_available`): the binary and its polkit rule
/// are both in place.
fn helper_available(helper_path: &str, policy_path: &str) -> bool {
    Path::new(helper_path).exists() && Path::new(policy_path).exists()
}

// ============================================================================
// The executor: the executor does
// ============================================================================

/// `subprocess.run(argv, capture_output=True, timeout=…)` — the return code or
/// `None` (did not start / timeout / killed by a signal).
fn run_capture(argv: &[String], timeout_secs: u64) -> Option<i32> {
    if argv.is_empty() {
        return None;
    }
    let mut child = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.code(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    // In Python a timeout kills the process and raises an exception;
                    // here the same outcome as a single value: "there was no success".
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

// Copying is shared with the other install paths (`install_plan`).
use crate::install_plan::copy_file;

// ============================================================================
// The system part of the plan (everything that goes through pkexec)
// ============================================================================
//
// ⭐ Fix of 28.09.2026 — a silent refusal. Previously the executor CALLED pkexec and
// THREW AWAY the answer: both for the wrapper script and for the Rexon helper. A password
// refusal, "read-only file system" (SteamOS), any error
// writing to `/usr` — the person pressed «Install», saw "installed", and
// the login or boot screen theme never appeared. Now any
// non-zero answer becomes a step error with text for the person, and
// the install pipeline rolls back just as with any other error.

/// How a privileged step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivOutcome {
    /// Code 0.
    Done,
    /// pkexec returned a return code ≠ 0; `stderr` — what the command said.
    Failed { code: i32, stderr: String },
    /// Did not fit into the time — the process was stopped.
    TimedOut,
    /// pkexec did not start at all (it is not on the system).
    NotStarted,
}

// Which steps are system ones and what they write (SteamOS, checking the theme before the password) is
// decided by the shared executor (`install_plan`); here — only the launch itself.

/// The text for the person by the privileged step's outcome; `None` — success.
///
/// A password refusal is recognized the same way as in all other system actions of
/// Rexon ([`crate::system_plan::pkexec_auth_denied`]). For other refusals the
/// text gets the last line of the command's answer — "read-only",
/// "no space" and anything else we cannot know in advance are all there.
pub fn privileged_failure_text(outcome: &PrivOutcome) -> Option<String> {
    match outcome {
        PrivOutcome::Done => None,
        PrivOutcome::NotStarted => Some(crate::system_plan::pkexec_missing_message()),
        PrivOutcome::TimedOut => Some(tr(
            "Rexon could not change the system part of the theme: it took too long and was stopped.",
        )),
        PrivOutcome::Failed { code, stderr } => {
            if crate::system_plan::pkexec_auth_denied(*code, stderr) {
                return Some(tr(
                    "The administrator password was not confirmed, so Rexon could not change the \
                     system part of the theme.",
                ));
            }
            let detail = stderr
                .lines()
                .map(str::trim)
                .rfind(|l| !l.is_empty())
                .map(|l| l.chars().take(300).collect::<String>());
            Some(match detail {
                // TRANSLATORS: %1 is the last line printed by the failed system command, as is (often English).
                Some(line) => trf("Rexon could not change the system part of the theme: %1", &[&line]),
                None => trf(
                    "Rexon could not change the system part of the theme (error code %1).",
                    &[code],
                ),
            })
        }
    }
}

/// Run a privileged command, keeping its error answer.
///
/// The error stream goes to a file, not a pipe: a pipe would overflow on
/// a verbose boot image rebuild, and the process would stall.
fn run_privileged(argv: &[String], timeout_secs: u64, work: &Path) -> PrivOutcome {
    crate::log_info!("privileged", "запуск: {}", crate::applog::redact_argv(argv));
    let started = Instant::now();
    let outcome = run_privileged_inner(argv, timeout_secs, work);
    let what = match &outcome {
        PrivOutcome::Done => "готово".to_string(),
        PrivOutcome::NotStarted => "не запустилось".to_string(),
        PrivOutcome::TimedOut => format!("не уложилось в {timeout_secs} с"),
        PrivOutcome::Failed { code, stderr } => format!(
            "код {code}: {}",
            stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("")
        ),
    };
    crate::log_info!(
        "privileged",
        "{}: {what} за {:.1} с",
        argv.first().map(String::as_str).unwrap_or(""),
        started.elapsed().as_secs_f64()
    );
    outcome
}

/// The old path of a system step — via the shared command executor
/// ([`crate::utils::run`]: pipe readers in their own threads, a verbose
/// kernel image rebuild will not hang the process). There is no `Command::new` of our own for
/// `pkexec` here (the guard `no_raw_pkexec_spawn_outside_privilege`).
fn run_privileged_inner(argv: &[String], timeout_secs: u64, _work: &Path) -> PrivOutcome {
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(timeout_secs),
            ..crate::utils::RunSpec::default()
        },
        None,
    );
    use crate::utils::RunOutcome as Ro;
    match report.outcome {
        Ro::Ok => PrivOutcome::Done,
        Ro::Timeout => PrivOutcome::TimedOut,
        Ro::Error => PrivOutcome::NotStarted,
        // Killed by a signal — there is no code; for the person it is the same refusal.
        Ro::Failed => PrivOutcome::Failed {
            code: report.returncode.unwrap_or(-1),
            stderr: crate::utils::decode_text(&report.stderr),
        },
    }
}

/// The privilege session operation for a system step of a `.rxn` plan and its files
/// (the layout directories). The core builds the argv (`ext_manager`: the layout
/// and system theme helpers); the operation is accepted ONLY if its "as now" path
/// ([`PrivOp::pkexec_argv`]) gives exactly this argv — there is no second
/// truth about the helpers' flags and paths. `None` — the step goes the old way
/// (the wrapper script, another helper).
pub(crate) fn step_priv_op(step: &ExtStep) -> Option<(crate::privilege::op::PrivOp, Vec<String>)> {
    use crate::privilege::op::{Helpers, LocalFile, PrivOp, SystemThemeKind, SystemThemeOp, ThemeFamily};
    let ExtStep::Run { argv, .. } = step else { return None };
    if argv.first().map(String::as_str) != Some("pkexec") {
        return None;
    }
    let helper = argv.get(1)?.as_str();
    let rest = crate::install_plan::helper_args(argv);
    let file = |label: &str| LocalFile { slot: 0, label: label.to_string() };
    let (op, files) = if helper == crate::system_plan::stage_helper_path() {
        let [category, name, staged] = rest else { return None };
        (PrivOp::Stage { category: category.clone(), name: name.clone(), source: file(staged) }, vec![staged.clone()])
    } else if helper == crate::installer::system_helper_path() {
        let [word, theme, src, fam] = rest else { return None };
        let family = match fam.as_str() {
            "arch" => ThemeFamily::Arch,
            "debian" => ThemeFamily::Debian,
            _ => return None,
        };
        let kind_of = |k: &str| match k {
            "sddm" => Some(SystemThemeKind::Sddm),
            "plymouth" => Some(SystemThemeKind::Plymouth),
            _ => None,
        };
        let theme = theme.clone();
        if let Some(k) = word.strip_prefix("apply-").and_then(kind_of) {
            (PrivOp::SystemTheme(SystemThemeOp::Apply { kind: k, theme, family }), Vec::new())
        } else if let Some(k) = word.strip_prefix("remove-").and_then(kind_of) {
            (PrivOp::SystemTheme(SystemThemeOp::Remove { kind: k, theme, family }), Vec::new())
        } else {
            let kind = kind_of(word)?;
            (PrivOp::SystemTheme(SystemThemeOp::Install { kind, theme, source: file(src), family }), vec![src.clone()])
        }
    } else {
        return None;
    };
    let helpers = Helpers { stage: true, system: true, ..Helpers::default() };
    (op.pkexec_argv(&helpers).as_ref() == Some(argv)).then_some((op, files))
}

/// A system command of the themes and layouts helper (the core's argv) through the window's
/// privilege session — for executors that have no `.rxn` plan but have an argv
/// (the appearance store: installing, «Apply», «Remove» of a system theme).
/// `None` — the command is not one of these or there is no session: the executor runs it
/// the old way. The helper's answer is in the form of an ordinary launch report.
pub(crate) fn run_system_argv_session(argv: &[String], timeout_secs: Option<u64>) -> Option<crate::utils::RunReport> {
    let (op, files) = step_priv_op(&ExtStep::Run { argv: argv.to_vec(), timeout_secs })?;
    let out = crate::privilege::run::run_live(&op, &files, None, &mut |_| {})?;
    crate::log_info!("privileged", "сеанс прав: код {:?}", out.returncode);
    Some(crate::utils::RunReport {
        outcome: out.outcome,
        returncode: out.returncode,
        stdout: out.stdout.into_bytes(),
        stderr: out.stderr.into_bytes(),
        timeout_secs: timeout_secs.unwrap_or(0),
        error: String::new(),
    })
}

/// The privilege session's result in the form of a system step result.
fn priv_outcome_of(out: &crate::system_exec::RunOut) -> PrivOutcome {
    use crate::utils::RunOutcome as Ro;
    match (out.outcome, out.returncode) {
        (_, Some(0)) => PrivOutcome::Done,
        (Ro::Timeout, _) => PrivOutcome::TimedOut,
        (Ro::Error, _) => PrivOutcome::NotStarted,
        (_, code) => PrivOutcome::Failed {
            code: code.unwrap_or(-1),
            // The helper's explanation — as progress lines (`stdout`), client refusals — in `stderr`.
            stderr: if out.stderr.trim().is_empty() { out.stdout.clone() } else { out.stderr.clone() },
        },
    }
}

/// The live executor of a privileged step: the wrapper script —
/// `pkexec bash <file>` (timeout 60 s, as in the original), the helper — its argv.
fn run_privileged_step_live(step: &ExtStep) -> PrivOutcome {
    use std::os::unix::fs::PermissionsExt;
    // The window's privilege session (one password); if there is none or the step is beyond it —
    // the old path. A helper refusal is not repeated via the old path (Z5).
    if let Some((op, files)) = step_priv_op(step) {
        if let Some(out) = crate::privilege::run::run_live(&op, &files, None, &mut |_| {}) {
            let outcome = priv_outcome_of(&out);
            crate::log_info!("privileged", "сеанс прав: {outcome:?}");
            return outcome;
        }
    }
    let prefix = match step {
        ExtStep::PkexecScript { prefix, .. } => prefix.as_str(),
        _ => "rexon_priv",
    };
    let Ok(holder) = mkdtemp(&std::env::temp_dir(), &format!("rexon_{prefix}_")) else {
        return PrivOutcome::NotStarted;
    };
    let outcome = match step {
        ExtStep::PkexecScript { script, .. } => {
            let path = holder.join("script.sh");
            let written = (|| -> std::io::Result<()> {
                let mut f = std::fs::File::create(&path)?;
                f.write_all(script.as_bytes())?;
                f.flush()?;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            })();
            match written {
                Ok(()) => run_privileged(
                    &["pkexec".to_string(), "bash".to_string(), s(&path)],
                    60,
                    &holder,
                ),
                Err(e) => PrivOutcome::Failed { code: -1, stderr: e.to_string() },
            }
        }
        ExtStep::Run { argv, timeout_secs } => {
            run_privileged(argv, timeout_secs.unwrap_or(30), &holder)
        }
        _ => PrivOutcome::Done,
    };
    let _ = std::fs::remove_dir_all(&holder);
    outcome
}

/// Execute the core's plan (port of `_exec_plan`): the same side calls Python
/// made. A step error is propagated — the original's semantics: an install failure
/// leads to a rollback in [`install_from_file`].
///
/// The system part (steps through pkexec) is not executed on SteamOS at all:
/// the system partition is closed there, and an update would erase what was written. An installation
/// with such a part REFUSES entirely even before the first step — with a reason for
/// the person; uninstalling skips the system part (there is nothing to remove it with there, and
/// the person has the right to remove the user part).
pub fn exec_plan(plan: Plan) -> ExecResult<()> {
    exec_plan_noted(plan).map(|_| ())
}

/// The same, with notes for the person: what of the plan was not done and why
/// ([`ExtStep::Notice`], a failed [`ExtStep::RunNoted`]).
///
/// On failure the install plan is rolled back entirely (`install_plan::exec`).
pub fn exec_plan_noted(plan: Plan) -> ExecResult<Vec<String>> {
    let mut undo = if plan.install { crate::install_plan::Undo::new() } else { crate::install_plan::Undo::off() };
    let outcome = exec_plan_noted_undo(plan, &mut undo);
    undo.finish(outcome.is_ok());
    outcome
}

/// The same, but the rollback accumulates in the caller's `undo` and is not executed: this way
/// a `.rxn` installation on failure removes the plan, the package copy, and whatever
/// broke after the plan (writing the state).
fn exec_plan_noted_undo(plan: Plan, undo: &mut crate::install_plan::Undo) -> ExecResult<Vec<String>> {
    let Plan { steps, mut pre_ran, tmp_base, install } = plan;
    let ctx = ExecCtx {
        steamos: crate::distro_compat::running_on_steamos(),
        install,
        privileged: &run_privileged_step_live,
    };
    let mut notes = Vec::new();
    let outcome = exec_steps_noted(&steps, &mut pre_ran, &ctx, &mut notes, undo);
    let _ = std::fs::remove_dir_all(&tmp_base);
    outcome.map(|()| notes)
}

/// The externals for a plan run: where we are and what to execute system steps with.
struct ExecCtx<'a> {
    steamos: bool,
    install: bool,
    privileged: &'a dyn Fn(&ExtStep) -> PrivOutcome,
}

fn exec_steps_noted(
    steps: &[ExtStep],
    pre_ran: &mut Vec<Vec<String>>,
    ctx: &ExecCtx,
    notes: &mut Vec<String>,
    undo: &mut crate::install_plan::Undo,
) -> ExecResult<()> {
    let plan: Vec<crate::install_plan::Step> = steps.iter().cloned().map(Into::into).collect();
    let common = crate::install_plan::Ctx::live(ctx.install, ctx.steamos, Some(ctx.privileged), false);
    match crate::install_plan::exec_undoable(&plan, pre_ran, &common, undo) {
        Ok(report) => {
            notes.extend(report.notes);
            Ok(())
        }
        Err(f) => err(crate::install_plan::failure_text(&f)),
    }
}

// ============================================================================
// Script extensions and .desktop
// ============================================================================

/// Run an extension's `install.sh`/`uninstall.sh` (port of `_run_script`):
/// `bash <script>` from the extension directory, timeout 120 s.
fn run_script(ext: &Extension, name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let script = ext.dir.join(name);
    if !script.is_file() {
        return false;
    }
    let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    let Ok(mut child) = Command::new("bash")
        .arg(&script)
        .current_dir(&ext.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

/// The path of the extension's `.desktop` file in the user menu.
fn desktop_entry_path(ext_id: &str) -> PathBuf {
    PathBuf::from(crate::utils::expand_user_xdg(
        "~/.local/share/applications",
        &home(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
    ))
    .join(format!("rexon-{ext_id}.desktop"))
}

/// Port of `install_desktop_entry`: a copy of the `.desktop` in the menu plus an appended
/// category if the file did not have one.
fn install_desktop_entry(ext: &Extension) -> bool {
    // The file name is fixed (`Extension.desktop_file` is `<dir>/app.desktop`),
    // the manifest does not set it.
    let src = ext.dir.join("app.desktop");
    if !src.is_file() {
        return false;
    }
    let dest = desktop_entry_path(ext.ext_id());
    let Some(parent) = dest.parent() else { return false };
    if std::fs::create_dir_all(parent).is_err() || copy_file(&src, &dest).is_err() {
        return false;
    }
    match std::fs::read_to_string(&dest) {
        Ok(content) if !content.contains("Categories=") => {
            let patched = format!("{}\nCategories=Utility;Rexon;\n", content.trim_end());
            std::fs::write(&dest, patched).is_ok()
        }
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Port of `remove_desktop_entry`.
fn remove_desktop_entry(ext_id: &str) {
    let dest = desktop_entry_path(ext_id);
    if dest.exists() {
        let _ = std::fs::remove_file(&dest);
    }
}

// ============================================================================
// On/off state
// ============================================================================

/// Mark the extension `ext_id` as enabled/disabled (`Some`) or forget
/// it (`None` — removed). Only the shared install log is written (step 5b
/// of merging the installers); the state map `extensions_state.json` is its
/// mirror with the same text the old `_save_state` wrote
/// (`install_journal::set_rxn_enabled_at`).
fn save_state_with(ext_id: &str, set: Option<bool>) -> ExecResult<()> {
    crate::install_journal::set_rxn_enabled_at(&rexon_config_dir(), ext_id, set)
        .map_err(|e| trf("couldn't save the state: %1", &[&e]))
}

/// The Rexon config directory where the state map and the shared log are.
fn rexon_config_dir() -> PathBuf {
    qt_extensions::state_file().parent().map(Path::to_path_buf).unwrap_or_default()
}

// ============================================================================
// The tab's pipelines
// ============================================================================

/// The install result: the extension name plus what the person must learn about.
pub struct Installed {
    /// The NAME of the installed extension — exactly what the tab's status
    /// line showed (`ext.name` from the Python worker).
    pub name: String,
    /// Warnings about SKIPPED optional steps: the installation succeeded,
    /// but something was done in a simplified way, and that must not be kept silent.
    pub notes: Vec<String>,
}

/// Install an extension from a file: a `.rxn` directly, a foreign format — via
/// conversion.
pub fn install_from_file(rxn_path: &str) -> ExecResult<Installed> {
    let path = Path::new(rxn_path);
    if !path.is_file() {
        return err(trf("File not found: %1", &[&rxn_path]));
    }

    // A foreign format — first conversion to .rxn, then a regular installation.
    // The order of checks and the names of temporary directories — as in the original
    // (`_install_from_file_unlocked`): first .themepack, then the other
    // Windows formats.
    if let Some(converted) = import_foreign(rxn_path)? {
        let outcome = install_rxn(Path::new(&converted.rxn_path));
        let _ = std::fs::remove_dir_all(&converted.tmp_dir);
        return outcome.map(|(name, notes)| Installed { name, notes: [converted.notes, notes].concat() });
    }

    install_rxn(path).map(|(name, notes)| Installed { name, notes })
}

/// Installing a ready `.rxn` (port of `_install_rxn`).
///
/// ⚠️ The message to the bridge lives HERE, not with the callers: this function handles both
/// `.rxn` and converted Windows themes, and both windows that install
/// extensions («Extensions» and theme import in «Settings»). The original sent
/// `extension_installed` from the same place (`_install_rxn`), while the Rust window did not
/// send it at all — the bridge's external application never learned about the installation.
fn install_rxn(path: &Path) -> ExecResult<(String, Vec<String>)> {
    let tmp = mkdtemp(&std::env::temp_dir(), "rexon_rxn_")?;
    let mut outcome = install_unpacked(path, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    // Any package might have put icons or cursors: also check the profile's shared
    // `hicolor` set — a truncated `index.theme` in it hides the icons of
    // the whole desktop ([`crate::icon_hygiene`]). The check is one file read.
    if let Ok((_, notes)) = &mut outcome {
        if let Some(line) = crate::icon_hygiene::heal_live() {
            notes.push(line);
        }
    }
    if let Ok((name, _)) = &outcome {
        let bridge = crate::rexon_bridge::RexonBridge::new_default();
        if let Err(e) = announce_installed(&bridge, name, std::time::SystemTime::now()) {
            // The bridge is auxiliary: its refusal does not cancel the installation.
            crate::log_warn!("bridge", "весть об установке «{name}» не отправлена — {e}");
        }
    }
    outcome
}

/// Tell the bridge that the extension is installed (port of
/// `rexon_bridge.notify("notify_extension_installed", …)`).
///
/// Cleaning up files older than 5 minutes — as in the original's `notify`: whoever litters with events
/// sweeps up. It sweeps up on a send failure too: old files do not become
/// more needed because of it. Uninstalling an extension sends no message — the original has
/// no such event, and the listener has no lines for it.
pub fn announce_installed(
    bridge: &crate::rexon_bridge::RexonBridge,
    name: &str,
    now: std::time::SystemTime,
) -> std::io::Result<()> {
    let since = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let iso = crate::engine_update::now_iso(since.as_secs() as i64);
    let epoch_ms = since.as_millis() as u64;
    let sent = bridge
        .ensure_dirs()
        .and_then(|_| bridge.notify_extension_installed(name, &iso, epoch_ms));
    bridge.cleanup_commands(300.0, epoch_ms as f64 / 1000.0);
    sent
}

/// A ready `.rxn` from a foreign format, the temporary directory and what was skipped.
struct Converted {
    rxn_path: String,
    tmp_dir: String,
    notes: Vec<String>,
}

/// The package manager family of this machine — to name packages
/// by ITS names, not by Arch ones.
fn pkg_family() -> String {
    crate::system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

/// Convert a foreign format to `.rxn`. `Ok(None)` — the format is not foreign,
/// the file is installed as is.
///
/// ⭐ TWO SILENCES ARE CLOSED HERE.
///
/// The first is the converters. Import relies on third-party programs
/// (`cabextract`, `convert`, `7z`, `wrestool`…). A mandatory one is missing — a refusal
/// in Russian with the package name of THIS distribution, even before the first side
/// action. An optional one is missing — the import goes in a simplified way, and that is recorded
/// in the warnings. The original (`_require_tools` in `extensions/importers.py`)
/// only put such warnings into the log: the buyer never saw them,
/// and it is the same silent refusal, only with a record in a file (rule #16 —
/// the original's bug is fixed in the port).
///
/// The second is upscaling small wallpapers (the tail of importing XP/Vista themes). It
/// is connected with the live executor [`crate::imp_live::upscale_hook`] — the very one
/// the bridge substituted in a process with Python. If the picture size
/// cannot be determined, the step is skipped — but again out loud.
fn import_foreign(src: &str) -> ExecResult<Option<Converted>> {
    use crate::importers as imp;

    let mut probe = crate::imp_live::LiveImpIo::new(None);
    let (prefix, themepack) = if imp::themepack_can_import(&mut probe, src) {
        ("rexon_tp_", true)
    } else if imp::windows_theme_can_import(&mut probe, src) {
        ("rexon_win_", false)
    } else {
        return Ok(None);
    };

    // Checking the converters BEFORE the first side action — as in the original.
    let family = pkg_family();
    let tools_kind = if themepack {
        "themepack"
    } else {
        imp::import_kind(src, "")
    };
    let mut notes = match imp::require_tools(tools_kind, &family, &|tool| {
        crate::imp_live::tool_available(tool)
    }) {
        Ok(warnings) => warnings,
        Err(refusal) => return err(refusal),
    };

    let tmp = mkdtemp(&std::env::temp_dir(), prefix)?;
    let tmp_s = s(&tmp);
    let skipped = crate::imp_live::new_notes();
    let mut io = crate::imp_live::LiveImpIo::new(Some(
        crate::imp_live::upscale_hook(skipped.clone(), &family),
    ));
    let outcome = if themepack {
        imp::themepack_import_to_rxn(&mut io, src, Some(&tmp_s))
    } else {
        imp::windows_theme_import_to_rxn(&mut io, src, Some(&tmp_s))
    };
    notes.extend(crate::imp_live::take_notes(&skipped));
    // A failure of a REAL file operation matters more than the semantic outcome: in Python
    // an OSError would fly at this place and the installation would stop.
    if let Some(message) = io.io_err {
        let _ = std::fs::remove_dir_all(&tmp);
        return err(message);
    }
    match outcome {
        Ok(rxn_path) => Ok(Some(Converted { rxn_path, tmp_dir: tmp_s, notes })),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            // The error text — with the package name of THIS system, not the Arch one.
            err(imp::friendly_error_text(&e.message, &family)
                .unwrap_or(e.message))
        }
    }
}

fn install_unpacked(rxn_path: &Path, tmp: &Path) -> ExecResult<(String, Vec<String>)> {
    if ext_utils::safe_zip_extract(rxn_path, tmp).is_err() {
        return err(tr("The file is damaged or is not an .rxn archive"));
    }

    let snap_value = snap_tree(tmp);
    let fs = FsSnap::from_json(&snap_value);
    let Some(src_dir) = ext_manager::find_manifest_root(&fs, &s(tmp)) else {
        return err(tr("manifest.json not found in the .rxn archive"));
    };
    let manifest_path = Path::new(&src_dir).join("manifest.json");
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| trf("manifest.json can't be read: %1", &[&e]))?;
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|e| ext_manager::bad_manifest_json_error(&e.to_string()).message)?;

    let extensions_dir = qt_extensions::extensions_dir();
    let realpath = |p: &str| {
        std::fs::canonicalize(p)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_else(|_| p.to_string())
    };
    let dest_exists = |p: &str| Path::new(p).exists();
    let check = ext_manager::install_rxn_check(
        &fs,
        &src_dir,
        &s(&extensions_dir),
        manifest,
        &realpath,
        &dest_exists,
    )
    .map_err(|e| e.message)?;

    // The planner might have rewritten the manifest (deriving the id for v4, type migration) —
    // we put its text back BEFORE copying, as the original did.
    if let Some(rewritten) = &check.rewritten_manifest_text {
        std::fs::write(&manifest_path, rewritten)
            .map_err(|e| trf("couldn't write manifest.json: %1", &[&e]))?;
    }

    let dest_dir = extensions_dir.join(&check.ext_id);
    std::fs::create_dir_all(&extensions_dir)
        .map_err(|e| trf("couldn't create the extensions folder: %1", &[&e]))?;
    // One rollback for the whole installation — the package copy, the plan and the state record
    // (step 4 of merging the installers): a failure anywhere removes what this
    // installation placed and returns what it displaced, the profile is as before it.
    let mut undo = crate::install_plan::Undo::new();
    if let Err(e) = crate::install_plan::copy_tree(Path::new(&src_dir), &dest_dir, &mut undo) {
        undo.finish(false);
        return err(trf("couldn't copy the extension: %1", &[&e]));
    }

    let state = qt_extensions::load_state();
    type Done = (String, Vec<String>, Extension, Option<Vec<crate::install_journal::Placed>>);
    let mut finish = || -> ExecResult<Done> {
        let ext = Extension::load_from_dir(&dest_dir, &state)
            .map_err(|e| trf("the extension manifest was not accepted: %1", &[&e]))?;
        let mut notes = Vec::new();
        // What the plan will place — for the shared log, before execution (for script
        // packages only their script knows the paths).
        let mut placed = None;
        if check.is_scripted {
            run_script(&ext, "install.sh");
            if ext.ext_type() == "desktop-tool" {
                install_desktop_entry(&ext);
            }
        } else {
            let plan = make_plan(&ext, true)?;
            placed = Some(crate::install_journal::placed_from_steps(&plan.steps, &|p| {
                std::fs::symlink_metadata(p).is_ok()
            }));
            notes = exec_plan_noted_undo(plan, &mut undo)?;
            // A widget set from a file: the widgets' position — as an overlay,
            // the notes "where the set is" and "which fonts are missing".
            if let Some(set) = crate::conky_sets::set_of_manifest(&ext.manifest) {
                let ctx = crate::de_apply::Ctx::live();
                let state = std::env::var("XDG_STATE_HOME").ok().filter(|v| !v.is_empty());
                notes.extend(crate::conky_sets::after_set_install(
                    &crate::animations::SystemIo,
                    &ctx,
                    state.as_deref(),
                    &check.ext_id,
                    &set,
                    &crate::conky_sets::live_have_font,
                ));
            }
        }
        save_state_with(&check.ext_id, Some(true))?;
        let name = ext.manifest.get("name").and_then(Value::as_str).unwrap_or(&check.ext_id).to_string();
        Ok((name, notes, ext, placed))
    };
    let outcome = finish();
    // Rollback: remove what this installation placed (the package copy and the plan's files)
    // and return what was displaced. The shared log is written only after success.
    undo.finish(outcome.is_ok());
    let (name, notes, ext, placed) = outcome?;
    record_in_journal(&ext, placed);
    Ok((name, notes))
}

/// Write the installation into the shared install log (after its success). A log
/// failure does not cancel the installation: the package's "enabled" is already recorded
/// ([`save_state_with`]), and without paths the package is removed the old way, by the plan.
fn record_in_journal(ext: &Extension, placed: Option<Vec<crate::install_journal::Placed>>) {
    use crate::install_journal as journal;
    let known = placed.is_some();
    let info = journal::LegacyExt { enabled: true, ..journal::legacy_ext(ext) };
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let at = crate::engine_update::now_iso(since.as_secs() as i64);
    let entry = journal::rxn_entry(&info, placed.unwrap_or_default(), known, &at);
    if let Err(e) = journal::record_rxn_at(&rexon_config_dir(), entry) {
        crate::log_warn!("install", "установка «{}» не записана в общий журнал — {e}", ext.ext_id());
    }
}

// ============================================================================
// Cursors from Windows: converting again from the originals — only for a fix
// ============================================================================
//
// Wehrwolfmann's decision of 14.11.2026: when the «Cursors» section is opened, installed
// themes «from Windows» converted by a converter version lower than the current one are
// converted again in the background from the originals kept in the package — but only if
// at least one fix between their version and the current one concerns them
// ([`imp::CURSOR_CONVERTER_CHANGES`]). If the result is byte for byte the same, the files are not
// replaced, only the version is written to the manifest. Without originals — a skip without
// noise. Errors go to the log; the person only gets "Cursors X updated".

use crate::importers as imp;

/// The result of a pass over installed cursor themes «from Windows».
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CursorRefresh {
    /// Files replaced: (theme name, the theme directory in `data/` — the name for applying).
    pub replaced: Vec<(String, String)>,
    /// The conversion came out the same — only the version was written (package ids).
    pub version_only: Vec<String>,
    /// What did not work (already in the log).
    pub errors: Vec<String>,
}

/// Whether two trees are the same: the same names, files byte for byte, links with the same
/// targets.
fn same_tree(a: &Path, b: &Path) -> bool {
    let names = |d: &Path| -> Option<Vec<std::ffi::OsString>> {
        let mut v: Vec<_> = std::fs::read_dir(d).ok()?.flatten().map(|e| e.file_name()).collect();
        v.sort();
        Some(v)
    };
    let (Some(na), Some(nb)) = (names(a), names(b)) else {
        return false;
    };
    if na != nb {
        return false;
    }
    na.iter().all(|n| {
        let (pa, pb) = (a.join(n), b.join(n));
        let (Ok(ma), Ok(mb)) = (std::fs::symlink_metadata(&pa), std::fs::symlink_metadata(&pb)) else {
            return false;
        };
        if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
            ma.file_type().is_symlink()
                && mb.file_type().is_symlink()
                && std::fs::read_link(&pa).ok() == std::fs::read_link(&pb).ok()
        } else if ma.is_dir() || mb.is_dir() {
            ma.is_dir() && mb.is_dir() && same_tree(&pa, &pb)
        } else {
            ma.len() == mb.len() && std::fs::read(&pa).ok() == std::fs::read(&pb).ok()
        }
    })
}

/// The bytes of the package's original cursors (regular `.cur`/`.ani` files).
fn original_cursor_bytes(originals: &Path) -> Vec<Vec<u8>> {
    let Ok(rd) = std::fs::read_dir(originals) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            let l = p.to_string_lossy().to_lowercase();
            l.ends_with(".cur") || l.ends_with(".ani")
        })
        .collect();
    paths.sort();
    paths.into_iter().filter_map(|p| std::fs::read(p).ok()).collect()
}

fn write_manifest(path: &Path, m: &Value) -> std::io::Result<()> {
    std::fs::write(path, imp::dumps_indent2(m))
}

/// Go through the installed packages in `extensions_dir` and convert again the cursor
/// themes «from Windows» that a converter fix concerns.
///
/// `current`/`changes` — the converter's version and table (on the live path —
/// [`imp::CURSOR_CONVERTER_VERSION`] and [`imp::CURSOR_CONVERTER_CHANGES`]);
/// `reinstall(id, new tree)` — put the package's new tree in place of
/// the previous one (on the live path — [`replace_installed`]). The new tree's manifest
/// already carries the `current` version.
pub fn refresh_windows_cursors_in(
    extensions_dir: &Path,
    current: u32,
    changes: &[(u32, imp::CursorFixScope)],
    reinstall: &mut dyn FnMut(&str, &Path) -> Result<(), String>,
) -> CursorRefresh {
    let mut out = CursorRefresh::default();
    let Ok(rd) = std::fs::read_dir(extensions_dir) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let manifest_path = dir.join("manifest.json");
        let Some(mut manifest) = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            continue;
        };
        let Some(from) = imp::cursor_manifest_converter(&manifest) else {
            continue;
        };
        if from >= current {
            continue;
        }
        // Old themes without originals have nothing to be converted from — silently skipped.
        let originals = dir.join("originals");
        if !originals.is_dir() {
            continue;
        }
        let traits = imp::cursor_set_traits(&original_cursor_bytes(&originals));
        if !imp::cursor_retranslate_due(from, current, changes, &traits) {
            continue;
        }
        let id = manifest.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let name = manifest.get("name").and_then(Value::as_str).unwrap_or(&id).to_string();
        let fail = |out: &mut CursorRefresh, what: String| {
            crate::log_warn!("cursors", "перевод заново «{name}»: {what}");
            out.errors.push(format!("{name}: {what}"));
        };
        let tmp = match mkdtemp(&std::env::temp_dir(), "rexon_wincur_again_") {
            Ok(t) => t,
            Err(e) => {
                fail(&mut out, e);
                continue;
            }
        };
        let step = (|| -> Result<Option<String>, String> {
            let rxn = crate::imp_live::retranslate_windows_cursors(&dir, &tmp.join("out"))?;
            let fresh = tmp.join("new");
            ext_utils::safe_zip_extract(&rxn, &fresh)
                .map_err(|_| tr("The file is damaged or is not an .rxn archive"))?;
            let fresh_manifest = fresh.join("manifest.json");
            let mut m: Value = std::fs::read_to_string(&fresh_manifest)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .ok_or_else(|| tr("manifest.json not found in the .rxn archive"))?;
            imp::set_cursor_manifest_converter(&mut m, current);
            write_manifest(&fresh_manifest, &m).map_err(|e| e.to_string())?;
            if same_tree(&dir.join("data"), &fresh.join("data")) {
                // Byte for byte the same: the files are in place, only the version.
                imp::set_cursor_manifest_converter(&mut manifest, current);
                write_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
                return Ok(None);
            }
            let theme_dir = m
                .get("windows_cursors")
                .and_then(|w| w.get("theme_dir"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            reinstall(&id, &fresh)?;
            Ok(Some(theme_dir))
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        match step {
            Ok(None) => out.version_only.push(id),
            Ok(Some(theme_dir)) => out.replaced.push((name.clone(), theme_dir)),
            Err(e) => fail(&mut out, e),
        }
    }
    out
}

/// Put the new tree `fresh` of the package `ext_id` in place of the installed one —
/// with the same plan as a regular installation (the theme copy in the cursors directory
/// is replaced by the owner mark). On failure — everything as it was.
pub fn replace_installed(ext_id: &str, fresh: &Path) -> ExecResult<()> {
    let extensions_dir = qt_extensions::extensions_dir();
    let dest = extensions_dir.join(ext_id);
    if !dest.is_dir() {
        return err(trf("Extension '%1' not found", &[&ext_id]));
    }
    // The previous package — set aside on the same FS (a rename), restored on failure.
    let backup = rexon_config_dir().join(format!("extensions-old-{ext_id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&backup);
    std::fs::rename(&dest, &backup).map_err(|e| trf("couldn't copy the extension: %1", &[&e]))?;
    let restore = |dest: &Path, backup: &Path| {
        let _ = std::fs::remove_dir_all(dest);
        let _ = std::fs::rename(backup, dest);
    };
    let mut undo = crate::install_plan::Undo::new();
    if let Err(e) = crate::install_plan::copy_tree(fresh, &dest, &mut undo) {
        undo.finish(false);
        restore(&dest, &backup);
        return err(trf("couldn't copy the extension: %1", &[&e]));
    }
    let state = qt_extensions::load_state();
    let outcome = (|| -> ExecResult<(Extension, Option<Vec<crate::install_journal::Placed>>, Vec<String>)> {
        let ext = Extension::load_from_dir(&dest, &state)
            .map_err(|e| trf("the extension manifest was not accepted: %1", &[&e]))?;
        let plan = make_plan(&ext, true)?;
        let placed = Some(crate::install_journal::placed_from_steps(&plan.steps, &|p| {
            std::fs::symlink_metadata(p).is_ok()
        }));
        let notes = exec_plan_noted_undo(plan, &mut undo)?;
        Ok((ext, placed, notes))
    })();
    undo.finish(outcome.is_ok());
    match outcome {
        Ok((ext, placed, notes)) => {
            let _ = std::fs::remove_dir_all(&backup);
            for n in notes {
                crate::log_warn!("cursors", "перевод заново «{ext_id}»: {n}");
            }
            record_in_journal(&ext, placed);
            Ok(())
        }
        Err(e) => {
            restore(&dest, &backup);
            Err(e)
        }
    }
}

/// The live «Cursors» pass ([`refresh_windows_cursors_in`] over the extensions
/// directory). Returns lines for the person — only about what was really
/// replaced; empty — nothing to say. The active theme is applied again
/// with the same plan as «Apply», and the label honestly says where the new look
/// is visible right away and where — after logging in again.
pub fn refresh_windows_cursors_live() -> Vec<String> {
    let report = refresh_windows_cursors_in(
        &qt_extensions::extensions_dir(),
        imp::CURSOR_CONVERTER_VERSION,
        imp::CURSOR_CONVERTER_CHANGES,
        &mut |id, fresh| replace_installed(id, fresh),
    );
    if report.replaced.is_empty() {
        return Vec::new();
    }
    let mut installer =
        crate::appearance_installer::LiveInstaller::new("", Box::new(|_: &str| {}));
    let active = installer.active_cursor_theme();
    report
        .replaced
        .iter()
        .map(|(name, theme_dir)| {
            if !theme_dir.is_empty() && active.as_deref() == Some(theme_dir.as_str()) {
                if !installer.refresh_cursor(theme_dir) {
                    crate::log_warn!("cursors", "«{name}»: тема курсора не применилась заново");
                }
                trf(
                    // TRANSLATORS: %1 is a cursor theme name; it is the active one.
                    "Cursors “%1” updated. Apps opened from now on show the new look; everywhere — after you log in again.",
                    &[name],
                )
            } else {
                // TRANSLATORS: %1 is a cursor theme name.
                trf("Cursors “%1” updated", &[name])
            }
        })
        .collect()
}

// ── A second widget of a regular Conky package (defect of 14.11.2026) ────
//
// The old installation of a regular Conky widget (`theme-conky`, not a set) put
// the whole `data/` into `<Conky root>/<id>`, and each subfolder `data/<subfolder>`
// also into `<Conky root>/<subfolder>` (with an owner mark). The second copy is
// a second widget with the same settings file: it is visible twice in the list and
// starts twice. A new installation does not put the second copy
// (`ext_manager::safe_install_into`); the pass below removes those already there — and
// only those Rexon put itself and nobody has changed since.

/// The copy `dup` is what Rexon put from `src`: only regular files and
/// folders (no links), each file is in `src` with the same bytes or in
/// the form the path repair before launch left it in
/// ([`crate::de_apply::conky_repair_text`] for the `widget` folder). The owner
/// mark in the root does not count. An extra file (the widget wrote something itself,
/// the person edited it) — not our copy, it stays.
fn conky_copy_is_pristine(src: &Path, dup: &Path, home: &str, widget: &str, top: bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dup) else { return false };
    let exists = |p: &str| Path::new(p).exists();
    let is_dir = |p: &str| Path::new(p).is_dir();
    for e in entries.flatten() {
        let name = e.file_name();
        if top && name == ext_manager::OWNER_MARK {
            continue;
        }
        let (Ok(ft), Ok(sm)) = (e.file_type(), std::fs::symlink_metadata(src.join(&name))) else {
            return false;
        };
        if ft.is_symlink() || sm.file_type().is_symlink() {
            return false;
        }
        if ft.is_dir() {
            if !sm.is_dir() || !conky_copy_is_pristine(&src.join(&name), &e.path(), home, widget, false) {
                return false;
            }
            continue;
        }
        if !ft.is_file() || !sm.is_file() {
            return false;
        }
        let (Ok(mine), Ok(orig)) = (std::fs::read(e.path()), std::fs::read(src.join(&name))) else {
            return false;
        };
        if mine == orig {
            continue;
        }
        let repaired = std::str::from_utf8(&orig)
            .ok()
            .and_then(|t| crate::de_apply::conky_repair_text(t, home, widget, &exists, &is_dir));
        if repaired.as_deref().map(str::as_bytes) != Some(mine.as_slice()) {
            return false;
        }
    }
    true
}

/// Remove the second copies of regular Conky widgets for packages from
/// `extensions_dir` (see the note above). The copy `<root>/<subfolder>`
/// is removed only if: the package is a regular widget (not a set: in a set the
/// subfolders are the widgets), its directory `<root>/<id>/<subfolder>` is in
/// place (the widget does not go missing), the owner mark is this package's id,
/// the contents are an untouched copy ([`conky_copy_is_pristine`]). A running
/// second copy with a non-running main one is not removed (the widget will not vanish
/// from the desktop in the middle of a session — it will be removed next time). Before removal
/// the copy's processes are stopped, its autostart moves to the same widget in
/// the main directory (a nested `<id>/<subfolder>` for a package of several
/// widgets, otherwise `<id>`), if that one has none of its own. Returns the removed paths.
pub fn heal_conky_duplicates(io: &dyn crate::animations::LookIo, ctx: &crate::de_apply::Ctx, extensions_dir: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(rd) = std::fs::read_dir(extensions_dir) else { return removed };
    let mut dirs: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    dirs.sort();
    let root = crate::de_apply::conky_root(ctx);
    let valid = |n: &str| !n.is_empty() && n != "." && n != ".." && !n.contains('/');
    let mut running: Option<Vec<String>> = None;
    for dir in dirs {
        let Some(manifest) = std::fs::read_to_string(dir.join("manifest.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Map<String, Value>>(&t).ok())
        else {
            continue;
        };
        if manifest.get("type").and_then(Value::as_str) != Some(crate::de_apply::CONKY_KIND)
            || manifest.get(crate::conky_sets::SET_MANIFEST_KEY).is_some_and(Value::is_object)
        {
            continue;
        }
        let id = manifest.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let main = Path::new(&root).join(&id);
        if !valid(&id) || !main.is_dir() {
            continue;
        }
        let Ok(subs) = std::fs::read_dir(dir.join("data")) else { continue };
        let mut names: Vec<String> = subs
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            if name == id || !valid(&name) {
                continue;
            }
            let dup = format!("{root}/{name}");
            let dup_p = Path::new(&dup);
            let real_dir = std::fs::symlink_metadata(dup_p).is_ok_and(|m| m.is_dir());
            let mark = std::fs::read_to_string(dup_p.join(ext_manager::OWNER_MARK)).unwrap_or_default();
            if !real_dir || mark.trim() != id || !main.join(&name).is_dir() {
                continue;
            }
            if !conky_copy_is_pristine(&dir.join("data").join(&name), dup_p, &ctx.home, &dup, true) {
                crate::log_info!("conky", "вторая копия «{dup}» пакета «{id}» изменена — оставлена");
                continue;
            }
            // Where this widget moves to: a nested `<id>/<subfolder>` for
            // a package of several widgets, otherwise `<id>` itself.
            let nested = format!("{id}/{name}");
            let target = if crate::de_apply::conky_nested_widgets(io, ctx, &id).contains(&nested) { nested } else { id.clone() };
            let running = running.get_or_insert_with(|| crate::de_apply::conky_running_ids(io, ctx));
            if running.contains(&name) && !running.contains(&target) {
                continue;
            }
            let autostart = |w: &str| Path::new(&crate::de_apply::conky_autostart_path(ctx, w)).is_file();
            if autostart(&name) && !autostart(&target) {
                let _ = crate::de_apply::set_conky_autostart(io, ctx, &target, true);
            }
            crate::de_apply::conky_forget_widget(io, ctx, &dup);
            match std::fs::remove_dir_all(dup_p) {
                Ok(()) => {
                    crate::log_info!("conky", "снята вторая копия виджета «{id}»: {dup}");
                    removed.push(dup);
                }
                Err(e) => crate::log_warn!("conky", "вторая копия «{dup}» не снялась: {e}"),
            }
        }
    }
    removed
}

/// The live pass of [`heal_conky_duplicates`] (Rexon start). Module checks
/// do not touch the live profile.
pub fn heal_conky_duplicates_live() {
    if cfg!(test) {
        return;
    }
    let ctx = crate::de_apply::Ctx::live();
    heal_conky_duplicates(&crate::animations::SystemIo, &ctx, &qt_extensions::extensions_dir());
}

/// Uninstall an extension (port of `uninstall`).
pub fn uninstall(ext_id: &str) -> ExecResult<()> {
    let state = qt_extensions::load_state();
    let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    else {
        return err(trf("Extension '%1' not found", &[&ext_id]));
    };

    if constants::is_scripted(ext.ext_type()) {
        run_script(&ext, "uninstall.sh");
        if ext.ext_type() == "desktop-tool" {
            remove_desktop_entry(ext_id);
        }
    } else {
        if ext.ext_type() == "theme-conky" {
            // Stop the widget (by process ids) and remove its autostart.
            let ctx = crate::de_apply::Ctx::live();
            let dir = format!("{}/{ext_id}", crate::de_apply::conky_root(&ctx));
            crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, &dir);
            // A widget set from a file: the widgets are their own folders (only with this
            // package's mark; we do not touch a foreign one with the same name).
            if let Some(set) = crate::conky_sets::set_of_manifest(&ext.manifest) {
                for w in &set.widgets {
                    let wdir = format!("{}/{}", crate::de_apply::conky_root(&ctx), w.folder);
                    let mark = std::fs::read_to_string(format!("{wdir}/{}", ext_manager::OWNER_MARK)).unwrap_or_default();
                    if mark.trim() == ext_id {
                        crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, &wdir);
                    }
                }
            }
        }
        let plan = make_plan(&ext, false)?;
        // Files the installation placed are removed by the shared log (step 3d
        // of merging the installers): only what was recorded and only our own. Disabling
        // and reloading the shell are plan steps around it; a package installed
        // before the log (no paths) — with the old plan entirely.
        let journal = crate::install_journal::view_at(&rexon_config_dir());
        let key = crate::install_journal::key_of(crate::install_journal::Source::Rxn, ext_id);
        let by_journal = journal
            .items
            .get(&key)
            .and_then(|e| crate::install_journal::rxn_removal(&plan.steps, e, &crate::install_journal::probe_live));
        match by_journal {
            Some(removal) => {
                exec_plan(Plan { steps: removal.steps.clone(), ..plan })?;
                crate::install_journal::prune(&removal);
            }
            None => exec_plan(plan)?,
        }
    }
    ext_utils::safe_rmtree(&ext.dir);
    save_state_with(ext_id, None)
}

/// Forget extensions whose themes the person removed from an «Appearance» tile.
///
/// A cursor or icon theme from a Windows file is installed as an extension: a copy of the
/// package lies in the extensions directory, and the theme itself — in the themes directory.
/// «Remove» on a tile removes only the theme directory, and the extension record
/// stayed forever — with the source files and the "enabled" mark
/// (acceptance of 10.10.2026). `removed` — the removed paths. The extension is forgotten
/// when at least one of its themes was removed and none is left on disk.
/// Returns the ids of the forgotten extensions.
pub fn forget_extensions_of_removed_themes(removed: &[String]) -> Vec<String> {
    if removed.is_empty() {
        return Vec::new();
    }
    let removed: Vec<&str> = removed.iter().map(|p| p.trim_end_matches('/')).collect();
    let state = qt_extensions::load_state();
    let mut forgotten = Vec::new();
    for ext in qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state) {
        let kind = ext.ext_type().to_string();
        if constants::is_scripted(&kind) {
            continue;
        }
        let Some(base) = constants::install_path(&kind) else { continue };
        let Ok(entries) = std::fs::read_dir(ext.dir.join("data")) else { continue };
        let themes: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| base.join(e.file_name()))
            .collect();
        let hit = themes.iter().any(|t| removed.contains(&t.to_string_lossy().as_ref()));
        if !hit || themes.iter().any(|t| std::fs::symlink_metadata(t).is_ok()) {
            continue;
        }
        let id = ext.ext_id().to_string();
        match uninstall(&id) {
            Ok(()) => forgotten.push(id),
            Err(e) => crate::log_warn!("theme", "запись расширения «{id}» не снята — {e}"),
        }
    }
    forgotten
}

/// The desktop add-on's objects in the package (names, as on disk after
/// installation). Empty — the package is not a desktop add-on.
pub fn de_addon_objects(ext: &Extension) -> Vec<String> {
    let kind = ext.ext_type().to_string();
    if !matches!(crate::de_apply::mode(&kind), Some(crate::de_apply::Mode::Addon(_))) {
        return Vec::new();
    }
    let mut b = SnapBuilder::default();
    b.deep(&ext.dir, false);
    let fs = FsSnap::from_json(&b.into_value());
    let tmp = TmpAlloc::new("/tmp/rexon-de-objects");
    let which = |_: &str| None;
    let run_ok = |_: &[String]| false;
    let home = home();
    let env = ExtEnv {
        home: &home,
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "",
        xdg_session_type: "",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let dir = s(&ext.dir);
    crate::ext_manager::de_object_names(&env, &ExtInfo { dir: &dir, manifest: &ext.manifest })
}

/// Whether the desktop add-on is enabled now — by the desktop's truth, not by
/// Rexon's mark (the person may have bypassed it with the desktop's own settings).
pub fn de_addon_enabled(ext: &Extension) -> bool {
    let io = crate::animations::SystemIo;
    let ctx = crate::de_apply::Ctx::live();
    de_addon_objects(ext)
        .iter()
        .any(|n| crate::de_apply::addon_enabled(&io, &ctx, ext.ext_type(), n))
}

/// Toggle an extension (port of `toggle`). Returns the NEW state.
///
/// A desktop add-on (a KWin script, a GNOME extension, a Cinnamon applet…)
/// is enabled and disabled for real — the same way as the «Enable»/«Disable»
/// button of the «Appearance» tab (`de_apply`); installation no longer
/// enables it (decision of 04.10.2026).
pub fn toggle(ext_id: &str) -> ExecResult<bool> {
    let state = qt_extensions::load_state();
    if let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    {
        let objects = de_addon_objects(&ext);
        if !objects.is_empty() {
            let io = crate::animations::SystemIo;
            let ctx = crate::de_apply::Ctx::live();
            let next = !de_addon_enabled(&ext);
            for name in &objects {
                crate::de_apply::set_addon(&io, &ctx, ext.ext_type(), name, next)?;
            }
            save_state_with(ext_id, Some(next))?;
            return Ok(next);
        }
    }
    toggle_flag(ext_id)
}

/// Toggle the "enabled" mark of a script extension.
fn toggle_flag(ext_id: &str) -> ExecResult<bool> {
    let state = qt_extensions::load_state();
    let Some(ext) = qt_extensions::installed_in(&qt_extensions::extensions_dir(), &state)
        .into_iter()
        .find(|e| e.ext_id() == ext_id)
    else {
        // TRANSLATORS: shown in the status line after the "couldn't switch
        // the extension" prefix; %1 is the extension id.
        return err(trf("extension “%1” not found", &[&ext_id]));
    };
    let next = !ext.enabled;
    save_state_with(ext_id, Some(next))?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install_plan::steamos_block_of;

    // ── The system part of the plan: the refusal reaches the person ──────

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-ext-exec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A "user part + system part" plan — as for a login screen
    /// theme: a file in the home folder, then the helper via pkexec.
    fn plan_with_system_part(dir: &Path, kind: &str) -> Vec<ExtStep> {
        vec![
            ExtStep::WriteFile { path: s(&dir.join("user-part.txt")), content: "x".into() },
            ExtStep::Run {
                argv: vec!["pkexec".into(), "/usr/lib/rexon-system-helper".into(), kind.into()],
                timeout_secs: Some(300),
            },
        ]
    }

    fn exec_steps(steps: &[ExtStep], pre_ran: &mut Vec<Vec<String>>, ctx: &ExecCtx) -> ExecResult<()> {
        // As with `exec_plan_noted`: on failure the install plan is rolled back.
        let mut undo = if ctx.install { crate::install_plan::Undo::new() } else { crate::install_plan::Undo::off() };
        let outcome = exec_steps_noted(steps, pre_ran, ctx, &mut Vec::new(), &mut undo);
        undo.finish(outcome.is_ok());
        outcome
    }

    fn run(steps: &[ExtStep], steamos: bool, install: bool, answer: PrivOutcome) -> (ExecResult<()>, usize) {
        let calls = std::cell::Cell::new(0usize);
        let fake = |_: &ExtStep| {
            calls.set(calls.get() + 1);
            answer.clone()
        };
        let ctx = ExecCtx { steamos, install, privileged: &fake };
        let r = exec_steps(steps, &mut Vec::new(), &ctx);
        (r, calls.get())
    }

    /// A Plymouth theme with a module from the theme itself (code run as root at boot)
    /// is rejected BEFORE the password window — both via the helper and on the fallback
    /// raw `pkexec bash`, where there are no other checks at all.
    #[test]
    fn boot_splash_with_own_code_is_refused_before_password() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let src = scratch("ply-src");
        std::fs::write(src.join("neon.plymouth"), "[Plymouth Theme]\nModuleName=../../../share/plymouth/themes/neon/evil\n").unwrap();
        std::fs::write(src.join("evil.so"), b"\x7fELF").unwrap();
        let staged = scratch("ply-staged");
        let _ = std::fs::remove_dir_all(&staged);
        let stage = ExtStep::Stage { staged: s(&staged), sources: vec![s(&src)] };
        let helper = ExtStep::Run {
            argv: vec![
                "pkexec".into(), "/usr/lib/rexon-system-helper".into(), "plymouth".into(),
                "neon".into(), s(&staged), "arch".into(),
            ],
            timeout_secs: Some(900),
        };
        let fallback = ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() };
        for privileged in [helper, fallback] {
            let steps = vec![stage.clone(), privileged];
            let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
            let msg = r.expect_err("тема с кодом прошла");
            assert!(msg.contains("evil.so") || msg.contains("neon.plymouth"), "{msg}");
            assert_eq!(calls, 0, "окно пароля показано для отвергнутой темы");
            let _ = std::fs::remove_dir_all(&staged);
        }
        // A stock theme goes further, to the helper.
        std::fs::remove_file(src.join("evil.so")).unwrap();
        std::fs::write(src.join("neon.plymouth"), "[Plymouth Theme]\nModuleName=script\n").unwrap();
        let steps = vec![stage.clone(), ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() }];
        let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
        assert!(r.is_ok() && calls == 1);
        let _ = std::fs::remove_dir_all(&staged);
        let _ = std::fs::remove_dir_all(&src);
    }

    /// A password refusal is no longer swallowed: the installation gets an error with
    /// human text, not "installed".
    #[test]
    fn denied_password_reaches_the_message() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = scratch("denied");
        let steps = plan_with_system_part(&dir, "sddm");
        let (r, calls) = run(&steps, false, true, PrivOutcome::Failed { code: 126, stderr: String::new() });
        assert_eq!(calls, 1);
        let text = r.expect_err("отказ в пароле — ошибка установки");
        assert!(text.contains("administrator password was not confirmed"), "{text}");
        // All or nothing: the user part that went in before the password window
        // is removed by the rollback.
        assert!(!dir.join("user-part.txt").exists(), "отказ в пароле оставил пользовательскую часть");
        // The same — by the polkit agent's text with any code.
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed {
            code: 127,
            stderr: "Error executing command as another user: Not authorized\n".into(),
        });
        assert!(r.unwrap_err().contains("password"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A write error to the system partition (as on SteamOS or with a full
    /// disk) reaches the message verbatim — as the last line of the answer.
    #[test]
    fn write_error_in_usr_reaches_the_message() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let dir = scratch("readonly");
        let steps = plan_with_system_part(&dir, "plymouth");
        let stderr = "install: creating directory '/usr/share/plymouth/themes/x'\n\
                      cp: cannot create regular file '/usr/share/plymouth/themes/x/x.plymouth': \
                      Read-only file system\n\n";
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed { code: 1, stderr: stderr.into() });
        let text = r.unwrap_err();
        assert!(text.starts_with("Rexon could not change the system part of the theme: cp:"), "{text}");
        assert!(text.ends_with("Read-only file system"), "{text}");
        // A silent command — at least the code, but not emptiness.
        let (r, _) = run(&steps, false, true, PrivOutcome::Failed { code: 3, stderr: String::new() });
        assert!(r.unwrap_err().contains("error code 3"));
        // A timeout and a missing pkexec are errors too, not success.
        assert!(run(&steps, false, true, PrivOutcome::TimedOut).0.is_err());
        assert_eq!(
            run(&steps, false, true, PrivOutcome::NotStarted).0.unwrap_err(),
            crate::system_plan::pkexec_missing_message()
        );
        // Success is success.
        assert_eq!(run(&steps, false, true, PrivOutcome::Done).0, Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regular (non-system) plan commands still do not bring the installation down:
    /// failing to refresh the menu cache is no reason to roll back a theme.
    #[test]
    fn plain_command_failure_is_still_not_fatal() {
        let steps = vec![ExtStep::Run { argv: vec!["false".into()], timeout_secs: Some(5) }];
        let (r, calls) = run(&steps, false, true, PrivOutcome::Done);
        assert_eq!((r, calls), (Ok(()), 0));
    }

    /// SteamOS (Steam Deck and Steam Machine): an installation with a system part
    /// refuses BEFORE the first step and names the reason by what is being installed;
    /// pkexec is not called at all.
    #[test]
    fn steamos_refuses_system_part_before_any_step() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        use crate::distro_compat::SteamOsBlock;
        for (kind, block) in [
            ("sddm", SteamOsBlock::LoginScreen),
            ("gdm-bg", SteamOsBlock::LoginScreen),
            ("plymouth", SteamOsBlock::BootSplash),
            ("mate-layouts", SteamOsBlock::SystemPart),
        ] {
            let dir = scratch(&format!("steamos-{kind}"));
            let steps = plan_with_system_part(&dir, kind);
            let (r, calls) = run(&steps, true, true, PrivOutcome::Done);
            assert_eq!(r, Err(block.reason()), "{kind}");
            assert_eq!(calls, 0, "{kind}: pkexec на SteamOS не зовётся");
            assert!(!dir.join("user-part.txt").exists(), "{kind}: ни одного шага до отказа");
            let _ = std::fs::remove_dir_all(&dir);
        }
        // The wrapper script is recognized by its prefix.
        let script = ExtStep::PkexecScript { prefix: "rexon_plymouth".into(), script: String::new() };
        assert_eq!(steamos_block_of(&script), SteamOsBlock::BootSplash);
    }

    /// Uninstalling on SteamOS skips the system part and finishes its own part
    /// — otherwise the extension could not be removed at all.
    #[test]
    fn steamos_uninstall_skips_system_part() {
        let dir = scratch("steamos-uninstall");
        let steps = plan_with_system_part(&dir, "sddm");
        let (r, calls) = run(&steps, true, false, PrivOutcome::Failed { code: 1, stderr: "x".into() });
        assert_eq!((r, calls), (Ok(()), 0));
        assert!(dir.join("user-part.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The snapshot of an unpacked archive must go into the OBSERVATIONS layer: by
    /// the pre-layer the planner would decide that the tree existed before us, and
    /// `find_manifest_root` would look for the manifest in the wrong place.
    #[test]
    fn snap_tree_is_observations_only() {
        let value = snap_tree(Path::new("/nonexistent-rexon-snap"));
        assert_eq!(value["pre_dirs"].as_array().map(Vec::len), Some(0));
        assert_eq!(value["pre_files"].as_array().map(Vec::len), Some(0));
    }

    /// Install directories in the snapshot — BOTH sets: the one the planner builds
    /// from a bare `$HOME`, and the product's XDG-aware one.
    /// The divergence of these two sets already cost the «Appearance» tab an empty
    /// storefront (rule #12).
    #[test]
    fn target_dirs_cover_both_path_tables() {
        let dirs = target_dirs("/home/tester");
        let suffix = constants::install_path_suffix("theme-icons").expect("тип есть в матрице");
        let plain = PathBuf::from(format!("/home/tester/{suffix}"));
        assert!(dirs.contains(&plain), "нет пути планировщика: {plain:?}");
        assert!(dirs.iter().any(|d| d.ends_with(".themes")), "нет ~/.themes");
    }

    /// The executor's answers go into the tab's status line as is — in
    /// the English interface there is not a single Russian letter in them. The Russian is checked word
    /// for word by `installer_parity::ext_exec_errors_in_russian_as_before`.
    #[test]
    fn english_install_errors_have_no_russian() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let missing = install_from_file("/nonexistent/rexon-test.rxn").err().unwrap_or_default();
        assert_eq!(missing, "File not found: /nonexistent/rexon-test.rxn");
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_broken_rxn_").expect("каталог создан");
        let file = dir.join("broken.rxn");
        std::fs::write(&file, b"not a zip").expect("файл записан");
        let broken = install_from_file(&s(&file)).err().unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(broken, "The file is damaged or is not an .rxn archive");
        let texts = [
            missing,
            broken,
            privileged_failure_text(&PrivOutcome::NotStarted).unwrap_or_default(),
            privileged_failure_text(&PrivOutcome::TimedOut).unwrap_or_default(),
        ];
        for t in texts {
            assert!(!t.is_empty());
            assert!(!t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')), "{t}");
        }
    }

    /// A foreign format is no longer rejected by the name extension.
    ///
    /// A guard for the importers' move into the core: while the live `ImpIo` lived only in
    /// the bridge, `install_from_file` answered "I can only do .rxn" to any
    /// `.themepack`. The return of this branch would mean that the tab again
    /// deceives with its own file chooser filter — so what is checked is
    /// not "it worked" but "the answer is no longer about the name extension".
    #[test]
    fn foreign_format_is_no_longer_refused_by_extension() {
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_foreign_")
            .expect("каталог создан");
        let file = Path::new(&dir).join("тема.themepack");
        // Not a real CAB: it will not get as far as conversion, but there must no longer be
        // a refusal by the name extension.
        std::fs::write(&file, b"not a real themepack").expect("файл записан");
        let answer = install_from_file(&s(&file));
        let _ = std::fs::remove_dir_all(&dir);
        let text = answer.err().unwrap_or_default();
        assert!(
            !text.contains("умею ставить только .rxn"),
            "отказ по расширению имени вернулся: {text}"
        );
    }

    /// The installation message: an event file with the extension name — exactly the one
    /// the bridge listener reads — and cleaning up those older than 5 minutes.
    #[test]
    fn announce_installed_writes_event_and_sweeps_stale() {
        use crate::rexon_bridge::RexonBridge;
        let dir = mkdtemp(&std::env::temp_dir(), "rexon_test_bridge_").expect("каталог создан");
        let bridge = RexonBridge::new(dir.join("bridge.json"), dir.join("bridge_commands"));
        bridge.ensure_dirs().expect("каталоги моста");
        let stale = dir.join("bridge_commands/event_old_1.json");
        std::fs::write(&stale, b"{}").expect("старое событие");
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .and_then(|f| f.set_modified(old))
            .expect("mtime в прошлом");

        announce_installed(&bridge, "Часы", std::time::SystemTime::now()).expect("весть ушла");

        let names: Vec<String> = SnapBuilder::names_of(&dir.join("bridge_commands"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!names.contains(&"event_old_1.json".to_string()), "старьё не подметено: {names:?}");
        assert_eq!(names.len(), 1, "ровно одно событие: {names:?}");
        assert!(names[0].starts_with("event_extension_installed_"), "{names:?}");
    }

    /// The variable by which the child run learns its temporary directory.
    const E2E_BASE_ENV: &str = "REXON_EXT_BRIDGE_E2E_BASE";

    /// An end-to-end guard: installation sends the bridge `extension_installed` with the NAME,
    /// uninstalling and failure stay silent (like the original's `_install_rxn`), and the storefront
    /// the «Extensions» page rereads sees the installation and
    /// the uninstallation without a restart.
    ///
    /// ⚠️ The pipeline reads the process's `HOME`/`XDG_CONFIG_HOME`, and neighboring tests
    /// of this binary run in parallel. So the check itself runs in a CHILD
    /// process of the same binary with a substituted home — the live profile and the neighbors
    /// do not see it.
    #[test]
    fn install_announces_to_bridge_and_list_follows_without_restart() {
        let base = mkdtemp(&std::env::temp_dir(), "rexon_test_ext_bridge_").expect("каталог");
        let home = base.join("home");
        std::fs::create_dir_all(&home).expect("дом");
        let child = "ext_exec::tests::bridge_e2e_child";
        let out = Command::new(std::env::current_exe().expect("путь бинаря"))
            .args(["--exact", child, "--ignored", "--test-threads=1"])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env(E2E_BASE_ENV, &base)
            .output()
            .expect("дочерний прогон");
        let _ = std::fs::remove_dir_all(&base);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "дочерний прогон не прошёл:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn e2e_rxn(dir: &Path, id: &str, name: &str) -> PathBuf {
        use std::io::Write as _;
        let path = dir.join(format!("{id}.rxn"));
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&path).expect("архив"));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let manifest = serde_json::json!({"id": id, "name": name, "type": "plugin", "version": "1.0"});
        for (entry, data) in [
            ("manifest.json", manifest.to_string()),
            ("install.sh", "exit 0\n".to_string()),
            ("uninstall.sh", "exit 0\n".to_string()),
        ] {
            zw.start_file(entry, opts).expect("запись");
            zw.write_all(data.as_bytes()).expect("запись");
        }
        zw.finish().expect("архив закрыт");
        path
    }

    fn e2e_events(_home: &Path) -> Vec<(String, Value)> {
        // We ask the core for the directory rather than gluing a literal: the guard must
        // read EXACTLY the folder the product writes to (including with
        // `$XDG_CONFIG_HOME` set in the fake environment).
        let dir = crate::rexon_bridge::default_commands_dir();
        let mut out: Vec<(String, Value)> = SnapBuilder::names_of(&dir)
            .into_iter()
            .map(|n| {
                let text = std::fs::read_to_string(dir.join(&n)).unwrap_or_default();
                (n, serde_json::from_str(&text).unwrap_or(Value::Null))
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// The body of the end-to-end guard — only in the child process (see above).
    #[test]
    #[ignore = "запускается дочерним процессом install_announces_to_bridge_and_list_follows_without_restart"]
    fn bridge_e2e_child() {
        let base = PathBuf::from(std::env::var_os(E2E_BASE_ENV).expect("только дочерний прогон"));
        let home = PathBuf::from(home());
        assert!(home.starts_with(&base), "дом не подменён: {home:?}");

        let rxn = e2e_rxn(&base, "rexon-test-clock", "Тестовые часы");
        let done = install_from_file(&s(&rxn)).expect("установка прошла");
        assert_eq!(done.name, "Тестовые часы");
        let events = e2e_events(&home);
        assert_eq!(events.len(), 1, "ровно одно событие: {events:?}");
        let (file, msg) = &events[0];
        assert!(file.starts_with("event_extension_installed_"), "{file}");
        assert_eq!(msg["target"], "*");
        assert_eq!(msg["command"], "event");
        assert_eq!(msg["params"]["type"], "extension_installed");
        assert_eq!(msg["params"]["ext_name"], "Тестовые часы");
        let listed = |id: &str| qt_extensions::installed().iter().any(|v| v.ext_id == id);
        assert!(listed("rexon-test-clock"), "витрина не видит установленное");

        // A repeat ("already installed") and a broken archive — a refusal without a message.
        assert!(install_from_file(&s(&rxn)).is_err());
        let broken = base.join("broken.rxn");
        std::fs::write(&broken, b"not a zip").expect("файл");
        assert!(install_from_file(&s(&broken)).is_err());
        assert_eq!(e2e_events(&home).len(), 1, "неудача не шлёт весть");

        // Uninstalling: the storefront empties, no message — the original has none.
        uninstall("rexon-test-clock").expect("удаление прошло");
        assert!(!listed("rexon-test-clock"), "витрина видит удалённое");
        assert_eq!(e2e_events(&home).len(), 1, "удаление не шлёт весть");
    }

    /// The plan's temporary directories are unique: two plans in a row have no right to
    /// share a staged directory — the second would erase the first one's files.
    #[test]
    fn mkdtemp_gives_unique_dirs() {
        let base = std::env::temp_dir();
        let a = mkdtemp(&base, "rexon_test_").expect("каталог создан");
        let b = mkdtemp(&base, "rexon_test_").expect("каталог создан");
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    // ── The window's privilege session (B9) ──────────────────────────────

    /// Steps of the layout and system theme helpers are recognized as an operation, and its
    /// "as now" path is exactly the same argv; the rest — the old way.
    #[test]
    fn step_priv_op_recognizes_only_exact_helper_argv() {
        use crate::privilege::op::{PrivOp, SystemThemeKind, SystemThemeOp, ThemeFamily};
        let run = |argv: Vec<String>| ExtStep::Run { argv, timeout_secs: Some(60) };
        // The layout (the login screen background) — Stage with the layout directory.
        let stage = crate::system_plan::privileged_stage_cmd("gdm-bg", "rexon-login", "/tmp/st", true, Some("/tmp/st"), true)
            .unwrap();
        let (op, files) = step_priv_op(&run(stage.clone())).expect("раскладка");
        assert!(matches!(&op, PrivOp::Stage { category, name, source } if category == "gdm-bg" && name == "rexon-login" && source.label == "/tmp/st"));
        assert_eq!(files, vec!["/tmp/st".to_string()]);
        // System themes: installing, «Apply», «Remove».
        let helper = crate::installer::system_helper_path();
        let lang = crate::system_l10n::lang_args(helper);
        let mut install = vec!["pkexec".to_string(), helper.to_string()];
        install.extend(lang.clone());
        install.extend(["plymouth", "neon", "/tmp/th", "debian"].map(String::from));
        let (op, files) = step_priv_op(&run(install.clone())).expect("установка темы");
        assert!(matches!(
            &op,
            PrivOp::SystemTheme(SystemThemeOp::Install { kind: SystemThemeKind::Plymouth, theme, family: ThemeFamily::Debian, .. })
                if theme == "neon"
        ));
        assert_eq!(files, vec!["/tmp/th".to_string()]);
        let apply = crate::de_apply::system_apply_argv(helper, &lang, "sddm", "sugar", "arch");
        assert!(matches!(
            step_priv_op(&run(apply)),
            Some((PrivOp::SystemTheme(SystemThemeOp::Apply { kind: SystemThemeKind::Sddm, .. }), f)) if f.is_empty()
        ));
        let remove = crate::de_apply::system_remove_argv(helper, &lang, "plymouth", "neon", "arch");
        assert!(matches!(
            step_priv_op(&run(remove)),
            Some((PrivOp::SystemTheme(SystemThemeOp::Remove { kind: SystemThemeKind::Plymouth, .. }), _))
        ));
        // Not recognized: a raw script, an extra word, a foreign helper, a foreign
        // family, not pkexec, the wrapper script.
        let mut extra = stage.clone();
        extra.push("--force".into());
        let mut other = install.clone();
        other[1] = "/tmp/evil-helper".into();
        let mut fam = install.clone();
        *fam.last_mut().unwrap() = "fedora".into();
        for argv in [
            vec!["pkexec".to_string(), "bash".into(), "/tmp/x.sh".into()],
            extra,
            other,
            fam,
            stage[1..].to_vec(),
        ] {
            assert_eq!(step_priv_op(&run(argv.clone())), None, "{argv:?}");
        }
        let script = ExtStep::PkexecScript { prefix: "rexon_sddm".into(), script: "true".into() };
        assert_eq!(step_priv_op(&script), None);
    }

    /// ⛔ Guard B9: nobody except the privilege session (`privilege/`) starts
    /// `pkexec` with its own `Command::new` — everyone else calls the shared executor
    /// or `privilege::run`. Texts and argv literals (`"pkexec"` in a vector)
    /// are not touched by the guard.
    #[test]
    fn no_raw_pkexec_spawn_outside_privilege() {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let rust = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for krate in std::fs::read_dir(&rust).unwrap().flatten() {
            let src = krate.path().join("src");
            if src.is_dir() {
                walk(&src, &mut files);
            }
        }
        assert!(files.len() > 50, "исходники не найдены: {}", rust.display());
        let mut bad = Vec::new();
        for f in files {
            let shown = f.to_string_lossy().replace('\\', "/");
            if shown.contains("/src/privilege/") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            let flat: String = text.split_whitespace().collect();
            for needle in ["Command::new(\"pkexec\")", "Command::new(\"/usr/bin/pkexec\")"] {
                if flat.contains(needle) {
                    bad.push(format!("{shown}: {needle}"));
                }
            }
        }
        assert!(bad.is_empty(), "pkexec запускается мимо сеанса прав: {bad:#?}");
    }


    // ── Cursors from Windows: converting again only for a fix ────────────

    /// Put a theme «from Windows» into a fake extensions directory the way
    /// installation puts it: an unpacked `.rxn` under a directory with the package id.
    fn put_windows_theme(root: &Path, ext_dir: &Path, name: &str, files: &[(&str, Vec<u8>)]) -> PathBuf {
        let set = root.join(format!("set-{name}"));
        std::fs::create_dir_all(&set).unwrap();
        for (f, b) in files {
            std::fs::write(set.join(f), b).unwrap();
        }
        let out = root.join(format!("rxn-{name}"));
        std::fs::create_dir_all(&out).unwrap();
        let mut io = crate::imp_live::LiveImpIo::new(None);
        let rxn = imp::cursor_import_to_rxn(&mut io, &s(&set), name, Some(&s(&out))).unwrap();
        let tmp = root.join(format!("unpacked-{name}"));
        ext_utils::safe_zip_extract(Path::new(&rxn), &tmp).unwrap();
        let m: Value = serde_json::from_str(&std::fs::read_to_string(tmp.join("manifest.json")).unwrap()).unwrap();
        let dest = ext_dir.join(m["id"].as_str().unwrap());
        std::fs::rename(&tmp, &dest).unwrap();
        dest
    }

    fn converter_of(dir: &Path) -> u64 {
        let m: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
        m["windows_cursors"]["converter"].as_u64().unwrap()
    }

    #[test]
    fn windows_cursors_are_converted_again_only_where_the_fix_applies() {
        use crate::wincursor::testbytes::{ani, cur32};
        let root = scratch("wincur-again");
        let ext_dir = root.join("extensions");
        std::fs::create_dir_all(&ext_dir).unwrap();
        let busy = ani(&[cur32(32, 1, 1, 10), cur32(32, 2, 2, 20)], 10, None, None, 2);
        let anim = put_windows_theme(&root, &ext_dir, "Anim", &[("busy.ani", busy.clone()), ("pointer.cur", cur32(32, 0, 0, 1))]);
        let still = put_windows_theme(&root, &ext_dir, "Still", &[("pointer.cur", cur32(32, 0, 0, 1))]);
        let changed = put_windows_theme(&root, &ext_dir, "Changed", &[("busy.ani", busy.clone())]);
        let bare = put_windows_theme(&root, &ext_dir, "Bare", &[("busy.ani", busy)]);
        // An old-style theme — without originals: a skip without noise.
        std::fs::remove_dir_all(bare.join("originals")).unwrap();
        // "The previous converter" gave different bytes for one theme.
        let wait = changed.join("data/changed/cursors/wait");
        assert!(wait.is_file(), "нет {wait:?}");
        std::fs::write(&wait, b"old output").unwrap();
        let anim_data = std::fs::read(anim.join("data/anim/cursors/wait")).unwrap();

        // A fake: version 3 fixes only animated ones (the real one is 2).
        let changes = [(2, imp::CursorFixScope::All), (3, imp::CursorFixScope::Animated)];
        let mut calls: Vec<String> = Vec::new();
        let mut reinstall = |id: &str, fresh: &Path| -> Result<(), String> {
            calls.push(id.to_string());
            let dest = ext_dir.join(id);
            std::fs::remove_dir_all(&dest).unwrap();
            let mut undo = crate::install_plan::Undo::new();
            crate::install_plan::copy_tree(fresh, &dest, &mut undo).map_err(|e| e.to_string())?;
            undo.finish(true);
            Ok(())
        };
        let report = refresh_windows_cursors_in(&ext_dir, 3, &changes, &mut reinstall);
        assert_eq!(report.errors, Vec::<String>::new());
        assert_eq!(report.replaced, vec![("Changed".to_string(), "changed".to_string())]);
        assert_eq!(report.version_only, vec!["wincur-anim".to_string()]);
        assert_eq!(calls, vec!["wincur-changed".to_string()]);
        // Byte for byte the same — the files are untouched, only the version.
        assert_eq!(std::fs::read(anim.join("data/anim/cursors/wait")).unwrap(), anim_data);
        assert_eq!(converter_of(&anim), 3);
        // The replaced theme — new bytes and a new version.
        assert_ne!(std::fs::read(&wait).unwrap(), b"old output");
        assert_eq!(converter_of(&changed), 3);
        // The fix does not concern a static one, without originals — skipped.
        assert_eq!(converter_of(&still), 2);
        assert_eq!(converter_of(&bare), 2);

        // The second pass: everything is already on the current version — no replacement, no noise.
        let again = refresh_windows_cursors_in(&ext_dir, 3, &changes, &mut |_, _| panic!("второй раз не переводится"));
        assert_eq!(again, CursorRefresh::default());
        // With the real table (version 2) nothing is touched.
        let real = refresh_windows_cursors_in(
            &ext_dir,
            imp::CURSOR_CONVERTER_VERSION,
            imp::CURSOR_CONVERTER_CHANGES,
            &mut |_, _| panic!("при версии 2 переводить нечего"),
        );
        assert_eq!(real, CursorRefresh::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn same_tree_compares_names_bytes_and_link_targets() {
        let root = scratch("same-tree");
        for side in ["a", "b"] {
            let d = root.join(side).join("cursors");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("default"), b"xcur").unwrap();
            std::os::unix::fs::symlink("default", d.join("left_ptr")).unwrap();
        }
        assert!(same_tree(&root.join("a"), &root.join("b")));
        std::fs::write(root.join("b/cursors/default"), b"xcuR").unwrap();
        assert!(!same_tree(&root.join("a"), &root.join("b")));
        std::fs::write(root.join("b/cursors/default"), b"xcur").unwrap();
        std::fs::remove_file(root.join("b/cursors/left_ptr")).unwrap();
        std::os::unix::fs::symlink("other", root.join("b/cursors/left_ptr")).unwrap();
        assert!(!same_tree(&root.join("a"), &root.join("b")));
        assert!(!same_tree(&root.join("a"), &root.join("missing")));
        let _ = std::fs::remove_dir_all(&root);
    }

    // ── The second copy of a regular Conky widget ──────────────────────

    /// A machine for the cleanup pass: the files are real (in a temporary folder),
    /// there are no conky processes, commands are not run but recorded.
    struct NoProcIo {
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl crate::animations::LookIo for NoProcIo {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            false
        }
        fn read(&self, path: &str) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            let p = Path::new(path);
            p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(p, text).is_ok()
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            if path == "/proc" {
                return Vec::new();
            }
            std::fs::read_dir(path)
                .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                .unwrap_or_default()
        }
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn conky_ctx(home: &Path) -> crate::de_apply::Ctx {
        crate::de_apply::Ctx { home: s(home), xdg_config_home: None, wayland: false, de: String::new() }
    }

    /// The package `my-theme` with subfolders `data/<name>` (each has `conky.conf`
    /// and `lua/rings.lua`).
    fn conky_package(exts: &Path, subs: &[&str]) -> PathBuf {
        let pkg = exts.join("my-theme");
        put(&pkg.join("manifest.json"), r#"{"id": "my-theme", "name": "My Theme", "type": "theme-conky", "version": "1.0"}"#);
        for sub in subs {
            put(&pkg.join(format!("data/{sub}/conky.conf")), &format!("conky.text = [[{sub}]]\n"));
            put(&pkg.join(format!("data/{sub}/lua/rings.lua")), "function conky_main() end\n");
        }
        pkg
    }

    /// A copy of a package subfolder in `dest` (as the old Rexon put it), with the mark
    /// `mark` (empty — without a mark).
    fn copy_sub(pkg: &Path, sub: &str, dest: &Path, mark: &str) {
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data").join(sub), dest, &mut undo).unwrap();
        if !mark.is_empty() {
            std::fs::write(dest.join(ext_manager::OWNER_MARK), format!("{mark}\n")).unwrap();
        }
    }

    /// The cleanup removes only our own untouched second copy: a copy without
    /// a mark (the person's folder), with another package's mark, modified and with
    /// an extra file — it leaves; it does not touch the widget's main directory;
    /// the second copy's autostart moves to the main one.
    #[test]
    fn conky_heal_removes_only_own_untouched_duplicate() {
        let home = scratch("conky-heal");
        let exts = home.join(".config/rexon/extensions");
        let subs = ["MyTheme", "Human", "Other", "Edited", "Extra"];
        let pkg = conky_package(&exts, &subs);
        let root = home.join(".config/conky");
        let main = root.join("my-theme");
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &main, &mut undo).unwrap();
        copy_sub(&pkg, "MyTheme", &root.join("MyTheme"), "my-theme");
        copy_sub(&pkg, "Human", &root.join("Human"), "");
        copy_sub(&pkg, "Other", &root.join("Other"), "other-pack");
        copy_sub(&pkg, "Edited", &root.join("Edited"), "my-theme");
        put(&root.join("Edited/conky.conf"), "conky.text = [[mine]]\n");
        copy_sub(&pkg, "Extra", &root.join("Extra"), "my-theme");
        put(&root.join("Extra/notes.txt"), "человек\n");
        let ctx = conky_ctx(&home);
        let old_autostart = crate::de_apply::conky_autostart_path(&ctx, "MyTheme");
        put(Path::new(&old_autostart), "[Desktop Entry]\nExec=conky -c x\n");

        let io = NoProcIo { ran: Default::default() };
        let removed = heal_conky_duplicates(&io, &ctx, &exts);
        assert_eq!(removed, vec![s(&root.join("MyTheme"))]);
        assert!(!root.join("MyTheme").exists());
        for keep in ["Human", "Other", "Edited", "Extra"] {
            assert!(root.join(keep).join("conky.conf").is_file(), "{keep} снят");
        }
        assert_eq!(std::fs::read_to_string(root.join("Edited/conky.conf")).unwrap(), "conky.text = [[mine]]\n");
        for sub in subs {
            assert!(main.join(sub).join("conky.conf").is_file(), "основной каталог потерял {sub}");
        }
        assert!(!Path::new(&old_autostart).exists(), "автозапуск второй копии остался");
        // A package of several widgets: the autostart is at the nested `my-theme/MyTheme`.
        let new_autostart = std::fs::read_to_string(crate::de_apply::conky_autostart_path(&ctx, "my-theme/MyTheme")).unwrap();
        assert!(new_autostart.contains("/my-theme/MyTheme/conky.conf"), "{new_autostart}");
        // conky is not started; a version probe (`conky -v`, the XWayland launch
        // file) is not a launch.
        assert!(io.ran.borrow().iter().all(|a| a.first().map(String::as_str) != Some("conky") || a[1..] == ["-v"]));

        // A repeat does nothing.
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A copy whose paths were already fixed by the repair before launch
    /// (`de_apply::conky_repair_paths`) is still our own; in a widget set
    /// the subfolders are the widgets themselves, the cleanup does not touch them; without the main
    /// directory the second copy stays.
    #[test]
    fn conky_heal_repaired_copy_sets_and_missing_main() {
        let home = scratch("conky-heal-2");
        let exts = home.join(".config/rexon/extensions");
        let pkg = conky_package(&exts, &["MyTheme"]);
        put(&pkg.join("data/MyTheme/conky.conf"), "lua_load = '~/.conky/MyTheme/lua/rings.lua'\n");
        let root = home.join(".config/conky");
        let dup = root.join("MyTheme");
        copy_sub(&pkg, "MyTheme", &dup, "my-theme");
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };

        // There is no main directory — the copy is the only one, it stays.
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        assert!(dup.is_dir());

        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &root.join("my-theme"), &mut undo).unwrap();
        let changed = crate::de_apply::conky_repair_paths(&ctx.home, &s(&dup));
        assert_eq!(changed.len(), 1, "починка ничего не поправила");

        // A widget set: the subfolders are its widgets.
        let manifest = pkg.join("manifest.json");
        put(&manifest, r#"{"id": "my-theme", "type": "theme-conky", "conky_set": {"widgets": []}}"#);
        assert!(heal_conky_duplicates(&io, &ctx, &exts).is_empty());
        assert!(dup.is_dir());

        put(&manifest, r#"{"id": "my-theme", "type": "theme-conky"}"#);
        assert_eq!(heal_conky_duplicates(&io, &ctx, &exts), vec![s(&dup)]);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A regular package with two nested widgets (`data/A`, `data/B`,
    /// no settings file of its own in `data/`): the files — as one directory
    /// `<root>/my-theme`, but there are two widgets — two tiles `my-theme/A` and
    /// `my-theme/B`, each with its own settings file, autostart, removal;
    /// the cleanup moves the autostart of the old copy `<root>/A` to `my-theme/A`.
    #[test]
    fn conky_package_with_two_widgets_is_two_tiles() {
        use crate::de_apply as da;
        let home = scratch("conky-two");
        let exts = home.join(".config/rexon/extensions");
        let pkg = conky_package(&exts, &["A", "B"]);
        put(&pkg.join("data/fonts/x.ttf"), "font");
        let root = home.join(".config/conky");
        let main = root.join("my-theme");
        let mut undo = crate::install_plan::Undo::off();
        crate::install_plan::copy_tree(&pkg.join("data"), &main, &mut undo).unwrap();
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };

        assert_eq!(da::conky_nested_widgets(&io, &ctx, "my-theme"), vec!["my-theme/A", "my-theme/B"]);
        let top = crate::models::ThemeItem {
            id: "local:conky:my-theme".into(),
            name: "my-theme".into(),
            install_path: s(&main),
            ..Default::default()
        };
        let tiles = crate::conky_scan::split_package_items(&io, &ctx, vec![top], &["my-theme/B".to_string()]);
        let ids: Vec<&str> = tiles.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["local:conky:my-theme/A", "local:conky:my-theme/B"]);
        assert_eq!(tiles[0].name, "A");
        assert_eq!(tiles[0].install_path, s(&main.join("A")));
        assert_eq!((tiles[0].local_kind.as_deref(), tiles[1].local_kind.as_deref()), (None, Some("active")));

        let conf_a = s(&main.join("A/conky.conf"));
        assert_eq!(da::conky_conf(&io, &ctx, "my-theme/A").as_deref(), Some(conf_a.as_str()));
        assert_eq!(da::conky_widget_of_conf(&io, &ctx, &conf_a).as_deref(), Some("my-theme/A"));
        assert_eq!(da::conky_widget_of_conf(&io, &ctx, &s(&main.join("B/lua/rings.lua"))).as_deref(), Some("my-theme/B"));
        assert_ne!(da::conky_autostart_path(&ctx, "my-theme/A"), da::conky_autostart_path(&ctx, "my-theme/B"));

        // The old second copy `<root>/A` with an autostart — goes away, the autostart
        // moves to `my-theme/A`.
        copy_sub(&pkg, "A", &root.join("A"), "my-theme");
        let old = da::conky_autostart_path(&ctx, "A");
        put(Path::new(&old), "[Desktop Entry]\nExec=conky -c x\n");
        assert_eq!(heal_conky_duplicates(&io, &ctx, &exts), vec![s(&root.join("A"))]);
        assert!(!Path::new(&old).exists());
        assert!(da::conky_autostart(&io, &ctx, "my-theme/A"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/B"));
        let entry = std::fs::read_to_string(da::conky_autostart_path(&ctx, "my-theme/A")).unwrap();
        assert!(entry.contains(&conf_a), "{entry}");

        // B has its own autostart, then «Remove» of tile A: only A is removed.
        assert!(da::set_conky_autostart(&io, &ctx, "my-theme/B", true).0);
        let roots = crate::theme_removal::RemovalRoots::new(&s(&home), "/usr");
        let a_dir = s(&main.join("A"));
        assert_eq!(
            roots.verdict(&a_dir, false, &Map::new(), &std::collections::HashSet::new()),
            crate::theme_removal::Verdict::Local { path: a_dir.clone() }
        );
        assert_eq!(crate::conky_scan::own_widget_name(&ctx, &a_dir).as_deref(), Some("my-theme/A"));
        std::fs::remove_dir_all(&a_dir).unwrap();
        assert_eq!(da::conky_forget_widget(&io, &ctx, &a_dir).as_deref(), Some("my-theme/A"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/A"));
        assert!(da::conky_autostart(&io, &ctx, "my-theme/B"));
        let left = crate::conky_scan::split_package_items(
            &io,
            &ctx,
            vec![crate::models::ThemeItem { id: "local:conky:my-theme".into(), ..Default::default() }],
            &[],
        );
        assert_eq!(left.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["local:conky:my-theme/B"]);

        // Removing the package removes the nested widget's autostart too.
        assert_eq!(da::conky_forget_widget(&io, &ctx, &s(&main)).as_deref(), Some("my-theme"));
        assert!(!da::conky_autostart(&io, &ctx, "my-theme/B"));
        assert!(!root.join("A").exists() && !root.join("B").exists(), "копии в корне Conky");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A package with a settings file right in `data/` (and subfolders next to it) is one
    /// widget `<id>`; with a single subfolder widget — also one.
    #[test]
    fn conky_package_with_own_conf_is_one_tile() {
        let home = scratch("conky-one");
        let exts = home.join(".config/rexon/extensions");
        let ctx = conky_ctx(&home);
        let io = NoProcIo { ran: Default::default() };
        let pkg = conky_package(&exts, &["A", "B"]);
        put(&pkg.join("data/conky.conf"), "conky.text = [[top]]\n");
        assert!(crate::de_apply::conky_nested_widgets(&io, &ctx, "my-theme").is_empty());
        std::fs::remove_dir_all(&pkg).unwrap();
        conky_package(&exts, &["A"]);
        assert!(crate::de_apply::conky_nested_widgets(&io, &ctx, "my-theme").is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }
}
