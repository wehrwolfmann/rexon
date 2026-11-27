//! ExtensionManager — port of `rexon/extensions/manager.py` (the pure part).
//!
//! What is ported is the extension manager's LOGIC without execution:
//!   * `is_safe_sys_name` — an allowlist of names for system paths under root;
//!   * `find_manifest_root` + `install_rxn_check` — the whole validation
//!     pipeline of `_install_rxn` BEFORE copying (a v4 manifest with the id derived from
//!     the title, path-traversal checks of the id, v2 migration, VALID_TYPES,
//!     install.sh/uninstall.sh being mandatory, `_check_rexon_compat`,
//!     "already installed") — error texts byte for byte with Python;
//!   * `safe_install_plan` / `safe_uninstall_plan` — installing/uninstalling
//!     appearance types as a PURE PLAN of steps (`ExtStep`) modeled on
//!     `system_plan.rs`: copies, symlinks, file writes (the texts of
//!     generated configs one to one), commands with timeouts, pkexec scripts
//!     (texts one to one), staging under /tmp. The parity reference is the RECORDED
//!     side calls (shutil/os/subprocess) of live Python on the same
//!     inputs (see `ext_manager_parity.rs`);
//!   * `detect_display_manager_plan` — the DM choice policy (the probes themselves are
//!     plan steps, their result is passed as a closure);
//!   * `find_background` — finding the background in data/ (from a snapshot with sizes);
//!   * `discover` — scanning the extensions directory (the installed tree);
//!   * `stats` / `save_state_text` / `load_state` — catalog structures and
//!     the on/off state (the JSON text byte for byte with `json.dump(indent=2)`).
//!
//! The FILE SYSTEM is BEHIND AN INTERFACE (as in installed_scan.rs): the input is
//! `FsSnap`, a tree snapshot "as the Python reference saw it" (name order =
//! os.scandir/os.listdir order, including directories Python read
//! AFTER its own copying — the snapshot keeps what the original saw).
//! On top of the snapshot — `PlanFs`: an overlay of paths CREATED by the plan itself
//! (copytree/makedirs/write), so that the exists/isdir checks inside the plan
//! see its own steps.
//!
//! NOT ported (execution — stays in Python): threading.Lock, the actual
//! copying/deleting, subprocess/pkexec/notify-send/pkill and reading their
//! output, tempfile (staged/tmp paths are issued deterministically by `TmpAlloc` —
//! the executor substitutes real ones), importers of Windows formats
//! (`importers.py`), running install.sh/uninstall.sh and `.desktop` installation
//! (`extension.py`, execution), loading/unloading plugins (importlib),
//! `_detect_display_manager` as a process (systemctl/which — probes in the plan,
//! answers injected), `_staged_under_tmp` as copying (the `Stage` step).

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::constants::{self, is_supported_font};
use crate::de_customization;
use crate::extension::Extension;
use crate::i18n::{tr, trf};
use crate::manifest_v4::validate_v4_manifest;
use crate::system_plan::privileged_stage_cmd;

// ============================================================================
// FsSnap — a file system snapshot (with file sizes and symlinks)
// ============================================================================

/// A snapshot file: the size (for `_find_background`) and the text (None — binary).
struct FileEntry {
    size: u64,
    text: Option<String>,
}

/// A tree snapshot in TWO layers:
///   * pre — the state BEFORE the run (the plan's decisions are made by it:
///     exists/isdir "did it exist before us");
///   * observed (whole dirs/files) — plus what Python read AFTER
///     its own copying (listings of dest directories, unpacked
///     .plasmoid) — the plan reads the contents of its own steps from it.
///
/// A directory → names in the reference's os.scandir order; a file → (size, text);
/// links — the pre-state's symlink paths (for lexists/islink).
pub struct FsSnap {
    dirs: HashMap<String, Vec<String>>,
    files: HashMap<String, FileEntry>,
    links: HashSet<String>,
    pre_dirs: HashSet<String>,
    pre_files: HashSet<String>,
    /// Single files the shared log recorded for THIS package
    /// (the snapshot's `owned_files`): installing them replaces, removal takes them away.
    owned_files: HashSet<String>,
    /// `(package file, file in place)` pairs that match byte for byte
    /// (the snapshot's `same_files`): such a file in place counts as our own.
    same_files: HashSet<(String, String)>,
}

impl FsSnap {
    /// From a JSON fixture `{"dirs": {p: [names]}, "files": {p: {"size": n,
    /// "text": str|null}}, "links": [p], "pre_dirs": [p], "pre_files": [p],
    /// "owned_files": [p], "same_files": [[src, dest]]}`.
    /// Without the pre lists the whole snapshot counts as the pre-state.
    pub fn from_json(v: &Value) -> Self {
        let mut dirs = HashMap::new();
        if let Some(obj) = v["dirs"].as_object() {
            for (k, names) in obj {
                let list = names
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|n| n.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                dirs.insert(k.clone(), list);
            }
        }
        let mut files = HashMap::new();
        if let Some(obj) = v["files"].as_object() {
            for (k, meta) in obj {
                files.insert(
                    k.clone(),
                    FileEntry {
                        size: meta["size"].as_u64().unwrap_or(0),
                        text: meta["text"].as_str().map(String::from),
                    },
                );
            }
        }
        let str_set = |key: &str| -> Option<HashSet<String>> {
            v[key].as_array().map(|a| {
                a.iter()
                    .filter_map(|n| n.as_str().map(String::from))
                    .collect()
            })
        };
        let links = str_set("links").unwrap_or_default();
        let pre_dirs = str_set("pre_dirs")
            .unwrap_or_else(|| dirs.keys().cloned().collect());
        let pre_files = str_set("pre_files")
            .unwrap_or_else(|| files.keys().cloned().collect());
        let owned_files = str_set("owned_files").unwrap_or_default();
        let same_files = v["same_files"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|pair| {
                        Some((pair[0].as_str()?.to_string(), pair[1].as_str()?.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        FsSnap { dirs, files, links, pre_dirs, pre_files, owned_files, same_files }
    }

    /// The file `dest` was recorded by the shared log for this package.
    pub fn owns_file(&self, dest: &str) -> bool {
        self.owned_files.contains(dest)
    }

    /// The file in place `dest` is byte for byte the package file `src`: a pair from the snapshot
    /// or (in fixtures with texts) identical texts of the same size.
    pub fn same_bytes(&self, src: &str, dest: &str) -> bool {
        if self.same_files.contains(&(src.to_string(), dest.to_string())) {
            return true;
        }
        match (self.files.get(src), self.files.get(dest)) {
            (Some(a), Some(b)) => a.size == b.size && a.text.is_some() && a.text == b.text,
            _ => false,
        }
    }

    /// The directory existed BEFORE the run (`os.path.isdir` in the plan's decisions).
    pub fn is_dir(&self, path: &str) -> bool {
        self.pre_dirs.contains(path)
    }

    /// The file existed BEFORE the run (`os.path.isfile` in the plan's decisions).
    pub fn is_file(&self, path: &str) -> bool {
        self.pre_files.contains(path)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.is_dir(path) || self.is_file(path)
    }

    /// `os.path.lexists` — the path itself exists, including broken symlinks.
    pub fn lexists(&self, path: &str) -> bool {
        self.exists(path) || self.links.contains(path)
    }

    /// The directory is in the observations (including those created by the run itself —
    /// for example, dest after copytree).
    pub fn obs_is_dir(&self, path: &str) -> bool {
        self.dirs.contains_key(path)
    }

    /// The file is in the observations.
    pub fn obs_is_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    pub fn is_link(&self, path: &str) -> bool {
        self.links.contains(path)
    }

    pub fn read_text(&self, path: &str) -> Option<&str> {
        self.files.get(path).and_then(|f| f.text.as_deref())
    }

    pub fn size(&self, path: &str) -> u64 {
        self.files.get(path).map(|f| f.size).unwrap_or(0)
    }

    /// Directory entry names in snapshot order ([] — no directory).
    pub fn scandir(&self, path: &str) -> &[String] {
        self.dirs.get(path).map(Vec::as_slice).unwrap_or(&[])
    }

    /// An emulation of `os.walk` (topdown): (root, subdirectories, files), classes —
    /// in the snapshot's listing order.
    pub fn walk(&self, root: &str) -> Vec<(String, Vec<String>, Vec<String>)> {
        let mut out = Vec::new();
        self.walk_into(root, &mut out);
        out
    }

    fn walk_into(&self, root: &str, out: &mut Vec<(String, Vec<String>, Vec<String>)>) {
        if !self.obs_is_dir(root) {
            return;
        }
        let mut subdirs = Vec::new();
        let mut files = Vec::new();
        for name in self.scandir(root) {
            if self.obs_is_dir(&join(root, name)) {
                subdirs.push(name.clone());
            } else {
                files.push(name.clone());
            }
        }
        out.push((root.to_string(), subdirs.clone(), files));
        for d in subdirs {
            self.walk_into(&join(root, &d), out);
        }
    }
}

fn join(dir: &str, rel: &str) -> String {
    format!("{dir}/{rel}")
}

/// An overlay on top of the snapshot: paths the plan itself CREATES (copytree/makedirs/
/// write) are visible to later exists/isdir checks — as Python saw them,
/// actually performing the copies during `_safe_install`.
struct PlanFs<'a> {
    snap: &'a FsSnap,
    made_dirs: HashSet<String>,
    made_files: HashSet<String>,
    /// How many main parts the plan skipped: something foreign is in place (the notes
    /// [`taken_by_other_notice`]/[`taken_file_notice`]). Everything skipped —
    /// the installation refuses entirely ([`ExtStep::Refuse`]).
    skipped: usize,
}

impl<'a> PlanFs<'a> {
    fn new(snap: &'a FsSnap) -> Self {
        PlanFs { snap, made_dirs: HashSet::new(), made_files: HashSet::new(), skipped: 0 }
    }

    fn mk_dir(&mut self, path: &str) {
        self.made_dirs.insert(path.to_string());
    }

    fn mk_file(&mut self, path: &str) {
        self.made_files.insert(path.to_string());
    }

    fn is_dir(&self, path: &str) -> bool {
        self.snap.is_dir(path) || self.made_dirs.contains(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.snap.exists(path)
            || self.made_dirs.contains(path)
            || self.made_files.contains(path)
    }
}

// ============================================================================
// Plan steps
// ============================================================================

/// A step of an extension install/uninstall plan — a side call Python
/// would make (`shutil`/`os`/`subprocess`). Execution — when hooked up to
/// live Python.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op")]
pub enum ExtStep {
    /// `os.makedirs(path, exist_ok=True)`.
    MakeDirs { path: String },
    /// `shutil.copytree(src, dest)`.
    CopyTree { src: String, dest: String },
    /// `shutil.copy2(src, dest)` (dest may be a directory — as in Python).
    CopyFile { src: String, dest: String },
    /// Deleting a tree: `safe` — via `_safe_rmtree` ($HOME boundary checks),
    /// otherwise `shutil.rmtree` (`ignore_errors` — as in the original's call).
    RemoveTree { path: String, safe: bool, ignore_errors: bool },
    /// `os.remove(path)`.
    RemoveFile { path: String },
    /// `os.symlink(target, link)`.
    Symlink { target: String, link: String },
    /// Writing a file with the given contents (texts one to one with Python).
    WriteFile { path: String, content: String },
    /// Unpacking a ZIP via `_safe_zip_extract` (e.g. `.plasmoid`).
    ExtractZip { archive: String, dest: String },
    /// `_staged_under_tmp`: copy `sources` into a staged directory under /tmp
    /// (directories — by contents, files — as is).
    Stage { staged: String, sources: Vec<String> },
    /// `_run_pkexec_script`: write the script into a temporary file
    /// `rexon_{prefix}_*.sh` and run `pkexec bash <file>` (timeout 60).
    PkexecScript { prefix: String, script: String },
    /// `subprocess.run(argv, capture_output=True, timeout=…)`.
    Run { argv: Vec<String>, timeout_secs: Option<u64> },
    /// `subprocess.Popen(argv, …)` — a launch without waiting.
    Spawn { argv: Vec<String> },
    /// A command whose refusal must be told to the person: if it failed, `failed` goes
    /// into the install notes (`Installed::notes`), the installation continues.
    RunNoted { argv: Vec<String>, timeout_secs: Option<u64>, failed: String },
    /// Tell the person after installation that a part was not done and
    /// why (`Installed::notes`); executes nothing.
    Notice { text: String },
    /// Refusing the installation entirely, even before the first step: all the package's main parts were
    /// skipped (something foreign is in place, see [`put_owned_copy`]/[`put_owned_file`]).
    /// `text` — the reason for the person; the executor answers with it as an error.
    Refuse { text: String },
}

fn run(argv: Vec<&str>, timeout: u64) -> ExtStep {
    ExtStep::Run {
        argv: argv.into_iter().map(str::to_string).collect(),
        timeout_secs: Some(timeout),
    }
}

// ============================================================================
// The plan environment
// ============================================================================

/// A deterministic tmp path provider (a replacement for tempfile.mkdtemp): paths
/// of the form `{base}/{prefix}{N}` — the executor on a live system substitutes
/// real mkdtemp directories in the same order.
pub struct TmpAlloc {
    base: String,
    n: Cell<u64>,
}

impl TmpAlloc {
    pub fn new(base: &str) -> Self {
        TmpAlloc { base: base.to_string(), n: Cell::new(0) }
    }

    pub fn mkdtemp(&self, prefix: &str) -> String {
        let n = self.n.get();
        self.n.set(n + 1);
        format!("{}/{}{}", self.base, prefix, n)
    }

    /// A permanent path `{base}/{name}` under the plan directory — without a counter
    /// (probes the snapshot prepares in advance: [`PLASMOID_PROBE_DIR`]).
    pub fn path(&self, name: &str) -> String {
        format!("{}/{}", self.base, name)
    }
}

/// The directory under the plan directory into which the snapshot (`ext_exec::plan_snapshot`)
/// unpacks the package's `.plasmoid` archive in advance — into a subdirectory named after
/// the archive file. By it the plan learns `KPlugin.Id` BEFORE starting `kpackagetool`
/// (which on update would replace a foreign plasmoid with the same id).
pub const PLASMOID_PROBE_DIR: &str = "rexon_plasmoid_probe";

/// The environment of `_safe_install`/`_safe_uninstall` — everything external is injected.
pub struct ExtEnv<'a> {
    /// $HOME (as the reference saw it; INSTALL_PATHS are built from it).
    pub home: &'a str,
    pub fs: &'a FsSnap,
    /// `shutil.which(name)` → a path or None.
    pub which: &'a dyn Fn(&str) -> Option<String>,
    /// Whether a recorded run of argv succeeded (returncode == 0) — for branches
    /// depending on a command's result (kpackagetool -i, systemctl is-active).
    pub run_ok: &'a dyn Fn(&[String]) -> bool,
    /// `$XDG_CURRENT_DESKTOP` (raw, the logic itself does lower()).
    pub xdg_current_desktop: &'a str,
    /// `$XDG_SESSION_TYPE`.
    pub xdg_session_type: &'a str,
    /// Whether `$WAYLAND_DISPLAY` is set.
    pub wayland_display: bool,
    /// Whether rexon-stage-helper (org.rexon.stage) is deployed.
    pub stage_helper_available: bool,
    /// The SYSTEM_HELPER path (org.rexon.system-install) and whether it is on disk.
    pub system_helper_path: &'a str,
    pub system_helper_exists: bool,
    /// The family for the system helper's argv: "debian" or "arch"
    /// (Python: DEBIAN → "debian", everything else → "arch").
    pub helper_family: &'a str,
    pub tmp: &'a TmpAlloc,
}

impl ExtEnv<'_> {
    /// INSTALL_PATHS[t] from the injected $HOME. The suffix table is one —
    /// `constants::INSTALL_PATH_SUFFIXES` (there is no second copy here anymore).
    fn install_path(&self, t: &str) -> Option<String> {
        constants::install_path_suffix(t).map(|s| format!("{}/{}", self.home, s))
    }

    /// Whether the system helper is available (`shutil.which("pkexec")` + the file).
    fn system_helper_ok(&self) -> bool {
        (self.which)("pkexec").is_some() && self.system_helper_exists
    }
}

/// An extension as a plan input: the directory + the parsed manifest.
pub struct ExtInfo<'a> {
    pub dir: &'a str,
    pub manifest: &'a Map<String, Value>,
}

impl ExtInfo<'_> {
    fn get_str(&self, key: &str, default: &'static str) -> String {
        self.manifest
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(default)
            .to_string()
    }

    pub fn ext_id(&self) -> String {
        self.get_str("id", "")
    }

    pub fn ext_type(&self) -> String {
        self.get_str("type", "plugin")
    }

    pub fn name(&self) -> String {
        match self.manifest.get("name").and_then(Value::as_str) {
            Some(name) => name.to_string(),
            None => crate::i18n::trc("extension name", "Untitled"),
        }
    }

    pub fn description(&self) -> String {
        self.get_str("description", "")
    }
}

// ============================================================================
// is_safe_sys_name — an allowlist of names for system paths under root
// ============================================================================

/// Port of `_is_safe_sys_name`: only `[A-Za-z0-9._-]+`, without a leading
/// `-`/`.`/`/` and without `..` (anti path-traversal / flag injection).
pub fn is_safe_sys_name(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('.')
        || name.starts_with('/')
        || name.contains("..")
    {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

// ============================================================================
// The install validation pipeline (_install_rxn before copying)
// ============================================================================

/// The root with manifest.json: tmp_dir itself or its first-level subdirectory —
/// port of `_find_manifest_root`.
pub fn find_manifest_root(fs: &FsSnap, tmp_dir: &str) -> Option<String> {
    // The unpacked .rxn tree is "observations" (created during the run).
    if fs.obs_is_file(&join(tmp_dir, "manifest.json")) {
        return Some(tmp_dir.to_string());
    }
    for name in fs.scandir(tmp_dir) {
        let sub = join(tmp_dir, name);
        if fs.obs_is_dir(&sub) && fs.obs_is_file(&join(&sub, "manifest.json")) {
            return Some(sub);
        }
    }
    None
}

/// An install error: the Python exception type + the text (byte for byte, except
/// BadJson — serde's JSONDecodeError detail is different, the prefix is checked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallError {
    /// "FileNotFoundError" | "ValueError" | "RuntimeError" | "FileExistsError"
    pub kind: &'static str,
    pub message: String,
}

fn verr(message: String) -> InstallError {
    InstallError { kind: "ValueError", message }
}

/// The result of a successful check.
pub struct InstallCheck {
    pub ext_id: String,
    pub ext_type: String,
    /// Requires install.sh/uninstall.sh (types with scripts).
    pub is_scripted: bool,
    /// The manifest after possible edits (v4: derived id/name; v2: the type).
    pub manifest: Value,
    /// The manifest.json text if Python rewrote it (json.dump indent=2,
    /// ensure_ascii=False — byte for byte), otherwise None.
    pub rewritten_manifest_text: Option<String>,
}

/// Type migration v2 → v3 — port of `V2_TYPE_MIGRATION` (empty since 2026-06-12,
/// kept in case of future migrations — as in Python).
pub const V2_TYPE_MIGRATION: &[(&str, &str)] = &[];

/// Port of the `_install_rxn` validation (from parsing the manifest to `shutil.copytree`,
/// not including it). `manifest` — the already parsed JSON from `src_dir`
/// (the caller wraps a parse error into the text
/// `manifest.json is damaged: …` via [`bad_manifest_json_error`]).
///
/// Injections:
///   * `realpath` — `os.path.realpath` (symbolic links are up to
///     the executor; for nonexistent paths lexical normalization
///     is enough);
///   * `dest_exists` — whether `extensions_dir/<ext_id>` exists.
pub fn install_rxn_check(
    fs: &FsSnap,
    src_dir: &str,
    extensions_dir: &str,
    manifest: Value,
    realpath: &dyn Fn(&str) -> String,
    dest_exists: &dyn Fn(&str) -> bool,
) -> Result<InstallCheck, InstallError> {
    let mut meta = match manifest {
        Value::Object(m) => m,
        // A pathological non-dict manifest: Python would crash on meta.get —
        // real .rxn files are always an object (as in extension.rs).
        _ => return Err(verr(missing_id())),
    };
    let mut rewritten = false;

    // ── v4 adaptive wallpaper: schema validation + deriving the id from the title ──
    if v4_fill_id_and_name(&mut meta)? {
        rewritten = true;
    }

    // ── id: presence + anti-traversal ──
    let ext_id = manifest_ext_id(&meta)?;
    let dest_dir = join(extensions_dir, &ext_id);
    let real_dest = realpath(&dest_dir);
    let real_base = realpath(extensions_dir);
    if !real_dest.starts_with(&format!("{real_base}/")) {
        return Err(verr(trf(
            "manifest.json: id '%1' points outside the extensions folder",
            &[&ext_id],
        )));
    }

    // ── type: v2 → v3 migration + validity ──
    let mut ext_type = meta
        .get("type")
        .map(py_str)
        .unwrap_or_default();
    if let Some((_, to)) = V2_TYPE_MIGRATION.iter().find(|(from, _)| *from == ext_type) {
        ext_type = to.to_string();
        meta.insert("type".to_string(), Value::String(ext_type.clone()));
        rewritten = true;
    }
    if !constants::is_valid_type(&ext_type) {
        return Err(verr(trf("Unknown extension type: '%1'", &[&ext_type])));
    }

    // ── script types: install.sh / uninstall.sh are mandatory ──
    let is_scripted = constants::is_scripted(&ext_type);
    if is_scripted {
        // src_dir is the unpacked tree (observations).
        if !fs.obs_is_file(&join(src_dir, "install.sh")) {
            return Err(InstallError {
                kind: "FileNotFoundError",
                message: trf("install.sh is required for type '%1'", &[&ext_type]),
            });
        }
        if !fs.obs_is_file(&join(src_dir, "uninstall.sh")) {
            return Err(InstallError {
                kind: "FileNotFoundError",
                message: trf("uninstall.sh is required for type '%1'", &[&ext_type]),
            });
        }
    }

    // ── compatibility with the Rexon version ──
    check_rexon_compat(&meta).map_err(|message| InstallError {
        kind: "RuntimeError",
        message,
    })?;

    // ── duplicate ──
    if dest_exists(&dest_dir) {
        return Err(InstallError {
            kind: "FileExistsError",
            message: trf(
                "Extension '%1' is already installed. Remove the old version first.",
                &[&ext_id],
            ),
        });
    }

    let manifest = Value::Object(meta);
    let rewritten_manifest_text = if rewritten {
        Some(dumps_indent2(&manifest))
    } else {
        None
    };
    Ok(InstallCheck {
        ext_id,
        ext_type,
        is_scripted,
        manifest,
        rewritten_manifest_text,
    })
}

/// A v4 manifest (`wallpaper-video-adaptive`): schema check and deriving `id`
/// from `title` (`v4-<slug>`) and `name` if they are missing. `Ok(true)` — the manifest
/// is v4 and something was added to it; `Ok(false)` — it is not v4. The shared part of
/// [`install_rxn_check`] and `rxn_manifest` (the in-memory v4 → v5 conversion).
pub fn v4_fill_id_and_name(meta: &mut Map<String, Value>) -> Result<bool, InstallError> {
    if meta.get("type").and_then(Value::as_str) != Some("wallpaper-video-adaptive") {
        return Ok(false);
    }
    let (ok, err) = validate_v4_manifest(&Value::Object(meta.clone()));
    if !ok {
        return Err(verr(trf("Invalid v4 manifest: %1", &[&err])));
    }
    if !is_truthy(meta.get("id")) {
        let title = meta
            .get("title")
            .map(py_str)
            .unwrap_or_default()
            .to_lowercase();
        let slug = slugify(&title);
        let id = if slug.is_empty() {
            "v4-adaptive".to_string()
        } else {
            format!("v4-{slug}")
        };
        meta.insert("id".to_string(), Value::String(id));
    }
    if !meta.contains_key("name") {
        // setdefault(name, title or id): an empty/falsy title → id.
        let name = match meta.get("title") {
            Some(t) if is_truthy(Some(t)) => t.clone(),
            _ => meta.get("id").cloned().unwrap_or(Value::Null),
        };
        meta.insert("name".to_string(), name);
    }
    Ok(true)
}

/// The `id` of a v3/v4 manifest: present, a non-empty string, without `/` and `..`.
pub fn manifest_ext_id(meta: &Map<String, Value>) -> Result<String, InstallError> {
    let ext_id = match meta.get("id") {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        _ => return Err(verr(missing_id())),
    };
    if ext_id.contains('/') || ext_id.contains("..") {
        return Err(verr(trf(
            "manifest.json: unsafe id '%1' (it contains path characters)",
            &[&ext_id],
        )));
    }
    Ok(ext_id)
}

/// "No id field" — one message for both check branches.
fn missing_id() -> String {
    tr("manifest.json: the 'id' field is missing")
}

/// The error text for a damaged manifest.json (Python `ValueError`:
/// `manifest.json is damaged: {JSONDecodeError}`; the detail after the colon is
/// serde's own — parity checks the prefix).
pub fn bad_manifest_json_error(detail: &str) -> InstallError {
    verr(trf("manifest.json is damaged: %1", &[&detail]))
}

/// Port of `_check_rexon_compat`: parsing `rexon_version` (`re.match` — only
/// from the start of the string) and comparing with REXON_VERSION. An unknown operator or
/// an unrecognized string — skipped (like `checks.get(op, True)`).
pub fn check_rexon_compat(meta: &Map<String, Value>) -> Result<(), String> {
    let req_str = meta
        .get("rexon_version")
        .map(py_str)
        .unwrap_or_default();
    if req_str.is_empty() {
        return Ok(());
    }
    // r"([><=!]+)\s*(\d+)\.(\d+)\.(\d+)" with re.match semantics (anchored only
    // at the start of the string; a tail after the third group is allowed).
    let cs: Vec<char> = req_str.chars().collect();
    let mut i = 0;
    while i < cs.len() && matches!(cs[i], '>' | '<' | '=' | '!') {
        i += 1;
    }
    if i == 0 {
        return Ok(()); // no match — like `if not m: return`
    }
    let op: String = cs[..i].iter().collect();
    while i < cs.len() && cs[i].is_whitespace() {
        i += 1;
    }
    let take_num = |i: &mut usize| -> Option<u32> {
        let start = *i;
        while *i < cs.len() && cs[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == start {
            return None;
        }
        cs[start..*i].iter().collect::<String>().parse().ok()
    };
    let expect_dot = |i: &mut usize| -> bool {
        if cs.get(*i) == Some(&'.') {
            *i += 1;
            true
        } else {
            false
        }
    };
    let Some(a) = take_num(&mut i) else { return Ok(()) };
    if !expect_dot(&mut i) {
        return Ok(());
    }
    let Some(b) = take_num(&mut i) else { return Ok(()) };
    if !expect_dot(&mut i) {
        return Ok(());
    }
    let Some(c) = take_num(&mut i) else { return Ok(()) };
    let req = (a, b, c);
    let cur = constants::REXON_VERSION;
    let ok = match op.as_str() {
        ">=" => cur >= req,
        ">" => cur > req,
        "==" => cur == req,
        "<=" => cur <= req,
        "<" => cur < req,
        "!=" => cur != req,
        _ => true, // checks.get(op, True)
    };
    if ok {
        Ok(())
    } else {
        Err(trf(
            "Requires Rexon %1, but %2 is installed",
            &[&req_str, &format!("{}.{}.{}", cur.0, cur.1, cur.2)],
        ))
    }
}

/// `re.sub(r"[^a-z0-9]+", "-", s).strip("-")` (the input is already lower()).
fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ============================================================================
// _safe_install as a plan
// ============================================================================

/// Port of `_safe_install`: installing appearance types as a pure plan.
pub fn safe_install_plan(env: &ExtEnv, ext: &ExtInfo) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    let mut pfs = PlanFs::new(env.fs);
    safe_install_into(env, ext, &mut pfs, &mut steps);
    // All main parts were skipped (something foreign is in place) — nothing to install: not "success with
    // notes" but a clear refusal entirely, before the first step. A partial
    // skip — an installation with notes.
    if pfs.skipped > 0 && !steps.iter().any(places_something) {
        let mut lines = vec![trf(
            "“%1” was not installed: everything in it would replace something that this package \
             did not install.",
            &[&ext.name()],
        )];
        lines.extend(steps.iter().filter_map(|s| match s {
            ExtStep::Notice { text } => Some(text.clone()),
            _ => None,
        }));
        return vec![ExtStep::Refuse { text: lines.join("\n") }];
    }
    steps
}

/// Whether a step puts something from the package: a copy, unpacking, a link, staging for
/// the system part, a system step or a KDE tool. Base directories
/// (`MakeDirs`), marks and notes do not count.
fn places_something(step: &ExtStep) -> bool {
    match step {
        ExtStep::CopyTree { .. }
        | ExtStep::CopyFile { .. }
        | ExtStep::ExtractZip { .. }
        | ExtStep::Symlink { .. }
        | ExtStep::Stage { .. }
        | ExtStep::PkexecScript { .. } => true,
        ExtStep::Run { argv, .. } => argv.first().is_some_and(|a| {
            let bin = a.rsplit('/').next().unwrap_or(a);
            bin == "pkexec" || bin.starts_with("kpackagetool")
        }),
        _ => false,
    }
}

fn safe_install_into(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let ext_type = ext.ext_type();
    let Some(target_base) = env.install_path(&ext_type) else {
        // wallpaper-pack is not copied — wallpapers are applied directly.
        return;
    };

    let data_dir = join(ext.dir, "data");
    if !env.fs.is_dir(&data_dir) {
        return; // a warning in the log — no steps
    }

    steps.push(ExtStep::MakeDirs { path: target_base.clone() });
    pfs.mk_dir(&target_base);

    // v4 adaptive: a symlink to the extension directory (we do not duplicate media).
    if ext_type == "wallpaper-video-adaptive" {
        let link = join(&target_base, &ext.ext_id());
        if !env.fs.lexists(&link) {
            steps.push(ExtStep::Symlink {
                target: ext.dir.to_string(),
                link,
            });
        }
        return;
    }

    if ext_type == "font-pack" {
        install_fonts_plan(env, ext, steps);
        return;
    }

    // KDE color schemes: .colors files (not folders).
    if ext_type == "theme-kde-colors" {
        for f in env.fs.scandir(&data_dir) {
            let src = join(&data_dir, f);
            if env.fs.is_file(&src) && f.ends_with(".colors") {
                let dest = join(&target_base, f);
                // Only the file: the scheme is chosen by «Apply» of the «Appearance» tab
                // (see [`crate::appearance_installer::applied_by_button`]).
                // A scheme of the person or another package with the same file name (and
                // different contents) is not overwritten.
                if !put_owned_file(env, pfs, &src, &dest, steps) {
                    steps.push(taken_file_notice(f, &dest));
                    pfs.skipped += 1;
                }
            }
        }
        return;
    }

    // Customization of specific desktops (KWin, GNOME, Cinnamon, XFCE, MATE,
    // LXQt) — ONE generic path by the `de_customization` matrix, not a new
    // branch per type (rule #12).
    if let Some(row) = de_customization::de_managed_row(&ext_type) {
        install_de_custom_plan(env, ext, row, &data_dir, pfs, steps);
        return;
    }

    // Other themes: each data/ subfolder is a theme/set. login-screen and
    // plymouth are installed via pkexec — the user copy is skipped.
    let skip_user_copy = matches!(
        ext_type.as_str(),
        "theme-login-screen" | "theme-plymouth"
    );
    let mut installed_names: Vec<String> = Vec::new();
    for name in env.fs.scandir(&data_dir) {
        let entry = join(&data_dir, name);
        if !env.fs.is_dir(&entry) {
            continue;
        }
        if skip_user_copy {
            installed_names.push(name.clone());
            continue;
        }
        // The shared icon set (`hicolor`, `locolor`) is not installed as a theme: all programs
        // share its directory, and the package's `index.theme` would shadow the
        // system one (defect of 09.11.2026, [`crate::icon_hygiene`]).
        if matches!(ext_type.as_str(), "theme-icons" | "theme-cursors")
            && crate::icon_hygiene::is_reserved_icon_set(name)
        {
            steps.push(shared_icon_set_notice(name));
            pfs.skipped += 1;
            continue;
        }
        let dest = join(&target_base, name);
        // Copies of bundle subfolders (`~/.local/share/rexon/bundles/<subfolder>`)
        // and Conky ones (`~/.config/conky/<subfolder>`) lie under SHARED names —
        // the same folder may be someone else's. They are put with an owner mark, so that
        // removal takes away exactly our own (defects D2/D3, 25.10.2026).
        if ext_type == "theme-bundle" || ext_type == "theme-conky" {
            installed_names.push(name.clone());
            // A regular Conky widget: the whole `data/` is ONE directory
            // `~/.config/conky/<id>` (`install_conky_plan`), subfolders
            // are not put separately. The old copy of `data/<subfolder>` in
            // `~/.config/conky/<subfolder>` was a second widget with the same
            // settings file — it was shown and started twice (defect of
            // 14.11.2026; copies already in place are removed by
            // `ext_exec::heal_conky_duplicates`). Several widgets in
            // one package — still separate tiles `<id>/<subfolder>`
            // (`de_apply::conky_nested_widgets`).
            if ext_type == "theme-conky" && !is_conky_set(ext) {
                continue;
            }
            // A subfolder named after the set itself is its directory
            // `~/.config/conky/<id>`: `install_conky_plan` puts it.
            if !(ext_type == "theme-conky" && *name == ext.ext_id())
                && !put_owned_copy(env, ext, pfs, &entry, &dest, steps)
                && is_conky_set(ext)
            {
                // A widget set from a file: the folder is taken by someone else's widget with the same
                // name — say that the set will use it.
                steps.push(taken_by_other_notice(name, &dest));
                pfs.skipped += 1;
            }
            continue;
        }
        // The theme's main copy — by the same owner rule: if in place is
        // a directory of someone else's theme (installed by hand, by another package or
        // by the old Rexon without a mark) — do not touch it, but tell the person.
        if !put_owned_copy(env, ext, pfs, &entry, &dest, steps) {
            steps.push(taken_by_other_notice(name, &dest));
            pfs.skipped += 1;
            continue;
        }
        installed_names.push(name.clone());
    }

    // ⛔ Installing a theme does NOT apply it. Previously
    // plasma-apply-lookandfeel/-colorscheme/-desktoptheme, kvantummanager --set
    // and writing ~/.icons/default ran here (the cursor of X11 programs changed right at
    // installation, before «Apply» — defect of 03.10.2026). All these types are enabled by
    // the «Apply» button of the «Appearance» tab with each kind's shared plan
    // (the cursor — `themes::apply_cursor_plan_sized` on all desktops); the guard is
    // `rxn_install_never_applies_what_the_button_applies` in the tests.

    match ext_type.as_str() {
        "theme-window-decoration" => {
            install_window_decoration_plan(env, ext, &installed_names, pfs, steps)
        }
        "theme-login-screen" => {
            install_login_screen_plan(env, ext, &data_dir, &installed_names, steps)
        }
        "theme-plymouth" => {
            install_plymouth_plan(env, ext, &data_dir, &installed_names, steps)
        }
        "theme-conky" => install_conky_plan(env, ext, &data_dir, pfs, steps),
        "theme-bundle" => install_bundle_plan(env, ext, pfs, steps),
        "theme-plasmoid" => install_plasmoid_plan(env, ext, &data_dir, pfs, steps),
        _ => {}
    }
}

// ============================================================================
// Desktop customization: one path for all matrix types
// ============================================================================

/// An install object: what we copy and under what name it will be enabled.
struct DeObject {
    src: String,
    /// The destination directory name; it is also the identifier for activation
    /// (uuid for GNOME/Cinnamon, KPlugin.Id for KDE, the theme name for ~/.themes).
    name: String,
}

/// Read a field from a snapshot JSON file (`meta`), supporting a dotted path
/// (`KPlugin.Id`).
fn json_field(fs: &FsSnap, path: &str, field: &str) -> Option<String> {
    let text = fs.read_text(path)?;
    let v: Value = serde_json::from_str(text).ok()?;
    let mut cur = &v;
    for part in field.split('.') {
        cur = cur.get(part)?;
    }
    cur.as_str().map(String::from)
}

/// Split `data/` into objects by the payload shape of the matrix row.
///
/// A single pitfall of all desktops: the destination directory name is not always equal to the
/// folder name in the archive — GNOME and Cinnamon require the directory to be named exactly `uuid`,
/// KDE — `KPlugin.Id`. Get the name wrong — the object installs and silently
/// does not work.
fn de_objects(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    data_dir: &str,
) -> Vec<DeObject> {
    use de_customization::Payload;

    let mut out: Vec<DeObject> = Vec::new();

    // The object name by the candidate directory.
    let resolve = |src: &str, fallback: &str| -> String {
        match row.payload {
            Payload::DirUuid { meta, field } => {
                json_field(env.fs, &join(src, meta), field)
                    .unwrap_or_else(|| fallback.to_string())
            }
            Payload::KPackage { id_field, .. } => {
                json_field(env.fs, &join(src, "metadata.json"), id_field)
                    .or_else(|| {
                        env.fs
                            .read_text(&join(src, "metadata.desktop"))
                            .and_then(|t| {
                                ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name")
                            })
                    })
                    .unwrap_or_else(|| fallback.to_string())
            }
            _ => fallback.to_string(),
        }
    };

    // The sign "this directory is the object itself" (rather than a container of objects).
    let is_object = |dir: &str| -> bool {
        match row.payload {
            Payload::DirUuid { meta, .. } => env.fs.is_file(&join(dir, meta)),
            Payload::KPackage { .. } => {
                env.fs.is_file(&join(dir, "metadata.json"))
                    || env.fs.is_file(&join(dir, "metadata.desktop"))
            }
            Payload::ThemeSubdir { sub } => env.fs.is_dir(&join(dir, sub)),
            Payload::Dir => true,
            Payload::Files { .. } => false,
        }
    };

    if let Payload::Files { ext: suffix } = row.payload {
        for f in env.fs.scandir(data_dir) {
            let src = join(data_dir, f);
            if env.fs.is_file(&src) && f.ends_with(suffix) {
                out.push(DeObject { src, name: f.clone() });
            }
        }
        return out;
    }

    for name in env.fs.scandir(data_dir) {
        let entry = join(data_dir, name);
        if env.fs.is_dir(&entry) && is_object(&entry) {
            out.push(DeObject {
                name: resolve(&entry, name),
                src: entry,
            });
        }
    }

    // The package was put without a wrapper — data/ itself is the object (a common case:
    // an archive with a gnome-shell extension unpacked "as is").
    if out.is_empty() && is_object(data_dir) {
        let fallback = ext.ext_id();
        out.push(DeObject {
            name: resolve(data_dir, &fallback),
            src: data_dir.to_string(),
        });
    }
    out
}

/// The "run a shell script" step (changing gsettings lists and editing
/// ini files cannot be done with one command — see `de_customization`).
fn sh(script: String) -> ExtStep {
    ExtStep::Run {
        argv: vec!["bash".into(), "-c".into(), script],
        timeout_secs: Some(30),
    }
}

/// An object's activation steps. Empty means there is nothing to enable or enabling
/// is deliberately left to the user (a Plasma wallpaper plugin).
fn de_activate_steps(
    env: &ExtEnv,
    row: &de_customization::Row,
    obj_name: &str,
    dest: &str,
) -> Vec<ExtStep> {
    use de_customization::{Activate, EntryFmt};

    let kw = |args: Vec<String>| ExtStep::Run { argv: args, timeout_secs: Some(10) };

    match row.activate {
        // Rexon — types of the shared appearance installer: they do not come here
        // (`de_managed_row` cuts them off), the theme installer enables them.
        Activate::Auto | Activate::Rexon | Activate::Manual(_) => Vec::new(),
        Activate::KdeConfig { file, group, key, extra } => {
            let mut v = vec![kw(vec![
                "kwriteconfig6".into(),
                "--file".into(),
                file.into(),
                "--group".into(),
                group.into(),
                "--key".into(),
                key.into(),
                obj_name.into(),
            ])];
            if let Some((k, val)) = extra {
                v.push(kw(vec![
                    "kwriteconfig6".into(),
                    "--file".into(),
                    file.into(),
                    "--group".into(),
                    group.into(),
                    "--key".into(),
                    k.into(),
                    val.into(),
                ]));
            }
            v
        }
        Activate::KwinPlugin => vec![kw(vec![
            "kwriteconfig6".into(),
            "--file".into(),
            "kwinrc".into(),
            "--group".into(),
            "Plugins".into(),
            "--key".into(),
            format!("{obj_name}Enabled"),
            "true".into(),
        ])],
        Activate::Gsettings { schema, key } => vec![kw(vec![
            "gsettings".into(),
            "set".into(),
            schema.into(),
            key.into(),
            obj_name.into(),
        ])],
        Activate::GsettingsList { schema, key, entry, .. } => match entry {
            EntryFmt::CinnamonApplet => {
                vec![sh(de_customization::cinnamon_applet_enable_script(obj_name))]
            }
            EntryFmt::CinnamonDesklet => {
                vec![sh(de_customization::cinnamon_desklet_enable_script(obj_name))]
            }
            EntryFmt::Uuid => vec![sh(de_customization::gsettings_list_add_script(
                schema, key, obj_name, obj_name,
            ))],
        },
        Activate::GnomeExtension => {
            vec![sh(de_customization::gnome_extension_enable_script(obj_name, dest))]
        }
        // `-n -t string`: on a fresh profile the property does not exist yet, and a bare `-s`
        // refuses ("does not exist on channel") — test bench of 04.10.2026.
        Activate::Xfconf { channel, prop } => vec![kw(vec![
            "xfconf-query".into(),
            "-c".into(),
            channel.into(),
            "-p".into(),
            prop.into(),
            "-n".into(),
            "-t".into(),
            "string".into(),
            "-s".into(),
            obj_name.into(),
        ])],
        Activate::IniKey { file, group, key } => vec![sh(de_customization::ini_set_script(
            &format!("{}/{}", env.home, file),
            group,
            key,
            obj_name,
        ))],
        // An `rc.xml` node is edited by its text, and this plan cannot read
        // files: the Openbox and labwc frame theme is enabled by the section's button
        // (`crate::de_apply::rc_theme_steps`), installation does not enable it.
        Activate::RcXmlTheme => Vec::new(),
    }
}

/// A desktop refresh step — only where a restart is safe.
///
/// We deliberately do NOT restart the GNOME shell (on Wayland it is impossible, and on
/// X11 it requires rights we do not have), the LXQt panel (there is nobody to bring up a crashed panel
/// — the user would be left without a panel) and plasmashell (redrawing
/// the desktop is not worth the risk of losing the layout). There the change is applied following the hint
/// [`Refresh::hint`] in the interface.
fn qdbus_tool(env: &ExtEnv) -> Option<String> {
    (env.which)("qdbus6")
        .or_else(|| (env.which)("qdbus"))
        .or_else(|| (env.which)("qdbus-qt6"))
}

fn de_refresh_step(env: &ExtEnv, row: &de_customization::Row) -> Option<ExtStep> {
    use de_customization::Refresh;
    match row.refresh {
        Refresh::KwinReconfigure => {
            let tool = qdbus_tool(env)?;
            Some(ExtStep::Run {
                argv: vec![
                    tool,
                    "org.kde.KWin".into(),
                    "/KWin".into(),
                    "reconfigure".into(),
                ],
                timeout_secs: Some(10),
            })
        }
        Refresh::CinnamonReplace => {
            // On Wayland Cinnamon cannot be restarted — only logging in again.
            if env.wayland_display || env.xdg_session_type.eq_ignore_ascii_case("wayland") {
                return None;
            }
            // `cinnamon --replace` in someone else's session (MATE, XFCE…) would replace
            // the window manager there with Cinnamon: only in a running Cinnamon.
            if !env.xdg_current_desktop.to_lowercase().contains("cinnamon") {
                return None;
            }
            let tool = (env.which)("cinnamon")?;
            Some(ExtStep::Spawn { argv: vec![tool, "--replace".into()] })
        }
        _ => None,
    }
}

/// Enable an already installed object of a matrix type — the «Apply» /
/// «Enable» button of the «Appearance» tab (`crate::de_apply`).
///
/// The same steps installation used to do ([`de_activate_steps`] plus
/// a safe desktop refresh): installation now only puts the files,
/// and the person enables with a button (decision of 04.10.2026). There is no second list of commands
/// — rule #12. Empty — the type is not from the matrix or there is nothing to enable.
pub fn de_enable_steps(env: &ExtEnv, kind: &str, obj_name: &str) -> Vec<ExtStep> {
    let Some(row) = de_customization::de_managed_row(kind) else {
        return Vec::new();
    };
    let dest = join(&row.dest_path(env.home), obj_name);
    let mut steps = de_activate_steps(env, row, obj_name, &dest);
    if !steps.is_empty() {
        steps.extend(de_refresh_step(env, row));
    }
    steps
}

/// Names of the objects of a matrix type inside a `.rxn` package — as they lie
/// on disk after installation (uuid for GNOME/Cinnamon, `KPlugin.Id` for
/// KDE, the file name for Nemo). By them the «Extensions» tab enables and
/// disables the add-on the same way as the «Appearance» button.
pub fn de_object_names(env: &ExtEnv, ext: &ExtInfo) -> Vec<String> {
    let Some(row) = de_customization::de_managed_row(&ext.ext_type()) else {
        return Vec::new();
    };
    let data_dir = join(ext.dir, "data");
    de_objects(env, ext, row, &data_dir).into_iter().map(|o| o.name).collect()
}

/// Disable an object of a matrix type — the «Disable» button (the same steps
/// uninstalling does before removing the directory).
pub fn de_disable_steps(env: &ExtEnv, kind: &str, obj_name: &str) -> Vec<ExtStep> {
    let Some(row) = de_customization::de_managed_row(kind) else {
        return Vec::new();
    };
    let mut steps = de_deactivate_steps(env, row, obj_name);
    if !steps.is_empty() {
        steps.extend(de_refresh_step(env, row));
    }
    steps
}

/// Installing a desktop customization type: copying + enabling + refreshing.
fn install_de_custom_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    use de_customization::{Activate, Payload};

    let objects = de_objects(env, ext, row, data_dir);
    if objects.is_empty() {
        return; // a warning in the log — no steps
    }

    let base = row.dest_path(env.home);
    let mut installed: Vec<String> = Vec::new();

    for obj in &objects {
        if let Payload::Files { .. } = row.payload {
            // A single file — by the owner rule for files: a foreign file with
            // different contents in place — skipped with a note.
            let dest = join(&base, &obj.name);
            if !put_owned_file(env, pfs, &obj.src, &dest, steps) {
                steps.push(taken_file_notice(&obj.name, &dest));
                pfs.skipped += 1;
                continue;
            }
            installed.push(obj.name.clone());
            continue;
        }

        // A directory with the same name is already installed and was not put by this package
        // (the person's theme, another package, a copy of the old Rexon without a mark) —
        // the object is skipped entirely: no copy, no KDE tool, no
        // enabling someone else's theme. Our own earlier copy (this package's mark)
        // is removed and put again below by `put_owned_copy`.
        let dest = join(&base, &obj.name);
        if (pfs.exists(&dest) || env.fs.lexists(&dest)) && !owned_by(env.fs, &dest, &ext.ext_id()) {
            steps.push(taken_by_other_notice(&obj.name, &dest));
            pfs.skipped += 1;
            continue;
        }

        // KDE packages are installed by their own tool — it puts the files into the
        // right directory and updates the registry; copying remains
        // the fallback path (as already done for plasmoids).
        if let Payload::KPackage { ptype, .. } = row.payload {
            if let Some(tool) =
                (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
            {
                steps.push(ExtStep::Run {
                    argv: vec![
                        tool.clone(),
                        "--type".into(),
                        ptype.into(),
                        "-u".into(),
                        obj.src.clone(),
                    ],
                    timeout_secs: Some(30),
                });
                let install_argv = vec![
                    tool,
                    "--type".into(),
                    ptype.into(),
                    "-i".into(),
                    obj.src.clone(),
                ];
                steps.push(ExtStep::Run {
                    argv: install_argv.clone(),
                    timeout_secs: Some(30),
                });
                installed.push(obj.name.clone());
                if (env.run_ok)(&install_argv) {
                    // An owner mark for a package installed by the KDE tool
                    // too: by it removal recognizes our own (someone else's with the same
                    // name is not removed).
                    steps.push(ExtStep::WriteFile {
                        path: join(&dest, OWNER_MARK),
                        content: format!("{}\n", ext.ext_id()),
                    });
                    continue;
                }
            } else {
                installed.push(obj.name.clone());
            }
        } else {
            installed.push(obj.name.clone());
        }

        steps.push(ExtStep::MakeDirs { path: base.clone() });
        pfs.mk_dir(&base);
        put_owned_copy(env, ext, pfs, &obj.src, &dest, steps);
    }

    if installed.is_empty() {
        return;
    }

    // Scalar keys (a theme name, the Alt+Tab layout) take ONE value:
    // we take apply_name from the manifest, otherwise the first object. List enablings
    // (applets, extensions) apply to all objects of the package.
    let scalar_target = ext
        .manifest
        .get("apply_name")
        .map(py_str)
        .unwrap_or_else(|| installed[0].clone());

    let list_like = matches!(
        row.activate,
        Activate::GsettingsList { .. } | Activate::GnomeExtension | Activate::KwinPlugin
    );
    let targets: Vec<String> = if list_like {
        installed.clone()
    } else {
        vec![scalar_target]
    };

    // ⛔ A type enabled by «Apply» of the «Appearance» tab (the KDE splash and
    // lock screen) is only put in place by installation: the choice is the person's
    // (defect of 03.10.2026). The key in `activate` still stays — by it
    // removal resets the setting if it pointed at the theme being removed.
    if crate::appearance_installer::applied_by_button(row.kind) {
        return;
    }

    let mut activated = false;
    for name in &targets {
        let dest = join(&base, name);
        for step in de_activate_steps(env, row, name, &dest) {
            steps.push(step);
            activated = true;
        }
    }

    if activated {
        if let Some(step) = de_refresh_step(env, row) {
            steps.push(step);
        }
    }
}

/// An object's disabling steps — the reverse of [`de_activate_steps`].
///
/// Shared by removal (disable first, then remove) and by the
/// «Disable» button of the «Appearance» tab ([`de_disable_steps`]): there is no second way
/// to disable the same thing (rule #12). Empty — nothing to disable.
fn de_deactivate_steps(env: &ExtEnv, row: &de_customization::Row, obj_name: &str) -> Vec<ExtStep> {
    use de_customization::{Activate, EntryFmt, Payload};
    let mut v: Vec<ExtStep> = Vec::new();
    match row.activate {
        Activate::Auto | Activate::Rexon | Activate::Manual(_) => {}
        Activate::KdeConfig { file, group, key, .. } => {
            v.push(sh(de_customization::kde_config_reset_if_script(
                file, group, key, obj_name,
            )));
        }
        Activate::KwinPlugin => {
            v.push(ExtStep::Run {
                argv: vec![
                    "kwriteconfig6".into(),
                    "--file".into(),
                    "kwinrc".into(),
                    "--group".into(),
                    "Plugins".into(),
                    "--key".into(),
                    format!("{}Enabled", obj_name),
                    "false".into(),
                ],
                timeout_secs: Some(10),
            });
            // Checked live on Plasma 6: `reconfigure` does NOT unload a removed
            // script — it keeps working until the end of the session, even though
            // the files are already deleted. We unload explicitly, KWin has
            // a separate method for that (effects do not have this problem — the
            // key in kwinrc is enough for them).
            if matches!(
                row.payload,
                Payload::KPackage { ptype: "KWin/Script", .. }
            ) {
                if let Some(tool) = qdbus_tool(env) {
                    v.push(ExtStep::Run {
                        argv: vec![
                            tool,
                            "org.kde.KWin".into(),
                            "/Scripting".into(),
                            "unloadScript".into(),
                            obj_name.to_string(),
                        ],
                        timeout_secs: Some(10),
                    });
                }
            }
        }
        Activate::Gsettings { schema, key } => {
            v.push(sh(de_customization::gsettings_reset_if_script(
                schema, key, obj_name,
            )));
        }
        Activate::GsettingsList { schema, key, entry, .. } => {
            let marker = match entry {
                EntryFmt::CinnamonApplet => format!(":{}:", obj_name),
                EntryFmt::CinnamonDesklet | EntryFmt::Uuid => obj_name.to_string(),
            };
            v.push(sh(de_customization::gsettings_list_remove_script(
                schema, key, &marker,
            )));
        }
        Activate::GnomeExtension => {
            v.push(sh(de_customization::gnome_extension_disable_script(
                obj_name,
            )));
        }
        Activate::Xfconf { channel, prop } => {
            v.push(ExtStep::Run {
                argv: vec![
                    "xfconf-query".into(),
                    "-c".into(),
                    channel.into(),
                    "-p".into(),
                    prop.into(),
                    "-r".into(),
                ],
                timeout_secs: Some(10),
            });
        }
        Activate::IniKey { .. } => {
            // An LXQt theme: we do not edit someone else's config, the panel will simply go back to
            // the system theme at the next login.
        }
        Activate::RcXmlTheme => {
            // An Openbox and labwc frame theme: on removal we do not touch `rc.xml` —
            // the window manager, not finding the theme, draws frames with its built-in one.
        }
    }
    v
}

/// Uninstalling a desktop customization type: disable first, then remove.
///
/// The order matters: if the directory is deleted first while the key keeps pointing at
/// it, the desktop gets a link to nowhere (for the KDE lock screen this
/// means a broken lock).
fn uninstall_de_custom_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    row: &de_customization::Row,
    steps: &mut Vec<ExtStep>,
) {
    use de_customization::Payload;

    let data_dir = join(ext.dir, "data");
    let objects = de_objects(env, ext, row, &data_dir);
    if objects.is_empty() {
        return;
    }
    let base = row.dest_path(env.home);
    let mut deactivated = false;

    for obj in &objects {
        let off = de_deactivate_steps(env, row, &obj.name);
        if !off.is_empty() {
            deactivated = true;
        }
        steps.extend(off);

        let dest = join(&base, &obj.name);
        // A directory with this name is installed but was not put by this package (the person's
        // theme that installation skipped, or a copy of the old Rexon
        // without a mark) — remove it neither with the KDE tool nor by deletion.
        let foreign = !matches!(row.payload, Payload::Files { .. })
            && env.fs.lexists(&dest)
            && !owned_by(env.fs, &dest, &ext.ext_id());
        if foreign {
            continue;
        }

        // KDE packages are removed by their tool — otherwise a registry entry stays.
        if let Payload::KPackage { ptype, .. } = row.payload {
            if let Some(tool) =
                (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
            {
                steps.push(ExtStep::Run {
                    argv: vec![
                        tool,
                        "--type".into(),
                        ptype.into(),
                        "-r".into(),
                        obj.name.clone(),
                    ],
                    timeout_secs: Some(30),
                });
            }
        }

        if let Payload::Files { .. } = row.payload {
            // Only our own file (this package's log or byte for byte its own):
            // installation skipped the person's file with the same name — removal
            // does not touch it either.
            if file_is_ours(env.fs, &obj.src, &dest) {
                steps.push(ExtStep::RemoveFile { path: dest });
            }
        } else if env.fs.is_dir(&dest) {
            steps.push(ExtStep::RemoveTree {
                path: dest,
                safe: true,
                ignore_errors: true,
            });
        }
    }

    if deactivated {
        if let Some(step) = de_refresh_step(env, row) {
            steps.push(step);
        }
    }
}

/// Port of `Extension.install_fonts` (fonts into ~/.local/share/fonts/rexon-<id>).
fn install_fonts_plan(env: &ExtEnv, ext: &ExtInfo, steps: &mut Vec<ExtStep>) {
    let fonts = font_files(env.fs, ext);
    if fonts.is_empty() {
        return; // a warning in the log
    }
    let dest_dir = format!("{}/.local/share/fonts/rexon-{}", env.home, ext.ext_id());
    steps.push(ExtStep::MakeDirs { path: dest_dir.clone() });
    for font in &fonts {
        // File to file under its own name: the shared executor has no copy "into a
        // directory" (step 2 of merging the installers).
        let name = font.rsplit('/').next().unwrap_or(font);
        steps.push(ExtStep::CopyFile { src: font.clone(), dest: join(&dest_dir, name) });
    }
    steps.push(run(vec!["fc-cache", "-f", &dest_dir], 30));
}

/// Port of `Extension.font_files` from the snapshot: data/ (or the root) recursively,
/// format matching — by the whole suffix (`is_supported_font`), so that
/// a compound `.pcf.gz` matches too.
fn font_files(fs: &FsSnap, ext: &ExtInfo) -> Vec<String> {
    let data_dir = join(ext.dir, "data");
    let root = if fs.is_dir(&data_dir) { data_dir } else { ext.dir.to_string() };
    let mut out = Vec::new();
    for (dir, _subdirs, files) in fs.walk(&root) {
        for f in &files {
            if is_supported_font(f) {
                out.push(join(&dir, f));
            }
        }
    }
    out
}

/// The owner mark of a copy: a file in the root of a directory put by the package under
/// a SHARED name (`~/.themes/<theme>` for frames outside KDE, bundle subfolders in
/// `~/.local/share/rexon/bundles`, Conky subfolders in `~/.config/conky`).
/// The text is the package id. Removal and update touch the directory only if
/// the mark is theirs — a theme of the person or another package with the same name stays
/// as it was (defects D1–D3, found by the safety net on 25.10.2026).
pub const OWNER_MARK: &str = ".rexon-owner";

/// The directory `dir` was put by the package `ext_id`: a regular directory (not a link) with
/// our mark.
fn owned_by(fs: &FsSnap, dir: &str, ext_id: &str) -> bool {
    fs.is_dir(dir)
        && !fs.is_link(dir)
        && fs.read_text(&join(dir, OWNER_MARK)).is_some_and(|t| t.trim() == ext_id)
}

/// Put a copy of `src` into `dest` on behalf of the package: no place — a copy and
/// a mark; the place is ours — remove and put again (a package update); the place is
/// someone else's — do not touch (`false`: the copy was not put).
///
/// Compatibility: copies Rexon put before the mark (25.10.2026) have no mark
/// and are indistinguishable from foreign ones — they are not touched either. To install over
/// such a copy — delete its directory and install the package again.
fn put_owned_copy(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    src: &str,
    dest: &str,
    steps: &mut Vec<ExtStep>,
) -> bool {
    let ext_id = ext.ext_id();
    if pfs.exists(dest) || env.fs.lexists(dest) {
        if !owned_by(env.fs, dest, &ext_id) {
            return false;
        }
        steps.push(ExtStep::RemoveTree {
            path: dest.to_string(),
            safe: true,
            ignore_errors: true,
        });
    }
    steps.push(ExtStep::CopyTree { src: src.to_string(), dest: dest.to_string() });
    pfs.mk_dir(dest);
    steps.push(ExtStep::WriteFile {
        path: join(dest, OWNER_MARK),
        content: format!("{ext_id}\n"),
    });
    true
}

/// An install note: the theme `name` was not installed — in its place `dest` there is
/// a directory not put by this package (like a skip in the store).
/// An install note: a package subfolder is named like a shared icon set
/// (`hicolor`, `locolor`) and is not installed.
fn shared_icon_set_notice(name: &str) -> ExtStep {
    ExtStep::Notice {
        text: trf("“%1” skipped: this is the shared icon set of all programs, a theme must not replace it", &[&name]),
    }
}

fn taken_by_other_notice(name: &str, dest: &str) -> ExtStep {
    ExtStep::Notice {
        text: trf(
            "“%1” skipped: %2 already holds a theme with this name that this package did not \
             install. To replace it, remove that folder and install the package again.",
            &[&name, &dest],
        ),
    }
}

/// Put a single file `src` into `dest` on behalf of the package (the owner rule
/// for files: a mark cannot be put into a file name, the owner is the shared
/// log). No place — a copy. In place is a file the shared log recorded
/// for this package, or the same byte for byte (then it counts as our own and
/// goes into this installation's log) — a copy over it. Otherwise — a foreign file
/// (the person's, another package's, a link, a directory): do not touch (`false`).
fn put_owned_file(env: &ExtEnv, pfs: &mut PlanFs, src: &str, dest: &str, steps: &mut Vec<ExtStep>) -> bool {
    if !pfs.made_files.contains(dest) && (pfs.exists(dest) || env.fs.lexists(dest)) && !file_is_ours(env.fs, src, dest) {
        return false;
    }
    steps.push(ExtStep::CopyFile { src: src.to_string(), dest: dest.to_string() });
    pfs.mk_file(dest);
    true
}

/// The file in place `dest` is our own for the package with the file `src`: a regular file (not
/// a link), recorded for the package by the shared log or the same byte for byte.
fn file_is_ours(fs: &FsSnap, src: &str, dest: &str) -> bool {
    fs.is_file(dest) && !fs.is_link(dest) && (fs.owns_file(dest) || fs.same_bytes(src, dest))
}

/// An install note: the file `name` was not put — in its place `dest` lies
/// another file, not put by this package.
fn taken_file_notice(name: &str, dest: &str) -> ExtStep {
    ExtStep::Notice {
        text: trf(
            "“%1” skipped: %2 already exists, differs from the file in this package and was not \
             installed by it. To replace it, remove that file and install the package again.",
            &[&name, &dest],
        ),
    }
}

/// Remove the copy `dest` if this package put it ([`put_owned_copy`]).
fn remove_owned_copy(env: &ExtEnv, ext: &ExtInfo, dest: &str, steps: &mut Vec<ExtStep>) {
    if owned_by(env.fs, dest, &ext.ext_id()) {
        steps.push(ExtStep::RemoveTree {
            path: dest.to_string(),
            safe: true,
            ignore_errors: true,
        });
    }
}

/// Frames outside KDE: a theme from the Aurorae directory also goes into `~/.themes/<theme>`
/// (GNOME, Cinnamon, Budgie, Pantheon and XFCE look for it there).
fn copy_decoration_to_gtk_themes(
    env: &ExtEnv,
    ext: &ExtInfo,
    twd_base: &str,
    installed_names: &[String],
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let gtk_themes = format!("{}/.themes", env.home);
    for name in installed_names {
        let src = join(twd_base, name);
        if pfs.is_dir(&src) {
            put_owned_copy(env, ext, pfs, &src, &join(&gtk_themes, name), steps);
        }
    }
}

/// Port of `_install_window_decoration` (DE-dependent decoration installation).
fn install_window_decoration_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    installed_names: &[String],
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let xdg = env.xdg_current_desktop.to_lowercase();
    let twd_base = env
        .install_path("theme-window-decoration")
        .expect("theme-window-decoration в INSTALL_PATHS");

    if xdg.contains("kde") || xdg.contains("plasma") {
        // Frames (Aurorae) are enabled by «Apply» of the «Appearance» tab
        // (`aurorae_plan`), installation only puts the theme into the Aurorae directory.
        // Previously kwinrc org.kde.kdecoration2 was written here and
        // reloadConfig was sent — frames changed at installation (defect of 03.10.2026).
    } else if xdg.contains("gnome")
        || xdg.contains("budgie")
        || xdg.contains("pantheon")
        || xdg.contains("cinnamon")
    {
        copy_decoration_to_gtk_themes(env, ext, &twd_base, installed_names, pfs, steps);
    } else if xdg.contains("xfce") {
        copy_decoration_to_gtk_themes(env, ext, &twd_base, installed_names, pfs, steps);
        // XFCE frames are enabled by «Apply» of the «Window frames (XFCE)» section (the
        // `theme-xfwm4` type, decision of 04.10.2026): installation only puts the theme into
        // ~/.themes. The branch writing xfconf remains in case the type loses its
        // button — otherwise there would be nothing to enable the theme with.
        if crate::appearance_installer::applied_by_button("theme-xfwm4") {
            return;
        }
        if let Some(first) = installed_names.first() {
            steps.push(run(
                vec!["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-s", first],
                10,
            ));
        }
    }
    // other DEs — only the log
}

/// Port of `_detect_display_manager` as a plan: systemctl probes are steps, their
/// result comes through `env.run_ok`; the `shutil.which` fallback — without steps.
pub fn detect_display_manager_plan(env: &ExtEnv, steps: &mut Vec<ExtStep>) -> String {
    const DM_SERVICES: &[(&str, &str)] = &[
        ("sddm", "sddm"),
        ("gdm", "gdm"),
        ("lightdm", "lightdm"),
        ("cosmic-greeter", "cosmic-greeter"),
        ("lxdm", "lxdm"),
    ];
    for (service, name) in DM_SERVICES {
        let argv: Vec<String> = vec![
            "systemctl".into(),
            "is-active".into(),
            format!("{service}.service"),
        ];
        steps.push(ExtStep::Run { argv: argv.clone(), timeout_secs: Some(5) });
        if (env.run_ok)(&argv) {
            return name.to_string();
        }
    }
    const DM_EXES: &[(&str, &str)] = &[
        ("sddm", "sddm"),
        ("gdm", "gdm"),
        ("gdm3", "gdm"),
        ("lightdm", "lightdm"),
        ("cosmic-greeter", "cosmic-greeter"),
    ];
    for (exe, name) in DM_EXES {
        if (env.which)(exe).is_some() {
            return name.to_string();
        }
    }
    String::new()
}

/// Port of `_find_background`: `background*`/`bg*` in the root, otherwise the largest
/// image file > 10 000 bytes (from the snapshot with sizes).
pub fn find_background(fs: &FsSnap, data_dir: &str) -> String {
    const BG_EXTS: &[&str] = &[".png", ".jpg", ".jpeg", ".webp", ".bmp"];
    let ext_of = |f: &str| -> String {
        f.rfind('.').map(|i| f[i..].to_lowercase()).unwrap_or_default()
    };
    for f in fs.scandir(data_dir) {
        let lower = f.to_lowercase();
        if (lower.starts_with("background") || lower.starts_with("bg"))
            && BG_EXTS.contains(&ext_of(f).as_str())
        {
            return join(data_dir, f);
        }
    }
    let mut biggest = (String::new(), 0u64);
    for (root, _dirs, files) in fs.walk(data_dir) {
        for f in &files {
            if BG_EXTS.contains(&ext_of(f).as_str()) {
                let full = join(&root, f);
                let sz = fs.size(&full);
                if sz > biggest.1 {
                    biggest = (full, sz);
                }
            }
        }
    }
    if biggest.1 > 10_000 {
        biggest.0
    } else {
        String::new()
    }
}

/// Port of `_install_login_screen` (SDDM/GDM/LightDM/COSMIC).
fn install_login_screen_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    installed_names: &[String],
    steps: &mut Vec<ExtStep>,
) {
    let dm = detect_display_manager_plan(env, steps);
    if dm.is_empty() {
        return; // warning
    }

    if dm == "sddm" && !installed_names.is_empty() {
        let mut theme_name = installed_names[0].clone();
        let mut src = join(data_dir, &theme_name);
        if !env.fs.is_dir(&src) {
            // no subfolder — data/ itself is the theme
            src = data_dir.to_string();
            theme_name = ext.ext_id();
        }
        if is_safe_sys_name(&theme_name) {
            stage_sddm_plan(env, &theme_name, &src, steps);
        }
        // an unsafe name — only the log
    } else if dm == "gdm" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            stage_login_bg_plan(env, "gdm-bg", &bg, steps);
        }
    } else if dm == "lightdm" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            let greeter = lightdm_greeter(env.fs);
            match lightdm_background_way(&greeter) {
                LightdmBackground::GreeterConfig => stage_login_bg_plan(env, "lightdm-bg", &bg, steps),
                LightdmBackground::DeepinAccount => {
                    // The picture — into the shared directory (the login screen reads it as
                    // its own user), then — into the account.
                    stage_login_bg_plan(env, "login-bg", &bg, steps);
                    steps.push(deepin_greeter_background_step());
                }
                LightdmBackground::UserWallpaper => steps.push(ExtStep::Notice {
                    text: tr("The Pantheon login screen (io.elementary.greeter) shows each user's own desktop \
                              wallpaper and has no background of its own: set the picture as your wallpaper, \
                              and it will show up on the login screen too."),
                }),
                LightdmBackground::Unknown => steps.push(ExtStep::Notice {
                    text: trf(
                        "The login screen “%1” keeps its background in settings Rexon doesn't know, so the \
                         login screen background was not changed.",
                        &[&greeter],
                    ),
                }),
            }
        }
        for name in installed_names {
            let theme_dir = join(data_dir, name);
            if env.fs.is_dir(&theme_dir)
                && env.fs.is_file(&join(&theme_dir, "index.html"))
            {
                if is_safe_sys_name(name) {
                    stage_dir_plan(
                        env,
                        "lightdm-webkit",
                        name,
                        &theme_dir,
                        "/usr/share/lightdm-webkit2-greeter/themes",
                        "rexon_lightdm_web",
                        steps,
                    );
                }
                break; // and on an unsafe name — break, as in Python
            }
        }
    } else if dm == "cosmic-greeter" {
        let bg = find_background(env.fs, data_dir);
        if !bg.is_empty() {
            stage_login_bg_plan(env, "cosmic-greeter", &bg, steps);
        }
    }
    // other DMs — only the log
}

/// Where the LightDM greeter takes the background from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightdmBackground {
    /// `background=` in its own settings file: slick-greeter
    /// (`/etc/lightdm/slick-greeter.conf`), lightdm-gtk-greeter
    /// (`/etc/lightdm/lightdm-gtk-greeter.conf`). The same — when the greeter
    /// could not be found out: the old behavior.
    GreeterConfig,
    /// lightdm-deepin-greeter: the account background is `GreeterBackground` of the
    /// `org.deepin.dde.Accounts1` service (the user changes their own without a password).
    DeepinAccount,
    /// io.elementary.greeter: it has no background of its own — it shows each user
    /// their desktop wallpaper (a copy is put by
    /// io.elementary.settings-daemon when the wallpaper changes).
    UserWallpaper,
    /// Another greeter: Rexon does not know where it looks.
    Unknown,
}

/// LightDM settings files in the order LightDM itself reads them: the
/// `lightdm.conf.d` directories (the system one, then our own), then `lightdm.conf`; a later one
/// overrides an earlier one.
pub const LIGHTDM_CONF_DIRS: &[&str] = &["/usr/share/lightdm/lightdm.conf.d", "/etc/lightdm/lightdm.conf.d"];
pub const LIGHTDM_CONF: &str = "/etc/lightdm/lightdm.conf";
/// Greeter descriptions: `<greeter-session>.desktop`, the `Exec=` line.
pub const XGREETERS_DIR: &str = "/usr/share/xgreeters";

/// `greeter-session` from the LightDM settings text (the `[Seat:*]` sections and
/// the old `[SeatDefaults]`).
fn lightdm_greeter_session_in(text: &str) -> Option<String> {
    let mut in_seat = false;
    let mut found = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_seat = l == "[Seat:*]" || l == "[SeatDefaults]";
            continue;
        }
        if !in_seat {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim() == "greeter-session" && !v.trim().is_empty() {
                found = Some(v.trim().to_string());
            }
        }
    }
    found
}

/// Which greeter LightDM will show: the program name from `Exec=` of the description
/// `/usr/share/xgreeters/<greeter-session>.desktop` (on Debian the name is
/// the generic `lightdm-greeter`, and the description is a link to the real one), otherwise the
/// session name itself. Empty — set nowhere.
pub fn lightdm_greeter(fs: &FsSnap) -> String {
    let mut files: Vec<String> = Vec::new();
    for dir in LIGHTDM_CONF_DIRS {
        let mut names: Vec<&String> = fs.scandir(dir).iter().filter(|n| n.ends_with(".conf")).collect();
        names.sort();
        files.extend(names.into_iter().map(|n| join(dir, n)));
    }
    files.push(LIGHTDM_CONF.to_string());
    let session = files
        .iter()
        .filter_map(|f| fs.read_text(f).and_then(lightdm_greeter_session_in))
        .next_back()
        .unwrap_or_default();
    if session.is_empty() {
        return session;
    }
    let exec = fs.read_text(&format!("{XGREETERS_DIR}/{session}.desktop")).and_then(|d| {
        d.lines()
            .find_map(|l| l.trim().strip_prefix("Exec="))
            .and_then(|e| e.split_whitespace().next())
            .map(|e| e.rsplit('/').next().unwrap_or(e).to_string())
    });
    exec.filter(|e| !e.is_empty()).unwrap_or(session)
}

/// Where to write the background for the greeter `greeter` ([`lightdm_greeter`]).
pub fn lightdm_background_way(greeter: &str) -> LightdmBackground {
    match greeter {
        "" | "slick-greeter" | "lightdm-slick-greeter" | "lightdm-gtk-greeter" => LightdmBackground::GreeterConfig,
        "lightdm-deepin-greeter" => LightdmBackground::DeepinAccount,
        "io.elementary.greeter" | "pantheon-greeter" => LightdmBackground::UserWallpaper,
        _ => LightdmBackground::Unknown,
    }
}

/// The Deepin login screen background for our own account: `SetGreeterBackground`
/// of the `org.deepin.dde.Accounts1` service (the system bus; the service changes our own
/// record without a password). The picture is the one the helper has just put into
/// `/usr/share/backgrounds/rexon-login.*` (it removes previous ones, so it is
/// the only one there; it decides the extension). The object path is `User<uid>`.
pub fn deepin_greeter_background_step() -> ExtStep {
    ExtStep::RunNoted {
        argv: vec![
            "sh".into(),
            "-c".into(),
            DEEPIN_GREETER_BG_SCRIPT.into(),
        ],
        timeout_secs: Some(30),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        failed: tr("Deepin's account service didn't take the login screen background — set it in Control \
                    Center → Personalization → Wallpaper."),
    }
}

const DEEPIN_GREETER_BG_SCRIPT: &str = "set -- /usr/share/backgrounds/rexon-login.*; [ -f \"$1\" ] || exit 1; \
     exec busctl --system call org.deepin.dde.Accounts1 \"/org/deepin/dde/Accounts1/User$(id -u)\" \
     org.deepin.dde.Accounts1.User SetGreeterBackground s \"file://$1\"";

/// Port of `_stage_login_bg`: staging the background + the named helper or legacy pkexec.
fn stage_login_bg_plan(env: &ExtEnv, category: &str, bg: &str, steps: &mut Vec<ExtStep>) {
    if !env.fs.is_file(bg) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![bg.to_string()],
    });
    let named = privileged_stage_cmd(
        category,
        "rexon-login",
        &staged,
        true,
        Some(&staged),
        env.stage_helper_available,
    );
    if let Some(argv) = named {
        steps.push(ExtStep::Run { argv, timeout_secs: Some(120) });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: format!("rexon_{category}"),
            script: login_bg_legacy_script(category, bg),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// The text of the legacy script `_stage_login_bg_legacy` (byte for byte).
pub fn login_bg_legacy_script(category: &str, bg: &str) -> String {
    // os.path.splitext(bg)[1].lstrip(".").lower(): the basename's extension;
    // leading dots of the name do not count as a separator (as in CPython).
    let base = bg.rsplit('/').next().unwrap_or(bg);
    let raw_ext = match base.rfind('.') {
        Some(i) if base[..i].chars().any(|c| c != '.') => &base[i..],
        _ => "",
    };
    let ext = raw_ext.trim_start_matches('.').to_lowercase();
    // ^[a-z0-9]+$ otherwise png (as in Python).
    let ext = if !ext.is_empty()
        && ext.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        ext
    } else {
        "png".to_string()
    };
    let dest_bg = format!("/usr/share/backgrounds/rexon-login.{ext}");
    let qbg = shlex_quote(bg);
    let qdest = shlex_quote(&dest_bg);
    let mut lines: Vec<String> = vec![
        "#!/bin/bash".into(),
        "set -e".into(),
        "install -d /usr/share/backgrounds".into(),
        "rm -f /usr/share/backgrounds/rexon-login.* 2>/dev/null || true".into(),
        format!("cp -f {qbg} {qdest}"),
        format!("chmod 644 {qdest}"),
    ];
    if category == "gdm-bg" {
        lines.extend([
            "install -d /etc/dconf/db/gdm.d".to_string(),
            "cat > /etc/dconf/db/gdm.d/02-rexon-login <<REXON_EOF".to_string(),
            "[org/gnome/desktop/background]".to_string(),
            format!("picture-uri='file://{dest_bg}'"),
            format!("picture-uri-dark='file://{dest_bg}'"),
            "picture-options='zoom'".to_string(),
            "REXON_EOF".to_string(),
            "dconf update 2>/dev/null || true".to_string(),
        ]);
    } else if category == "lightdm-bg" {
        lines.extend([
            "if [ -f /etc/lightdm/slick-greeter.conf ]; then".to_string(),
            format!(
                "  sed -i 's|^background=.*|background={dest_bg}|' \
                 /etc/lightdm/slick-greeter.conf"
            ),
            "elif [ -d /etc/lightdm ]; then".to_string(),
            "  cat >> /etc/lightdm/slick-greeter.conf <<REXON_EOF".to_string(),
            "[Greeter]".to_string(),
            format!("background={dest_bg}"),
            "REXON_EOF".to_string(),
            "fi".to_string(),
            "if [ -f /etc/lightdm/lightdm-gtk-greeter.conf ]; then".to_string(),
            format!(
                "  sed -i 's|^background=.*|background={dest_bg}|' \
                 /etc/lightdm/lightdm-gtk-greeter.conf"
            ),
            "fi".to_string(),
        ]);
    }
    lines.join("\n") + "\n"
}

/// Port of `_stage_dir`: a theme directory → <dest_root>/<name> under root.
#[allow(clippy::too_many_arguments)]
fn stage_dir_plan(
    env: &ExtEnv,
    category: &str,
    name: &str,
    src_dir: &str,
    dest_root: &str,
    prefix: &str,
    steps: &mut Vec<ExtStep>,
) {
    if !is_safe_sys_name(name) || !env.fs.is_dir(src_dir) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src_dir.to_string()],
    });
    let named = privileged_stage_cmd(
        category,
        name,
        &staged,
        true,
        Some(&staged),
        env.stage_helper_available,
    );
    if let Some(argv) = named {
        steps.push(ExtStep::Run { argv, timeout_secs: Some(300) });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: prefix.to_string(),
            script: stage_dir_fallback_script(name, src_dir, dest_root),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// The text of the `_stage_dir` fallback script (byte for byte).
pub fn stage_dir_fallback_script(name: &str, src_dir: &str, dest_root: &str) -> String {
    let dest = format!("{dest_root}/{name}");
    let qsrc = shlex_quote(src_dir);
    let qdest = shlex_quote(&dest);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
    ]
    .join("\n")
        + "\n"
}

/// Port of `_stage_sddm`: the system helper org.rexon.system-install or
/// the hardened raw pkexec.
fn stage_sddm_plan(env: &ExtEnv, theme_name: &str, src: &str, steps: &mut Vec<ExtStep>) {
    if !is_safe_sys_name(theme_name) || !env.fs.is_dir(src) {
        return;
    }
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src.to_string()],
    });
    if env.system_helper_ok() {
        steps.push(ExtStep::Run {
            argv: [
                vec!["pkexec".to_string(), env.system_helper_path.to_string()],
                // The language of the helper's messages (decision of 30.09.2026).
                crate::system_l10n::lang_args(env.system_helper_path),
                vec![
                    "sddm".to_string(),
                    theme_name.to_string(),
                    staged.clone(),
                    env.helper_family.to_string(),
                ],
            ]
            .concat(),
            timeout_secs: Some(300),
        });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: "rexon_sddm".to_string(),
            script: sddm_fallback_script(theme_name, src),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// The text of the `_stage_sddm` fallback script: only copying the theme, without the choice.
pub fn sddm_fallback_script(theme_name: &str, src: &str) -> String {
    let dest = format!("/usr/share/sddm/themes/{theme_name}");
    let qsrc = shlex_quote(src);
    let qdest = shlex_quote(&dest);
    let qtheme = shlex_quote(theme_name);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
        // Choosing the theme (`zz-rexon-theme.conf`) is the «Apply» button of the login
        // screen section (decision of 04.10.2026), installation only puts the files.
        format!("echo \"SDDM theme installed: \"{qtheme}"),
    ]
    .join("\n")
        + "\n"
}

/// Port of `_install_plymouth` + `_stage_plymouth`.
fn install_plymouth_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    installed_names: &[String],
    steps: &mut Vec<ExtStep>,
) {
    let names: Vec<String> = if installed_names.is_empty() {
        vec![ext.ext_id()]
    } else {
        installed_names.to_vec()
    };
    let theme_name = names[0].clone();
    let mut src = join(data_dir, &theme_name);
    if !env.fs.is_dir(&src) {
        src = data_dir.to_string();
    }

    let has_plymouth_conf = env.fs.scandir(&src).iter().any(|f| {
        f.ends_with(".plymouth") && env.fs.is_file(&join(&src, f))
    });

    if !has_plymouth_conf {
        // Auto-generating .plymouth (+ a simple .script when there is a picture).
        let logo = env
            .fs
            .scandir(&src)
            .iter()
            .find(|f| {
                let l = f.to_lowercase();
                l.ends_with(".png") || l.ends_with(".svg")
            })
            .cloned()
            .unwrap_or_default();
        steps.push(ExtStep::WriteFile {
            path: join(&src, &format!("{theme_name}.plymouth")),
            content: plymouth_autogen_conf(&ext.name(), &ext.description(), &theme_name),
        });
        if !logo.is_empty() {
            steps.push(ExtStep::WriteFile {
                path: join(&src, &format!("{theme_name}.script")),
                content: plymouth_autogen_script(&logo),
            });
        }
    }

    // The initramfs is not rebuilt here: the theme is only put in place, and it is chosen
    // (and the image rebuilt) by the «Apply» button (decision of 04.10.2026).

    if !is_safe_sys_name(&theme_name) {
        return; // an error in the log
    }
    // _stage_plymouth (the guard is already passed; src is a directory by construction).
    let staged = env.tmp.mkdtemp("rexon_stage_");
    steps.push(ExtStep::Stage {
        staged: staged.clone(),
        sources: vec![src.clone()],
    });
    if env.system_helper_ok() {
        steps.push(ExtStep::Run {
            argv: [
                vec!["pkexec".to_string(), env.system_helper_path.to_string()],
                // The language of the helper's messages (decision of 30.09.2026).
                crate::system_l10n::lang_args(env.system_helper_path),
                vec![
                    "plymouth".to_string(),
                    theme_name.clone(),
                    staged.clone(),
                    env.helper_family.to_string(),
                ],
            ]
            .concat(),
            timeout_secs: Some(900),
        });
    } else {
        steps.push(ExtStep::PkexecScript {
            prefix: "rexon_plymouth".to_string(),
            script: plymouth_fallback_script(&theme_name, &src),
        });
    }
    steps.push(ExtStep::RemoveTree {
        path: staged,
        safe: false,
        ignore_errors: true,
    });
}

/// An auto-generated `.plymouth` config (byte for byte with Python).
pub fn plymouth_autogen_conf(name: &str, description: &str, theme_name: &str) -> String {
    let desc = if description.is_empty() {
        "Rexon Plymouth Theme"
    } else {
        description
    };
    format!(
        "[Plymouth Theme]\n\
         Name={name}\n\
         Description={desc}\n\
         ModuleName=script\n\n\
         [script]\n\
         ImageDir=/usr/share/plymouth/themes/{theme_name}\n\
         ScriptFile=/usr/share/plymouth/themes/{theme_name}/{theme_name}.script\n"
    )
}

/// An auto-generated Plymouth script (a centered logo, byte for byte).
pub fn plymouth_autogen_script(logo: &str) -> String {
    format!(
        "logo = Image(\"{logo}\");\n\
         logo_sprite = Sprite(logo);\n\
         logo_sprite.SetX(Window.GetWidth() / 2 - logo.GetWidth() / 2);\n\
         logo_sprite.SetY(Window.GetHeight() / 2 - logo.GetHeight() / 2);\n\
         \n\
         fun refresh_callback() {{\n\
         \x20   logo_sprite.SetOpacity(1);\n\
         }}\n\
         Plymouth.SetRefreshFunction(refresh_callback);\n"
    )
}

/// The text of the `_stage_plymouth` fallback script: only copying the theme.
///
/// Choosing the theme (`plymouth-set-default-theme`) and rebuilding the kernel image are
/// the «Apply» button of the boot splash section via the system helper
/// (decision of 04.10.2026); previously installation did them right away.
pub fn plymouth_fallback_script(theme_name: &str, src: &str) -> String {
    let dest = format!("/usr/share/plymouth/themes/{theme_name}");
    let qsrc = shlex_quote(src);
    let qdest = shlex_quote(&dest);
    [
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("rm -rf {qdest}"),
        format!("install -d {qdest}"),
        format!("cp -rT {qsrc} {qdest}"),
        format!("chmod -R a+rX {qdest}"),
    ]
    .join("\n")
        + "\n"
}

/// The package is a Conky widget set saved by Rexon to a file
/// ([`crate::conky_sets::SET_MANIFEST_KEY`]).
fn is_conky_set(ext: &ExtInfo) -> bool {
    ext.manifest.get(crate::conky_sets::SET_MANIFEST_KEY).is_some_and(Value::is_object)
}

/// The fonts directory of the widget set `id`.
fn conky_set_fonts_dir(home: &str, id: &str) -> String {
    format!("{home}/.local/share/fonts/rexon-conky-{id}")
}

/// A widget set from a file: the `data/` subfolders have already gone in as their own widgets
/// (`~/.config/conky/<folder>`, with an owner mark); here — the folder of the set
/// itself `data/<id>` (`.cmtheme` and the wallpaper) and fonts from the widget folders — into
/// one directory of its own, as for a theme pack. The old path copied the WHOLE
/// `data/` also into `~/.config/conky/<id>`, and each widget of the set
/// was shown twice.
fn install_conky_set_plan(env: &ExtEnv, ext: &ExtInfo, data_dir: &str, pfs: &mut PlanFs, steps: &mut Vec<ExtStep>) {
    let id = ext.ext_id();
    let dest_dir = format!("{}/.config/conky/{id}", env.home);
    let set_src = join(data_dir, &id);
    if env.fs.is_dir(&set_src) {
        if pfs.exists(&dest_dir) {
            steps.push(ExtStep::RemoveTree { path: dest_dir.clone(), safe: true, ignore_errors: true });
        }
        steps.push(ExtStep::CopyTree { src: set_src, dest: dest_dir.clone() });
        pfs.mk_dir(&dest_dir);
    }
    let fonts: Vec<String> = env
        .fs
        .walk(data_dir)
        .into_iter()
        .flat_map(|(root, _, files)| files.into_iter().filter(|f| is_supported_font(f)).map(move |f| join(&root, &f)))
        .collect();
    if !fonts.is_empty() {
        let fonts_dir = conky_set_fonts_dir(&env.home, &id);
        steps.push(ExtStep::MakeDirs { path: fonts_dir.clone() });
        pfs.mk_dir(&fonts_dir);
        for src in fonts {
            let dst = join(&fonts_dir, src.rsplit('/').next().unwrap_or(&src));
            if !pfs.exists(&dst) {
                steps.push(ExtStep::CopyFile { src, dest: dst.clone() });
                pfs.mk_file(&dst);
            }
        }
        steps.push(run(vec!["fc-cache", "-f"], 30));
    }
}

/// Port of `_install_conky` (without launching — Popen as a Spawn step).
fn install_conky_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    if is_conky_set(ext) {
        return install_conky_set_plan(env, ext, data_dir, pfs, steps);
    }
    let theme_id = ext.ext_id();
    let dest_dir = format!("{}/.config/conky/{}", env.home, theme_id);

    if pfs.exists(&dest_dir) {
        steps.push(ExtStep::RemoveTree {
            path: dest_dir.clone(),
            safe: true,
            ignore_errors: true,
        });
    }
    steps.push(ExtStep::CopyTree {
        src: data_dir.to_string(),
        dest: dest_dir.clone(),
    });
    pfs.mk_dir(&dest_dir);

    // Find the Conky config. The dest_dir listing comes from the snapshot: it records what
    // Python saw AFTER its copytree.
    let mut conky_conf = String::new();
    for f in env.fs.scandir(&dest_dir) {
        if f == "conky.conf" || f == ".conkyrc" || f.ends_with(".conkyrc") {
            conky_conf = join(&dest_dir, f);
            break;
        }
    }
    if conky_conf.is_empty() {
        'outer: for (root, _dirs, files) in env.fs.walk(&dest_dir) {
            for f in &files {
                if f.to_lowercase().contains("conky")
                    && (f.ends_with(".conf") || f.ends_with(".conkyrc"))
                {
                    conky_conf = join(&root, f);
                    break 'outer;
                }
            }
        }
    }
    if conky_conf.is_empty() {
        return; // warning
    }

    // The dangerous directives check — only the log, no steps.

    // Fonts (fonts/ inside dest — copied by our own step → observations).
    let fonts_dir = join(&dest_dir, "fonts");
    if env.fs.obs_is_dir(&fonts_dir) {
        let user_fonts = format!("{}/.local/share/fonts", env.home);
        steps.push(ExtStep::MakeDirs { path: user_fonts.clone() });
        pfs.mk_dir(&user_fonts);
        for f in env.fs.scandir(&fonts_dir) {
            if is_supported_font(f) {
                let dst = join(&user_fonts, f);
                if !pfs.exists(&dst) {
                    steps.push(ExtStep::CopyFile {
                        src: join(&fonts_dir, f),
                        dest: dst.clone(),
                    });
                    pfs.mk_file(&dst);
                }
            }
        }
        steps.push(run(vec!["fc-cache", "-f"], 30));
    }

    // ⛔ Starting and autostart are the «▶ Start» button and the
    // «Start at login» switch of the «Appearance» tab (`crate::de_apply`, decision
    // of 04.10.2026): installation only puts the widget and its fonts. The old
    // branch (autostart at installation, `pkill -f conky` against ALL conky, including
    // foreign ones, and an immediate start) became unreachable after that decision and
    // was removed; the Wayland caveat comes in the result of the «Start» button.
}

/// The autostart `.desktop` text for Conky.
///
/// The widget is started from its settings file's folder — the same as with the
/// «Start» button ([`crate::de_apply::conky_work_dir`]). Not all autostart launchers
/// know the `Path=` key, so changing to the folder is done by the
/// launch line itself: `sh -c` with a permanent script
/// ([`crate::de_apply::CONKY_AUTOSTART_SCRIPT`]), the path as a separate
/// argument by the `Exec=` rules ([`crate::installer::desktop_exec_arg`]).
/// `Path=` stays as a hint for those who read it. The old line
/// `Exec=conky -c '<path>'` started the widget from the home folder (its
/// relative paths went astray), and an apostrophe in the folder name broke it.
pub fn conky_desktop_entry(name: &str, conky_conf: &str) -> String {
    conky_desktop_entry_via(name, conky_conf, None)
}

/// [`conky_desktop_entry`] launching through the Rexon overlay `launch`
/// ([`crate::conky_overlay`]): conky gets the overlay, and the working directory is
/// still the folder of the author's file
/// ([`crate::de_apply::CONKY_OVERLAY_AUTOSTART_SCRIPT`]).
pub fn conky_desktop_entry_via(name: &str, conky_conf: &str, launch: Option<&str>) -> String {
    conky_desktop_entry_with(name, conky_conf, launch, None, 0, &[])
}

/// [`conky_desktop_entry_via`] with a delay at login and environments.
///
/// The delay is a `sleep` in the launch line itself, not `X-GNOME-Autostart-Delay`:
/// that key is executed only by GNOME, Cinnamon, MATE and Budgie, while KDE, XFCE,
/// LXQt and `systemd-xdg-autostart-generator` silently skip it. The number is
/// also in the key [`crate::conky_autostart::DELAY_KEY`], so that rewriting
/// the record does not lose it. `only_in` — `OnlyShowIn=` ("only on this
/// desktop"). `X-KDE-autostart-after=panel` — Plasma starts the widget
/// after the panel, and it does not end up under it.
pub fn conky_desktop_entry_with(
    name: &str,
    conky_conf: &str,
    launch: Option<&str>,
    xwayland: Option<&str>,
    delay: u32,
    only_in: &[String],
) -> String {
    use crate::installer::desktop_exec_arg;
    // In a Wayland session — via XWayland, where the Wayland output is no good
    // (`conky_overlay::xwayland_shell`; the session and the conky version are found out at login),
    // with the `xwayland` file where the widget window is below windows. A path with `%` is not put
    // into the script (launchers parse `%%` in `Exec=` quotes differently).
    let xwayland = xwayland.filter(|f| !f.contains('%'));
    let session = crate::conky_overlay::xwayland_shell(crate::conky_overlay::widget_needs_x11(conky_conf), xwayland);
    let (base, args) = match launch {
        None => (crate::de_apply::CONKY_AUTOSTART_SCRIPT, vec![desktop_exec_arg(conky_conf)]),
        Some(overlay) => (
            crate::de_apply::CONKY_OVERLAY_AUTOSTART_SCRIPT,
            vec![desktop_exec_arg(overlay), desktop_exec_arg(conky_conf)],
        ),
    };
    conky_desktop_entry_exec(
        name,
        crate::de_apply::conky_work_dir(conky_conf),
        &format!("{session}{base}"),
        &args,
        delay,
        only_in,
    )
}

/// A Conky widget autostart record: an `sh -c` script `body` with arguments `args`
/// already quoted for `Exec=`, `dir` — `Path=`. The delay is
/// `sleep N &&` at the start of the script (see [`conky_desktop_entry_with`]; the same
/// is used to write launching a theme with its `start.sh`, `conky_theme`).
pub fn conky_desktop_entry_exec(name: &str, dir: &str, body: &str, args: &[String], delay: u32, only_in: &[String]) -> String {
    use crate::installer::desktop_exec_arg;
    let script = if delay > 0 { format!("sleep {delay} && {body}") } else { body.to_string() };
    let mut exec = format!("sh -c {}", desktop_exec_arg(&script));
    for a in args {
        exec.push(' ');
        exec.push_str(a);
    }
    // A string key value: a backslash is doubled, a newline is `\n`.
    let dir = dir.replace('\\', "\\\\").replace('\n', "\\n");
    let mut extra = String::new();
    if !only_in.is_empty() {
        extra.push_str(&format!("OnlyShowIn={};\n", only_in.join(";")));
    }
    if delay > 0 {
        extra.push_str(&format!("{}={delay}\n", crate::conky_autostart::DELAY_KEY));
    }
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Conky — {name}\n\
         Comment=Rexon Conky widget: {name}\n\
         Exec={exec}\n\
         Path={dir}\n\
         {extra}\
         X-GNOME-Autostart-enabled=true\n\
         X-KDE-autostart-after=panel\n\
         StartupNotify=false\n\
         Terminal=false\n"
    )
}

/// What to install from the package directory `dir`: (a `.plasmoid` archive, a plasmoid
/// directory) — the first by name order; an empty string — none. `dir` itself
/// with a description is also a plasmoid directory.
fn plasmoid_source(env: &ExtEnv, dir: &str) -> (String, String) {
    let has_meta = |d: &str| env.fs.is_file(&join(d, "metadata.json")) || env.fs.is_file(&join(d, "metadata.desktop"));
    for f in env.fs.scandir(dir) {
        let full = join(dir, f);
        if f.ends_with(".plasmoid") {
            return (full, String::new());
        }
        if env.fs.is_dir(&full) && has_meta(&full) {
            return (String::new(), full);
        }
    }
    if has_meta(dir) {
        return (String::new(), dir.to_string());
    }
    (String::new(), String::new())
}

/// The id of the plasmoid directory `dir`: `KPlugin.Id` from metadata.json, otherwise
/// `X-KDE-PluginInfo-Name` from metadata.desktop.
fn plasmoid_dir_id(env: &ExtEnv, dir: &str) -> Option<String> {
    let meta_json = join(dir, "metadata.json");
    if env.fs.is_file(&meta_json) {
        return env
            .fs
            .read_text(&meta_json)
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .and_then(|v| v["KPlugin"]["Id"].as_str().map(String::from));
    }
    env.fs
        .read_text(&join(dir, "metadata.desktop"))
        .and_then(|t| ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name"))
}

/// The directory of the plasmoid `id` in the profile.
fn plasmoid_dest(env: &ExtEnv, id: &str) -> String {
    format!("{}/.local/share/plasma/plasmoids/{}", env.home, id)
}

/// `KPlugin.Id` (for old ones — `X-KDE-PluginInfo-Name`) of the first description in
/// the snapshot tree `root`.
fn plasmoid_tree_id(fs: &FsSnap, root: &str) -> Option<String> {
    for (dir, _dirs, files) in fs.walk(root) {
        if files.iter().any(|f| f == "metadata.json") {
            return json_field(fs, &join(&dir, "metadata.json"), "KPlugin.Id");
        }
        if files.iter().any(|f| f == "metadata.desktop") {
            return fs
                .read_text(&join(&dir, "metadata.desktop"))
                .and_then(|t| ini_get(t, "Desktop Entry", "X-KDE-PluginInfo-Name"));
        }
    }
    None
}

/// The plasmoid id in the archive `plasmoid_file` — from the unpacking the snapshot
/// did in advance ([`PLASMOID_PROBE_DIR`]). `None` — there is no probe (the archive
/// is broken) or there is no description in it.
fn plasmoid_archive_id(env: &ExtEnv, plasmoid_file: &str) -> Option<String> {
    let name = plasmoid_file.rsplit('/').next().unwrap_or(plasmoid_file);
    plasmoid_tree_id(env.fs, &env.tmp.path(&format!("{PLASMOID_PROBE_DIR}/{name}")))
}

/// In place of `dest` there is a plasmoid not put by this package (by hand,
/// by the store, by another package, by the old Rexon without a mark). A directory
/// put by this same plan (a shared copy of a `data/` subfolder with a name equal to the
/// id) is our own.
fn plasmoid_taken(env: &ExtEnv, ext: &ExtInfo, pfs: &PlanFs, dest: &str) -> bool {
    !pfs.made_dirs.contains(dest) && env.fs.lexists(dest) && !owned_by(env.fs, dest, &ext.ext_id())
}

/// Skipping the plasmoid `id`: a note with the reason.
fn skip_plasmoid(id: &str, dest: &str, pfs: &mut PlanFs, steps: &mut Vec<ExtStep>) {
    // A `data/` subfolder with a name equal to the id has already been skipped by the shared loop with
    // the same reason — do not repeat it a second time.
    let notice = taken_by_other_notice(id, dest);
    if !steps.contains(&notice) {
        steps.push(notice);
    }
    pfs.skipped += 1;
}

/// The owner mark in the plasmoid directory `dest`.
fn plasmoid_mark(ext: &ExtInfo, dest: &str) -> ExtStep {
    ExtStep::WriteFile { path: join(dest, OWNER_MARK), content: format!("{}\n", ext.ext_id()) }
}

/// Port of `_install_plasmoid` (+ `_install_plasmoid_manual`).
///
/// The owner rule (as for a theme's main copy): the directory
/// `~/.local/share/plasma/plasmoids/<KPlugin.Id>` of someone else's plasmoid with the same
/// id is neither removed nor updated by `kpackagetool -u` — the plasmoid
/// is skipped with a note. For a `.plasmoid` archive the id is known only after
/// unpacking: the plan's snapshot unpacks it in advance, so the decision
/// is made BEFORE the first launch of `kpackagetool` (the `run_ok` probes run
/// it already while building the plan).
fn install_plasmoid_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    data_dir: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let (plasmoid_file, plasmoid_dir) = plasmoid_source(env, data_dir);

    if !plasmoid_file.is_empty() {
        let probe_id = plasmoid_archive_id(env, &plasmoid_file);
        if let Some(id) = &probe_id {
            let dest = plasmoid_dest(env, id);
            if plasmoid_taken(env, ext, pfs, &dest) {
                skip_plasmoid(id, &dest, pfs, steps);
                return;
            }
        }
        let tool = (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"));
        if let Some(tool) = tool {
            // First update, then install.
            steps.push(ExtStep::Run {
                argv: vec![tool.clone(), "-u".into(), plasmoid_file.clone()],
                timeout_secs: Some(30),
            });
            let install_argv = vec![tool.clone(), "-i".into(), plasmoid_file.clone()];
            steps.push(ExtStep::Run {
                argv: install_argv.clone(),
                timeout_secs: Some(30),
            });
            if (env.run_ok)(&install_argv) {
                // A mark for a plasmoid installed by the tool too: by it
                // removal and a repeat recognize our own.
                if let Some(id) = &probe_id {
                    steps.push(plasmoid_mark(ext, &plasmoid_dest(env, id)));
                }
            } else {
                install_plasmoid_manual_plan(env, ext, &plasmoid_file, pfs, steps);
            }
        } else {
            install_plasmoid_manual_plan(env, ext, &plasmoid_file, pfs, steps);
        }
    } else if !plasmoid_dir.is_empty() {
        let plugin_id = plasmoid_dir_id(env, &plasmoid_dir).unwrap_or_else(|| ext.ext_id());
        let dest = plasmoid_dest(env, &plugin_id);
        if plasmoid_taken(env, ext, pfs, &dest) {
            skip_plasmoid(&plugin_id, &dest, pfs, steps);
            return;
        }
        if pfs.exists(&dest) {
            steps.push(ExtStep::RemoveTree {
                path: dest.clone(),
                safe: false,
                ignore_errors: false,
            });
        }
        steps.push(ExtStep::CopyTree { src: plasmoid_dir, dest: dest.clone() });
        pfs.mk_dir(&dest);
        steps.push(plasmoid_mark(ext, &dest));
    }
    // nothing found — a warning
}

/// Port of `_install_plasmoid_manual`: unpack the .plasmoid and copy.
/// Listings of the unpacked tree come from the snapshot (Python really unpacked).
/// Someone else's plasmoid with the same id — skipped with a note (nothing is unpacked).
fn install_plasmoid_manual_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    plasmoid_file: &str,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let tmp = env.tmp.mkdtemp("rexon_plasmoid_");

    let mut plugin_id = ext.ext_id();
    for (root, _dirs, files) in env.fs.walk(&tmp) {
        if files.iter().any(|f| f == "metadata.json") {
            if let Some(text) = env.fs.read_text(&join(&root, "metadata.json")) {
                if let Ok(v) = serde_json::from_str::<Value>(text) {
                    if let Some(id) = v["KPlugin"]["Id"].as_str() {
                        plugin_id = id.to_string();
                    }
                }
            }
            break;
        }
    }

    let dest = plasmoid_dest(env, &plugin_id);
    if plasmoid_taken(env, ext, pfs, &dest) {
        skip_plasmoid(&plugin_id, &dest, pfs, steps);
        return;
    }
    steps.push(ExtStep::ExtractZip {
        archive: plasmoid_file.to_string(),
        dest: tmp.clone(),
    });
    if env.fs.exists(&dest) {
        steps.push(ExtStep::RemoveTree {
            path: dest.clone(),
            safe: false,
            ignore_errors: false,
        });
    }

    let mut src = tmp.clone();
    for (root, dirs, files) in env.fs.walk(&tmp) {
        if files.iter().any(|f| f == "metadata.json")
            || dirs.iter().any(|d| d == "contents")
        {
            src = root;
            break;
        }
    }
    steps.push(ExtStep::CopyTree { src, dest: dest.clone() });
    pfs.mk_dir(&dest);
    steps.push(plasmoid_mark(ext, &dest));
    steps.push(ExtStep::RemoveTree {
        path: tmp,
        safe: false,
        ignore_errors: true,
    });
}

/// Port of `_install_bundle`: a full theme pack — by the types from the manifest's
/// `bundle` (the manifest's key order).
fn install_bundle_plan(
    env: &ExtEnv,
    ext: &ExtInfo,
    pfs: &mut PlanFs,
    steps: &mut Vec<ExtStep>,
) {
    let Some(bundle_map) = ext.manifest.get("bundle").and_then(Value::as_object) else {
        return; // warning: the 'bundle' field is missing
    };

    for (sub_type, sub_dir_rel) in bundle_map {
        if !constants::is_valid_type(sub_type) || sub_type == "theme-bundle" {
            continue;
        }
        let sub_dir = join(ext.dir, &py_str(sub_dir_rel));
        if !env.fs.is_dir(&sub_dir) {
            continue;
        }
        let Some(target_base) = env.install_path(sub_type) else {
            continue; // no install path (wallpaper-pack)
        };
        steps.push(ExtStep::MakeDirs { path: target_base.clone() });
        pfs.mk_dir(&target_base);

        match sub_type.as_str() {
            "theme-conky" => install_conky_plan(env, ext, &sub_dir, pfs, steps),
            "theme-plymouth" => {
                let names = subdir_names_or_id(env.fs, &sub_dir, &ext.ext_id());
                install_plymouth_plan(env, ext, &sub_dir, &names, steps);
            }
            "theme-login-screen" => {
                let names = subdir_names_or_id(env.fs, &sub_dir, &ext.ext_id());
                install_login_screen_plan(env, ext, &sub_dir, &names, steps);
            }
            "theme-window-decoration" => {
                let names = subdir_names(env.fs, &sub_dir);
                // The original's pitfall is kept: bundle does NOT copy decorations into
                // INSTALL_PATHS before the call — the GNOME/XFCE branches will not find
                // src and will copy nothing (the KDE branch writes
                // kwinrc anyway).
                install_window_decoration_plan(env, ext, &names, pfs, steps);
            }
            "theme-plasmoid" => install_plasmoid_plan(env, ext, &sub_dir, pfs, steps),
            "font-pack" => {
                for f in env.fs.scandir(&sub_dir) {
                    if is_supported_font(f) {
                        // The shared fonts directory: the person's font with the same name
                        // and different contents is not overwritten.
                        let dest = join(&target_base, f);
                        if !put_owned_file(env, pfs, &join(&sub_dir, f), &dest, steps) {
                            steps.push(taken_file_notice(f, &dest));
                            pfs.skipped += 1;
                        }
                    }
                }
                steps.push(run(vec!["fc-cache", "-f"], 30));
            }
            _ => {
                // The standard installation: subfolders and files → the target path.
                // A bundle part's theme goes under its own name into the shared
                // directory of its kind — by the owner rule, like the main copy of
                // a standalone installation: someone else's theme (file) with the same name is neither
                // removed nor overwritten, the reason goes into a note.
                for name in env.fs.scandir(&sub_dir) {
                    let entry = join(&sub_dir, name);
                    let dest = join(&target_base, name);
                    if env.fs.is_dir(&entry) {
                        if !put_owned_copy(env, ext, pfs, &entry, &dest, steps) {
                            steps.push(taken_by_other_notice(name, &dest));
                            pfs.skipped += 1;
                        }
                    } else if env.fs.is_file(&entry) && !put_owned_file(env, pfs, &entry, &dest, steps) {
                        steps.push(taken_file_notice(name, &dest));
                        pfs.skipped += 1;
                    }
                }
                // We do not apply a theme from a bundle either: it is chosen by
                // «Apply» of its section, as with a standalone installation.
            }
        }
    }
}

fn subdir_names(fs: &FsSnap, dir: &str) -> Vec<String> {
    fs.scandir(dir)
        .iter()
        .filter(|n| fs.is_dir(&join(dir, n)))
        .cloned()
        .collect()
}

fn subdir_names_or_id(fs: &FsSnap, dir: &str, ext_id: &str) -> Vec<String> {
    let names = subdir_names(fs, dir);
    if names.is_empty() {
        vec![ext_id.to_string()]
    } else {
        names
    }
}

// ============================================================================
// _safe_uninstall as a plan
// ============================================================================

/// The step removing a system theme (`theme-plymouth` / `theme-login-screen`)
/// installed from a `.rxn`: the name is the same the installation chose (the first
/// data subfolder or the extension id). Only if the helper exists and the theme
/// lies in `/usr/share/<kind>/themes` as a regular directory.
fn system_theme_remove_steps(
    env: &ExtEnv,
    ext: &ExtInfo,
    ext_type: &str,
    data_dir: &str,
    steps: &mut Vec<ExtStep>,
) {
    let kind = if ext_type == "theme-plymouth" { "plymouth" } else { "sddm" };
    let Some(name) = subdir_names_or_id(env.fs, data_dir, &ext.ext_id()).into_iter().next() else {
        return;
    };
    if !is_safe_sys_name(&name) || !env.system_helper_ok() {
        return;
    }
    let dest = format!("/usr/share/{kind}/themes/{name}");
    if !env.fs.is_dir(&dest) || env.fs.is_link(&dest) {
        return;
    }
    steps.push(ExtStep::Run {
        argv: crate::de_apply::system_remove_argv(
            env.system_helper_path,
            &crate::system_l10n::lang_args(env.system_helper_path),
            kind,
            &name,
            env.helper_family,
        ),
        timeout_secs: Some(crate::installer::SYSTEM_INSTALL_TIMEOUT_SECS),
    });
}

/// Port of `_safe_uninstall`: removing appearance files from system paths.
/// (The final `_safe_rmtree(ext.dir)` is done by `uninstall()` — outside this plan.)
pub fn safe_uninstall_plan(env: &ExtEnv, ext: &ExtInfo) -> Vec<ExtStep> {
    let mut steps = Vec::new();
    let ext_type = ext.ext_type();
    let target_base = env.install_path(&ext_type);

    // v4 adaptive: remove the symlink from the gallery.
    if ext_type == "wallpaper-video-adaptive" {
        if let Some(base) = target_base {
            let link = join(&base, &ext.ext_id());
            if env.fs.is_link(&link) {
                steps.push(ExtStep::RemoveFile { path: link });
            }
        }
        return steps;
    }

    if ext_type == "font-pack" {
        // Extension.remove_fonts.
        let dest_dir = format!(
            "{}/.local/share/fonts/rexon-{}",
            env.home,
            ext.ext_id()
        );
        if env.fs.is_dir(&dest_dir) {
            steps.push(ExtStep::RemoveTree {
                path: dest_dir,
                safe: false,
                ignore_errors: true,
            });
            steps.push(run(vec!["fc-cache", "-f"], 30));
        }
        return steps;
    }

    if ext_type == "theme-conky" {
        let conky_dir = format!("{}/.config/conky/{}", env.home, ext.ext_id());
        if env.fs.is_dir(&conky_dir) {
            steps.push(ExtStep::RemoveTree {
                path: conky_dir,
                safe: true,
                ignore_errors: true,
            });
        }
        let autostart = format!(
            "{}/.config/autostart/rexon-conky-{}.desktop",
            env.home,
            ext.ext_id()
        );
        if env.fs.is_file(&autostart) {
            steps.push(ExtStep::RemoveFile { path: autostart });
        }
        // The fonts of a widget set from a file — its own directory entirely.
        // (the plan's snapshot does not expand the shared fonts directory — the step always
        // runs, the executor skips a missing directory).
        if is_conky_set(ext) {
            let set_fonts = conky_set_fonts_dir(&env.home, &ext.ext_id());
            steps.push(ExtStep::RemoveTree { path: set_fonts, safe: true, ignore_errors: true });
            steps.push(run(vec!["fc-cache", "-f"], 30));
        }
        // Copies of `data/` subfolders in `~/.config/conky/<subfolder>` — only
        // our own, by the mark (a shared `fonts` may belong to another widget).
        if let Some(base) = &target_base {
            for name in subdir_names(env.fs, &join(ext.dir, "data")) {
                if name != ext.ext_id() {
                    remove_owned_copy(env, ext, &join(base, &name), &mut steps);
                }
            }
        }
        // The widget's processes are stopped by the executor BEFORE the plan — by the ids found
        // by the settings file (`de_apply::conky_forget_widget`). The old
        // `pkill -f conky.*<id>` matched a command line pattern and hit
        // any process where "conky" and the widget name stand next to each other.
        return steps;
    }

    if ext_type == "theme-plasmoid" {
        let data_dir = join(ext.dir, "data");
        // Determine the plugin id from metadata. The original's order is kept:
        // the outer loop over (data/, root) is NOT interrupted after the first
        // find — the second candidate may overwrite plugin_id (including
        // with None for a metadata.json without KPlugin.Id). But None no longer
        // means "the package id": then the id is taken by the same choice as in
        // installation (below) — removal takes away the directory installation
        // put, not the one named after the package (25.10.2026).
        let mut plugin_id: Option<String> = None;
        for candidate in [data_dir, ext.dir.to_string()] {
            for meta_name in ["metadata.json", "metadata.desktop"] {
                let meta_path = join(&candidate, meta_name);
                if env.fs.is_file(&meta_path) {
                    if meta_name.ends_with(".json") {
                        plugin_id = env
                            .fs
                            .read_text(&meta_path)
                            .and_then(|t| serde_json::from_str::<Value>(t).ok())
                            .and_then(|v| {
                                v["KPlugin"]["Id"].as_str().map(String::from)
                            });
                    }
                    // metadata.desktop — a fallback to ext_id (not parsed).
                    break;
                }
            }
        }
        // The id was not found — the same choice as in installation: a data/ subfolder with
        // a description or a `.plasmoid` archive (the id — from the unpacking the
        // snapshot did in advance). Otherwise a directory named after the
        // package id would be removed, not the plasmoid's.
        let plugin_id = plugin_id
            .or_else(|| {
                let (file, dir) = plasmoid_source(env, &join(ext.dir, "data"));
                if !file.is_empty() {
                    plasmoid_archive_id(env, &file)
                } else if !dir.is_empty() {
                    plasmoid_dir_id(env, &dir)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| ext.ext_id());
        let plasmoid_dir = plasmoid_dest(env, &plugin_id);
        // A plasmoid with this id is installed but was not put by this package (installation
        // skipped it; or a copy of the old Rexon without a mark — the shared log
        // removes it if the path is recorded there): neither `kpackagetool -r` nor
        // deletion.
        if env.fs.lexists(&plasmoid_dir) && !owned_by(env.fs, &plasmoid_dir, &ext.ext_id()) {
            return steps;
        }
        if let Some(tool) =
            (env.which)("kpackagetool6").or_else(|| (env.which)("kpackagetool5"))
        {
            steps.push(ExtStep::Run {
                argv: vec![tool, "-r".into(), plugin_id.clone()],
                timeout_secs: Some(30),
            });
        }
        if env.fs.is_dir(&plasmoid_dir) {
            steps.push(ExtStep::RemoveTree {
                path: plasmoid_dir,
                safe: true,
                ignore_errors: true,
            });
        }
        return steps;
    }

    // Plymouth / the login screen: the directory in /usr/share is removed by the system
    // helper (the remove-<kind> mode). It does not touch the currently selected theme and the
    // system package's files and answers with a reason.
    if ext_type == "theme-plymouth" || ext_type == "theme-login-screen" {
        system_theme_remove_steps(env, ext, &ext_type, &join(ext.dir, "data"), &mut steps);
        return steps;
    }

    // Desktop customization — the same generic path as for installation.
    if let Some(row) = de_customization::de_managed_row(&ext_type) {
        uninstall_de_custom_plan(env, ext, row, &mut steps);
        return steps;
    }

    if ext_type == "theme-bundle" {
        let bundle = ext
            .manifest
            .get("bundle")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let data_dir = join(ext.dir, "data");
        for (sub_type, sub_folder) in &bundle {
            if sub_type == "theme-plymouth" || sub_type == "theme-login-screen" {
                // The same directory the bundle installation takes.
                let sub_dir = join(ext.dir, &py_str(sub_folder));
                if env.fs.is_dir(&sub_dir) {
                    system_theme_remove_steps(env, ext, sub_type, &sub_dir, &mut steps);
                }
            }
            let Some(sub_path) = env.install_path(sub_type) else {
                continue;
            };
            let folder = py_str(sub_folder);
            let base_name = folder.rsplit('/').next().unwrap_or(&folder);
            let sub_data = join(&data_dir, base_name);
            if !env.fs.is_dir(&sub_data) {
                continue;
            }
            // Only our own: a directory with this package's mark, a file from its
            // log or byte for byte its own (installation skipped foreign ones).
            for name in env.fs.scandir(&sub_data) {
                let entry = join(&sub_data, name);
                let dest = join(&sub_path, name);
                if env.fs.is_dir(&entry) {
                    remove_owned_copy(env, ext, &dest, &mut steps);
                } else if env.fs.is_file(&entry) && file_is_ours(env.fs, &entry, &dest) {
                    steps.push(ExtStep::RemoveFile { path: dest });
                }
            }
        }
        // Copies of `data/` subfolders in `~/.local/share/rexon/bundles` — our own.
        if let Some(base) = &target_base {
            for name in subdir_names(env.fs, &data_dir) {
                remove_owned_copy(env, ext, &join(base, &name), &mut steps);
            }
        }
        return steps;
    }

    let Some(target_base) = target_base else {
        return steps;
    };
    let data_dir = join(ext.dir, "data");
    if !env.fs.is_dir(&data_dir) {
        return steps;
    }
    for name in env.fs.scandir(&data_dir) {
        let entry = join(&data_dir, name);
        let dest = join(&target_base, name);
        if env.fs.is_dir(&entry) {
            // Only our own copy (this package's mark): installation skipped the person's theme
            // with the same name — removal does not touch it either.
            // A copy of the old Rexon without a mark stays in place if the path is not
            // recorded in the shared log (then the log removes it).
            if owned_by(env.fs, &dest, &ext.ext_id()) {
                steps.push(ExtStep::RemoveTree {
                    path: dest,
                    safe: false,
                    ignore_errors: true,
                });
            }
            // Frames outside KDE: the copy in `~/.themes/<theme>` — if it is ours
            // (the desktop might have changed after installation, so without regard to
            // the current one).
            if ext_type == "theme-window-decoration" {
                remove_owned_copy(env, ext, &format!("{}/.themes/{name}", env.home), &mut steps);
            }
        } else if env.fs.is_file(&entry) && file_is_ours(env.fs, &entry, &dest) {
            // KDE color schemes (.colors files) — only our own file: installation skipped
            // the person's scheme with the same name.
            steps.push(ExtStep::RemoveFile { path: dest });
        }
    }
    steps
}

// ============================================================================
// Discovery, catalog, state
// ============================================================================

/// Port of `_discover`: scanning the extensions directory. Returns a list in the
/// insertion order of a Python dict; a directory with a duplicate id OVERWRITES an
/// earlier one in place (the semantics of dict[ext_id] = ext — the pitfall is kept).
/// Broken extensions are skipped (like `except Exception`).
pub fn discover(
    fs: &FsSnap,
    extensions_dir: &str,
    state: &Map<String, Value>,
) -> Vec<Extension> {
    let mut order: Vec<String> = Vec::new();
    let mut by_id: HashMap<String, Extension> = HashMap::new();
    for name in fs.scandir(extensions_dir) {
        let dir = join(extensions_dir, name);
        if !fs.is_dir(&dir) {
            continue;
        }
        let manifest_path = join(&dir, "manifest.json");
        if !fs.is_file(&manifest_path) {
            continue;
        }
        let Some(text) = fs.read_text(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = serde_json::from_str::<Value>(text) else {
            continue; // JSONDecodeError → log.exception + skip
        };
        let Ok(ext) = Extension::from_manifest(manifest, &dir, state) else {
            continue; // ValueError → log.exception + skip
        };
        let id = ext.ext_id().to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, ext);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// Port of `get_stats`: counts by types and categories.
/// The by_type key order is the order of `VALID_TYPES` (in Python it is a set, the order
/// of its iteration; comparing JSON objects is order-insensitive).
pub fn stats(exts: &[(String, bool)]) -> Map<String, Value> {
    let total = exts.len();
    let enabled = exts.iter().filter(|(_, e)| *e).count();
    let mut by_type = Map::new();
    for t in constants::valid_types() {
        let n = exts.iter().filter(|(ty, _)| ty == t).count();
        by_type.insert(t.to_string(), Value::from(n));
    }
    let appearance = exts
        .iter()
        .filter(|(ty, _)| constants::is_appearance(ty))
        .count();
    let extensions = exts
        .iter()
        .filter(|(ty, _)| constants::is_extension(ty))
        .count();
    let mut by_category = Map::new();
    by_category.insert("appearance".to_string(), Value::from(appearance));
    by_category.insert("extensions".to_string(), Value::from(extensions));

    let mut out = Map::new();
    out.insert("total".to_string(), Value::from(total));
    out.insert("enabled".to_string(), Value::from(enabled));
    out.insert("by_type".to_string(), Value::Object(by_type));
    out.insert("by_category".to_string(), Value::Object(by_category));
    out
}

/// Port of `_save_state` (the text part): `json.dump(state, indent=2,
/// ensure_ascii=False)` — byte for byte. The order of pairs is the order of the
/// `self.extensions` dictionary.
pub fn save_state_text(state: &[(String, bool)]) -> String {
    let mut m = Map::new();
    for (id, enabled) in state {
        m.insert(id.clone(), Value::Bool(*enabled));
    }
    dumps_indent2(&Value::Object(m))
}

/// Port of `_load_state`: parsing the JSON state; any problems → an empty dict.
pub fn load_state(text: Option<&str>) -> Map<String, Value> {
    let Some(text) = text else {
        return Map::new();
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// `json.dumps(v, indent=2, ensure_ascii=False)` — serde `to_string_pretty`
/// with `preserve_order` matches byte for byte (as in ext_utils::write_manifest).
fn dumps_indent2(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("Value сериализуется")
}

/// Python `str(value)` for manifest string values: strings as is
/// (non-strings do not occur in real manifests — we take them as a JSON string
/// or Display).
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Python truthiness of Option<&Value> (for `meta.get("id")` and the like).
fn is_truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Port of `shlex.quote`: empty → `''`; safe characters — as is; otherwise
/// single quotes with `'` replaced by `'"'"'`.
pub fn shlex_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    let safe = s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(c, '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-' | '_')
    });
    if safe {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\"'\"'"))
    }
}

/// A minimal `.desktop`/INI parse for `configparser.get(section, key)`
/// (strict=False, keys case-insensitive — optionxform=str.lower).
fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let want_key = key.to_lowercase();
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = &line[1..line.len() - 1] == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((k, v)) = line.split_once('=').or_else(|| line.split_once(':')) {
            if k.trim().to_lowercase() == want_key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}
