//! Preparing Razer Axon wallpapers in advance, right inside the Axon library.
//!
//! Downloaded Axon wallpapers are stored as a ZIP container (see
//! [`crate::axon_extract`]). So that "Apply" in Rexon works instantly, the
//! background preparation decrypts the container in advance and puts the
//! video UNDER THE SAME NAME in the same place, and removes two fields from
//! `ResourceConfig.txt`: `SourceEncryptedTypes` and `SourceEncryptedLevel`.
//! Experiment of 31.10.2026 on Axon 2.11.1.0: Axon only checks that the files
//! exist, and it hands wallpapers with such a description to the player
//! directly, like old unencrypted ones — its display does not break and there
//! is no re-download.
//!
//! The order for a single wallpaper:
//! 1. the original description goes into a backup
//!    `~/.local/share/rexon/axon-originals/<Id>/ResourceConfig.txt`
//!    (next to it `origin.json`, where it came from); after editing the
//!    description the container password is different, and "as it was"
//!    cannot be restored without this copy;
//! 2. depending on the [`KEEP_ORIGINALS_KEY`] setting (off by default), the
//!    encrypted file goes there too (a hard link, or a copy without one);
//! 3. decryption into a temporary file in the same folder → fsync → rename
//!    over the container;
//! 4. the description without the two fields, just as atomically, everything
//!    else byte for byte (BOM, line endings, indentation, field order).
//!
//! If it was interrupted between 3 and 4 (the video is already decrypted, the
//! description is old), the next pass recognizes this by the backup and
//! finishes step 4.
//!
//! Wallpapers Axon is showing right now (`Playing` in the library, current in
//! the playlist, or the file is open in some process — `/proc/*/fd`) are not
//! touched: they are retried later. Old unencrypted wallpapers and already
//! prepared ones are skipped.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::axon_extract::{self, ExtractError};
use crate::axon_library::{self as ax, AxonEnv, AxonExtractor, AxonKind, AxonLibrary, AxonWallpaper, Prepared};
use crate::i18n::{tr, trf};

/// Setting: prepare wallpapers in advance (on by default).
pub const PREPARE_AHEAD_KEY: &str = "axon_prepare_ahead";
/// Setting: whether to keep the encrypted original video in the backup (off
/// by default: only the decrypted one remains).
pub const KEEP_ORIGINALS_KEY: &str = "axon_keep_encrypted_originals";
/// The description fields the preparation removes.
pub const ENCRYPTION_FIELDS: &[&str] = &["SourceEncryptedTypes", "SourceEncryptedLevel"];
/// Where the backup came from (the wallpaper directory and the display file path).
pub const ORIGIN_FILE: &str = "origin.json";

/// The delay of the background pass after Rexon starts.
pub const STARTUP_DELAY: Duration = Duration::from_secs(45);
/// The pause between wallpapers in the background.
const PAUSE_BETWEEN: Duration = Duration::from_secs(2);
/// How soon to retry wallpapers that were busy.
const RETRY_BUSY_AFTER: Duration = Duration::from_secs(600);

// ============================================================================
// Editing the description
// ============================================================================

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// `b[i]` is the opening quote; returns the end of the string (after the closing quote).
fn scan_string(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// The end of the JSON value starting at `i`.
fn scan_value(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => scan_string(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = scan_string(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            None
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// A member of the top-level object: key start, value end, key.
struct Member {
    key_start: usize,
    value_end: usize,
    key: String,
}

fn top_members(b: &[u8]) -> Option<Vec<Member>> {
    let start = if b.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    let mut i = skip_ws(b, start);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    i = skip_ws(b, i + 1);
    let mut out = Vec::new();
    if b.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        let key_start = i;
        let key_end = scan_string(b, i)?;
        let key: String = serde_json::from_slice(&b[key_start..key_end]).ok()?;
        i = skip_ws(b, key_end);
        if b.get(i) != Some(&b':') {
            return None;
        }
        i = skip_ws(b, i + 1);
        let value_end = scan_value(b, i)?;
        out.push(Member { key_start, value_end, key });
        i = skip_ws(b, value_end);
        match b.get(i)? {
            b',' => i = skip_ws(b, i + 1),
            b'}' => return Some(out),
            _ => return None,
        }
    }
}

fn parse_loose(b: &[u8]) -> Option<Value> {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    serde_json::from_slice(b).ok()
}

/// The description without the encryption fields: all other bytes as they
/// were (BOM, line endings, indentation, order). `Ok(None)` means the fields
/// are absent and there is nothing to edit. An error means the description
/// cannot be parsed (or the result did not match the expectation).
pub fn strip_encryption_fields(raw: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let members = top_members(raw).ok_or_else(|| "ResourceConfig.txt: not a JSON object".to_string())?;
    let gone = |m: &Member| ENCRYPTION_FIELDS.contains(&m.key.as_str());
    if !members.iter().any(gone) {
        return Ok(None);
    }
    // The pieces to cut out. If there is a remaining member before it, cut from
    // the end of the previous member's value (with its comma) to the end of our
    // value; otherwise from our key to the next key (together with the comma).
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut kept_before = false;
    for (i, m) in members.iter().enumerate() {
        if !gone(m) {
            kept_before = true;
            continue;
        }
        if kept_before {
            cuts.push((members[i - 1].value_end, m.value_end));
        } else {
            let end = members.get(i + 1).map_or(m.value_end, |n| n.key_start);
            cuts.push((m.key_start, end));
        }
    }
    let mut out = Vec::with_capacity(raw.len());
    let mut pos = 0;
    for (a, b) in cuts {
        let a = a.max(pos);
        out.extend_from_slice(&raw[pos..a]);
        pos = pos.max(b);
    }
    out.extend_from_slice(&raw[pos..]);

    // Check: the result is the same object without the two fields.
    let mut want = parse_loose(raw).ok_or_else(|| "ResourceConfig.txt: bad JSON".to_string())?;
    if let Some(o) = want.as_object_mut() {
        for k in ENCRYPTION_FIELDS {
            o.remove(*k);
        }
    }
    if parse_loose(&out).as_ref() != Some(&want) {
        return Err("ResourceConfig.txt: edit check failed".into());
    }
    Ok(Some(out))
}

// ============================================================================
// Writing to disk
// ============================================================================

/// Writes `data` to `target` atomically: a temporary file in the same folder →
/// fsync → rename → fsync of the folder. Permissions are those of the old file.
pub fn write_bytes_atomically(target: &Path, data: &[u8]) -> io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let res = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        if let Ok(m) = fs::metadata(target) {
            let _ = f.set_permissions(m.permissions());
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, target)?;
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(())
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Saves a copy of `src` to `dst`: a hard link, otherwise a copy with fsync.
fn keep_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let _ = fs::remove_file(dst);
    if fs::hard_link(src, dst).is_ok() {
        return Ok(());
    }
    let tmp = dst.with_extension("part");
    let res = (|| {
        fs::copy(src, &tmp)?;
        File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, dst)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Puts `src` back in place of `dst` atomically (by renaming; across file
/// systems, by copying into a temporary file next to `dst` and renaming).
fn put_back(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    let dir = dst.parent().unwrap_or(Path::new("."));
    let name = dst.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let res = (|| {
        fs::copy(src, &tmp)?;
        File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, dst)?;
        fs::remove_file(src)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Removes abandoned temporary preparation files next to `file`
/// (`.<name>.tmp-<pid>`: Rexon was interrupted in the middle of a write).
fn clean_stale_temps(file: &Path) {
    let (Some(dir), Some(name)) = (file.parent(), file.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let prefix = format!(".{name}.tmp-");
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with(&prefix) {
            crate::log_info!("axon", "убран брошенный временный файл {}", e.path().display());
            let _ = fs::remove_file(e.path());
        }
    }
}

// ============================================================================
// A single wallpaper
// ============================================================================

/// How the preparation handled a single wallpaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Decrypted this time (or an interrupted description was finished).
    Prepared,
    /// No encryption: prepared earlier, or an old wallpaper — left alone.
    NotEncrypted,
}

/// The wallpaper's backup directory.
pub fn backup_dir(originals_root: &Path, w: &AxonWallpaper) -> PathBuf {
    originals_root.join(w.cache_key())
}

/// Prepares the wallpaper directory `resource_dir` in place. `backup` is the
/// backup directory of this wallpaper; `keep_media` means also keeping the
/// encrypted original there. An error is a text for the log.
pub fn prepare_dir_with_key(
    key: &[u8],
    resource_dir: &Path,
    backup: &Path,
    keep_media: bool,
) -> Result<Outcome, String> {
    let config_path = resource_dir.join(ax::RESOURCE_CONFIG);
    let raw = fs::read(&config_path).map_err(|e| format!("{}: {e}", config_path.display()))?;
    let info = axon_extract::parse_config(&raw).map_err(|e| e.to_string())?;
    match info.zip_encrypted {
        None => return Ok(Outcome::NotEncrypted),
        Some(false) => {
            return Err(format!("unsupported encryption {:?}", info.scheme.unwrap_or_default()));
        }
        Some(true) => {}
    }
    let container = resource_dir.join(&info.source);
    if !container.is_file() {
        return Err(format!("{}: no such file", container.display()));
    }
    let stripped = strip_encryption_fields(&raw)?
        .ok_or_else(|| "ResourceConfig.txt: encryption field not found".to_string())?;
    let backup_config = backup.join(ax::RESOURCE_CONFIG);

    let is_zip = axon_extract::is_zip_file(&container).map_err(|e| format!("{}: {e}", container.display()))?;
    if !is_zip {
        // The video is already decrypted, the description is old: interrupted before
        // step 4. We finish only if the description is exactly the one in the backup.
        if fs::read(&backup_config).ok().as_deref() == Some(raw.as_slice()) {
            write_bytes_atomically(&config_path, &stripped)
                .map_err(|e| format!("{}: {e}", config_path.display()))?;
            crate::log_info!("axon", "дописано описание после обрыва: {}", resource_dir.display());
            return Ok(Outcome::Prepared);
        }
        return Err(format!("{}: not a ZIP container", container.display()));
    }

    // 1. Backup of the description (and where it came from).
    fs::create_dir_all(backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    write_bytes_atomically(&backup_config, &raw).map_err(|e| format!("{}: {e}", backup_config.display()))?;
    let origin = serde_json::json!({
        "dir": resource_dir.to_string_lossy(),
        "source": info.source.to_string_lossy(),
    });
    write_bytes_atomically(&backup.join(ORIGIN_FILE), origin.to_string().as_bytes())
        .map_err(|e| format!("{}: {e}", backup.display()))?;

    // 2. The encrypted original, depending on the setting.
    if keep_media {
        keep_copy(&container, &backup.join(&info.file_name))
            .map_err(|e| format!("{}: {e}", backup.display()))?;
    }

    // 3. Decryption over the container.
    clean_stale_temps(&container);
    axon_extract::decrypt_resource_with_key(key, &raw, &container, &container).map_err(|e| match e {
        ExtractError::Config(m) | ExtractError::Format(m) | ExtractError::Write(m) => m,
    })?;

    // 4. The description without the encryption fields.
    write_bytes_atomically(&config_path, &stripped).map_err(|e| format!("{}: {e}", config_path.display()))?;
    Ok(Outcome::Prepared)
}

/// The state of a wallpaper right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Encrypted: waiting for preparation.
    Encrypted,
    /// Prepared by Rexon (a backup of the description exists).
    Prepared,
    /// Not our business: not downloaded, web, old without encryption, unreadable.
    Other,
}

pub fn state_of(w: &AxonWallpaper, originals_root: &Path) -> State {
    let Some(dir) = w.resource_dir.as_ref().filter(|_| w.downloaded && w.kind != AxonKind::Web) else {
        return State::Other;
    };
    let Ok(raw) = fs::read(dir.join(ax::RESOURCE_CONFIG)) else { return State::Other };
    match axon_extract::parse_config(&raw).map(|i| i.zip_encrypted) {
        Ok(Some(true)) => State::Encrypted,
        Ok(None) if backup_dir(originals_root, w).join(ax::RESOURCE_CONFIG).is_file() => State::Prepared,
        _ => State::Other,
    }
}

/// The finished file right in the Axon library, if the wallpaper was prepared in advance.
pub fn ready_in_place(w: &AxonWallpaper, originals_root: &Path) -> Option<PathBuf> {
    if state_of(w, originals_root) != State::Prepared {
        return None;
    }
    let container = w.container.as_ref()?;
    match axon_extract::is_zip_file(container) {
        Ok(false) => Some(container.clone()),
        _ => None,
    }
}

/// Prepares a wallpaper for "Apply": one prepared in advance gives the file
/// straight from `Resource/`, the others go the old way through the cache.
pub fn prepare_for_apply(
    w: &AxonWallpaper,
    extractor: Option<&dyn AxonExtractor>,
    env: &AxonEnv,
) -> Result<Prepared, String> {
    if let Some(path) = ready_in_place(w, &env.originals_root()) {
        if let Some(format) = ax::apply_format(&path) {
            let audio = w.audible && w.kind == AxonKind::Video;
            return Ok(Prepared { path, format, audio, name: ax::display_title(w) });
        }
    }
    ax::prepare(w, extractor, &env.cache_root())
}

// ============================================================================
// Busy state
// ============================================================================

/// Files opened by the user's processes (`/proc/*/fd`), only those lying under
/// one of `roots`.
pub fn open_files_under(roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    let Ok(procs) = fs::read_dir("/proc") else { return out };
    for p in procs.flatten() {
        if !p.file_name().to_string_lossy().bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(fds) = fs::read_dir(p.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            if let Ok(target) = fs::read_link(fd.path()) {
                if roots.iter().any(|r| target.starts_with(r)) {
                    out.insert(target);
                }
            }
        }
    }
    out
}

/// Whether Axon (or anyone else) is showing this wallpaper right now.
pub fn is_busy(w: &AxonWallpaper, open: &HashSet<PathBuf>) -> bool {
    if w.playing || w.status.eq_ignore_ascii_case("Playing") {
        return true;
    }
    let Some(c) = &w.container else { return false };
    let real = c.canonicalize().unwrap_or_else(|_| c.clone());
    open.contains(&real) || open.contains(c)
}

// ============================================================================
// A pass over the library
// ============================================================================

/// Preparation settings.
#[derive(Debug, Clone)]
pub struct Options {
    pub originals_root: PathBuf,
    pub keep_media: bool,
    pub pause: Duration,
}

impl Options {
    pub fn from_settings(env: &AxonEnv, settings: &serde_json::Map<String, Value>) -> Options {
        Options {
            originals_root: env.originals_root(),
            keep_media: settings.get(KEEP_ORIGINALS_KEY).and_then(Value::as_bool).unwrap_or(false),
            pause: PAUSE_BETWEEN,
        }
    }
}

/// Whether advance preparation is enabled.
pub fn enabled(settings: &serde_json::Map<String, Value>) -> bool {
    settings.get(PREPARE_AHEAD_KEY).and_then(Value::as_bool).unwrap_or(true)
}

/// The result of a pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassReport {
    pub prepared: usize,
    pub busy: usize,
    pub failed: usize,
}

/// A single pass over the library: one wallpaper at a time, no parallelism.
/// `busy` tells whether a wallpaper is busy right now; `stop` interrupts the
/// pass (disabled, or we are exiting).
pub fn run_pass_with_key(
    key: &[u8],
    lib: &AxonLibrary,
    opts: &Options,
    busy: &dyn Fn(&AxonWallpaper) -> bool,
    stop: &dyn Fn() -> bool,
) -> PassReport {
    let mut report = PassReport::default();
    let mut first = true;
    for w in &lib.wallpapers {
        if state_of(w, &opts.originals_root) != State::Encrypted {
            continue;
        }
        if stop() {
            break;
        }
        if busy(w) {
            crate::log_info!("axon", "обои {} сейчас на экране — подготовка позже", w.id);
            report.busy += 1;
            continue;
        }
        if !first && !opts.pause.is_zero() {
            std::thread::sleep(opts.pause);
        }
        first = false;
        let Some(dir) = &w.resource_dir else { continue };
        let _guard = WORK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match prepare_dir_with_key(key, dir, &backup_dir(&opts.originals_root, w), opts.keep_media) {
            Ok(Outcome::Prepared) => {
                crate::log_info!("axon", "обои {} подготовлены заранее: {}", w.id, dir.display());
                report.prepared += 1;
            }
            Ok(Outcome::NotEncrypted) => {}
            Err(why) => {
                crate::log_warn!("axon", "обои {} ({}) не подготовлены заранее: {why}", w.id, dir.display());
                report.failed += 1;
            }
        }
    }
    report
}

/// How many have been prepared and how many remain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub prepared: usize,
    pub remaining: usize,
}

pub fn counts(lib: &AxonLibrary, originals_root: &Path) -> Counts {
    let mut c = Counts::default();
    for w in &lib.wallpapers {
        match state_of(w, originals_root) {
            State::Prepared => c.prepared += 1,
            State::Encrypted => c.remaining += 1,
            State::Other => {}
        }
    }
    c
}

// ============================================================================
// Restoring the original descriptions
// ============================================================================

/// The result of restoring.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestoreReport {
    /// Descriptions restored.
    pub restored: usize,
    /// Of these, together with the encrypted original video.
    pub with_media: usize,
    pub failed: usize,
}

/// Restores the original `ResourceConfig.txt` files from the backups (and the
/// videos, if the originals were kept). Wallpapers whose description is
/// already the original are skipped.
pub fn restore_originals(originals_root: &Path) -> RestoreReport {
    let _guard = WORK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = RestoreReport::default();
    let Ok(rd) = fs::read_dir(originals_root) else { return r };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for b in dirs {
        match restore_one(&b) {
            Ok(None) => {}
            Ok(Some(media)) => {
                r.restored += 1;
                if media {
                    r.with_media += 1;
                }
            }
            Err(why) => {
                crate::log_warn!("axon", "описание из {} не возвращено: {why}", b.display());
                r.failed += 1;
            }
        }
    }
    r
}

/// `Ok(None)` means there is nothing to restore; `Ok(Some(with the video?))`.
fn restore_one(backup: &Path) -> Result<Option<bool>, String> {
    let origin = fs::read(backup.join(ORIGIN_FILE)).map_err(|e| format!("{ORIGIN_FILE}: {e}"))?;
    let origin: Value = serde_json::from_slice(&origin).map_err(|e| format!("{ORIGIN_FILE}: {e}"))?;
    let dir = PathBuf::from(origin.get("dir").and_then(Value::as_str).unwrap_or_default());
    let source = origin.get("source").and_then(Value::as_str).unwrap_or_default();
    let saved = fs::read(backup.join(ax::RESOURCE_CONFIG)).map_err(|e| format!("{}: {e}", ax::RESOURCE_CONFIG))?;
    let config_path = dir.join(ax::RESOURCE_CONFIG);
    let current = fs::read(&config_path).map_err(|e| format!("{}: {e}", config_path.display()))?;
    if current == saved {
        return Ok(None);
    }
    let info = axon_extract::parse_config(&current).map_err(|e| e.to_string())?;
    if info.zip_encrypted.is_some() {
        // The description has encryption again: Axon downloaded the wallpaper anew.
        return Ok(None);
    }
    let container = dir.join(source);
    let media = backup.join(&info.file_name);
    let with_media = media.is_file();
    if with_media {
        put_back(&media, &container).map_err(|e| format!("{}: {e}", container.display()))?;
    }
    write_bytes_atomically(&config_path, &saved).map_err(|e| format!("{}: {e}", config_path.display()))?;
    Ok(Some(with_media))
}

// ============================================================================
// Background
// ============================================================================

/// One preparation or restore at a time.
static WORK_LOCK: Mutex<()> = Mutex::new(());

struct Bg {
    running: bool,
    rerun: bool,
}

static BG: Mutex<Bg> = Mutex::new(Bg { running: false, rerun: false });
static STOP: AtomicBool = AtomicBool::new(false);

/// Whether background preparation is running.
pub fn is_running() -> bool {
    BG.lock().map(|b| b.running).unwrap_or(false)
}

/// Interrupts background preparation after the current wallpaper.
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// The live process environment with folders from Rexon's settings.
pub fn live_env() -> (AxonEnv, serde_json::Map<String, Value>) {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    let mut env = AxonEnv::from_process();
    env.source_dirs = ax::saved_source_dirs(&settings);
    (env, settings)
}

/// Low priority for our own thread: nice 19 and "idle" I/O.
fn lower_priority() {
    // SAFETY: system calls on our own thread, without pointers.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 19);
        // IOPRIO_WHO_PROCESS = 1, IOPRIO_CLASS_IDLE = 3 << 13.
        libc::syscall(libc::SYS_ioprio_set, 1, tid as libc::c_long, (3 << 13) as libc::c_long);
    }
}

/// Starts background preparation after `delay` (if it is already running,
/// another pass follows the current one). If disabled in the settings, the
/// thread exits immediately.
pub fn spawn_background(delay: Duration) {
    {
        let mut bg = BG.lock().unwrap_or_else(|e| e.into_inner());
        // A new request cancels the previous stop — for the running thread too,
        // otherwise enabling it while it is working would be lost.
        STOP.store(false, Ordering::SeqCst);
        if bg.running {
            bg.rerun = true;
            return;
        }
        bg.running = true;
        bg.rerun = false;
    }
    let spawned = std::thread::Builder::new().name("rexon-axon-prepare".into()).spawn(move || {
        std::thread::sleep(delay);
        lower_priority();
        background_loop();
    });
    if let Err(e) = spawned {
        crate::log_warn!("axon", "фоновая подготовка не запущена: {e}");
        BG.lock().unwrap_or_else(|e| e.into_inner()).running = false;
    }
}

fn background_loop() {
    loop {
        let (env, settings) = live_env();
        let mut wait_busy = false;
        if enabled(&settings) && !STOP.load(Ordering::SeqCst) {
            let lib = ax::load_library(&env);
            let opts = Options::from_settings(&env, &settings);
            let roots: Vec<PathBuf> = lib
                .wallpapers
                .iter()
                .filter_map(|w| w.resource_dir.as_ref())
                .filter_map(|d| d.parent().map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf())))
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();
            let stop = || STOP.load(Ordering::SeqCst) || !enabled(&crate::settings::load_settings(&crate::settings::settings_file()));
            // Open files are checked before each wallpaper: the player may have
            // switched while the previous ones were being prepared.
            let busy = |w: &AxonWallpaper| is_busy(w, &open_files_under(&roots));
            let report = run_pass_with_key(axon_extract::RAZER_HMAC_KEY, &lib, &opts, &busy, &stop);
            if report != PassReport::default() {
                crate::log_info!(
                    "axon",
                    "подготовка заранее: готово {}, заняты {}, неудачи {}",
                    report.prepared,
                    report.busy,
                    report.failed
                );
            }
            wait_busy = report.busy > 0;
        }
        if wait_busy {
            // Busy ones: try later; an early restart: right away.
            let mut waited = Duration::ZERO;
            while waited < RETRY_BUSY_AFTER && !STOP.load(Ordering::SeqCst) {
                if BG.lock().map(|b| b.rerun).unwrap_or(false) {
                    break;
                }
                std::thread::sleep(Duration::from_secs(5));
                waited += Duration::from_secs(5);
            }
        }
        let mut bg = BG.lock().unwrap_or_else(|e| e.into_inner());
        if bg.rerun || (wait_busy && !STOP.load(Ordering::SeqCst)) {
            bg.rerun = false;
            continue;
        }
        bg.running = false;
        return;
    }
}

// ============================================================================
// Texts
// ============================================================================

/// The switch of the "Razer Axon" block in the "⚙️ Settings" of the wallpaper section.
pub fn toggle_text() -> String {
    tr("Prepare wallpapers in advance")
}

pub fn toggle_tooltip() -> String {
    tr("Rexon decrypts downloaded Razer Axon wallpapers in the background, right in the Razer Axon \
        library, so that “Apply” starts at once. Razer Axon keeps showing them.")
}

/// The summary line.
pub fn status_text(c: Counts, running: bool) -> String {
    // TRANSLATORS: %1 and %2 are numbers of Razer Axon wallpapers.
    let line = trf("Prepared: %1, remaining: %2.", &[&c.prepared, &c.remaining]);
    if running && c.remaining > 0 {
        format!("{line} {}", tr("Preparing in the background…"))
    } else {
        line
    }
}

pub fn restore_button_text() -> String {
    tr("Restore original descriptions")
}

/// An honest warning before restoring.
pub fn restore_warning_text() -> String {
    tr("The original ResourceConfig.txt files saved before preparation will be put back. \
        The wallpaper files themselves are already decrypted: unless the encrypted originals were kept, \
        Razer Axon will not show these wallpapers with the restored descriptions until you download them again. \
        Preparing in advance will be turned off.")
}

pub fn restore_confirm_text() -> String {
    tr("Restore descriptions")
}

pub fn restore_done_text(r: RestoreReport) -> String {
    if r.restored == 0 && r.failed == 0 {
        return tr("No saved descriptions to restore.");
    }
    // TRANSLATORS: %1 — descriptions restored, %2 — of them together with the
    // original encrypted file, %3 — failures.
    trf("Descriptions restored: %1 (with original files: %2). Failed: %3.", &[&r.restored, &r.with_media, &r.failed])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_compact_last_fields() {
        let raw = br#"{"WallPaperType":"VIDEO","Source":"Resource\\a.mp4","Integrity":["Resource\\a.mp4"],"SourceEncryptedTypes":"ZIP","SourceEncryptedLevel":"LOW"}"#;
        let out = strip_encryption_fields(raw).unwrap().unwrap();
        assert_eq!(
            out,
            br#"{"WallPaperType":"VIDEO","Source":"Resource\\a.mp4","Integrity":["Resource\\a.mp4"]}"#
        );
    }

    #[test]
    fn strips_with_bom_crlf_and_tabs() {
        let raw = "\u{feff}{\r\n\t\"Type\": \"VIDEO\",\r\n\t\"SourceEncryptedTypes\": \"ZIP\",\r\n\t\"Source\": \"Resource\\\\a.mp4\",\r\n\t\"SourceEncryptedLevel\": \"LOW\"\r\n}\r\n";
        let out = strip_encryption_fields(raw.as_bytes()).unwrap().unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\u{feff}{\r\n\t\"Type\": \"VIDEO\",\r\n\t\"Source\": \"Resource\\\\a.mp4\"\r\n}\r\n"
        );
    }

    #[test]
    fn strips_leading_fields_and_keeps_strings_with_braces() {
        let raw = br#"{ "SourceEncryptedLevel":"LOW" , "SourceEncryptedTypes":"ZIP", "Title":"a } \" , b", "Source":"x"}"#;
        let out = strip_encryption_fields(raw).unwrap().unwrap();
        assert_eq!(out, br#"{ "Title":"a } \" , b", "Source":"x"}"#);
        // Only these fields: an empty object.
        let only = br#"{"SourceEncryptedTypes":"ZIP","SourceEncryptedLevel":"LOW"}"#;
        assert_eq!(strip_encryption_fields(only).unwrap().unwrap(), b"{}");
    }

    #[test]
    fn nothing_to_strip_and_bad_json() {
        assert_eq!(strip_encryption_fields(br#"{"Source":"x"}"#).unwrap(), None);
        assert!(strip_encryption_fields(b"[1]").is_err());
        assert!(strip_encryption_fields(b"{\"a\":").is_err());
        // A field deeper than the top level is left alone.
        assert_eq!(strip_encryption_fields(br#"{"x":{"SourceEncryptedTypes":"ZIP"}}"#).unwrap(), None);
    }

    #[test]
    fn status_line() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(status_text(Counts { prepared: 2, remaining: 1 }, false), "Prepared: 2, remaining: 1.");
        assert_eq!(restore_done_text(RestoreReport::default()), "No saved descriptions to restore.");
    }
}
