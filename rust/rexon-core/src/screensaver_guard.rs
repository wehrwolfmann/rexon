//! Guarding each desktop's own screen saver: Rexon does not let another
//! desktop's screen saver take over without the person's explicit choice.
//!
//! WHY. Rexon installs many desktops side by side, and each brings its own
//! screen saver (the one that also locks the screen). Their autostart entries
//! are already confined by `OnlyShowIn=` (mate-screensaver → MATE,
//! xfce4-screensaver → XFCE, xscreensaver → LXQt through
//! `lxqt-xscreensaver-autostart.desktop`, csd-screensaver-proxy → Cinnamon),
//! so at login there is no conflict. The conflict comes from the menu: the
//! "XScreenSaver Settings" launcher (`xscreensaver-settings.desktop`) is shown
//! in the MATE Control Center and in the menus of other desktops; it offers to
//! "Kill" the running mate-screensaver (the screen no longer locks) or to
//! "Launch" a second daemon next to the desktop's own. The XScreenSaver daemon
//! launcher (`xscreensaver.desktop`) is not confined at all. Seen on
//! Wehrwolfmann's machine on 24.11.2026: CachyOS installs xscreensaver
//! together with XFCE.
//!
//! WHAT IS DONE.
//! * Hiding ([`sync_live`]): for every known launcher of a screen saver's
//!   settings or daemon ([`LAUNCHERS`]) Rexon puts a user copy into
//!   `$XDG_DATA_HOME/applications` with the desktops where the saver is NOT
//!   the desktop's own added to `NotShowIn=` (or removed from `OnlyShowIn=`).
//!   The user directory shadows the system one by file name, so nothing in the
//!   system is touched, and deleting the copy brings everything back. The copy
//!   carries [`MARKER_LINE`]; a file without it belongs to the person and is
//!   never touched.
//! * The person's choice wins: a saver the person starts with their own
//!   autostart entry (`~/.config/autostart`), or the XFCE lock command set to
//!   it, counts as chosen in the desktops that entry covers, and is neither
//!   hidden there nor closed.
//! * Checking the session ([`survey`]): on "Check / Repair" of the current
//!   desktop Rexon looks whether the desktop's own saver runs and whether a
//!   foreign one runs. If something is wrong the person gets a plain message
//!   and one button that brings the desktop's own saver back ([`restore_live`]):
//!   the own one is started, a foreign one is closed ONLY by PID, only in this
//!   session and only if the person did not choose it.
//!
//! The table of desktops ([`DESKTOPS`]) is one for the whole product (rule
//! #12): a new desktop is a line here, and a test checks that every desktop
//! Rexon recognizes has one.

use std::path::{Path, PathBuf};

use crate::i18n::{tr, trf};
use crate::idle_screensaver::NativeModule;
use crate::{log_info, log_warn};

// ============================================================================
// Tables
// ============================================================================

/// A screen saver daemon Rexon knows by its program name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaverDaemon {
    /// The executable's name (the last segment of `/proc/<pid>/exe`).
    pub program: &'static str,
    /// The product name shown to the person (product names stay untranslated).
    pub name: &'static str,
    /// How the desktop's session starts it; empty — Rexon never starts it.
    pub start: &'static [&'static str],
}

/// Screen saver daemons. A daemon not listed here is never closed by Rexon.
pub const DAEMONS: &[SaverDaemon] = &[
    SaverDaemon { program: "xscreensaver", name: "XScreenSaver", start: &["xscreensaver", "-no-splash"] },
    SaverDaemon { program: "mate-screensaver", name: "MATE Screensaver", start: &["mate-screensaver"] },
    SaverDaemon { program: "xfce4-screensaver", name: "Xfce Screensaver", start: &["xfce4-screensaver"] },
    SaverDaemon { program: "deepin-screensaver", name: "Deepin Screensaver", start: &[] },
    SaverDaemon { program: "light-locker", name: "Light Locker", start: &[] },
    SaverDaemon { program: "gnome-screensaver", name: "GNOME Screensaver", start: &[] },
    SaverDaemon { program: "budgie-screensaver", name: "Budgie Screensaver", start: &[] },
];

/// The daemon by program name.
pub fn daemon(program: &str) -> Option<&'static SaverDaemon> {
    DAEMONS.iter().find(|d| d.program == program)
}

/// The daemon of the stock screen saver module of [`crate::idle_screensaver`].
pub fn daemon_of_module(m: NativeModule) -> Option<&'static SaverDaemon> {
    daemon(m.id())
}

/// What locks the screen in a desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSaver {
    /// A separate daemon the session starts from its autostart; Rexon sees it
    /// and, where [`SaverDaemon::start`] is known, starts it again (X11
    /// sessions only: all these daemons are X11).
    Daemon(&'static str),
    /// Built into the desktop's shell or started on demand over D-Bus
    /// (KWin's screen locker, GNOME Shell, cinnamon-screensaver…): there is
    /// nothing to start, any other daemon is foreign.
    BuiltIn,
    /// The person sets up the locker themselves (wlroots compositors:
    /// swaylock, hyprlock…): Rexon does not interfere at all.
    PersonsChoice,
}

/// One desktop: Rexon's identifier, its `XDG_CURRENT_DESKTOP` names (used in
/// `NotShowIn=`) and its own screen saver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopSaver {
    pub de: &'static str,
    pub tokens: &'static [&'static str],
    pub native: NativeSaver,
}

pub const DESKTOPS: &[DesktopSaver] = &[
    DesktopSaver { de: "kde", tokens: &["KDE"], native: NativeSaver::BuiltIn },
    DesktopSaver { de: "gnome", tokens: &["GNOME"], native: NativeSaver::BuiltIn },
    DesktopSaver { de: "cinnamon-mint", tokens: &["X-Cinnamon"], native: NativeSaver::BuiltIn },
    DesktopSaver { de: "budgie", tokens: &["Budgie"], native: NativeSaver::Daemon("budgie-screensaver") },
    DesktopSaver { de: "pantheon", tokens: &["Pantheon"], native: NativeSaver::Daemon("light-locker") },
    DesktopSaver { de: "deepin", tokens: &["Deepin", "DDE"], native: NativeSaver::Daemon("deepin-screensaver") },
    DesktopSaver { de: "cosmic", tokens: &["COSMIC"], native: NativeSaver::BuiltIn },
    DesktopSaver { de: "ukui", tokens: &["UKUI"], native: NativeSaver::BuiltIn },
    DesktopSaver { de: "mate", tokens: &["MATE"], native: NativeSaver::Daemon("mate-screensaver") },
    DesktopSaver { de: "xfce", tokens: &["XFCE"], native: NativeSaver::Daemon("xfce4-screensaver") },
    DesktopSaver { de: "lxqt", tokens: &["LXQt"], native: NativeSaver::Daemon("xscreensaver") },
    DesktopSaver { de: "hyprland", tokens: &["Hyprland"], native: NativeSaver::PersonsChoice },
    DesktopSaver { de: "sway", tokens: &["sway"], native: NativeSaver::PersonsChoice },
    DesktopSaver { de: "niri", tokens: &["niri"], native: NativeSaver::PersonsChoice },
    DesktopSaver { de: "river", tokens: &["river"], native: NativeSaver::PersonsChoice },
    DesktopSaver { de: "wayfire", tokens: &["Wayfire"], native: NativeSaver::PersonsChoice },
    DesktopSaver { de: "labwc", tokens: &["labwc"], native: NativeSaver::PersonsChoice },
];

/// The desktop by Rexon's identifier.
pub fn desktop(de: &str) -> Option<&'static DesktopSaver> {
    DESKTOPS.iter().find(|d| d.de == de)
}

/// A menu launcher that belongs to a screen saver daemon: its settings window
/// or the daemon itself. File names are those of the packages (Arch:
/// xscreensaver 6.x ships `xscreensaver-settings.desktop` and
/// `xscreensaver.desktop`; older releases and other distributions used
/// `xscreensaver-demo` / `xscreensaver-properties`). The launchers of
/// mate-screensaver and xfce4-screensaver are already confined by their
/// packages; they are listed so that a distribution that ships them without
/// `OnlyShowIn=` is handled the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Launcher {
    pub file: &'static str,
    pub daemon: &'static str,
}

pub const LAUNCHERS: &[Launcher] = &[
    Launcher { file: "xscreensaver-settings.desktop", daemon: "xscreensaver" },
    Launcher { file: "xscreensaver.desktop", daemon: "xscreensaver" },
    Launcher { file: "xscreensaver-demo.desktop", daemon: "xscreensaver" },
    Launcher { file: "xscreensaver-properties.desktop", daemon: "xscreensaver" },
    Launcher { file: "mate-screensaver-preferences.desktop", daemon: "mate-screensaver" },
    Launcher { file: "xfce4-screensaver-preferences.desktop", daemon: "xfce4-screensaver" },
];

/// The line that marks a user copy as made by Rexon.
pub const MARKER_LINE: &str = "X-Rexon-Screensaver-Guard=true";

/// The comment above the marker: what the copy is and how to undo it.
const MARKER_COMMENT: &str =
    "# Rexon: hides another desktop's screen saver here; delete this file to undo.";

// ============================================================================
// The person's choice
// ============================================================================

/// A screen saver the person runs themselves, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonChoice {
    pub program: String,
    /// `OnlyShowIn=` of the entry; empty — every desktop.
    pub only: Vec<String>,
    /// `NotShowIn=` of the entry.
    pub not: Vec<String>,
    /// `false` — the entry turns the program OFF (`Hidden=true`,
    /// `X-GNOME-Autostart-enabled=false`).
    pub enabled: bool,
}

impl PersonChoice {
    /// Whether the entry acts in a desktop with these names.
    pub fn covers(&self, tokens: &[&str]) -> bool {
        tokens
            .iter()
            .any(|t| (self.only.is_empty() || self.only.iter().any(|o| o == t)) && !self.not.iter().any(|n| n == t))
    }
}

fn list_value(v: &str) -> Vec<String> {
    v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The `[Desktop Entry]` keys as `(key, value)` pairs (localized keys too).
fn entry_keys(text: &str) -> Vec<(String, String)> {
    let mut in_entry = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
            continue;
        }
        if !in_entry || t.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

/// The program of an `Exec=` line: the first word that is not `env`, a
/// variable assignment or `sh -c`, by its last path segment.
fn exec_program(exec: &str) -> Option<String> {
    let mut words = exec.split_whitespace();
    loop {
        let w = words.next()?;
        if w == "env" || w.contains('=') {
            continue;
        }
        let base = w.rsplit('/').next().unwrap_or(w).trim_matches(['"', '\'']);
        return Some(base.to_string());
    }
}

/// An autostart entry of the person (`~/.config/autostart/<file>`) that is
/// about a known screen saver; `None` — it is about something else.
pub fn parse_autostart(file: &str, text: &str) -> Option<PersonChoice> {
    let keys = entry_keys(text);
    let get = |k: &str| keys.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let program = get("Exec")
        .and_then(exec_program)
        .or_else(|| get("TryExec").and_then(exec_program))
        .filter(|p| daemon(p).is_some())
        .or_else(|| {
            // An override with only `Hidden=true` has no `Exec=`: the system
            // entry of the same name tells whose it is.
            let stem = file.strip_suffix(".desktop").unwrap_or(file);
            daemon(stem).map(|d| d.program.to_string())
        })?;
    daemon(&program)?;
    let off = get("Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || get("X-GNOME-Autostart-enabled").is_some_and(|v| v.eq_ignore_ascii_case("false"))
        || get("X-MATE-Autostart-enabled").is_some_and(|v| v.eq_ignore_ascii_case("false"));
    Some(PersonChoice {
        program,
        only: get("OnlyShowIn").map(list_value).unwrap_or_default(),
        not: get("NotShowIn").map(list_value).unwrap_or_default(),
        enabled: !off,
    })
}

/// XFCE: the person set the session's lock command to a saver
/// (`xfce4-session.xml`, property `LockCommand`). Counts as a choice in XFCE.
pub fn parse_xfce_lock_command(xml: &str) -> Option<PersonChoice> {
    let line = xml.lines().find(|l| l.contains("name=\"LockCommand\""))?;
    let value = line.split("value=\"").nth(1)?.split('"').next()?;
    let program = exec_program(value)?;
    // `xscreensaver-command -lock` belongs to xscreensaver.
    let program = program.strip_suffix("-command").unwrap_or(&program).to_string();
    daemon(&program)?;
    Some(PersonChoice { program, only: vec!["XFCE".into()], not: Vec::new(), enabled: true })
}

/// Whether the person chose `program` in a desktop with these names.
pub fn chosen_in(choices: &[PersonChoice], program: &str, tokens: &[&str]) -> bool {
    choices.iter().any(|c| c.enabled && c.program == program && c.covers(tokens))
}

/// Whether the person turned `program` off in a desktop with these names.
pub fn disabled_in(choices: &[PersonChoice], program: &str, tokens: &[&str]) -> bool {
    choices.iter().any(|c| !c.enabled && c.program == program && c.covers(tokens))
}

// ============================================================================
// Hiding launchers: pure decisions
// ============================================================================

/// The desktops (their `XDG_CURRENT_DESKTOP` names) where a launcher of
/// `program` must not show: there the desktop has its own saver, it is
/// installed (a desktop with a separate daemon) and the person did not choose
/// `program` there. `installed` answers whether a program is on the machine.
pub fn hidden_tokens(program: &str, installed: &dyn Fn(&str) -> bool, choices: &[PersonChoice]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for d in DESKTOPS {
        let own_is_present = match d.native {
            NativeSaver::PersonsChoice => false,
            NativeSaver::BuiltIn => true,
            NativeSaver::Daemon(own) if own == program => false,
            // Without its own daemon the desktop may well use this one (XFCE
            // with xflock4 → xscreensaver): then it is not foreign there.
            NativeSaver::Daemon(own) => installed(own),
        };
        if own_is_present && !chosen_in(choices, program, d.tokens) {
            out.extend(d.tokens.iter().copied());
        }
    }
    out
}

/// Whether the file is our copy.
pub fn is_ours(text: &str) -> bool {
    text.lines().any(|l| l.trim() == MARKER_LINE)
}

/// Text of the user copy of a system launcher with `hidden` desktops taken
/// away. `None` — the system launcher already does not show there.
pub fn render_copy(system_text: &str, hidden: &[&str]) -> Option<String> {
    let keys = entry_keys(system_text);
    let get = |k: &str| keys.iter().find(|(key, _)| key == k).map(|(_, v)| list_value(v));
    let only = get("OnlyShowIn");
    let not = get("NotShowIn").unwrap_or_default();
    // The replacement for the visibility lines of `[Desktop Entry]`.
    let visibility: String = match &only {
        Some(only) => {
            let kept: Vec<&String> = only.iter().filter(|t| !hidden.contains(&t.as_str())).collect();
            if kept.len() == only.len() {
                return None;
            }
            if kept.is_empty() {
                "NoDisplay=true\n".to_string()
            } else {
                let list: String = kept.iter().map(|t| format!("{t};")).collect();
                format!("OnlyShowIn={list}\n")
            }
        }
        None => {
            let added: Vec<&str> = hidden.iter().copied().filter(|t| !not.iter().any(|n| n == t)).collect();
            if added.is_empty() {
                return None;
            }
            let mut list: Vec<String> = not.clone();
            list.extend(added.iter().map(|t| t.to_string()));
            let list: String = list.iter().map(|t| format!("{t};")).collect();
            format!("NotShowIn={list}\n")
        }
    };
    let mut out = String::new();
    let mut in_entry = false;
    for line in system_text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
            out.push_str(line);
            out.push('\n');
            if in_entry {
                out.push_str(MARKER_COMMENT);
                out.push('\n');
                out.push_str(MARKER_LINE);
                out.push('\n');
                out.push_str(&visibility);
            }
            continue;
        }
        if in_entry {
            let key = t.split_once('=').map(|(k, _)| k.trim());
            if matches!(key, Some("OnlyShowIn" | "NotShowIn")) {
                continue;
            }
            if only.as_ref().is_some_and(|_| visibility.starts_with("NoDisplay")) && key == Some("NoDisplay") {
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// What to do with one user copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Already as it should be, or the file belongs to the person.
    Nothing,
    /// Write this text.
    Write(String),
    /// Remove our copy (the system launcher is gone, or nothing to hide).
    Remove,
}

/// The decision for one launcher: `system` — the system file (`None` — the
/// package is not installed), `user` — the file in the user directory.
pub fn decide(system: Option<&str>, user: Option<&str>, hidden: &[&str]) -> Action {
    if user.is_some_and(|u| !is_ours(u)) {
        return Action::Nothing;
    }
    let desired = system.and_then(|s| render_copy(s, hidden));
    match (user, desired) {
        (Some(cur), Some(text)) if cur == text => Action::Nothing,
        (_, Some(text)) => Action::Write(text),
        (Some(_), None) => Action::Remove,
        (None, None) => Action::Nothing,
    }
}

// ============================================================================
// Hiding launchers: files
// ============================================================================

/// Where to look and where to write.
#[derive(Debug, Clone)]
pub struct GuardPaths {
    /// System `applications` directories in priority order.
    pub system_dirs: Vec<PathBuf>,
    /// `$XDG_DATA_HOME/applications`.
    pub user_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/autostart`.
    pub autostart_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/xfce4/xfconf/xfce-perchannel-xml/xfce4-session.xml`.
    pub xfce_session: PathBuf,
}

impl GuardPaths {
    pub fn from_env(home: &str, xdg_data_home: &str, xdg_config_home: &str, xdg_data_dirs: &str) -> Self {
        let abs = |v: &str| Some(v.to_string()).filter(|v| v.starts_with('/'));
        let data = crate::utils::data_home(abs(xdg_data_home).as_deref(), home);
        let config = crate::utils::config_home(abs(xdg_config_home).as_deref(), home);
        let mut system_dirs: Vec<PathBuf> = xdg_data_dirs
            .split(':')
            .filter(|d| d.starts_with('/'))
            .map(|d| Path::new(d).join("applications"))
            .collect();
        if system_dirs.is_empty() {
            system_dirs = vec!["/usr/local/share/applications".into(), "/usr/share/applications".into()];
        }
        GuardPaths {
            system_dirs,
            user_dir: Path::new(&data).join("applications"),
            autostart_dir: Path::new(&config).join("autostart"),
            xfce_session: Path::new(&config).join("xfce4/xfconf/xfce-perchannel-xml/xfce4-session.xml"),
        }
    }

    pub fn from_process() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        GuardPaths::from_env(&env("HOME"), &env("XDG_DATA_HOME"), &env("XDG_CONFIG_HOME"), &env("XDG_DATA_DIRS"))
    }
}

fn read_lossy(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|raw| String::from_utf8_lossy(&raw).into_owned())
}

/// The person's choices from their autostart directory and the XFCE session.
pub fn read_choices(paths: &GuardPaths) -> Vec<PersonChoice> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&paths.autostart_dir) {
        let mut files: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            .filter(|n| n.ends_with(".desktop"))
            .collect();
        files.sort();
        for file in files {
            // Our own copies of autostart entries are not the person's choice.
            let Some(text) = read_lossy(&paths.autostart_dir.join(&file)) else { continue };
            if text.lines().any(|l| l.trim().starts_with("X-Rexon-")) {
                continue;
            }
            if let Some(c) = parse_autostart(&file, &text) {
                out.push(c);
            }
        }
    }
    if let Some(c) = read_lossy(&paths.xfce_session).as_deref().and_then(parse_xfce_lock_command) {
        out.push(c);
    }
    out
}

/// One launcher's outcome — for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    pub file: &'static str,
    pub action: Action,
}

/// Bring the user copies in line. `installed` — whether a program exists.
pub fn sync(paths: &GuardPaths, installed: &dyn Fn(&str) -> bool) -> Vec<Result<Synced, String>> {
    let choices = read_choices(paths);
    let mut out = Vec::new();
    for l in LAUNCHERS {
        let system = paths.system_dirs.iter().find_map(|d| read_lossy(&d.join(l.file)));
        let user_path = paths.user_dir.join(l.file);
        let user = read_lossy(&user_path);
        let hidden = hidden_tokens(l.daemon, installed, &choices);
        let action = decide(system.as_deref(), user.as_deref(), &hidden);
        let done = match &action {
            Action::Nothing => Ok(()),
            Action::Write(text) => std::fs::create_dir_all(&paths.user_dir)
                .map_err(|e| format!("{}: {e}", paths.user_dir.display()))
                .and_then(|_| crate::utils::atomic_write_str_err(&user_path.to_string_lossy(), text)),
            Action::Remove => std::fs::remove_file(&user_path).map_err(|e| format!("{}: {e}", user_path.display())),
        };
        out.push(done.map(|_| Synced { file: l.file, action }));
    }
    out
}

/// Remove all our copies (Rexon uninstall). Returns the removed paths.
pub fn release(user_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for l in LAUNCHERS {
        let p = user_dir.join(l.file);
        if read_lossy(&p).is_some_and(|t| is_ours(&t)) && std::fs::remove_file(&p).is_ok() {
            out.push(p);
        }
    }
    out
}

/// Whether a program is on `PATH` (the live answer for [`sync`]/[`survey`]).
pub fn installed_live(program: &str) -> bool {
    crate::utils::which(program, std::env::var("PATH").ok().as_deref()).is_some()
}

/// The live session: put the copies in order. Kills and starts nothing;
/// menus watch the directory themselves. Failures go only to the log.
pub fn sync_live() {
    let paths = GuardPaths::from_process();
    if !paths.user_dir.is_absolute() {
        return;
    }
    for r in sync(&paths, &installed_live) {
        match r {
            Ok(Synced { action: Action::Nothing, .. }) => {}
            Ok(Synced { file, action }) => {
                let what = if action == Action::Remove { "removed" } else { "written" };
                log_info!("screensaver-guard", "launcher copy {what}: {}", paths.user_dir.join(file).display());
            }
            Err(e) => log_warn!("screensaver-guard", "launcher copy: {e}"),
        }
    }
}

// ============================================================================
// The session check
// ============================================================================

/// A screen saver daemon running in this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningSaver {
    pub pid: u32,
    pub program: String,
}

/// Observations of the session for [`survey`].
pub struct SessionFacts<'a> {
    /// The current desktop (`system::detect_current_de`).
    pub de: Option<&'a str>,
    pub wayland: bool,
    /// Whether this session's processes can be told apart (`XDG_SESSION_ID`
    /// is known). Without it nothing is judged: a "not running" would be a guess.
    pub session_known: bool,
    pub running: &'a [RunningSaver],
    pub installed: &'a dyn Fn(&str) -> bool,
    pub choices: &'a [PersonChoice],
}

/// What "bring the desktop's own saver back" will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restore {
    /// The desktop's own daemon to start (`None` — it runs, or there is none).
    pub start: Option<&'static SaverDaemon>,
    /// Foreign daemons to close, by PID.
    pub stop: Vec<RunningSaver>,
}

/// The session check. `None` — everything is in order (or nothing can be
/// judged here).
pub fn survey(f: &SessionFacts) -> Option<Restore> {
    if !f.session_known {
        return None;
    }
    let d = desktop(f.de?)?;
    let own = match d.native {
        NativeSaver::PersonsChoice => return None,
        NativeSaver::BuiltIn => None,
        NativeSaver::Daemon(p) => Some(p),
    };
    let start = own
        .filter(|_| !f.wayland)
        .filter(|p| (f.installed)(p))
        .filter(|p| !disabled_in(f.choices, p, d.tokens))
        .filter(|p| !f.running.iter().any(|r| r.program == *p))
        .and_then(daemon)
        .filter(|dm| !dm.start.is_empty());
    let stop: Vec<RunningSaver> = f
        .running
        .iter()
        .filter(|r| Some(r.program.as_str()) != own)
        .filter(|r| daemon(&r.program).is_some())
        // Without its own daemon on the machine the desktop may be using this
        // one on purpose (XFCE without xfce4-screensaver → xscreensaver).
        .filter(|_| own.is_none_or(|p| (f.installed)(p)))
        .filter(|r| !chosen_in(f.choices, &r.program, d.tokens))
        .cloned()
        .collect();
    if start.is_none() && stop.is_empty() {
        return None;
    }
    Some(Restore { start, stop })
}

/// The person-facing names of the daemons to close, without repeats.
fn stop_names(r: &Restore) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    for s in &r.stop {
        if let Some(d) = daemon(&s.program) {
            if !names.contains(&d.name) {
                names.push(d.name);
            }
        }
    }
    names
}

/// The question window: title.
pub fn restore_title() -> String {
    tr("Restore the screen saver?")
}

/// The operation's name (progress window and footer).
pub fn restore_operation_title() -> String {
    tr("Restoring the screen saver")
}

/// The question window: what is wrong and what will be done (simple HTML).
pub fn restore_text(r: &Restore) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(d) = r.start {
        parts.push(trf(
            // TRANSLATORS: %1 is a screen saver product name, e.g. "MATE Screensaver".
            "The screen saver of this desktop (%1) is not running, so the screen will not lock by itself when you step away.",
            &[&d.name],
        ));
    }
    let names = stop_names(r).join(", ");
    if !names.is_empty() {
        parts.push(trf(
            // TRANSLATORS: %1 is a screen saver product name, e.g. "XScreenSaver".
            "A screen saver from another desktop (%1) is running in this session. It can replace or switch off the desktop's own screen lock.",
            &[&names],
        ));
    }
    let mut todo: Vec<String> = Vec::new();
    if !names.is_empty() {
        // TRANSLATORS: %1 is a screen saver product name.
        todo.push(trf("Rexon will close %1 in this session only — nothing is uninstalled.", &[&names]));
    }
    if let Some(d) = r.start {
        // TRANSLATORS: %1 is a screen saver product name.
        todo.push(trf("Rexon will start %1 again.", &[&d.name]));
    }
    todo.push(tr("Your screen saver settings are not changed."));
    format!("{}<br><br>{}", parts.join("<br><br>"), todo.join("<br>"))
}

/// The note under the "Check / Repair" result: the question follows.
pub fn found_note() -> String {
    tr("Rexon found a problem with the screen saver and will offer to fix it when you close this window.")
}

// ============================================================================
// The session check and restore: live
// ============================================================================

/// The session's processes of known screen saver daemons. A process counts
/// only if it is ours (uid), its executable is a known daemon and it carries
/// our `XDG_SESSION_ID` ([`crate::backup::is_session_process`]).
pub fn running_live(my_session: &str) -> Vec<RunningSaver> {
    if my_session.is_empty() {
        return Vec::new();
    }
    // SAFETY: getuid has no preconditions and cannot fail.
    let my_uid = unsafe { libc::getuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut out = Vec::new();
    for pid in entries.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok()) {
        if let Some(program) = session_saver(pid, my_uid, my_session) {
            out.push(RunningSaver { pid, program });
        }
    }
    out.sort_by_key(|r| r.pid);
    out
}

/// The known daemon behind `pid` if it is a process of this session.
fn session_saver(pid: u32, my_uid: u32, my_session: &str) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let base = format!("/proc/{pid}");
    let meta = std::fs::metadata(&base).ok()?;
    let exe = std::fs::read_link(format!("{base}/exe")).ok()?;
    let exe = exe.to_string_lossy();
    let environ = std::fs::read(format!("{base}/environ")).ok()?;
    DAEMONS
        .iter()
        .find(|d| crate::backup::is_session_process(&exe, d.program, meta.uid(), &environ, my_uid, my_session))
        .map(|d| d.program.to_string())
}

/// The live check of the current session.
pub fn survey_live() -> Option<Restore> {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        !env("WAYLAND_DISPLAY").is_empty(),
    );
    let session = env("XDG_SESSION_ID");
    let running = running_live(&session);
    let choices = read_choices(&GuardPaths::from_process());
    let wayland = crate::wallpaper_backend::is_wayland_session(&env("XDG_SESSION_TYPE"), &env("WAYLAND_DISPLAY"));
    let found = survey(&SessionFacts {
        de: de.as_deref(),
        wayland,
        session_known: !session.is_empty(),
        running: &running,
        installed: &installed_live,
        choices: &choices,
    });
    if let Some(r) = &found {
        log_info!("screensaver-guard", "session check: start {:?}, foreign {:?}", r.start.map(|d| d.program), r.stop);
    }
    found
}

/// Start a daemon detached from Rexon (its own session, no output).
fn spawn_detached(argv: &[&str]) -> bool {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    let argv = crate::utils::resolved_argv(&argv, std::env::var("PATH").ok().as_deref());
    let Some((head, rest)) = argv.split_first() else { return false };
    let mut command = Command::new(head);
    command.args(rest).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: between fork and exec only setsid is called; it is async-signal-safe.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

/// "Bring the desktop's own saver back" on the live machine. Foreign daemons
/// are closed by PID after checking once more that the PID is still that
/// daemon of this session; the own one is started and awaited for 3 s.
pub fn restore_live(r: &Restore) -> Result<String, String> {
    let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
    // SAFETY: getuid has no preconditions and cannot fail.
    let my_uid = unsafe { libc::getuid() };
    for s in &r.stop {
        if session.is_empty() || session_saver(s.pid, my_uid, &session).as_deref() != Some(s.program.as_str()) {
            continue;
        }
        // SAFETY: a plain signal to one verified PID (never 0 or a group).
        let rc = unsafe { libc::kill(s.pid as libc::pid_t, libc::SIGTERM) };
        log_info!("screensaver-guard", "closed {} (pid {}): rc {rc}", s.program, s.pid);
    }
    let Some(d) = r.start else {
        return Ok(tr("The other screen saver has been closed. The desktop's own screen lock works as before."));
    };
    // TRANSLATORS: %1 is a screen saver product name.
    let failed = || trf("Could not start %1. Log out and log in again — the desktop starts it by itself.", &[&d.name]);
    if !spawn_detached(d.start) {
        log_warn!("screensaver-guard", "could not start {}", d.program);
        return Err(failed());
    }
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if running_live(&session).iter().any(|x| x.program == d.program) {
            log_info!("screensaver-guard", "started {}", d.program);
            // TRANSLATORS: %1 is a screen saver product name.
            return Ok(trf("%1 is running again: the screen will lock by itself as before.", &[&d.name]));
        }
    }
    log_warn!("screensaver-guard", "{} did not come up in 3 s", d.program);
    Err(failed())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XSS_SETTINGS: &str = "#!/usr/bin/env xdg-open\n[Desktop Entry]\nExec=xscreensaver-settings\nName=XScreenSaver Settings\nType=Application\nNotShowIn=KDE;GNOME;\n";
    const XSS_DAEMON: &str = "[Desktop Entry]\nExec=xscreensaver\nName=XScreenSaver\nType=Application\n\n[Desktop Action x]\nExec=xscreensaver-command -lock\n";

    fn all_installed(_: &str) -> bool {
        true
    }
    fn none_installed(_: &str) -> bool {
        false
    }

    #[test]
    fn every_recognized_desktop_has_a_row() {
        // Rule #12: a desktop Rexon recognizes and does not list here would get
        // no protection silently.
        for (_, de) in crate::system::XDG_TOKEN_TO_DE {
            assert!(desktop(de).is_some(), "no screen saver row for {de}");
        }
    }

    #[test]
    fn stock_modules_and_native_daemons_are_known() {
        for m in NativeModule::all() {
            assert!(daemon_of_module(*m).is_some(), "{}", m.id());
        }
        for d in DESKTOPS {
            if let NativeSaver::Daemon(p) = d.native {
                assert!(daemon(p).is_some(), "{p}");
            }
        }
        for l in LAUNCHERS {
            assert!(daemon(l.daemon).is_some(), "{}", l.file);
        }
    }

    #[test]
    fn xscreensaver_is_hidden_where_it_is_foreign_and_kept_in_lxqt() {
        let h = hidden_tokens("xscreensaver", &all_installed, &[]);
        for t in ["KDE", "GNOME", "X-Cinnamon", "MATE", "XFCE", "Budgie", "Deepin", "DDE", "COSMIC", "Pantheon"] {
            assert!(h.contains(&t), "{t}");
        }
        assert!(!h.contains(&"LXQt"));
        // Wayland compositors: the person's own locker, not ours to judge.
        assert!(!h.contains(&"sway") && !h.contains(&"Hyprland"));
    }

    #[test]
    fn desktop_without_its_own_daemon_keeps_xscreensaver() {
        // XFCE without xfce4-screensaver may lock through xscreensaver.
        let installed = |p: &str| p != "xfce4-screensaver";
        let h = hidden_tokens("xscreensaver", &installed, &[]);
        assert!(!h.contains(&"XFCE"));
        assert!(h.contains(&"MATE"));
        let h = hidden_tokens("xscreensaver", &none_installed, &[]);
        assert!(!h.contains(&"MATE") && h.contains(&"KDE"));
    }

    #[test]
    fn persons_autostart_is_a_choice_only_where_it_acts() {
        let c = parse_autostart("xss.desktop", "[Desktop Entry]\nExec=/usr/bin/xscreensaver -no-splash\nOnlyShowIn=MATE;\n").unwrap();
        assert!(c.enabled);
        let h = hidden_tokens("xscreensaver", &all_installed, &[c]);
        assert!(!h.contains(&"MATE"));
        assert!(h.contains(&"XFCE"));
    }

    #[test]
    fn disabled_override_is_not_a_choice() {
        let c = parse_autostart("mate-screensaver.desktop", "[Desktop Entry]\nHidden=true\n").unwrap();
        assert_eq!(c.program, "mate-screensaver");
        assert!(!c.enabled);
        assert!(!chosen_in(&[c.clone()], "mate-screensaver", &["MATE"]));
        assert!(disabled_in(&[c], "mate-screensaver", &["MATE"]));
        assert!(parse_autostart("foo.desktop", "[Desktop Entry]\nExec=foo\n").is_none());
    }

    #[test]
    fn xfce_lock_command_counts_as_choice() {
        let xml = r#"<channel name="xfce4-session"><property name="general" type="empty"><property name="LockCommand" type="string" value="xscreensaver-command -lock"/></property></channel>"#;
        let c = parse_xfce_lock_command(xml).unwrap();
        assert_eq!(c.program, "xscreensaver");
        assert!(!hidden_tokens("xscreensaver", &all_installed, &[c]).contains(&"XFCE"));
        assert!(parse_xfce_lock_command(r#"<property name="LockCommand" type="string" value="xflock4"/>"#).is_none());
    }

    #[test]
    fn copy_extends_not_show_in_and_keeps_the_rest() {
        let text = render_copy(XSS_SETTINGS, &["KDE", "MATE", "XFCE"]).unwrap();
        assert!(text.starts_with("#!/usr/bin/env xdg-open\n[Desktop Entry]\n"));
        assert!(text.contains("NotShowIn=KDE;GNOME;MATE;XFCE;\n"));
        assert_eq!(text.matches("NotShowIn=").count(), 1);
        assert!(text.contains("Exec=xscreensaver-settings\n") && is_ours(&text));
        // Nothing new to hide — no copy.
        assert!(render_copy(XSS_SETTINGS, &["KDE"]).is_none());
    }

    #[test]
    fn copy_touches_only_the_desktop_entry_group() {
        let text = render_copy(XSS_DAEMON, &["MATE"]).unwrap();
        assert!(text.contains("NotShowIn=MATE;\n"));
        assert!(text.contains("[Desktop Action x]\nExec=xscreensaver-command -lock\n"));
        assert_eq!(text.matches(MARKER_LINE).count(), 1);
    }

    #[test]
    fn copy_narrows_only_show_in() {
        let sys = "[Desktop Entry]\nExec=mate-screensaver-preferences\nOnlyShowIn=MATE;XFCE;\n";
        let text = render_copy(sys, &["XFCE"]).unwrap();
        assert!(text.contains("OnlyShowIn=MATE;\n") && !text.contains("NotShowIn"));
        let text = render_copy("[Desktop Entry]\nExec=x\nOnlyShowIn=XFCE;\nNoDisplay=false\n", &["XFCE"]).unwrap();
        assert!(text.contains("NoDisplay=true\n") && !text.contains("NoDisplay=false") && !text.contains("OnlyShowIn"));
        assert!(render_copy("[Desktop Entry]\nOnlyShowIn=MATE;\n", &["XFCE"]).is_none());
    }

    #[test]
    fn decisions_respect_the_persons_file() {
        let hidden = ["MATE"];
        let ours = render_copy(XSS_SETTINGS, &hidden).unwrap();
        assert_eq!(decide(Some(XSS_SETTINGS), None, &hidden), Action::Write(ours.clone()));
        assert_eq!(decide(Some(XSS_SETTINGS), Some(&ours), &hidden), Action::Nothing);
        // A person's own copy is never touched.
        assert_eq!(decide(Some(XSS_SETTINGS), Some("[Desktop Entry]\nExec=mine\n"), &hidden), Action::Nothing);
        // The package is gone, or there is nothing left to hide: our copy goes.
        assert_eq!(decide(None, Some(&ours), &hidden), Action::Remove);
        assert_eq!(decide(Some(XSS_SETTINGS), Some(&ours), &[]), Action::Remove);
        assert_eq!(decide(None, None, &hidden), Action::Nothing);
        // The system file changed: the copy follows it.
        let newer = XSS_SETTINGS.replace("Type=Application", "Type=Application\nIcon=new");
        assert!(matches!(decide(Some(&newer), Some(&ours), &hidden), Action::Write(t) if t.contains("Icon=new")));
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-ss-guard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn paths_in(root: &Path) -> GuardPaths {
        GuardPaths::from_env(
            &root.join("home").to_string_lossy(),
            "",
            "",
            &root.join("usr/share").to_string_lossy(),
        )
    }

    #[test]
    fn sync_and_release_in_a_temporary_home() {
        let root = scratch("sync");
        let p = paths_in(&root);
        std::fs::create_dir_all(&p.system_dirs[0]).unwrap();
        std::fs::write(p.system_dirs[0].join("xscreensaver-settings.desktop"), XSS_SETTINGS).unwrap();
        std::fs::write(p.system_dirs[0].join("xscreensaver.desktop"), XSS_DAEMON).unwrap();
        // The person's own copy of the daemon launcher.
        std::fs::create_dir_all(&p.user_dir).unwrap();
        std::fs::write(p.user_dir.join("xscreensaver.desktop"), "[Desktop Entry]\nExec=xscreensaver\n").unwrap();

        let done = sync(&p, &all_installed);
        assert!(done.iter().all(Result::is_ok));
        let copy = std::fs::read_to_string(p.user_dir.join("xscreensaver-settings.desktop")).unwrap();
        assert!(is_ours(&copy) && copy.contains("MATE;"));
        assert_eq!(std::fs::read_to_string(p.user_dir.join("xscreensaver.desktop")).unwrap(), "[Desktop Entry]\nExec=xscreensaver\n");
        // A second run changes nothing.
        assert!(sync(&p, &all_installed).iter().all(|r| matches!(r, Ok(Synced { action: Action::Nothing, .. }))));

        // The person chose xscreensaver everywhere through their autostart.
        std::fs::create_dir_all(&p.autostart_dir).unwrap();
        std::fs::write(p.autostart_dir.join("xss.desktop"), "[Desktop Entry]\nExec=xscreensaver -no-splash\n").unwrap();
        sync(&p, &all_installed);
        assert!(!p.user_dir.join("xscreensaver-settings.desktop").exists());

        std::fs::remove_file(p.autostart_dir.join("xss.desktop")).unwrap();
        sync(&p, &all_installed);
        assert_eq!(release(&p.user_dir), vec![p.user_dir.join("xscreensaver-settings.desktop")]);
        assert!(p.user_dir.join("xscreensaver.desktop").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn run(pid: u32, program: &str) -> RunningSaver {
        RunningSaver { pid, program: program.into() }
    }

    fn facts<'a>(de: &'a str, running: &'a [RunningSaver], choices: &'a [PersonChoice], installed: &'a dyn Fn(&str) -> bool) -> SessionFacts<'a> {
        SessionFacts { de: Some(de), wayland: false, session_known: true, running, installed, choices }
    }

    #[test]
    fn mate_after_kill_offers_to_start_its_own() {
        let r = survey(&facts("mate", &[], &[], &all_installed)).unwrap();
        assert_eq!(r.start.map(|d| d.program), Some("mate-screensaver"));
        assert!(r.stop.is_empty());
        assert!(survey(&facts("mate", &[run(5, "mate-screensaver")], &[], &all_installed)).is_none());
    }

    #[test]
    fn foreign_daemon_is_closed_unless_chosen() {
        let running = [run(5, "mate-screensaver"), run(9, "xscreensaver")];
        let r = survey(&facts("mate", &running, &[], &all_installed)).unwrap();
        assert_eq!(r.stop, vec![run(9, "xscreensaver")]);
        assert!(r.start.is_none());
        let chosen = [parse_autostart("x.desktop", "[Desktop Entry]\nExec=xscreensaver\n").unwrap()];
        assert!(survey(&facts("mate", &running, &chosen, &all_installed)).is_none());
        // KDE has a built-in locker: any daemon is foreign, nothing to start.
        let r = survey(&facts("kde", &[run(9, "xscreensaver")], &[], &all_installed)).unwrap();
        assert!(r.start.is_none() && r.stop.len() == 1);
        // Deepin's own daemon is not foreign there, and Rexon does not start it.
        assert!(survey(&facts("deepin", &[run(4, "deepin-screensaver")], &[], &all_installed)).is_none());
        assert!(survey(&facts("deepin", &[], &[], &all_installed)).is_none());
        // LXQt: xscreensaver is its own.
        assert!(survey(&facts("lxqt", &[run(9, "xscreensaver")], &[], &all_installed)).is_none());
    }

    #[test]
    fn nothing_is_judged_without_grounds() {
        // Unknown session, the person's compositor, the own daemon not installed
        // or turned off by the person, a Wayland session for an X11 daemon.
        let mut f = facts("mate", &[], &[], &all_installed);
        f.session_known = false;
        assert!(survey(&f).is_none());
        assert!(survey(&facts("sway", &[run(1, "xscreensaver")], &[], &all_installed)).is_none());
        assert!(survey(&facts("mate", &[], &[], &none_installed)).is_none());
        let off = [parse_autostart("mate-screensaver.desktop", "[Desktop Entry]\nHidden=true\n").unwrap()];
        assert!(survey(&facts("mate", &[], &off, &all_installed)).is_none());
        let mut f = facts("mate", &[], &[], &all_installed);
        f.wayland = true;
        assert!(survey(&f).is_none());
        // XFCE without xfce4-screensaver: xscreensaver may be its locker.
        let installed = |p: &str| p != "xfce4-screensaver";
        assert!(survey(&facts("xfce", &[run(3, "xscreensaver")], &[], &installed)).is_none());
    }

    #[test]
    fn restore_text_says_what_and_why() {
        let r = Restore { start: daemon("mate-screensaver"), stop: vec![run(9, "xscreensaver"), run(10, "xscreensaver")] };
        let t = restore_text(&r);
        assert!(t.contains("MATE Screensaver") && t.matches("XScreenSaver").count() == 2);
    }
}
