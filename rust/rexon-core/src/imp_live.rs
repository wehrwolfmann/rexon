//! `LiveImpIo` — the REAL executor of the [`crate::importers::ImpIo`] trait.
//!
//! WHY A SEPARATE MODULE. The logic of the importers of foreign formats
//! (`.themepack`, `.theme`, `.msstyles`, cursors, icons) is fully ported
//! (`crate::importers`), but it is INTERACTIVE: the course of parsing depends on
//! the output of `convert`/`ffmpeg`/`wrestool`/`7z` and on the files that appear
//! after them. So it has no static plan — only the `ImpIo` trait, behind which
//! stand the live file system and live subprocesses.
//!
//! Until 07.09.2026 the only live implementation of the trait lived in the PyO3
//! bridge (`rust/rexon-py/src/lib.rs`), i.e. importing a Windows theme worked
//! ONLY in a process with Python. Because of that `ext_exec::install_from_file`
//! could install only `.rxn`, and the "Extensions" tab in a process without
//! Python lost half of what its own file-dialog filter promises
//! (`qt_extensions::FILE_DIALOG_FILTER`). The executor moved here in full and
//! byte for byte: the bridge now takes it from here too, there is no second
//! copy.
//!
//! THE ONLY SEAM TO THE OUTSIDE is wallpaper upscaling: `upscaler.rs` runs its
//! own flow through the `UpIo` trait, and its live executor may differ (in a
//! process with Python — the one that knows Pillow and downloading
//! Real-ESRGAN). That is why [`LiveImpIo`] takes upscaling as a CLOSURE: `None`
//! — do not upscale at all (that is how `can_import` behaves, which needs no
//! side effects).
//!
//! ⭐ NO SKIPPED STEP HERE IS SILENT. Importing a foreign theme relies on
//! third-party programs, and some of them may be missing. This used to end in a
//! silent loss: wallpapers were copied as is, no preview was built, the person
//! saw "✅ Installed" and decided that was how it should be. Now every skip goes
//! into [`Notes`] and reaches the tab's status line.
//!
//! Warning: the tool synonym tables below are the same as `utils::TOOL_SETS`,
//! but they are NOT them: the set used here is deliberately the one with which
//! the bridge parity reference was captured
//! (`rust/rexon-py/tests/importers_roundtrip.py`). Merging them into one is
//! separate work with its own measurement, not a side effect of the move.

use crate::ext_utils as core_ext;
use crate::importers as core_imp;

/// Wallpaper upscaling closure: `f(data_dir) -> how many were processed`.
///
/// Port of `WallpaperUpscaler.process_wallpapers_in_dir`. It lives outside
/// because the upscaler's live executor depends on the process (see the module
/// header).
pub type UpscaleFn = Box<dyn FnMut(&str) -> u64 + Send>;

/// A collector of warnings about SKIPPED optional import steps.
///
/// Shared across the whole pipeline: the upscaling closure runs inside the
/// import, and what it wrote has to be read outside, after the import ends. A
/// skip nobody learned about is exactly the silent failure the collector was
/// created for.
pub type Notes = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// An empty warning collector.
pub fn new_notes() -> Notes {
    std::sync::Arc::new(std::sync::Mutex::new(Vec::new()))
}

/// Record a warning if there is no such one yet.
///
/// Duplicates are suppressed on purpose: a theme may have a dozen wallpapers,
/// and the same line "nothing to determine the size with" ten times in a row is
/// not information but a wall of text (rule #13).
fn note(notes: &Notes, text: String) {
    if let Ok(mut list) = notes.lock() {
        if !list.iter().any(|t| *t == text) {
            list.push(text);
        }
    }
}

/// Take what was collected and leave the collector empty.
pub fn take_notes(notes: &Notes) -> Vec<String> {
    notes
        .lock()
        .map(|mut list| std::mem::take(&mut *list))
        .unwrap_or_default()
}


/// The real ImpIo executor: actual file-system operations and subprocesses.
pub struct LiveImpIo {
    /// Wallpaper upscaling (None — only can_import is possible).
    upscaler: Option<UpscaleFn>,
    /// The first error of a side-effecting operation (text for OSError).
    pub io_err: Option<String>,
    /// Counter for unique mkdtemp names within the process.
    tmp_seq: u64,
}

impl LiveImpIo {
    /// Executor with wallpaper upscaling (`None` — without upscaling).
    pub fn new(upscaler: Option<UpscaleFn>) -> Self {
        LiveImpIo { upscaler, io_err: None, tmp_seq: 0 }
    }

    fn fail(&mut self, what: &str, path: &str, e: &std::io::Error) {
        if self.io_err.is_none() {
            self.io_err = Some(format!("{what} {path}: {e}"));
        }
    }
}

/// Whether there is an executable file at the path (for which).
fn is_executable(p: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

// Names of external converters in different distributions — a mirror of the
// `_TOOL_SETS` / `_TOOL_SUBCOMMANDS` tables from `rexon/utils.py`. The core
// builds argv with the CANONICAL name (`convert`, `7z`, `identify`), while the
// real names are known to the executor — here, this module, not Python (for
// other plans `utils.safe_run` does the substitution). Without this, the
// "program exists" check on the Python side (where `convert` is found as
// `magick`) would diverge from the launch on the bridge side.
const TOOL_ALIASES: &[(&str, &[&str])] = &[
    // ImageMagick 7: `magick` is a drop-in replacement for `convert`
    ("convert", &["convert", "magick"]),
    // the `7zip` package installs `7zz` (Debian/Ubuntu), the old p7zip — `7za`
    ("7z", &["7z", "7zz", "7za"]),
];

// Tools that may have no binary of their own at all: in ImageMagick 7
// `identify` is the subcommand `magick identify`, i.e. TWO words.
const TOOL_SUBCOMMANDS: &[(&str, &[&str])] = &[("identify", &["magick", "identify"])];

/// Whether a program with this name is in PATH.
fn in_path(name: &str) -> bool {
    if name.contains('/') {
        return is_executable(std::path::Path::new(name));
    }
    let Some(paths) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&paths).any(|d| is_executable(&d.join(name)))
}

/// Whether the tool is present on the system — taking synonyms and subcommands
/// into account.
pub fn tool_available(name: &str) -> bool {
    if in_path(name) {
        return true;
    }
    for (canon, names) in TOOL_ALIASES {
        if *canon == name && names.iter().any(|n| in_path(n)) {
            return true;
        }
    }
    for (canon, prefix) in TOOL_SUBCOMMANDS {
        if *canon == name && in_path(prefix[0]) {
            return true;
        }
    }
    false
}

/// argv with the real tool name (if the canonical one was not found).
fn resolved_tool_argv(argv: Vec<String>) -> Vec<String> {
    let Some(head) = argv.first() else { return argv };
    if in_path(head) {
        return argv;
    }
    for (canon, names) in TOOL_ALIASES {
        if canon == head {
            if let Some(found) = names.iter().find(|n| in_path(n)) {
                let mut out = argv.clone();
                out[0] = (*found).to_string();
                return out;
            }
        }
    }
    for (canon, prefix) in TOOL_SUBCOMMANDS {
        if canon == head && in_path(prefix[0]) {
            let mut out: Vec<String> = prefix.iter().map(|s| s.to_string()).collect();
            out.extend(argv.into_iter().skip(1));
            return out;
        }
    }
    argv
}

/// Recursive traversal for walk (topdown, directory symlinks are not followed).
fn walk_rec(dir: &std::path::Path, out: &mut Vec<core_imp::WalkEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let mut descend = Vec::new();
    for n in names {
        let p = dir.join(&n);
        if p.is_dir() {
            let is_link = p
                .symlink_metadata()
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_link {
                descend.push(p);
            }
            dirs.push(n);
        } else {
            files.push(n);
        }
    }
    out.push((dir.to_string_lossy().into_owned(), dirs, files));
    for p in descend {
        walk_rec(&p, out);
    }
}

/// Recursive directory copy (shutil.copytree, symlinks=False).
fn copy_tree_rec(src: &std::path::Path, dest: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let s = entry.path();
        let d = dest.join(entry.file_name());
        if s.is_dir() {
            copy_tree_rec(&s, &d)?;
        } else {
            std::fs::copy(&s, &d)?;
        }
    }
    Ok(())
}

/// Run an external converter: `(whether it succeeded, stdout)`.
///
/// A shared body for both live executors of the module (`ImpIo::run` of the
/// importers and `UpIo::run` of the upscaler): they have the same original —
/// `utils.run_tool` — and two copies of this loop would silently drift apart
/// (rule #12).
fn run_tool(argv: Vec<String>, timeout: u64) -> (bool, Vec<u8>) {
        use std::io::Read;
        use std::process::{Command, Stdio};
        let none = (false, Vec::new());
        // The converter name goes through the shared resolver: on ImageMagick 7
        // `magick` runs instead of `convert`, and `magick identify` instead of
        // `identify`. If there is no program at all — an honest command failure below.
        let argv = resolved_tool_argv(argv);
        let Some((bin, rest)) = argv.split_first() else { return none };
        let child = Command::new(bin)
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            // no binary — an executor assumption: treated as a command failure
            Err(_) => return none,
        };
        let reader = |pipe: Option<std::process::ChildStdout>,
                      errpipe: Option<std::process::ChildStderr>| {
            let mut out = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut out);
            }
            if let Some(mut p) = errpipe {
                let mut sink = Vec::new();
                let _ = p.read_to_end(&mut sink);
            }
            out
        };
        let stdout_pipe = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let t = std::thread::spawn(move || reader(stdout_pipe, stderr_pipe));
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(timeout);
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        // timeout — an executor assumption: a command failure
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break None,
            }
        };
        let stdout = t.join().unwrap_or_default();
        (status.map(|s| s.success()).unwrap_or(false), stdout)
}

impl core_imp::ImpIo for LiveImpIo {
    fn mkdtemp(&mut self, prefix: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::var_os("TMPDIR")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
        let mut last = base.join(prefix);
        for _ in 0..100 {
            self.tmp_seq += 1;
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            let name = format!(
                "{prefix}{:x}{:x}{:x}",
                std::process::id(),
                nanos,
                self.tmp_seq
            );
            last = base.join(&name);
            match std::fs::create_dir(&last) {
                Ok(()) => {
                    let _ = std::fs::set_permissions(
                        &last,
                        std::fs::Permissions::from_mode(0o700),
                    );
                    return last.to_string_lossy().into_owned();
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => {
                    self.fail("mkdtemp", &last.to_string_lossy(), &e);
                    break;
                }
            }
        }
        last.to_string_lossy().into_owned()
    }

    fn makedirs(&mut self, path: &str) {
        if let Err(e) = std::fs::create_dir_all(path) {
            self.fail("makedirs", path, &e);
        }
    }

    fn run(&mut self, argv: Vec<String>, timeout: u64) -> core_imp::RunResult {
        let (ok, stdout) = run_tool(argv, timeout);
        core_imp::RunResult { ok, stdout }
    }

    fn walk(&mut self, root: &str) -> Vec<core_imp::WalkEntry> {
        let mut out = Vec::new();
        walk_rec(std::path::Path::new(root), &mut out);
        out
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else { return Vec::new() };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn is_dir(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn getsize(&mut self, path: &str) -> u64 {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }

    fn read_text(&mut self, path: &str) -> Option<String> {
        // errors="replace": U+FFFD on broken bytes — like from_utf8_lossy
        std::fs::read(path)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    fn read_head_text(&mut self, path: &str, n: usize) -> Option<String> {
        // .read(n) in text mode — n CHARACTERS
        self.read_text(path).map(|s| s.chars().take(n).collect())
    }

    fn read_head_bytes(&mut self, path: &str, n: usize) -> Option<Vec<u8>> {
        std::fs::read(path).ok().map(|mut b| {
            b.truncate(n);
            b
        })
    }

    fn read_bytes(&mut self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn is_zipfile(&mut self, path: &str) -> Option<bool> {
        match std::fs::File::open(path) {
            Err(_) => None,
            Ok(f) => Some(zip::ZipArchive::new(f).is_ok()),
        }
    }

    fn zip_open(&mut self, path: &str) -> bool {
        // a file open error is treated as BadZipFile (an assumption)
        std::fs::File::open(path)
            .ok()
            .map(|f| zip::ZipArchive::new(f).is_ok())
            .unwrap_or(false)
    }

    fn which(&mut self, name: &str) -> bool {
        // Taking synonyms into account — otherwise the core would consider `7z`
        // missing where the package installed only `7zz` (see `TOOL_ALIASES`).
        tool_available(name)
    }

    fn copy_file(&mut self, src: &str, dest: &str) {
        if let Err(e) = std::fs::copy(src, dest) {
            self.fail("copy", src, &e);
        }
    }

    fn copy_tree(&mut self, src: &str, dest: &str) {
        if let Err(e) = copy_tree_rec(
            std::path::Path::new(src),
            std::path::Path::new(dest),
        ) {
            self.fail("copytree", src, &e);
        }
    }

    fn symlink(&mut self, target: &str, link: &str) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    fn remove_file(&mut self, path: &str) {
        if let Err(e) = std::fs::remove_file(path) {
            self.fail("remove", path, &e);
        }
    }

    fn remove_tree(&mut self, path: &str) {
        // shutil.rmtree(..., ignore_errors=True)
        let _ = std::fs::remove_dir_all(path);
    }

    fn write_file(&mut self, path: &str, content: &str) {
        if let Err(e) = std::fs::write(path, content) {
            self.fail("write", path, &e);
        }
    }

    fn extract_zip(&mut self, archive: &str, dest: &str) {
        if let Err(e) = core_ext::safe_zip_extract(
            std::path::Path::new(archive),
            std::path::Path::new(dest),
        ) {
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            if self.io_err.is_none() {
                self.io_err = Some(format!("extract_zip {archive}: {msg}"));
            }
        }
    }

    fn pack_rxn(&mut self, src: &str, dest: &str) {
        if let Err(e) = core_ext::pack_rxn(
            std::path::Path::new(src),
            std::path::Path::new(dest),
        ) {
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            if self.io_err.is_none() {
                self.io_err = Some(format!("pack_rxn {dest}: {msg}"));
            }
        }
    }

    fn write_bytes(&mut self, path: &str, data: &[u8]) {
        if let Err(e) = std::fs::write(path, data) {
            self.fail("write", path, &e);
        }
    }

    fn zip_names(&mut self, path: &str) -> Option<Vec<String>> {
        let f = std::fs::File::open(path).ok()?;
        let zip = zip::ZipArchive::new(f).ok()?;
        Some(zip.file_names().map(str::to_string).collect())
    }

    fn is_symlink(&mut self, path: &str) -> bool {
        std::fs::symlink_metadata(path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    }

    fn upscale_wallpapers(&mut self, data_dir: &str) -> u64 {
        match &mut self.upscaler {
            Some(cb) => cb(data_dir),
            None => 0,
        }
    }
}

// ============================================================================
// Windows cursors: a stable entry point for storefronts and "convert again"
// ============================================================================

/// Package manager family of this machine (for package names in refusals).
fn live_family() -> String {
    crate::system::DistroDetector::detect_live()
        .family_live()
        .as_str()
        .to_string()
}

/// ⭐ STABLE ENTRY POINT: a Windows cursor set → `.rxn` in `out_dir`.
///
/// `path` — an archive (`.zip`/`.rar`/`.7z`), a folder, an `.inf`/`.crs` or a
/// single `.cur`/`.ani`. The `.rxn` contains an Xcursor theme (all animation
/// frames), the set's originals and a manifest with `source: "windows"` and the
/// details from `source` (site, page address, author, license). It is then
/// installed like an ordinary `.rxn` (`ext_exec::install_from_file`). An error
/// is a ready text for the person in the interface language.
///
/// Whether third-party programs are needed: only for `.rar`/`.7z` (`bsdtar` or
/// `7z`); if they are missing — a refusal with this system's package name
/// BEFORE any action.
pub fn import_windows_cursor_archive(
    path: &std::path::Path,
    out_dir: &std::path::Path,
    source: &core_imp::CursorSource,
) -> Result<std::path::PathBuf, String> {
    let p = path.to_string_lossy().into_owned();
    let family = live_family();
    core_imp::require_tools(core_imp::import_kind(&p, "cursor"), &family, &|t| {
        tool_available(t)
    })?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let out = out_dir.to_string_lossy().into_owned();
    let mut io = LiveImpIo::new(None);
    let result = core_imp::cursor_import_with_source(&mut io, &p, source, Some(&out));
    if let Some(message) = io.io_err {
        return Err(message);
    }
    result
        .map(std::path::PathBuf::from)
        .map_err(|e| core_imp::friendly_error_text(&e.message, &family).unwrap_or(e.message))
}

/// Convert a Windows cursor theme again from its originals — without
/// re-downloading.
///
/// `src` — an `.rxn` or a directory (an unpacked `.rxn` or an installed
/// extension `~/.config/rexon/extensions/wincur-…`). A new `.rxn` with the same
/// id is put into `out_dir`; it is installed as usual and replaces the previous
/// one.
pub fn retranslate_windows_cursors(
    src: &std::path::Path,
    out_dir: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let out = out_dir.to_string_lossy().into_owned();
    let mut io = LiveImpIo::new(None);
    let mut unpacked: Option<std::path::PathBuf> = None;
    let dir = if src.is_dir() {
        src.to_path_buf()
    } else {
        let tmp = std::path::PathBuf::from(core_imp::ImpIo::mkdtemp(&mut io, "rexon_wincur_rxn_"));
        unpacked = Some(tmp.clone());
        if let Err(e) = core_ext::safe_zip_extract(src, &tmp) {
            let _ = std::fs::remove_dir_all(&tmp);
            let msg = match e {
                core_ext::ArchiveError::ZipSlip(m)
                | core_ext::ArchiveError::BadZip(m)
                | core_ext::ArchiveError::Io(m) => m,
            };
            return Err(crate::i18n::trf("Couldn't unpack the archive: %1", &[&msg]));
        }
        tmp
    };
    let result =
        core_imp::cursor_retranslate_dir(&mut io, &dir.to_string_lossy(), &out);
    if let Some(tmp) = unpacked {
        let _ = std::fs::remove_dir_all(tmp);
    }
    if let Some(message) = io.io_err {
        return Err(message);
    }
    result.map(std::path::PathBuf::from).map_err(|e| e.message)
}

// ============================================================================
// Live executor of the wallpaper upscaler (`upscaler::UpIo`)
// ============================================================================
//
// WHY HERE. Importing a `.themepack`/`.theme` ends with upscaling small
// wallpapers (`ImpIo::upscale_wallpapers`), and the upscaling itself is a
// second trait with its own live executor. Without it, moving the importers to
// a process without Python would be a trade-off: import would work, but the
// wallpapers from XP/Vista themes would stay at their 800×600. That is why the
// upscaler's executor lies next to the importers' executor and is plugged in
// by a single function, [`upscale_hook`].
//
// THERE IS EXACTLY ONE DIFFERENCE FROM THE PYTHON EXECUTOR, and it is named out
// loud: `image_size`. The original asks Pillow for the size first and only then
// `identify`; there is no Pillow here, so the method is not overridden and the
// core takes the fallback path via `identify`. For this path that is no loss:
// the whole Windows theme import pipeline relies on ImageMagick anyway
// (`convert` for previews and bitmaps), i.e. on a machine where import is
// possible at all, `identify` is present.

/// Live executor of [`crate::upscaler::UpIo`]: real file system and subprocesses.
pub struct LiveUpIo {
    /// Where to report that upscaling was skipped and why.
    notes: Notes,
    /// Package manager family — to name the package by THIS system's name, not the
    /// Arch one (rule #13).
    family: String,
}

impl LiveUpIo {
    /// An executor that knows how to complain.
    pub fn new(notes: Notes, family: &str) -> Self {
        LiveUpIo { notes, family: family.to_string() }
    }
}

/// The home directory (`$HOME`) or the current one — as for other core entry points.
fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// Path of an executable in `$PATH` — `shutil.which` semantics.
fn which_path(name: &str) -> Option<String> {
    if name.contains('/') {
        let p = std::path::Path::new(name);
        return is_executable(p).then(|| name.to_string());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
        .map(|p| p.to_string_lossy().into_owned())
}

/// `os.access(path, os.X_OK)`.
fn access_x(path: &str) -> bool {
    is_executable(std::path::Path::new(path))
}

/// Set permissions `0o755` (port of `os.chmod(..., 0o755)`).
fn chmod_755(path: &str) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

impl crate::upscaler::UpIo for LiveUpIo {
    /// Image size — BY OUR OWN MEANS, without third-party programs.
    ///
    /// The order is exactly as in the original, only instead of Pillow we parse the
    /// file's own header: it covers all five formats the upscaler ever gets to. If
    /// it is not recognized, we try `identify`. If that fails too, we say so in words
    /// and answer "0×0": the same answer the core would have got by itself, but now
    /// the person will learn about it.
    ///
    /// `identify` is called here rather than left to the core precisely for the
    /// sake of the last branch: otherwise the executor would have no way to learn
    /// that the fallback did not work either, and the skip would become silent
    /// again.
    fn image_size(&mut self, path: &str) -> Option<(i64, i64)> {
        let head = read_head_bytes(path, crate::upscaler::HEADER_PROBE_BYTES);
        if let Some(size) = head
            .as_deref()
            .and_then(crate::upscaler::image_size_from_header)
        {
            return Some(size);
        }
        let r = run_tool(
            vec![
                "identify".into(),
                "-format".into(),
                "%w %h".into(),
                path.into(),
            ],
            10,
        );
        if r.0 {
            let size = crate::upscaler::parse_identify(&r.1);
            if size.0 > 0 && size.1 > 0 {
                return Some(size);
            }
        }
        note(&self.notes, crate::upscaler::note_upscale_skipped(&self.family));
        Some((0, 0))
    }

    fn run(&mut self, argv: Vec<String>, timeout: u64) -> crate::upscaler::RunResult {
        let (ok, stdout) = run_tool(argv, timeout);
        crate::upscaler::RunResult { ok, stdout }
    }

    fn which(&mut self, name: &str) -> Option<String> {
        which_path(name)
    }

    fn exists(&mut self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn is_file(&mut self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn access_x(&mut self, path: &str) -> bool {
        access_x(path)
    }

    fn remove_file(&mut self, path: &str) {
        // the error is suppressed by the original (`except OSError: log.debug`)
        let _ = std::fs::remove_file(path);
    }

    fn listdir(&mut self, path: &str) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(path) else { return Vec::new() };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn replace(&mut self, src: &str, dst: &str) {
        let _ = std::fs::rename(src, dst);
    }

    fn download_realesrgan(&mut self) -> Option<String> {
        download_realesrgan_impl()
    }
}

/// Port of `WallpaperUpscaler._download_realesrgan_impl`: download and unpack
/// Real-ESRGAN, return the path to the binary.
///
/// The original (Python) always failed at this point with `NameError` —
/// `zipfile` and `_safe_zip_extract` are not imported in the module — i.e. the
/// buyer never once had automatic download. Here it works: rule #16 (a bug of
/// the original is fixed in the port). Any failure is a silent `None`: upscaling
/// simply moves on to the next strategy (waifu2x, then ImageMagick).
fn download_realesrgan_impl() -> Option<String> {
    use crate::net::NetIo;

    let home = home_dir();
    let local_exe = crate::upscaler::esrgan_local_exe(&home);
    let esrgan_dir = std::path::Path::new(&local_exe).parent()?.to_path_buf();
    std::fs::create_dir_all(&esrgan_dir).ok()?;
    let zip_path = esrgan_dir.join("realesrgan.zip");

    // Downloading as a stream into a file rather than the whole body into memory:
    // the Real-ESRGAN archive is tens of megabytes, and the original's
    // `urlretrieve` also wrote it to disk in chunks.
    let mut net = crate::net::UreqNet::new();
    let zip_str = zip_path.to_string_lossy().into_owned();
    let ok = (|| -> bool {
        if net.open(crate::upscaler::ESRGAN_URL, "Rexon", 60).is_err() {
            return false;
        }
        if net.create(&zip_str).is_err() {
            net.close();
            return false;
        }
        loop {
            match net.read(64 * 1024) {
                Ok(chunk) if chunk.is_empty() => break,
                Ok(chunk) => {
                    if net.write(&chunk).is_err() {
                        net.close_file();
                        net.close();
                        return false;
                    }
                }
                Err(_) => {
                    net.close_file();
                    net.close();
                    return false;
                }
            }
        }
        net.close_file();
        net.close();
        true
    })();
    if !ok {
        let _ = std::fs::remove_file(&zip_path);
        return None;
    }

    if core_ext::safe_zip_extract(&zip_path, &esrgan_dir).is_err() {
        let _ = std::fs::remove_file(&zip_path);
        return None;
    }

    if std::path::Path::new(&local_exe).is_file() {
        chmod_755(&local_exe);
    } else {
        // The binary may lie in a subfolder of the archive — as in the original, the
        // first one found is moved to the standard place together with its models.
        let mut walked = Vec::new();
        walk_rec(&esrgan_dir, &mut walked);
        'outer: for (root, _dirs, files) in &walked {
            for f in files {
                if f != "realesrgan-ncnn-vulkan" {
                    continue;
                }
                let found = std::path::Path::new(root).join(f);
                let found_s = found.to_string_lossy().into_owned();
                chmod_755(&found_s);
                if found_s != local_exe {
                    let _ = std::fs::rename(&found, &local_exe);
                }
                for mf in std::fs::read_dir(root).into_iter().flatten().flatten() {
                    let name = mf.file_name().to_string_lossy().into_owned();
                    if !name.ends_with(".bin") && !name.ends_with(".param") {
                        continue;
                    }
                    let dst = esrgan_dir.join(&name);
                    if !dst.exists() {
                        let _ = std::fs::copy(mf.path(), dst);
                    }
                }
                break 'outer;
            }
        }
    }

    let _ = std::fs::remove_file(&zip_path);

    if std::path::Path::new(&local_exe).is_file() && access_x(&local_exe) {
        return Some(local_exe);
    }
    None
}

/// Read the start of a file (up to `limit` bytes). `None` — the file did not open.
fn read_head_bytes(path: &str, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(limit as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Wallpaper upscaling by the live executor — a ready closure for [`LiveImpIo`].
///
/// Port of `WallpaperUpscaler.process_wallpapers_in_dir`: in a process with
/// Python the bridge plugged it in, here the core itself does the same. Skipped
/// steps go into `notes`, not into silence.
pub fn upscale_hook(notes: Notes, family: &str) -> UpscaleFn {
    let family = family.to_string();
    Box::new(move |data_dir: &str| {
        let local_exe = crate::upscaler::esrgan_local_exe(&home_dir());
        let mut io = LiveUpIo::new(notes.clone(), &family);
        crate::upscaler::process_wallpapers_in_dir(&mut io, data_dir, &local_exe)
            .max(0) as u64
    })
}
