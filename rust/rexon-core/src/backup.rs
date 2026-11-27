//! Backups of DE configs: a port of the deterministic parts of
//! `rexon/backup.py` (`BackupManager`, `PlasmaProtector`).
//!
//! ONLY the pure input->output pieces are ported (checked against Python
//! itself by the golden test `backup_parity.rs`):
//!   * the staging/rollback suffix constants and the Plasma config lists
//!     (`PANEL_CONFIGS`, `FULL_CONFIGS`);
//!   * the backup name `{de}_{timestamp}` (`backup_name`);
//!   * the safe file name in a snapshot: `relpath(path, home)` with the
//!     separator replaced and leading `.`/`_` stripped (`safe_config_name`,
//!     `posix_relpath`);
//!   * Plasma detection by `XDG_CURRENT_DESKTOP` (`is_plasma_desktop`);
//!   * expanding `~` in a list of paths (`expand_tilde`);
//!   * metadata selections/sorts (`de_backups_for`, `plasma_backups`,
//!     `plasma_latest_backup`, `plasma_has_backups`).
//!
//! Since 2026-07-30 the transactional restore engine ITSELF has been ported
//! too (`transactional_restore`): it fundamentally cannot be expressed as
//! a "plan": staging -> atomic swap -> commit/rollback is a state machine
//! over the filesystem, and half of the rollback would live in the shim. It
//! works directly through `std::fs` (like `ext_utils.rs`/`settings.rs`),
//! repairs its own state on failure and NEVER panics: any error is
//! returned in [`RestoreOutcome`], so for the shim "a bridge exception"
//! means "the filesystem was never reached", and the fallback is safe.
//! Together with it the decisions of both managers are ported: the
//! snapshot layout, choosing the entry to restore, rotation of Plasma
//! snapshots, the plasmashell restart argv and all user messages.
//!
//! ⚠ Rule #16: in the original, on a failure IN THE MIDDLE of the atomic
//! swap, an item whose `rename(dest -> rollback)` had already happened but
//! whose `rename(stage -> dest)` had not yet did not get into the `swapped`
//! list: it was not rolled back, and the user was left WITHOUT the file
//! (the rollback copy was orphaned next to it). The port tracks such an
//! "in flight" item separately and rolls it back too; the same fix was
//! made in the Python reference `_transactional_restore` (it remains the
//! fallback).
//!
//! What stays on the Python side: `glob` of config patterns, reading/writing
//! the metadata JSON, calling `subprocess` for plasmashell.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

use crate::system_plan::Cmd;
use crate::i18n::{ntr, tr, trf};

/// Suffix of the staging copy in a transactional restore (`dest + _STAGE_SUFFIX`).
pub const STAGE_SUFFIX: &str = ".rexon-new";
/// Suffix of the rollback copy of the original during the atomic swap.
pub const ROLLBACK_SUFFIX: &str = ".rexon-rollback";

/// Critical Plasma panel configs (`PlasmaProtector.PANEL_CONFIGS`).
pub const PANEL_CONFIGS: &[&str] = &[
    "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
    "~/.config/plasmashellrc",
    "~/.config/plasmarc",
    "~/.config/plasmanotifyrc",
    "~/.config/plasma-localerc",
];

/// The extended set of Plasma configs (`FULL_CONFIGS = PANEL_CONFIGS + extra`).
/// Order = the panel ones, then the extra ones (matters for copying/checks).
pub const FULL_CONFIGS: &[&str] = &[
    "~/.config/plasma-org.kde.plasma.desktop-appletsrc",
    "~/.config/plasmashellrc",
    "~/.config/plasmarc",
    "~/.config/plasmanotifyrc",
    "~/.config/plasma-localerc",
    "~/.config/kdeglobals",
    "~/.config/kwinrc",
    "~/.config/kglobalshortcutsrc",
    "~/.config/kcminputrc",
    "~/.config/kscreenlockerrc",
    "~/.config/ksmserverrc",
    "~/.config/klaunchrc",
    "~/.config/khotkeysrc",
    "~/.config/lattedockrc",
    "~/.local/share/plasma",
];

/// Plasma keyboard shortcuts taken together with the panel by a manual copy:
/// the global shortcuts file and the person's own command shortcuts
/// (Plasma 6 keeps them as `.desktop` files next to it). The section's
/// tooltip has always promised them; before 25.11.2026 a manual copy took only
/// [`PANEL_CONFIGS`], so the shortcuts were silently missing from it.
pub const PANEL_SHORTCUT_CONFIGS: &[&str] = &["~/.config/kglobalshortcutsrc", "~/.local/share/kglobalaccel"];

/// The backup name: `f"{de_name}_{timestamp}"` (`create_backup`).
pub fn backup_name(de_name: &str, timestamp: &str) -> String {
    format!("{de_name}_{timestamp}")
}

/// Expand `~` in each path to `home` (a port of `PlasmaProtector._expand`).
/// Like Python's `str.replace("~", home)`: replaces ALL occurrences of `~`.
pub fn expand_tilde(paths: &[&str], home: &str) -> Vec<String> {
    expand_tilde_xdg(paths, home, None)
}

/// The same, honouring `$XDG_CONFIG_HOME`: a leading `~/.config` goes to
/// the config root (the core's single expander,
/// [`crate::utils::expand_user`]), and the other `~` are expanded to
/// `home` as before. Without the variable the result matches
/// [`expand_tilde`] byte for byte.
pub fn expand_tilde_xdg(
    paths: &[&str],
    home: &str,
    xdg_config_home: Option<&str>,
) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            let head = crate::utils::expand_user(p, home, xdg_config_home);
            // trailing `~` inside the path (the original's quirk: replace ALL)
            head.replace('~', home)
        })
        .collect()
}

/// Detect a running Plasma by `XDG_CURRENT_DESKTOP`
/// (`PlasmaProtector.is_plasma_running`): `"kde"` or `"plasma"` in the
/// lowercased string.
pub fn is_plasma_desktop(xdg_current_desktop: &str) -> bool {
    let de = xdg_current_desktop.to_lowercase();
    de.contains("kde") || de.contains("plasma")
}

/// Normalised components of an absolute POSIX path (the analogue of
/// `os.path.normpath` for an absolute path: drop empty ones and `.`,
/// collapse `..` without going above the root).
fn norm_abs_components(path: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|&l| l != "..") {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The relative path from `start` to `path` for absolute POSIX paths: a
/// port of `os.path.relpath` (without resolving symlinks). `path == start`
/// -> `"."`.
pub fn posix_relpath(path: &str, start: &str) -> String {
    let sc = norm_abs_components(start);
    let pc = norm_abs_components(path);
    let i = sc.iter().zip(pc.iter()).take_while(|(a, b)| a == b).count();
    let mut rel: Vec<&str> = vec![".."; sc.len() - i];
    rel.extend_from_slice(&pc[i..]);
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel.join("/")
    }
}

/// A safe file name in a snapshot, without collisions (the `create_backup`
/// logic): `relpath(path, home)` -> replace `/` with `_` -> strip leading
/// `.`/`_`. `~/.config/gtk-3.0/settings.ini` -> `config_gtk-3.0_settings.ini`.
pub fn safe_config_name(path: &str, home: &str) -> String {
    let rel = posix_relpath(path, home);
    rel.replace('/', "_")
        .trim_start_matches(['.', '_'])
        .to_string()
}

// ── Metadata: selections and sorts (pure) ──────────────────────────────────

/// Backup summary for a specific DE (`BackupManager.get_backups_for_de`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeBackupSummary {
    pub name: String,
    pub timestamp: String,
    pub files_count: usize,
}

/// A `BackupManager` metadata entry (the value of `metadata[name]`), from
/// which `de_name`, `timestamp` and the number of saved paths are taken.
#[derive(Debug, Clone)]
pub struct DeMetaEntry {
    pub name: String,
    pub de_name: String,
    pub timestamp: String,
    pub files_count: usize,
}

/// Backups for a DE, newest first (`get_backups_for_de`): filter by
/// `de_name`, then a stable sort by `timestamp` descending (like Python's
/// `sorted(..., reverse=True)`: with equal keys insertion order is kept).
pub fn de_backups_for(entries: &[DeMetaEntry], de_name: &str) -> Vec<DeBackupSummary> {
    let mut v: Vec<DeBackupSummary> = entries
        .iter()
        .filter(|e| e.de_name == de_name)
        .map(|e| DeBackupSummary {
            name: e.name.clone(),
            timestamp: e.timestamp.clone(),
            files_count: e.files_count,
        })
        .collect();
    v.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    v
}

/// One Plasma backup entry (`PlasmaProtector._meta["backups"][i]`).
///
/// `deny_unknown_fields`: Python returns the meta file entries AS IS
/// (together with unknown keys); a typed parse would silently lose them;
/// instead there is a parse error, and the shim goes to the Python
/// fallback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlasmaBackup {
    pub timestamp: String,
    pub reason: String,
    pub path: String,
    pub files: Vec<String>,
    pub full: bool,
}

/// The list of Plasma backups, newest first (`get_backups` = `reversed(...)`).
pub fn plasma_backups(backups: &[PlasmaBackup]) -> Vec<PlasmaBackup> {
    backups.iter().rev().cloned().collect()
}

/// The latest Plasma backup (`get_latest_backup` = `backups[-1]` or None).
pub fn plasma_latest_backup(backups: &[PlasmaBackup]) -> Option<PlasmaBackup> {
    backups.last().cloned()
}

/// Whether there are Plasma backups (`has_backups`).
pub fn plasma_has_backups(backups: &[PlasmaBackup]) -> bool {
    !backups.is_empty()
}

// ══════════════════════════════════════════════════════════════════════════
//  Transactional restore (a port of `_transactional_restore`)
// ══════════════════════════════════════════════════════════════════════════

/// The transaction result: the original's `(ok, message, count_restored)` tuple.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RestoreOutcome {
    pub ok: bool,
    pub message: String,
    pub restored: usize,
}

/// `os.path.exists(p) or os.path.islink(p)`: the path is taken (a broken link too).
fn exists_or_link(p: &Path) -> bool {
    p.symlink_metadata().is_ok()
}

/// A port of `_remove_path`: a symlink/file is unlinked, a directory is
/// removed recursively, a missing path is not an error (the original has
/// `ignore_errors=True`).
fn remove_path(p: &Path) {
    if let Ok(md) = p.symlink_metadata() {
        if md.is_dir() {
            let _ = fs::remove_dir_all(p);
        } else {
            let _ = fs::remove_file(p);
        }
    }
}

/// A file copy "as is": a port of `shutil.copy2(..., follow_symlinks=False)`:
/// a symlink is reproduced as a symlink, a regular file keeps its
/// permissions and modification time (without the time the config would
/// look freshly changed, and DE daemons watch mtime).
fn copy_file_like(src: &Path, dst: &Path) -> io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
        return Ok(());
    }
    fs::copy(src, dst)?;
    if let (Ok(modified), Ok(f)) = (
        md.modified(),
        fs::OpenOptions::new().write(true).open(dst),
    ) {
        let mut t = fs::FileTimes::new().set_modified(modified);
        if let Ok(accessed) = md.accessed() {
            t = t.set_accessed(accessed);
        }
        let _ = f.set_times(t);
    }
    Ok(())
}

/// A recursive directory copy: a port of `shutil.copytree(...,
/// symlinks=True)`: nested symlinks are NOT dereferenced (otherwise the
/// snapshot bloats and loses the structure of `~/.local/share/plasma`).
fn copy_tree(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    if let Ok(md) = fs::metadata(src) {
        let _ = fs::set_permissions(dst, md.permissions());
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            remove_path(&to);
            std::os::unix::fs::symlink(fs::read_link(&from)?, &to)?;
        } else if ft.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            copy_file_like(&from, &to)?;
        }
    }
    Ok(())
}

/// Transactional restore of a set of `(src, dest)`.
///
/// The original's guarantees are kept: either ALL dest are replaced or
/// none; staging sits next to dest (one filesystem, so `rename` is
/// atomic); on any error it returns to the original state. The function
/// does not panic: filesystem errors go into `message`, so an exception at
/// the bridge boundary means the filesystem was never reached, and the
/// fallback to Python is safe.
pub fn transactional_restore(items: &[(String, String)]) -> RestoreOutcome {
    // ── Phase 1: staging: copies next to the target, originals untouched ──
    let mut staged: Vec<(String, String)> = Vec::new();
    for (src, dest) in items {
        let src_p = Path::new(src);
        if !exists_or_link(src_p) {
            continue;
        }
        let stage = format!("{dest}{STAGE_SUFFIX}");
        let stage_p = Path::new(&stage);
        let staged_one = (|| -> io::Result<()> {
            let parent = Path::new(dest).parent().unwrap_or(Path::new(""));
            let parent =
                if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
            fs::create_dir_all(parent)?;
            // Remove staging left from a previous failed attempt.
            remove_path(stage_p);
            if fs::symlink_metadata(src_p)?.is_dir() {
                copy_tree(src_p, stage_p)
            } else {
                copy_file_like(src_p, stage_p)
            }
        })();
        if let Err(e) = staged_one {
            for (_d, s) in &staged {
                remove_path(Path::new(s));
            }
            remove_path(stage_p);
            return RestoreOutcome {
                ok: false,
                message: format!("staging failed: {e}"),
                restored: 0,
            };
        }
        staged.push((dest.clone(), stage));
    }

    // ── Phase 2: atomic swap: rename(dest -> rollback), rename(stage -> dest) ──
    let mut swapped: Vec<(String, Option<String>)> = Vec::new();
    for (dest, stage) in &staged {
        let dest_p = Path::new(dest);
        let mut rollback: Option<String> = None;
        // Whether the first rename went through: only after it has the target
        // "moved away" and must be returned. If the first rename itself failed,
        // the target is untouched, and touching it in the rollback is NOT
        // allowed (otherwise we would wipe the live config).
        let mut moved = false;
        let swap_one = (|| -> io::Result<()> {
            if exists_or_link(dest_p) {
                let rb = format!("{dest}{ROLLBACK_SUFFIX}");
                remove_path(Path::new(&rb));
                fs::rename(dest_p, &rb)?;
                rollback = Some(rb);
            }
            moved = true;
            fs::rename(stage, dest_p)
        })();
        if let Err(e) = swap_one {
            // ⚠ Rule #16: the original did NOT include an "in flight" item (the
            // target moved to rollback, but stage -> dest has not happened yet)
            // in the rollback, and the target stayed deleted. We add it to the
            // items being rolled back.
            let mut done = swapped.clone();
            if moved {
                done.push((dest.clone(), rollback));
            }
            for (d, rb) in &done {
                let dp = Path::new(d);
                remove_path(dp);
                if let Some(rb) = rb {
                    let rbp = Path::new(rb);
                    if exists_or_link(rbp) {
                        let _ = fs::rename(rbp, dp);
                    }
                }
            }
            // Remove staging that did not reach the swap: for the actually
            // swapped ones it has already been renamed to dest, for the "in
            // flight" item it is still in place.
            let done_stages: Vec<String> = swapped
                .iter()
                .map(|(d, _)| format!("{d}{STAGE_SUFFIX}"))
                .collect();
            for (_d, s) in &staged {
                if !done_stages.contains(s) && exists_or_link(Path::new(s)) {
                    remove_path(Path::new(s));
                }
            }
            return RestoreOutcome {
                ok: false,
                message: format!("swap failed, rolled back: {e}"),
                restored: 0,
            };
        }
        swapped.push((dest.clone(), rollback));
    }

    // ── Commit: the rollback copies are no longer needed ──
    for (_d, rb) in &swapped {
        if let Some(rb) = rb {
            remove_path(Path::new(rb));
        }
    }
    RestoreOutcome {
        ok: true,
        message: msg_transaction_restored(swapped.len()),
        restored: swapped.len(),
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  BackupManager: snapshot layout, choice of sources, messages
// ══════════════════════════════════════════════════════════════════════════

/// A successful transaction result (the internal message of
/// `_transactional_restore`).
///
/// The plural fix is the same as in [`msg_restored_items`]: this string
/// reaches the person's eyes inside the restore failure message.
pub fn msg_transaction_restored(n: usize) -> String {
    // TRANSLATORS: goes after a colon inside a restore failure message, hence lowercase.
    ntr("restored %n item", "restored %n items", n as u64)
}

/// The result of `BackupManager.restore_backup`.
///
/// Fix (rule #16): the count had one ending for all numbers, so the
/// customer read the Russian for "Restored 1 items".
pub fn msg_restored_items(n: usize) -> String {
    ntr("Restored %n item", "Restored %n items", n as u64)
}

/// `restore_backup`: there is no backup with this name in the metadata.
pub fn msg_backup_not_found() -> String {
    tr("Backup not found")
}

/// Joining paths like `os.path.join` for our cases (`base` is absolute,
/// `name` is always relative: `safe_config_name`/basename).
fn join(base: &str, name: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// A port of `os.path.basename` for POSIX paths.
fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// The DE snapshot layout (`create_backup`): where each config goes.
/// Returns `(src, dest)`; the name in the snapshot keeps the path structure.
pub fn snapshot_items(
    paths: &[String],
    backup_path: &str,
    home: &str,
) -> Vec<(String, String)> {
    paths
        .iter()
        .map(|p| (p.clone(), join(backup_path, &safe_config_name(p, home))))
        .collect()
}

/// Sources for `restore_backup`: `safe_config_name` with a fallback to the
/// basename (backward compatibility with snapshots of old versions); what
/// is missing is skipped.
pub fn restore_items(
    backup_path: &str,
    orig_paths: &[String],
    home: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut items = Vec::new();
    for orig in orig_paths {
        let mut src = join(backup_path, &safe_config_name(orig, home));
        if !exists(&src) {
            src = join(backup_path, basename(orig));
            if !exists(&src) {
                continue;
            }
        }
        items.push((src, orig.clone()));
    }
    items
}

/// The `metadata[backup_name]` entry (`create_backup`): key order as in the
/// Python dictionary, so that `metadata.json` matches byte for byte.
pub fn de_meta_entry(
    de_name: &str,
    timestamp: &str,
    paths: &[String],
    backup_path: &str,
) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    m.insert("de_name".into(), de_name.into());
    m.insert("timestamp".into(), timestamp.into());
    m.insert("paths".into(), paths.to_vec().into());
    m.insert("backup_path".into(), backup_path.into());
    serde_json::Value::Object(m)
}

// ══════════════════════════════════════════════════════════════════════════
//  PlasmaProtector: panel snapshots, rotation, choice, shell restart
// ══════════════════════════════════════════════════════════════════════════

/// How many panel snapshots we keep (`while len(...) > 10` in `backup_panel`).
pub const MAX_PLASMA_SNAPSHOTS: usize = 10;

/// Pause between stopping and starting plasmashell (`time.sleep(2)`).
pub const PLASMASHELL_SETTLE_SECS: u64 = 2;

// ⭐ The words of these messages are the same ones the "KDE Plasma panel
// protection" section itself is labelled with: "Save a copy", "Saved
// copies", "Copy folder". The original (Python) said "backup" and
// "snapshot" here, and the customer read two different names for one
// thing in one window. The divergence from the original is deliberate
// (rules #13 and #16); the guard is `tests/backup_parity.rs`.

/// `backup_panel`: there is nothing to copy.
///
/// It used to be "No files to back up": the word "backup" explains nothing
/// to the customer, and the phrase itself does not show which files were
/// not found.
pub fn msg_nothing_to_backup() -> String {
    tr("Nothing to copy: there are no panel settings files")
}
/// `restore_panel`: there are no panel snapshots at all.
///
/// It used to be "No Plasma backups"; the empty list in the window is
/// labelled with the same words ("No saved copies yet").
pub fn msg_no_plasma_backups() -> String {
    tr("No saved copies of the panel yet")
}
/// `restore_panel`: the transaction went through, but not a single file was restored.
pub fn msg_none_restored() -> String {
    tr("Couldn't restore any files")
}
/// The tail of the `restore_panel` message after the shell restart.
///
/// It used to be "Plasmashell restarted": the name of a service program
/// instead of what the person sees on screen. The neighbouring failure line
/// ([`msg_plasmashell_not_restarted`]) said "panel" from the very start.
///
/// The tail starts with a newline: it is appended to the restore result.
pub fn msg_plasmashell_restarted() -> String {
    format!("\n{}", tr("Panel restarted"))
}

/// The result of `backup_panel`.
///
/// Fix (rule #16): the count had one ending for all numbers, the Russian
/// for "Backup created (1 files)". The translation catalog handles the
/// plural ([`ntr`]), as with the product's other counting strings.
pub fn msg_backup_created(n: usize) -> String {
    ntr("Copy saved (%n file)", "Copy saved (%n files)", n as u64)
}

/// `restore_panel`: there is no snapshot with this timestamp.
///
/// The timestamp is shown in the same human form the rows of the copies
/// list are labelled with: the machine "20260730_010203" tells the
/// customer nothing.
pub fn msg_snapshot_not_found(timestamp: &str) -> String {
    // The timestamp goes at the end, not in the middle of the phrase: an
    // unparsed timestamp stays as is, and "Copy from no-such not found"
    // would read as broken Russian.
    // TRANSLATORS: %1 is the date and time the copy was made.
    trf("Copy not found: %1", &[&crate::qt_settings::pretty_backup_ts(timestamp)])
}

/// `restore_panel`: the snapshot directory has disappeared from disk.
pub fn msg_snapshot_dir_missing(dir: &str) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf("The copy's folder wasn't found: %1", &[&dir])
}

/// `restore_panel`: the transaction failed (the internal text comes from it).
pub fn msg_restore_failed(detail: &str) -> String {
    // TRANSLATORS: %1 explains what went wrong.
    trf("The restore failed: %1", &[&detail])
}

/// The result of `restore_panel` (before the line about the shell restart).
///
/// The same two fixes as in [`msg_backup_created`] and
/// [`msg_snapshot_not_found`]: plural forms of the count and a human
/// timestamp instead of the machine one.
pub fn msg_restored_files(n: usize, timestamp: &str) -> String {
    crate::i18n::fill(
        &ntr(
            // TRANSLATORS: %n is the number of files, %1 the date and time the copy was made.
            "Restored %n file from the copy of %1",
            "Restored %n files from the copy of %1",
            n as u64,
        ),
        &[&crate::qt_settings::pretty_backup_ts(timestamp)],
    )
}

/// The panel snapshot layout (`backup_panel`): the name in the snapshot is
/// the basename, because all PANEL/FULL configs lie in different
/// directories without name collisions. Missing configs do not get into
/// the snapshot.
pub fn plasma_snapshot_items(
    configs: &[String],
    snap_dir: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    configs
        .iter()
        .filter(|p| exists(p))
        .map(|p| (p.clone(), join(snap_dir, basename(p))))
        .collect()
}

/// The `_meta["backups"]` entry for a snapshot just taken.
pub fn plasma_entry(
    timestamp: &str,
    reason: &str,
    snap_dir: &str,
    saved: &[String],
    full: bool,
) -> PlasmaBackup {
    PlasmaBackup {
        timestamp: timestamp.to_string(),
        reason: reason.to_string(),
        path: snap_dir.to_string(),
        files: saved.to_vec(),
        full,
    }
}

/// Snapshot rotation: while there are more than [`MAX_PLASMA_SNAPSHOTS`],
/// drop the oldest ones (from the start of the list). Returns the
/// directories the executor must delete; the list itself is already
/// trimmed.
pub fn plasma_rotate(backups: &mut Vec<PlasmaBackup>) -> Vec<String> {
    let mut dropped = Vec::new();
    while backups.len() > MAX_PLASMA_SNAPSHOTS {
        dropped.push(backups.remove(0).path);
    }
    dropped
}

/// Choosing a snapshot to restore (`restore_panel`): by timestamp or the
/// latest. `Err` is a ready message for the user.
pub fn plasma_pick<'a>(
    backups: &'a [PlasmaBackup],
    timestamp: Option<&str>,
) -> Result<&'a PlasmaBackup, String> {
    if backups.is_empty() {
        return Err(msg_no_plasma_backups());
    }
    match timestamp.filter(|t| !t.is_empty()) {
        Some(ts) => backups
            .iter()
            .find(|b| b.timestamp == ts)
            .ok_or_else(|| msg_snapshot_not_found(ts)),
        // `backups[-1]`: the last by insertion order, not by timestamp.
        None => Ok(backups.last().expect("проверено на пустоту выше")),
    }
}

/// Sources for restoring the panel: `snap_dir/basename(orig)`; what is
/// missing is skipped (a port of the loop in `restore_panel`).
pub fn plasma_restore_items(
    entry: &PlasmaBackup,
    exists: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    entry
        .files
        .iter()
        .map(|orig| (join(&entry.path, basename(orig)), orig.clone()))
        .filter(|(src, _)| exists(src))
        .collect()
}

/// Index of the snapshot with the given timestamp (`delete_backup`); None means there is none.
pub fn plasma_delete_index(backups: &[PlasmaBackup], timestamp: &str) -> Option<usize> {
    backups.iter().position(|b| b.timestamp == timestamp)
}

/// The regular shell restart: `systemctl --user restart
/// plasma-plasmashell.service`.
///
/// ⚠️ Previously this was `kquitapp6 plasmashell`, which does not exist on
/// Ubuntu 24.04 (Plasma 5 and `kquitapp5` there), so the regular path on
/// the most widespread target distribution always failed and the shell was
/// finished off with `killall`. Besides, in two other places of the
/// project the same task was already solved correctly, via the unit
/// (`waywallen_kde_plugin.rs::restart_plasmashell_cmds`,
/// `app.rs::plasmashell_restart_argv`), so this was a third, lagging copy
/// of one mechanism (rule #12). The `plasma-plasmashell` unit brings the
/// shell up by itself, so on success neither stopping nor starting is
/// needed; if there is no systemd unit (a non-systemd system, Plasma
/// without systemd startup), [`plasma_restart_shell`] falls back to
/// `kquitapp` + `kstart`, and then to the PID of our own session's shell.
pub fn plasma_quit_cmd() -> Cmd {
    Cmd {
        argv: vec![
            "systemctl".into(),
            "--user".into(),
            "restart".into(),
            "plasma-plasmashell.service".into(),
        ],
        // The restart is synchronous: the unit must have time to come up (as in app.rs).
        timeout_secs: Some(15),
        lang_c: false,
    }
}

/// The regular shell stop when there is no unit: `kquitapp6 plasmashell`
/// (the executor substitutes `kquitapp5` on Plasma 5, `utils::TOOL_SETS`).
///
/// ⚠️ This used to be `killall plasmashell`: killing BY NAME, which hits
/// any process with that name in any session of this person and does not
/// tell the current session's shell from another's. `kquitapp` asks the
/// shell to quit over THIS session's D-Bus: it cannot reach another one.
pub fn plasma_kill_cmd() -> Cmd {
    Cmd {
        argv: vec!["kquitapp6".into(), "plasmashell".into()],
        timeout_secs: Some(10),
        lang_c: false,
    }
}

/// Bring the shell up again after stopping it: `kstart plasmashell`
/// (`kstart5` on Plasma 5). `kstart` starts the program detached from the
/// caller, so the panel will not die together with Rexon; there is no
/// timeout, the start is in the background.
pub fn plasma_start_cmd() -> Cmd {
    Cmd {
        argv: vec!["kstart".into(), "plasmashell".into()],
        timeout_secs: None,
        lang_c: false,
    }
}

/// Starting the shell directly: only when there is no `kstart` at all. The
/// executor detaches it from our process (`setsid`).
pub fn plasma_direct_start_cmd() -> Cmd {
    Cmd {
        argv: vec!["plasmashell".into()],
        timeout_secs: None,
        lang_c: false,
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  Panel snapshot executor (`PlasmaProtector` on the live filesystem)
// ══════════════════════════════════════════════════════════════════════════
//
// WHY HERE AND NOT IN THE SHIM. The snapshot decisions are above in this
// same file (`plasma_snapshot_items`, `plasma_entry`, `plasma_rotate`,
// `plasma_pick`, `plasma_restore_items`), while CARRYING THEM OUT was
// until now possible only in Python (`PlasmaProtector` in
// `rexon/backup.py`). Because of this both tabs that show panel copies
// ("Settings" and "Desktops") answered the button with "no executor". The
// executor is written next to the decisions, where the transactional
// restore engine already lives, and by the same rules: plain `std::fs`,
// no panics, any trouble is returned in words.
//
// ⚠️ THERE IS DELIBERATELY NO PROCESS LAUNCHING HERE. The shell restart is
// the only step by which a snapshot touches the LIVE desktop, and it is
// moved into [`plasma_restart_shell`] with FAKE launchers: the parity
// check must be able to verify the order "unit -> kill+start" without
// ever touching the person's panel.

/// Name of the panel snapshot metadata file (`PlasmaProtector._meta_file`).
pub const PLASMA_META_NAME: &str = "plasma_meta.json";

/// Path of the snapshot metadata file in the copies directory.
pub fn plasma_meta_path(dir: &str) -> String {
    join(dir, PLASMA_META_NAME)
}

/// Meta file entries IN WRITE ORDER and AS IS (raw JSON).
///
/// ⚠️ They are read raw not out of laziness. [`PlasmaBackup`] has
/// `deny_unknown_fields`, so an entry left by a newer Rexon version will
/// not pass a typed parse. If we wrote the file back from a typed list,
/// such an entry WOULD VANISH together with the reference to the snapshot,
/// and the snapshot would remain on disk as dead weight. So what goes to
/// disk is always what was read from it, plus our new entry.
pub fn plasma_meta_entries(dir: &str) -> Vec<serde_json::Value> {
    let Ok(text) = fs::read_to_string(plasma_meta_path(dir)) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    match value.get("backups") {
        Some(serde_json::Value::Array(items)) => items.clone(),
        _ => Vec::new(),
    }
}

/// Parsing raw entries into typed ones. An entry of an unknown form is
/// skipped: there is nothing to show it with anyway, and the whole list
/// must not fail because of it.
pub fn plasma_parse_entries(raw: &[serde_json::Value]) -> Vec<PlasmaBackup> {
    raw.iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect()
}

/// Write the meta file (`_save_meta`): the same two-space indent and the
/// same literal letters (not `\uXXXX`) as
/// `json.dump(..., indent=2, ensure_ascii=False)`.
fn plasma_write_meta(dir: &str, raw: &[serde_json::Value]) -> io::Result<()> {
    let mut root = serde_json::Map::new();
    root.insert("backups".into(), serde_json::Value::Array(raw.to_vec()));
    let text = serde_json::to_string_pretty(&serde_json::Value::Object(root))
        .map_err(|e| io::Error::other(e.to_string()))?;
    fs::create_dir_all(dir)?;
    // A full replacement rather than appending: half a meta file is worse
    // than none, since the copies list would stop being readable at all.
    // Hence the common door: the panel copies themselves would stay on disk,
    // but there would be nothing to find them with.
    crate::utils::atomic_write(&plasma_meta_path(dir), &text)
}

/// Copy one config into a snapshot: a file as is, a directory as a tree.
pub(crate) fn copy_config_into(src: &str, dst: &str) -> io::Result<()> {
    let src_p = Path::new(src);
    if src_p.is_dir() {
        copy_tree(src_p, Path::new(dst))
    } else {
        if let Some(parent) = Path::new(dst).parent() {
            fs::create_dir_all(parent)?;
        }
        copy_file_like(src_p, Path::new(dst))
    }
}

/// Take a panel copy (`backup_panel`) -> `(did it work, what to say)`.
///
/// `Err` is what was an EXCEPTION in Python (the directory was not
/// created, the meta file was not written): the tab shows such trouble as
/// a separate text ("Could not save the copy: …"), not as the general
/// result.
///
/// `timestamp` comes from outside: the clock is an observation, not a
/// decision, and by substituting the timestamp the parity check gets a
/// reproducible snapshot.
pub fn plasma_backup_run(
    dir: &str,
    configs: &[String],
    reason: &str,
    full: bool,
    timestamp: &str,
) -> Result<(bool, String), String> {
    let snap_dir = join(dir, timestamp);
    fs::create_dir_all(&snap_dir).map_err(|e| e.to_string())?;

    let mut saved: Vec<String> = Vec::new();
    for (src, dst) in plasma_snapshot_items(configs, &snap_dir, |p| Path::new(p).exists()) {
        // One config failing to copy is no reason to drop the others: the
        // original has a `try/except` around EACH file here.
        if copy_config_into(&src, &dst).is_ok() {
            saved.push(src);
        }
    }

    if saved.is_empty() {
        // An empty snapshot directory on disk is garbage that later looks like a
        // broken copy.
        let _ = fs::remove_dir_all(&snap_dir);
        return Ok((false, msg_nothing_to_backup()));
    }

    let entry = plasma_entry(timestamp, reason, &snap_dir, &saved, full);
    let mut raw = plasma_meta_entries(dir);
    raw.push(serde_json::to_value(&entry).map_err(|e| e.to_string())?);

    // Rotation: which snapshots to drop is decided by the core above; here
    // only `rmtree` and striking their entries from the raw list BY snapshot
    // PATH.
    let mut parsed = plasma_parse_entries(&raw);
    let dropped = plasma_rotate(&mut parsed);
    for path in &dropped {
        let _ = fs::remove_dir_all(path);
    }
    if !dropped.is_empty() {
        raw.retain(|v| {
            let path = v.get("path").and_then(|p| p.as_str()).unwrap_or_default();
            !dropped.iter().any(|d| d == path)
        });
    }

    plasma_write_meta(dir, &raw).map_err(|e| e.to_string())?;
    Ok((true, msg_backup_created(saved.len())))
}

/// Restore a panel copy (`restore_panel`) -> `(did it work, what to say)`.
///
/// The shell restart is NOT included: the decision "restart or not"
/// depends on whether Plasma is running right now, and is made where the
/// session environment is visible ([`is_plasma_desktop`]).
pub fn plasma_restore_run(dir: &str, timestamp: Option<&str>) -> (bool, String) {
    let raw = plasma_meta_entries(dir);
    let backups = plasma_parse_entries(&raw);
    let entry = match plasma_pick(&backups, timestamp) {
        Ok(entry) => entry.clone(),
        Err(message) => return (false, message),
    };

    if !Path::new(&entry.path).is_dir() {
        return (false, msg_snapshot_dir_missing(&entry.path));
    }

    let items = plasma_restore_items(&entry, |p| Path::new(p).exists());
    let outcome = transactional_restore(&items);
    if !outcome.ok {
        return (false, msg_restore_failed(&outcome.message));
    }
    if outcome.restored == 0 {
        return (false, msg_none_restored());
    }
    (true, msg_restored_files(outcome.restored, &entry.timestamp))
}

/// How the shell restart ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRestart {
    /// The systemd unit restarted the shell by itself.
    Unit,
    /// `kquitapp` stopped the shell, `kstart` brought it up again.
    QuitAndStart,
    /// The CURRENT session's shell was stopped by its PID, then brought up again.
    SessionPid,
    /// No path worked: there is no unit, `kquitapp` did not help, and no
    /// shell of our own session was found. Other processes were not touched.
    NotRestarted,
}

/// The tail of the panel restore message if the shell could not be restarted.
pub fn msg_plasmashell_not_restarted() -> String {
    format!(
        "\n{}",
        tr("The panel didn't restart on its own — log out and back in to see the restored panel")
    )
}

/// The tail of the panel restore message by the restart result.
pub fn plasma_restart_message(outcome: ShellRestart) -> String {
    match outcome {
        ShellRestart::NotRestarted => msg_plasmashell_not_restarted(),
        _ => msg_plasmashell_restarted(),
    }
}

/// The hands of the restart. Substitutable: checking the order of steps
/// must not touch the person's live panel, so in tests a step log takes
/// their place.
pub trait ShellOps {
    /// Run and wait; `true` means exit code 0.
    fn run(&self, cmd: &Cmd) -> bool;
    /// Start detached from our process and do not wait.
    fn spawn(&self, cmd: &Cmd) -> bool;
    /// Whether the tool exists (taking the `utils::TOOL_SETS` synonyms into account).
    fn has(&self, tool: &str) -> bool;
    /// PIDs of the CURRENT session's shells (see [`is_session_shell`]).
    fn session_shell_pids(&self) -> Vec<u32>;
    /// Ask a process to exit (SIGTERM); `true` means the signal was delivered.
    fn terminate(&self, pid: u32) -> bool;
    /// Pause between stopping and starting.
    fn settle(&self);
}

/// Restart the Plasma shell: the decision and the execution in ONE place
/// for the whole product.
///
/// The order is KDE's regular sequence, from gentle to precise:
///   1. `systemctl --user restart plasma-plasmashell.service`: the unit
///      stops and brings up the shell itself (a second start would give
///      two panels);
///   2. `kquitapp6 plasmashell` + `kstart plasmashell` (`*5` on Plasma 5):
///      only if BOTH exist: stopping without bringing it up is worse than
///      not touching it;
///   3. the PID of the CURRENT session's shell: the same uid,
///      `/proc/PID/exe` is `plasmashell`, the same `XDG_SESSION_ID`; a
///      signal by PID, then a start.
///
/// ⚠️ There is no killing by name (`killall plasmashell`) here, and there
/// must not be: it hits the shell of any session of this person. If our
/// own shell was not found, [`ShellRestart::NotRestarted`], and nothing is
/// touched.
pub fn plasma_restart_shell_with(ops: &impl ShellOps) -> ShellRestart {
    if ops.run(&plasma_quit_cmd()) {
        return ShellRestart::Unit;
    }
    let can_start = ops.has("kstart");
    if can_start && ops.has("kquitapp6") && ops.run(&plasma_kill_cmd()) {
        ops.settle();
        ops.spawn(&plasma_start_cmd());
        return ShellRestart::QuitAndStart;
    }
    let mut stopped = false;
    for pid in ops.session_shell_pids() {
        stopped |= ops.terminate(pid);
    }
    if !stopped {
        return ShellRestart::NotRestarted;
    }
    ops.settle();
    let start = if can_start {
        plasma_start_cmd()
    } else {
        plasma_direct_start_cmd()
    };
    ops.spawn(&start);
    ShellRestart::SessionPid
}

/// Restart the Plasma shell on the live machine (`_restart_plasmashell`).
pub fn plasma_restart_shell() -> ShellRestart {
    plasma_restart_shell_with(&LiveShellOps)
}

/// Whether this is the CURRENT session's shell.
///
/// Three signs at once, and none is superfluous: the uid cuts off other
/// people, `exe` other programs with a similar name, `XDG_SESSION_ID` the
/// shell of another session of the same person (a second login, a remote
/// session). Without our own `XDG_SESSION_ID` we do not guess: we touch
/// nobody.
pub fn is_session_shell(
    exe: &str,
    proc_uid: u32,
    environ: &[u8],
    my_uid: u32,
    my_session: &str,
) -> bool {
    is_session_process(exe, "plasmashell", proc_uid, environ, my_uid, my_session)
}

/// Whether this is a process of program `name` in the CURRENT session: the
/// same three signs as in [`is_session_shell`].
pub fn is_session_process(
    exe: &str,
    name: &str,
    proc_uid: u32,
    environ: &[u8],
    my_uid: u32,
    my_session: &str,
) -> bool {
    if my_session.is_empty() || proc_uid != my_uid {
        return false;
    }
    let exe = exe.strip_suffix(" (deleted)").unwrap_or(exe);
    if Path::new(exe).file_name().and_then(|n| n.to_str()) != Some(name) {
        return false;
    }
    let wanted = format!("XDG_SESSION_ID={my_session}");
    environ.split(|b| *b == 0).any(|kv| kv == wanted.as_bytes())
}

/// The live hands of the restart. `PATH` is taken from the process
/// environment: without it the resolver would look in the fallback
/// directory list rather than where the person (or the test bench) keeps
/// their tools.
struct LiveShellOps;

impl LiveShellOps {
    fn path_env() -> Option<String> {
        std::env::var("PATH").ok()
    }
}

impl ShellOps for LiveShellOps {
    fn run(&self, cmd: &Cmd) -> bool {
        let spec = crate::utils::RunSpec {
            argv: cmd.argv.clone(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: cmd.timeout_secs,
            lang_c: cmd.lang_c,
            ..Default::default()
        };
        crate::utils::run(&spec, Self::path_env().as_deref()).returncode == Some(0)
    }

    fn spawn(&self, cmd: &Cmd) -> bool {
        use std::os::unix::process::CommandExt;
        use std::process::{Command, Stdio};

        let argv = crate::utils::resolved_argv(&cmd.argv, Self::path_env().as_deref());
        let Some((head, rest)) = argv.split_first() else {
            return false;
        };
        let mut command = Command::new(head);
        command
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Its own session: the panel must not die together with Rexon.
        // SAFETY: between fork and exec only setsid is called, and it is
        // async-signal-safe.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        match command.spawn() {
            Ok(mut child) => {
                // Collect the exit status so as not to leave a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                true
            }
            Err(_) => false,
        }
    }

    fn has(&self, tool: &str) -> bool {
        crate::utils::resolve_tool(tool, &[], Self::path_env().as_deref()).is_some()
    }

    fn session_shell_pids(&self) -> Vec<u32> {
        use std::os::unix::fs::MetadataExt;

        let my_session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        if my_session.is_empty() {
            return Vec::new();
        }
        // SAFETY: getuid has no preconditions and cannot fail.
        let my_uid = unsafe { libc::getuid() };
        let me = std::process::id();
        let Ok(entries) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter(|pid| *pid != me)
            .filter(|pid| {
                let base = format!("/proc/{pid}");
                let (Ok(meta), Ok(exe), Ok(environ)) = (
                    fs::metadata(&base),
                    fs::read_link(format!("{base}/exe")),
                    fs::read(format!("{base}/environ")),
                ) else {
                    return false;
                };
                is_session_shell(&exe.to_string_lossy(), meta.uid(), &environ, my_uid, &my_session)
            })
            .collect()
    }

    fn terminate(&self, pid: u32) -> bool {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: kill with a specific positive PID and SIGTERM is an ordinary
        // system call with no memory requirements.
        unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
    }

    fn settle(&self) {
        std::thread::sleep(std::time::Duration::from_secs(PLASMASHELL_SETTLE_SECS));
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  Executor parity checks
// ══════════════════════════════════════════════════════════════════════════
//
// ⚠️ THERE ARE NO LIVE PANELS HERE AND THERE CANNOT BE. Everything below
// works in a fake directory in a temporary folder: its own "configs", its
// own copies directory, its own meta file. No path leads to `~/.config`,
// and the shell restart is checked with fake launchers that launch
// nothing.
// ============================================================================
// The original panel: a snapshot before Rexon's first intervention
// ============================================================================
//
// Wehrwolfmann's decision of 20.10.2026: "Restore the previous panel"
// returns the panel as the person had it BEFORE Rexon's first
// intervention, not Rexon's previous layout. Previously every layout
// application overwrote the single copy: after "Like Windows" ->
// "Classic", restoring gave "Like Windows".
//
// So the panel copy (one per desktop, the path is set by the desktop
// itself) is the original one: the first intervention takes it, later
// ones do not touch it and put their own snapshot next to it
// (`<copy>.last`, for a future "step back"; there is no button). Once the
// original is restored, a `<copy>.returned` mark is placed next to it,
// and the next intervention takes the original again: the panel after
// the restore is the person's panel. "Save panel" is the person's own
// choice: the copy is replaced (after a question) and the mark is
// removed. A copy without a mark, taken by earlier releases, counts as
// the original.

/// Contents of the "the original panel has already been restored" mark.
pub const PANEL_ORIGINAL_RETURNED: &str = "returned";

/// The "original panel restored" mark next to copy `copy`.
pub fn panel_original_returned_mark(copy: &str) -> String {
    format!("{copy}.returned")
}

/// The last snapshot before a Rexon intervention when the original copy
/// already exists (there is no "step back" button yet; the file is for it).
pub fn panel_last_snapshot_path(copy: &str) -> String {
    format!("{copy}.last")
}

/// Where to put the panel snapshot before a Rexon intervention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelSnapshotSlot {
    /// The original copy: it does not exist yet or it has already been restored.
    Original,
    /// Next to it ([`panel_last_snapshot_path`]): the original copy is in place.
    Last,
}

/// The decision by copy `copy_exists` and the mark text `returned_mark`
/// ([`panel_original_returned_mark`], `None` means no file).
pub fn panel_snapshot_slot(copy_exists: bool, returned_mark: Option<&str>) -> PanelSnapshotSlot {
    let returned = returned_mark.is_some_and(|m| m.trim() == PANEL_ORIGINAL_RETURNED);
    if copy_exists && !returned {
        PanelSnapshotSlot::Last
    } else {
        PanelSnapshotSlot::Original
    }
}

// ============================================================================
// XFCE panel copy
// ============================================================================
//
// It is taken and restored by `xfce4-panel-profiles` (`save <file>` /
// `load <file>`, the program's CLI): an archive of the `xfce4-panel`
// xfconf channel and the files in `~/.config/xfce4/panel`; on load it
// restarts the panel itself. There is one copy, for "Save panel" and
// "Restore panel" in the "Desktops" tab.

/// The XFCE panel copy program.
pub const XFCE_PANEL_PROFILES: &str = "xfce4-panel-profiles";

/// The XFCE panel copy file (`data_home` is `$XDG_DATA_HOME` or
/// `~/.local/share`).
pub fn xfce_panel_copy_path(data_home: &str) -> String {
    format!("{}/rexon/backups/xfce4-panel.tar.bz2", data_home.trim_end_matches('/'))
}

/// The command that takes a copy.
pub fn xfce_panel_save_argv(path: &str) -> Vec<String> {
    vec![XFCE_PANEL_PROFILES.to_string(), "save".to_string(), path.to_string()]
}

/// The command that restores from a copy.
pub fn xfce_panel_load_argv(path: &str) -> Vec<String> {
    vec![XFCE_PANEL_PROFILES.to_string(), "load".to_string(), path.to_string()]
}

// ============================================================================
// Panel copy of other environments (Cinnamon, Budgie, LXQt, GNOME with panel
// extensions, Pantheon, COSMIC, tiling compositor bars)
// ============================================================================
//
// One copy per environment: `~/.local/share/rexon/backups/panel-<desktop>/`.
// What to take differs per desktop ([`panel_copy_spec`]): dconf branches
// (whole, or root keys by prefix), files and directories relative to
// HOME. The copy contains `manifest.json`, what exactly was taken, and
// "Restore" returns exactly that. A new copy is assembled next to it
// (`.new`) and takes its place only as a whole: a failed snapshot does
// not spoil the previous copy. How the panel picks up what was returned
// is also up to the desktop ([`PanelReload`]): most watch their own
// settings by themselves.

/// What a panel copy contains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelPart {
    /// A whole dconf branch (on restore the branch is reset first).
    DconfDir(&'static str),
    /// Root keys of a dconf branch whose names start with one of the prefixes
    /// (on restore they are written over, the other keys of the branch are
    /// not touched).
    DconfKeys { dir: &'static str, prefixes: &'static [&'static str] },
    /// A file or directory relative to HOME (`.config/lxqt/panel.conf`).
    Home(String),
    /// A file or directory relative to the config home (`$XDG_CONFIG_HOME`
    /// or `~/.config`): with a non-standard config root the desktop reads
    /// its files there, not in `~/.config`.
    Config(&'static str),
    /// A directory relative to the data home (`$XDG_DATA_HOME` or
    /// `~/.local/share`) without its `exclude` subpaths: they are not taken,
    /// and on restore the person's current ones stay as they are (Rexon's own
    /// wallpaper plugin and shell extension are managed apart).
    DataTree { path: &'static str, exclude: &'static [&'static str] },
    /// The MATE panel branch `/org/mate/panel/`, put back with the panel's
    /// own procedure ([`mate_panel_load`]).
    MatePanel,
    /// An xfconf channel, from its XML file; put back through `xfconf-query`
    /// ([`xfconf_restore_argvs`]).
    XfconfChannel(&'static str),
    /// Deepin shortcuts and touchpad gestures of dde-services (DConfig
    /// subpaths of `org.deepin.dde.keybinding`) and the person's own shortcuts.
    DdeShortcuts,
    /// Plasma global shortcuts (`kglobalshortcutsrc`). KWin keeps them in
    /// memory, so in a Plasma session the changed ones are also handed to it
    /// over D-Bus ([`kde_shortcuts_restore`]).
    KdeShortcuts,
    /// Directories in `parent` (relative to HOME) whose names start with `prefix`.
    HomeMatching { parent: &'static str, prefix: &'static str },
    /// A whole Deepin DConfig resource: all writable keys of its schema
    /// (`/usr/share/dsg/configs/<appid>/<resource>.json`) through
    /// `dde-dconfig`. A key with the default value is reset on restore rather
    /// than written.
    DdeDconfig { appid: &'static str, resource: &'static str },
    /// An inventory of GNOME Shell extensions ([`SavedGnomeExt`]): uuid,
    /// whether enabled, user or system. The settings are restored by the dconf
    /// branches next to it, and missing extensions are delivered separately
    /// ([`gnome_ext_copy_deliver`]): with the network and the shell's window
    /// this takes long.
    GnomeExtensions,
}

/// How the panel picks up the restored settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelReload {
    /// By itself: it watches its own settings (dconf, files).
    Live,
    /// Start a program in a separate session (`budgie-panel --replace`).
    Spawn(&'static [&'static str]),
    /// An LXQt session module: stop and start it through `org.lxqt.session`.
    LxqtModule(&'static str),
    /// Send SIGUSR2 to our session's processes with this name (waybar rereads
    /// its settings on it).
    ReloadSignal(&'static str),
    /// Restart a systemd user service if it is running
    /// (`systemctl --user --no-block try-restart`).
    RestartUserUnit(&'static str),
    /// Run a command and wait for it (`xfce4-panel -r`, `swaymsg reload`).
    Run(&'static [&'static str]),
    /// Restart the Plasma shell of this session ([`plasma_restart_shell`]).
    PlasmaShell,
}

/// What to take from a desktop and how to reread it afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelCopySpec {
    /// Panels, applets, docks, panel extensions.
    pub parts: Vec<PanelPart>,
    /// Keyboard shortcuts: kept apart for the copy's caption and for the
    /// question before restoring.
    pub shortcuts: Vec<PanelPart>,
    pub reload: Vec<PanelReload>,
    /// The restored keyboard shortcuts take effect only after logging in
    /// again: the desktop applies them once, at session start.
    pub needs_relogin: bool,
    /// The restore restarts the desktop's services for a moment (Deepin's
    /// service group: theme, wallpaper slideshow and power blink): the
    /// question before restoring says so.
    pub restarts_services: bool,
}

impl PanelCopySpec {
    /// Every part of the copy: the panel ones, then the shortcuts.
    pub fn all_parts(&self) -> impl Iterator<Item = &PanelPart> {
        self.parts.iter().chain(self.shortcuts.iter())
    }
}

/// What the machine says about the desktop, for choosing the parts of the copy.
pub struct PanelProbe<'a> {
    /// Whether the program is in the system.
    pub have: &'a dyn Fn(&str) -> bool,
    /// Whether a path relative to HOME exists.
    pub home_exists: &'a dyn Fn(&str) -> bool,
    /// Enabled GNOME Shell extensions (`enabled-extensions`).
    pub gnome_extensions: &'a [String],
    /// Whether a file exists at an absolute path (Deepin DConfig schemas).
    pub system_exists: &'a dyn Fn(&str) -> bool,
}

/// The DConfig application under which dde-shell (the Deepin V23/V25 dock)
/// keeps its settings since release 0.0.35 (August 2024, "correct app id
/// used by DConfig"); the former `org.deepin.ds.dock` remains only as a
/// copy of the schemas for compatibility.
pub const DDE_SHELL_DCONFIG_APPID: &str = "org.deepin.dde.shell";

/// The dde-shell dock resources: the dock itself (size, position, hiding,
/// plugins), the notification area and the task bar (pinned programs).
pub const DDE_DOCK_RESOURCES: &[&str] =
    &["org.deepin.ds.dock", "org.deepin.ds.dock.tray", "org.deepin.ds.dock.taskmanager"];

/// The DConfig application of the Deepin tray plugins.
pub const DDE_TRAY_LOADER_APPID: &str = "org.deepin.dde.tray-loader";

/// The tray plugins' own settings resources.
pub const DDE_TRAY_PLUGIN_RESOURCES: &[&str] = &[
    "org.deepin.dde.dock.plugin.common",
    "org.deepin.dde.dock.plugin.datetime",
    "org.deepin.dde.dock.plugin.power",
    "org.deepin.dde.dock.plugin.shutdown",
];

/// The systemd service of the Deepin dock (dde-session): `dde-shell -C DDE`.
pub const DDE_DOCK_UNIT: &str = "dde-shell@DDE.service";

/// The dconf branch where GNOME Shell extensions keep their settings.
pub const GNOME_EXTENSIONS_DCONF_DIR: &str = "/org/gnome/shell/extensions/";

/// GNOME's window manager shortcuts: read by GNOME, Budgie and Pantheon alike.
pub const GNOME_WM_KEYS_DIR: &str = "/org/gnome/desktop/wm/keybindings/";
/// GNOME's media and custom keys: read by GNOME, Budgie and Pantheon alike.
pub const GNOME_MEDIA_KEYS_DIR: &str = "/org/gnome/settings-daemon/plugins/media-keys/";

/// Cinnamon extensions a copy never takes nor puts back: Rexon's own.
pub const CINNAMON_EXT_KEEP: &[&str] = &[crate::installer::EXT_ID, crate::installer::EXT_ID_LEGACY];

/// Other desktops whose keyboard shortcuts change when `de`'s are restored:
/// they read the same GNOME branches. The question before a restore names
/// the installed ones.
pub fn shared_shortcut_desktops(de: &str) -> &'static [&'static str] {
    match de {
        "gnome" => &["budgie", "pantheon"],
        "budgie" => &["gnome", "pantheon"],
        "pantheon" => &["gnome", "budgie"],
        _ => &[],
    }
}

/// Subpaths of `~/.local/share/plasma` a Plasma copy never takes nor puts
/// back: Rexon's own wallpaper plugin is installed and updated apart, and an
/// old copy must not bring back an old plugin.
pub const PLASMA_DATA_KEEP: &[&str] = &["wallpapers/org.waywallen.kde"];

/// What to take from desktop `de` (a Rexon key). `None` means nothing to
/// take: such a desktop has no panel copy buttons.
pub fn panel_copy_spec(de: &str, probe: &PanelProbe) -> Option<PanelCopySpec> {
    match de {
        // KDE Plasma: the panels and widgets are the applets file and the shell's
        // own files; Latte Dock, if it is there, has its own. The person's
        // widgets, desktop themes and looks lie in `~/.local/share/plasma`. The
        // shell does not reread its files: it is restarted.
        "kde" if (probe.have)("plasmashell") => Some(PanelCopySpec {
            parts: vec![
                PanelPart::Config("plasma-org.kde.plasma.desktop-appletsrc"),
                PanelPart::Config("plasmashellrc"),
                PanelPart::Config("plasmarc"),
                PanelPart::Config("plasmanotifyrc"),
                PanelPart::Config("plasma-localerc"),
                PanelPart::Config("lattedockrc"),
                PanelPart::Config("latte"),
                PanelPart::DataTree { path: "plasma", exclude: PLASMA_DATA_KEEP },
            ],
            // Global shortcuts and the person's own command shortcuts (Plasma 6
            // keeps them as `.desktop` files); `khotkeysrc` is Plasma 5's.
            shortcuts: vec![
                PanelPart::KdeShortcuts,
                PanelPart::DataTree { path: "kglobalaccel", exclude: &[] },
                PanelPart::Config("khotkeysrc"),
            ],
            reload: vec![PanelReload::PlasmaShell],
            ..Default::default()
        }),
        // MATE: everything about the panels is the `/org/mate/panel/` branch;
        // the running panel picks the branch up itself, after the procedure
        // measured for the layout section (`mate_panel::restore`).
        "mate" if (probe.have)("mate-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::MatePanel],
            // Marco and the settings daemon watch their keys themselves.
            shortcuts: vec![
                PanelPart::DconfDir("/org/mate/marco/global-keybindings/"),
                PanelPart::DconfDir("/org/mate/marco/window-keybindings/"),
                PanelPart::DconfDir("/org/mate/marco/keybinding-commands/"),
                PanelPart::DconfDir("/org/mate/settings-daemon/plugins/media-keys/"),
                PanelPart::DconfDir("/org/mate/desktop/keybindings/"),
            ],
            reload: vec![PanelReload::Live],
            ..Default::default()
        }),
        // XFCE: the `xfce4-panel` xfconf channel and the panel's own files
        // (launchers, plugin settings) in `~/.config/xfce4/panel`; the panel is
        // restarted to read them (`xfce4-panel -r`). No xfce4-panel-profiles
        // needed: the channel is put back through `xfconf-query`.
        "xfce" if (probe.have)("xfce4-panel") && (probe.have)("xfconf-query") => Some(PanelCopySpec {
            parts: vec![PanelPart::XfconfChannel("xfce4-panel"), PanelPart::Config("xfce4/panel")],
            // xfsettingsd and xfwm4 follow their channel themselves.
            shortcuts: vec![PanelPart::XfconfChannel("xfce4-keyboard-shortcuts")],
            reload: vec![PanelReload::Run(&["xfce4-panel", "-r"])],
            ..Default::default()
        }),
        // Budgie: everything about the panels is the
        // `com.solus-project.budgie-panel` branch (panels, applet `instance/`,
        // their settings). budgie-panel does not rebuild existing panels from
        // dconf; `--replace` brings them up again (that is how Budgie itself is
        // restarted too).
        // LXQt: the panels and their plugins are one file,
        // `~/.config/lxqt/panel.conf`. The panel does not reread its file by
        // itself; the LXQt session stops and starts its module again
        // (`org.lxqt.session`, `stopModule` / `startModule`, the module name is
        // the autostart file).
        // On X11 the shortcuts are `lxqt-globalkeysd`'s file, reread when its
        // session module is restarted too (on Wayland the compositor owns them).
        "lxqt" if (probe.have)("lxqt-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::Home(".config/lxqt/panel.conf".to_string())],
            shortcuts: vec![PanelPart::Config("lxqt/globalkeyshortcuts.conf")],
            reload: vec![
                PanelReload::LxqtModule("lxqt-panel.desktop"),
                PanelReload::LxqtModule("lxqt-globalkeyshortcuts.desktop"),
            ],
            ..Default::default()
        }),
        // GNOME: it has no panel of its own; the panel, the dock and the icons in
        // it come from extensions. So the copy is all of the person's extensions:
        // their inventory (uuid, whether enabled, user or system), the settings
        // of all extensions (the `/org/gnome/shell/extensions/` branch) and both
        // shell lists (`enabled-extensions`, `disabled-extensions`). Extensions
        // watch their own settings; missing ones are installed from
        // extensions.gnome.org on restore ([`gnome_ext_copy_deliver`]).
        "gnome" => {
            if !(probe.have)("gnome-shell") && probe.gnome_extensions.is_empty() {
                return None;
            }
            Some(PanelCopySpec {
                parts: vec![
                    PanelPart::DconfDir(GNOME_EXTENSIONS_DCONF_DIR),
                    PanelPart::DconfKeys {
                        dir: "/org/gnome/shell/",
                        prefixes: &["enabled-extensions", "disabled-extensions", "favorite-apps"],
                    },
                    PanelPart::GnomeExtensions,
                ],
                // ⚠️ The window manager and media key branches are read by Budgie
                // and Pantheon too ([`shared_shortcut_desktops`]).
                shortcuts: vec![
                    PanelPart::DconfDir(GNOME_WM_KEYS_DIR),
                    PanelPart::DconfDir("/org/gnome/shell/keybindings/"),
                    PanelPart::DconfDir("/org/gnome/mutter/keybindings/"),
                    PanelPart::DconfDir("/org/gnome/mutter/wayland/keybindings/"),
                    PanelPart::DconfDir(GNOME_MEDIA_KEYS_DIR),
                ],
                reload: vec![PanelReload::Live],
                ..Default::default()
            })
        }
        // Pantheon: the Wingpanel top panel (`io.elementary.desktop.wingpanel`)
        // and the installed dock: Plank (`net.launchpad.plank`, the dock icons
        // are files in `~/.config/plank`) or the new elementary dock
        // (`io.elementary.dock`). All of them watch their own settings.
        "pantheon" if (probe.have)("io.elementary.wingpanel") => {
            let mut parts = vec![PanelPart::DconfDir("/io/elementary/desktop/wingpanel/")];
            if (probe.have)("plank") {
                parts.push(PanelPart::DconfDir("/net/launchpad/plank/"));
                parts.push(PanelPart::Home(".config/plank".to_string()));
            }
            if (probe.have)("io.elementary.dock") {
                parts.push(PanelPart::DconfDir("/io/elementary/dock/"));
            }
            let shortcuts = vec![
                PanelPart::DconfDir(GNOME_WM_KEYS_DIR),
                PanelPart::DconfDir("/io/elementary/desktop/wm/keybindings/"),
                PanelPart::DconfDir(GNOME_MEDIA_KEYS_DIR),
            ];
            Some(PanelCopySpec { parts, shortcuts, reload: vec![PanelReload::Live], ..Default::default() })
        }
        // Wayfire: the wf-panel panel and the wf-background background (wf-shell)
        // are one file, `~/.config/wf-shell.ini`; wf-shell watches it itself
        // (inotify) and rereads it on write.
        "wayfire" if (probe.have)("wf-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::Home(".config/wf-shell.ini".to_string())],
            // Key bindings live in the compositor's own file, watched the same way.
            shortcuts: vec![PanelPart::Home(".config/wayfire.ini".to_string())],
            reload: vec![PanelReload::Live],
            ..Default::default()
        }),
        // COSMIC: the panel, the dock and their buttons are the settings
        // directories `~/.config/cosmic/com.system76.CosmicPanel*`
        // (cosmic-config); the panel watches them itself.
        "cosmic" if (probe.have)("cosmic-panel") => Some(PanelCopySpec {
            // The pinned programs of the dock are the app list applet's own settings.
            parts: vec![
                PanelPart::HomeMatching { parent: ".config/cosmic", prefix: "com.system76.CosmicPanel" },
                PanelPart::Home(".config/cosmic/com.system76.CosmicAppList".to_string()),
            ],
            shortcuts: vec![PanelPart::Home(".config/cosmic/com.system76.CosmicSettings.Shortcuts".to_string())],
            reload: vec![PanelReload::Live],
            ..Default::default()
        }),
        // Tiling compositors: the panel is a bar. Configs of whatever is
        // installed: waybar (`~/.config/waybar`, rereads it on SIGUSR2), the
        // Noctalia (`~/.config/noctalia`) and DankMaterialShell
        // (`~/.config/DankMaterialShell`) shells, which watch their own settings.
        // River also uses waybar (brought up by Rexon's startup init).
        "hyprland" | "niri" | "sway" | "river" => {
            let mut parts = Vec::new();
            let mut reload = Vec::new();
            if (probe.have)("waybar") && (probe.home_exists)(".config/waybar") {
                parts.push(PanelPart::Home(".config/waybar".to_string()));
                reload.push(PanelReload::ReloadSignal("waybar"));
            }
            for dir in [".config/noctalia", ".config/DankMaterialShell"] {
                if (probe.home_exists)(dir) {
                    parts.push(PanelPart::Home(dir.to_string()));
                }
            }
            // Key bindings are the compositor's own config (with its included
            // files). Hyprland and niri reread it themselves, sway on `reload`;
            // river runs its init once per session, so its bindings come back at
            // the next login.
            let mut shortcuts = Vec::new();
            let config = match de {
                "hyprland" => ".config/hypr",
                "niri" => ".config/niri",
                "sway" => ".config/sway",
                _ => ".config/river",
            };
            if (probe.home_exists)(config) {
                shortcuts.push(PanelPart::Home(config.to_string()));
                if de == "sway" {
                    reload.push(PanelReload::Run(&["swaymsg", "reload"]));
                }
            }
            if parts.is_empty() && shortcuts.is_empty() {
                return None;
            }
            if reload.is_empty() {
                reload.push(PanelReload::Live);
            }
            let needs_relogin = de == "river" && !shortcuts.is_empty();
            Some(PanelCopySpec { parts, shortcuts, reload, needs_relogin, ..Default::default() })
        }
        // Deepin V23/V25: the dock is dde-shell, its settings are three DConfig
        // resources of the `org.deepin.dde.shell` application. The dock watches
        // its keys only partly (the notification area picks up only hidden
        // icons), so after a restore Deepin restarts the dock service itself.
        // Another release (the former dde-dock of deepin 20, dde-shell before
        // 0.0.35 with its own DConfig application) gets no buttons: we do not
        // know its keys.
        "deepin"
            if (probe.have)("dde-dconfig")
                && (probe.have)("dde-shell")
                && DDE_DOCK_RESOURCES.iter().all(|r| {
                    (probe.system_exists)(&crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r))
                }) =>
        {
            let mut parts: Vec<PanelPart> = DDE_DOCK_RESOURCES
                .iter()
                .map(|r| PanelPart::DdeDconfig { appid: DDE_SHELL_DCONFIG_APPID, resource: r })
                .collect();
            // The tray plugins' own settings (seconds in the clock, the charge
            // limit, the shutdown menu), where this release has them.
            for r in DDE_TRAY_PLUGIN_RESOURCES {
                if (probe.system_exists)(&crate::dde::dconfig_schema_path(DDE_TRAY_LOADER_APPID, r)) {
                    parts.push(PanelPart::DdeDconfig { appid: DDE_TRAY_LOADER_APPID, resource: r });
                }
            }
            let mut reload = vec![PanelReload::RestartUserUnit(DDE_DOCK_UNIT)];
            // Shortcuts and gestures: only dde-services' scheme (Deepin 25, Arch
            // since July 2026). After the values, the service group is restarted
            // so the whole set is bound anew (two swapped shortcuts would
            // otherwise leave one unbound): that blinks the theme, the wallpaper
            // slideshow and power for a second, and the question says so.
            let mut shortcuts = Vec::new();
            if (probe.system_exists)(crate::dde::SHORTCUT_SERVICE_MARK) {
                shortcuts.push(PanelPart::DdeShortcuts);
                reload.push(PanelReload::RestartUserUnit(crate::dde::SERVICE_GROUP_UNIT));
            }
            let restarts_services = !shortcuts.is_empty();
            Some(PanelCopySpec {
                parts,
                shortcuts,
                restarts_services,
                reload,
                ..Default::default()
            })
        }
        // Budgie's shortcuts are partly GNOME's branches (shared), partly its
        // own; on labwc its bridge rewrites `rc.xml` from them by itself.
        "budgie" if (probe.have)("budgie-panel") => Some(PanelCopySpec {
            parts: vec![PanelPart::DconfDir("/com/solus-project/budgie-panel/")],
            shortcuts: vec![
                PanelPart::DconfDir(GNOME_WM_KEYS_DIR),
                PanelPart::DconfDir("/org/gnome/mutter/keybindings/"),
                PanelPart::DconfDir(GNOME_MEDIA_KEYS_DIR),
                PanelPart::DconfDir("/org/buddiesofbudgie/settings-daemon/plugins/media-keys/"),
                PanelPart::DconfDir("/com/solus-project/budgie-wm/"),
                PanelPart::Config("budgie-desktop/labwc/rc.xml"),
            ],
            reload: vec![PanelReload::Spawn(&["budgie-panel", "--replace"])],
            ..Default::default()
        }),
        // Cinnamon panels and applets are `org.cinnamon` keys (`panels-*`,
        // `panel-zone-*`, `enabled-applets`, `next-applet-id`), applet settings
        // are files in `~/.config/cinnamon/spices/`. Cinnamon watches both
        // itself (changing `enabled-applets` rebuilds the panels).
        // Downloaded applets, desklets and extensions are taken too: without
        // them `enabled-applets` would name an applet that is not on disk.
        // Rexon's own wallpaper extension is managed apart.
        "cinnamon" | "cinnamon-mint" => Some(PanelCopySpec {
            parts: vec![
                PanelPart::DconfKeys {
                    dir: "/org/cinnamon/",
                    prefixes: &[
                        "panels-",
                        "panel-zone-",
                        "enabled-applets",
                        "next-applet-id",
                        "enabled-desklets",
                        "next-desklet-id",
                        "enabled-extensions",
                    ],
                },
                PanelPart::Home(".config/cinnamon/spices".to_string()),
                PanelPart::DataTree { path: "cinnamon/applets", exclude: &[] },
                PanelPart::DataTree { path: "cinnamon/desklets", exclude: &[] },
                PanelPart::DataTree { path: "cinnamon/extensions", exclude: CINNAMON_EXT_KEEP },
            ],
            shortcuts: vec![PanelPart::DconfDir("/org/cinnamon/desktop/keybindings/")],
            reload: vec![PanelReload::Live],
            ..Default::default()
        }),
        _ => None,
    }
}

/// The panel copy directory of desktop `de`.
pub fn panel_copy_dir(data_home: &str, de: &str) -> String {
    format!("{}/rexon/backups/panel-{de}", data_home.trim_end_matches('/'))
}

/// The hands of the panel copy: commands, files, processes. The live ones are [`LivePanelIo`].
pub trait PanelIo {
    fn run(&self, argv: &[String]) -> (bool, String);
    fn read(&self, path: &str) -> Option<String>;
    fn write(&self, path: &str, text: &str) -> bool;
    fn exists(&self, path: &str) -> bool;
    fn list_dir(&self, path: &str) -> Vec<String>;
    fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String>;
    fn remove_tree(&self, path: &str) -> Result<(), String>;
    fn rename(&self, from: &str, to: &str) -> Result<(), String>;
    fn spawn(&self, argv: &[String]) -> bool;
    /// Send SIGUSR2 to our session's processes named `exe`; returns how many.
    fn reload_signal(&self, exe: &str) -> usize;
    fn pause(&self);
    /// A snapshot of extensions from GNOME Shell; `None` means there is no
    /// shell (then the inventory comes from disk and keys,
    /// [`gnome_ext_inventory`]).
    fn gnome_shell_list(&self) -> Option<Vec<crate::gnome_shell_ext::ExtInfo>> {
        None
    }
    /// The user data directory (`$XDG_DATA_HOME` or `~/.local/share`).
    fn data_home(&self, home: &str) -> String {
        format!("{}/.local/share", home.trim_end_matches('/'))
    }
    /// The user config directory (`$XDG_CONFIG_HOME` or `~/.config`).
    fn config_home(&self, home: &str) -> String {
        format!("{}/.config", home.trim_end_matches('/'))
    }
    /// Restart the Plasma shell -> the line to tell the person.
    fn restart_plasma_shell(&self) -> Option<String> {
        None
    }
    /// When the file was last changed, as `YYYYMMDD_HHMMSS` local time.
    fn mtime_stamp(&self, _path: &str) -> Option<String> {
        None
    }
}

fn sv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

/// Whether a copy/manifest path is acceptable: relative, without `..`.
fn safe_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// Root keys of a dconf dump (the `[/]` group) whose names start with one
/// of `prefixes`, as a ready dump for `dconf load`.
pub fn dconf_root_keys(dump: &str, prefixes: &[&str]) -> String {
    let mut out = String::new();
    let mut in_root = false;
    for line in dump.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_root = t == "[/]";
            continue;
        }
        if !in_root {
            continue;
        }
        let key = t.split('=').next().unwrap_or("").trim();
        if prefixes.iter().any(|p| key.starts_with(p)) {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        out
    } else {
        format!("[/]\n{out}")
    }
}

/// Take a panel copy by `spec` into `copy_dir` -> the copy path or the reason.
pub fn panel_copy_save(io: &dyn PanelIo, spec: &PanelCopySpec, home: &str, copy_dir: &str) -> Result<String, String> {
    let home = home.trim_end_matches('/');
    let config = io.config_home(home);
    let data = io.data_home(home);
    let tmp = format!("{copy_dir}.new");
    io.remove_tree(&tmp)?;
    let mut manifest: Vec<serde_json::Value> = Vec::new();
    for (i, part) in spec.all_parts().enumerate() {
        match part {
            PanelPart::DconfDir(dir) | PanelPart::DconfKeys { dir, .. } => {
                let (ok, dump) = io.run(&sv(&["dconf", "dump", dir]));
                if !ok {
                    return Err(dump);
                }
                let (kind, text) = match part {
                    PanelPart::DconfKeys { prefixes, .. } => ("dconf_keys", dconf_root_keys(&dump, prefixes)),
                    _ => ("dconf_dir", dump),
                };
                let file = format!("dconf-{i}.ini");
                if !io.write(&format!("{tmp}/{file}"), &text) {
                    return Err(format!("{tmp}/{file}"));
                }
                manifest.push(serde_json::json!({ "kind": kind, "dir": dir, "file": file }));
            }
            PanelPart::MatePanel => {
                let (ok, dump) = io.run(&sv(&["dconf", "dump", MATE_PANEL_DCONF_DIR]));
                if !ok {
                    return Err(dump);
                }
                let file = format!("dconf-{i}.ini");
                if !io.write(&format!("{tmp}/{file}"), &dump) {
                    return Err(format!("{tmp}/{file}"));
                }
                manifest.push(serde_json::json!({ "kind": "mate_panel", "file": file }));
            }
            PanelPart::Home(rel) => {
                if io.exists(&format!("{home}/{rel}")) {
                    io.copy_tree(&format!("{home}/{rel}"), &format!("{tmp}/home/{rel}"))?;
                    manifest.push(serde_json::json!({ "kind": "home", "path": rel }));
                }
            }
            PanelPart::Config(rel) => {
                if io.exists(&format!("{config}/{rel}")) {
                    io.copy_tree(&format!("{config}/{rel}"), &format!("{tmp}/config/{rel}"))?;
                    manifest.push(serde_json::json!({ "kind": "config", "path": rel }));
                }
            }
            PanelPart::DataTree { path, exclude } => {
                if io.exists(&format!("{data}/{path}")) {
                    let dest = format!("{tmp}/data/{path}");
                    io.copy_tree(&format!("{data}/{path}"), &dest)?;
                    for ex in exclude.iter() {
                        io.remove_tree(&format!("{dest}/{ex}"))?;
                    }
                    manifest.push(serde_json::json!({ "kind": "data", "path": path }));
                }
            }
            PanelPart::XfconfChannel(channel) => {
                // No file means the channel is all defaults: there is nothing to
                // keep, and resetting a live panel to nothing would make it ask
                // the person how to set itself up.
                if let Some(xml) = io.read(&xfconf_channel_path(&config, channel)) {
                    let file = format!("xfconf-{i}.xml");
                    if !io.write(&format!("{tmp}/{file}"), &xml) {
                        return Err(format!("{tmp}/{file}"));
                    }
                    manifest.push(serde_json::json!({ "kind": "xfconf", "channel": channel, "file": file }));
                }
            }
            PanelPart::KdeShortcuts => {
                if let Some(text) = io.read(&format!("{config}/{KDE_SHORTCUTS_FILE}")) {
                    let file = format!("kde-shortcuts-{i}.ini");
                    if !io.write(&format!("{tmp}/{file}"), &text) {
                        return Err(format!("{tmp}/{file}"));
                    }
                    manifest.push(serde_json::json!({ "kind": "kde_shortcuts", "file": file }));
                }
            }
            PanelPart::DdeShortcuts => {
                let body = dde_shortcuts_save(io, &config)?;
                if !io.write(&format!("{tmp}/{DDE_SHORTCUTS_COPY_FILE}"), &body.to_string()) {
                    return Err(format!("{tmp}/{DDE_SHORTCUTS_COPY_FILE}"));
                }
                manifest.push(serde_json::json!({ "kind": "dde_shortcuts", "file": DDE_SHORTCUTS_COPY_FILE }));
            }
            PanelPart::HomeMatching { parent, prefix } => {
                let names: Vec<String> =
                    io.list_dir(&format!("{home}/{parent}")).into_iter().filter(|n| n.starts_with(prefix)).collect();
                for n in &names {
                    io.copy_tree(&format!("{home}/{parent}/{n}"), &format!("{tmp}/home/{parent}/{n}"))?;
                }
                manifest.push(serde_json::json!({ "kind": "home_matching", "parent": parent, "prefix": prefix, "names": names }));
            }
            PanelPart::DdeDconfig { appid, resource } => {
                let schema_path = crate::dde::dconfig_schema_path(appid, resource);
                let Some(schema) = io.read(&schema_path) else {
                    return Err(schema_path);
                };
                let mut values = serde_json::Map::new();
                for key in crate::dde::dconfig_schema_writable_keys(&schema) {
                    let (ok, raw) = io.run(&crate::dde::dconfig_get_key_argv(appid, resource, &key));
                    if !ok {
                        return Err(raw);
                    }
                    let (_, default) = io.run(&crate::dde::dconfig_is_default_argv(appid, resource, &key));
                    values.insert(
                        key,
                        serde_json::json!({
                            "value": crate::dde::dconfig_cli_value(&raw),
                            "default": default.trim() == "true",
                        }),
                    );
                }
                let file = format!("dde-{i}.json");
                let body = serde_json::json!({ "appid": appid, "resource": resource, "values": values });
                if !io.write(&format!("{tmp}/{file}"), &body.to_string()) {
                    return Err(format!("{tmp}/{file}"));
                }
                manifest.push(serde_json::json!({ "kind": "dde_dconfig", "appid": appid, "resource": resource, "file": file }));
            }
            PanelPart::GnomeExtensions => {
                let list = gnome_ext_inventory(io, &data);
                let body = serde_json::json!({ "extensions": list });
                if !io.write(&format!("{tmp}/{GNOME_EXT_COPY_FILE}"), &body.to_string()) {
                    return Err(format!("{tmp}/{GNOME_EXT_COPY_FILE}"));
                }
                manifest.push(serde_json::json!({ "kind": "gnome_extensions", "file": GNOME_EXT_COPY_FILE }));
            }
        }
    }
    if !io.write(&format!("{tmp}/manifest.json"), &serde_json::Value::Array(manifest).to_string()) {
        return Err(format!("{tmp}/manifest.json"));
    }
    io.remove_tree(copy_dir)?;
    io.rename(&tmp, copy_dir)?;
    Ok(copy_dir.to_string())
}

/// Restore the panel from copy `copy_dir` and let the desktop reread it.
pub fn panel_copy_restore(io: &dyn PanelIo, spec: &PanelCopySpec, home: &str, copy_dir: &str) -> Result<String, String> {
    panel_copy_restore_in(io, spec, home, copy_dir, true).map(|_| copy_dir.to_string())
}

/// Restore the panel from copy `copy_dir` -> what else to tell the person
/// (lines, may be empty) or the reason it failed.
///
/// `live`: the copy's desktop is the running session. Otherwise there is
/// nobody to reread anything: the files and keys are put back, and the
/// desktop picks them up at its next login.
pub fn panel_copy_restore_in(
    io: &dyn PanelIo,
    spec: &PanelCopySpec,
    home: &str,
    copy_dir: &str,
    live: bool,
) -> Result<Vec<String>, String> {
    let home = home.trim_end_matches('/');
    let config = io.config_home(home);
    let data = io.data_home(home);
    let Some(text) = io.read(&format!("{copy_dir}/manifest.json")) else {
        return Err(tr("There is no saved panel copy yet."));
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let s = |v: &serde_json::Value, k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let mut notes: Vec<String> = Vec::new();
    for e in &entries {
        match s(e, "kind").as_str() {
            kind @ ("dconf_dir" | "dconf_keys") => {
                let (dir, file) = (s(e, "dir"), s(e, "file"));
                // Only the branches this desktop takes: the manifest is a file
                // on disk, and we do not reset a foreign branch from it.
                // A branch inside one taken as a whole is ours too: earlier GNOME
                // copies took the branches of individual extensions.
                let known = spec.all_parts().any(|p| match p {
                    PanelPart::DconfDir(d) => *d == dir || (dir.starts_with(d) && d.ends_with('/') && !dir.contains("..")),
                    PanelPart::DconfKeys { dir: d, .. } => *d == dir,
                    _ => false,
                });
                if !known || !safe_rel(&file) {
                    continue;
                }
                if kind == "dconf_dir" {
                    let (ok, out) = io.run(&sv(&["dconf", "reset", "-f", &dir]));
                    if !ok {
                        return Err(out);
                    }
                }
                let path = format!("{copy_dir}/{file}");
                let (ok, out) = io.run(&dconf_load_argv(&dir, &path));
                if !ok {
                    return Err(out);
                }
            }
            "mate_panel" => {
                let file = s(e, "file");
                if !spec.all_parts().any(|p| *p == PanelPart::MatePanel) || !safe_rel(&file) {
                    continue;
                }
                mate_panel_load(io, &format!("{copy_dir}/{file}"), live)?;
            }
            "home" => {
                let rel = s(e, "path");
                if !safe_rel(&rel) {
                    continue;
                }
                io.remove_tree(&format!("{home}/{rel}"))?;
                io.copy_tree(&format!("{copy_dir}/home/{rel}"), &format!("{home}/{rel}"))?;
            }
            "config" => {
                let rel = s(e, "path");
                if !safe_rel(&rel) || !spec.all_parts().any(|p| matches!(p, PanelPart::Config(c) if *c == rel)) {
                    continue;
                }
                io.remove_tree(&format!("{config}/{rel}"))?;
                io.copy_tree(&format!("{copy_dir}/config/{rel}"), &format!("{config}/{rel}"))?;
            }
            "data" => {
                let rel = s(e, "path");
                let exclude = spec.all_parts().find_map(|p| match p {
                    PanelPart::DataTree { path, exclude } if *path == rel => Some(*exclude),
                    _ => None,
                });
                let Some(exclude) = exclude else { continue };
                if !safe_rel(&rel) {
                    continue;
                }
                restore_tree_keeping(io, &format!("{copy_dir}/data/{rel}"), &format!("{data}/{rel}"), exclude)?;
            }
            "xfconf" => {
                let (channel, file) = (s(e, "channel"), s(e, "file"));
                if !safe_rel(&file) || !spec.all_parts().any(|p| matches!(p, PanelPart::XfconfChannel(c) if *c == channel)) {
                    continue;
                }
                let Some(xml) = io.read(&format!("{copy_dir}/{file}")) else {
                    return Err(format!("{copy_dir}/{file}"));
                };
                let props = xfconf_parse(&xml)?;
                for argv in xfconf_restore_argvs(&channel, &props) {
                    let (ok, out) = io.run(&argv);
                    if !ok {
                        return Err(out);
                    }
                }
            }
            "kde_shortcuts" => {
                let file = s(e, "file");
                if !safe_rel(&file) || !spec.all_parts().any(|p| *p == PanelPart::KdeShortcuts) {
                    continue;
                }
                let Some(saved) = io.read(&format!("{copy_dir}/{file}")) else {
                    return Err(format!("{copy_dir}/{file}"));
                };
                notes.extend(kde_shortcuts_restore(io, &config, &saved, live)?);
            }
            "dde_shortcuts" => {
                let file = s(e, "file");
                if !safe_rel(&file) || !spec.all_parts().any(|p| *p == PanelPart::DdeShortcuts) {
                    continue;
                }
                let Some(text) = io.read(&format!("{copy_dir}/{file}")) else {
                    return Err(format!("{copy_dir}/{file}"));
                };
                let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                dde_shortcuts_restore(io, &config, &body, live)?;
            }
            "home_matching" => {
                let (parent, prefix) = (s(e, "parent"), s(e, "prefix"));
                if !safe_rel(&parent) || prefix.is_empty() || prefix.contains('/') {
                    continue;
                }
                for n in io.list_dir(&format!("{home}/{parent}")).into_iter().filter(|n| n.starts_with(&prefix)) {
                    io.remove_tree(&format!("{home}/{parent}/{n}"))?;
                }
                let names: Vec<String> = e
                    .get("names")
                    .and_then(serde_json::Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                for n in names.iter().filter(|n| n.starts_with(&prefix) && safe_rel(n) && !n.contains('/')) {
                    io.copy_tree(&format!("{copy_dir}/home/{parent}/{n}"), &format!("{home}/{parent}/{n}"))?;
                }
            }
            "dde_dconfig" => {
                let (appid, resource, file) = (s(e, "appid"), s(e, "resource"), s(e, "file"));
                // Only the resources this desktop takes.
                let known = spec.all_parts().any(
                    |p| matches!(p, PanelPart::DdeDconfig { appid: a, resource: r } if *a == appid && *r == resource),
                );
                if !known || !safe_rel(&file) {
                    continue;
                }
                let Some(text) = io.read(&format!("{copy_dir}/{file}")) else {
                    return Err(format!("{copy_dir}/{file}"));
                };
                let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                let Some(values) = body.get("values").and_then(serde_json::Value::as_object) else {
                    continue;
                };
                for (key, v) in values {
                    if !crate::dde::dconfig_key_ok(key) {
                        continue;
                    }
                    let argv = if v.get("default").and_then(serde_json::Value::as_bool) == Some(true) {
                        crate::dde::dconfig_reset_key_argv(&appid, &resource, key)
                    } else {
                        crate::dde::dconfig_set_key_argv(&appid, &resource, key, &s(v, "value"))
                    };
                    let (ok, out) = io.run(&argv);
                    if !ok {
                        return Err(out);
                    }
                }
            }
            // The GNOME extension inventory: the settings have already been
            // restored by dconf branches, and delivering the missing ones is
            // separate (`gnome_ext_copy_deliver`).
            _ => {}
        }
    }
    if !live {
        return Ok(notes);
    }
    for r in &spec.reload {
        match r {
            PanelReload::Live => {}
            PanelReload::Spawn(argv) => {
                io.spawn(&sv(argv));
            }
            PanelReload::Run(argv) => {
                let (ok, out) = io.run(&sv(argv));
                if !ok {
                    crate::log_warn!("backup", "panel reread command {argv:?} failed: {}", out.trim());
                }
            }
            PanelReload::PlasmaShell => {
                if let Some(note) = io.restart_plasma_shell() {
                    notes.push(note);
                }
            }
            PanelReload::LxqtModule(module) => {
                let call = |method: &str| {
                    sv(&[
                        "dbus-send",
                        "--session",
                        "--type=method_call",
                        "--dest=org.lxqt.session",
                        "/LXQtSession",
                        &format!("org.lxqt.session.{method}"),
                        &format!("string:{module}"),
                    ])
                };
                io.run(&call("stopModule"));
                io.pause();
                io.run(&call("startModule"));
            }
            PanelReload::ReloadSignal(exe) => {
                io.reload_signal(exe);
            }
            PanelReload::RestartUserUnit(unit) => {
                io.run(&sv(&["systemctl", "--user", "--no-block", "try-restart", unit]));
            }
        }
    }
    Ok(notes)
}

/// `dconf load <dir> < <file>` through the shell (dconf reads the dump from stdin).
pub fn dconf_load_argv(dir: &str, file: &str) -> Vec<String> {
    sv(&["sh", "-c", "dconf load \"$1\" < \"$2\"", "sh", dir, file])
}

/// Put folder `dest` back from `src`, keeping the person's current `keep`
/// subpaths (relative to `dest`) as they are: they belong to someone else.
fn restore_tree_keeping(io: &dyn PanelIo, src: &str, dest: &str, keep: &[&str]) -> Result<(), String> {
    let stash = format!("{dest}.rexon-keep");
    io.remove_tree(&stash)?;
    let kept: Vec<&str> = keep.iter().copied().filter(|k| io.exists(&format!("{dest}/{k}"))).collect();
    for k in &kept {
        io.copy_tree(&format!("{dest}/{k}"), &format!("{stash}/{k}"))?;
    }
    io.remove_tree(dest)?;
    if io.exists(src) {
        io.copy_tree(src, dest)?;
    }
    for k in keep {
        io.remove_tree(&format!("{dest}/{k}"))?;
    }
    for k in &kept {
        io.copy_tree(&format!("{stash}/{k}"), &format!("{dest}/{k}"))?;
    }
    io.remove_tree(&stash)
}

// ── MATE: the panel branch is loaded by the panel's own procedure ─────────

/// The MATE panel settings branch.
pub const MATE_PANEL_DCONF_DIR: &str = crate::mate_panel::PANEL_DCONF_DIR;

/// Load MATE panel dump `file`. On a live panel the objects are emptied first
/// and the panel is given time to drop them (see `mate_panel::restore`, where
/// the procedure was measured); on failure the panel as it was is put back.
pub fn mate_panel_load(io: &dyn PanelIo, file: &str, live: bool) -> Result<(), String> {
    let undo = format!("{file}.before-restore");
    let (ok, now) = io.run(&sv(&["dconf", "dump", MATE_PANEL_DCONF_DIR]));
    if !ok || !io.write(&undo, &now) {
        return Err(if ok { undo } else { now });
    }
    if live {
        let (ok, out) = io.run(&crate::mate_panel::empty_objects_argv());
        if !ok {
            return Err(out);
        }
        for _ in 0..3 {
            let (ok, out) = io.run(&sv(&["dconf", "list", "/org/mate/panel/objects/"]));
            if !ok || out.trim().is_empty() {
                break;
            }
            io.pause();
        }
    }
    for step in crate::mate_panel::load_steps(file) {
        let (ok, out) = io.run(&step);
        if !ok {
            for back in crate::mate_panel::rollback_steps(&undo) {
                io.run(&back);
            }
            let _ = io.remove_tree(&undo);
            return Err(out);
        }
    }
    let _ = io.remove_tree(&undo);
    Ok(())
}

// ── KDE: global shortcuts back into a running KWin ────────────────────────
//
// In Plasma 6 the global shortcuts service lives inside KWin: it reads
// `kglobalshortcutsrc` once, at login, and keeps the shortcuts in memory.
// Writing the file back is therefore not enough for the running session, so
// the actions whose keys differ are also handed to KWin over D-Bus
// (`org.kde.KGlobalAccel.setForeignShortcutKeys`). KConfig writes back only
// the entries it changed itself, so whatever could not be handed over stays
// in the file and takes effect at the next login: the person is told so.

/// The Plasma global shortcuts file in the config home.
pub const KDE_SHORTCUTS_FILE: &str = "kglobalshortcutsrc";

/// One action of `kglobalshortcutsrc`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdeShortcut {
    /// The component (`kwin`, `org.kde.dolphin.desktop` for a program).
    pub component: String,
    pub action: String,
    /// The active key sequences in Qt's portable text (`Meta+E`); empty = none.
    pub keys: Vec<String>,
    pub component_name: String,
    pub action_name: String,
}

/// Split a KConfig list value on unescaped commas and undo KConfig escapes.
fn kconfig_list(value: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('t') => out.last_mut().unwrap().push('\t'),
                Some('n') => out.last_mut().unwrap().push('\n'),
                Some('s') => out.last_mut().unwrap().push(' '),
                Some(other) => out.last_mut().unwrap().push(other),
                None => {}
            },
            ',' => out.push(String::new()),
            other => out.last_mut().unwrap().push(other),
        }
    }
    out
}

/// Parse `kglobalshortcutsrc`: `[component]` groups (`[services][x.desktop]`
/// for programs), `action=active,default,friendly name` entries,
/// `_k_friendly_name` for the component's name.
pub fn kde_shortcuts_parse(text: &str) -> Vec<KdeShortcut> {
    let mut out: Vec<KdeShortcut> = Vec::new();
    let mut component = String::new();
    let mut component_name = String::new();
    let mut start = 0;
    let close = |out: &mut Vec<KdeShortcut>, from: usize, name: &str| {
        for s in out.iter_mut().skip(from) {
            s.component_name = name.to_string();
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            close(&mut out, start, &component_name);
            start = out.len();
            component = t.rsplit('[').next().unwrap_or("").trim_end_matches(']').to_string();
            component_name = component.clone();
            continue;
        }
        if t.is_empty() || t.starts_with('#') || component.is_empty() {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else { continue };
        let key = key.trim();
        if key == "_k_friendly_name" {
            component_name = value.to_string();
            continue;
        }
        let fields = kconfig_list(value);
        let active = fields.first().cloned().unwrap_or_default();
        let keys: Vec<String> = if active == "none" || active.is_empty() {
            Vec::new()
        } else {
            active.split('\t').filter(|k| !k.is_empty() && *k != "none").map(str::to_string).collect()
        };
        out.push(KdeShortcut {
            component: component.clone(),
            action: key.to_string(),
            keys,
            component_name: String::new(),
            action_name: fields.get(2).cloned().unwrap_or_else(|| key.to_string()),
        });
    }
    close(&mut out, start, &component_name);
    out
}

/// Qt key codes by portable name (`Qt::Key`), for the keys global shortcuts
/// use. A name missing here is not guessed: that action is left to the next
/// login.
const QT_KEYS: &[(&str, u32)] = &[
    ("Esc", 0x0100_0000), ("Tab", 0x0100_0001), ("Backtab", 0x0100_0002), ("Backspace", 0x0100_0003),
    ("Return", 0x0100_0004), ("Enter", 0x0100_0005), ("Ins", 0x0100_0006), ("Del", 0x0100_0007),
    ("Pause", 0x0100_0008), ("Print", 0x0100_0009), ("SysReq", 0x0100_000a), ("Clear", 0x0100_000b),
    ("Home", 0x0100_0010), ("End", 0x0100_0011), ("Left", 0x0100_0012), ("Up", 0x0100_0013),
    ("Right", 0x0100_0014), ("Down", 0x0100_0015), ("PgUp", 0x0100_0016), ("PgDown", 0x0100_0017),
    ("CapsLock", 0x0100_0024), ("NumLock", 0x0100_0025), ("ScrollLock", 0x0100_0026), ("Menu", 0x0100_0055),
    ("Space", 0x20),
    ("Back", 0x0100_0061), ("Forward", 0x0100_0062), ("Stop", 0x0100_0063), ("Refresh", 0x0100_0064),
    ("Volume Down", 0x0100_0070), ("Volume Mute", 0x0100_0071), ("Volume Up", 0x0100_0072),
    ("Media Play", 0x0100_0080), ("Media Stop", 0x0100_0081), ("Media Previous", 0x0100_0082),
    ("Media Next", 0x0100_0083), ("Media Record", 0x0100_0084), ("Media Pause", 0x0100_0085),
    ("Toggle Media Play/Pause", 0x0100_0086),
    ("Home Page", 0x0100_0090), ("Favorites", 0x0100_0091), ("Search", 0x0100_0092),
    ("Launch Mail", 0x0100_00a0), ("Launch Media", 0x0100_00a1),
    ("Monitor Brightness Up", 0x0100_00b2), ("Monitor Brightness Down", 0x0100_00b3),
    ("Keyboard Light On/Off", 0x0100_00b4), ("Keyboard Brightness Up", 0x0100_00b5),
    ("Keyboard Brightness Down", 0x0100_00b6), ("Power Off", 0x0100_00b7), ("Eject", 0x0100_00b9),
    ("Screensaver", 0x0100_00ba), ("Calculator", 0x0100_00cb),
    ("Hibernate", 0x0100_0108), ("Suspend", 0x0100_010c),
    ("Touchpad Toggle", 0x0100_0110), ("Touchpad On", 0x0100_0111), ("Touchpad Off", 0x0100_0112),
    ("Microphone Mute", 0x0100_0113),
];

/// One key combination (`Meta+Shift+E`) as Qt's combined int, if every part is known.
fn qt_key_combination(text: &str) -> Option<u32> {
    let text = text.trim();
    // `Ctrl++` is Ctrl with the plus key.
    let (mods_part, key) = match text.strip_suffix("++") {
        Some(rest) => (rest, "+"),
        None => match text.rsplit_once('+') {
            Some((m, k)) => (m, k),
            None => ("", text),
        },
    };
    let mut combined = 0u32;
    if !mods_part.is_empty() {
        for m in mods_part.split('+') {
            combined |= match m {
                "Shift" => 0x0200_0000,
                "Ctrl" => 0x0400_0000,
                "Alt" => 0x0800_0000,
                "Meta" => 0x1000_0000,
                "Num" => 0x2000_0000,
                _ => return None,
            };
        }
    }
    let code = if let Some((_, c)) = QT_KEYS.iter().find(|(n, _)| *n == key) {
        *c
    } else if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=35).contains(n)) {
        0x0100_0030 + n - 1
    } else if let Some(n) = key.strip_prefix("Launch (").and_then(|r| r.strip_suffix(')')) {
        let c = n.chars().next().filter(|_| n.chars().count() == 1)?;
        match c {
            '0'..='9' => 0x0100_00a2 + (c as u32 - '0' as u32),
            'A'..='F' => 0x0100_00ac + (c as u32 - 'A' as u32),
            _ => return None,
        }
    } else {
        let mut cs = key.chars();
        let c = cs.next()?;
        if cs.next().is_some() || !(c.is_ascii_graphic()) {
            return None;
        }
        c.to_ascii_uppercase() as u32
    };
    Some(combined | code)
}

/// A key sequence (`Ctrl+X, Ctrl+C`, up to four combinations) as the four
/// ints KGlobalAccel takes; `None` if any part is unknown.
pub fn qt_key_sequence(text: &str) -> Option<[u32; 4]> {
    let mut out = [0u32; 4];
    let combos: Vec<&str> = if text.trim() == "," { vec![","] } else { text.split(", ").collect() };
    if combos.is_empty() || combos.len() > 4 {
        return None;
    }
    for (i, c) in combos.iter().enumerate() {
        out[i] = qt_key_combination(c)?;
    }
    Some(out)
}

/// A string as a GVariant text literal (for `gdbus call`).
fn gvariant_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The `gdbus` call that gives action `s` its keys in the running KWin;
/// `None` if a key name is not known.
pub fn kde_set_shortcut_argv(s: &KdeShortcut) -> Option<Vec<String>> {
    let mut seqs = Vec::new();
    for k in &s.keys {
        let q = qt_key_sequence(k)?;
        seqs.push(format!("([{}, {}, {}, {}],)", q[0], q[1], q[2], q[3]));
    }
    let id = [&s.component, &s.action, &s.component_name, &s.action_name]
        .iter()
        .map(|v| gvariant_str(v))
        .collect::<Vec<_>>()
        .join(", ");
    let keys = if seqs.is_empty() { "@a(ai) []".to_string() } else { format!("[{}]", seqs.join(", ")) };
    Some(sv(&[
        "gdbus",
        "call",
        "--session",
        "--dest",
        "org.kde.kglobalaccel",
        "--object-path",
        "/kglobalaccel",
        "--method",
        "org.kde.KGlobalAccel.setForeignShortcutKeys",
        &format!("[{id}]"),
        &keys,
    ]))
}

/// Message: some shortcuts take effect only at the next login.
pub fn msg_kde_shortcuts_next_login(n: usize) -> String {
    ntr(
        "%n keyboard shortcut will take effect after you log in again.",
        "%n keyboard shortcuts will take effect after you log in again.",
        n as u64,
    )
}

/// Put `kglobalshortcutsrc` back from `saved` -> notes for the person.
///
/// `live` (a Plasma session): the actions whose keys differ from the current
/// file are handed to KWin first; what it does not take is counted for the
/// note. The file is written in any case, so the next login reads the copy.
pub fn kde_shortcuts_restore(io: &dyn PanelIo, config: &str, saved: &str, live: bool) -> Result<Vec<String>, String> {
    let path = format!("{config}/{KDE_SHORTCUTS_FILE}");
    let current = kde_shortcuts_parse(&io.read(&path).unwrap_or_default());
    if !io.write(&path, saved) {
        return Err(path);
    }
    if !live {
        return Ok(Vec::new());
    }
    let mut later = 0usize;
    for s in kde_shortcuts_parse(saved) {
        let now = current.iter().find(|c| c.component == s.component && c.action == s.action);
        if now.is_some_and(|c| c.keys == s.keys) {
            continue;
        }
        // An action KWin does not know yet is not registered there: it is
        // read from the file at the next login.
        let applied = now.is_some() && kde_set_shortcut_argv(&s).is_some_and(|argv| io.run(&argv).0);
        if !applied {
            later += 1;
        }
    }
    Ok(if later > 0 { vec![msg_kde_shortcuts_next_login(later)] } else { Vec::new() })
}


// ── Deepin: shortcuts and gestures of dde-services ────────────────────────
//
// Since dde-daemon 6.1.105 the shortcuts belong to dde-services: every entry
// is a DConfig subpath of `org.deepin.dde.keybinding` (its writable keys are
// the keys and whether it is on), the list of entries is in the registration
// files, the person's own shortcuts are subpaths named in
// `custom-shortcuts.ini`. The files under `~/.config/dsg` belong to the
// DConfig daemon, so values go through `dde-dconfig` only. dde-services
// follows every entry and rebinds it in Treeland at once; a swap of two
// shortcuts can still leave one unbound on the way, so after a restore the
// service group is restarted and registers the whole set anew.

/// The shortcuts file in a Deepin copy.
pub const DDE_SHORTCUTS_COPY_FILE: &str = "dde-shortcuts.json";

/// The resource of entry `subpath` on this machine: a shortcut or a gesture;
/// `None` means the release does not know it.
fn dde_entry_resource(io: &dyn PanelIo, subpath: &str) -> Option<(&'static str, String)> {
    use crate::dde::{dconfig_subpath_schema_path, GESTURE_RESOURCE, KEYBINDING_APPID, SHORTCUT_RESOURCE};
    [SHORTCUT_RESOURCE, GESTURE_RESOURCE]
        .into_iter()
        .find_map(|r| io.read(&dconfig_subpath_schema_path(KEYBINDING_APPID, subpath, r)).map(|schema| (r, schema)))
}

/// The system entries: the registration files' subpaths and the gesture
/// folders of the schemas, without repeats.
fn dde_system_subpaths(io: &dyn PanelIo) -> Vec<String> {
    use crate::dde::{keybinding_ini_subpaths, KEYBINDING_INI_DIR, KEYBINDING_SCHEMA_DIR};
    let mut out: Vec<String> = Vec::new();
    for name in io.list_dir(KEYBINDING_INI_DIR).into_iter().filter(|n| n.ends_with(".ini")) {
        for sub in keybinding_ini_subpaths(&io.read(&format!("{KEYBINDING_INI_DIR}/{name}")).unwrap_or_default()) {
            if !out.contains(&sub) {
                out.push(sub);
            }
        }
    }
    for name in io.list_dir(KEYBINDING_SCHEMA_DIR).into_iter().filter(|n| n.contains(".gesture.")) {
        let sub = format!("/{name}");
        if crate::dde::dconfig_subpath_ok(&sub) && !out.contains(&sub) {
            out.push(sub);
        }
    }
    out
}

/// The person's own shortcuts from `custom-shortcuts.ini`, in its order.
fn dde_custom_subpaths(text: &str) -> Vec<String> {
    crate::dde::keybinding_ini_subpaths(text)
}

/// The base schema of the person's own shortcuts: all their keys.
fn dde_custom_keys(io: &dyn PanelIo) -> Vec<String> {
    use crate::dde::{dconfig_schema_path, KEYBINDING_APPID, SHORTCUT_RESOURCE};
    crate::dde::dconfig_schema_writable_keys(
        &io.read(&dconfig_schema_path(KEYBINDING_APPID, SHORTCUT_RESOURCE)).unwrap_or_default(),
    )
}

/// The values of `keys` of entry `subpath`: `{key: {value, default}}`.
fn dde_entry_values(
    io: &dyn PanelIo,
    resource: &str,
    subpath: &str,
    keys: &[String],
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    use crate::dde::{dconfig_cli_value, dconfig_get_sub_argv, dconfig_is_default_sub_argv, KEYBINDING_APPID};
    let mut values = serde_json::Map::new();
    for key in keys {
        let (ok, raw) = io.run(&dconfig_get_sub_argv(KEYBINDING_APPID, resource, subpath, key));
        if !ok {
            return Err(raw);
        }
        let (_, default) = io.run(&dconfig_is_default_sub_argv(KEYBINDING_APPID, resource, subpath, key));
        values.insert(
            key.clone(),
            serde_json::json!({ "value": dconfig_cli_value(&raw), "default": default.trim() == "true" }),
        );
    }
    Ok(values)
}

/// Take the shortcuts and gestures -> the body of [`DDE_SHORTCUTS_COPY_FILE`].
fn dde_shortcuts_save(io: &dyn PanelIo, config: &str) -> Result<serde_json::Value, String> {
    let mut system = Vec::new();
    for sub in dde_system_subpaths(io) {
        // An entry this release lists but has no schema for is skipped, not fatal.
        let Some((resource, schema)) = dde_entry_resource(io, &sub) else { continue };
        let keys = crate::dde::dconfig_schema_writable_keys(&schema);
        if keys.is_empty() {
            continue;
        }
        let values = dde_entry_values(io, resource, &sub, &keys)?;
        system.push(serde_json::json!({ "subpath": sub, "resource": resource, "values": values }));
    }
    let ini = io.read(&format!("{config}/{}", crate::dde::CUSTOM_SHORTCUTS_INI));
    let keys = dde_custom_keys(io);
    let mut custom = Vec::new();
    for sub in dde_custom_subpaths(ini.as_deref().unwrap_or("")) {
        let values = dde_entry_values(io, crate::dde::SHORTCUT_RESOURCE, &sub, &keys)?;
        custom.push(serde_json::json!({ "subpath": sub, "values": values }));
    }
    Ok(serde_json::json!({ "system": system, "custom": custom, "custom_ini": ini }))
}

/// Write entry `subpath`'s values back: a default one is reset, the rest set.
/// Only keys the current schema allows (`allowed`) are touched.
fn dde_entry_restore(
    io: &dyn PanelIo,
    resource: &str,
    subpath: &str,
    values: &serde_json::Map<String, serde_json::Value>,
    allowed: &[String],
) -> Result<(), String> {
    use crate::dde::{dconfig_reset_sub_argv, dconfig_set_sub_argv, KEYBINDING_APPID};
    for (key, v) in values {
        if !allowed.contains(key) {
            continue;
        }
        let argv = if v.get("default").and_then(serde_json::Value::as_bool) == Some(true) {
            dconfig_reset_sub_argv(KEYBINDING_APPID, resource, subpath, key)
        } else {
            let value = v.get("value").and_then(serde_json::Value::as_str).unwrap_or("");
            dconfig_set_sub_argv(KEYBINDING_APPID, resource, subpath, key, value)
        };
        let (ok, out) = io.run(&argv);
        if !ok {
            return Err(out);
        }
    }
    Ok(())
}

/// Put the shortcuts and gestures back from `body`.
fn dde_shortcuts_restore(io: &dyn PanelIo, config: &str, body: &serde_json::Value, live: bool) -> Result<(), String> {
    let list = |k: &str| body.get(k).and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let sub_of = |e: &serde_json::Value| e.get("subpath").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let empty = serde_json::Map::new();
    for e in list("system") {
        let sub = sub_of(&e);
        if !crate::dde::dconfig_subpath_ok(&sub) {
            continue;
        }
        // The copy may come from another release: an entry unknown here is skipped.
        let Some((resource, schema)) = dde_entry_resource(io, &sub) else {
            crate::log_info!("backup", "Deepin shortcut {sub} from the copy is unknown here, skipped");
            continue;
        };
        let allowed = crate::dde::dconfig_schema_writable_keys(&schema);
        let values = e.get("values").and_then(serde_json::Value::as_object).unwrap_or(&empty);
        dde_entry_restore(io, resource, &sub, values, &allowed)?;
    }
    let keys = dde_custom_keys(io);
    let ini_path = format!("{config}/{}", crate::dde::CUSTOM_SHORTCUTS_INI);
    let saved: Vec<serde_json::Value> = list("custom");
    let saved_subs: Vec<String> = saved.iter().map(sub_of).collect();
    // Own shortcuts made after the copy: their values are reset, as removing
    // one in the control center does.
    for sub in dde_custom_subpaths(&io.read(&ini_path).unwrap_or_default()) {
        if saved_subs.contains(&sub) {
            continue;
        }
        let defaults: serde_json::Map<String, serde_json::Value> =
            keys.iter().map(|k| (k.clone(), serde_json::json!({ "default": true }))).collect();
        dde_entry_restore(io, crate::dde::SHORTCUT_RESOURCE, &sub, &defaults, &keys)?;
    }
    for e in &saved {
        let sub = sub_of(e);
        if !crate::dde::dconfig_subpath_ok(&sub) {
            continue;
        }
        let values = e.get("values").and_then(serde_json::Value::as_object).unwrap_or(&empty);
        dde_entry_restore(io, crate::dde::SHORTCUT_RESOURCE, &sub, values, &keys)?;
    }
    match body.get("custom_ini").and_then(serde_json::Value::as_str) {
        Some(text) => {
            if !io.write(&ini_path, text) {
                return Err(ini_path);
            }
        }
        None => io.remove_tree(&ini_path)?,
    }
    if live {
        let (ok, out) = io.run(&crate::dde::keybinding_reload_argv());
        if !ok {
            crate::log_warn!("backup", "dde-services did not reread its shortcuts: {}", out.trim());
        }
    }
    Ok(())
}

// ── XFCE: xfconf channels without xfce4-panel-profiles ────────────────────
//
// A channel is saved as its own XML file (it carries the types), and is put
// back property by property through `xfconf-query`: writing the file over
// would be undone by xfconfd, which keeps the channel in memory and writes
// it back.

/// The xfconf file of channel `channel` under config home `config`.
pub fn xfconf_channel_path(config: &str, channel: &str) -> String {
    format!("{}/xfce4/xfconf/xfce-perchannel-xml/{channel}.xml", config.trim_end_matches('/'))
}

/// One xfconf property with its value(s): `(type, value)` pairs; an array has
/// any number of them, a scalar exactly one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XfconfProp {
    pub path: String,
    pub array: bool,
    pub values: Vec<(String, String)>,
}

/// xfconf value types `xfconf-query -t` knows.
const XFCONF_TYPES: &[&str] = &[
    "string", "int", "uint", "bool", "double", "float", "int64", "uint64", "char", "uchar", "int16", "uint16",
];

/// Parse a channel file: `<channel>` with nested `<property name type value>`;
/// `type="empty"` is only a container, `type="array"` has `<value type value/>` children.
pub fn xfconf_parse(xml: &str) -> Result<Vec<XfconfProp>, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let attrs = |e: &quick_xml::events::BytesStart| -> Result<(String, String, String), String> {
        let (mut name, mut ty, mut value) = (String::new(), String::new(), String::new());
        for attr in e.attributes() {
            let attr = attr.map_err(|e| e.to_string())?;
            let v = attr.unescape_value().map_err(|e| e.to_string())?.into_owned();
            match attr.key.as_ref() {
                b"name" => name = v,
                b"type" => ty = v,
                b"value" => value = v,
                _ => {}
            }
        }
        Ok((name, ty, value))
    };
    let mut out: Vec<XfconfProp> = Vec::new();
    // The open properties: their paths; an open array collects its values.
    let mut stack: Vec<String> = Vec::new();
    // Open arrays (an array may hold child properties, arrays among them).
    let mut arrays: Vec<XfconfProp> = Vec::new();
    let mut seen_channel = false;
    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let (e, empty) = match &event {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                match e.name().as_ref() {
                    b"property" => {
                        let path = stack.pop().ok_or("unbalanced <property>")?;
                        if arrays.last().is_some_and(|a| a.path == path) {
                            out.extend(arrays.pop());
                        }
                    }
                    b"value" | b"channel" => {}
                    other => return Err(format!("unexpected </{}>", String::from_utf8_lossy(other))),
                }
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        match e.name().as_ref() {
            b"channel" => seen_channel = true,
            b"property" => {
                let (name, ty, value) = attrs(e)?;
                if name.is_empty() || name.contains('/') {
                    return Err(format!("bad property name “{name}”"));
                }
                let path = format!("{}/{name}", stack.last().map(String::as_str).unwrap_or(""));
                match ty.as_str() {
                    "empty" => {}
                    "array" => {
                        let prop = XfconfProp { path: path.clone(), array: true, values: Vec::new() };
                        // An empty array has nothing to write.
                        if !empty {
                            arrays.push(prop);
                        }
                    }
                    t if XFCONF_TYPES.contains(&t) => {
                        out.push(XfconfProp { path: path.clone(), array: false, values: vec![(ty.clone(), value)] })
                    }
                    other => return Err(format!("unknown xfconf type “{other}” at {path}")),
                }
                if !empty {
                    stack.push(path);
                }
            }
            b"value" => {
                let (_, ty, value) = attrs(e)?;
                if !XFCONF_TYPES.contains(&ty.as_str()) {
                    return Err(format!("unknown xfconf array type “{ty}”"));
                }
                match arrays.last_mut() {
                    Some(a) if stack.last() == Some(&a.path) => a.values.push((ty, value)),
                    _ => return Err("<value> outside an array".to_string()),
                }
            }
            other => return Err(format!("unexpected <{}>", String::from_utf8_lossy(other))),
        }
    }
    if !seen_channel || !stack.is_empty() {
        return Err("not an xfconf channel file".to_string());
    }
    Ok(out)
}

/// Commands that put channel `channel` back to `props`: the whole channel is
/// reset first (a property added since the copy must go), then each property
/// is created with its type.
pub fn xfconf_restore_argvs(channel: &str, props: &[XfconfProp]) -> Vec<Vec<String>> {
    let mut out = vec![sv(&["xfconf-query", "-c", channel, "-p", "/", "-r", "-R"])];
    for p in props {
        if p.values.is_empty() {
            continue;
        }
        let mut argv = sv(&["xfconf-query", "-c", channel, "-p", &p.path, "-n"]);
        if p.array {
            argv.push("-a".to_string());
        }
        for (ty, value) in &p.values {
            argv.extend(["-t".to_string(), ty.clone(), "-s".to_string(), value.clone()]);
        }
        out.push(argv);
    }
    out
}

// ── GNOME extensions in the copy ─────────────────────────────────────────
//
// A GNOME copy remembers which extensions were installed: uuid, whether
// enabled, user or system. "Restore" first returns the settings (dconf
// branches and both shell lists), then delivers the missing user
// extensions from extensions.gnome.org: with a live shell through its
// installation (a confirmation window, started right away), without it
// through the fallback path (the release zip, it will work at the next
// GNOME login). Enabled state as it was. What the site does not have,
// what does not support this GNOME version, and system extensions
// missing from the machine are stated in words, without attempts.

/// The extension inventory file in the copy.
pub const GNOME_EXT_COPY_FILE: &str = "gnome-extensions.json";

/// Directories of system GNOME Shell extensions.
pub const GNOME_EXT_SYSTEM_ROOTS: &[&str] =
    &["/usr/share/gnome-shell/extensions", "/usr/local/share/gnome-shell/extensions"];

/// A GNOME extension in the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedGnomeExt {
    pub uuid: String,
    /// The name for messages; empty means the uuid is shown.
    #[serde(default)]
    pub name: String,
    pub enabled: bool,
    /// A system one (from a distribution package): not installed from the site.
    #[serde(default)]
    pub system: bool,
}

impl SavedGnomeExt {
    fn label(&self) -> &str {
        if self.name.is_empty() { &self.uuid } else { &self.name }
    }
}

/// The inventory from the shell's snapshot (it is the truth).
pub fn gnome_ext_inventory_from_shell(list: &[crate::gnome_shell_ext::ExtInfo]) -> Vec<SavedGnomeExt> {
    let mut out: Vec<SavedGnomeExt> = list
        .iter()
        .map(|e| SavedGnomeExt {
            uuid: e.uuid.clone(),
            name: e.name.clone(),
            enabled: e.enabled || e.state == crate::gnome_shell_ext::ExtState::Active,
            system: e.is_system,
        })
        .collect();
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    out
}

/// The inventory without the shell: extension directories (a user one
/// overrides a system one with the same uuid, as in GNOME) and the
/// settings lists (`disabled-extensions` beats `enabled-extensions`).
pub fn gnome_ext_inventory_from_disk(
    user: &[String],
    system: &[String],
    enabled: &[String],
    disabled: &[String],
) -> Vec<SavedGnomeExt> {
    let mut map: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for u in system {
        map.insert(u.clone(), true);
    }
    for u in user {
        map.insert(u.clone(), false);
    }
    map.into_iter()
        .filter(|(u, _)| crate::rxn_manifest::is_extension_uuid(u))
        .map(|(uuid, system)| SavedGnomeExt {
            enabled: enabled.contains(&uuid) && !disabled.contains(&uuid),
            name: String::new(),
            system,
            uuid,
        })
        .collect()
}

/// The extension inventory of this machine: from the shell, otherwise from disk and keys.
pub fn gnome_ext_inventory(io: &dyn PanelIo, data_home: &str) -> Vec<SavedGnomeExt> {
    if let Some(list) = io.gnome_shell_list() {
        return gnome_ext_inventory_from_shell(&list);
    }
    let user = io.list_dir(&format!("{}/gnome-shell/extensions", data_home.trim_end_matches('/')));
    let system: Vec<String> = GNOME_EXT_SYSTEM_ROOTS.iter().flat_map(|r| io.list_dir(r)).collect();
    let key = |k: &str| {
        let (ok, out) = io.run(&sv(&["gsettings", "get", "org.gnome.shell", k]));
        if ok { crate::de_apply::parse_gsettings_list(&out) } else { Vec::new() }
    };
    gnome_ext_inventory_from_disk(&user, &system, &key("enabled-extensions"), &key("disabled-extensions"))
}

/// The extension inventory from copy `copy_dir`; none means empty.
pub fn gnome_ext_copy_saved(io: &dyn PanelIo, copy_dir: &str) -> Vec<SavedGnomeExt> {
    #[derive(Deserialize)]
    struct Body {
        #[serde(default)]
        extensions: Vec<SavedGnomeExt>,
    }
    let Some(text) = io.read(&format!("{copy_dir}/manifest.json")) else {
        return Vec::new();
    };
    let entries: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
    let Some(file) = entries.iter().find_map(|e| {
        (e.get("kind").and_then(serde_json::Value::as_str) == Some("gnome_extensions"))
            .then(|| e.get("file").and_then(serde_json::Value::as_str).unwrap_or("").to_string())
    }) else {
        return Vec::new();
    };
    if !safe_rel(&file) {
        return Vec::new();
    }
    io.read(&format!("{copy_dir}/{file}"))
        .and_then(|t| serde_json::from_str::<Body>(&t).ok())
        .map(|b| b.extensions.into_iter().filter(|e| crate::rxn_manifest::is_extension_uuid(&e.uuid)).collect())
        .unwrap_or_default()
}

/// The result of delivering extensions from the copy.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GnomeExtDelivery {
    /// Installed (through the shell or the fallback path).
    pub installed: Vec<String>,
    /// Not installed: one line per extension, in words.
    pub problems: Vec<String>,
    /// Other lines (installed, cancelled in the shell's window).
    pub notes: Vec<String>,
}

impl GnomeExtDelivery {
    /// Empty means nothing to say (everything was in place).
    pub fn is_empty(&self) -> bool {
        self.installed.is_empty() && self.problems.is_empty() && self.notes.is_empty()
    }

    /// The text for the status line.
    pub fn message(&self) -> String {
        let mut lines = self.notes.clone();
        lines.extend(self.problems.iter().cloned());
        lines.join("\n")
    }
}

/// Message: the system extension from the copy is not on the machine.
pub fn msg_gnome_ext_system_missing(name: &str) -> String {
    trf(
        "“%1” is a system extension and isn't on this computer — it comes with a distribution package.",
        &[&name],
    )
}

/// Message: the extension is not on extensions.gnome.org.
pub fn msg_gnome_ext_not_on_site(name: &str) -> String {
    trf("“%1” isn't on extensions.gnome.org, so Rexon can't install it again.", &[&name])
}

/// Message: the extension has no release for this GNOME version.
pub fn msg_gnome_ext_unsupported(name: &str, shell: &str) -> String {
    trf("“%1” has no release for GNOME %2, so it wasn't installed.", &[&name, &shell])
}

/// Message: the installation failed.
pub fn msg_gnome_ext_failed(name: &str, why: &str) -> String {
    trf("“%1” wasn't installed: %2", &[&name, &why])
}

/// Releases of an extension from extensions.gnome.org.
pub type GnomeExtReleases<'a> =
    dyn FnMut(&str) -> Result<Vec<crate::gnome_ego::Release>, crate::gnome_ego::FetchError> + 'a;
/// Download the release zip for the shell version (the fallback path without the shell).
pub type GnomeExtFetchZip<'a> =
    dyn FnMut(&str, Option<&crate::gnome_ego::ShellVersion>) -> Result<std::path::PathBuf, String> + 'a;

/// Deliver the extensions from copy `saved` that are not among `present`.
/// The network goes only through `releases` and `fetch_zip`, so it is
/// checked with fakes.
pub fn gnome_ext_copy_deliver(
    shell: &mut dyn crate::gnome_shell_ext::ShellExtIo,
    saved: &[SavedGnomeExt],
    present: &[SavedGnomeExt],
    data_home: &str,
    releases: &mut GnomeExtReleases,
    fetch_zip: &mut GnomeExtFetchZip,
) -> GnomeExtDelivery {
    use crate::gnome_shell_ext::InstallOutcome;
    let mut report = GnomeExtDelivery::default();
    let missing: Vec<&SavedGnomeExt> =
        saved.iter().filter(|s| !present.iter().any(|p| p.uuid == s.uuid)).collect();
    if missing.is_empty() {
        return report;
    }
    let version = crate::gnome_shell_ext::detect_shell_version(shell)
        .and_then(|v| crate::gnome_ego::parse_shell_version(&v));
    for ext in missing {
        let name = ext.label();
        if ext.system {
            report.problems.push(msg_gnome_ext_system_missing(name));
            continue;
        }
        let list = match releases(&ext.uuid) {
            Ok(l) => l,
            Err(e) if e.is_not_found() => {
                report.problems.push(msg_gnome_ext_not_on_site(name));
                continue;
            }
            Err(e) => {
                report.problems.push(msg_gnome_ext_failed(name, &e.message()));
                continue;
            }
        };
        if crate::gnome_ext_section::offline_release(&list, None).is_none() {
            report.problems.push(msg_gnome_ext_not_on_site(name));
            continue;
        }
        if let Some(v) = &version {
            if crate::gnome_ego::release_for(&list, v).is_none() {
                report.problems.push(msg_gnome_ext_unsupported(name, &crate::gnome_ego::shell_label(v)));
                continue;
            }
        }
        let uuid = ext.uuid.clone();
        let outcome =
            crate::gnome_ext_section::install(shell, &uuid, data_home, &mut || fetch_zip(&uuid, version.as_ref()));
        match outcome {
            Ok(InstallOutcome::Cancelled) => {
                report.notes.push(crate::gnome_ext_section::install_message(name, InstallOutcome::Cancelled).1);
            }
            Ok(o) => {
                // What was installed is enabled (both the shell and the fallback
                // path install that way); what was disabled in the copy gets
                // disabled back.
                if !ext.enabled {
                    let _ = crate::gnome_ext_section::set_enabled(shell, &uuid, false, data_home);
                }
                report.notes.push(crate::gnome_ext_section::install_message(name, o).1);
                report.installed.push(uuid);
            }
            Err(e) => report.problems.push(msg_gnome_ext_failed(name, &e.message())),
        }
    }
    report
}

/// Delivering extensions from GNOME copy `dir` on the live machine (network,
/// shell; call it from a worker thread: the shell's window waits for the
/// person). `None` means there is no inventory in the copy or everything is
/// in place.
pub fn gnome_ext_copy_deliver_live_from(dir: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let saved = gnome_ext_copy_saved(&LivePanelIo, dir);
    if saved.is_empty() {
        return None;
    }
    let data_home = LivePanelIo.data_home(&home);
    let present = gnome_ext_inventory(&LivePanelIo, &data_home);
    let token = crate::net::CancelToken::new();
    let work = std::env::temp_dir().join(format!("rexon-gnome-ext-{}", std::process::id()));
    let report = gnome_ext_copy_deliver(
        &mut crate::gnome_shell_ext::LiveShell::new(),
        &saved,
        &present,
        &data_home,
        &mut |uuid| crate::gnome_ego::live_releases(uuid, &token),
        &mut |uuid, shell| crate::gnome_ext_section::live_fetch_zip(uuid, shell, &work, &token),
    );
    let _ = fs::remove_dir_all(&work);
    (!report.is_empty()).then(|| report.message())
}

/// The live hands of the panel copy.
pub struct LivePanelIo;

fn copy_tree_keep_links(src: &Path, dest: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        if let Some(p) = dest.parent() {
            fs::create_dir_all(p)?;
        }
        let target = fs::read_link(src)?;
        let _ = fs::remove_file(dest);
        return std::os::unix::fs::symlink(target, dest);
    }
    if meta.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_tree_keep_links(&entry.path(), &dest.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if let Some(p) = dest.parent() {
        fs::create_dir_all(p)?;
    }
    fs::copy(src, dest).map(|_| ())
}

impl PanelIo for LivePanelIo {
    fn run(&self, argv: &[String]) -> (bool, String) {
        let spec = crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(30),
            stderr_to_stdout: true,
            ..Default::default()
        };
        let r = crate::utils::run(&spec, std::env::var("PATH").ok().as_deref());
        let mut out = String::from_utf8_lossy(&r.stdout).into_owned();
        if out.trim().is_empty() {
            out = String::from_utf8_lossy(&r.stderr).into_owned();
        }
        if out.trim().is_empty() && !r.error.is_empty() {
            out = r.error.clone();
        }
        (r.returncode == Some(0), out)
    }
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(path).ok()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        if let Some(p) = Path::new(path).parent() {
            if fs::create_dir_all(p).is_err() {
                return false;
            }
        }
        fs::write(path, text).is_ok()
    }
    fn exists(&self, path: &str) -> bool {
        fs::symlink_metadata(path).is_ok()
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }
    fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String> {
        copy_tree_keep_links(Path::new(src), Path::new(dest)).map_err(|e| format!("{src}: {e}"))
    }
    fn remove_tree(&self, path: &str) -> Result<(), String> {
        let p = Path::new(path);
        let res = match fs::symlink_metadata(p) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => fs::remove_dir_all(p),
            Ok(_) => fs::remove_file(p),
        };
        res.map_err(|e| format!("{path}: {e}"))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        fs::rename(from, to).map_err(|e| format!("{to}: {e}"))
    }
    fn spawn(&self, argv: &[String]) -> bool {
        LiveShellOps.spawn(&Cmd { argv: argv.to_vec(), timeout_secs: None, lang_c: false })
    }
    fn reload_signal(&self, exe: &str) -> usize {
        let mut sent = 0;
        for pid in session_pids_named(exe) {
            let Ok(pid) = libc::pid_t::try_from(pid) else { continue };
            // SAFETY: kill with a specific positive PID is an ordinary call.
            if unsafe { libc::kill(pid, libc::SIGUSR2) } == 0 {
                sent += 1;
            }
        }
        sent
    }
    fn gnome_shell_list(&self) -> Option<Vec<crate::gnome_shell_ext::ExtInfo>> {
        crate::gnome_ext_section::live_snapshot()
    }
    fn data_home(&self, home: &str) -> String {
        crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), home)
    }
    fn config_home(&self, home: &str) -> String {
        match std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty()) {
            Some(dir) => dir.trim_end_matches('/').to_string(),
            None => format!("{}/.config", home.trim_end_matches('/')),
        }
    }
    fn restart_plasma_shell(&self) -> Option<String> {
        Some(plasma_restart_message(plasma_restart_shell()).trim_start_matches('\n').to_string())
    }
    fn mtime_stamp(&self, path: &str) -> Option<String> {
        let secs = fs::metadata(path).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        crate::qt_app::Timestamp::from_epoch_local(secs).map(|t| t.file_stamp())
    }
    fn pause(&self) {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// The live desktop probe for [`panel_copy_spec`].
pub fn live_panel_copy_spec(de: &str) -> Option<PanelCopySpec> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = std::env::var("PATH").ok();
    let have = |tool: &str| crate::utils::which(tool, path.as_deref()).is_some();
    let home_exists = |rel: &str| Path::new(&format!("{}/{rel}", home.trim_end_matches('/'))).exists();
    let gnome_extensions = if de == "gnome" {
        let (ok, out) = LivePanelIo.run(&sv(&["gsettings", "get", "org.gnome.shell", "enabled-extensions"]));
        if ok { crate::de_apply::parse_gsettings_list(&out) } else { Vec::new() }
    } else {
        Vec::new()
    };
    let system_exists = |p: &str| Path::new(p).is_file();
    panel_copy_spec(
        de,
        &PanelProbe { have: &have, home_exists: &home_exists, gnome_extensions: &gnome_extensions, system_exists: &system_exists },
    )
}

// ============================================================================
// Panel protection on every desktop: many copies per desktop
// ============================================================================
//
// One model for all desktops (Wehrwolfmann's decision of 25.11.2026). Every
// copy is a folder `<root>/<desktop>/<stamp>/` with the same `manifest.json`
// that [`panel_copy_save`] writes, and one inventory, `<root>/panel_meta.json`
// (`{"version": 2, "copies": [...]}`), names them all. A desktop keeps up to
// [`MAX_PANEL_COPIES`] copies, the oldest go first. The very first copy of a
// desktop is its "original" (the panel as it was before Rexon touched it): it
// is kept apart and rotation never removes it.
//
// Copies of earlier releases (the Plasma snapshots, the single MATE, XFCE and
// `panel-<desktop>` copies) are not moved: the inventory refers to them where
// they lie (see the migration below). Such "linked" entries are never deleted
// by rotation: other parts of Rexon still read those files.
//
// The inventory is read raw, like the Plasma meta file above: an entry written
// by a newer release with fields we do not know must survive our rewrite.

/// Name of the panel copies inventory in the copies root.
pub const PANEL_META_FILE: &str = "panel_meta.json";
/// Version of the inventory format.
pub const PANEL_META_VERSION: u64 = 2;
/// How many copies of one desktop are kept, not counting the original one.
pub const MAX_PANEL_COPIES: usize = 10;

/// How the files of a copy are laid out (`PanelSnapshot::format`).
pub mod snapshot_format {
    /// A [`super::panel_copy_save`] folder with `manifest.json`.
    pub const MANIFEST: &str = "manifest";
    /// A Plasma snapshot of earlier releases: files by basename, the original
    /// paths are in `files`.
    pub const PLASMA_FILES: &str = "plasma_files";
    /// One `dconf dump /org/mate/panel/` file (the MATE layout section's copy).
    pub const MATE_DCONF: &str = "mate_dconf";
    /// An `xfce4-panel-profiles` archive of earlier releases.
    pub const XFCE_PROFILE: &str = "xfce_profile";
}

/// What a copy holds, for its caption (`PanelSnapshot::parts`).
pub mod snapshot_part {
    /// Panels, applets, docks, panel extensions.
    pub const PANEL: &str = "panel";
    /// Keyboard shortcuts.
    pub const SHORTCUTS: &str = "shortcuts";
}

fn default_snapshot_format() -> String {
    snapshot_format::MANIFEST.to_string()
}

/// One entry of the panel copies inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelSnapshot {
    /// Rexon desktop key (`kde`, `mate`, `cinnamon-mint`…).
    pub de: String,
    /// `YYYYMMDD_HHMMSS`, local time.
    pub timestamp: String,
    /// Why it was taken: `manual`, `before_install`, `before_switch`,
    /// `before_layout`, `migrated`.
    pub reason: String,
    /// The copy folder (or file, for the linked single-file copies).
    pub path: String,
    /// [`snapshot_part`] names.
    #[serde(default)]
    pub parts: Vec<String>,
    /// How many items the copy holds (manifest entries or files).
    #[serde(default)]
    pub items: usize,
    /// The desktop's original copy: never removed by rotation.
    #[serde(default)]
    pub original: bool,
    /// [`snapshot_format`].
    #[serde(default = "default_snapshot_format")]
    pub format: String,
    /// [`snapshot_format::PLASMA_FILES`]: the original paths of the files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// A copy of an earlier release that lies where that release put it:
    /// rotation neither counts nor deletes it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linked: bool,
}

/// The copies root: `<data home>/rexon/backups/panel`.
pub fn panel_root(data_home: &str) -> String {
    format!("{}/rexon/backups/panel", data_home.trim_end_matches('/'))
}

/// One key per desktop in the inventory: Cinnamon is known to Rexon under two
/// names, and its copies must not split into two lists.
pub fn panel_de_key(de: &str) -> &str {
    match de {
        "cinnamon" => "cinnamon-mint",
        other => other,
    }
}

/// Inventory entries IN WRITE ORDER and AS IS (raw JSON); no file or a broken
/// one means none.
pub fn panel_meta_raw(io: &dyn PanelIo, root: &str) -> Vec<serde_json::Value> {
    let Some(text) = io.read(&format!("{root}/{PANEL_META_FILE}")) else {
        return Vec::new();
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(v) => match v.get("copies") {
            Some(serde_json::Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        },
        Err(e) => {
            crate::log_warn!("backup", "panel copies inventory is unreadable, treated as empty: {e}");
            Vec::new()
        }
    }
}

/// Write the inventory as a whole: next to it first, then in its place, so a
/// failed write never leaves half a list.
pub fn panel_meta_store(io: &dyn PanelIo, root: &str, raw: &[serde_json::Value]) -> Result<(), String> {
    let body = serde_json::json!({ "version": PANEL_META_VERSION, "copies": raw });
    let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    let path = format!("{root}/{PANEL_META_FILE}");
    let tmp = format!("{path}.tmp");
    if !io.write(&tmp, &text) {
        return Err(tmp);
    }
    io.rename(&tmp, &path)
}

/// Typed entries; an entry of an unknown form is skipped, not fatal.
pub fn panel_snapshots(raw: &[serde_json::Value]) -> Vec<PanelSnapshot> {
    raw.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect()
}

/// Copies of desktop `de`, newest first (the original among them, in its place by time).
pub fn panel_copies_of(list: &[PanelSnapshot], de: &str) -> Vec<PanelSnapshot> {
    let de = panel_de_key(de);
    let mut out: Vec<PanelSnapshot> = list.iter().filter(|s| s.de == de).cloned().collect();
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    out
}

/// Desktops that have copies, in first-seen order.
pub fn panel_desktops_with_copies(list: &[PanelSnapshot]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in list {
        if !out.contains(&s.de) {
            out.push(s.de.clone());
        }
    }
    out
}

/// Copies of `de` that rotation removes: Rexon's own (not linked), not the
/// original, the oldest beyond [`MAX_PANEL_COPIES`]. Paths.
pub fn panel_rotation(list: &[PanelSnapshot], de: &str) -> Vec<String> {
    let de = panel_de_key(de);
    let mut own: Vec<&PanelSnapshot> = list.iter().filter(|s| s.de == de && !s.original && !s.linked).collect();
    own.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let extra = own.len().saturating_sub(MAX_PANEL_COPIES);
    own.into_iter().take(extra).map(|s| s.path.clone()).collect()
}

/// The entry with path `path` (the copy's id on the page and in questions).
pub fn panel_snapshot_by_path(list: &[PanelSnapshot], path: &str) -> Option<PanelSnapshot> {
    list.iter().find(|s| s.path == path).cloned()
}

/// What spec `spec` takes, as [`snapshot_part`] names.
pub fn panel_spec_parts(spec: &PanelCopySpec) -> Vec<String> {
    let mut out = Vec::new();
    if !spec.parts.is_empty() {
        out.push(snapshot_part::PANEL.to_string());
    }
    if !spec.shortcuts.is_empty() {
        out.push(snapshot_part::SHORTCUTS.to_string());
    }
    out
}

/// Take a copy of desktop `de` by `spec` into the copies root and name it in
/// the inventory. `stamp` comes from outside (the clock is an observation).
///
/// The first copy of a desktop becomes its original. After the new entry the
/// oldest copies beyond the limit are removed, from disk and from the list.
pub fn panel_snapshot_save(
    io: &dyn PanelIo,
    spec: &PanelCopySpec,
    de: &str,
    home: &str,
    root: &str,
    reason: &str,
    stamp: &str,
) -> Result<PanelSnapshot, String> {
    let de = panel_de_key(de);
    let mut raw = panel_meta_raw(io, root);
    let list = panel_snapshots(&raw);
    // Two copies within one second (an automatic one right after a manual one)
    // get their own folders.
    let mut dir = format!("{root}/{de}/{stamp}");
    let mut n = 2;
    while io.exists(&dir) || list.iter().any(|s| s.path == dir) {
        dir = format!("{root}/{de}/{stamp}-{n}");
        n += 1;
    }
    panel_copy_save(io, spec, home, &dir)?;
    let items = io
        .read(&format!("{dir}/manifest.json"))
        .and_then(|t| serde_json::from_str::<Vec<serde_json::Value>>(&t).ok())
        .map_or(0, |v| v.len());
    let entry = PanelSnapshot {
        de: de.to_string(),
        timestamp: stamp.to_string(),
        reason: reason.to_string(),
        path: dir.clone(),
        parts: panel_spec_parts(spec),
        items,
        original: !list.iter().any(|s| s.de == de && s.original),
        format: snapshot_format::MANIFEST.to_string(),
        files: Vec::new(),
        linked: false,
    };
    raw.push(serde_json::to_value(&entry).map_err(|e| e.to_string())?);
    let dropped = panel_rotation(&panel_snapshots(&raw), de);
    raw.retain(|v| {
        let path = v.get("path").and_then(serde_json::Value::as_str).unwrap_or_default();
        !dropped.iter().any(|d| d == path)
    });
    if let Err(e) = panel_meta_store(io, root, &raw) {
        // A copy nobody can find is dead weight: take it back.
        let _ = io.remove_tree(&dir);
        return Err(e);
    }
    for path in &dropped {
        if let Err(e) = io.remove_tree(path) {
            crate::log_warn!("backup", "old panel copy {path} was not removed: {e}");
        }
    }
    Ok(entry)
}

// ── Copies of earlier releases in the inventory ───────────────────────────
//
// Nothing is moved: every old copy is referred to where it lies. The Plasma
// snapshots keep their folders (the old `plasma_meta.json` points at them and
// stays readable for an older Rexon; a `.migrated` mark is put next to it);
// the single MATE copy and its `.last` neighbour are the files the MATE layout
// section still reads and writes; the XFCE archive and the `panel-<desktop>`
// folders are the last copies of the previous release. One idempotent
// function brings the inventory in line with them, on every read: a second
// run changes nothing.

/// The mark next to the old Plasma meta file once its entries are in the inventory.
pub fn plasma_meta_migrated_mark(plasma_dir: &str) -> String {
    format!("{}.migrated", plasma_meta_path(plasma_dir))
}

const MISSING_STAMP: &str = "00000000_000000";

/// Bring the inventory in line with the copies of earlier releases under
/// `data_home` -> whether it changed (and was written).
pub fn panel_migrate(io: &dyn PanelIo, data_home: &str) -> Result<bool, String> {
    let backups = format!("{}/rexon/backups", data_home.trim_end_matches('/'));
    let root = panel_root(data_home);
    let mut raw = panel_meta_raw(io, &root);
    let before = raw.clone();
    let known = |raw: &[serde_json::Value], path: &str| {
        raw.iter().position(|v| v.get("path").and_then(serde_json::Value::as_str) == Some(path))
    };
    let stamp_of = |path: &str| io.mtime_stamp(path).unwrap_or_else(|| MISSING_STAMP.to_string());

    // 1. Plasma snapshots: Rexon's own copies, so rotation counts them.
    let plasma_dir = format!("{backups}/plasma-panel");
    let plasma_raw: Vec<serde_json::Value> = io
        .read(&plasma_meta_path(&plasma_dir))
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("backups").and_then(serde_json::Value::as_array).cloned())
        .unwrap_or_default();
    let plasma = plasma_parse_entries(&plasma_raw);
    for b in &plasma {
        if known(&raw, &b.path).is_some() || !io.exists(&b.path) {
            continue;
        }
        let shortcuts = b.full || b.files.iter().any(|f| f.ends_with("/kglobalshortcutsrc"));
        let mut parts = vec![snapshot_part::PANEL.to_string()];
        if shortcuts {
            parts.push(snapshot_part::SHORTCUTS.to_string());
        }
        let entry = PanelSnapshot {
            de: "kde".into(),
            timestamp: b.timestamp.clone(),
            reason: b.reason.clone(),
            path: b.path.clone(),
            parts,
            items: b.files.len(),
            original: false,
            format: snapshot_format::PLASMA_FILES.into(),
            files: b.files.clone(),
            linked: false,
        };
        raw.push(serde_json::to_value(&entry).map_err(|e| e.to_string())?);
    }
    if !plasma.is_empty() && !io.exists(&plasma_meta_migrated_mark(&plasma_dir)) {
        io.write(&plasma_meta_migrated_mark(&plasma_dir), "");
    }

    // 2. Linked single copies: `(path, desktop, format, reason, original candidate)`.
    let mate = format!("{backups}/mate-panel.dconf");
    let mate_returned = io
        .read(&panel_original_returned_mark(&mate))
        .is_some_and(|m| m.trim() == PANEL_ORIGINAL_RETURNED);
    let mut linked: Vec<(String, &str, &str, &str, bool)> = vec![
        (mate.clone(), "mate", snapshot_format::MATE_DCONF, "migrated", !mate_returned),
        (panel_last_snapshot_path(&mate), "mate", snapshot_format::MATE_DCONF, "before_layout", false),
        (xfce_panel_copy_path(data_home), "xfce", snapshot_format::XFCE_PROFILE, "migrated", true),
    ];
    let old_dirs: Vec<String> = io.list_dir(&backups).into_iter().filter(|n| n.starts_with("panel-")).collect();
    for name in &old_dirs {
        let de = &name["panel-".len()..];
        if let Some(meta) = crate::system::DE_META.iter().find(|m| m.id == panel_de_key(de) || m.id == de) {
            linked.push((format!("{backups}/{name}"), meta.id, snapshot_format::MANIFEST, "migrated", true));
        }
    }
    for (path, de, format, reason, may_be_original) in linked {
        let probe = if format == snapshot_format::MANIFEST { format!("{path}/manifest.json") } else { path.clone() };
        let at = known(&raw, &path);
        if !io.exists(&probe) {
            // The file is gone (the layout section or the person removed it).
            if let Some(i) = at {
                if raw[i].get("linked").and_then(serde_json::Value::as_bool) == Some(true) {
                    raw.remove(i);
                }
            }
            continue;
        }
        let stamp = stamp_of(&probe);
        let de = panel_de_key(de);
        let other_original = panel_snapshots(&raw).iter().any(|s| s.de == de && s.original && s.path != path);
        let original = may_be_original && !other_original;
        match at {
            Some(i) => {
                // The layout section rewrites these files: the time follows the file.
                if let Some(obj) = raw[i].as_object_mut() {
                    obj.insert("timestamp".into(), stamp.into());
                    obj.insert("original".into(), original.into());
                }
            }
            None => {
                // The single copies of earlier releases held the panel only.
                let parts = vec![snapshot_part::PANEL.to_string()];
                let entry = PanelSnapshot {
                    de: de.to_string(),
                    timestamp: stamp,
                    reason: reason.to_string(),
                    path: path.clone(),
                    parts,
                    items: 1,
                    original,
                    format: format.to_string(),
                    files: Vec::new(),
                    linked: true,
                };
                raw.push(serde_json::to_value(&entry).map_err(|e| e.to_string())?);
            }
        }
    }

    // 3. A desktop without an original: its oldest copy is the closest to how
    // the panel was before Rexon.
    let list = panel_snapshots(&raw);
    for de in panel_desktops_with_copies(&list) {
        if list.iter().any(|s| s.de == de && s.original) {
            continue;
        }
        // Only Rexon's own copies: a linked one says itself whether it is the original.
        let oldest = list.iter().filter(|s| s.de == de && !s.linked).min_by(|a, b| a.timestamp.cmp(&b.timestamp)).map(|s| s.path.clone());
        if let Some(i) = oldest.and_then(|p| known(&raw, &p)) {
            if let Some(obj) = raw[i].as_object_mut() {
                obj.insert("original".into(), true.into());
            }
        }
    }

    if raw == before {
        return Ok(false);
    }
    panel_meta_store(io, &root, &raw)?;
    crate::log_info!("backup", "panel copies inventory brought in line with earlier copies ({} entries)", raw.len());
    Ok(true)
}

// ── Restoring a copy of any format ────────────────────────────────────────

/// The spec a restore checks the manifest against: everything the desktop
/// could have had when the copy was taken, whatever is installed now (a
/// desktop being restored from another one may lack its programs).
pub fn panel_restore_spec(de: &str) -> Option<PanelCopySpec> {
    let yes = |_: &str| true;
    let some_ext = ["any@rexon.app".to_string()];
    panel_copy_spec(
        panel_de_key(de),
        &PanelProbe { have: &yes, home_exists: &yes, gnome_extensions: &some_ext, system_exists: &yes },
    )
}

/// Message: an XFCE copy of an earlier release needs xfce4-panel-profiles.
pub fn msg_xfce_profile_needed(detail: &str) -> String {
    // TRANSLATORS: %1 is what the program said.
    trf(
        "This copy was made by an earlier version of Rexon with xfce4-panel-profiles; install that program to restore it. (%1)",
        &[&detail],
    )
}

/// Restore copy `snap` -> notes for the person (may be empty) or the reason.
///
/// `live`: `snap.de` is the running session's desktop, so it is told to
/// reread what came back; otherwise the desktop reads it at its next login.
pub fn panel_snapshot_restore(io: &dyn PanelIo, home: &str, snap: &PanelSnapshot, live: bool) -> Result<Vec<String>, String> {
    let home = home.trim_end_matches('/');
    match snap.format.as_str() {
        snapshot_format::MANIFEST => {
            let spec = panel_restore_spec(&snap.de).ok_or_else(|| tr("There is no saved panel copy yet."))?;
            let notes = panel_copy_restore_in(io, &spec, home, &snap.path, live)?;
            Ok(notes)
        }
        snapshot_format::PLASMA_FILES => {
            if !io.exists(&snap.path) {
                return Err(msg_snapshot_dir_missing(&snap.path));
            }
            let entry = PlasmaBackup {
                timestamp: snap.timestamp.clone(),
                reason: snap.reason.clone(),
                path: snap.path.clone(),
                files: snap.files.clone(),
                full: false,
            };
            let items = plasma_restore_items(&entry, |p| io.exists(p));
            if items.is_empty() {
                return Err(msg_none_restored());
            }
            let mut notes = Vec::new();
            for (src, dest) in &items {
                if dest.ends_with(&format!("/{KDE_SHORTCUTS_FILE}")) {
                    let text = io.read(src).ok_or_else(|| src.clone())?;
                    let config = &dest[..dest.len() - KDE_SHORTCUTS_FILE.len() - 1];
                    notes.extend(kde_shortcuts_restore(io, config, &text, live)?);
                } else if dest.ends_with("/plasma") && dest.contains("/share/") {
                    // Old full copies took the whole `~/.local/share/plasma`,
                    // Rexon's wallpaper plugin included: the current plugin stays.
                    restore_tree_keeping(io, src, dest, PLASMA_DATA_KEEP)?;
                } else {
                    io.remove_tree(dest)?;
                    io.copy_tree(src, dest)?;
                }
            }
            if live {
                notes.extend(io.restart_plasma_shell());
            }
            Ok(notes)
        }
        snapshot_format::MATE_DCONF => {
            if !io.exists(&snap.path) {
                return Err(tr("⚠️ There is no saved panel to restore."));
            }
            mate_panel_load(io, &snap.path, live)?;
            // The layout section's original came back: its next layout takes a
            // fresh original (the rule of 20.10.2026, `panel_snapshot_slot`).
            if snap.original && snap.linked && !snap.path.ends_with(".last") {
                io.write(&panel_original_returned_mark(&snap.path), PANEL_ORIGINAL_RETURNED);
            }
            Ok(Vec::new())
        }
        snapshot_format::XFCE_PROFILE => {
            let (ok, out) = io.run(&xfce_panel_load_argv(&snap.path));
            if ok {
                Ok(Vec::new())
            } else {
                Err(msg_xfce_profile_needed(out.trim()))
            }
        }
        other => Err(format!("unknown copy format “{other}”")),
    }
}

// ── The live machine ──────────────────────────────────────────────────────

fn live_home_and_data() -> (String, String) {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = LivePanelIo.data_home(&home);
    (home, data)
}

/// The desktop of the running session (a Rexon key), if Rexon knows it.
pub fn live_session_de() -> Option<String> {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    crate::system::detect_current_de(&xdg, &session, wayland)
        .filter(|de| !de.is_empty())
        .map(|de| panel_de_key(&de).to_string())
}

/// All panel copies on this machine (earlier releases' ones included).
pub fn panel_inventory_live() -> Vec<PanelSnapshot> {
    let (_, data) = live_home_and_data();
    if let Err(e) = panel_migrate(&LivePanelIo, &data) {
        crate::log_warn!("backup", "panel copies inventory was not brought in line: {e}");
    }
    panel_snapshots(&panel_meta_raw(&LivePanelIo, &panel_root(&data)))
}

/// Whether Rexon can take a panel copy of desktop `de` on this machine.
pub fn panel_supported_live(de: &str) -> bool {
    live_panel_copy_spec(panel_de_key(de)).is_some()
}

/// The current local time as a copy stamp.
pub fn panel_stamp_now() -> String {
    crate::qt_app::Timestamp::now_local().unwrap_or_default().file_stamp()
}

/// Take a copy of desktop `de` now, with reason `reason`.
pub fn panel_save_live(de: &str, reason: &str) -> Result<PanelSnapshot, String> {
    let de = panel_de_key(de);
    let Some(spec) = live_panel_copy_spec(de) else {
        return Err(msg_nothing_to_backup());
    };
    let (home, data) = live_home_and_data();
    if let Err(e) = panel_migrate(&LivePanelIo, &data) {
        crate::log_warn!("backup", "panel copies inventory was not brought in line: {e}");
    }
    let snap = panel_snapshot_save(&LivePanelIo, &spec, de, &home, &panel_root(&data), reason, &panel_stamp_now())?;
    crate::log_info!("backup", "panel copy of {de} taken ({reason}): {}", snap.path);
    Ok(snap)
}

/// Restore the copy at `path` (its id in the list) -> notes or the reason.
pub fn panel_restore_live(path: &str) -> Result<(PanelSnapshot, Vec<String>), String> {
    let Some(snap) = panel_snapshot_by_path(&panel_inventory_live(), path) else {
        return Err(msg_no_plasma_backups());
    };
    let (home, _) = live_home_and_data();
    let live = live_session_de().as_deref() == Some(snap.de.as_str());
    crate::log_info!("backup", "restoring panel copy {} of {} (running session: {live})", snap.path, snap.de);
    let notes = panel_snapshot_restore(&LivePanelIo, &home, &snap, live)?;
    Ok((snap, notes))
}

/// The newest copy of desktop `de`.
pub fn panel_latest_live(de: &str) -> Option<PanelSnapshot> {
    panel_copies_of(&panel_inventory_live(), de).into_iter().next()
}

/// Processes of our own session (uid and `XDG_SESSION_ID`) with executable
/// `name`, by PID, not by name in `pkill`.
fn session_pids_named(name: &str) -> Vec<u32> {
    use std::os::unix::fs::MetadataExt;
    let my_session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    if my_session.is_empty() {
        return Vec::new();
    }
    // SAFETY: getuid has no preconditions.
    let my_uid = unsafe { libc::getuid() };
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            let base = format!("/proc/{pid}");
            let (Ok(meta), Ok(exe), Ok(environ)) =
                (fs::metadata(&base), fs::read_link(format!("{base}/exe")), fs::read(format!("{base}/environ")))
            else {
                return false;
            };
            is_session_process(&exe.to_string_lossy(), name, meta.uid(), &environ, my_uid, &my_session)
        })
        .collect()
}

#[cfg(test)]
mod original_panel_tests {
    use super::*;

    #[test]
    fn original_copy_is_kept_until_it_is_returned() {
        // No copy: the original is taken.
        assert_eq!(panel_snapshot_slot(false, None), PanelSnapshotSlot::Original);
        // The original is in place (including one taken by an earlier release,
        // without a mark): the next snapshots go next to it.
        assert_eq!(panel_snapshot_slot(true, None), PanelSnapshotSlot::Last);
        assert_eq!(panel_snapshot_slot(true, Some("")), PanelSnapshotSlot::Last);
        // The original was restored: the next intervention takes it again.
        assert_eq!(panel_snapshot_slot(true, Some("returned\n")), PanelSnapshotSlot::Original);
        let copy = "/h/.local/share/rexon/backups/mate-panel.dconf";
        assert_eq!(panel_original_returned_mark(copy), format!("{copy}.returned"));
        assert_eq!(panel_last_snapshot_path(copy), format!("{copy}.last"));
    }
}

#[cfg(test)]
mod xfce_panel_tests {
    use super::*;

    #[test]
    fn xfce_panel_copy_goes_through_panel_profiles() {
        let path = xfce_panel_copy_path("/home/w/.local/share/");
        assert_eq!(path, "/home/w/.local/share/rexon/backups/xfce4-panel.tar.bz2");
        assert_eq!(xfce_panel_save_argv(&path), ["xfce4-panel-profiles", "save", path.as_str()]);
        assert_eq!(xfce_panel_load_argv(&path), ["xfce4-panel-profiles", "load", path.as_str()]);
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;

    /// Its own directory in a temporary folder (tempfile is not among the
    /// core's dependencies; the same trick as in `appearance_cache.rs`).
    fn scratch(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("rexon-plasma-{tag}-{nanos}"));
        fs::create_dir_all(&path).expect("временный каталог обязан создаться");
        path.to_string_lossy().into_owned()
    }

    /// A fake "desktop": a config directory with two files and one tree, like
    /// `~/.local/share/plasma` for a real panel.
    fn fake_desktop(root: &str) -> Vec<String> {
        let cfg = format!("{root}/config");
        fs::create_dir_all(format!("{cfg}/plasma/look")).unwrap();
        fs::write(format!("{cfg}/appletsrc"), "панель=слева\n").unwrap();
        fs::write(format!("{cfg}/plasmashellrc"), "тема=breeze\n").unwrap();
        fs::write(format!("{cfg}/plasma/look/theme"), "обои\n").unwrap();
        vec![
            format!("{cfg}/appletsrc"),
            format!("{cfg}/plasmashellrc"),
            format!("{cfg}/plasma"),
            // A config known to be missing: it must not be in the snapshot.
            format!("{cfg}/нет-такого-файла"),
        ]
    }

    /// The snapshot must land on disk IN FULL, tree included, and the meta
    /// file must name exactly what was copied. Without this "Restore" would
    /// return half a panel, and the person would not understand why.
    #[test]
    fn снимок_копирует_файлы_и_дерево() {
        let root = scratch("snap");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");

        let (ok, msg) = plasma_backup_run(&dir, &configs, "manual", false, "20260908_120000")
            .expect("подставной каталог обязан записаться");
        assert!(ok, "снимок не снялся: {msg}");

        let snap = format!("{dir}/20260908_120000");
        assert_eq!(
            fs::read_to_string(format!("{snap}/appletsrc")).unwrap(),
            "панель=слева\n"
        );
        assert_eq!(
            fs::read_to_string(format!("{snap}/plasma/look/theme")).unwrap(),
            "обои\n",
            "вложенное дерево конфигов в снимок не попало"
        );
        assert!(
            !Path::new(&format!("{snap}/нет-такого-файла")).exists(),
            "в снимок попал конфиг, которого на диске нет"
        );

        let entries = plasma_parse_entries(&plasma_meta_entries(&dir));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].files.len(), 3, "мета-файл врёт о составе снимка");
        assert_eq!(entries[0].reason, "manual");

        let _ = fs::remove_dir_all(&root);
    }

    /// Nothing to copy: no empty directory that looks like a broken copy must
    /// remain on disk, and the person must be told in words.
    #[test]
    fn пустой_снимок_не_остаётся_на_диске() {
        let root = scratch("empty");
        let dir = format!("{root}/backups");
        let configs = vec![format!("{root}/ничего-нет")];

        let (ok, msg) = plasma_backup_run(&dir, &configs, "manual", false, "20260908_130000")
            .expect("каталог копий обязан создаться");
        assert!(!ok);
        assert_eq!(msg, msg_nothing_to_backup());
        assert!(
            !Path::new(&format!("{dir}/20260908_130000")).exists(),
            "пустой каталог снимка остался на диске"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// ⚠️ An entry of an UNKNOWN form (a newer Rexon version would leave one)
    /// must survive our write of the meta file. Losing it means losing the
    /// reference to someone else's snapshot: the snapshot itself would remain
    /// on disk as dead weight, and it would not be in the list.
    #[test]
    fn чужая_запись_переживает_новую_копию() {
        let root = scratch("alien");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            plasma_meta_path(&dir),
            r#"{"backups":[{"timestamp":"20250101_000000","reason":"manual",
                "path":"/tmp/чужой","files":[],"full":false,"новое_поле":42}]}"#,
        )
        .unwrap();

        plasma_backup_run(&dir, &configs, "manual", false, "20260908_140000").unwrap();

        let raw = plasma_meta_entries(&dir);
        assert_eq!(raw.len(), 2, "чужая запись пропала из мета-файла");
        assert_eq!(
            raw[0].get("новое_поле").and_then(|v| v.as_i64()),
            Some(42),
            "незнакомый ключ чужой записи потерян"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// Rotation: no more than the limit of snapshots accumulates, and the
    /// extra directories are really deleted from disk; otherwise panel copies
    /// would quietly eat up space, and nobody would know.
    #[test]
    fn ротация_удаляет_старые_каталоги() {
        let root = scratch("rotate");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");

        for i in 0..(MAX_PLASMA_SNAPSHOTS + 2) {
            let stamp = format!("20260908_1500{i:02}");
            plasma_backup_run(&dir, &configs, "manual", false, &stamp).unwrap();
        }

        let entries = plasma_parse_entries(&plasma_meta_entries(&dir));
        assert_eq!(entries.len(), MAX_PLASMA_SNAPSHOTS);
        assert_eq!(entries[0].timestamp, "20260908_150002", "выброшены не самые старые");
        assert!(
            !Path::new(&format!("{dir}/20260908_150000")).exists(),
            "каталог выброшенного снимка остался на диске"
        );
        assert!(Path::new(&format!("{dir}/20260908_150002")).is_dir());

        let _ = fs::remove_dir_all(&root);
    }

    /// Restoring a copy: the files must return to their previous content, and
    /// the timestamp must get into the answer (by it the person learns what
    /// they restored).
    #[test]
    fn возврат_копии_восстанавливает_файлы() {
        let root = scratch("restore");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_160000").unwrap();

        // The panel was "broken" after the copy was taken.
        fs::write(format!("{root}/config/appletsrc"), "панель=пусто\n").unwrap();

        let (ok, msg) = plasma_restore_run(&dir, Some("20260908_160000"));
        assert!(ok, "возврат не удался: {msg}");
        // The timestamp gets into the answer in HUMAN form, the same as the
        // row of the copies list in the window is labelled (rule #13, 18.09).
        assert!(
            msg.contains(&crate::qt_settings::pretty_backup_ts("20260908_160000"))
                && msg.contains("08.09.2026"),
            "в ответе нет метки копии: {msg}"
        );
        assert_eq!(
            fs::read_to_string(format!("{root}/config/appletsrc")).unwrap(),
            "панель=слева\n",
            "файл не вернулся к сохранённому виду"
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// An empty timestamp = "the latest copy", as in the original
    /// (`backups[-1]`), and a foreign timestamp is a refusal in words, not
    /// silence.
    #[test]
    fn выбор_копии_по_метке_и_без_неё() {
        let root = scratch("pick");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_170000").unwrap();
        fs::write(format!("{root}/config/appletsrc"), "панель=вторая\n").unwrap();
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_170500").unwrap();
        fs::write(format!("{root}/config/appletsrc"), "панель=сломана\n").unwrap();

        let (ok, _) = plasma_restore_run(&dir, None);
        assert!(ok);
        assert_eq!(
            fs::read_to_string(format!("{root}/config/appletsrc")).unwrap(),
            "панель=вторая\n",
            "без метки вернулась не последняя копия"
        );

        let (ok, msg) = plasma_restore_run(&dir, Some("нет-такой-метки"));
        assert!(!ok);
        assert_eq!(msg, msg_snapshot_not_found("нет-такой-метки"));

        // No copies at all: also in words.
        let empty = format!("{root}/пусто");
        let (ok, msg) = plasma_restore_run(&empty, None);
        assert!(!ok);
        assert_eq!(msg, msg_no_plasma_backups());

        let _ = fs::remove_dir_all(&root);
    }

    /// The snapshot directory has disappeared from disk (the person cleaned
    /// `~/.local/share`): say so rather than try to restore from nothing.
    #[test]
    fn исчезнувший_каталог_снимка_назван() {
        let root = scratch("gone");
        let configs = fake_desktop(&root);
        let dir = format!("{root}/backups");
        plasma_backup_run(&dir, &configs, "manual", false, "20260908_180000").unwrap();
        fs::remove_dir_all(format!("{dir}/20260908_180000")).unwrap();

        let (ok, msg) = plasma_restore_run(&dir, Some("20260908_180000"));
        assert!(!ok);
        assert!(msg.contains("20260908_180000"), "в отказе нет пути снимка: {msg}");

        let _ = fs::remove_dir_all(&root);
    }

    /// ⚠️ The order of the shell restart. The systemd unit brings up the panel
    /// ITSELF: a second start after a successful unit would give the person
    /// TWO panels. And if there is no unit, both fallback steps must run,
    /// otherwise the panel stays down. The live panel is not touched: the
    /// launchers are fake.
    #[test]
    fn перезапуск_оболочки_не_поднимает_панель_дважды() {
        use std::cell::RefCell;

        /// Fake hands: what "exists" in the system, what "succeeds", and the log.
        struct Fake {
            unit_ok: bool,
            tools: &'static [&'static str],
            quit_ok: bool,
            pids: Vec<u32>,
            log: RefCell<Vec<String>>,
        }
        impl ShellOps for Fake {
            fn run(&self, cmd: &Cmd) -> bool {
                self.log.borrow_mut().push(cmd.argv.join(" "));
                if cmd.argv[0] == "systemctl" {
                    self.unit_ok
                } else {
                    self.quit_ok
                }
            }
            fn spawn(&self, cmd: &Cmd) -> bool {
                self.log.borrow_mut().push(format!("spawn: {}", cmd.argv.join(" ")));
                true
            }
            fn has(&self, tool: &str) -> bool {
                self.tools.contains(&tool)
            }
            fn session_shell_pids(&self) -> Vec<u32> {
                self.pids.clone()
            }
            fn terminate(&self, pid: u32) -> bool {
                self.log.borrow_mut().push(format!("term: {pid}"));
                true
            }
            fn settle(&self) {}
        }
        let fake = |unit_ok, tools, quit_ok, pids: Vec<u32>| Fake {
            unit_ok,
            tools,
            quit_ok,
            pids,
            log: RefCell::new(Vec::new()),
        };
        let both: &[&str] = &["kquitapp6", "kstart"];

        // The unit did its job: nothing else; a second start would give two panels.
        let f = fake(true, both, true, vec![7]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::Unit);
        assert_eq!(f.log.borrow().len(), 1, "после юнита сделано лишнее: {:?}", f.log.borrow());

        // No unit: kquitapp stops it, kstart brings it up detached.
        let f = fake(false, both, true, vec![7]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::QuitAndStart);
        assert_eq!(
            *f.log.borrow(),
            [
                "systemctl --user restart plasma-plasmashell.service",
                "kquitapp6 plasmashell",
                "spawn: kstart plasmashell",
            ]
        );

        // kquitapp did not help: the shell of OUR OWN session by PID, then a start.
        let f = fake(false, both, false, vec![4242]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::SessionPid);
        assert_eq!(f.log.borrow()[2..], ["term: 4242", "spawn: kstart plasmashell"]);

        // Without kstart the kquitapp stop does not start at all (stopping
        // without bringing it up is worse than not touching it), and the start
        // goes directly.
        let f = fake(false, &["kquitapp6"], true, vec![4242]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::SessionPid);
        assert_eq!(f.log.borrow()[1..], ["term: 4242", "spawn: plasmashell"]);

        // No shell of our own was found: NOTHING is touched, and no `killall`.
        let f = fake(false, both, false, vec![]);
        assert_eq!(plasma_restart_shell_with(&f), ShellRestart::NotRestarted);
        assert!(f.log.borrow().iter().all(|s| !s.contains("killall")), "{:?}", f.log.borrow());
        assert!(f.log.borrow().iter().all(|s| !s.starts_with("spawn")), "{:?}", f.log.borrow());
        assert_eq!(
            plasma_restart_message(ShellRestart::NotRestarted),
            msg_plasmashell_not_restarted()
        );
    }

    /// Our own session's shell is recognised by THREE signs at once; any one
    /// alone is not enough to avoid hitting someone else's panel.
    #[test]
    fn оболочка_своего_сеанса_опознаётся_тремя_признаками() {
        let env = b"HOME=/h\0XDG_SESSION_ID=2\0LANG=ru_RU.UTF-8\0";
        assert!(is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, "2"));
        assert!(is_session_shell("/usr/bin/plasmashell (deleted)", 1000, env, 1000, "2"));
        // another person
        assert!(!is_session_shell("/usr/bin/plasmashell", 1001, env, 1000, "2"));
        // another session of the same person
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, "3"));
        // a similar name, not the shell
        assert!(!is_session_shell("/usr/bin/plasmashell-helper", 1000, env, 1000, "2"));
        // XDG_SESSION_ID=22 is not equal to "2"
        let env22 = b"XDG_SESSION_ID=22\0";
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env22, 1000, "2"));
        // we do not know our own session: touch nobody
        assert!(!is_session_shell("/usr/bin/plasmashell", 1000, env, 1000, ""));
    }
}

#[cfg(test)]
mod panel_copy_tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A machine in memory: files (a directory needs no key with a trailing
    /// `/`: a directory exists if a file under it exists), dconf is a
    /// dictionary of branches.
    #[derive(Default)]
    pub(super) struct Mem {
        pub files: RefCell<BTreeMap<String, String>>,
        pub dconf: RefCell<BTreeMap<String, String>>,
        pub runs: RefCell<Vec<Vec<String>>>,
        pub spawned: RefCell<Vec<Vec<String>>>,
        pub signals: RefCell<Vec<String>>,
        /// Deepin DConfig: `application|resource|key` -> the `dde-dconfig get` answer.
        pub dde: RefCell<BTreeMap<String, String>>,
        /// DConfig keys with the default value.
        pub dde_default: RefCell<std::collections::BTreeSet<String>>,
        /// `gsettings get org.gnome.shell <key>` -> the answer.
        pub gsettings: RefCell<BTreeMap<String, String>>,
        /// File change times (`mtime_stamp`).
        pub mtimes: RefCell<BTreeMap<String, String>>,
    }

    impl PanelIo for Mem {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            match argv.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["dconf", "dump", dir] => (true, self.dconf.borrow().get(*dir).cloned().unwrap_or_default()),
                ["gsettings", "get", "org.gnome.shell", key] => {
                    (true, self.gsettings.borrow().get(*key).cloned().unwrap_or_else(|| "@as []".into()))
                }
                ["dconf", "reset", "-f", dir] => {
                    self.dconf.borrow_mut().remove(*dir);
                    (true, String::new())
                }
                ["sh", "-c", _, "sh", dir, file] => {
                    let text = self.files.borrow().get(*file).cloned().unwrap_or_default();
                    self.dconf.borrow_mut().insert(dir.to_string(), text);
                    (true, String::new())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-s", s, "-k", k, "-m", "isDefaultValue"] => {
                    (true, self.dde_default.borrow().contains(&format!("{a}|{r}|{k}@{s}")).to_string())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-s", s, "-k", k] => {
                    (true, self.dde.borrow().get(&format!("{a}|{r}|{k}@{s}")).cloned().unwrap_or_default())
                }
                ["dde-dconfig", "set", "-a", a, "-r", r, "-s", s, "-k", k, "-v", v] => {
                    let id = format!("{a}|{r}|{k}@{s}");
                    self.dde.borrow_mut().insert(id.clone(), format!("\"{v}\""));
                    self.dde_default.borrow_mut().remove(&id);
                    (true, String::new())
                }
                ["dde-dconfig", "reset", "-a", a, "-r", r, "-s", s, "-k", k] => {
                    let id = format!("{a}|{r}|{k}@{s}");
                    self.dde.borrow_mut().insert(id.clone(), "default".into());
                    self.dde_default.borrow_mut().insert(id);
                    (true, String::new())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-k", k, "-m", "isDefaultValue"] => {
                    (true, self.dde_default.borrow().contains(&format!("{a}|{r}|{k}")).to_string())
                }
                ["dde-dconfig", "get", "-a", a, "-r", r, "-k", k] => {
                    (true, self.dde.borrow().get(&format!("{a}|{r}|{k}")).cloned().unwrap_or_default())
                }
                // As `get` prints after `set`: booleans as is, everything else in quotes.
                ["dde-dconfig", "set", "-a", a, "-r", r, "-k", k, "-v", v] => {
                    let id = format!("{a}|{r}|{k}");
                    let shown = if matches!(*v, "true" | "false") { v.to_string() } else { format!("\"{v}\"") };
                    self.dde.borrow_mut().insert(id.clone(), shown);
                    self.dde_default.borrow_mut().remove(&id);
                    (true, String::new())
                }
                ["dde-dconfig", "reset", "-a", a, "-r", r, "-k", k] => {
                    let id = format!("{a}|{r}|{k}");
                    self.dde.borrow_mut().insert(id.clone(), "default".into());
                    self.dde_default.borrow_mut().insert(id);
                    (true, String::new())
                }
                _ => (true, String::new()),
            }
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn exists(&self, path: &str) -> bool {
            let pre = format!("{path}/");
            self.files.borrow().keys().any(|k| k == path || k.starts_with(&pre))
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{}/", path.trim_end_matches('/'));
            let mut out: Vec<String> = self
                .files
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&pre).map(|r| r.split('/').next().unwrap_or("").to_string()))
                .collect();
            out.dedup();
            out
        }
        fn copy_tree(&self, src: &str, dest: &str) -> Result<(), String> {
            let pre = format!("{src}/");
            let items: Vec<(String, String)> = self
                .files
                .borrow()
                .iter()
                .filter_map(|(k, v)| {
                    if k == src {
                        Some((dest.to_string(), v.clone()))
                    } else {
                        k.strip_prefix(&pre).map(|r| (format!("{dest}/{r}"), v.clone()))
                    }
                })
                .collect();
            self.files.borrow_mut().extend(items);
            Ok(())
        }
        fn remove_tree(&self, path: &str) -> Result<(), String> {
            let pre = format!("{path}/");
            self.files.borrow_mut().retain(|k, _| k != path && !k.starts_with(&pre));
            Ok(())
        }
        fn rename(&self, from: &str, to: &str) -> Result<(), String> {
            self.copy_tree(from, to)?;
            self.remove_tree(from)
        }
        fn spawn(&self, argv: &[String]) -> bool {
            self.spawned.borrow_mut().push(argv.to_vec());
            true
        }
        fn reload_signal(&self, exe: &str) -> usize {
            self.signals.borrow_mut().push(exe.to_string());
            1
        }
        fn pause(&self) {}
        fn mtime_stamp(&self, path: &str) -> Option<String> {
            self.mtimes.borrow().get(path).cloned()
        }
    }

    pub(super) fn spec_for(de: &str, tools: &[&str], home_paths: &[&str], exts: &[&str]) -> Option<PanelCopySpec> {
        let tools: Vec<String> = tools.iter().map(|s| s.to_string()).collect();
        let paths: Vec<String> = home_paths.iter().map(|s| s.to_string()).collect();
        let exts: Vec<String> = exts.iter().map(|s| s.to_string()).collect();
        let have = |t: &str| tools.iter().any(|x| x == t);
        let home_exists = |p: &str| paths.iter().any(|x| x == p);
        let system_exists = |p: &str| paths.iter().any(|x| x == p);
        panel_copy_spec(
            de,
            &PanelProbe { have: &have, home_exists: &home_exists, gnome_extensions: &exts, system_exists: &system_exists },
        )
    }

    const HOME: &str = "/home/w";
    const COPY: &str = "/home/w/.local/share/rexon/backups/panel-x";

    #[test]
    fn only_panel_keys_of_a_dconf_root_are_taken() {
        let dump = "[/]\nenabled-applets=['panel1:left:0:menu@cinnamon.org:0']\ngtk-theme='Mint-Y'\npanels-height=['1:40']\n\n[theme]\nname='x'\n";
        assert_eq!(
            dconf_root_keys(dump, &["panels-", "enabled-applets"]),
            "[/]\nenabled-applets=['panel1:left:0:menu@cinnamon.org:0']\npanels-height=['1:40']\n"
        );
        assert_eq!(dconf_root_keys("[sub]\npanels-x=1\n", &["panels-"]), "");
    }

    /// Cinnamon: panels and applets are `org.cinnamon` keys, applet settings
    /// are in `~/.config/cinnamon/spices`; the theme and other things from the
    /// same branch are not touched on restore.
    #[test]
    fn cinnamon_panel_round_trip() {
        let spec = spec_for("cinnamon-mint", &[], &[], &[]).expect("Cinnamon снимается");
        assert_eq!(spec, spec_for("cinnamon", &[], &[], &[]).unwrap());
        let m = Mem::default();
        m.dconf.borrow_mut().insert(
            "/org/cinnamon/".into(),
            "[/]\nenabled-applets=['a']\ngtk-theme='Mint-Y'\npanels-enabled=['1:0:bottom']\n".into(),
        );
        m.write(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json"), "{\"a\":1}");
        assert_eq!(panel_copy_save(&m, &spec, HOME, COPY).as_deref(), Ok(COPY));
        assert!(!m.exists(&format!("{COPY}.new")), "сборочный каталог убран");
        assert_eq!(
            m.read(&format!("{COPY}/dconf-0.ini")).as_deref(),
            Some("[/]\nenabled-applets=['a']\npanels-enabled=['1:0:bottom']\n")
        );
        // The person changed the panel and the applet settings.
        m.write(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json"), "{\"a\":2}");
        m.write(&format!("{HOME}/.config/cinnamon/spices/new@x/0.json"), "{}");
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_ok());
        assert_eq!(m.read(&format!("{HOME}/.config/cinnamon/spices/menu@cinnamon.org/0.json")).as_deref(), Some("{\"a\":1}"));
        assert!(!m.exists(&format!("{HOME}/.config/cinnamon/spices/new@x")));
        // The whole branch was not reset: only the panel keys were written over
        // (the shortcut branch is a separate one and is reset as a whole).
        assert!(!m.runs.borrow().iter().any(|r| r.get(1).map(String::as_str) == Some("reset")
            && r.get(3).map(String::as_str) == Some("/org/cinnamon/")));
    }

    #[test]
    fn restore_without_copy_and_hostile_manifest() {
        let spec = spec_for("cinnamon", &[], &[], &[]).unwrap();
        let m = Mem::default();
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_err(), "копии нет");
        // The manifest is a file on disk: a foreign branch and a path with ".." are not executed.
        m.write(
            &format!("{COPY}/manifest.json"),
            r#"[{"kind":"dconf_dir","dir":"/org/gnome/","file":"x.ini"},{"kind":"home","path":"../../etc"}]"#,
        );
        assert!(panel_copy_restore(&m, &spec, HOME, COPY).is_ok());
        assert!(m.runs.borrow().is_empty(), "{:?}", m.runs.borrow());
    }

    /// Budgie: the whole `com.solus-project.budgie-panel` branch (panels,
    /// applets, their settings); on restore the branch is reset entirely, and
    /// `budgie-panel --replace` builds the panels again.
    #[test]
    fn budgie_panel_round_trip() {
        assert!(spec_for("budgie", &[], &[], &[]).is_none(), "без budgie-panel снимать нечего");
        let spec = spec_for("budgie", &["budgie-panel"], &[], &[]).expect("Budgie снимается");
        let m = Mem::default();
        m.dconf.borrow_mut().insert("/com/solus-project/budgie-panel/".into(), "[/]\npanels=['a']\n".into());
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.dconf.borrow_mut().insert("/com/solus-project/budgie-panel/".into(), "[/]\npanels=['b']\n".into());
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.dconf.borrow()["/com/solus-project/budgie-panel/"], "[/]\npanels=['a']\n");
        assert!(m.runs.borrow().iter().any(|r| r == &sv(&["dconf", "reset", "-f", "/com/solus-project/budgie-panel/"])));
        assert_eq!(*m.spawned.borrow(), [sv(&["budgie-panel", "--replace"])]);
    }

    /// LXQt: the `~/.config/lxqt/panel.conf` file; the panel rereads it when
    /// the LXQt session stops and starts its module again.
    #[test]
    fn lxqt_panel_round_trip() {
        let spec = spec_for("lxqt", &["lxqt-panel"], &[], &[]).expect("LXQt снимается");
        let m = Mem::default();
        let conf = format!("{HOME}/.config/lxqt/panel.conf");
        m.write(&conf, "[panel1]\nposition=Bottom\n");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&conf, "[panel1]\nposition=Top\n");
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.read(&conf).as_deref(), Some("[panel1]\nposition=Bottom\n"));
        let runs = m.runs.borrow();
        let calls: Vec<&String> = runs.iter().filter(|r| r[0] == "dbus-send").map(|r| &r[5]).collect();
        // The panel module, then the shortcuts module.
        assert_eq!(
            calls,
            [
                "org.lxqt.session.stopModule",
                "org.lxqt.session.startModule",
                "org.lxqt.session.stopModule",
                "org.lxqt.session.startModule"
            ]
        );
        assert!(runs.iter().any(|r| r.last().map(String::as_str) == Some("string:lxqt-panel.desktop")));
    }

    /// GNOME: there is no panel, so only the enabled panel extensions (Dash to
    /// Dock, Dash to Panel) and the enabled list are taken.
    #[test]
    fn gnome_copies_all_extensions() {
        assert!(spec_for("gnome", &[], &[], &[]).is_none(), "ни оболочки, ни расширений — снимать нечего");
        let spec = spec_for("gnome", &["gnome-shell"], &[], &[]).expect("GNOME");
        assert_eq!(
            spec.parts,
            vec![
                PanelPart::DconfDir("/org/gnome/shell/extensions/"),
                PanelPart::DconfKeys {
                    dir: "/org/gnome/shell/",
                    prefixes: &["enabled-extensions", "disabled-extensions", "favorite-apps"],
                },
                PanelPart::GnomeExtensions,
            ]
        );
        assert!(spec.shortcuts.contains(&PanelPart::DconfDir(GNOME_WM_KEYS_DIR)));
        assert!(spec_for("gnome", &[], &[], &["appindicatorsupport@rgcjonas.gmail.com"]).is_some());
    }

    /// The inventory without the shell: a user one overrides a system one,
    /// `disabled-extensions` beats `enabled-extensions`, garbage directory
    /// names do not count.
    #[test]
    fn gnome_inventory_from_disk_and_shell() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let inv = gnome_ext_inventory_from_disk(
            &v(&["own@x.org", "both@x.org", "junk"]),
            &v(&["sys@gnome.org", "both@x.org"]),
            &v(&["own@x.org", "sys@gnome.org", "both@x.org"]),
            &v(&["sys@gnome.org"]),
        );
        let row = |u: &str| inv.iter().find(|e| e.uuid == u).cloned().unwrap();
        assert_eq!(inv.len(), 3, "{inv:?}");
        assert!(row("own@x.org").enabled && !row("own@x.org").system);
        assert!(!row("sys@gnome.org").enabled && row("sys@gnome.org").system);
        assert!(!row("both@x.org").system, "своё перекрывает системное");

        use crate::gnome_shell_ext::{ExtInfo, ExtState, Field, RawInfo};
        let mut raw = RawInfo::new();
        raw.insert("uuid".into(), Field::Str("a@x.org".into()));
        raw.insert("name".into(), Field::Str("A".into()));
        raw.insert("state".into(), Field::Num(1.0));
        raw.insert("type".into(), Field::Num(1.0));
        let info = ExtInfo::from_raw(&raw).unwrap();
        assert_eq!(info.state, ExtState::Active);
        assert_eq!(
            gnome_ext_inventory_from_shell(&[info]),
            vec![SavedGnomeExt { uuid: "a@x.org".into(), name: "A".into(), enabled: true, system: true }]
        );
    }

    /// A GNOME copy: the inventory, all extension settings and both lists;
    /// restoring resets the extensions branch and puts back what was taken,
    /// the other shell keys are not touched; the inventory is read back.
    #[test]
    fn gnome_extensions_round_trip() {
        let io = Mem::default();
        io.files.borrow_mut().insert(format!("{HOME}/.local/share/gnome-shell/extensions/own@x.org/metadata.json"), "{}".into());
        io.files.borrow_mut().insert("/usr/share/gnome-shell/extensions/sys@gnome.org/metadata.json".into(), "{}".into());
        io.gsettings.borrow_mut().insert("enabled-extensions".into(), "['own@x.org']".into());
        io.gsettings.borrow_mut().insert("disabled-extensions".into(), "['sys@gnome.org']".into());
        io.dconf.borrow_mut().insert("/org/gnome/shell/extensions/".into(), "[own]\nsize=3\n".into());
        io.dconf.borrow_mut().insert(
            "/org/gnome/shell/".into(),
            "[/]\nenabled-extensions=['own@x.org']\ndisabled-extensions=['sys@gnome.org']\nfavorite-apps=['a.desktop']\n".into(),
        );
        let spec = spec_for("gnome", &["gnome-shell"], &[], &[]).unwrap();
        panel_copy_save(&io, &spec, HOME, COPY).unwrap();
        let saved = gnome_ext_copy_saved(&io, COPY);
        assert_eq!(
            saved,
            vec![
                SavedGnomeExt { uuid: "own@x.org".into(), name: String::new(), enabled: true, system: false },
                SavedGnomeExt { uuid: "sys@gnome.org".into(), name: String::new(), enabled: false, system: true },
            ]
        );
        let keys = io.files.borrow().get(&format!("{COPY}/dconf-1.ini")).cloned().unwrap();
        assert!(keys.contains("enabled-extensions") && keys.contains("disabled-extensions"));
        // The dock's favourites are taken since 25.11.2026 (they are the dock's icons).
        assert!(!keys.contains("gtk-theme"), "{keys}");

        io.dconf.borrow_mut().insert("/org/gnome/shell/extensions/".into(), "[own]\nsize=9\n[other]\nx=1\n".into());
        panel_copy_restore(&io, &spec, HOME, COPY).unwrap();
        assert_eq!(io.dconf.borrow().get("/org/gnome/shell/extensions/").unwrap(), "[own]\nsize=3\n");
        assert!(io.runs.borrow().iter().any(|r| r == &sv(&["dconf", "reset", "-f", "/org/gnome/shell/extensions/"])));

        // A foreign inventory in the manifest is not read from outside the copy.
        io.files.borrow_mut().insert(format!("{COPY}/manifest.json"), r#"[{"kind":"gnome_extensions","file":"../x.json"}]"#.into());
        assert!(gnome_ext_copy_saved(&io, COPY).is_empty());
    }

    /// A copy from earlier releases (a branch of one extension) is restored
    /// with the new layout too: a branch inside one taken whole is ours.
    #[test]
    fn old_gnome_copy_with_single_extension_branch() {
        let io = Mem::default();
        io.files.borrow_mut().insert(
            format!("{COPY}/manifest.json"),
            r#"[{"kind":"dconf_dir","dir":"/org/gnome/shell/extensions/dash-to-dock/","file":"dconf-0.ini"},
                {"kind":"dconf_dir","dir":"/org/gnome/shell/extensions/../../desktop/","file":"dconf-0.ini"}]"#
                .into(),
        );
        io.files.borrow_mut().insert(format!("{COPY}/dconf-0.ini"), "[/]\ndock-fixed=true\n".into());
        let spec = spec_for("gnome", &["gnome-shell"], &[], &[]).unwrap();
        panel_copy_restore(&io, &spec, HOME, COPY).unwrap();
        let dconf = io.dconf.borrow();
        assert_eq!(dconf.get("/org/gnome/shell/extensions/dash-to-dock/").unwrap(), "[/]\ndock-fixed=true\n");
        assert_eq!(dconf.len(), 1, "ветка с `..` не тронута: {dconf:?}");
        assert!(gnome_ext_copy_saved(&io, COPY).is_empty(), "в старой копии описи нет");
    }


    /// Pantheon: the branch of Wingpanel and of the installed dock (Plank or
    /// the new elementary dock).
    #[test]
    fn pantheon_copies_wingpanel_and_dock() {
        assert!(spec_for("pantheon", &[], &[], &[]).is_none());
        let spec = spec_for("pantheon", &["io.elementary.wingpanel", "plank"], &[], &[]).unwrap();
        // Plank dock icons are files in `~/.config/plank/<dock>/launchers`.
        assert_eq!(
            spec.parts,
            [
                PanelPart::DconfDir("/io/elementary/desktop/wingpanel/"),
                PanelPart::DconfDir("/net/launchpad/plank/"),
                PanelPart::Home(".config/plank".into()),
            ]
        );
        let spec = spec_for("pantheon", &["io.elementary.wingpanel", "io.elementary.dock"], &[], &[]).unwrap();
        assert!(spec.parts.contains(&PanelPart::DconfDir("/io/elementary/dock/")));
    }

    /// COSMIC: the `com.system76.CosmicPanel*` directories in `~/.config/cosmic`
    /// (the panel, the dock, their buttons); an extra directory that appeared
    /// after the copy was taken is removed on restore.
    #[test]
    fn cosmic_panel_round_trip() {
        let spec = spec_for("cosmic", &["cosmic-panel"], &[], &[]).expect("COSMIC");
        let m = Mem::default();
        let base = format!("{HOME}/.config/cosmic");
        m.write(&format!("{base}/com.system76.CosmicPanel/v1/entries"), "[\"Panel\"]");
        m.write(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor"), "Bottom");
        m.write(&format!("{base}/com.system76.CosmicTheme.Dark/v1/x"), "keep");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor"), "Left");
        m.write(&format!("{base}/com.system76.CosmicPanel.Extra/v1/x"), "new");
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.read(&format!("{base}/com.system76.CosmicPanel.Dock/v1/anchor")).as_deref(), Some("Bottom"));
        assert!(!m.exists(&format!("{base}/com.system76.CosmicPanel.Extra")));
        assert_eq!(m.read(&format!("{base}/com.system76.CosmicTheme.Dark/v1/x")).as_deref(), Some("keep"));
    }

    /// Tiling compositors: the panel is a bar. The config of whatever is
    /// installed is taken: waybar (rereads on SIGUSR2), Noctalia,
    /// DankMaterialShell.
    #[test]
    fn tiling_bars_are_copied() {
        for de in ["hyprland", "niri", "sway", "river"] {
            assert!(spec_for(de, &[], &[], &[]).is_none(), "{de}: бара нет — кнопок нет");
            let spec = spec_for(de, &["waybar"], &[".config/waybar"], &[]).expect(de);
            assert_eq!(spec.parts, [PanelPart::Home(".config/waybar".into())]);
            assert_eq!(spec.reload, [PanelReload::ReloadSignal("waybar")]);
            let spec = spec_for(de, &[], &[".config/noctalia", ".config/DankMaterialShell"], &[]).unwrap();
            assert_eq!(
                spec.parts,
                [PanelPart::Home(".config/noctalia".into()), PanelPart::Home(".config/DankMaterialShell".into())]
            );
        }
        let m = Mem::default();
        let spec = spec_for("sway", &["waybar"], &[".config/waybar"], &[]).unwrap();
        m.write(&format!("{HOME}/.config/waybar/config.jsonc"), "{}");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(*m.signals.borrow(), ["waybar"]);
    }

    /// Deepin: three DConfig resources of the dde-shell dock, all writable
    /// schema keys; on restore normal keys are written, "default" ones are
    /// reset, read-only keys are not touched, and the dock is restarted by its
    /// service.
    #[test]
    fn deepin_dock_round_trip() {
        let schema = |r: &str| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r);
        let schemas: Vec<String> = DDE_DOCK_RESOURCES.iter().map(|r| schema(r)).collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let spec = spec_for("deepin", &["dde-dconfig", "dde-shell"], &refs, &[]).expect("Deepin V23/V25");
        assert_eq!(spec.reload, [PanelReload::RestartUserUnit("dde-shell@DDE.service")]);
        let m = Mem::default();
        let rw = |v: &str| format!("{{\"value\": {v}, \"permissions\": \"readwrite\"}}");
        m.write(
            &schema("org.deepin.ds.dock"),
            &format!(
                "{{\"contents\": {{\"Dock_Size\": {}, \"Position\": {}, \"Plugins_Visible\": {}, \"Locked\": {}}}}}",
                rw("48"), rw("\"bottom\""), rw("{}"), rw("false")
            ),
        );
        m.write(
            &schema("org.deepin.ds.dock.taskmanager"),
            &format!(
                "{{\"contents\": {{\"dockedElements\": {}, \"cgroupsBasedGrouping\": {{\"value\": true, \"permissions\": \"readonly\"}}}}}}",
                rw("[]")
            ),
        );
        m.write(&schema("org.deepin.ds.dock.tray"), "{\"contents\": {}}");
        let id = |r: &str, k: &str| format!("{DDE_SHELL_DCONFIG_APPID}|{r}|{k}");
        let dock = "org.deepin.ds.dock";
        let tm = "org.deepin.ds.dock.taskmanager";
        m.dde.borrow_mut().extend([
            (id(dock, "Dock_Size"), "\"48\"".to_string()),
            (id(dock, "Position"), "\"bottom\"".to_string()),
            (id(dock, "Plugins_Visible"), "\"{\n    \"datetime\": true\n}\n\"".to_string()),
            (id(dock, "Locked"), "false".to_string()),
            (id(tm, "dockedElements"), "\"[\n    \"desktop/dde-file-manager\"\n]\n\"".to_string()),
        ]);
        m.dde_default.borrow_mut().insert(id(dock, "Locked"));
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        // The person changed the dock after the copy.
        for (k, v) in [("Position", "left"), ("Dock_Size", "70"), ("Locked", "true")] {
            m.run(&crate::dde::dconfig_set_key_argv(DDE_SHELL_DCONFIG_APPID, dock, k, v));
        }
        m.runs.borrow_mut().clear();
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        let runs = m.runs.borrow().clone();
        let set = |r: &str, k: &str, v: &str| crate::dde::dconfig_set_key_argv(DDE_SHELL_DCONFIG_APPID, r, k, v);
        assert!(runs.contains(&set(dock, "Position", "bottom")), "{runs:?}");
        assert!(runs.contains(&set(dock, "Dock_Size", "48")));
        assert!(runs.contains(&set(dock, "Plugins_Visible", "{\"datetime\":true}")), "словарь — компактным JSON");
        assert!(runs.contains(&set(tm, "dockedElements", "[\"desktop/dde-file-manager\"]")));
        assert!(runs.contains(&crate::dde::dconfig_reset_key_argv(DDE_SHELL_DCONFIG_APPID, dock, "Locked")));
        assert!(!runs.iter().any(|a| a.contains(&"cgroupsBasedGrouping".to_string())), "ключ только для чтения не трогаем");
        assert_eq!(runs.last().unwrap(), &sv(&["systemctl", "--user", "--no-block", "try-restart", "dde-shell@DDE.service"]));
        assert!(m.dde_default.borrow().contains(&id(dock, "Locked")));
    }

    /// A dock of another release (no `org.deepin.dde.shell` schemas: dde-shell
    /// before 0.0.35 or the old dde-dock) gets no buttons rather than a copy
    /// of foreign keys.
    #[test]
    fn deepin_dock_of_unknown_version_has_no_copy() {
        assert!(spec_for("deepin", &["dde-dconfig", "dde-shell"], &[], &[]).is_none());
        let old = crate::dde::dconfig_schema_path("org.deepin.ds.dock", "org.deepin.ds.dock");
        assert!(spec_for("deepin", &["dde-dconfig", "dde-shell"], &[&old], &[]).is_none());
        let schemas: Vec<String> =
            DDE_DOCK_RESOURCES.iter().map(|r| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r)).collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        assert!(spec_for("deepin", &["dde-shell"], &refs, &[]).is_none(), "без dde-dconfig писать нечем");
    }

    /// Wayfire: the panel is wf-shell, one file, reread by itself.
    #[test]
    fn wayfire_panel_is_wf_shell_ini() {
        assert!(spec_for("wayfire", &[], &[], &[]).is_none(), "без wf-shell кнопок нет");
        let spec = spec_for("wayfire", &["wf-panel"], &[], &[]).expect("Wayfire");
        assert_eq!(spec.parts, [PanelPart::Home(".config/wf-shell.ini".into())]);
        assert_eq!(spec.reload, [PanelReload::Live]);
    }

    #[test]
    fn unknown_desktop_has_nothing_to_copy() {
        assert!(spec_for("labwc", &[], &[], &[]).is_none());
        assert!(spec_for("kde", &[], &[], &[]).is_none(), "KDE снимается своим путём");
    }
}

#[cfg(test)]
mod gnome_ext_delivery_tests {
    use super::*;
    use crate::gnome_ego::{FetchError, Release, ReleaseShell, ShellVersion};
    use crate::gnome_shell_ext::{BusFailure, CmdOutput, RawInfo, ShellExtIo, INSTALL_SUCCESSFUL};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    /// A fake shell: a call log, without the bus and the network.
    struct Shell {
        running: bool,
        install_reply: String,
        calls: Vec<String>,
    }

    impl ShellExtIo for Shell {
        fn shell_running(&mut self) -> bool {
            self.running
        }
        fn list(&mut self) -> Result<BTreeMap<String, RawInfo>, BusFailure> {
            Ok(BTreeMap::new())
        }
        fn info(&mut self, uuid: &str) -> Result<RawInfo, BusFailure> {
            let mut m = RawInfo::new();
            m.insert("uuid".into(), crate::gnome_shell_ext::Field::Str(uuid.into()));
            m.insert("state".into(), crate::gnome_shell_ext::Field::Num(1.0));
            Ok(m)
        }
        fn errors(&mut self, _uuid: &str) -> Result<Vec<String>, BusFailure> {
            Ok(Vec::new())
        }
        fn install_remote(&mut self, uuid: &str, _t: Duration) -> Result<String, BusFailure> {
            self.calls.push(format!("install {uuid}"));
            Ok(self.install_reply.clone())
        }
        fn uninstall(&mut self, _uuid: &str) -> Result<bool, BusFailure> {
            Ok(false)
        }
        fn enable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("enable {uuid}"));
            Ok(true)
        }
        fn disable(&mut self, uuid: &str) -> Result<bool, BusFailure> {
            self.calls.push(format!("disable {uuid}"));
            Ok(true)
        }
        fn open_prefs(&mut self, _u: &str, _p: &str, _m: bool) -> Result<(), BusFailure> {
            Ok(())
        }
        fn launch_prefs(&mut self, _uuid: &str) -> Result<(), BusFailure> {
            Ok(())
        }
        fn check_for_updates(&mut self) -> Result<(), BusFailure> {
            Ok(())
        }
        fn shell_version(&mut self) -> Result<String, BusFailure> {
            Ok("47.2".into())
        }
        fn user_extensions_enabled(&mut self) -> Result<bool, BusFailure> {
            Ok(true)
        }
        fn set_user_extensions_enabled(&mut self, _on: bool) -> Result<(), BusFailure> {
            Ok(())
        }
        fn shell_setting_bool(&mut self, _key: &str) -> Option<bool> {
            None
        }
        fn which(&mut self, cmd: &str) -> bool {
            cmd == "gnome-shell"
        }
        fn run(&mut self, argv: &[String]) -> Result<CmdOutput, String> {
            self.calls.push(format!("run {}", argv.join(" ")));
            if argv.first().map(String::as_str) == Some("gnome-shell") {
                return Ok(CmdOutput { success: true, text: "GNOME Shell 47.2".into() });
            }
            Ok(CmdOutput { success: true, text: String::new() })
        }
    }

    fn ext(uuid: &str, enabled: bool, system: bool) -> SavedGnomeExt {
        SavedGnomeExt { uuid: uuid.into(), name: String::new(), enabled, system }
    }

    fn rel(version: i64, major: i64) -> Release {
        Release {
            version,
            status: crate::gnome_ego::STATUS_ACTIVE,
            shell_versions: vec![ReleaseShell { major, minor: -1, point: -1 }],
            ..Default::default()
        }
    }

    /// "Site" releases: `gone@` is 404, `old@` only for GNOME 42, `dead@` has
    /// no active release, the others are for 47.
    fn site(uuid: &str) -> Result<Vec<Release>, FetchError> {
        match uuid.split('@').next().unwrap_or("") {
            "gone" => Err(FetchError::Net(crate::net::NetError::Status { code: 404, reason: "Not Found".into() })),
            "down" => Err(FetchError::Net(crate::net::NetError::Transport { detail: "no route".into() })),
            "old" => Ok(vec![rel(3, 42)]),
            "dead" => Ok(vec![Release { status: 1, ..rel(1, 47) }]),
            _ => Ok(vec![rel(5, 46), rel(7, 47)]),
        }
    }

    #[test]
    fn missing_extensions_come_back_through_the_shell() {
        let mut shell = Shell { running: true, install_reply: INSTALL_SUCCESSFUL.into(), calls: Vec::new() };
        let saved = vec![
            ext("here@x.org", true, false),
            ext("own@x.org", true, false),
            ext("off@x.org", false, false),
            ext("gone@x.org", true, false),
            ext("old@x.org", true, false),
            ext("dead@x.org", true, false),
            ext("down@x.org", true, false),
            ext("sys@gnome.org", true, true),
        ];
        let present = vec![ext("here@x.org", false, false)];
        let mut zips = 0;
        let r = gnome_ext_copy_deliver(&mut shell, &saved, &present, "/h", &mut site, &mut |_, _| {
            zips += 1;
            Err("не нужно".into())
        });
        assert_eq!(zips, 0, "при оболочке zip не качаем");
        assert_eq!(r.installed, vec!["own@x.org".to_string(), "off@x.org".to_string()]);
        assert_eq!(
            shell.calls,
            vec!["install own@x.org".to_string(), "install off@x.org".into(), "disable off@x.org".into()],
            "уже стоящее не ставится, выключенное в копии — выключено"
        );
        assert!(r.problems.contains(&msg_gnome_ext_not_on_site("gone@x.org")));
        assert!(r.problems.contains(&msg_gnome_ext_not_on_site("dead@x.org")));
        assert!(r.problems.contains(&msg_gnome_ext_unsupported("old@x.org", "47")));
        assert!(r.problems.contains(&msg_gnome_ext_system_missing("sys@gnome.org")));
        assert!(r.problems.iter().any(|p| p.contains("down@x.org") && p.contains("extensions.gnome.org")), "{:?}", r.problems);
        assert_eq!(r.problems.len(), 5);
        assert!(r.message().contains("own@x.org"));
    }

    #[test]
    fn cancelled_in_shell_window_is_a_note() {
        let mut shell = Shell { running: true, install_reply: "cancelled".into(), calls: Vec::new() };
        let r = gnome_ext_copy_deliver(&mut shell, &[ext("own@x.org", true, false)], &[], "/h", &mut site, &mut |_, _| {
            Err(String::new())
        });
        assert!(r.installed.is_empty() && r.problems.is_empty());
        assert_eq!(r.notes.len(), 1);
    }

    /// Without the shell, the fallback path: a zip for the version from
    /// `gnome-shell --version`; a download failure is named.
    #[test]
    fn without_shell_zip_is_fetched_for_this_gnome() {
        let mut shell = Shell { running: false, install_reply: String::new(), calls: Vec::new() };
        let mut asked: Vec<(String, Option<ShellVersion>)> = Vec::new();
        let r = gnome_ext_copy_deliver(&mut shell, &[ext("own@x.org", true, false)], &[], "/h", &mut site, &mut |u, v| {
            asked.push((u.to_string(), v.cloned()));
            Err::<PathBuf, _>("нет сети".into())
        });
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].1.as_ref().map(|v| v.major), Some(47));
        assert!(!shell.calls.iter().any(|c| c.starts_with("install")));
        assert_eq!(r.problems.len(), 1);
        assert!(r.problems[0].contains("нет сети"), "{:?}", r.problems);
    }

    #[test]
    fn nothing_missing_says_nothing() {
        let mut shell = Shell { running: true, install_reply: String::new(), calls: Vec::new() };
        let all = vec![ext("a@x.org", true, false)];
        let r = gnome_ext_copy_deliver(&mut shell, &all, &all, "/h", &mut |_| panic!("сеть не нужна"), &mut |_, _| {
            panic!("сеть не нужна")
        });
        assert!(r.is_empty());
        assert!(shell.calls.is_empty());
    }
}

#[cfg(test)]
mod panel_model_tests {
    use super::panel_copy_tests::{spec_for, Mem};
    use super::*;

    const HOME: &str = "/home/w";
    const ROOT: &str = "/home/w/.local/share/rexon/backups/panel";

    fn lxqt(m: &Mem) -> PanelCopySpec {
        m.write(&format!("{HOME}/.config/lxqt/panel.conf"), "[panel1]\n");
        spec_for("lxqt", &["lxqt-panel"], &[], &[]).expect("LXQt is taken")
    }

    fn list(m: &Mem) -> Vec<PanelSnapshot> {
        panel_snapshots(&panel_meta_raw(m, ROOT))
    }

    #[test]
    fn first_copy_is_the_original_and_rotation_spares_it() {
        let m = Mem::default();
        let spec = lxqt(&m);
        for i in 0..(MAX_PANEL_COPIES + 3) {
            let stamp = format!("20261125_1200{i:02}");
            let s = panel_snapshot_save(&m, &spec, "lxqt", HOME, ROOT, "manual", &stamp).expect("saved");
            assert_eq!(s.original, i == 0, "copy {i}");
            assert_eq!(s.parts, vec![snapshot_part::PANEL.to_string(), snapshot_part::SHORTCUTS.to_string()]);
            assert_eq!(s.items, 1);
        }
        let all = list(&m);
        assert_eq!(all.len(), MAX_PANEL_COPIES + 1, "ten copies plus the original");
        assert!(all.iter().any(|s| s.original && s.timestamp == "20261125_120000"));
        // The two oldest non-original copies are gone from disk too.
        assert!(!m.exists(&format!("{ROOT}/lxqt/20261125_120001")));
        assert!(!m.exists(&format!("{ROOT}/lxqt/20261125_120002")));
        assert!(m.exists(&format!("{ROOT}/lxqt/20261125_120000/manifest.json")));
        assert!(m.exists(&format!("{ROOT}/lxqt/20261125_120003/manifest.json")));
        // Newest first.
        let mine = panel_copies_of(&all, "lxqt");
        assert_eq!(mine[0].timestamp, format!("20261125_1200{:02}", MAX_PANEL_COPIES + 2));
    }

    #[test]
    fn copies_of_one_second_get_their_own_folders() {
        let m = Mem::default();
        let spec = lxqt(&m);
        let a = panel_snapshot_save(&m, &spec, "lxqt", HOME, ROOT, "manual", "20261125_120000").unwrap();
        let b = panel_snapshot_save(&m, &spec, "lxqt", HOME, ROOT, "before_install", "20261125_120000").unwrap();
        assert_ne!(a.path, b.path);
        assert!(b.path.ends_with("/20261125_120000-2"));
        assert!(!b.original);
    }

    #[test]
    fn unknown_fields_and_foreign_entries_survive_a_rewrite() {
        let m = Mem::default();
        let spec = lxqt(&m);
        m.write(
            &format!("{ROOT}/{PANEL_META_FILE}"),
            r#"{"version":2,"copies":[{"de":"kde","timestamp":"20260101_000000","reason":"manual","path":"/x","future":1},{"strange":true}]}"#,
        );
        panel_snapshot_save(&m, &spec, "lxqt", HOME, ROOT, "manual", "20261125_120000").unwrap();
        let raw = panel_meta_raw(&m, ROOT);
        assert_eq!(raw.len(), 3);
        assert_eq!(raw[0]["future"], 1);
        assert_eq!(raw[1]["strange"], true);
        let typed = panel_snapshots(&raw);
        assert_eq!(typed.len(), 2);
        assert_eq!(typed[0].format, snapshot_format::MANIFEST, "a missing format means a manifest copy");
        assert!(!m.exists(&format!("{ROOT}/{PANEL_META_FILE}.tmp")));
    }

    #[test]
    fn linked_copies_are_neither_counted_nor_removed() {
        let mut list: Vec<PanelSnapshot> = (0..MAX_PANEL_COPIES)
            .map(|i| PanelSnapshot {
                de: "mate".into(),
                timestamp: format!("20261125_1200{i:02}"),
                reason: "manual".into(),
                path: format!("/c/{i}"),
                parts: vec![],
                items: 1,
                original: false,
                format: snapshot_format::MANIFEST.into(),
                files: vec![],
                linked: false,
            })
            .collect();
        let mut linked = list[0].clone();
        linked.path = "/old/mate-panel.dconf".into();
        linked.timestamp = "20200101_000000".into();
        linked.linked = true;
        list.push(linked);
        assert!(panel_rotation(&list, "mate").is_empty());
        let mut extra = list[1].clone();
        extra.path = "/c/new".into();
        extra.timestamp = "20261126_000000".into();
        list.push(extra);
        assert_eq!(panel_rotation(&list, "mate"), vec!["/c/0".to_string()]);
    }

    #[test]
    fn cinnamon_has_one_list() {
        assert_eq!(panel_de_key("cinnamon"), "cinnamon-mint");
        assert_eq!(panel_de_key("kde"), "kde");
    }
}

#[cfg(test)]
mod desktop_spec_tests {
    use super::panel_copy_tests::{spec_for, Mem};
    use super::*;

    const HOME: &str = "/home/w";
    const COPY: &str = "/home/w/.local/share/rexon/backups/panel/x/1";

    /// A trimmed copy of the XML that xfce4-panel 4.20 ships
    /// (`/etc/xdg/xfce4/panel/default.xml`): an array with child properties,
    /// containers, all scalar kinds, an escaped value.
    const XFCE_PANEL_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<channel name="xfce4-panel" version="1.0">
  <property name="configver" type="int" value="2"/>
  <property name="panels" type="array">
    <value type="int" value="1"/>
    <value type="int" value="2"/>
    <property name="dark-mode" type="bool" value="true"/>
    <property name="panel-1" type="empty">
      <property name="position" type="string" value="p=6;x=0;y=0"/>
      <property name="length" type="uint" value="100"/>
      <property name="plugin-ids" type="array">
        <value type="int" value="1"/>
        <value type="int" value="2"/>
      </property>
    </property>
  </property>
  <property name="plugins" type="empty">
    <property name="plugin-1" type="string" value="applicationsmenu"/>
    <property name="plugin-2" type="string" value="tasklist">
      <property name="grouping" type="uint" value="1"/>
      <property name="label" type="string" value="Tom &amp; Jerry &quot;x&quot;"/>
    </property>
  </property>
</channel>
"#;

    #[test]
    fn xfconf_file_parses_with_types() {
        let props = xfconf_parse(XFCE_PANEL_XML).expect("a real channel file parses");
        let get = |p: &str| props.iter().find(|x| x.path == p).cloned().unwrap_or_else(|| panic!("{p} missing"));
        assert_eq!(get("/configver").values, [("int".to_string(), "2".to_string())]);
        let panels = get("/panels");
        assert!(panels.array);
        assert_eq!(panels.values, [("int".into(), "1".into()), ("int".into(), "2".into())]);
        assert_eq!(get("/panels/dark-mode").values, [("bool".into(), "true".into())]);
        assert_eq!(get("/panels/panel-1/length").values, [("uint".into(), "100".into())]);
        assert!(get("/panels/panel-1/plugin-ids").array);
        // A property with a value can have children too.
        assert_eq!(get("/plugins/plugin-2").values, [("string".into(), "tasklist".into())]);
        assert_eq!(get("/plugins/plugin-2/label").values[0].1, "Tom & Jerry \"x\"");
        assert!(!props.iter().any(|p| p.path == "/plugins" || p.path == "/panels/panel-1"), "containers are not written");
        assert_eq!(props.len(), 10);

        let argvs = xfconf_restore_argvs("xfce4-panel", &props);
        assert_eq!(argvs[0], ["xfconf-query", "-c", "xfce4-panel", "-p", "/", "-r", "-R"]);
        assert!(argvs.contains(&sv(&[
            "xfconf-query", "-c", "xfce4-panel", "-p", "/panels", "-n", "-a", "-t", "int", "-s", "1", "-t", "int", "-s", "2"
        ])));
        assert!(argvs.contains(&sv(&["xfconf-query", "-c", "xfce4-panel", "-p", "/configver", "-n", "-t", "int", "-s", "2"])));
    }

    #[test]
    fn broken_xfconf_files_are_refused() {
        assert!(xfconf_parse("<channel name=\"x\"><property name=\"a\" type=\"weird\" value=\"1\"/></channel>").is_err());
        assert!(xfconf_parse("<channel name=\"x\"><property name=\"a/b\" type=\"int\" value=\"1\"/></channel>").is_err());
        assert!(xfconf_parse("<channel name=\"x\"><value type=\"int\" value=\"1\"/></channel>").is_err());
        assert!(xfconf_parse("<html></html>").is_err());
        assert_eq!(xfconf_parse("<channel name=\"x\" version=\"1.0\"/>").map(|p| p.len()), Ok(0));
    }

    /// XFCE without xfce4-panel-profiles: the channel goes through its file
    /// and comes back through `xfconf-query`, the panel is restarted.
    #[test]
    fn xfce_round_trip_without_panel_profiles() {
        assert!(spec_for("xfce", &["xfce4-panel"], &[], &[]).is_none(), "no xfconf-query, nothing to restore with");
        let spec = spec_for("xfce", &["xfce4-panel", "xfconf-query"], &[], &[]).expect("XFCE is taken");
        let m = Mem::default();
        m.write(&xfconf_channel_path(&format!("{HOME}/.config"), "xfce4-panel"), XFCE_PANEL_XML);
        m.write(&format!("{HOME}/.config/xfce4/panel/launcher-3/1.desktop"), "[Desktop Entry]\n");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{HOME}/.config/xfce4/panel/launcher-3/1.desktop"), "broken");
        let notes = panel_copy_restore_in(&m, &spec, HOME, COPY, true).unwrap();
        assert!(notes.is_empty());
        assert_eq!(m.read(&format!("{HOME}/.config/xfce4/panel/launcher-3/1.desktop")).as_deref(), Some("[Desktop Entry]\n"));
        let runs = m.runs.borrow();
        let reset = runs.iter().position(|r| r.contains(&"-R".to_string())).expect("channel reset");
        let restart = runs.iter().position(|r| r == &sv(&["xfce4-panel", "-r"])).expect("panel restarted");
        assert!(reset < restart);
        assert_eq!(runs.iter().filter(|r| r[0] == "xfconf-query").count(), 11);
    }

    /// Not the running desktop: nothing is restarted.
    #[test]
    fn restore_outside_the_desktop_rereads_nothing() {
        let spec = spec_for("xfce", &["xfce4-panel", "xfconf-query"], &[], &[]).unwrap();
        let m = Mem::default();
        m.write(&xfconf_channel_path(&format!("{HOME}/.config"), "xfce4-panel"), XFCE_PANEL_XML);
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        assert!(!m.runs.borrow().iter().any(|r| r[0] == "xfce4-panel"));
    }

    /// KDE: the shell's files from the config home, `~/.local/share/plasma`
    /// without Rexon's wallpaper plugin, which stays as it is on restore.
    #[test]
    fn kde_copy_keeps_the_wallpaper_plugin_apart() {
        assert_eq!(PLASMA_DATA_KEEP, [format!("wallpapers/{}", crate::waywallen_kde_plugin::PLUGIN_ID)]);
        assert!(spec_for("kde", &[], &[], &[]).is_none());
        let spec = spec_for("kde", &["plasmashell"], &[], &[]).expect("Plasma is taken");
        let m = Mem::default();
        let cfg = format!("{HOME}/.config");
        let data = format!("{HOME}/.local/share/plasma");
        m.write(&format!("{cfg}/plasma-org.kde.plasma.desktop-appletsrc"), "panel=left");
        m.write(&format!("{data}/plasmoids/my.widget/metadata.json"), "{}");
        m.write(&format!("{data}/wallpapers/org.waywallen.kde/old.qml"), "old plugin");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        assert!(!m.exists(&format!("{COPY}/data/plasma/wallpapers/org.waywallen.kde")), "the plugin is not taken");
        assert!(m.exists(&format!("{COPY}/data/plasma/plasmoids/my.widget/metadata.json")));

        // The panel broke, a widget vanished, the plugin was updated meanwhile.
        m.write(&format!("{cfg}/plasma-org.kde.plasma.desktop-appletsrc"), "panel=gone");
        m.remove_tree(&format!("{data}/plasmoids")).unwrap();
        m.remove_tree(&format!("{data}/wallpapers")).unwrap();
        m.write(&format!("{data}/wallpapers/org.waywallen.kde/new.qml"), "new plugin");
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        assert_eq!(m.read(&format!("{cfg}/plasma-org.kde.plasma.desktop-appletsrc")).as_deref(), Some("panel=left"));
        assert!(m.exists(&format!("{data}/plasmoids/my.widget/metadata.json")));
        assert_eq!(m.read(&format!("{data}/wallpapers/org.waywallen.kde/new.qml")).as_deref(), Some("new plugin"));
        assert!(!m.exists(&format!("{data}/wallpapers/org.waywallen.kde/old.qml")));
        assert!(!m.exists(&format!("{data}.rexon-keep")));
    }

    /// MATE: on the live panel the objects are emptied first, then the dump is
    /// loaded by the layout section's own steps.
    #[test]
    fn mate_panel_goes_through_its_procedure() {
        let spec = spec_for("mate", &["mate-panel"], &[], &[]).expect("MATE is taken");
        let m = Mem::default();
        m.dconf.borrow_mut().insert(MATE_PANEL_DCONF_DIR.into(), "[general]\nobject-id-list=['a']\n".into());
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.runs.borrow_mut().clear();
        panel_copy_restore_in(&m, &spec, HOME, COPY, true).unwrap();
        let runs = m.runs.borrow();
        let empty = runs.iter().position(|r| *r == crate::mate_panel::empty_objects_argv()).expect("objects emptied");
        let steps = crate::mate_panel::load_steps(&format!("{COPY}/dconf-0.ini"));
        let load = runs.iter().position(|r| *r == steps[3]).expect("dump loaded");
        assert!(empty < load);
        assert!(!m.exists(&format!("{COPY}/dconf-0.ini.before-restore")), "the insurance is cleaned up");
        drop(runs);
        // Outside MATE: no panel to wait for.
        m.runs.borrow_mut().clear();
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        assert!(!m.runs.borrow().contains(&crate::mate_panel::empty_objects_argv()));
    }
}

#[cfg(test)]
mod shortcut_spec_tests {
    use super::panel_copy_tests::{spec_for, Mem};
    use super::*;

    const HOME: &str = "/home/w";
    const COPY: &str = "/home/w/.local/share/rexon/backups/panel/x/1";

    /// Every desktop with a copy spec takes its keyboard shortcuts too.
    #[test]
    fn every_desktop_takes_its_shortcuts() {
        let cases: &[(&str, &[&str], &[&str])] = &[
            ("kde", &["plasmashell"], &[]),
            ("mate", &["mate-panel"], &[]),
            ("xfce", &["xfce4-panel", "xfconf-query"], &[]),
            ("gnome", &["gnome-shell"], &[]),
            ("cinnamon-mint", &[], &[]),
            ("budgie", &["budgie-panel"], &[]),
            ("pantheon", &["io.elementary.wingpanel"], &[]),
            ("lxqt", &["lxqt-panel"], &[]),
            ("cosmic", &["cosmic-panel"], &[]),
            ("wayfire", &["wf-panel"], &[]),
            ("hyprland", &[], &[".config/hypr"]),
            ("niri", &[], &[".config/niri"]),
            ("sway", &[], &[".config/sway"]),
            ("river", &[], &[".config/river"]),
        ];
        for (de, tools, paths) in cases {
            let spec = spec_for(de, tools, paths, &[]).unwrap_or_else(|| panic!("{de}: no spec"));
            assert!(!spec.shortcuts.is_empty(), "{de}: no shortcuts");
            assert_eq!(spec.needs_relogin, *de == "river", "{de}");
        }
        // A compositor with only its config still gets buttons.
        let sway = spec_for("sway", &[], &[".config/sway"], &[]).unwrap();
        assert!(sway.parts.is_empty());
        assert!(sway.reload.contains(&PanelReload::Run(&["swaymsg", "reload"])));
        // Deepin's shortcuts are a separate study: none yet.
        assert!(shared_shortcut_desktops("deepin").is_empty());
    }

    #[test]
    fn gnome_branches_are_shared_with_budgie_and_pantheon() {
        assert_eq!(shared_shortcut_desktops("gnome"), ["budgie", "pantheon"]);
        assert_eq!(shared_shortcut_desktops("budgie"), ["gnome", "pantheon"]);
        assert!(shared_shortcut_desktops("mate").is_empty());
        for de in ["gnome", "budgie"] {
            let tools: &[&str] = if de == "gnome" { &["gnome-shell"] } else { &["budgie-panel"] };
            let spec = spec_for(de, tools, &[], &[]).unwrap();
            assert!(spec.shortcuts.contains(&PanelPart::DconfDir(GNOME_WM_KEYS_DIR)), "{de}");
            assert!(spec.shortcuts.contains(&PanelPart::DconfDir(GNOME_MEDIA_KEYS_DIR)), "{de}");
        }
    }

    /// Cinnamon: downloaded applets come with the copy, Rexon's own wallpaper
    /// extension neither goes into it nor gets replaced by it.
    #[test]
    fn cinnamon_takes_downloaded_applets_but_not_rexon() {
        let spec = spec_for("cinnamon-mint", &[], &[], &[]).unwrap();
        let m = Mem::default();
        let data = format!("{HOME}/.local/share/cinnamon");
        m.write(&format!("{data}/applets/weather@x/metadata.json"), "{}");
        m.write(&format!("{data}/extensions/{}/extension.js", crate::installer::EXT_ID), "ours");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        assert!(m.exists(&format!("{COPY}/data/cinnamon/applets/weather@x/metadata.json")));
        assert!(!m.exists(&format!("{COPY}/data/cinnamon/extensions/{}", crate::installer::EXT_ID)));
        m.remove_tree(&format!("{data}/applets")).unwrap();
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        assert!(m.exists(&format!("{data}/applets/weather@x/metadata.json")));
        assert_eq!(m.read(&format!("{data}/extensions/{}/extension.js", crate::installer::EXT_ID)).as_deref(), Some("ours"));
        assert!(m.runs.borrow().iter().any(|r| r.get(4).map(String::as_str) == Some("/org/cinnamon/desktop/keybindings/")));
    }

    /// COSMIC: the dock's pinned programs and the shortcuts come with the panel.
    #[test]
    fn cosmic_takes_pinned_apps_and_shortcuts() {
        let spec = spec_for("cosmic", &["cosmic-panel"], &[], &[]).unwrap();
        let m = Mem::default();
        let base = format!("{HOME}/.config/cosmic");
        m.write(&format!("{base}/com.system76.CosmicAppList/v1/favorites"), "[\"firefox\"]");
        m.write(&format!("{base}/com.system76.CosmicSettings.Shortcuts/v1/custom"), "{}");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{base}/com.system76.CosmicAppList/v1/favorites"), "[]");
        panel_copy_restore(&m, &spec, HOME, COPY).unwrap();
        assert_eq!(m.read(&format!("{base}/com.system76.CosmicAppList/v1/favorites")).as_deref(), Some("[\"firefox\"]"));
        assert!(m.exists(&format!("{base}/com.system76.CosmicSettings.Shortcuts/v1/custom")));
    }

    const KDE_FILE: &str = "[kwin]\n_k_friendly_name=KWin\nWindow Close=Alt+F4,Alt+F4,Close Window\nExpose=Ctrl+F9\\tMeta+W,Ctrl+F9,Toggle Present Windows\nOdd=Ctrl+Weird Key,none,Odd thing\n\n[services][org.kde.dolphin.desktop]\n_launch=Meta+E\n";

    #[test]
    fn kde_shortcuts_file_parses() {
        let list = kde_shortcuts_parse(KDE_FILE);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].component, "kwin");
        assert_eq!(list[0].component_name, "KWin");
        assert_eq!(list[0].keys, ["Alt+F4"]);
        assert_eq!(list[0].action_name, "Close Window");
        assert_eq!(list[1].keys, ["Ctrl+F9", "Meta+W"]);
        assert_eq!(list[3].component, "org.kde.dolphin.desktop");
        assert_eq!(list[3].action, "_launch");
        assert_eq!(kconfig_list("Ctrl+\\,,none,Comma"), ["Ctrl+,", "none", "Comma"]);
    }

    #[test]
    fn key_names_become_qt_codes() {
        assert_eq!(qt_key_sequence("Alt+F4"), Some([0x0800_0000 | 0x0100_0033, 0, 0, 0]));
        assert_eq!(qt_key_sequence("Meta+E"), Some([0x1000_0000 | 0x45, 0, 0, 0]));
        assert_eq!(qt_key_sequence("Ctrl++"), Some([0x0400_0000 | 0x2b, 0, 0, 0]));
        assert_eq!(qt_key_sequence("Volume Up"), Some([0x0100_0072, 0, 0, 0]));
        assert_eq!(qt_key_sequence("Ctrl+X, Ctrl+C"), Some([0x0400_0058, 0x0400_0043, 0, 0]));
        assert_eq!(qt_key_sequence("Launch (A)"), Some([0x0100_00ac, 0, 0, 0]));
        assert_eq!(qt_key_sequence("Ctrl+Weird Key"), None);
        assert_eq!(qt_key_sequence("Hyper+A"), None);
    }

    /// In a Plasma session only the changed actions go to KWin; an unknown key
    /// name is left to the next login, and the person hears how many.
    #[test]
    fn kde_shortcuts_go_to_kwin_where_changed() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let m = Mem::default();
        let cfg = format!("{HOME}/.config");
        let current = KDE_FILE
            .replace("Window Close=Alt+F4", "Window Close=none")
            .replace("Odd=Ctrl+Weird Key", "Odd=none")
            .replace("_launch=Meta+E", "_launch=Meta+D");
        m.write(&format!("{cfg}/{KDE_SHORTCUTS_FILE}"), &current);
        let notes = kde_shortcuts_restore(&m, &cfg, KDE_FILE, true).unwrap();
        assert_eq!(m.read(&format!("{cfg}/{KDE_SHORTCUTS_FILE}")).as_deref(), Some(KDE_FILE));
        let calls: Vec<Vec<String>> = m.runs.borrow().iter().filter(|r| r[0] == "gdbus").cloned().collect();
        assert_eq!(calls.len(), 2, "{calls:#?}");
        assert_eq!(calls[0][9], "['kwin', 'Window Close', 'KWin', 'Close Window']");
        assert_eq!(calls[0][10], format!("[([{}, 0, 0, 0],)]", 0x0800_0000u32 | 0x0100_0033));
        assert_eq!(calls[1][9], "['org.kde.dolphin.desktop', '_launch', 'org.kde.dolphin.desktop', '_launch']");
        assert_eq!(notes, [msg_kde_shortcuts_next_login(1)]);
        // Outside Plasma: the file only.
        m.runs.borrow_mut().clear();
        assert!(kde_shortcuts_restore(&m, &cfg, KDE_FILE, false).unwrap().is_empty());
        assert!(m.runs.borrow().is_empty());
    }

    #[test]
    fn kde_copy_round_trip_with_shortcuts() {
        let spec = spec_for("kde", &["plasmashell"], &[], &[]).unwrap();
        let m = Mem::default();
        let cfg = format!("{HOME}/.config");
        m.write(&format!("{cfg}/{KDE_SHORTCUTS_FILE}"), KDE_FILE);
        m.write(&format!("{HOME}/.local/share/kglobalaccel/net.local.mine.desktop"), "[Desktop Entry]\n");
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{cfg}/{KDE_SHORTCUTS_FILE}"), "[kwin]\n");
        m.remove_tree(&format!("{HOME}/.local/share/kglobalaccel")).unwrap();
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        assert_eq!(m.read(&format!("{cfg}/{KDE_SHORTCUTS_FILE}")).as_deref(), Some(KDE_FILE));
        assert!(m.exists(&format!("{HOME}/.local/share/kglobalaccel/net.local.mine.desktop")));
    }
}

#[cfg(test)]
mod panel_migration_tests {
    use super::panel_copy_tests::Mem;
    use super::*;

    const DATA: &str = "/home/w/.local/share";
    const B: &str = "/home/w/.local/share/rexon/backups";

    fn inventory(m: &Mem) -> Vec<PanelSnapshot> {
        panel_snapshots(&panel_meta_raw(m, &panel_root(DATA)))
    }

    fn machine() -> Mem {
        let m = Mem::default();
        let plasma = format!("{B}/plasma-panel");
        m.write(
            &plasma_meta_path(&plasma),
            &format!(
                r#"{{"backups":[
                {{"timestamp":"20261001_100000","reason":"manual","path":"{plasma}/20261001_100000","files":["/home/w/.config/plasmashellrc"],"full":false}},
                {{"timestamp":"20261002_100000","reason":"before_switch","path":"{plasma}/20261002_100000","files":["/home/w/.config/kglobalshortcutsrc"],"full":true}},
                {{"timestamp":"20261003_100000","reason":"manual","path":"{plasma}/gone","files":[],"full":false}}
                ]}}"#
            ),
        );
        m.write(&format!("{plasma}/20261001_100000/plasmashellrc"), "a");
        m.write(&format!("{plasma}/20261002_100000/kglobalshortcutsrc"), "b");
        m.write(&format!("{B}/mate-panel.dconf"), "[toplevels]\n");
        m.write(&format!("{B}/mate-panel.dconf.last"), "[toplevels]\n");
        m.write(&format!("{B}/xfce4-panel.tar.bz2"), "bz");
        m.write(&format!("{B}/panel-lxqt/manifest.json"), "[]");
        m.mtimes.borrow_mut().insert(format!("{B}/mate-panel.dconf"), "20261019_120000".into());
        m.mtimes.borrow_mut().insert(format!("{B}/mate-panel.dconf.last"), "20261101_120000".into());
        m
    }

    #[test]
    fn old_copies_are_referred_to_where_they_lie() {
        let m = machine();
        assert_eq!(panel_migrate(&m, DATA), Ok(true));
        let list = inventory(&m);
        let kde = panel_copies_of(&list, "kde");
        assert_eq!(kde.len(), 2, "a snapshot whose folder is gone is not listed");
        assert_eq!(kde[0].format, snapshot_format::PLASMA_FILES);
        assert_eq!(kde[0].parts, [snapshot_part::PANEL, snapshot_part::SHORTCUTS]);
        assert_eq!(kde[1].parts, [snapshot_part::PANEL]);
        assert!(kde[1].original && !kde[0].original, "the oldest one is the original");
        assert!(!kde[0].linked, "Plasma snapshots are Rexon's own: rotation counts them");
        assert!(m.exists(&plasma_meta_migrated_mark(&format!("{B}/plasma-panel"))));
        assert!(m.exists(&plasma_meta_path(&format!("{B}/plasma-panel"))), "the old meta file stays");

        let mate = panel_copies_of(&list, "mate");
        assert_eq!(mate.len(), 2);
        assert_eq!(mate[0].reason, "before_layout");
        assert_eq!(mate[1].timestamp, "20261019_120000");
        assert!(mate[1].original && mate[1].linked && mate[1].format == snapshot_format::MATE_DCONF);
        let xfce = panel_copies_of(&list, "xfce");
        assert_eq!(xfce[0].format, snapshot_format::XFCE_PROFILE);
        let lxqt = panel_copies_of(&list, "lxqt");
        assert_eq!(lxqt[0].path, format!("{B}/panel-lxqt"));
        assert!(lxqt[0].linked && lxqt[0].original);
    }

    #[test]
    fn migration_is_idempotent() {
        let m = machine();
        assert_eq!(panel_migrate(&m, DATA), Ok(true));
        let first = m.read(&format!("{}/{PANEL_META_FILE}", panel_root(DATA)));
        assert_eq!(panel_migrate(&m, DATA), Ok(false));
        assert_eq!(m.read(&format!("{}/{PANEL_META_FILE}", panel_root(DATA))), first);
    }

    /// MATE: the layout section restored the original (the "returned" mark):
    /// it is no longer the original; the layout section rewrote `.last`: the
    /// time follows the file; a vanished file leaves the list.
    #[test]
    fn linked_mate_copies_follow_their_files() {
        let m = machine();
        panel_migrate(&m, DATA).unwrap();
        m.write(&panel_original_returned_mark(&format!("{B}/mate-panel.dconf")), PANEL_ORIGINAL_RETURNED);
        m.mtimes.borrow_mut().insert(format!("{B}/mate-panel.dconf.last"), "20261120_080000".into());
        m.remove_tree(&format!("{B}/xfce4-panel.tar.bz2")).unwrap();
        assert_eq!(panel_migrate(&m, DATA), Ok(true));
        let list = inventory(&m);
        let mate = panel_copies_of(&list, "mate");
        assert_eq!(mate[0].timestamp, "20261120_080000");
        assert!(!mate.iter().any(|s| s.original), "{mate:#?}");
        assert!(panel_copies_of(&list, "xfce").is_empty());
    }

    #[test]
    fn broken_old_meta_and_unknown_fields() {
        let m = Mem::default();
        m.write(&plasma_meta_path(&format!("{B}/plasma-panel")), "{not json");
        assert_eq!(panel_migrate(&m, DATA), Ok(false), "nothing to bring in");
        // An old entry with a field we do not know is skipped by the typed parse,
        // but stays in the old file untouched.
        let m = machine();
        m.write(
            &plasma_meta_path(&format!("{B}/plasma-panel")),
            &format!(r#"{{"backups":[{{"timestamp":"1","reason":"manual","path":"{B}/plasma-panel/20261001_100000","files":[],"full":false,"new":1}}]}}"#),
        );
        panel_migrate(&m, DATA).unwrap();
        assert!(panel_copies_of(&inventory(&m), "kde").is_empty());
        assert!(m.read(&plasma_meta_path(&format!("{B}/plasma-panel"))).unwrap().contains("\"new\":1"));
    }

    /// The first copy taken with the new model after a migration does not
    /// become a second original.
    #[test]
    fn a_new_copy_after_migration_is_not_a_second_original() {
        let m = machine();
        panel_migrate(&m, DATA).unwrap();
        m.write("/home/w/.config/lxqt/panel.conf", "[panel1]\n");
        let spec = super::panel_copy_tests::spec_for("lxqt", &["lxqt-panel"], &[], &[]).unwrap();
        let s = panel_snapshot_save(&m, &spec, "lxqt", "/home/w", &panel_root(DATA), "manual", "20261125_100000").unwrap();
        assert!(!s.original);
        assert_eq!(panel_copies_of(&inventory(&m), "lxqt").iter().filter(|s| s.original).count(), 1);
    }
}

#[cfg(test)]
mod panel_restore_tests {
    use super::panel_copy_tests::{spec_for, Mem};
    use super::*;

    const HOME: &str = "/home/w";
    const DATA: &str = "/home/w/.local/share";

    #[test]
    fn manifest_copy_restores_even_without_the_desktop_installed() {
        let m = Mem::default();
        m.write(&format!("{HOME}/.config/lxqt/panel.conf"), "[panel1]\nposition=Bottom\n");
        let spec = spec_for("lxqt", &["lxqt-panel"], &[], &[]).unwrap();
        let snap = panel_snapshot_save(&m, &spec, "lxqt", HOME, &panel_root(DATA), "manual", "20261125_100000").unwrap();
        m.write(&format!("{HOME}/.config/lxqt/panel.conf"), "broken");
        // `panel_restore_spec` does not ask whether lxqt-panel is installed.
        assert!(panel_restore_spec("lxqt").is_some());
        let notes = panel_snapshot_restore(&m, HOME, &snap, false).unwrap();
        assert!(notes.is_empty());
        assert_eq!(m.read(&format!("{HOME}/.config/lxqt/panel.conf")).as_deref(), Some("[panel1]\nposition=Bottom\n"));
        assert!(m.runs.borrow().iter().all(|r| r[0] != "dbus-send"), "not the running desktop: nothing restarted");
    }

    /// An old Plasma snapshot: files back by basename, shortcuts through
    /// their own path, the current wallpaper plugin kept.
    #[test]
    fn old_plasma_snapshot_restores() {
        let m = Mem::default();
        let dir = format!("{DATA}/rexon/backups/plasma-panel/20261001_100000");
        m.write(&format!("{dir}/plasmashellrc"), "old shell");
        m.write(&format!("{dir}/kglobalshortcutsrc"), "[kwin]\nExpose=Meta+W,Ctrl+F9,Present\n");
        m.write(&format!("{dir}/plasma/plasmoids/a/x"), "widget");
        m.write(&format!("{dir}/plasma/wallpapers/org.waywallen.kde/main.qml"), "old plugin");
        m.write(&format!("{DATA}/plasma/wallpapers/org.waywallen.kde/main.qml"), "new plugin");
        let snap = PanelSnapshot {
            de: "kde".into(),
            timestamp: "20261001_100000".into(),
            reason: "before_install".into(),
            path: dir.clone(),
            parts: vec![],
            items: 3,
            original: true,
            format: snapshot_format::PLASMA_FILES.into(),
            files: vec![
                format!("{HOME}/.config/plasmashellrc"),
                format!("{HOME}/.config/kglobalshortcutsrc"),
                format!("{DATA}/plasma"),
            ],
            linked: false,
        };
        panel_snapshot_restore(&m, HOME, &snap, false).unwrap();
        assert_eq!(m.read(&format!("{HOME}/.config/plasmashellrc")).as_deref(), Some("old shell"));
        assert!(m.read(&format!("{HOME}/.config/kglobalshortcutsrc")).unwrap().contains("Meta+W"));
        assert_eq!(m.read(&format!("{DATA}/plasma/plasmoids/a/x")).as_deref(), Some("widget"));
        assert_eq!(m.read(&format!("{DATA}/plasma/wallpapers/org.waywallen.kde/main.qml")).as_deref(), Some("new plugin"));
    }

    /// The MATE layout section's original: restoring it places the
    /// "returned" mark, the `.last` neighbour does not.
    #[test]
    fn linked_mate_copy_restores_and_marks_the_original() {
        let m = Mem::default();
        let file = format!("{DATA}/rexon/backups/mate-panel.dconf");
        m.write(&file, "[toplevels/top]\nexpand=true\n");
        m.write(&format!("{file}.last"), "[toplevels/top]\nexpand=false\n");
        panel_migrate(&m, DATA).unwrap();
        let list = panel_snapshots(&panel_meta_raw(&m, &panel_root(DATA)));
        let last = panel_snapshot_by_path(&list, &format!("{file}.last")).unwrap();
        panel_snapshot_restore(&m, HOME, &last, false).unwrap();
        assert!(!m.exists(&panel_original_returned_mark(&file)));
        let orig = panel_snapshot_by_path(&list, &file).unwrap();
        assert!(orig.original);
        panel_snapshot_restore(&m, HOME, &orig, true).unwrap();
        assert_eq!(m.read(&panel_original_returned_mark(&file)).as_deref(), Some(PANEL_ORIGINAL_RETURNED));
        assert!(m.runs.borrow().contains(&crate::mate_panel::empty_objects_argv()));
    }

    #[test]
    fn old_xfce_archive_goes_through_panel_profiles() {
        let m = Mem::default();
        let path = xfce_panel_copy_path(DATA);
        let snap = PanelSnapshot {
            de: "xfce".into(),
            timestamp: "1".into(),
            reason: "migrated".into(),
            path: path.clone(),
            parts: vec![],
            items: 1,
            original: true,
            format: snapshot_format::XFCE_PROFILE.into(),
            files: vec![],
            linked: true,
        };
        panel_snapshot_restore(&m, HOME, &snap, true).unwrap();
        assert!(m.runs.borrow().contains(&xfce_panel_load_argv(&path)));
    }
}

#[cfg(test)]
mod deepin_shortcut_tests {
    use super::panel_copy_tests::{spec_for, Mem};
    use super::*;
    use crate::dde::{
        dconfig_schema_path, dconfig_subpath_schema_path, GESTURE_RESOURCE, KEYBINDING_APPID, KEYBINDING_INI_DIR,
        SHORTCUT_RESOURCE, SHORTCUT_SERVICE_MARK,
    };

    const HOME: &str = "/home/w";
    const COPY: &str = "/home/w/.local/share/rexon/backups/panel/deepin/1";

    fn rw(keys: &[&str]) -> String {
        let body: Vec<String> = keys.iter().map(|k| format!("\"{k}\": {{\"value\": [], \"permissions\": \"readwrite\"}}")).collect();
        format!("{{\"contents\": {{{}, \"away\": {{\"value\": 1, \"permissions\": \"\"}}}}}}", body.join(", "))
    }

    /// A Deepin with dde-services: two system shortcuts listed by a
    /// registration file (one over a `\` line), a gesture folder, the base
    /// schema of own shortcuts and one own shortcut.
    fn machine() -> Mem {
        let m = Mem::default();
        m.write(
            &format!("{KEYBINDING_INI_DIR}/org.deepin.dde.keybinding.ini"),
            "[Config]\nSubPaths=org.deepin.dde.keybinding.shortcut.app.launcher,\\\n  org.deepin.dde.keybinding.shortcut.system.terminal\n",
        );
        for sub in ["/org.deepin.dde.keybinding.shortcut.app.launcher", "/org.deepin.dde.keybinding.shortcut.system.terminal"] {
            m.write(&dconfig_subpath_schema_path(KEYBINDING_APPID, sub, SHORTCUT_RESOURCE), &rw(&["hotkeys", "enabled"]));
        }
        let gesture = "/org.deepin.dde.keybinding.gesture.swipe-up-3";
        m.write(&dconfig_subpath_schema_path(KEYBINDING_APPID, gesture, GESTURE_RESOURCE), &rw(&["action"]));
        m.write(&dconfig_schema_path(KEYBINDING_APPID, SHORTCUT_RESOURCE), &rw(&["hotkeys", "triggerValue"]));
        m.write(&format!("{HOME}/.config/{}", crate::dde::CUSTOM_SHORTCUTS_INI), "[Config]\nSubPaths=/mine-1\n");
        let set = |sub: &str, res: &str, key: &str, v: &str| {
            m.dde.borrow_mut().insert(format!("{KEYBINDING_APPID}|{res}|{key}@{sub}"), v.to_string());
        };
        set("/org.deepin.dde.keybinding.shortcut.app.launcher", SHORTCUT_RESOURCE, "hotkeys", "\"[\n    \"Meta+S\"\n]\"");
        set("/org.deepin.dde.keybinding.shortcut.system.terminal", SHORTCUT_RESOURCE, "hotkeys", "\"[\n    \"Ctrl+Alt+T\"\n]\"");
        set("/mine-1", SHORTCUT_RESOURCE, "hotkeys", "\"[\n    \"Meta+B\"\n]\"");
        m
    }

    fn spec() -> PanelCopySpec {
        let schemas: Vec<String> = DDE_DOCK_RESOURCES.iter().map(|r| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r)).collect();
        let mut refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        refs.push(SHORTCUT_SERVICE_MARK);
        let spec = spec_for("deepin", &["dde-dconfig", "dde-shell"], &refs, &[]).expect("Deepin V25");
        PanelCopySpec { parts: Vec::new(), ..spec }
    }

    #[test]
    fn registration_files_and_subpaths() {
        use crate::dde::{dconfig_subpath_ok, keybinding_ini_subpaths};
        assert_eq!(
            keybinding_ini_subpaths("[Other]\nSubPaths=x\n[Config]\nSubPaths=a.b,\\\n c ,\\\n\n"),
            ["/a.b", "/c"]
        );
        assert!(keybinding_ini_subpaths("[Config]\nX=1\n").is_empty());
        assert!(dconfig_subpath_ok("/org.deepin.x"));
        assert!(!dconfig_subpath_ok("org.deepin.x"), "dtkcore refuses a subpath without the slash");
        assert!(!dconfig_subpath_ok("/../x") && !dconfig_subpath_ok("/-x"));
        assert_eq!(
            crate::dde::dconfig_get_sub_argv(KEYBINDING_APPID, SHORTCUT_RESOURCE, "/s", "hotkeys"),
            ["dde-dconfig", "get", "-a", KEYBINDING_APPID, "-r", SHORTCUT_RESOURCE, "-s", "/s", "-k", "hotkeys"]
        );
    }

    /// The shortcut part exists only with dde-services' module, restarts the
    /// service group on restore and asks for no relogin.
    #[test]
    fn spec_takes_shortcuts_only_with_dde_services() {
        let schemas: Vec<String> = DDE_DOCK_RESOURCES.iter().map(|r| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r)).collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let old = spec_for("deepin", &["dde-dconfig", "dde-shell"], &refs, &[]).unwrap();
        assert!(old.shortcuts.is_empty() && !old.restarts_services);
        let new = spec();
        assert_eq!(new.shortcuts, [PanelPart::DdeShortcuts]);
        assert!(new.reload.contains(&PanelReload::RestartUserUnit(crate::dde::SERVICE_GROUP_UNIT)));
        assert!(new.restarts_services && !new.needs_relogin);
    }

    /// A swap of two shortcuts, an own shortcut made after the copy, an entry
    /// unknown to this release: all handled, then dde-services rereads.
    #[test]
    fn shortcuts_round_trip() {
        let m = machine();
        let spec = spec();
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        let body: serde_json::Value = serde_json::from_str(&m.read(&format!("{COPY}/{DDE_SHORTCUTS_COPY_FILE}")).unwrap()).unwrap();
        assert_eq!(body["system"].as_array().unwrap().len(), 3, "two shortcuts and a gesture");
        assert_eq!(body["system"][0]["values"]["hotkeys"]["value"], "[\"Meta+S\"]");
        assert!(body["system"][0]["values"].get("away").is_none(), "read-only keys are not taken");
        assert_eq!(body["custom"][0]["subpath"], "/mine-1");

        // Swap A and B, add an own shortcut, make one entry unknown.
        let key = |sub: &str| format!("{KEYBINDING_APPID}|{SHORTCUT_RESOURCE}|hotkeys@{sub}");
        m.dde.borrow_mut().insert(key("/org.deepin.dde.keybinding.shortcut.app.launcher"), "\"[\\\"Ctrl+Alt+T\\\"]\"".into());
        m.dde.borrow_mut().insert(key("/org.deepin.dde.keybinding.shortcut.system.terminal"), "\"[\\\"Meta+S\\\"]\"".into());
        m.write(&format!("{HOME}/.config/{}", crate::dde::CUSTOM_SHORTCUTS_INI), "[Config]\nSubPaths=/mine-1,/mine-2\n");
        m.remove_tree(&dconfig_subpath_schema_path(KEYBINDING_APPID, "/org.deepin.dde.keybinding.gesture.swipe-up-3", GESTURE_RESOURCE)).unwrap();
        m.runs.borrow_mut().clear();

        panel_copy_restore_in(&m, &spec, HOME, COPY, true).unwrap();
        let runs = m.runs.borrow();
        let sets: Vec<&Vec<String>> = runs.iter().filter(|r| r[0] == "dde-dconfig" && r[1] != "get").collect();
        assert!(sets.iter().any(|r| r.contains(&"/org.deepin.dde.keybinding.shortcut.app.launcher".to_string())
            && r.contains(&"[\"Meta+S\"]".to_string())), "{sets:#?}");
        assert!(sets.iter().any(|r| r[1] == "reset" && r.contains(&"/mine-2".to_string())), "a later own shortcut is reset");
        assert!(!sets.iter().any(|r| r.iter().any(|a| a.contains("gesture"))), "an unknown entry is skipped");
        assert!(runs.contains(&crate::dde::keybinding_reload_argv()));
        let restart = sv(&["systemctl", "--user", "--no-block", "try-restart", crate::dde::SERVICE_GROUP_UNIT]);
        let reload = runs.iter().position(|r| *r == crate::dde::keybinding_reload_argv()).unwrap();
        assert!(runs.iter().position(|r| *r == restart).unwrap() > reload, "the group restarts after the values");
        drop(runs);
        assert_eq!(
            m.read(&format!("{HOME}/.config/{}", crate::dde::CUSTOM_SHORTCUTS_INI)).as_deref(),
            Some("[Config]\nSubPaths=/mine-1\n")
        );
    }

    /// Outside Deepin nothing is restarted or reread.
    #[test]
    fn restore_outside_deepin_restarts_nothing() {
        let m = machine();
        let spec = spec();
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.runs.borrow_mut().clear();
        panel_copy_restore_in(&m, &spec, HOME, COPY, false).unwrap();
        let runs = m.runs.borrow();
        assert!(!runs.iter().any(|r| r[0] == "systemctl" || r[0] == "busctl"), "{runs:#?}");
    }

    /// The tray plugins' own settings come with the dock when their schemas exist.
    #[test]
    fn tray_plugin_settings_are_taken() {
        let schemas: Vec<String> = DDE_DOCK_RESOURCES
            .iter()
            .map(|r| crate::dde::dconfig_schema_path(DDE_SHELL_DCONFIG_APPID, r))
            .chain([crate::dde::dconfig_schema_path(DDE_TRAY_LOADER_APPID, "org.deepin.dde.dock.plugin.power")])
            .collect();
        let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
        let spec = spec_for("deepin", &["dde-dconfig", "dde-shell"], &refs, &[]).unwrap();
        assert!(spec.parts.contains(&PanelPart::DdeDconfig { appid: DDE_TRAY_LOADER_APPID, resource: "org.deepin.dde.dock.plugin.power" }));
        assert!(!spec.parts.iter().any(|p| matches!(p, PanelPart::DdeDconfig { resource: "org.deepin.dde.dock.plugin.datetime", .. })));
    }

    #[test]
    fn broken_copy_file_is_refused() {
        let m = machine();
        let spec = spec();
        panel_copy_save(&m, &spec, HOME, COPY).unwrap();
        m.write(&format!("{COPY}/{DDE_SHORTCUTS_COPY_FILE}"), "{broken");
        assert!(panel_copy_restore_in(&m, &spec, HOME, COPY, true).is_err());
    }
}
