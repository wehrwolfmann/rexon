//! Port of `rexon/qt_appearance/gtk_sanitizer.py` — the GTK/KDE config sanitizer.
//!
//! Why the module exists at all: after applying a Plasma Global Theme,
//! `gtk-{3,4}.0/settings.ini` and `kdeglobals` keep references to themes that do
//! not exist on the system. GTK4 4.22 goes into resolution recursion on such
//! references and crashes with SIGSEGV — along with Firefox and any GTK4
//! application. The sanitizer finds broken references and replaces them with a
//! safe default.
//!
//! This module holds PURE logic: parsing ini text, the rules "the theme exists /
//! the cursor theme is safe", choosing the default, building the NEW text of
//! `settings.ini`, the plan of edits to `kdeglobals`/`kcminputrc` and the log
//! texts. File-system observations come through the [`GtkFs`] trait (lazy:
//! exactly the same file-system accesses the original made), while the actual
//! disk writes and running `kwriteconfig6` stay with the executor — the core
//! writes nothing.
//!
//! Warning: THIS path IS CANONICAL: three defects of the original are fixed in
//! it (each marked `Fix:` in place) —
//!   1. the paths `~/.local/share/{icons,sounds,themes,color-schemes}` were
//!      built from `Path.home()`, bypassing `XDG_DATA_HOME`;
//!   2. the log line was named just `settings.ini` — an edit to gtk-3.0 and to
//!      gtk-4.0 looked the same;
//!   3. in `kdeglobals` an empty default was written as a value (the GTK branch
//!      had such protection).
//!
//! Parity with live Python — `rust/rexon-core/tests/gtk_sanitizer_parity.rs`
//! (on REAL configs and theme directories of this machine, read-only) and
//! `rust/rexon-py/tests/gtk_sanitizer_roundtrip.py`.

use std::collections::HashMap;

use crate::i18n::{tr, trf};
use crate::installed_scan::FsSnapshot;

// ============================================================================
// File-system observations
// ============================================================================

/// The three file-system operations the sanitizer needs. `&mut self` — so that
/// the live executor can remember its first error and raise it afterwards (the
/// trait is synchronous and without `Result`, like `WaywallenIo`).
pub trait GtkFs {
    fn is_dir(&mut self, path: &str) -> bool;
    fn is_file(&mut self, path: &str) -> bool;
    /// `None` — no file or reading failed (the analogue of `except OSError`).
    fn read_text(&mut self, path: &str) -> Option<String>;
}

impl GtkFs for FsSnapshot {
    fn is_dir(&mut self, path: &str) -> bool {
        FsSnapshot::is_dir(self, path)
    }
    fn is_file(&mut self, path: &str) -> bool {
        FsSnapshot::is_file(self, path)
    }
    fn read_text(&mut self, path: &str) -> Option<String> {
        FsSnapshot::read_text(self, path).map(str::to_string)
    }
}

// ============================================================================
// Tables of defaults and paths
// ============================================================================

/// Theme types in checking order — port of the `SAFE_DEFAULTS` keys.
pub const THEME_TYPES: &[&str] = &["icon-theme", "cursor-theme", "gtk-theme", "sound-theme"];

/// Unsafe cursor themes. A SINGLE list for the whole project — a re-export of
/// [`crate::themes::UNSAFE_CURSOR_THEMES`], so that the theme-apply pre-flight
/// and the sanitizer cannot diverge.
pub use crate::themes::UNSAFE_CURSOR_THEMES;

/// Port of `SAFE_DEFAULTS` — the last resort when no fallback is found.
pub fn safe_default(theme_type: &str) -> &'static str {
    match theme_type {
        "icon-theme" => "breeze",
        "cursor-theme" => "breeze_cursors",
        "gtk-theme" => "Adwaita",
        "sound-theme" => "ocean",
        _ => "",
    }
}

/// Port of `DEFAULT_FALLBACKS` — candidates in order of preference.
pub fn default_fallbacks(theme_type: &str) -> &'static [&'static str] {
    match theme_type {
        "icon-theme" => &["breeze", "Adwaita", "hicolor"],
        "cursor-theme" => &["breeze_cursors", "Adwaita", "default"],
        "gtk-theme" => &["Adwaita", "Breeze", "Adwaita-dark"],
        "sound-theme" => &["ocean", "freedesktop"],
        _ => &[],
    }
}

/// Port of `THEME_DIRS` — where to look for a theme of the given type, in order
/// of priority.
///
/// `data_home` is the root of user data (`$XDG_DATA_HOME` or `~/.local/share`).
/// Warning — fix: the original built these paths literally from `Path.home()`,
/// bypassing XDG. With a non-standard `XDG_DATA_HOME` (Flatpak, the Steam
/// runtime, a custom profile) the sanitizer did not find themes the user REALLY
/// has installed, declared a working reference broken and replaced it with a
/// default — i.e. it broke the appearance itself instead of fixing it.
/// `~/.icons` and `~/.themes` are not XDG directories, they stay relative to
/// `home`.
pub fn theme_dirs(theme_type: &str, home: &str, data_home: &str) -> Vec<String> {
    let home = home.trim_end_matches('/');
    let data = data_home.trim_end_matches('/');
    match theme_type {
        // GTK reads `~/.icons` for icons too, not only for cursors: without it the
        // sanitizer declared a set lying there non-existent and replaced it with the
        // default (16.10.2026).
        "icon-theme" => vec![
            format!("{home}/.icons"),
            format!("{data}/icons"),
            "/usr/share/icons".to_string(),
        ],
        "cursor-theme" => vec![
            format!("{home}/.icons"),
            format!("{data}/icons"),
            "/usr/share/icons".to_string(),
        ],
        "gtk-theme" => vec![
            format!("{home}/.themes"),
            format!("{data}/themes"),
            "/usr/share/themes".to_string(),
        ],
        "sound-theme" => vec![format!("{data}/sounds"), "/usr/share/sounds".to_string()],
        _ => Vec::new(),
    }
}

/// Directories in which the `cursors/` of a parent from `Inherits=` is looked
/// for — port of the pair from `_is_cursor_theme_chain_safe` (the same XDG fix
/// as in [`theme_dirs`]).
pub fn cursor_parent_dirs(data_home: &str) -> Vec<String> {
    vec![
        "/usr/share/icons".to_string(),
        format!("{}/icons", data_home.trim_end_matches('/')),
    ]
}

// ============================================================================
// Theme existence and safety
// ============================================================================

/// Port of `_is_cursor_theme_chain_safe`: whether the `Inherits=` of a cursor
/// theme will lead GTK4 into recursion. It is safe if there is a
/// `cursor.theme`; or there is no `index.theme`; or `index.theme` cannot be read;
/// or there is no `Inherits=`; or at least one non-`hicolor` parent has its own
/// `cursors/`.
///
/// The same rule is already ported as
/// [`crate::themes::is_cursor_theme_chain_safe`] (the cursor-apply pre-flight,
/// on top of `FsSnapshot`). The only difference is the file-system abstraction;
/// the agreement of the two implementations is checked in
/// `gtk_sanitizer_parity.rs`, so that the rules do not drift apart.
pub fn is_cursor_theme_chain_safe<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_dir: &str,
    data_home: &str,
) -> bool {
    if fs.is_file(&format!("{theme_dir}/cursor.theme")) {
        return true;
    }
    let idx = format!("{theme_dir}/index.theme");
    if !fs.is_file(&idx) {
        return true; // self-contained, no inheritance
    }
    let Some(content) = fs.read_text(&idx) else {
        return true; // could not read — we consider it safe (as the original does)
    };
    let mut parents: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("Inherits=") {
            parents = rest.split(',').map(|p| p.trim().to_string()).collect();
            break;
        }
    }
    if parents.is_empty() {
        return true;
    }
    let bases = cursor_parent_dirs(data_home);
    for parent in &parents {
        if parent == "hicolor" {
            // For icons this is a terminator, for cursors a dead end: hicolor never has
            // its own `cursors/`.
            continue;
        }
        for base in &bases {
            if fs.is_dir(&format!("{base}/{parent}/cursors")) {
                return true;
            }
        }
    }
    false
}

/// Port of `_theme_exists`: whether the theme physically exists (and whether it
/// is safe, if it is a cursor theme). An empty name — `false`.
pub fn theme_exists<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_type: &str,
    name: &str,
    home: &str,
    data_home: &str,
) -> bool {
    if name.is_empty() {
        return false;
    }
    if theme_type == "cursor-theme" && UNSAFE_CURSOR_THEMES.contains(&name) {
        return false;
    }
    for base in theme_dirs(theme_type, home, data_home) {
        if !fs.is_dir(&base) {
            continue;
        }
        let candidate = format!("{base}/{name}");
        if !fs.is_dir(&candidate) {
            continue;
        }
        if theme_type == "cursor-theme" {
            if !fs.is_dir(&format!("{candidate}/cursors")) {
                continue; // not a cursor theme in this directory — keep looking
            }
            return is_cursor_theme_chain_safe(fs, &candidate, data_home);
        }
        return true;
    }
    false
}

/// Port of `_pick_safe_default`: the first existing fallback, otherwise
/// [`safe_default`] (even if it is not on the system — the last resort).
pub fn pick_safe_default<F: GtkFs + ?Sized>(
    fs: &mut F,
    theme_type: &str,
    home: &str,
    data_home: &str,
) -> String {
    for fallback in default_fallbacks(theme_type) {
        if theme_exists(fs, theme_type, fallback, home, data_home) {
            return (*fallback).to_string();
        }
    }
    safe_default(theme_type).to_string()
}

// ============================================================================
// gtk-{3,4}.0/settings.ini
// ============================================================================

/// GTK versions whose `settings.ini` is sanitized — port of the tuple in `sanitize_all`.
pub const GTK_VERSIONS: &[&str] = &["3.0", "4.0"];

/// Path of `settings.ini` of the given GTK version. `config_home` —
/// `$XDG_CONFIG_HOME` or `~/.config` (see [`crate::utils::config_home`]).
pub fn gtk_ini_path(config_home: &str, version: &str) -> String {
    format!("{}/gtk-{version}/settings.ini", config_home.trim_end_matches('/'))
}

/// Port of `_read_ini_settings`: the `[Settings]` section from the text of
/// `settings.ini`. `None` (no file / not read) — an empty map. Keys and values
/// are trimmed at the edges, comment lines (`#`) and empty ones are skipped.
pub fn read_ini_settings(text: Option<&str>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(text) = text else {
        return result;
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Settings" {
            if let Some((k, v)) = line.split_once('=') {
                result.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    result
}

/// Port of `_write_ini_setting` — the NEW text of the file (the executor does
/// the writing).
///
/// `existing = None` — there is no file: we get a minimal `[Settings]` file.
/// Otherwise the value is replaced in the existing `[Settings]` section; if the
/// key was not there, it is appended to the end of the section; if the section
/// itself is missing, it is created at the end of the file.
pub fn write_ini_setting(existing: Option<&str>, key: &str, value: &str) -> String {
    write_ini_setting_in(existing, "Settings", key, value)
}

/// The same for ANY section of an ini file.
///
/// Rule #12: the same engine serves both GTK's `settings.ini` and KDE's
/// `kdeglobals` — there must be no second copy of "find the section, replace or
/// append" in the project. The former copy in the "Settings" tab
/// (`_kde_set_terminal_konsole`, `_patch_gtk_ini`) appended the key at the index
/// "line after the first section found" and, when the needed section was
/// missing at the start of the file, ended up in someone else's.
pub fn write_ini_setting_in(
    existing: Option<&str>,
    section: &str,
    key: &str,
    value: &str,
) -> String {
    let Some(text) = existing else {
        return format!("[{section}]\n{key}={value}\n");
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut replaced = false;
    let prefix = format!("{key}=");
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with('[') && stripped.ends_with(']') {
            // The section ended and the key was not in it — append before the next
            // section's header.
            if in_section && !replaced {
                out.push(format!("{key}={value}"));
                replaced = true;
            }
            in_section = &stripped[1..stripped.len() - 1] == section;
            out.push(line.to_string());
            continue;
        }
        if in_section && stripped.starts_with(&prefix) {
            out.push(format!("{key}={value}"));
            replaced = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !replaced {
        if !in_section {
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key}={value}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// `settings.ini` key → theme type, in checking order — port of `keys_to_check`.
pub const INI_KEYS: &[(&str, &str)] = &[
    ("gtk-icon-theme-name", "icon-theme"),
    ("gtk-cursor-theme-name", "cursor-theme"),
    ("gtk-theme-name", "gtk-theme"),
    ("gtk-sound-theme-name", "sound-theme"),
];

/// Theme types whose reference the sanitizer does NOT replace, even if the theme
/// is missing.
///
/// Warning — fix (13.10.2026): the sound theme was replaced with `ocean` on a
/// par with icons and cursors. Reasons to leave it as is:
///   * a missing sound theme breaks nothing — libcanberra and KDE silently play
///     `freedesktop`; the sanitizer exists because of GTK4 crashes on broken
///     icon and cursor themes, which does not apply to sounds;
///   * only the pair `gtk-{3,4}.0/settings.ini` was edited, while `kdeglobals`
///     `[Sounds] Theme`, gsettings and `.gtkrc-2.0` stayed as before — the
///     setting contradicted itself;
///   * in KDE the `gtkconfig` service (kded) copies `[Sounds] Theme` into the GTK
///     files on every session login — the replacement lived until the next login
///     and was repeated on the very first launch of the window, so the value
///     "jumped";
///   * the person's choice was silently changed to a theme they did not choose.
///
/// The sanitizer only reports such a reference ([`plan_gtk_ini_notes`]); the
/// "Event sounds" section itself shows it as a missing theme.
pub const KEPT_TYPES: &[&str] = &["sound-theme"];

/// Whether sound theme `name` exists: `<dir>/<name>/index.theme` with a
/// `[Sound Theme]` section — the same rule as for applying
/// ([`crate::qt_settings::is_sound_theme_dir`]), on top of [`GtkFs`]. A bare
/// directory without `index.theme` (an archive wrapper,
/// `/usr/share/sounds/mate`) is not a theme.
pub fn sound_theme_exists<F: GtkFs + ?Sized>(
    fs: &mut F,
    name: &str,
    home: &str,
    data_home: &str,
) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return false;
    }
    theme_dirs("sound-theme", home, data_home).iter().any(|base| {
        fs.read_text(&format!("{base}/{name}/index.theme"))
            .map(|t| t.lines().any(|l| l.trim() == "[Sound Theme]"))
            .unwrap_or(false)
    })
}

/// Notes on keys from [`KEPT_TYPES`]: the reference points to a theme that does
/// not exist, but the value is LEFT as is. The lines are ready for the log; the
/// file is not changed either here or by the executor.
pub fn plan_gtk_ini_notes<F: GtkFs + ?Sized>(
    fs: &mut F,
    ini_path: &str,
    settings: &HashMap<String, String>,
    home: &str,
    data_home: &str,
) -> Vec<String> {
    let label = ini_label(ini_path);
    let mut notes = Vec::new();
    for (key, theme_type) in INI_KEYS {
        if *theme_type != "sound-theme" {
            continue;
        }
        let value = settings.get(*key).map(|v| v.trim()).unwrap_or("");
        if value.is_empty() || sound_theme_exists(fs, value, home, data_home) {
            continue;
        }
        // TRANSLATORS: %1 is a config file and key, e.g.
        // "gtk-3.0/settings.ini: gtk-sound-theme-name"; %2 is a theme name.
        notes.push(trf(
            "%1: sound theme “%2” is not installed; the setting was left as is",
            &[&format!("{label}: {key}"), &value],
        ));
    }
    notes
}

/// One edit of `settings.ini`: what to write and what to say about it in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IniEdit {
    pub key: String,
    pub value: String,
    /// The log line — added by the caller ONLY if the write succeeded.
    pub change: String,
}

/// Name for the log: `gtk-3.0/settings.ini`.
///
/// Warning — fix: the original took `ini_path.name`, i.e. always just
/// `settings.ini`. Edits of gtk-3.0 and gtk-4.0 were indistinguishable in the
/// log and in the UI report — when investigating a complaint "Firefox still
/// crashes" it was impossible to tell which of the two configs had already been
/// fixed.
pub fn ini_label(ini_path: &str) -> String {
    let mut it = ini_path.rsplitn(3, '/');
    let name = it.next().unwrap_or("");
    // An empty parent — the file is at the root or has no directory at all:
    // `Path.parent.name` is empty in these cases too, and only the file name remains.
    match it.next() {
        Some(parent) if !parent.is_empty() => format!("{parent}/{name}"),
        _ => name.to_string(),
    }
}

/// Port of the decisions of `sanitize_gtk_settings_ini`: which keys are broken
/// and what to replace them with. An empty/missing value and a live theme are
/// skipped; if there is nothing to substitute, there is no edit. Keys of types
/// from [`KEPT_TYPES`] (the sound theme) are never edited — [`plan_gtk_ini_notes`]
/// talks about them.
///
/// NO writing happens here: for each edit the executor calls
/// [`write_ini_setting`] on the FRESH contents of the file and writes the result
/// — exactly like the original, which re-read the file for every key.
pub fn plan_gtk_ini_edits<F: GtkFs + ?Sized>(
    fs: &mut F,
    ini_path: &str,
    settings: &HashMap<String, String>,
    home: &str,
    data_home: &str,
) -> Vec<IniEdit> {
    let label = ini_label(ini_path);
    let mut edits = Vec::new();
    for (key, theme_type) in INI_KEYS {
        if KEPT_TYPES.contains(theme_type) {
            continue;
        }
        let value = settings.get(*key).map(|v| v.trim()).unwrap_or("");
        if value.is_empty() {
            continue;
        }
        if theme_exists(fs, theme_type, value, home, data_home) {
            continue;
        }
        let safe = pick_safe_default(fs, theme_type, home, data_home);
        if safe.is_empty() {
            continue;
        }
        edits.push(IniEdit {
            key: (*key).to_string(),
            value: safe.clone(),
            change: format!("{label}: {key}: '{value}' → '{safe}'"),
        });
    }
    edits
}

// ============================================================================
// kdeglobals / kcminputrc
// ============================================================================

/// Port of `_read_kdeglobals`: `{section: {key: value}}`. Keys before the first
/// section are dropped, comments (`#`) and empty lines are skipped.
pub fn read_kde_ini(text: Option<&str>) -> HashMap<String, HashMap<String, String>> {
    let mut result: HashMap<String, HashMap<String, String>> = HashMap::new();
    let Some(text) = text else {
        return result;
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            result.entry(section.clone()).or_default();
        } else if !section.is_empty() {
            if let Some((k, v)) = line.split_once('=') {
                result
                    .entry(section.clone())
                    .or_default()
                    .insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    result
}

/// Value of `section/key` or `""` — port of `config.get(s, {}).get(k, "")`.
pub fn kde_value(cfg: &HashMap<String, HashMap<String, String>>, section: &str, key: &str) -> String {
    cfg.get(section)
        .and_then(|m| m.get(key))
        .cloned()
        .unwrap_or_default()
}

/// Port of `_read_kcminput_cursor`: the first `cursorTheme=` in the `[Mouse]`
/// section. Comments are NOT skipped here — as in the original.
pub fn read_kcminput_cursor(text: Option<&str>) -> String {
    let Some(text) = text else {
        return String::new();
    };
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
        } else if section == "Mouse" {
            if let Some(rest) = line.strip_prefix("cursorTheme=") {
                return rest.trim().to_string();
            }
        }
    }
    String::new()
}

/// Port of `_colorscheme_exists`: where the `.colors` of a color scheme lie (the
/// same XDG fix as in [`theme_dirs`]).
pub fn colorscheme_paths(name: &str, data_home: &str) -> Vec<String> {
    vec![
        format!("{}/color-schemes/{name}.colors", data_home.trim_end_matches('/')),
        format!("/usr/share/color-schemes/{name}.colors"),
    ]
}

/// Port of `_colorscheme_exists`.
pub fn colorscheme_exists<F: GtkFs + ?Sized>(fs: &mut F, name: &str, data_home: &str) -> bool {
    colorscheme_paths(name, data_home)
        .iter()
        .any(|p| fs.is_file(p))
}

/// Default color scheme when the current one is missing — port of the literal
/// `"BreezeDark"`.
pub const COLORSCHEME_DEFAULT: &str = "BreezeDark";

/// Timeout for running `kwriteconfig6` (seconds) — like `timeout=10` in Python.
pub const KWRITECONFIG_TIMEOUT: u64 = 10;

/// One edit of a KDE config: what to pass to `kwriteconfig6` and what to tell
/// the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdeEdit {
    pub file: String,
    pub group: String,
    pub key: String,
    pub value: String,
    /// The log line — added ONLY if `kwriteconfig6` succeeded.
    pub change: String,
}

/// argv for writing a KDE config — port of `_kwriteconfig`. `tool` — the already
/// resolved binary (`kwriteconfig6`/`kwriteconfig5`, see
/// `rexon.utils.resolve_tool`).
pub fn kwriteconfig_argv(tool: &str, edit: &KdeEdit) -> Vec<String> {
    vec![
        tool.to_string(),
        "--file".to_string(),
        edit.file.clone(),
        "--group".to_string(),
        edit.group.clone(),
        "--key".to_string(),
        edit.key.clone(),
        edit.value.clone(),
    ]
}

/// Port of the decisions of `sanitize_kdeglobals`: ColorScheme, icons and cursor
/// in `kdeglobals` plus the cursor in `kcminputrc` (Plasma 6 prefers it).
///
/// Warning — fix: the original checked "nothing to substitute" only in the GTK
/// branch, while in the KDE branch it wrote whatever it got — an empty default
/// went into the config as a value, and Plasma got `Theme=`/`cursorTheme=`
/// without any theme. Here an empty default cancels the edit, as for GTK.
pub fn plan_kde_edits<F: GtkFs + ?Sized>(
    fs: &mut F,
    kdeglobals: &HashMap<String, HashMap<String, String>>,
    kcminput_cursor: &str,
    home: &str,
    data_home: &str,
) -> Vec<KdeEdit> {
    let mut edits = Vec::new();

    // 1. ColorScheme in [General] — its default is a literal, not from the file system.
    let cs = kde_value(kdeglobals, "General", "ColorScheme");
    if !cs.is_empty() && !colorscheme_exists(fs, &cs, data_home) {
        edits.push(KdeEdit {
            file: "kdeglobals".into(),
            group: "General".into(),
            key: "ColorScheme".into(),
            value: COLORSCHEME_DEFAULT.into(),
            change: format!(
                "kdeglobals[General]ColorScheme: '{cs}' → '{COLORSCHEME_DEFAULT}'"
            ),
        });
    }

    // 2. Icon Theme in [Icons]
    let icons = kde_value(kdeglobals, "Icons", "Theme");
    if !icons.is_empty() && !theme_exists(fs, "icon-theme", &icons, home, data_home) {
        let safe = pick_safe_default(fs, "icon-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kdeglobals".into(),
                group: "Icons".into(),
                key: "Theme".into(),
                value: safe.clone(),
                change: format!("kdeglobals[Icons]Theme: '{icons}' → '{safe}'"),
            });
        }
    }

    // 3. Cursor in [Mouse] — kdeglobals
    let cursor = kde_value(kdeglobals, "Mouse", "cursorTheme");
    if !cursor.is_empty() && !theme_exists(fs, "cursor-theme", &cursor, home, data_home) {
        let safe = pick_safe_default(fs, "cursor-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kdeglobals".into(),
                group: "Mouse".into(),
                key: "cursorTheme".into(),
                value: safe.clone(),
                change: format!("kdeglobals[Mouse]cursorTheme: '{cursor}' → '{safe}'"),
            });
        }
    }

    // 4. Cursor in [Mouse] — kcminputrc
    if !kcminput_cursor.is_empty()
        && !theme_exists(fs, "cursor-theme", kcminput_cursor, home, data_home)
    {
        let safe = pick_safe_default(fs, "cursor-theme", home, data_home);
        if !safe.is_empty() {
            edits.push(KdeEdit {
                file: "kcminputrc".into(),
                group: "Mouse".into(),
                key: "cursorTheme".into(),
                value: safe.clone(),
                change: format!(
                    "kcminputrc[Mouse]cursorTheme: '{kcminput_cursor}' → '{safe}'"
                ),
            });
        }
    }

    edits
}

// ============================================================================
// Report
// ============================================================================

/// Port of the printing of `sanitize_all(verbose=True)` — the whole text (with a
/// trailing newline), so that only a single `print` is left to the executor.
pub fn verbose_report(changes: &[String]) -> String {
    if changes.is_empty() {
        return tr("✅ All GTK/KDE configs are OK, nothing was fixed") + "\n";
    }
    let mut out = tr("✅ Fixed broken links:") + "\n";
    for c in changes {
        out.push_str("  • ");
        out.push_str(c);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(v: serde_json::Value) -> FsSnapshot {
        FsSnapshot::from_json(&v)
    }

    #[test]
    fn ini_label_takes_version_directory() {
        assert_eq!(ini_label("/home/u/.config/gtk-3.0/settings.ini"), "gtk-3.0/settings.ini");
        assert_eq!(ini_label("settings.ini"), "settings.ini");
    }

    #[test]
    fn write_ini_setting_creates_file() {
        assert_eq!(
            write_ini_setting(None, "gtk-theme-name", "Adwaita"),
            "[Settings]\ngtk-theme-name=Adwaita\n"
        );
    }

    #[test]
    fn write_ini_setting_appends_key_to_section_end() {
        let src = "[Settings]\na=1\n\n[Other]\nb=2\n";
        let out = write_ini_setting(Some(src), "gtk-theme-name", "Breeze");
        assert_eq!(out, "[Settings]\na=1\n\ngtk-theme-name=Breeze\n[Other]\nb=2\n");
    }

    #[test]
    fn write_ini_setting_creates_missing_section() {
        let out = write_ini_setting(Some("[Other]\nb=2\n"), "k", "v");
        assert_eq!(out, "[Other]\nb=2\n[Settings]\nk=v\n");
    }

    #[test]
    fn empty_default_not_in_kde_edit() {
        // No icon theme at all on the system → pick_safe_default returns the literal
        // "breeze"; for an empty type — an empty string, no edit.
        let mut fs = snap(json!({"dirs": {}, "files": {}}));
        assert_eq!(pick_safe_default(&mut fs, "нет-такого", "/h", "/h/.local/share"), "");
    }

    /// A live case from 13.10.2026: `gtk-sound-theme-name=Sounds`, there is no
    /// "Sounds" theme. The value is not edited (it is not in the edit plan), there
    /// is a note about it.
    #[test]
    fn missing_sound_theme_not_substituted() {
        let mut fs = snap(json!({
            "dirs": {
                "/usr/share/sounds": ["ocean", "mate"],
                "/usr/share/sounds/ocean": ["index.theme"],
                "/usr/share/sounds/mate": [],
                "/h/.local/share/sounds": ["Zorin"],
                "/h/.local/share/sounds/Zorin": ["index.theme"],
                "/usr/share/icons": ["breeze"],
                "/usr/share/icons/breeze": []
            },
            "files": {
                "/usr/share/sounds/ocean/index.theme": "[Sound Theme]\nName=Ocean\n",
                "/h/.local/share/sounds/Zorin/index.theme": "[Sound Theme]\nName=Zorin \n"
            }
        }));
        let ini = "/h/.config/gtk-3.0/settings.ini";
        let settings: HashMap<String, String> = [
            ("gtk-sound-theme-name", "Sounds"),
            ("gtk-icon-theme-name", "НетЗначков"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

        let edits = plan_gtk_ini_edits(&mut fs, ini, &settings, "/h", "/h/.local/share");
        let keys: Vec<&str> = edits.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["gtk-icon-theme-name"], "значки чинятся, звук — нет");

        let notes = plan_gtk_ini_notes(&mut fs, ini, &settings, "/h", "/h/.local/share");
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("gtk-3.0/settings.ini: gtk-sound-theme-name"), "{}", notes[0]);
        assert!(notes[0].contains("Sounds"), "{}", notes[0]);
    }

    /// An existing theme (user or system) and an empty value — no notes; a directory
    /// without `index.theme` does not count as a theme.
    #[test]
    fn sound_theme_note_only_when_theme_missing() {
        let mut fs = snap(json!({
            "dirs": {
                "/usr/share/sounds": ["ocean", "mate"],
                "/usr/share/sounds/ocean": ["index.theme"],
                "/usr/share/sounds/mate": [],
                "/d/sounds": ["Zorin"],
                "/d/sounds/Zorin": ["index.theme"]
            },
            "files": {
                "/usr/share/sounds/ocean/index.theme": "[Sound Theme]\nName=Ocean\n",
                "/d/sounds/Zorin/index.theme": "[Sound Theme]\nName=Zorin\n"
            }
        }));
        let notes_for = |fs: &mut FsSnapshot, value: &str| {
            let settings: HashMap<String, String> =
                [("gtk-sound-theme-name".to_string(), value.to_string())].into_iter().collect();
            plan_gtk_ini_notes(fs, "/h/.config/gtk-4.0/settings.ini", &settings, "/h", "/d")
        };
        for ok in ["ocean", "Zorin", ""] {
            assert!(notes_for(&mut fs, ok).is_empty(), "«{ok}» — замечания быть не должно");
        }
        for bad in ["Sounds", "mate", "Sounds/Zorin", ".."] {
            assert_eq!(notes_for(&mut fs, bad).len(), 1, "«{bad}» — темы нет");
        }
    }

    #[test]
    fn unsafe_cursor_counted_as_broken() {
        let mut fs = snap(json!({
            "dirs": {"/usr/share/icons": [], "/usr/share/icons/Adwaita": [],
                     "/usr/share/icons/Adwaita/cursors": []},
            "files": {}
        }));
        assert!(!theme_exists(&mut fs, "cursor-theme", "Adwaita", "/h", "/h/.local/share"));
    }
}
