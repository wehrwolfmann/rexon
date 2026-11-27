//! «Remove» on an installed theme — from the section tile and from the disk
//! overviews.
//!
//! WHY. Previously only what was bought in the store could be removed (by the
//! installed log, `appearance_installer::uninstall_from_log`): a theme put in
//! by hand, by another installer or unpacked into `~/.icons` stayed forever. In
//! KDE any user theme can be removed (parity lines 0.8, 0.9 and the "Removal"
//! line of each section), and not at once: the tile is marked, a bar with
//! «Undo» pops up at the bottom, and the theme is removed when the bar closes.
//!
//! Rules (decided by [`RemovalRoots::verdict`] from the scan path —
//! `ThemeItem::install_path`, the "enabled" flag is `local_kind == "active"`):
//!   * the theme enabled right now cannot be removed — the button is disabled
//!     with a hint ([`removal_active_reason`]); on removal it is checked again
//!     against the live map of what is enabled, in all kinds that read this
//!     directory (one folder in `~/.themes` can be both a GTK theme and an xfwm4
//!     border). The exception is the person's own running Conky widget: after a
//!     question it stops and goes to the trash ([`Verdict::RunningConky`]), like
//!     one found on disk;
//!   * an own theme — a direct child of a user theme directory
//!     (`~/.local/share/…`, `~/.icons`, `~/.themes`, `~/.config/Kvantum` and the
//!     directories of desktop sections) — is removed deferred ([`RemovalQueue`]);
//!   * a login screen and a boot splash that Rexon itself installed (the path is
//!     in the installed log) — by the former «Remove» via the log
//!     (`LiveInstaller::uninstall`, the root helper; commit a135f89). A password
//!     window when the pop-up bar expires would be a surprise, so there is no
//!     delay here;
//!   * a login screen and a boot splash without an owner package, put in by
//!     hand or by another installer — by the same root helper by path
//!     (`LiveInstaller::remove_loose_system_theme`), also without delay;
//!   * other system things (`/usr/share/…`) — the button is disabled with a
//!     hint ([`removal_system_reason`]): the files belong to a package;
//!   * a tile without a path (a Qt style from `QStyleFactory`) has no button
//!     at all.
//!
//! Deferred removal touches nothing on disk while the «Undo» bar is open:
//! [`RemovalQueue`] only remembers the paths, and the page hides their tiles
//! ([`RemovalQueue::retain_visible`]). [`commit`] removes them when the bar
//! closes or expires (and on program exit). If the program crashed before that,
//! the theme stayed in place: losing a theme is worse than not removing it.

use std::collections::HashSet;
use std::path::Path;

use serde_json::{Map, Value};

use crate::i18n::{ntr, tr, trf};
use crate::installed_scan::{de_types, sidecar_preview_path, types_for};
use crate::models::ThemeItem;

/// How long the pop-up bar with «Undo» stays before the theme is removed.
pub const UNDO_TIMEOUT_MS: i32 = 8000;

/// What to do with the «Remove» button of a tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No button: the tile has no path on disk.
    Hidden,
    /// Own theme: deferred removal via [`RemovalQueue`], without a question, with
    /// the «Undo» bar — as in all «Appearance» sections. `path` is what actually
    /// lies on disk (for a color scheme — the `.colors` file). An own Conky widget
    /// that is not enabled goes the same way, but to the trash ([`commit`]).
    Local { path: String },
    /// A system theme installed by Rexon: the former «Remove» via the log
    /// (`LiveInstaller::uninstall(content_id)`), at once, with a password window.
    Journal { content_id: String },
    /// A system theme without an owner package and without a log record (put in by
    /// hand): the root helper removes it by path, at once, with a password window.
    Loose { path: String },
    /// A Conky widget found on disk outside the Rexon directory: after a question
    /// (`conky_scan::found_removal_questions`) it stops and goes to the trash
    /// (`conky_scan::live_remove_found`), at once, without the «Undo» bar — it is
    /// restored from the trash.
    FoundConky { path: String },
    /// An own Conky widget (a folder in the Rexon directory) that is running now:
    /// after a question (`conky_scan::running_removal_question`) it stops, loses
    /// autostart and goes to the trash ([`live_remove_running_conky`]), at once —
    /// like one found on disk.
    RunningConky { path: String },
    /// The button is visible but disabled; `reason` is the hint on it.
    Blocked { reason: String },
}

impl Verdict {
    /// Whether the button is visible.
    pub fn visible(&self) -> bool {
        !matches!(self, Verdict::Hidden)
    }
    /// Whether it can be pressed.
    pub fn enabled(&self) -> bool {
        matches!(
            self,
            Verdict::Local { .. } | Verdict::Journal { .. } | Verdict::Loose { .. } | Verdict::FoundConky { .. } | Verdict::RunningConky { .. }
        )
    }
    /// Hint on a disabled button (empty on an enabled one).
    pub fn tooltip(&self) -> String {
        match self {
            Verdict::Blocked { reason } => reason.clone(),
            _ => String::new(),
        }
    }
}

// ── Captions ───────────────────────────────────────────────────────────

// The captions of this flow have no noun "theme": icons, cursors, fonts, Conky
// and KDE widgets, GNOME extensions and wallpapers go through the same bar and
// the same hints (all «Remove» tiles of the «Appearance» section), and
// "The theme “Caffeine” will be removed" about a GNOME extension was confusing
// (live check 15.11). The core has no "kind → noun" map of its own, and
// starting one for a single line would breed a second list of kinds (rule #12).

/// Hint on «Remove» of what is enabled now.
pub fn removal_active_reason() -> String {
    tr("You can't remove what is in use now. Switch to something else first.")
}

/// Hint on «Remove» of a system item (from a package).
pub fn removal_system_reason() -> String {
    tr("This came with the system (it is in /usr/share), so Rexon doesn't remove it. \
        It goes away together with its package.")
}

/// Hint on «Remove» of an item lying outside the user directories.
pub fn removal_outside_reason() -> String {
    tr("This is outside your personal appearance folders, so Rexon doesn't remove it.")
}

/// Removal result for the status line. `names` are the names of what was
/// removed, in the order of marking.
pub fn removal_toast_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        // TRANSLATORS: %1 is the name of a removed theme, icon set, cursor, font,
        // widget, GNOME extension or wallpaper — keep the sentence noun-free.
        [one] => trf("Removed “%1”.", &[one]),
        many => ntr("%n item removed.", "%n items removed.", many.len() as u64),
    }
}

/// Text of the «Undo» bar while the marked items are still on disk: the removal
/// IS GOING ON, not done. The past tense ("removed") was misleading here: the
/// person decided everything was already gone and didn't look for «Undo». The
/// deadline is a separate countdown line ([`removal_countdown_text`]).
pub fn removal_pending_text(names: &[String]) -> String {
    match names {
        [] => String::new(),
        // TRANSLATORS: %1 is the name of anything removable on the Appearance
        // tab (theme, icons, cursor, font, widget, GNOME extension, wallpaper);
        // the removal is not done yet — a countdown with “Undo” follows.
        [one] => trf("Removing “%1”.", &[one]),
        many => ntr("Removing %n item.", "Removing %n items.", many.len() as u64),
    }
}

/// Countdown of the «Undo» bar: in how many seconds the marked items are removed
/// (the page counts from [`UNDO_TIMEOUT_MS`] and pauses while the pointer is over
/// the bar).
pub fn removal_countdown_text(seconds: u64) -> String {
    // A standalone sentence: the line comes right after «Removing “…”.», and the
    // former "In 6 seconds, if…" read like a fragment.
    ntr(
        "%n second left — “Undo” cancels the removal.",
        "%n seconds left — “Undo” cancels the removal.",
        seconds,
    )
}

/// Button of the pop-up bar: cancel the removal.
pub fn removal_undo_btn() -> String {
    tr("Undo")
}

/// The marked item turned out to be enabled by the time of removal.
fn removal_now_active(name: &str) -> String {
    trf("“%1” is in use now, so it wasn't removed.", &[&name])
}

// ── Theme directories ──────────────────────────────────────────────────

/// User theme directories (with the kinds looked for in them) and system
/// directories — from the same tables as the scans (`installed_scan::types_for`,
/// `de_types`, the former `~/.icons` and `~/.themes` from `section_types`): there
/// is no new list of places here (rule #12).
pub struct RemovalRoots {
    /// (user directory, theme kind).
    user: Vec<(String, String)>,
    /// System theme directories and the usr root.
    system: Vec<String>,
    /// Theme directories of the login screen and the boot splash (removed by the helper).
    helper: Vec<String>,
    /// Home directory (nested Conky widgets, `conky_nested_widgets`).
    home: String,
}

impl RemovalRoots {
    pub fn new(home: &str, usr_root: &str) -> Self {
        let home = home.trim_end_matches('/');
        let usr_root = usr_root.trim_end_matches('/');
        let mut user: Vec<(String, String)> = Vec::new();
        let mut system: Vec<String> = vec![usr_root.to_string()];
        let mut helper: Vec<String> = Vec::new();
        fn push(user: &mut Vec<(String, String)>, dir: &str, kind: &str) {
            let dir = dir.trim_end_matches('/');
            if !dir.is_empty() && !user.iter().any(|(d, k)| d == dir && k == kind) {
                user.push((dir.to_string(), kind.to_string()));
            }
        }
        let types = types_for(home, usr_root).into_iter().chain(de_types(home, usr_root, ""));
        for t in types {
            push(&mut user, &t.user_dir, t.apply_kind);
            if !t.sys_dir.is_empty() {
                let sys = t.sys_dir.trim_end_matches('/').to_string();
                if t.apply_kind == "sddm" || t.apply_kind == "plymouth" {
                    helper.push(sys.clone());
                }
                if !system.contains(&sys) {
                    system.push(sys);
                }
            }
            // Border and shell themes are also looked for in `~/.local/share/themes`
            // (`installed_scan::de_types`).
            if t.entry == "sub" {
                push(&mut user, &format!("{home}/.local/share/themes"), t.apply_kind);
                push(&mut user, &format!("{home}/.themes"), t.apply_kind);
            }
        }
        // Former directories read by both KDE and GTK.
        for kind in ["icons", "cursor"] {
            push(&mut user, &format!("{home}/.icons"), kind);
        }
        push(&mut user, &format!("{home}/.themes"), "gtk");
        // The KDE splash and lock screen are parts of look-and-feel packages.
        for kind in ["splash", "lockscreen"] {
            push(&mut user, &format!("{home}/.local/share/plasma/look-and-feel"), kind);
        }
        RemovalRoots { user, system, helper, home: home.to_string() }
    }

    /// Theme kinds looked for in the directory `dir` (empty — not a user theme
    /// directory).
    fn kinds_at(&self, dir: &str) -> Vec<&str> {
        self.user.iter().filter(|(d, _)| d == dir).map(|(_, k)| k.as_str()).collect()
    }

    /// Decision for a scan tile.
    ///
    /// `install_path` is `ThemeItem::install_path`; for a color scheme the scan
    /// gives the path without `.colors` — the file is found here. `active` — the
    /// tile is marked enabled. `journal` is the installed log
    /// (`ocs_store::parse_installed_log`). `loose` — system themes without an owner
    /// package (`LiveScan::loose_system_themes`).
    pub fn verdict(
        &self,
        install_path: &str,
        active: bool,
        journal: &Map<String, Value>,
        loose: &HashSet<String>,
    ) -> Verdict {
        let path = install_path.trim_end_matches('/');
        if path.is_empty() {
            return Verdict::Hidden;
        }
        if active {
            return Verdict::Blocked { reason: removal_active_reason() };
        }
        let target = disk_target(path);
        let Some((parent, name)) = target.rsplit_once('/') else {
            return Verdict::Blocked { reason: removal_outside_reason() };
        };
        if (safe_name(name) && !self.kinds_at(parent).is_empty()) || !self.nested_kinds(&target).is_empty() {
            return if present(&target) {
                Verdict::Local { path: target }
            } else {
                Verdict::Hidden // the theme is already gone — nothing to remove
            };
        }
        // A nested widget of a Conky package (`<root>/<id>/<subfolder>`) — its own
        // tile, removed on its own.
        if self.is_nested_conky(&target) {
            return if present(&target) { Verdict::Local { path: target } } else { Verdict::Hidden };
        }
        if self.helper.iter().any(|d| d == parent) {
            if let Some(content_id) = journal_owner(journal, &target) {
                return Verdict::Journal { content_id };
            }
            if safe_name(name) && loose.contains(&target) {
                return Verdict::Loose { path: target };
            }
        }
        if self.system.iter().any(|d| target == *d || target.starts_with(&format!("{d}/"))) {
            return Verdict::Blocked { reason: removal_system_reason() };
        }
        Verdict::Blocked { reason: removal_outside_reason() }
    }

    /// Whether `path` is the folder of a nested widget of a Conky package.
    fn is_nested_conky(&self, path: &str) -> bool {
        let ctx = crate::de_apply::Ctx::for_home(&self.home);
        crate::de_apply::conky_widget_of_dir(&crate::animations::SystemIo, &ctx, path).is_some_and(|w| w.contains('/'))
    }

    /// Kinds whose part of a theme is `path` of the form
    /// `<theme dir>/<name>/<subfolder>` (the `Arc/xfwm4` border inside a GTK theme,
    /// [`frame_part_target`]); empty — not such a part.
    fn nested_kinds(&self, path: &str) -> Vec<&str> {
        let Some((theme, sub)) = path.rsplit_once('/') else { return Vec::new() };
        let Some((dir, name)) = theme.rsplit_once('/') else { return Vec::new() };
        if !safe_name(name) || !safe_name(sub) {
            return Vec::new();
        }
        self.kinds_at(dir)
            .into_iter()
            .filter(|k| part_subdir(k) == Some(sub))
            .collect()
    }

    /// Whether the theme at `path` is enabled now in at least one kind that looks
    /// for it in this directory. `active_map` is `LiveScan::active_map`.
    /// For a part of a theme (`Arc/xfwm4`) — only in the kinds of that part.
    pub fn in_use(&self, path: &str, active_map: &std::collections::HashMap<String, String>) -> bool {
        let nested = self.nested_kinds(path);
        if !nested.is_empty() {
            let name = path.rsplit_once('/').and_then(|(t, _)| t.rsplit_once('/')).map(|(_, n)| n).unwrap_or("");
            return nested
                .into_iter()
                .any(|kind| crate::installed_scan::active_names(active_map, kind).iter().any(|n| *n == name));
        }
        let Some((parent, file)) = path.rsplit_once('/') else { return false };
        let name = file.strip_suffix(".colors").unwrap_or(file);
        self.kinds_at(parent).into_iter().any(|kind| {
            crate::installed_scan::active_names(active_map, kind)
                .iter()
                .any(|n| *n == name || *n == file)
        })
    }
}

/// Subfolder of a theme part for kind `kind` (`xfwm4`, `metacity-1`,
/// `gnome-shell`, `cinnamon`); `None` — the kind is not a part of a theme folder.
fn part_subdir(kind: &str) -> Option<&'static str> {
    match crate::appearance_installer::de_theme_marker(kind) {
        Some(crate::appearance_installer::DeThemeMarker::Subdir(sub)) => Some(sub),
        _ => None,
    }
}

/// What «Remove» removes for a tile of kind `kind` with folder `install_path`.
///
/// Decision 3c of Wehrwolfmann 20.10.2026: for a window border (and a shell)
/// that lies as a subfolder in a GTK theme folder (`Arc/xfwm4` next to
/// `Arc/gtk-3.0`) — only that subfolder: the GTK theme stays. A folder that has
/// no other folders besides the border subfolder is a separate border: it is
/// removed whole, as before. `subdirs` are the names of folders inside
/// `install_path`.
pub fn frame_part_target(install_path: &str, kind: &str, subdirs: &[String]) -> String {
    let path = install_path.trim_end_matches('/');
    match part_subdir(kind) {
        Some(sub) if subdirs.iter().any(|d| d == sub) && subdirs.iter().any(|d| d != sub && !d.starts_with('.')) => {
            format!("{path}/{sub}")
        }
        _ => path.to_string(),
    }
}

/// Parts installed into the folder `path` SEPARATELY (by another installed log
/// record, not the one that owns the folder itself): a window border that went
/// as a subfolder into a GTK theme. `own_id` is the record being removed (its
/// paths don't count). Returns `(record, name)`.
pub fn separate_parts_inside(journal: &Map<String, Value>, path: &str, own_id: Option<&str>) -> Vec<(String, String)> {
    let path = path.trim_end_matches('/');
    let under = format!("{path}/");
    let paths_of = |rec: &Value| -> Vec<String> {
        rec["paths"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.trim_end_matches('/').to_string())).collect())
            .unwrap_or_default()
    };
    journal
        .iter()
        .filter(|(id, _)| Some(id.as_str()) != own_id)
        .filter(|(_, rec)| {
            let paths = paths_of(rec);
            !paths.iter().any(|p| p == path) && paths.iter().any(|p| p.starts_with(&under))
        })
        .map(|(id, rec)| (id.clone(), rec["name"].as_str().unwrap_or(id).to_string()))
        .collect()
}

/// The question before removing the folder of theme `theme` that holds
/// separately installed parts `parts` ([`separate_parts_inside`]): (title,
/// text). `None` — there are no such parts, nothing to ask.
pub fn separate_parts_question(theme: &str, parts: &[(String, String)]) -> Option<(String, String)> {
    if parts.is_empty() {
        return None;
    }
    let names: Vec<&str> = parts.iter().map(|(_, n)| n.as_str()).collect();
    let title = tr("Remove the theme together with its window borders?");
    // TRANSLATORS: %1 is the theme name, %2 the names of window border themes installed into its folder.
    let text = trf("The folder of “%1” also holds “%2”, installed separately (window borders or a shell \
                    theme). Removing the theme removes them too.",
        &[&theme, &names.join("», «")],
    );
    Some((title, text))
}

/// A directory entry name that is safe to remove.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/')
}

/// Whether anything is at the path (a symlink is not dereferenced).
fn present(p: &str) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// What actually lies on disk under the scan path: the path itself, and for a
/// color scheme (the scan gives the path without the suffix) the file
/// `<path>.colors`.
fn disk_target(path: &str) -> String {
    if !present(path) {
        let colors = format!("{path}.colors");
        if Path::new(&colors).is_file() {
            return colors;
        }
    }
    path.to_string()
}

/// The installed log record whose paths include `path`.
fn journal_owner(journal: &Map<String, Value>, path: &str) -> Option<String> {
    journal.iter().find_map(|(id, rec)| {
        rec["paths"]
            .as_array()?
            .iter()
            .any(|p| p.as_str().map(|s| s.trim_end_matches('/')) == Some(path))
            .then(|| id.clone())
    })
}

// ── Deferred removal ───────────────────────────────────────────────────

/// A theme marked for removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRemoval {
    /// What to remove (`Verdict::Local::path`).
    pub path: String,
    /// Human-readable name (`ThemeItem::name`).
    pub name: String,
    /// Tile number (`ThemeItem::id`) — to remove the sidecar preview.
    pub item_id: String,
}

/// Themes marked for removal while the «Undo» bar is open.
#[derive(Debug, Default)]
pub struct RemovalQueue {
    pending: Vec<PendingRemoval>,
}

impl RemovalQueue {
    /// Mark a theme for removal (marking the same path again changes nothing).
    /// Returns the text of the pop-up bar.
    pub fn mark(&mut self, path: &str, name: &str, item_id: &str) -> String {
        if !self.pending.iter().any(|p| p.path == path) {
            self.pending.push(PendingRemoval {
                path: path.to_string(),
                name: name.to_string(),
                item_id: item_id.to_string(),
            });
        }
        self.toast_text()
    }

    /// «Undo»: clear all marks. Returns how many themes came back.
    pub fn undo_all(&mut self) -> usize {
        std::mem::take(&mut self.pending).len()
    }

    /// The bar closed or expired: hand over the marked items for removal.
    pub fn take_all(&mut self) -> Vec<PendingRemoval> {
        std::mem::take(&mut self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Text of the pop-up bar for the current marks.
    pub fn toast_text(&self) -> String {
        let names: Vec<String> = self.pending.iter().map(|p| p.name.clone()).collect();
        removal_pending_text(&names)
    }

    /// Whether the tile with scan path `install_path` is marked.
    pub fn is_pending(&self, install_path: &str) -> bool {
        let path = install_path.trim_end_matches('/');
        let colors = format!("{path}.colors");
        self.pending.iter().any(|p| p.path == path || p.path == colors)
    }

    /// Remove the tiles of marked themes from the scan response.
    pub fn retain_visible(&self, items: &mut Vec<ThemeItem>) {
        if self.pending.is_empty() {
            return;
        }
        items.retain(|it| it.install_path.is_empty() || !self.is_pending(&it.install_path));
    }
}

/// Remove the marked themes — `(everything removed, message for the status line)`.
///
/// Before removal each theme is checked again: the path is still a direct child
/// of a user theme directory, and the theme is not enabled in any kind (`in_use`,
/// by the live map of what is enabled). The removed paths also go out of the
/// installed log (`log_path`): a record with nothing left on disk is forgotten —
/// the store tile stops calling the theme installed. The sidecar preview is
/// removed along the way.
///
/// A Conky widget (tile `local:conky:…`) is not erased but goes to the trash
/// `home_trash` ([`crate::utils::move_to_trash`]) — like a running one and one
/// found on disk (`conky_scan::stop_and_trash`): a widget may carry the person's
/// edits, and it must be possible to restore it even after the «Undo» bar.
pub fn commit(
    pending: Vec<PendingRemoval>,
    roots: &RemovalRoots,
    in_use: &dyn Fn(&str) -> bool,
    home: &str,
    log_path: &str,
    home_trash: &Path,
) -> (bool, String) {
    let empty = Map::new();
    let mut removed_names: Vec<String> = Vec::new();
    let mut trashed_names: Vec<String> = Vec::new();
    let date = crate::utils::trash_deletion_date_now();
    let mut removed_paths: HashSet<String> = HashSet::new();
    let mut problems: Vec<String> = Vec::new();
    // Store purchases in the shared install journal (step 3d of merging the
    // installers): a recorded theme is removed by the journal — together with its
    // `~/.themes` link; a theme not put in by the store — the former way.
    let journal = crate::install_journal::store_view(log_path);
    for p in pending {
        match roots.verdict(&p.path, false, &empty, &HashSet::new()) {
            Verdict::Local { .. } => {}
            Verdict::Hidden => {
                // Already gone from disk — nothing to remove, but fix the log.
                removed_paths.insert(p.path.clone());
                removed_names.push(p.name);
                continue;
            }
            other => {
                problems.push(format!("{}: {}", p.name, other.tooltip()));
                continue;
            }
        }
        if in_use(&p.path) {
            problems.push(removal_now_active(&p.name));
            continue;
        }
        if p.item_id.starts_with("local:conky:") {
            match crate::utils::move_to_trash(Path::new(&p.path), home_trash, &date) {
                Ok(_) => {
                    remove_sidecar(home, &p.item_id);
                    removed_paths.insert(p.path.clone());
                    trashed_names.push(p.name);
                }
                // TRANSLATORS: %1 is a Conky widget name, %2 the system error.
                Err(e) => problems.push(trf(
                    "Couldn't move “%1” to the trash: %2. The widget stays where it was.",
                    &[&p.name, &e],
                )),
            }
            continue;
        }
        let res = match journal_part(&journal, &p.path) {
            Some(steps) => remove_steps(&steps),
            None => remove_on_disk(&p.path),
        };
        match res {
            Ok(()) => {
                remove_sidecar(home, &p.item_id);
                tidy_after_removal(home, &p.path);
                removed_paths.insert(p.path.clone());
                removed_names.push(p.name);
            }
            // TRANSLATORS: %1 is a file or folder path, %2 is the system error.
            Err(e) => problems.push(trf("Couldn't remove %1: %2.", &[&p.path, &e.to_string()])),
        }
    }
    let mut message = removal_toast_text(&removed_names);
    for name in &trashed_names {
        if !message.is_empty() {
            message.push(' ');
        }
        // TRANSLATORS: %1 is a Conky widget name.
        message.push_str(&trf("🗑️ “%1” moved to the trash", &[name]));
    }
    if !removed_paths.is_empty() {
        if let Err(e) = forget_in_journal(log_path, &removed_paths) {
            if !message.is_empty() {
                message.push(' ');
            }
            message.push_str(&trf(
                "⚠️ The installed-themes log wasn't updated (%1) — the theme will stay \
                 in the list until the next successful write.",
                &[&e],
            ));
        }
    }
    for problem in &problems {
        if !message.is_empty() {
            message.push(' ');
        }
        message.push_str(problem);
    }
    (problems.is_empty(), message)
}

/// Steps to remove the path `path` by the purchase record of the shared journal
/// that recorded it ([`crate::install_journal::part_removal`]): the path itself
/// and its `~/.themes` link. `None` — the path is not from the store.
///
/// The tile is what the person sees on disk and told to remove: if the journal
/// would leave the path itself (what lies there now is not what was put), what
/// lies on disk is removed, as before.
fn journal_part(journal: &crate::install_journal::Journal, path: &str) -> Option<Vec<crate::ext_manager::ExtStep>> {
    use crate::install_journal::{part_removal, probe_live, removal_path, Source};
    let r = journal
        .items
        .values()
        .filter(|e| e.source == Source::Store)
        .find_map(|e| part_removal(e, path, &probe_live))?;
    let path = path.trim_end_matches('/');
    let own = r.steps.iter().any(|st| removal_path(st).is_some_and(|q| q.trim_end_matches('/') == path));
    (own || std::fs::symlink_metadata(path).is_err()).then_some(r.steps)
}

/// Carry out the file removal steps; the first error is a refusal.
fn remove_steps(steps: &[crate::ext_manager::ExtStep]) -> std::io::Result<()> {
    use crate::ext_manager::ExtStep;
    for step in steps {
        match step {
            ExtStep::RemoveTree { path, .. } => remove_on_disk(path)?,
            ExtStep::RemoveFile { path } => remove_on_disk(path)?,
            _ => {}
        }
    }
    Ok(())
}

/// Remove what lies at the path (a directory — whole, a symlink — itself);
/// nothing there — nothing to remove.
fn remove_on_disk(path: &str) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Clean up what a theme leaves next to itself after its path `removed` is
/// removed. ONE behaviour for both removals: «Remove» on the tile ([`commit`])
/// and removal by the purchase log (`appearance_installer::uninstall_from_log`)
/// — previously the log left a dangling link `~/.themes/<theme>` (defect D4,
/// 25.10.2026).
///
/// * a theme from `~/.local/share/themes` — its link in `~/.themes`
///   ([`crate::installed_scan::drop_theme_link`]; doesn't touch someone else's
///   link and theme);
/// * a Cinnamon spice — translations, settings, for a Nemo action — its
///   directory.
///
/// Since step 3d of merging the installers the link of a PURCHASED theme is
/// removed by the shared install journal (it is recorded in the purchase,
/// [`crate::install_journal::Placed::of`]), and removal by the purchase log
/// calls only the spice cleanup from here. The full cleanup stays with the tile:
/// a theme in `~/.local/share/themes` put in not by the store (by hand, by
/// another installer) is not recorded in the journal, while `sync_theme_links`
/// gives it a link anyway; spice translations and settings are not written to
/// the journal at all.
pub fn tidy_after_removal(home: &str, removed: &str) {
    crate::installed_scan::drop_theme_link(home, removed);
    crate::cinnamon_spices::cleanup_after_removal(&crate::cinnamon_spices::SpiceEnv::for_home(home), removed);
}

/// Remove the sidecar preview of tile `local:<kind>:<name>` (the cached picture
/// of a theme without its own preview). No file — nothing to do.
fn remove_sidecar(home: &str, item_id: &str) {
    let Some((kind, name)) = item_id.strip_prefix("local:").and_then(|r| r.split_once(':')) else {
        return;
    };
    let _ = std::fs::remove_file(sidecar_preview_path(home, kind, name));
}

/// Remove the removed paths from the installed log; forget a record without a
/// single path on disk. No records or the log did not change — write nothing.
/// The shared install journal is written, `ocs_installed.json` is its mirror
/// (step 5b of merging the installers).
fn forget_in_journal(log_path: &str, removed: &HashSet<String>) -> Result<(), String> {
    let mut data = crate::install_journal::store_log_view(log_path);
    let mut changed = false;
    let mut forget: Vec<String> = Vec::new();
    for (id, rec) in data.iter_mut() {
        let Some(paths) = rec.get_mut("paths").and_then(Value::as_array_mut) else { continue };
        let before = paths.len();
        // A removed theme folder also takes away the parts that went into it as subfolders.
        paths.retain(|v| v.as_str().map(|s| !removed_covers(removed, s)).unwrap_or(true));
        if paths.len() != before {
            changed = true;
            let left = paths.iter().filter_map(Value::as_str).any(present);
            if !left {
                forget.push(id.clone());
            }
        }
    }
    if !changed {
        return Ok(());
    }
    for id in forget {
        crate::ocs_store::forget_installed(&mut data, &id);
    }
    crate::install_journal::put_store_log(log_path, &data).map_err(|e| e.to_string())
}

/// Whether the path `p` was removed — by itself or together with the folder it was in.
fn removed_covers(removed: &HashSet<String>, p: &str) -> bool {
    let p = p.trim_end_matches('/');
    removed.contains(p) || removed.iter().any(|r| p.starts_with(&format!("{r}/")))
}

/// A tile of a built-in KDE border ([`crate::kdecoration`]) that is not
/// removed: everything that lies outside the person's home.
fn builtin_decoration_without_removal(item: &ThemeItem, home: &str) -> bool {
    let builtin = item
        .id
        .strip_prefix("local:aurorae:")
        .and_then(crate::kdecoration::parse_entry)
        .is_some();
    let home = format!("{}/", home.trim_end_matches('/'));
    builtin && !item.install_path.starts_with(&home)
}

// ── Live wrappers ──────────────────────────────────────────────────────

fn live_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| ".".to_string())
}

/// The installed log of this machine.
pub fn live_log_path() -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    crate::ocs_store::install_log_path_xdg(xdg.as_deref(), &live_home())
}

/// Decisions for the tiles of one scan: the directories and the log are read once.
pub struct LiveRemoval {
    roots: RemovalRoots,
    journal: Map<String, Value>,
    /// System themes without an owner package; the package database is asked once
    /// and only when a tile from their directories comes up.
    loose: std::cell::OnceCell<HashSet<String>>,
}

/// Whether a tile needs the answer "which login screen and boot splash themes
/// have no owner package"
/// ([`crate::installed_scan::LiveScan::loose_system_themes`]).
///
/// The answer is expensive: one `pacman -Qo` over all SDDM and Plymouth theme
/// directories reads the file lists of all packages. On 08.11.2026 it took 5 s
/// on the bench, and on the live machine under disk load the «Active» section
/// hung on "Loading…" for 20–60 s because of it — for the sake of an enabled
/// boot splash whose «Remove» is closed anyway ("in use now",
/// [`RemovalRoots::verdict`] answers before looking at the answer).
fn loose_needed(in_helper_dir: bool, active: bool) -> bool {
    in_helper_dir && !active
}

impl LiveRemoval {
    pub fn new() -> Self {
        LiveRemoval {
            roots: RemovalRoots::new(&live_home(), "/usr"),
            journal: crate::install_journal::store_log_view(&live_log_path()),
            loose: std::cell::OnceCell::new(),
        }
    }

    /// Decision for a tile.
    pub fn verdict(&self, item: &ThemeItem) -> Verdict {
        // A Conky widget from «🔎 Found on disk» (the name is the path): its own rules,
        // and a running one is removed too — it stops first.
        let found = item.id.strip_prefix("local:conky:").is_some_and(crate::conky_scan::is_found_name);
        if found {
            return match crate::conky_scan::live_found_removal(&item.install_path) {
                Ok(Some(r)) => Verdict::FoundConky { path: r.path },
                Ok(None) => Verdict::Hidden,
                Err(reason) => Verdict::Blocked { reason },
            };
        }
        // A built-in KDE border (`local:aurorae:<module>/<theme>`): a Qt module
        // (Breeze, Oxygen) and a system QML theme package come with Plasma; KDE has no
        // remove button for them — and neither do we.
        if builtin_decoration_without_removal(item, &live_home()) {
            return Verdict::Hidden;
        }
        let own_conky = item.id.starts_with("local:conky:");
        let active = item.local_kind.as_deref() == Some("active");
        if own_conky && active {
            let base = self.roots.verdict(&item.install_path, false, &self.journal, &HashSet::new());
            return running_conky_verdict(base);
        }
        let none = HashSet::new();
        let path = item.install_path.trim_end_matches('/');
        let in_helper_dir = path
            .rsplit_once('/')
            .is_some_and(|(parent, _)| self.roots.helper.iter().any(|d| d == parent));
        let loose = if loose_needed(in_helper_dir, active) {
            self.loose.get_or_init(|| {
                crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).loose_system_themes()
            })
        } else {
            &none
        };
        // A border that went as a subfolder into a GTK theme: only the subfolder is removed.
        let kind = item.id.strip_prefix("local:").and_then(|r| r.split_once(':')).map(|(k, _)| k).unwrap_or("");
        let target = if part_subdir(kind).is_some() {
            let subdirs: Vec<String> = std::fs::read_dir(path)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| e.path().is_dir())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            frame_part_target(path, kind, &subdirs)
        } else {
            path.to_string()
        };
        self.roots.verdict(&target, active, &self.journal, loose)
    }

    /// The question before removing the folder `path` (the tile's «Remove»
    /// decision) if it holds separately installed parts ([`separate_parts_question`]).
    pub fn separate_parts_question(&self, path: &str, theme: &str) -> Option<(String, String)> {
        separate_parts_question(theme, &separate_parts_inside(&self.journal, path, None))
    }
}

/// The question before «Remove» of purchase `content_id` by the log: its folders
/// hold separately installed parts ([`separate_parts_question`]).
pub fn live_uninstall_question(content_id: &str) -> Option<(String, String)> {
    let journal = crate::install_journal::store_log_view(&live_log_path());
    let rec = journal.get(content_id)?;
    let theme = rec["name"].as_str().unwrap_or(content_id).to_string();
    let mut parts: Vec<(String, String)> = Vec::new();
    for p in rec["paths"].as_array()?.iter().filter_map(Value::as_str) {
        for part in separate_parts_inside(&journal, p, Some(content_id)) {
            if !parts.contains(&part) {
                parts.push(part);
            }
        }
    }
    separate_parts_question(&theme, &parts)
}

/// An own Conky widget is running: what would be a regular «Remove» of an own
/// theme (`base` — the decision as for a non-enabled one) becomes "stop and
/// remove"; other decisions (no button, disabled) stay as they are.
fn running_conky_verdict(base: Verdict) -> Verdict {
    match base {
        Verdict::Local { path } => Verdict::RunningConky { path },
        other => other,
    }
}

/// Remove an own running Conky widget `path` (tile `item_id`): its processes
/// are stopped by PID, the autostart entry goes away, the folder goes to the
/// trash (`conky_scan::live_remove_running_own`); then, as in [`commit`], the
/// path leaves the installed log and the sidecar preview is removed.
pub fn live_remove_running_conky(path: &str, item_id: &str) -> (bool, String) {
    let (ok, msg) = crate::conky_scan::live_remove_running_own(path);
    if ok {
        remove_sidecar(&live_home(), item_id);
        let gone: HashSet<String> = std::iter::once(path.trim_end_matches('/').to_string()).collect();
        if let Err(e) = forget_in_journal(&live_log_path(), &gone) {
            crate::log_warn!("theme", "журнал установленных не обновлён после снятия виджета: {e}");
        }
    }
    (ok, msg)
}

impl Default for LiveRemoval {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove the marked items on this machine. `de` is the tab's desktop (Rexon
/// key): by it the map of what is enabled knows the desktop's sections. Reads
/// the desktop settings (may call `gsettings`) — not from the UI thread.
pub fn live_commit(pending: Vec<PendingRemoval>, de: &str) -> (bool, String) {
    if pending.is_empty() {
        return (true, String::new());
    }
    let home = live_home();
    let roots = RemovalRoots::new(&home, "/usr");
    let mut scan = crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).with_de(de);
    // The login screen has nothing to do with removing own themes — `systemctl` is not called.
    scan.plasma_login = Some(false);
    let active = scan.active_map();
    let paths: Vec<String> = pending.iter().map(|p| p.path.clone()).collect();
    let xdg_data = std::env::var("XDG_DATA_HOME").ok();
    let trash = crate::utils::home_trash_dir(xdg_data.as_deref(), &home);
    let outcome = commit(pending, &roots, &|p| roots.in_use(p, &active), &home, &live_log_path(), Path::new(&trash));
    // A theme from a Windows file was also installed as an extension — its entry
    // is removed together with it, otherwise it stays in the extensions directory
    // forever.
    let gone: Vec<String> = paths.into_iter().filter(|p| !present(p)).collect();
    // A removed Conky widget from the Rexon directory: its processes are stopped
    // (by PID), the autostart entry goes away — otherwise on login conky would call
    // a file that doesn't exist. Not a widget — nothing is done.
    let ctx = crate::de_apply::Ctx::live();
    for p in &gone {
        crate::de_apply::conky_forget_widget(&crate::animations::SystemIo, &ctx, p);
    }
    for id in crate::ext_exec::forget_extensions_of_removed_themes(&gone) {
        crate::log_info!("theme", "вместе с темой снята запись расширения: {id}");
    }
    outcome
}

// ── «Remove» on a store item ────────────────────────────────────────────

/// Paths of purchase `content_id` on disk: from the installed log (KDE Store,
/// Cinnamon Spices; former `github:…` records — likewise). For a Cinnamon spice
/// also its directory: a theme installed by the Cinnamon settings window is not
/// in the log, yet it has «Remove» in the storefront.
pub fn store_paths(journal: &Map<String, Value>, content_id: &str, home: &str) -> Vec<String> {
    let mut out: Vec<String> = journal
        .get(content_id)
        .and_then(|r| r["paths"].as_array())
        .map(|a| a.iter().filter_map(Value::as_str).map(|p| p.trim_end_matches('/').to_string()).collect())
        .unwrap_or_default();
    if let Some((t, key)) = crate::cinnamon_spices::parse_item_id(content_id) {
        let dir = format!("{}/{key}", crate::cinnamon_spices::install_root(&crate::cinnamon_spices::SpiceEnv::for_home(home), t));
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// «Remove» on a store item (the details button): the theme enabled now cannot
/// be removed — the same check as for a tile of the installed grid
/// ([`RemovalRoots::in_use`]: GTK theme, xfwm4 and Marco borders, GNOME and
/// Cinnamon shells, icons, cursors, colors…), by the purchase paths.
/// `Some(reason)` — the button is disabled; `None` — removal is allowed.
pub fn store_blocked(paths: &[String], in_use: &dyn Fn(&str) -> bool) -> Option<String> {
    paths.iter().any(|p| in_use(p)).then(removal_active_reason)
}

/// [`store_blocked`] on this machine for purchase `content_id`. `de` is the
/// tab's desktop (as in [`live_commit`]). Reads the desktop settings (may call
/// `gsettings`) — not from the UI thread.
pub fn live_store_blocked(content_id: &str, de: &str) -> Option<String> {
    let home = live_home();
    let journal = crate::install_journal::store_log_view(&live_log_path());
    let paths: Vec<String> = store_paths(&journal, content_id, &home).into_iter().filter(|p| present(p)).collect();
    if paths.is_empty() {
        return None;
    }
    let roots = RemovalRoots::new(&home, "/usr");
    let mut scan = crate::installed_scan::LiveScan::new(Vec::new(), Default::default()).with_de(de);
    scan.plasma_login = Some(false);
    let active = scan.active_map();
    store_blocked(&paths, &|p| roots.in_use(p, &active))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Temporary HOME and a fake /usr — the live profile is not touched.
    fn sandbox(tag: &str) -> (PathBuf, String, String) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!("rexon_removal_{tag}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home = root.join("home").to_string_lossy().into_owned();
        let usr = root.join("usr").to_string_lossy().into_owned();
        (root, home, usr)
    }

    fn mkdir(p: &str) {
        std::fs::create_dir_all(p).unwrap();
    }

    fn touch(p: &str) {
        mkdir(Path::new(p).parent().unwrap().to_str().unwrap());
        std::fs::write(p, "x").unwrap();
    }

    fn item(id: &str, name: &str, path: &str, active: bool) -> ThemeItem {
        ThemeItem {
            id: id.to_string(),
            name: name.to_string(),
            install_path: path.to_string(),
            local_kind: active.then(|| "active".to_string()),
            ..Default::default()
        }
    }

    /// An own Conky widget: not running — the regular deferred «Remove»; running —
    /// not a refusal but "stop and remove"; disabled and missing stay as they were.
    #[test]
    fn running_own_conky_is_stop_and_remove_not_refusal() {
        let (_root, home, usr) = sandbox("conky-running");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        let w = format!("{home}/.config/conky/Jhalak");
        touch(&format!("{w}/Jhalak.conf"));
        let idle = roots.verdict(&w, false, &journal, &HashSet::new());
        assert_eq!(idle, Verdict::Local { path: w.clone() });
        let running = running_conky_verdict(roots.verdict(&w, false, &journal, &HashSet::new()));
        assert_eq!(running, Verdict::RunningConky { path: w.clone() });
        assert!(running.enabled() && running.visible() && running.tooltip().is_empty());
        let blocked = Verdict::Blocked { reason: removal_outside_reason() };
        assert_eq!(running_conky_verdict(blocked.clone()), blocked);
        assert_eq!(running_conky_verdict(Verdict::Hidden), Verdict::Hidden);
    }

    #[test]
    fn own_themes_in_every_user_folder_are_removable() {
        let (_root, home, usr) = sandbox("own");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        for dir in [
            format!("{home}/.local/share/icons/Papirus"),
            format!("{home}/.icons/Bibata"),
            format!("{home}/.themes/Arc"),
            format!("{home}/.local/share/themes/Arc-Dark"),
            format!("{home}/.local/share/plasma/look-and-feel/org.x.desktop"),
            format!("{home}/.local/share/plasma/desktoptheme/Sweet"),
            format!("{home}/.local/share/aurorae/themes/Win7"),
            format!("{home}/.local/share/sounds/ocean"),
            format!("{home}/.config/Kvantum/KvArc"),
        ] {
            mkdir(&dir);
            assert_eq!(roots.verdict(&dir, false, &journal, &HashSet::new()), Verdict::Local { path: dir.clone() }, "{dir}");
        }
        // Color scheme: the scan gives the path without the suffix, the file is removed.
        let colors = format!("{home}/.local/share/color-schemes/Nord");
        touch(&format!("{colors}.colors"));
        assert_eq!(
            roots.verdict(&colors, false, &journal, &HashSet::new()),
            Verdict::Local { path: format!("{colors}.colors") }
        );
    }

    /// An enabled login screen or splash theme doesn't need the expensive owner
    /// query: «Remove» is closed for it whatever the package database answers.
    #[test]
    fn active_helper_theme_skips_the_owner_query() {
        assert!(!loose_needed(true, true));
        assert!(loose_needed(true, false));
        assert!(!loose_needed(false, false));
        let (_, home, usr) = sandbox("active-ply");
        let roots = RemovalRoots::new(&home, &usr);
        let ply = format!("{usr}/share/plymouth/themes/cachyos-bootanimation");
        mkdir(&ply);
        let loose: HashSet<String> = [ply.clone()].into_iter().collect();
        // The same answer the full query would give.
        assert_eq!(
            roots.verdict(&ply, true, &Map::new(), &HashSet::new()),
            roots.verdict(&ply, true, &Map::new(), &loose)
        );
    }

    #[test]
    fn active_system_and_outside_are_blocked_with_reason() {
        let (root, home, usr) = sandbox("blocked");
        let roots = RemovalRoots::new(&home, &usr);
        let journal = Map::new();
        let own = format!("{home}/.local/share/icons/Papirus");
        mkdir(&own);
        let active = roots.verdict(&own, true, &journal, &HashSet::new());
        assert_eq!(active, Verdict::Blocked { reason: removal_active_reason() });
        assert!(active.visible() && !active.enabled());
        assert_eq!(active.tooltip(), removal_active_reason());

        let sys = format!("{usr}/share/icons/breeze");
        mkdir(&sys);
        assert_eq!(roots.verdict(&sys, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_system_reason() });
        // Plymouth not from the log — also a system one, the helper is not called.
        let ply = format!("{usr}/share/plymouth/themes/spinner");
        mkdir(&ply);
        assert_eq!(roots.verdict(&ply, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_system_reason() });

        let outside = root.join("elsewhere/Theme").to_string_lossy().into_owned();
        mkdir(&outside);
        assert_eq!(roots.verdict(&outside, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_outside_reason() });
        // A theme directory nested deeper is not a theme of the theme directory.
        let nested = format!("{home}/.local/share/icons/Papirus/scalable");
        mkdir(&nested);
        assert_eq!(roots.verdict(&nested, false, &journal, &HashSet::new()), Verdict::Blocked { reason: removal_outside_reason() });
        // The theme directory itself is not either.
        let base = format!("{home}/.local/share/icons");
        assert!(!roots.verdict(&base, false, &journal, &HashSet::new()).enabled());
        // A tile without a path (Qt style) — no button.
        assert_eq!(roots.verdict("", false, &journal, &HashSet::new()), Verdict::Hidden);
    }

    #[test]
    fn rexon_installed_plymouth_goes_through_journal() {
        let (_root, home, usr) = sandbox("journal");
        let roots = RemovalRoots::new(&home, &usr);
        let ply = format!("{usr}/share/plymouth/themes/neon");
        mkdir(&ply);
        let journal: Map<String, Value> =
            serde_json::from_value(serde_json::json!({"777": {"name": "Neon", "paths": [ply.clone()]}})).unwrap();
        assert_eq!(roots.verdict(&ply, false, &journal, &HashSet::new()), Verdict::Journal { content_id: "777".into() });
        // A theme without a log record and without an owner package — by the helper by
        // path; the one enabled now is not removed either.
        let hand = format!("{usr}/share/plymouth/themes/by-hand");
        mkdir(&hand);
        let loose: HashSet<String> = [hand.clone()].into();
        assert_eq!(roots.verdict(&hand, false, &journal, &loose), Verdict::Loose { path: hand.clone() });
        assert!(!roots.verdict(&hand, true, &journal, &loose).enabled());
        assert_eq!(
            roots.verdict(&hand, false, &journal, &HashSet::new()),
            Verdict::Blocked { reason: removal_system_reason() }
        );
        // The enabled one is not touched via the log either.
        assert!(!roots.verdict(&ply, true, &journal, &HashSet::new()).enabled());
    }

    /// The captions of the bar and of the «Remove» hints don't call the removed item
    /// a theme: GNOME extensions, widgets, icons and fonts go through them too (live
    /// check 15.11: "The theme “Caffeine” will be removed").
    #[test]
    fn removal_texts_name_no_kind() {
        let one = vec!["Caffeine".to_string()];
        let two = vec!["Caffeine".to_string(), "Papirus".to_string()];
        assert_eq!(removal_pending_text(&one), "Removing “Caffeine”.");
        assert_eq!(removal_pending_text(&two), "Removing 2 items.");
        assert_eq!(removal_toast_text(&one), "Removed “Caffeine”.");
        assert_eq!(removal_toast_text(&two), "2 items removed.");
        assert_eq!(removal_pending_text(&[]), "");
        assert_eq!(removal_toast_text(&[]), "");
        // All tile kinds see the bar — the queue has no branching by kind.
        for id in ["local:gnomeext:caffeine@patapon.info", "local:conky:Clock", "local:icons:Papirus", "local:cursor:Bibata"] {
            let mut q = RemovalQueue::default();
            assert_eq!(q.mark("/h/x", "Caffeine", id), removal_pending_text(&one), "{id}");
        }
        let texts = [
            removal_pending_text(&one),
            removal_pending_text(&two),
            removal_toast_text(&one),
            removal_toast_text(&two),
            removal_now_active("Caffeine"),
            removal_active_reason(),
            removal_system_reason(),
            removal_outside_reason(),
            removal_countdown_text(4),
        ];
        for t in texts {
            assert!(!t.to_lowercase().contains("theme"), "{t}");
        }
    }

    #[test]
    fn queue_marks_hides_and_undoes() {
        let mut q = RemovalQueue::default();
        let mut items = vec![
            item("local:icons:A", "A", "/h/.local/share/icons/A", false),
            item("local:colors:Nord", "Nord", "/h/.local/share/color-schemes/Nord", false),
            item("local:widgetstyle:Fusion", "Fusion", "", false),
        ];
        assert_eq!(q.mark("/h/.local/share/icons/A", "A", "local:icons:A"), removal_pending_text(&["A".into()]));
        let text = q.mark("/h/.local/share/color-schemes/Nord.colors", "Nord", "local:colors:Nord");
        assert_eq!(text, removal_pending_text(&["A".into(), "Nord".into()]));
        // Marking again adds nothing.
        q.mark("/h/.local/share/icons/A", "A", "local:icons:A");
        q.retain_visible(&mut items);
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["local:widgetstyle:Fusion"]);
        assert_eq!(q.undo_all(), 2);
        assert!(q.is_empty() && !q.is_pending("/h/.local/share/icons/A"));
    }

    #[test]
    fn commit_removes_only_unused_own_themes_and_cleans_journal() {
        let (_root, home, usr) = sandbox("commit");
        let roots = RemovalRoots::new(&home, &usr);
        let icons = format!("{home}/.local/share/icons/Papirus");
        touch(&format!("{icons}/index.theme"));
        let gtk = format!("{home}/.themes/Arc");
        touch(&format!("{gtk}/gtk-3.0/gtk.css"));
        let colors = format!("{home}/.local/share/color-schemes/Nord.colors");
        touch(&colors);
        // Symlink: the link itself is removed, the target stays.
        let target = format!("{home}/keep/Bibata");
        touch(&format!("{target}/index.theme"));
        let link = format!("{home}/.icons/Bibata");
        mkdir(&format!("{home}/.icons"));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        // Sidecar preview of a color scheme.
        let side = sidecar_preview_path(&home, "colors", "Nord");
        touch(&side);

        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        std::fs::write(
            &log,
            serde_json::json!({
                "1": {"name": "Papirus", "paths": [icons.clone()]},
                "2": {"name": "Pack", "paths": [colors.clone(), format!("{home}/.local/share/plasma/desktoptheme/Keep")]},
            })
            .to_string(),
        )
        .unwrap();
        mkdir(&format!("{home}/.local/share/plasma/desktoptheme/Keep"));

        // Arc became the xfwm4 border theme in the same folder — it is enabled.
        let mut active = HashMap::new();
        active.insert("xfwm4".to_string(), "Arc".to_string());
        let kinds = roots.kinds_at(&format!("{home}/.themes"));
        assert!(kinds.contains(&"gtk"), "{kinds:?}");
        let pend = |path: &str, name: &str, id: &str| PendingRemoval {
            path: path.into(),
            name: name.into(),
            item_id: id.into(),
        };
        let in_use = |p: &str| p == gtk || roots.in_use(p, &active);
        let (ok, msg) = commit(
            vec![
                pend(&icons, "Papirus", "local:icons:Papirus"),
                pend(&gtk, "Arc", "local:gtk:Arc"),
                pend(&colors, "Nord", "local:colors:Nord"),
                pend(&link, "Bibata", "local:cursor:Bibata"),
            ],
            &roots,
            &in_use,
            &home,
            &log,
            Path::new(&format!("{home}/.local/share/Trash")),
        );
        assert!(!ok, "{msg}");
        assert!(msg.contains(&removal_now_active("Arc")), "{msg}");
        assert!(msg.starts_with(&removal_toast_text(&["Papirus".into(), "Nord".into(), "Bibata".into()])), "{msg}");
        assert!(!present(&icons) && !present(&colors) && !present(&link) && !present(&side));
        assert!(present(&gtk), "включённая тема осталась");
        assert!(present(&target), "цель ссылки не тронута");

        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.get("1").is_none(), "запись без путей забыта");
        assert_eq!(
            data["2"]["paths"],
            serde_json::json!([format!("{home}/.local/share/plasma/desktoptheme/Keep")]),
            "у набора остался живой путь"
        );
    }

    #[test]
    fn commit_refuses_path_that_left_user_folders() {
        let (_root, home, usr) = sandbox("refuse");
        let roots = RemovalRoots::new(&home, &usr);
        let sys = format!("{usr}/share/icons/breeze");
        mkdir(&sys);
        let (ok, msg) = commit(
            vec![PendingRemoval { path: sys.clone(), name: "Breeze".into(), item_id: String::new() }],
            &roots,
            &|_| false,
            &home,
            &format!("{home}/nolog.json"),
            Path::new(&format!("{home}/.local/share/Trash")),
        );
        assert!(!ok && msg.contains(&removal_system_reason()), "{msg}");
        assert!(present(&sys));
    }

    /// Decision 3c: a border that went as a subfolder into a GTK theme is removed
    /// alone — the GTK theme stays; a GTK theme with a separately installed border
    /// inside asks and takes the border's record out of the log.
    #[test]
    fn frame_inside_gtk_theme_is_removed_alone() {
        let (_root, home, usr) = sandbox("frame-part");
        let roots = RemovalRoots::new(&home, &usr);
        let theme = format!("{home}/.local/share/themes/Arc");
        touch(&format!("{theme}/gtk-3.0/gtk.css"));
        touch(&format!("{theme}/metacity-1/metacity-theme-3.xml"));
        let subdirs = vec!["gtk-3.0".to_string(), "metacity-1".to_string()];
        let part = frame_part_target(&theme, "marco", &subdirs);
        assert_eq!(part, format!("{theme}/metacity-1"));
        // A separate border and the GTK theme itself — the whole folder.
        assert_eq!(frame_part_target(&theme, "marco", &["metacity-1".to_string()]), theme);
        assert_eq!(frame_part_target(&theme, "gtk", &subdirs), theme);

        let journal: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "10": {"name": "Arc GTK", "paths": [theme.clone()]},
            "11": {"name": "Arc Marco", "paths": [part.clone()]},
        }))
        .unwrap();
        assert_eq!(roots.verdict(&part, false, &journal, &HashSet::new()), Verdict::Local { path: part.clone() });
        // The GTK theme is enabled, not the border: the border can be removed.
        let mut active = HashMap::new();
        active.insert("gtk".to_string(), "Arc".to_string());
        assert!(!roots.in_use(&part, &active));
        assert!(roots.in_use(&theme, &active));
        active.insert("marco".to_string(), "Arc".to_string());
        assert!(roots.in_use(&part, &active));

        // Question: the GTK theme has a separate border inside; the border itself doesn't.
        let inside = separate_parts_inside(&journal, &theme, None);
        assert_eq!(inside, [("11".to_string(), "Arc Marco".to_string())]);
        assert!(separate_parts_question("Arc", &inside).is_some_and(|(_, t)| t.contains("Arc Marco")));
        assert!(separate_parts_inside(&journal, &part, None).is_empty());
        assert!(separate_parts_inside(&journal, &theme, Some("11")).is_empty());

        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        std::fs::write(&log, Value::Object(journal).to_string()).unwrap();
        let pend = |path: &str, id: &str| PendingRemoval { path: path.into(), name: "Arc".into(), item_id: id.into() };
        let (ok, msg) = commit(vec![pend(&part, "local:marco:Arc")], &roots, &|_| false, &home, &log, Path::new(&format!("{home}/.local/share/Trash")));
        assert!(ok, "{msg}");
        assert!(!present(&part) && present(&format!("{theme}/gtk-3.0/gtk.css")), "снята только рамка");
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.get("11").is_none() && data.get("10").is_some());

        // The whole GTK theme also takes away the record of the border that lay inside.
        touch(&format!("{theme}/metacity-1/metacity-theme-3.xml"));
        std::fs::write(
            &log,
            serde_json::json!({"10": {"name": "Arc GTK", "paths": [theme.clone()]}, "11": {"name": "Arc Marco", "paths": [part.clone()]}})
                .to_string(),
        )
        .unwrap();
        let (ok, msg) = commit(vec![pend(&theme, "local:gtk:Arc")], &roots, &|_| false, &home, &log, Path::new(&format!("{home}/.local/share/Trash")));
        assert!(ok, "{msg}");
        assert!(!present(&theme));
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert!(data.is_empty(), "{data:?}");
    }

    /// Step 3d: «Remove» on the tile of a GTK theme from the store removes it by the
    /// shared install journal — the theme and its `~/.themes` link, while the colors
    /// of the same purchase stay (and the path in the store log). A theme put in not
    /// by the store is removed the former way — its link is removed by the cleanup
    /// (`drop_theme_link`: the journal doesn't know it).
    #[test]
    fn store_gtk_tile_removal_goes_by_journal_with_its_link() {
        let (_root, home, usr) = sandbox("journal-tile");
        let roots = RemovalRoots::new(&home, &usr);
        let xdg = format!("{home}/.local/share/themes");
        touch(&format!("{xdg}/Nord/gtk-3.0/gtk.css"));
        touch(&format!("{xdg}/Hand/gtk-3.0/gtk.css"));
        let colors = format!("{home}/.local/share/color-schemes/Nord.colors");
        touch(&colors);
        assert_eq!(crate::installed_scan::sync_theme_links(&home).len(), 2);
        let log = format!("{home}/.config/rexon/ocs_installed.json");
        touch(&log);
        let theme = format!("{xdg}/Nord");
        std::fs::write(&log, serde_json::json!({"10": {"name": "Nord", "rexon_type": "gtk", "paths": [theme.clone(), colors.clone()]}}).to_string()).unwrap();
        let journal = crate::install_journal::store_view(&log);
        assert!(journal_part(&journal, &theme).is_some_and(|st| st.len() == 2), "тема и ссылка — по журналу");
        assert!(journal_part(&journal, &format!("{xdg}/Hand")).is_none(), "тема не из магазина — не по журналу");

        let pend = |path: &str, id: &str| PendingRemoval { path: path.into(), name: "x".into(), item_id: id.into() };
        let trash = format!("{home}/.local/share/Trash");
        let (ok, msg) = commit(
            vec![pend(&theme, "local:gtk:Nord"), pend(&format!("{xdg}/Hand"), "local:gtk:Hand")],
            &roots,
            &|_| false,
            &home,
            &log,
            Path::new(&trash),
        );
        assert!(ok, "{msg}");
        for name in ["Nord", "Hand"] {
            assert!(!present(&format!("{xdg}/{name}")), "{name}: тема осталась");
            assert!(std::fs::symlink_metadata(format!("{home}/.themes/{name}")).is_err(), "{name}: ссылка ~/.themes осталась");
        }
        assert!(present(&colors), "снята другая часть покупки");
        let data = crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref());
        assert_eq!(data["10"]["paths"], serde_json::json!([colors]), "{data:?}");
    }

    /// An own non-enabled Conky widget goes after the «Undo» bar to the freedesktop
    /// trash (the file and a `.trashinfo` record) instead of being erased — like a
    /// running one and one found on disk; a theme next to it is removed as before.
    #[test]
    fn own_conky_widget_goes_to_trash() {
        let (_root, home, usr) = sandbox("conky-trash");
        let roots = RemovalRoots::new(&home, &usr);
        let widget = format!("{home}/.config/conky/RexonProba");
        touch(&format!("{widget}/conky.conf"));
        let icons = format!("{home}/.local/share/icons/Papirus");
        touch(&format!("{icons}/index.theme"));
        assert_eq!(
            roots.verdict(&widget, false, &Map::new(), &HashSet::new()),
            Verdict::Local { path: widget.clone() }
        );
        let trash = format!("{home}/.local/share/Trash");
        let pend = |path: &str, name: &str, id: &str| PendingRemoval { path: path.into(), name: name.into(), item_id: id.into() };
        let (ok, msg) = commit(
            vec![pend(&widget, "RexonProba", "local:conky:RexonProba"), pend(&icons, "Papirus", "local:icons:Papirus")],
            &roots,
            &|_| false,
            &home,
            &format!("{home}/nolog.json"),
            Path::new(&trash),
        );
        assert!(ok, "{msg}");
        assert!(!present(&widget) && !present(&icons));
        assert!(present(&format!("{trash}/files/RexonProba/conky.conf")), "виджет в корзине");
        let info = std::fs::read_to_string(format!("{trash}/info/RexonProba.trashinfo")).unwrap();
        assert!(info.contains("Path=") && info.contains("RexonProba"), "{info}");
        assert!(!present(&format!("{trash}/files/Papirus")), "тема стирается, как прежде");
        assert!(msg.contains(&trf("🗑️ “%1” moved to the trash", &[&"RexonProba"])), "{msg}");
        assert!(msg.contains(&removal_toast_text(&["Papirus".into()])), "{msg}");
    }

    /// «Remove» on a store item: the same protection of the enabled theme as for the
    /// tile — by the purchase paths from the log, and for a Cinnamon spice also by
    /// its directory (one installed by the Cinnamon settings window is not in the
    /// log).
    #[test]
    fn store_item_removal_is_blocked_while_its_theme_is_on() {
        let roots = RemovalRoots::new("/h", "/u");
        let journal: Map<String, Value> = serde_json::from_value(serde_json::json!({
            "1234": {"name": "Arc", "paths": ["/h/.themes/Arc/"]},
            "spices:theme:CBlack": {"name": "CBlack", "paths": ["/h/.themes/CBlack"]},
        }))
        .unwrap();
        assert_eq!(store_paths(&journal, "1234", "/h"), ["/h/.themes/Arc"]);
        assert_eq!(store_paths(&journal, "spices:theme:CBlack", "/h"), ["/h/.themes/CBlack"], "без повтора");
        assert_eq!(store_paths(&journal, "spices:theme:Mint-Y", "/h"), ["/h/.themes/Mint-Y"], "не из журнала — по каталогу");
        assert!(store_paths(&journal, "999", "/h").is_empty());
        // Every kind that looks for themes in ~/.themes holds the theme: GTK, XFCE and
        // MATE borders, GNOME and Cinnamon shells.
        for kind in ["gtk", "xfwm4", "marco", "gnomeshell", "cinnamontheme"] {
            let active = HashMap::from([(kind.to_string(), "CBlack".to_string())]);
            let paths = store_paths(&journal, "spices:theme:CBlack", "/h");
            assert_eq!(store_blocked(&paths, &|p| roots.in_use(p, &active)), Some(removal_active_reason()), "{kind}");
            let other = store_paths(&journal, "1234", "/h");
            assert_eq!(store_blocked(&other, &|p| roots.in_use(p, &active)), None, "{kind}: чужая тема не держит");
        }
        // Icons from the KDE Store — the same path.
        let icons: Map<String, Value> =
            serde_json::from_value(serde_json::json!({"7": {"paths": ["/h/.local/share/icons/Papirus"]}})).unwrap();
        let active = HashMap::from([("icons".to_string(), "Papirus".to_string())]);
        assert!(store_blocked(&store_paths(&icons, "7", "/h"), &|p| roots.in_use(p, &active)).is_some());
    }

    #[test]
    fn in_use_checks_every_kind_of_the_folder() {
        let roots = RemovalRoots::new("/h", "/u");
        let mut active = HashMap::new();
        active.insert("cursor".to_string(), "Bibata".to_string());
        active.insert("colors".to_string(), "Nord".to_string());
        // The cursor lies in the icons directory — enabled as a cursor.
        assert!(roots.in_use("/h/.local/share/icons/Bibata", &active));
        assert!(roots.in_use("/h/.icons/Bibata", &active));
        assert!(roots.in_use("/h/.local/share/color-schemes/Nord.colors", &active));
        assert!(!roots.in_use("/h/.local/share/icons/Papirus", &active));
        // The splash from a global theme — the same look-and-feel package.
        active.insert("splash".to_string(), "org.x.desktop".to_string());
        assert!(roots.in_use("/h/.local/share/plasma/look-and-feel/org.x.desktop", &active));
    }

    #[test]
    fn builtin_kde_decorations_have_no_remove_button() {
        let tile = |id: &str, path: &str| ThemeItem { id: id.into(), install_path: path.into(), ..Default::default() };
        let home = "/h";
        let breeze = tile("local:aurorae:org.kde.breeze/Breeze", "/usr/lib/qt6/plugins/org.kde.kdecoration3/org.kde.breeze.so");
        assert!(builtin_decoration_without_removal(&breeze, home));
        let plastik = tile("local:aurorae:org.kde.kwin.aurorae/plastik", "/usr/share/kwin/decorations/plastik");
        assert!(builtin_decoration_without_removal(&plastik, home));
        // Own QML theme package and Aurorae SVG theme — by the general rules.
        let mine = tile("local:aurorae:org.kde.kwin.aurorae/mine", "/h/.local/share/kwin/decorations/mine");
        assert!(!builtin_decoration_without_removal(&mine, home));
        let svg = tile("local:aurorae:Sweet", "/usr/share/aurorae/themes/Sweet");
        assert!(!builtin_decoration_without_removal(&svg, home));
    }
}
