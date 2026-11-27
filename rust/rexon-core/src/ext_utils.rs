//! Utilities for working with extensions: a port of `rexon/extensions/utils.py`.
//!
//! Safe ZIP extraction (protection against ZipSlip), `.rxn` packing, writing
//! `manifest.json`, safe directory removal.
//!
//! Warning: the ZIP packing check is STRUCTURAL, not byte for byte: a `.zip`
//! contains file timestamps and depends on the deflate version/level, so it
//! can never match Python byte for byte. We check the invariant that matters
//! to the application: the set of names in the archive and the contents of
//! each file (golden test `ext_utils_parity.rs` compares them with an archive
//! built by Python itself).

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use zip::write::SimpleFileOptions;

/// Lexical path normalization (without touching the file system): removes
/// `.` and collapses `..`. The counterpart of what `os.path.realpath` does with
/// a non-existent path (symbolic links are NOT resolved — a fresh extraction
/// directory has none; a deliberate deviation from Python's realpath).
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `os.path.realpath` for a path that may not exist yet: the deepest existing
/// ancestor is canonicalized and the non-existent tail is appended lexically
/// (as in CPython's posixpath.realpath, strict=False).
fn realpath_missing_ok(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(real) = std::fs::canonicalize(path) {
        return Ok(real);
    }
    let lexical = normalize_lexical(&std::path::absolute(path)?);
    let mut existing = lexical.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            break;
        }
        let Some(name) = existing.file_name() else { break };
        tail.push(name.to_os_string());
        if !existing.pop() {
            break;
        }
    }
    let mut real = std::fs::canonicalize(&existing).unwrap_or(existing);
    for name in tail.into_iter().rev() {
        real.push(name);
    }
    Ok(real)
}

/// Where `zipfile.extractall` actually puts an entry: a port of the sanitizer
/// in `ZipFile._extract_member`. The name is split at `/`, and empty
/// components, `.` and `..` are DROPPED (not collapsed). So in Python
/// `a/../b.txt` ends up in `a/b.txt`, not in `b.txt`, and a leading `/` is
/// simply lost.
///
/// This used to be `normalize_lexical`, which collapsed `..`, so the same
/// archive was extracted by Rust into a DIFFERENT directory than by Python
/// (for a `.rxn` this means a different root with `manifest.json`). The
/// ZipSlip check above stays on the collapsing normalization: it reproduces
/// the `os.path.realpath` of the original's check.
fn extract_member_name(name: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in name.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }
        out.push(part);
    }
    out
}

/// A string's `repr()` as in CPython, for `{name!r}` in the original's error
/// texts. Printable non-ASCII stays as is; `\`, `\n`, `\r`, `\t`, the quote
/// itself and C0/DEL control characters are escaped.
pub(crate) fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// A ZIP error message in the original's words (`zipfile`).
///
/// The texts of the `zip` crate are English and internal ("invalid Zip
/// archive: Could not find EOCD"), and they go straight to the customer's
/// window: the `.rxn` installer appends `str(e)` to its prefix. In the same
/// places `zipfile` raises `BadZipFile` with short texts, and those are what
/// we return; otherwise the Rust path and the Python fallback would tell the
/// customer different things about the same archive.
pub fn zip_error_text(e: &zip::result::ZipError) -> String {
    use zip::result::ZipError;
    match e {
        // Not a ZIP at all, or truncated so that there is no central directory.
        ZipError::InvalidArchive(_) | ZipError::UnsupportedArchive(_) => {
            "File is not a zip file".to_string()
        }
        ZipError::FileNotFound => "There is no item named in the archive".to_string(),
        ZipError::InvalidPassword => "Bad password for file".to_string(),
        ZipError::Io(io) => io.to_string(),
        // The crate marks it non_exhaustive: anything new is also "not that archive".
        _ => "File is not a zip file".to_string(),
    }
}

/// An error reading an archive entry, in the original's words. The `zip`
/// crate reports a checksum mismatch as a plain io error "Invalid checksum",
/// `zipfile` as `BadZipFile("Bad CRC-32 for file 'name'")`.
fn entry_read_error(e: std::io::Error, member: &str) -> ArchiveError {
    if e.kind() == std::io::ErrorKind::InvalidData && e.to_string().contains("Invalid checksum") {
        return ArchiveError::BadZip(format!("Bad CRC-32 for file {}", py_repr_str(member)));
    }
    ArchiveError::Io(e.to_string())
}

/// An extraction/packing error.
#[derive(Debug)]
pub enum ArchiveError {
    /// ZipSlip detected: a dangerous path in the archive (the counterpart of `ValueError`).
    ZipSlip(String),
    /// A corrupted archive, the counterpart of `zipfile.BadZipFile`. Separate from
    /// `Io` because `BadZipFile` does NOT inherit from `OSError`: the callers
    /// (`.rxn` installation, the icon importer) catch exactly it to say "The file
    /// is corrupted…" and take the fallback path.
    BadZip(String),
    /// An I/O error.
    Io(String),
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveError::ZipSlip(m) | ArchiveError::BadZip(m) | ArchiveError::Io(m) => {
                write!(f, "{m}")
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<std::io::Error> for ArchiveError {
    fn from(e: std::io::Error) -> Self {
        ArchiveError::Io(e.to_string())
    }
}

impl From<zip::result::ZipError> for ArchiveError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => ArchiveError::Io(io.to_string()),
            other => ArchiveError::BadZip(zip_error_text(&other)),
        }
    }
}

/// Extracts a ZIP with protection against path traversal (ZipSlip): a port of
/// `_safe_zip_extract`. First checks ALL entries (like Python: the check loop
/// first, then `extractall`); on a dangerous path, fails before writing a
/// single file.
pub fn safe_zip_extract(archive_path: &Path, dest: &Path) -> Result<(), ArchiveError> {
    // Python: os.path.realpath(dest) does NOT require dest to exist
    // (extractall creates the directories itself); canonicalizing a destination
    // that does not exist yet (importers call this with dest inside a fresh
    // mkdtemp) would fail with ENOENT, so we canonicalize the nearest existing
    // ancestor and append the tail lexically.
    let dest = realpath_missing_ok(dest)?;
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Path check (ZipSlip): target must be inside dest or == dest.
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let target = normalize_lexical(&dest.join(entry.name()));
        if !target.starts_with(&dest) {
            return Err(ArchiveError::ZipSlip(crate::i18n::trf(
                "ZipSlip: unsafe path in the archive: %1",
                &[&py_repr_str(entry.name())],
            )));
        }
    }

    // Extraction (paths are formed the way zipfile.extractall forms them).
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let member = entry.name().to_string();
        let rel = extract_member_name(&member);
        if rel.as_os_str().is_empty() {
            // A name consisting entirely of dropped components ("/", "./", "..") —
            // extractall creates nothing for such an entry.
            continue;
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.is_symlink() {
            // An alias link inside the archive (see [`pack_rxn`]) is restored as a
            // link — only a relative one without `..`, i.e. reaching no further than
            // deeper into its own directory; any other is skipped (the archive cannot
            // reach beyond `dest` either with an entry or with a link).
            let mut link_to = String::new();
            entry
                .read_to_string(&mut link_to)
                .map_err(|e| entry_read_error(e, &member))?;
            if is_inner_link_target(Path::new(&link_to)) {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(&link_to, &target)?;
            }
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&target)?;
            // A broken payload with an intact central directory is BadZipFile
            // ("Bad CRC-32…"), not OSError.
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| entry_read_error(e, &member))?;
            // The execute bit from the archive (`external_attr >> 16`): without it,
            // widget or theme scripts that call each other directly get "Permission
            // denied". ONLY the x bits are carried over — see [`safe_extracted_mode`].
            if let Some(archived) = entry.unix_mode().filter(|m| m & 0o111 != 0) {
                use std::os::unix::fs::PermissionsExt;
                let disk = out.metadata()?.permissions().mode();
                let mode = safe_extracted_mode(disk | (archived & 0o111), false);
                out.set_permissions(std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

/// The permissions of a file or directory extracted from an archive, a common
/// rule for zip, tar and 7z: the execute bits (for owner, group, others) stay,
/// setuid/setgid/sticky and write access for group and others are removed,
/// and the owner always keeps read and write (and, for a directory, search)
/// so that Rexon can later update and remove what was installed.
///
/// Execute permissions do not bypass the Conky widget permission dialog: Rexon
/// starts a widget only after "Yes" ([`crate::conky_safety`]), and the
/// permission fingerprint is computed from the file contents, not from their
/// permissions.
pub fn safe_extracted_mode(mode: u32, is_dir: bool) -> u32 {
    (mode & 0o755) | if is_dir { 0o700 } else { 0o600 }
}

/// Brings the permissions of the whole extracted tree `root` to
/// [`safe_extracted_mode`]: the system `tar` and 7-Zip apply the permissions
/// from the archive as they are (within the umask), setuid included. Links are
/// neither touched nor dereferenced — the walk does not leave `root`.
pub fn clamp_extracted_modes(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fn walk(dir: &Path, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.file_type().is_symlink() || !(md.is_file() || md.is_dir()) {
                continue;
            }
            let mode = md.permissions().mode() & 0o7777;
            let safe = safe_extracted_mode(mode, md.is_dir());
            if safe != mode {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(safe));
            }
            if md.is_dir() && depth < 64 {
                walk(&path, depth + 1);
            }
        }
    }
    walk(root, 0);
}

/// Packs a directory into a `.rxn` (ZIP, DEFLATE): a port of `_pack_rxn`.
/// Walks the tree and stores the files under names relative to `source_dir`.
///
/// Timestamps are deliberately NOT fixed, as in Python; the archive cannot be
/// compared byte for byte (see the module header), only structurally.
pub fn pack_rxn(source_dir: &Path, rxn_path: &Path) -> Result<(), ArchiveError> {
    let file = std::fs::File::create(rxn_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut files = Vec::new();
    collect_files(source_dir, &mut files)?;
    for full in files {
        let arcname = full
            .strip_prefix(source_dir)
            .expect("файл внутри source_dir")
            .to_string_lossy()
            .replace('\\', "/");
        // An alias link to a file in the same tree is stored AS A LINK, not as a
        // copy: a cursor theme keeps dozens of names for one cursor (`left_ptr`,
        // `default`, `arrow`…), and copies of the animations would bloat the `.rxn`
        // several times over. A link pointing outside or to something non-existent
        // is still stored as contents.
        if let Ok(link_to) = std::fs::read_link(&full) {
            let resolved = full.parent().map(|p| p.join(&link_to));
            if is_inner_link_target(&link_to) && resolved.is_some_and(|r| r.is_file()) {
                zip.add_symlink(arcname, link_to.to_string_lossy(), options)?;
                continue;
            }
        }
        zip.start_file(arcname, options)?;
        let mut f = std::fs::File::open(&full)?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        zip.write_all(&buf)?;
    }
    zip.finish()?;
    Ok(())
}

/// An "inward" link: relative, non-empty, without `..` and the root — so it
/// does not leave the directory it lives in.
fn is_inner_link_target(target: &Path) -> bool {
    !target.as_os_str().is_empty()
        && target
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

/// Recursively collects the paths of FILES (not directories) under `dir`: the
/// counterpart of `os.walk` + collecting `files`.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Writes `manifest.json` into a directory: a port of `_write_manifest`
/// (`json.dump(indent=2, ensure_ascii=False)`: 2 spaces, UTF-8 without
/// escaping). Matches Python byte for byte.
pub fn write_manifest(manifest: &Value, directory: &Path) -> std::io::Result<()> {
    let path = directory.join("manifest.json");
    let data = serde_json::to_string_pretty(manifest).expect("serde_json::Value сериализуется");
    std::fs::write(path, data)
}

/// Safe directory removal: a port of `_safe_rmtree`.
///
/// * A non-existent path (and not a symlink) → `false`.
/// * A symlink → the link ITSELF is removed (not its target), `true`.
/// * A real path OUTSIDE `$HOME` and outside the temporary directory → refused,
///   `false`; `$HOME` itself (like the temporary directory itself) is refused
///   too — we only remove what lies INSIDE them.
/// * Otherwise, recursive removal (errors are ignored, like
///   `shutil.rmtree(ignore_errors=True)`), `true`.
///
/// As in Python, a plain FILE inside `$HOME` is not removed (rmtree works only
/// on a directory), but the function still returns `true`.
pub fn safe_rmtree(path: &Path) -> bool {
    let link_meta = std::fs::symlink_metadata(path);
    let is_symlink = link_meta.as_ref().map(|m| m.file_type().is_symlink()).unwrap_or(false);
    if !path.exists() && !is_symlink {
        return false;
    }
    if is_symlink {
        let _ = std::fs::remove_file(path);
        return true;
    }
    let real = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !rmtree_allowed(&real, &home_dir(), &std::env::temp_dir()) {
        return false;
    }
    let _ = std::fs::remove_dir_all(path);
    true
}

/// Whether [`safe_rmtree`] would remove anything at `path`: a link — yes; a
/// directory — only under `$HOME` or the temporary directory (not those
/// themselves); a file — no (rmtree does not work on a file). An installation
/// rollback (`install_plan::Undo`) uses this answer to move the path aside
/// instead of deleting it, within the same boundaries.
pub(crate) fn safe_rmtree_removes(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => true,
        Ok(m) if m.is_dir() => std::fs::canonicalize(path)
            .is_ok_and(|real| rmtree_allowed(&real, &home_dir(), &std::env::temp_dir())),
        _ => false,
    }
}

/// Whether an already resolved (`realpath`) path may be removed: extracted
/// from [`safe_rmtree`] into a separate pure function so that the check can be
/// tested without a real `$HOME` on disk.
///
/// The reference (`rexon/extensions/utils.py`) compares with `home + os.sep`,
/// i.e. it requires a strict DESCENDANT: `_py_safe_rmtree($HOME)` refuses.
/// For `Path::starts_with` equality is true, so the root is cut off
/// explicitly — without this `safe_rmtree($HOME)` would wipe the whole home
/// directory.
pub fn rmtree_allowed(real: &Path, home: &Path, tmp: &Path) -> bool {
    (real.starts_with(home) && real != home) || (real.starts_with(tmp) && real != tmp)
}

/// The home directory from `$HOME` (falls back to the current directory).
fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}
