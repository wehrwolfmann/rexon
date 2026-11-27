//! Wallpaper of a third-party shell (Noctalia, DankMaterialShell): whether
//! it draws the background, and, with the person's consent, "Give the
//! wallpaper to Rexon" / "Return to the shell".
//!
//! WHY. Both shells draw the background THEMSELVES, with their own full
//! screen surface on the Background layer, and it lies on top of the Rexon
//! wallpaper (analysis above `system::DesktopShell`). Fighting for the
//! layer is pointless, staying silent is not allowed. There is one honest
//! way: ask the person and turn off the shell's wallpaper with the shell's
//! own regular switch, remembering the previous value so "Return" restores
//! exactly it.
//!
//! The switches (checked against the sources of noctalia v5.2.0 and DMS
//! v1.6.2, survey of 01.10.2026):
//!   * Noctalia: `[wallpaper] enabled = false`. Write ONLY to the state
//!     file `settings.toml` (`$NOCTALIA_STATE_HOME/noctalia/`,
//!     `$XDG_STATE_HOME/noctalia/` or `~/.local/state/noctalia/`): it is
//!     loaded last and overrides `~/.config/noctalia/*.toml`, and the
//!     shell's GUI writes there too. The state would override an edit in
//!     `~/.config`. It cannot be turned off through `noctalia msg`: there
//!     is no such command. The shell watches the file and rereads it
//!     itself.
//!   * DMS: `screenPreferences.wallpaper = []` in
//!     `$XDG_CONFIG_HOME/DankMaterialShell/settings.json` ("Disable
//!     Built-in Wallpapers"). An empty list removes both the main surface
//!     and the second one (`dms:blurwallpaper` on niri): both are built by
//!     the same screen filter. ⚠️ `dms ipc call wallpaper clear` will NOT
//!     do: it erases the path, but the surface stays and is filled with a
//!     colour. An array cannot be set through `dms ipc call settings set`
//!     (arrays are forbidden there). The shell rereads the file itself.
//!
//! The writing rule: ONE managed key is edited, the rest of the file stays
//! byte for byte; the edit is verified by parsing before and after (the new
//! value must differ from the old one by EXACTLY this key, otherwise a
//! refusal rather than damage); the previous content is placed next to it
//! as a backup; writing is done by renaming over (`utils::atomic_write`, a
//! symlink is resolved to its target). The previous value of the key is
//! remembered in `$XDG_STATE_HOME/rexon/shell-wallpaper.json` BEFORE the
//! shell's file is written: if writing the note failed after the edit,
//! there would be nothing to restore.
//!
//! ⚠️ Limitation: neither shell is installed on the development machine.
//! Everything here is verified with fake files (`tests/desktop_shell.rs`)
//! and a bench with a private HOME, not live.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{tr, trf};
use crate::system::{detect_shell, DesktopShell, ShellDetection, ShellFacts, SHELL_ENV_NAMES};
use crate::wallpaper_engine::{wallpaper_layer_owner, WallpaperOwner};

// ============================================================================
// Where the shells keep their settings
// ============================================================================

/// Home roots from which the shells compute their paths. An empty string
/// means the variable is not set (empty = unset, as with XDG).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellHome {
    pub home: String,
    pub xdg_config_home: String,
    pub xdg_state_home: String,
    /// `NOCTALIA_CONFIG_HOME`: an XDG-style root (the shell reads `<it>/noctalia`).
    pub noctalia_config_home: String,
    /// `NOCTALIA_STATE_HOME`: the same for state.
    pub noctalia_state_home: String,
}

impl ShellHome {
    /// Roots via an environment reading function: this is how both the live
    /// call and the parity check obtain them.
    pub fn from_env(get: impl Fn(&str) -> String) -> ShellHome {
        ShellHome {
            home: get("HOME"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
            xdg_state_home: get("XDG_STATE_HOME"),
            noctalia_config_home: get("NOCTALIA_CONFIG_HOME"),
            noctalia_state_home: get("NOCTALIA_STATE_HOME"),
        }
    }

    /// Roots of this process.
    pub fn live() -> ShellHome {
        ShellHome::from_env(|k| std::env::var(k).unwrap_or_default())
    }

    fn config_home(&self) -> PathBuf {
        if self.xdg_config_home.is_empty() {
            Path::new(&self.home).join(".config")
        } else {
            PathBuf::from(&self.xdg_config_home)
        }
    }

    fn state_home(&self) -> PathBuf {
        if self.xdg_state_home.is_empty() {
            Path::new(&self.home).join(".local/state")
        } else {
            PathBuf::from(&self.xdg_state_home)
        }
    }

    /// Noctalia configuration directory (all `*.toml` in it are loaded
    /// alphabetically). The order of roots is as in the shell itself.
    pub fn noctalia_config_dir(&self) -> PathBuf {
        if !self.noctalia_config_home.is_empty() {
            Path::new(&self.noctalia_config_home).join("noctalia")
        } else {
            self.config_home().join("noctalia")
        }
    }

    /// Noctalia state file `settings.toml`: the layer that overrides the
    /// configuration; this is where we write.
    pub fn noctalia_state_file(&self) -> PathBuf {
        let dir = if !self.noctalia_state_home.is_empty() {
            Path::new(&self.noctalia_state_home).join("noctalia")
        } else {
            self.state_home().join("noctalia")
        };
        dir.join("settings.toml")
    }

    /// DankMaterialShell settings (`StandardPaths.ConfigLocation`).
    pub fn dms_settings_file(&self) -> PathBuf {
        self.config_home().join("DankMaterialShell/settings.json")
    }

    /// The file Rexon edits for this shell.
    pub fn managed_file(&self, shell: DesktopShell) -> PathBuf {
        match shell {
            DesktopShell::Noctalia => self.noctalia_state_file(),
            DesktopShell::DankMaterialShell => self.dms_settings_file(),
        }
    }

    /// Rexon notes: whose switch and what value was there before us (a list,
    /// one entry per shell).
    pub fn handover_file(&self) -> PathBuf {
        self.state_home().join("rexon/shell-wallpaper.json")
    }
}

/// Name of the backup next to the shell's file.
pub fn backup_path(file: &Path) -> PathBuf {
    let mut name = file.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".rexon-backup");
    file.with_file_name(name)
}

// ============================================================================
// Whether the shell draws the wallpaper
// ============================================================================

/// The value of `[wallpaper] enabled` in a parsed Noctalia table.
fn noctalia_enabled_in(table: &toml::Table) -> Option<bool> {
    table
        .get("wallpaper")
        .and_then(|w| w.as_table())
        .and_then(|w| w.get("enabled"))
        .and_then(|v| v.as_bool())
}

/// Whether Noctalia draws the wallpaper with these settings layers.
///
/// `config_layers` are the configuration texts IN LOADING ORDER (as
/// [`noctalia_config_layers`] put them together: for each file first its
/// `[include]`, then the file itself); `state` is `settings.toml` or `None`
/// if there is no file. The last layer where the key is set wins; by
/// default the wallpaper is on.
///
/// `None` means no answer is possible (some layer did not parse: in that
/// case the shell behaves in its own way, and we will not guess for it).
/// The caller treats `None` as "draws", the cautious side, see
/// [`wallpaper_layer_owner`].
///
/// ⚠️ Per-monitor `[wallpaper.monitor.<output>] enabled = false` is not
/// taken into account here: the answer "draws on at least one output" is
/// correct with a general `enabled = true`, and the core does not know
/// which outputs exist.
pub fn noctalia_draws_wallpaper(config_layers: &[String], state: Option<&str>) -> Option<bool> {
    let mut enabled = true;
    for text in config_layers.iter().map(String::as_str).chain(state) {
        let table: toml::Table = toml::from_str(text).ok()?;
        if let Some(v) = noctalia_enabled_in(&table) {
            enabled = v;
        }
    }
    Some(enabled)
}

/// Whether DMS draws the wallpaper with this `settings.json` (`None` means
/// there is no file).
///
/// No key, `null` or a list with screens: draws (on all or on the named
/// ones); an empty list: draws nowhere. `None`: the file did not parse.
pub fn dms_draws_wallpaper(settings: Option<&str>) -> Option<bool> {
    let Some(text) = settings else {
        return Some(true);
    };
    let value: Value = serde_json::from_str(text).ok()?;
    let prefs = value.get("screenPreferences").and_then(|p| p.get("wallpaper"));
    Some(!matches!(prefs, Some(Value::Array(a)) if a.is_empty()))
}

/// Noctalia configuration texts in loading order: all `*.toml` of the
/// directory alphabetically, for each one first its `[include]` (files and
/// directories, paths relative to the including file's directory, `~` is
/// expanded), then the file itself. If even one root file sets
/// `[include] autoload = false`, only such files are loaded (the shell
/// does the same).
///
/// An unreadable file is skipped silently, an unparsable one goes into the
/// list as is, so that [`noctalia_draws_wallpaper`] honestly answers "I do
/// not know". Paths with `$VAR` are not expanded (rare; such a file is
/// simply not counted).
pub fn noctalia_config_layers(home: &ShellHome) -> Vec<String> {
    let dir = home.noctalia_config_dir();
    let roots = toml_files_in(&dir);
    let parsed: Vec<(PathBuf, String, Option<toml::Table>)> = roots
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let table = toml::from_str::<toml::Table>(&text).ok();
            Some((p, text, table))
        })
        .collect();
    let no_autoload = |t: &Option<toml::Table>| {
        t.as_ref()
            .and_then(|t| t.get("include"))
            .and_then(|i| i.get("autoload"))
            .and_then(|a| a.as_bool())
            == Some(false)
    };
    let selective = parsed.iter().any(|(_, _, t)| no_autoload(t));
    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for (path, text, table) in &parsed {
        if selective && !no_autoload(table) {
            continue;
        }
        push_layer(home, path, text, table.as_ref(), &mut out, &mut seen, 0);
    }
    out
}

fn toml_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn push_layer(
    home: &ShellHome,
    path: &Path,
    text: &str,
    table: Option<&toml::Table>,
    out: &mut Vec<String>,
    seen: &mut Vec<PathBuf>,
    depth: usize,
) {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if seen.contains(&canon) || depth > 16 {
        return;
    }
    seen.push(canon);
    let includes: Vec<String> = table
        .and_then(|t| t.get("include"))
        .and_then(|i| i.get("files"))
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let base = path.parent().unwrap_or(Path::new("/")).to_path_buf();
    for inc in includes {
        if inc.contains('$') {
            continue;
        }
        let p = if let Some(rest) = inc.strip_prefix("~/") {
            Path::new(&home.home).join(rest)
        } else if inc == "~" {
            PathBuf::from(&home.home)
        } else {
            base.join(&inc)
        };
        let files = if p.is_dir() { toml_files_in(&p) } else { vec![p] };
        for f in files {
            if let Ok(t) = std::fs::read_to_string(&f) {
                let tb = toml::from_str::<toml::Table>(&t).ok();
                push_layer(home, &f, &t, tb.as_ref(), out, seen, depth + 1);
            }
        }
    }
    out.push(text.to_string());
}

/// Whether the shell draws the wallpaper, judging by the real files under `home`.
pub fn shell_draws_wallpaper(shell: DesktopShell, home: &ShellHome) -> Option<bool> {
    match shell {
        DesktopShell::Noctalia => {
            let layers = noctalia_config_layers(home);
            let state = read_optional(&home.noctalia_state_file()).ok()?;
            noctalia_draws_wallpaper(&layers, state.as_deref())
        }
        DesktopShell::DankMaterialShell => {
            let settings = read_optional(&home.dms_settings_file()).ok()?;
            dms_draws_wallpaper(settings.as_deref())
        }
    }
}

/// File text; `Ok(None)` means no file; `Err` means it exists but cannot be read.
fn read_optional(path: &Path) -> Result<Option<String>, std::io::Error> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// ============================================================================
// Editing one key: what was there before us
// ============================================================================

/// What stood in place of the managed key before Rexon's edit. Exactly this
/// is what "Return to the shell" restores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Previous {
    /// There was no file: Rexon created it; restoring removes the key and, if
    /// nothing else is left in the file, deletes the file.
    FileAbsent,
    /// There was no key. `parent_created`: Rexon also created the parent (the
    /// `[wallpaper]` table in Noctalia, the `screenPreferences` object in
    /// DMS); `prefix` is what Rexon appended before the new table at the end
    /// of the file (newlines), so that restoring removes exactly that.
    KeyAbsent {
        parent_created: bool,
        #[serde(default)]
        prefix: String,
    },
    /// There was a key: its value VERBATIM, as it stood in the file.
    Value { raw: String },
}

/// Result of restoring: the new file text or "delete the file" (Rexon
/// created it, and after restoring it is empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restored {
    Write(String),
    Delete,
}

/// Why editing is not possible. The texts are for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The file does not parse (the shell itself would not read it in this form).
    Unparsable,
    /// The file parses, but is written in a form Rexon does not edit safely
    /// (a dotted key, an inline table, the wrong type…).
    UnsafeForm,
}

impl EditError {
    pub fn message(&self) -> String {
        match self {
            EditError::Unparsable => tr("the settings file could not be read as valid settings"),
            EditError::UnsafeForm => {
                tr("the settings file is written in a form Rexon cannot edit safely")
            }
        }
    }
}

// ── Noctalia: TOML line by line ────────────────────────────────────────────

/// Text lines with their endings (the `\n` stays in the line).
fn lines_with_ends(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Table name in a `[name]` header line (without `[[…]]`), otherwise `None`.
fn toml_header(line: &str) -> Option<Option<String>> {
    let t = line.trim_start();
    if !t.starts_with('[') {
        return None;
    }
    if t.starts_with("[[") {
        return Some(None);
    }
    let close = t.find(']')?;
    let rest = t[close + 1..].trim();
    if !(rest.is_empty() || rest.starts_with('#')) {
        return Some(None);
    }
    Some(Some(t[1..close].trim().to_string()))
}

/// Lines inside TOML multiline strings (`"""`/`'''`) are neither headers
/// nor keys. A rough quote count; a counting error is caught by the parse
/// check.
fn toml_line_kinds(lines: &[&str]) -> Vec<bool> {
    let mut inside: Option<&str> = None;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        out.push(inside.is_none());
        let mut rest: &str = line;
        loop {
            match inside {
                None => {
                    let a = rest.find("\"\"\"");
                    let b = rest.find("'''");
                    let (pos, q) = match (a, b) {
                        (Some(x), Some(y)) if x <= y => (x, "\"\"\""),
                        (Some(_), Some(y)) => (y, "'''"),
                        (Some(x), None) => (x, "\"\"\""),
                        (None, Some(y)) => (y, "'''"),
                        (None, None) => break,
                    };
                    inside = Some(q);
                    rest = &rest[pos + 3..];
                }
                Some(q) => match rest.find(q) {
                    Some(pos) => {
                        inside = None;
                        rest = &rest[pos + 3..];
                    }
                    None => break,
                },
            }
        }
    }
    out
}

/// Where in the text the `[wallpaper]` table and its `enabled` key are.
struct NoctaliaSpot {
    /// Line number of the `[wallpaper]` header.
    header: Option<usize>,
    /// Line number of `enabled = …` in this table and the byte bounds of the
    /// value within the line.
    key: Option<(usize, usize, usize)>,
}

fn noctalia_spot(lines: &[&str]) -> NoctaliaSpot {
    let plain = toml_line_kinds(lines);
    let mut header = None;
    let mut key = None;
    let mut in_wallpaper = false;
    for (i, line) in lines.iter().enumerate() {
        if !plain[i] {
            continue;
        }
        if let Some(name) = toml_header(line) {
            in_wallpaper = name.as_deref() == Some("wallpaper");
            if in_wallpaper && header.is_none() {
                header = Some(i);
            }
            continue;
        }
        if !in_wallpaper || key.is_some() {
            continue;
        }
        let lead = line.len() - line.trim_start().len();
        let body = &line[lead..];
        let Some(after) = body.strip_prefix("enabled") else { continue };
        let ws = after.len() - after.trim_start().len();
        let Some(after_eq) = after[ws..].strip_prefix('=') else { continue };
        let vs = lead + "enabled".len() + ws + 1 + (after_eq.len() - after_eq.trim_start().len());
        let ve = line[vs..]
            .find(|c: char| c.is_whitespace() || c == '#')
            .map(|p| vs + p)
            .unwrap_or(line.len());
        key = Some((i, vs, ve));
    }
    NoctaliaSpot { header, key }
}

/// The parsed table with `[wallpaper] enabled` replaced: `Some(v)` sets
/// it, `None` removes the key (and the empty table, if `drop_empty`).
fn noctalia_expect(base: &toml::Table, value: Option<toml::Value>, drop_empty: bool) -> toml::Table {
    let mut t = base.clone();
    match value {
        Some(v) => {
            let w = t
                .entry("wallpaper")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let Some(w) = w.as_table_mut() {
                w.insert("enabled".to_string(), v);
            }
        }
        None => {
            if let Some(w) = t.get_mut("wallpaper").and_then(|w| w.as_table_mut()) {
                w.remove("enabled");
                if drop_empty && w.is_empty() {
                    t.remove("wallpaper");
                }
            }
        }
    }
    t
}

fn parse_toml(text: &str) -> Result<toml::Table, EditError> {
    toml::from_str::<toml::Table>(text).map_err(|_| EditError::Unparsable)
}

/// Turn off the Noctalia wallpaper in the `settings.toml` text (`None`
/// means no file). Returns the new text and what stood there before.
pub fn noctalia_disable(text: Option<&str>) -> Result<(String, Previous), EditError> {
    let Some(text) = text else {
        let new = "[wallpaper]\nenabled = false\n".to_string();
        return Ok((new, Previous::FileAbsent));
    };
    let before = parse_toml(text)?;
    if let Some(w) = before.get("wallpaper") {
        if !w.is_table() {
            return Err(EditError::UnsafeForm);
        }
        if let Some(v) = w.get("enabled") {
            if !v.is_bool() {
                return Err(EditError::UnsafeForm);
            }
        }
    }
    let lines = lines_with_ends(text);
    let spot = noctalia_spot(&lines);
    let (new, prev) = match (spot.header, spot.key) {
        (_, Some((i, vs, ve))) => {
            let raw = lines[i][vs..ve].to_string();
            let mut out = String::with_capacity(text.len());
            for (j, l) in lines.iter().enumerate() {
                if j == i {
                    out.push_str(&l[..vs]);
                    out.push_str("false");
                    out.push_str(&l[ve..]);
                } else {
                    out.push_str(l);
                }
            }
            (out, Previous::Value { raw })
        }
        (Some(h), None) => {
            let prefix = if lines[h].ends_with('\n') { "" } else { "\n" };
            let mut out = String::with_capacity(text.len() + 20);
            for (j, l) in lines.iter().enumerate() {
                out.push_str(l);
                if j == h {
                    out.push_str(prefix);
                    out.push_str("enabled = false\n");
                }
            }
            (out, Previous::KeyAbsent { parent_created: false, prefix: prefix.to_string() })
        }
        (None, None) => {
            let prefix = if text.is_empty() {
                ""
            } else if text.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let out = format!("{text}{prefix}[wallpaper]\nenabled = false\n");
            (out, Previous::KeyAbsent { parent_created: true, prefix: prefix.to_string() })
        }
    };
    let after = parse_toml(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != noctalia_expect(&before, Some(toml::Value::Boolean(false)), false) {
        return Err(EditError::UnsafeForm);
    }
    Ok((new, prev))
}

/// Give the Noctalia wallpaper back what stood there before Rexon's edit.
pub fn noctalia_restore(text: Option<&str>, prev: &Previous) -> Result<Restored, EditError> {
    let Some(text) = text else {
        // The file was deleted without us. If it was "no key" or "no file", it
        // stays that way. If there was a value, we put it back into a new file.
        return Ok(match prev {
            Previous::Value { raw } => Restored::Write(format!("[wallpaper]\nenabled = {raw}\n")),
            _ => Restored::Delete,
        });
    };
    let before = parse_toml(text)?;
    let lines = lines_with_ends(text);
    let spot = noctalia_spot(&lines);
    let (new, expect) = match prev {
        Previous::Value { raw } => {
            let raw_value: toml::Value = toml::from_str::<toml::Table>(&format!("v = {raw}"))
                .ok()
                .and_then(|t| t.get("v").cloned())
                .ok_or(EditError::UnsafeForm)?;
            let new = match (spot.header, spot.key) {
                (_, Some((i, vs, ve))) => {
                    let mut out = String::with_capacity(text.len());
                    for (j, l) in lines.iter().enumerate() {
                        if j == i {
                            out.push_str(&l[..vs]);
                            out.push_str(raw);
                            out.push_str(&l[ve..]);
                        } else {
                            out.push_str(l);
                        }
                    }
                    out
                }
                (Some(h), None) => {
                    let mut out = String::new();
                    for (j, l) in lines.iter().enumerate() {
                        out.push_str(l);
                        if j == h {
                            if !l.ends_with('\n') {
                                out.push('\n');
                            }
                            out.push_str(&format!("enabled = {raw}\n"));
                        }
                    }
                    out
                }
                (None, None) => {
                    let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
                    format!("{text}{sep}[wallpaper]\nenabled = {raw}\n")
                }
            };
            (new, noctalia_expect(&before, Some(raw_value), false))
        }
        Previous::KeyAbsent { parent_created, prefix } => {
            let new = noctalia_remove_key(&lines, &spot, *parent_created, prefix);
            (new, noctalia_expect(&before, None, *parent_created))
        }
        Previous::FileAbsent => {
            let new = noctalia_remove_key(&lines, &spot, true, "");
            let expect = noctalia_expect(&before, None, true);
            if new.trim().is_empty() && expect.is_empty() {
                return Ok(Restored::Delete);
            }
            (new, expect)
        }
    };
    let after = parse_toml(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != expect {
        return Err(EditError::UnsafeForm);
    }
    Ok(Restored::Write(new))
}

/// Remove the `enabled` line from `[wallpaper]`; if Rexon created the table
/// and it became empty, remove the header too; if the edit was at the very
/// end, also remove the `prefix` appended back then.
fn noctalia_remove_key(lines: &[&str], spot: &NoctaliaSpot, drop_empty: bool, prefix: &str) -> String {
    let Some((ki, _, _)) = spot.key else {
        return lines.concat();
    };
    let mut drop: Vec<usize> = vec![ki];
    if drop_empty {
        if let Some(h) = spot.header {
            // The table is empty if up to the next header (or the end) there is
            // nothing but the line being removed.
            let end = (h + 1..lines.len())
                .find(|&j| toml_header(lines[j]).is_some())
                .unwrap_or(lines.len());
            // Empty lines inside do not count (the shell itself could have
            // added them when rewriting the file) and stay in place; only the
            // header and the key are removed.
            if (h + 1..end).all(|j| j == ki || lines[j].trim().is_empty()) {
                drop.push(h);
            }
        }
    }
    // The edit was at the very end if nothing but removed lines remains
    // after the first removed line.
    let first = drop.iter().copied().min().unwrap_or(lines.len());
    let at_end = (first..lines.len()).all(|j| drop.contains(&j));
    let mut out: String = lines
        .iter()
        .enumerate()
        .filter(|(j, _)| !drop.contains(j))
        .map(|(_, l)| *l)
        .collect();
    if at_end && !prefix.is_empty() && out.ends_with(prefix) {
        out.truncate(out.len() - prefix.len());
    }
    out
}

// ── DMS: JSON by byte bounds ───────────────────────────────────────────────

/// A JSON object member: the key and the byte bounds of the value.
#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
        i += 1;
    }
    i
}

/// End of the JSON string starting at `i` (on the quote): the index after it.
fn string_end(b: &[u8], mut i: usize) -> Option<usize> {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// End of the JSON value starting at `i`.
fn value_end(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut j = i;
            while j < b.len() {
                match b[j] {
                    b'"' => {
                        j = string_end(b, j)?;
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
            while j < b.len() && !matches!(b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                j += 1;
            }
            Some(j)
        }
    }
}

/// Members of the object opened at `open` (`{`).
fn object_members(text: &str, open: usize) -> Option<Vec<Member>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = skip_ws(b, open + 1);
    if b.get(i) == Some(&b'}') {
        return Some(out);
    }
    loop {
        if b.get(i) != Some(&b'"') {
            return None;
        }
        let ke = string_end(b, i)?;
        let key: String = serde_json::from_str(&text[i..ke]).ok()?;
        let mut j = skip_ws(b, ke);
        if b.get(j) != Some(&b':') {
            return None;
        }
        j = skip_ws(b, j + 1);
        let ve = value_end(b, j)?;
        out.push(Member { key, key_start: i, value_start: j, value_end: ve });
        let k = skip_ws(b, ve);
        match b.get(k) {
            Some(b',') => i = skip_ws(b, k + 1),
            Some(b'}') => return Some(out),
            _ => return None,
        }
    }
}

/// Insert a member first into the `{` object at `open`, with the same
/// indent as the former first member.
fn insert_member(text: &str, open: usize, members: &[Member], member: &str) -> String {
    let mut out = String::with_capacity(text.len() + member.len() + 8);
    out.push_str(&text[..open + 1]);
    if let Some(first) = members.first() {
        let ws = &text[open + 1..first.key_start];
        out.push_str(ws);
        out.push_str(member);
        out.push(',');
        out.push_str(&text[open + 1..]);
    } else {
        out.push_str(member);
        out.push_str(&text[open + 1..]);
    }
    out
}

/// Remove member `idx` from the `{` object at `open` together with its comma.
fn remove_member(text: &str, open: usize, members: &[Member], idx: usize) -> String {
    let b = text.as_bytes();
    let m = &members[idx];
    let (start, end) = if members.len() == 1 {
        (open + 1, m.value_end)
    } else if idx + 1 < members.len() {
        let start = if idx == 0 {
            open + 1
        } else {
            skip_ws(b, members[idx - 1].value_end) + 1
        };
        let end = skip_ws(b, m.value_end) + 1;
        (start, end)
    } else {
        (skip_ws(b, members[idx - 1].value_end), m.value_end)
    };
    format!("{}{}", &text[..start], &text[end..])
}

/// Where the `screenPreferences` object and its `wallpaper` key are.
struct DmsSpot {
    root_open: usize,
    root: Vec<Member>,
    prefs: Option<usize>,
    prefs_members: Vec<Member>,
    wallpaper: Option<usize>,
}

fn dms_spot(text: &str) -> Result<DmsSpot, EditError> {
    let b = text.as_bytes();
    let root_open = skip_ws(b, 0);
    if b.get(root_open) != Some(&b'{') {
        return Err(EditError::UnsafeForm);
    }
    let root = object_members(text, root_open).ok_or(EditError::UnsafeForm)?;
    let prefs = root.iter().position(|m| m.key == "screenPreferences");
    let mut prefs_members = Vec::new();
    let mut wallpaper = None;
    if let Some(p) = prefs {
        let at = root[p].value_start;
        if b[at] != b'{' {
            return Err(EditError::UnsafeForm);
        }
        prefs_members = object_members(text, at).ok_or(EditError::UnsafeForm)?;
        wallpaper = prefs_members.iter().position(|m| m.key == "wallpaper");
    }
    Ok(DmsSpot { root_open, root, prefs, prefs_members, wallpaper })
}

fn parse_json(text: &str) -> Result<Value, EditError> {
    let v: Value = serde_json::from_str(text).map_err(|_| EditError::Unparsable)?;
    if !v.is_object() {
        return Err(EditError::UnsafeForm);
    }
    Ok(v)
}

/// The parsed settings with `screenPreferences.wallpaper` replaced.
fn dms_expect(base: &Value, value: Option<Value>, drop_empty: bool) -> Value {
    let mut v = base.clone();
    let root = v.as_object_mut().expect("корень — объект (проверено разбором)");
    match value {
        Some(x) => {
            let p = root
                .entry("screenPreferences")
                .or_insert_with(|| Value::Object(Default::default()));
            if let Some(p) = p.as_object_mut() {
                p.insert("wallpaper".to_string(), x);
            }
        }
        None => {
            let mut empty = false;
            if let Some(p) = root.get_mut("screenPreferences").and_then(|p| p.as_object_mut()) {
                p.remove("wallpaper");
                empty = p.is_empty();
            }
            if drop_empty && empty {
                root.remove("screenPreferences");
            }
        }
    }
    v
}

const DMS_EMPTY: &str = "[]";

/// Turn off the DMS built-in wallpaper in the `settings.json` text (`None`
/// means no file).
pub fn dms_disable(text: Option<&str>) -> Result<(String, Previous), EditError> {
    let Some(text) = text else {
        return Ok(("{\"screenPreferences\": {\"wallpaper\": []}}\n".to_string(), Previous::FileAbsent));
    };
    let before = parse_json(text)?;
    let spot = dms_spot(text)?;
    let (new, prev) = match (spot.prefs, spot.wallpaper) {
        (Some(_), Some(w)) => {
            let m = &spot.prefs_members[w];
            let raw = text[m.value_start..m.value_end].to_string();
            let new = format!("{}{DMS_EMPTY}{}", &text[..m.value_start], &text[m.value_end..]);
            (new, Previous::Value { raw })
        }
        (Some(p), None) => {
            let open = spot.root[p].value_start;
            let new = insert_member(text, open, &spot.prefs_members, "\"wallpaper\": []");
            (new, Previous::KeyAbsent { parent_created: false, prefix: String::new() })
        }
        (None, _) => {
            let new = insert_member(
                text,
                spot.root_open,
                &spot.root,
                "\"screenPreferences\": {\"wallpaper\": []}",
            );
            (new, Previous::KeyAbsent { parent_created: true, prefix: String::new() })
        }
    };
    let after = parse_json(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != dms_expect(&before, Some(Value::Array(Vec::new())), false) {
        return Err(EditError::UnsafeForm);
    }
    Ok((new, prev))
}

/// Give the DMS built-in wallpaper back what stood there before Rexon's edit.
pub fn dms_restore(text: Option<&str>, prev: &Previous) -> Result<Restored, EditError> {
    let Some(text) = text else {
        return Ok(match prev {
            Previous::Value { raw } => {
                Restored::Write(format!("{{\"screenPreferences\": {{\"wallpaper\": {raw}}}}}\n"))
            }
            _ => Restored::Delete,
        });
    };
    let before = parse_json(text)?;
    let spot = dms_spot(text)?;
    let (new, expect) = match prev {
        Previous::Value { raw } => {
            let raw_value: Value = serde_json::from_str(raw).map_err(|_| EditError::UnsafeForm)?;
            let new = match (spot.prefs, spot.wallpaper) {
                (Some(_), Some(w)) => {
                    let m = &spot.prefs_members[w];
                    format!("{}{raw}{}", &text[..m.value_start], &text[m.value_end..])
                }
                (Some(p), None) => insert_member(
                    text,
                    spot.root[p].value_start,
                    &spot.prefs_members,
                    &format!("\"wallpaper\": {raw}"),
                ),
                (None, _) => insert_member(
                    text,
                    spot.root_open,
                    &spot.root,
                    &format!("\"screenPreferences\": {{\"wallpaper\": {raw}}}"),
                ),
            };
            (new, dms_expect(&before, Some(raw_value), false))
        }
        Previous::KeyAbsent { .. } | Previous::FileAbsent => {
            let drop_parent = matches!(
                prev,
                Previous::FileAbsent | Previous::KeyAbsent { parent_created: true, .. }
            );
            let mut new = text.to_string();
            if let (Some(p), Some(w)) = (spot.prefs, spot.wallpaper) {
                let open = spot.root[p].value_start;
                new = remove_member(text, open, &spot.prefs_members, w);
                if drop_parent && spot.prefs_members.len() == 1 {
                    let again = dms_spot(&new)?;
                    if let Some(p2) = again.prefs {
                        new = remove_member(&new, again.root_open, &again.root, p2);
                    }
                }
            }
            let expect = dms_expect(&before, None, drop_parent);
            if matches!(prev, Previous::FileAbsent)
                && expect.as_object().is_some_and(|o| o.is_empty())
            {
                return Ok(Restored::Delete);
            }
            (new, expect)
        }
    };
    let after = parse_json(&new).map_err(|_| EditError::UnsafeForm)?;
    if after != expect {
        return Err(EditError::UnsafeForm);
    }
    Ok(Restored::Write(new))
}

// ============================================================================
// Execution: files on disk
// ============================================================================

/// Rexon's note about a turned-off shell wallpaper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handover {
    /// `noctalia` | `dms`.
    pub shell: String,
    /// Which file was edited (as named, before resolving the link).
    pub file: String,
    /// What stood there before the edit.
    pub previous: Previous,
}

impl Handover {
    pub fn desktop_shell(&self) -> Option<DesktopShell> {
        DesktopShell::all().iter().copied().find(|s| s.as_str() == self.shell)
    }
}

/// All notes: one per shell whose wallpaper Rexon currently keeps turned
/// off. A machine can have more than one shell (DMS under niri, Noctalia
/// under Hyprland), and turning off one must not erase the memory of the
/// other.
pub fn read_handovers(home: &ShellHome) -> Vec<Handover> {
    std::fs::read_to_string(home.handover_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The note about this shell, if Rexon keeps its wallpaper turned off.
pub fn read_handover(home: &ShellHome, shell: DesktopShell) -> Option<Handover> {
    read_handovers(home).into_iter().find(|h| h.desktop_shell() == Some(shell))
}

/// Write the notes; empty means there is no notes file at all.
fn write_handovers(home: &ShellHome, list: &[Handover]) -> Result<(), String> {
    if list.is_empty() {
        return match std::fs::remove_file(home.handover_file()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string_pretty(list).expect("памятки сериализуются");
    write_text(&home.handover_file(), &text)
}

fn write_text(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    crate::utils::atomic_write(&path.to_string_lossy(), text).map_err(|e| format!("{}: {e}", path.display()))
}

fn shell_edit_error(shell: DesktopShell, reason: &str) -> String {
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell), %2 the reason.
    trf("Could not change the settings of %1: %2. Nothing was changed.", &[&shell.label(), &reason])
}

/// Whether writing is allowed: an existing read-only file means refusal
/// (DMS itself marks this way the settings the person locked from edits).
fn writable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => true,
    }
}

/// "Give the wallpaper to Rexon": turn off the shell's wallpaper,
/// remembering the previous value.
///
/// Order: read -> compute the edit (with the parse check) -> write the note
/// -> backup -> replace the file. A failure at any step before the replace
/// changes nothing for the shell; a failure of the replace removes the
/// note. Returns a text for the person.
pub fn hand_over(shell: DesktopShell, home: &ShellHome) -> Result<String, String> {
    let before = read_handovers(home);
    let ours = before.iter().any(|h| h.desktop_shell() == Some(shell));
    if ours && shell_draws_wallpaper(shell, home) == Some(false) {
        // Already turned off by us: a second edit would overwrite the value that
        // stood BEFORE Rexon in the note with our own `false`.
        return Ok(hand_over_done(shell));
    }
    let file = home.managed_file(shell);
    let current = read_optional(&file)
        .map_err(|_| shell_edit_error(shell, &tr("the settings file could not be read")))?;
    if current.is_some() && !writable(&file) {
        return Err(shell_edit_error(shell, &tr("the settings file is read-only")));
    }
    let edited = match shell {
        DesktopShell::Noctalia => noctalia_disable(current.as_deref()),
        DesktopShell::DankMaterialShell => dms_disable(current.as_deref()),
    };
    let (new_text, previous) = edited.map_err(|e| shell_edit_error(shell, &e.message()))?;
    let mut after: Vec<Handover> =
        before.iter().filter(|h| h.desktop_shell() != Some(shell)).cloned().collect();
    after.push(Handover {
        shell: shell.as_str().to_string(),
        file: file.to_string_lossy().into_owned(),
        previous,
    });
    let write_failed = || shell_edit_error(shell, &tr("the settings file could not be written"));
    write_handovers(home, &after).map_err(|_| write_failed())?;
    if let Some(old) = &current {
        if write_text(&backup_path(&file), old).is_err() {
            let _ = write_handovers(home, &before);
            return Err(write_failed());
        }
    }
    if write_text(&file, &new_text).is_err() {
        let _ = write_handovers(home, &before);
        return Err(write_failed());
    }
    Ok(hand_over_done(shell))
}

fn hand_over_done(shell: DesktopShell) -> String {
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell).
    trf(
        "The wallpaper of %1 is turned off — the background now belongs to Rexon. \
         If the desktop is empty, apply a Rexon wallpaper.",
        &[&shell.label()],
    )
}

/// "Return to the shell": put back for the `shell` wallpaper what stood
/// there before Rexon. Only a successful restore removes the note.
pub fn give_back(shell: DesktopShell, home: &ShellHome) -> Result<String, String> {
    let all = read_handovers(home);
    let Some(record) = all.iter().find(|h| h.desktop_shell() == Some(shell)).cloned() else {
        return Err(tr("There is nothing to return: Rexon did not turn off the shell's wallpaper."));
    };
    let file = PathBuf::from(&record.file);
    let current = read_optional(&file)
        .map_err(|_| shell_edit_error(shell, &tr("the settings file could not be read")))?;
    if current.is_some() && !writable(&file) {
        return Err(shell_edit_error(shell, &tr("the settings file is read-only")));
    }
    let restored = match shell {
        DesktopShell::Noctalia => noctalia_restore(current.as_deref(), &record.previous),
        DesktopShell::DankMaterialShell => dms_restore(current.as_deref(), &record.previous),
    }
    .map_err(|e| shell_edit_error(shell, &e.message()))?;
    let write_failed = || shell_edit_error(shell, &tr("the settings file could not be written"));
    match restored {
        Restored::Write(text) => {
            if current.as_deref() != Some(text.as_str()) {
                if let Some(old) = &current {
                    write_text(&backup_path(&file), old).map_err(|_| write_failed())?;
                }
                write_text(&file, &text).map_err(|_| write_failed())?;
            }
        }
        Restored::Delete => {
            if current.is_some() {
                std::fs::remove_file(&file).map_err(|_| write_failed())?;
            }
        }
    }
    let rest: Vec<Handover> = all.into_iter().filter(|h| h.desktop_shell() != Some(shell)).collect();
    let _ = write_handovers(home, &rest);
    // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell).
    Ok(trf("The wallpaper of %1 is back on, with the setting it had before.", &[&shell.label()]))
}

// ============================================================================
// The live session
// ============================================================================

/// Filter out stale service files: a file in the session directory proves
/// that the shell WAS HERE (files remain after a crash).
///
/// * `danklinux-<pid>.*` is live only if process `<pid>` is alive (that is
///   the backend PID, also in the name);
/// * `noctalia-*.sock|.lock` only if there is a live `noctalia` process
///   (we do not take a flock on someone else's lock file: at the moment the
///   shell starts it would steal its single-instance lock);
/// * the rest as is.
pub fn live_runtime_files(
    files: &[String],
    pid_alive: impl Fn(u32) -> bool,
    noctalia_running: bool,
) -> Vec<String> {
    files
        .iter()
        .filter(|f| {
            if let Some(rest) = f.strip_prefix("danklinux-") {
                let pid = rest.split('.').next().and_then(|p| p.parse::<u32>().ok());
                return pid.is_some_and(&pid_alive);
            }
            if f.starts_with("noctalia-") && (f.ends_with(".sock") || f.ends_with(".lock")) {
                return noctalia_running;
            }
            true
        })
        .cloned()
        .collect()
}

/// Session observations gathered on this machine (the owning form of
/// [`ShellFacts`]).
#[derive(Debug, Clone, Default)]
pub struct LiveFacts {
    pub wayland_display: String,
    pub runtime_files: Vec<String>,
    pub process_names: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl LiveFacts {
    /// Gather without a single external process: the session directory, our
    /// own user's `/proc` and our own environment. Bus names and systemd units
    /// are NOT gathered here: those are `busctl`/`systemctl` calls, and the
    /// gathering is called from the UI thread; a file, a process and an
    /// environment marker are enough for a live shell.
    pub fn collect() -> LiveFacts {
        let procs = own_processes();
        let names: Vec<String> = procs.iter().map(|(_, n)| n.clone()).collect();
        let noctalia_running = names.iter().any(|n| n == "noctalia");
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
        let files: Vec<String> = if runtime_dir.is_empty() {
            Vec::new()
        } else {
            std::fs::read_dir(&runtime_dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let alive = |pid: u32| Path::new(&format!("/proc/{pid}")).exists();
        let env = SHELL_ENV_NAMES
            .iter()
            .filter_map(|k| {
                let v = std::env::var(k).ok()?;
                // A marker path to a socket that no longer exists is a memory
                // of a past shell, not the shell itself.
                if *k == "DMS_SOCKET" && !v.is_empty() && !Path::new(&v).exists() {
                    return None;
                }
                Some((k.to_string(), v))
            })
            .collect();
        LiveFacts {
            wayland_display: std::env::var("WAYLAND_DISPLAY").unwrap_or_default(),
            runtime_files: live_runtime_files(&files, alive, noctalia_running),
            process_names: names,
            env,
        }
    }

    /// The core's answer for these observations.
    pub fn detect(&self) -> Option<ShellDetection> {
        let env: Vec<(&str, &str)> = self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        detect_shell(&ShellFacts {
            wayland_display: &self.wayland_display,
            runtime_files: &self.runtime_files,
            active_user_units: &[],
            dbus_names: &[],
            process_names: &self.process_names,
            env: &env,
        })
    }
}

/// Processes of our own user: (pid, name from `/proc/<pid>/comm`).
fn own_processes() -> Vec<(u32, String)> {
    use std::os::unix::fs::MetadataExt;
    let my_uid = std::fs::metadata("/proc/self").map(|m| m.uid()).ok();
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            if my_uid.is_some() && e.metadata().ok().map(|m| m.uid()) != my_uid {
                return None;
            }
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            Some((pid, comm.trim_end().to_string()))
        })
        .collect()
}

// ============================================================================
// What to show the person
// ============================================================================

/// The "whose background" bar on the wallpaper tab.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShellBanner {
    pub visible: bool,
    pub text: String,
    /// Show "Give the wallpaper to Rexon".
    pub can_give: bool,
    /// Show "Return to the shell".
    pub can_return: bool,
    /// Which shell (`as_str`); empty means none.
    pub shell: String,
}

/// Label of the "Give the wallpaper to Rexon" button.
pub fn give_label() -> String {
    // TRANSLATORS: button; turns off the desktop shell's own wallpaper so Rexon's wallpaper shows.
    tr("Give Wallpaper to Rexon")
}

/// Label of the "Return to the shell" button.
pub fn return_label() -> String {
    // TRANSLATORS: button; turns the desktop shell's own wallpaper back on, as it was before Rexon.
    tr("Return to Shell")
}

/// The bar's decision. Shown ONLY with a live shell:
///   * the shell holds the background: "Give the wallpaper to Rexon";
///   * the background is ours because Rexon turned off this shell's
///     wallpaper: "Return to the shell";
///   * the background is ours for another reason (the person turned off
///     the shell's wallpaper themselves, X11, GNOME): no bar, there is
///     nothing to argue about.
pub fn shell_banner(
    detected: Option<DesktopShell>,
    owner: WallpaperOwner,
    handed_over: Option<DesktopShell>,
) -> ShellBanner {
    let Some(shell) = detected else {
        return ShellBanner::default();
    };
    if let WallpaperOwner::Shell(s) = owner {
        let text = trf(
            // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). The quoted names are the two buttons next to this text.
            "The background is drawn by %1, and its own wallpaper covers Rexon's. \
             “Give Wallpaper to Rexon” turns the shell's wallpaper off; Rexon \
             remembers the previous setting, and “Return to Shell” brings it back.",
            &[&s.label()],
        );
        return ShellBanner {
            visible: true,
            text,
            can_give: true,
            can_return: false,
            shell: s.as_str().to_string(),
        };
    }
    if handed_over == Some(shell) {
        let text = trf(
            // TRANSLATORS: %1 is the shell name (Noctalia, DankMaterialShell). “Return to Shell” is the button next to this text.
            "The wallpaper of %1 is turned off at your request, and the background \
             is drawn by Rexon. “Return to Shell” turns it back on.",
            &[&shell.label()],
        );
        return ShellBanner {
            visible: true,
            text,
            can_give: false,
            can_return: true,
            shell: shell.as_str().to_string(),
        };
    }
    ShellBanner::default()
}

/// The "whose background" state in this session.
#[derive(Debug, Clone)]
pub struct ShellWallpaperState {
    pub detection: Option<ShellDetection>,
    pub owner: WallpaperOwner,
    pub handed_over: Option<DesktopShell>,
    pub banner: ShellBanner,
}

/// Assemble the state from ready observations and settings roots.
pub fn shell_wallpaper_state(
    de: &str,
    session_type: &str,
    detection: Option<ShellDetection>,
    home: &ShellHome,
) -> ShellWallpaperState {
    let shell = detection.as_ref().map(|d| d.shell);
    let draws = shell.and_then(|s| shell_draws_wallpaper(s, home));
    let owner = wallpaper_layer_owner(de, session_type, shell, draws);
    let handed_over = shell.filter(|s| read_handover(home, *s).is_some());
    let banner = shell_banner(shell, owner, handed_over);
    ShellWallpaperState { detection, owner, handed_over, banner }
}

/// The same for the live machine: the environment and the session are
/// recognised the same way the wallpaper engine recognises them.
pub fn live_shell_wallpaper_state() -> ShellWallpaperState {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::wallpaper_engine::detect_de(&env("XDG_CURRENT_DESKTOP"), &env("DESKTOP_SESSION"));
    let session = crate::wallpaper_engine::detect_session_type(
        &env("XDG_SESSION_TYPE"),
        &env("WAYLAND_DISPLAY"),
        &env("DISPLAY"),
    );
    shell_wallpaper_state(&de, &session, LiveFacts::collect().detect(), &ShellHome::live())
}
