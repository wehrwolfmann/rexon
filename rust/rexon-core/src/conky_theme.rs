//! A Conky theme as a "folder with a description": `theme.json` and `preview.png`.
//!
//! WHY. Themes built with the Conky Studio visual editor (and those written by
//! hand for its manager) are folders that, besides the settings file, contain
//! a `theme.json` description: a display name, author, version, description,
//! required programs (`requires`) and screen (`monitor`). The format follows
//! the editor's documentation (the "Theme layout" wiki page). Rexon reads it
//! ONLY for reading and shows it to the person as its own card: the name,
//! author and description in the card, `preview.png` on the tile, `requires`
//! in the common check for missing programs (`conky_safety::health`: what is
//! in the repositories gets installed automatically, protected by the
//! password dialog), `monitor` as the widget's initial screen in its settings.
//!
//! Parsing is forgiving: unknown fields and fields of the wrong type are
//! silently skipped, a broken file is treated as if it did not exist. Rexon
//! does not write its own `theme.json`.

use crate::animations::LookIo;
use crate::de_apply::Ctx;

/// The name of the description file at the root of the theme folder.
pub const META_FILE: &str = "theme.json";

/// The theme picture for the tile.
pub const PREVIEW_FILE: &str = "preview.png";

/// Anything larger is not a description but something else: not read.
const META_MAX_BYTES: usize = 64 * 1024;

/// Anything longer is truncated (a description in a card, not a book).
const TEXT_MAX_CHARS: usize = 2000;

/// The theme description. All fields are optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeMeta {
    pub name: Option<String>,
    pub author: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    /// The resolution the theme was drawn for (`1920x1080`).
    pub resolution: Option<String>,
    /// What the theme needs: programs and conky features (`lua-cairo`).
    pub requires: Vec<String>,
    pub tags: Vec<String>,
    /// What it was made with (`conky-studio`).
    pub created_with: Option<String>,
    pub store_id: Option<String>,
    pub sha256: Option<String>,
    /// The screen: `auto`, `primary` or an output name (`DP-1`).
    pub monitor: Option<String>,
}

/// A string field: a string (or a number — `"version": 1.2`), trimmed, not
/// empty. Any other type gives `None`.
fn text_field(v: &serde_json::Value) -> Option<String> {
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    if s.is_empty() {
        return None;
    }
    Some(s.chars().take(TEXT_MAX_CHARS).collect())
}

/// A list of strings: an array (non-strings are skipped) or a single string.
fn list_field(v: &serde_json::Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: &str| {
        let s = s.trim();
        if !s.is_empty() && s.len() <= 200 && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    };
    match v {
        serde_json::Value::Array(items) => {
            for i in items.iter().take(100) {
                if let serde_json::Value::String(s) = i {
                    push(s);
                }
            }
        }
        serde_json::Value::String(s) => push(s),
        _ => {}
    }
    out
}

/// Parses the text of `theme.json`. `None` means it is not a JSON object.
pub fn parse_meta(text: &str) -> Option<ThemeMeta> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let map = v.as_object()?;
    let text = |k: &str| map.get(k).and_then(text_field);
    let list = |k: &str| map.get(k).map(list_field).unwrap_or_default();
    Some(ThemeMeta {
        name: text("name"),
        author: text("author"),
        version: text("version"),
        description: text("description"),
        resolution: text("resolution"),
        requires: list("requires"),
        tags: list("tags"),
        created_with: text("created_with"),
        store_id: text("store_id"),
        sha256: text("sha256"),
        monitor: text("monitor"),
    })
}

/// The theme description in folder `dir`; no file, too large or broken gives
/// `None`.
pub fn meta(io: &dyn LookIo, dir: &str) -> Option<ThemeMeta> {
    let text = io.read(&format!("{}/{META_FILE}", dir.trim_end_matches('/')))?;
    if text.len() > META_MAX_BYTES {
        return None;
    }
    parse_meta(&text)
}

/// The folder of widget `name` (the tile name), if the widget is a folder rather than a single file.
pub fn widget_dir(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    let path = crate::conky_scan::widget_path(ctx, name);
    (!io.list_dir(&path).is_empty()).then_some(path)
}

/// The theme description of widget `name`.
pub fn widget_meta(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<ThemeMeta> {
    meta(io, &widget_dir(io, ctx, name)?)
}

/// `preview.png` in a theme folder with a description: the author's picture,
/// which takes precedence over other snapshots next to it.
pub fn preview(io: &dyn LookIo, dir: &str) -> Option<String> {
    let dir = dir.trim_end_matches('/');
    let names = io.list_dir(dir);
    if !names.iter().any(|n| n == META_FILE) {
        return None;
    }
    names.iter().any(|n| n == PREVIEW_FILE).then(|| format!("{dir}/{PREVIEW_FILE}"))
}

/// The widget that the settings file `conf` belongs to: our own (under the
/// Rexon directory) or one found on disk. `None` means the file belongs to
/// nobody.
pub fn widget_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    crate::de_apply::conky_widget_of_conf(io, ctx, conf)
        .or_else(|| crate::conky_scan::owner_of_conf(ctx, &crate::conky_scan::extra_dirs_for(ctx), conf))
}

// ════════════════════════════════════════════════════════════════════════════
//  requires[]: into the missing programs check
// ════════════════════════════════════════════════════════════════════════════

/// A feature of conky itself rather than a separate program: `lua-cairo`
/// (what the editor writes by default), `cairo`, `lua`, `imlib2`, `conky`,
/// `conky-*` builds. It comes with the conky package, there is nothing to look
/// for in the PATH.
fn conky_feature(token: &str) -> bool {
    let t = token.to_ascii_lowercase();
    t == "conky"
        || t.starts_with("conky-")
        || t.starts_with("conky_")
        || t.starts_with("lua")
        || t.contains("cairo")
        || t.contains("imlib")
        || t.contains("font")
        || matches!(t.as_str(), "x11" | "xorg" | "wayland" | "xwayland")
}

/// Whether it looks like a program or package name: Latin letters, digits, `._+-`.
fn plain_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= 64
        && token.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
        && !token.starts_with(['-', '.'])
}

/// The programs from `requires` to look for in the PATH. Conky features and
/// entries that are not names ("Nerd Font", "internet") are skipped. A
/// package name (`lm_sensors`) is mapped to a program name via the
/// `utils::TOOL_PACKAGES` table (`sensors`), so that the check looks for what
/// actually lies in the PATH.
pub fn required_programs(meta: &ThemeMeta) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in &meta.requires {
        let token = raw.trim();
        if !plain_token(token) || conky_feature(token) {
            continue;
        }
        let table = crate::utils::TOOL_PACKAGES;
        let program = if table.iter().any(|(bin, _)| *bin == token) {
            token.to_string()
        } else if let Some((bin, _)) = table.iter().find(|(_, pkgs)| {
            pkgs.iter().any(|(_, p)| p.strip_suffix(crate::utils::AUR_ONLY_MARK).unwrap_or(p) == token)
        }) {
            bin.to_string()
        } else {
            token.to_string()
        };
        if !out.contains(&program) {
            out.push(program);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  monitor: the initial screen
// ════════════════════════════════════════════════════════════════════════════

/// The screen number (`xinerama_head`) for the `monitor` field given the
/// screen list `monitors`. `None` means there is nothing to choose: a single
/// screen, `auto`, or no output with that name (the monitor was unplugged).
pub fn monitor_head(meta: &ThemeMeta, monitors: &[crate::conky_overlay::Monitor]) -> Option<u32> {
    let want = meta.monitor.as_deref()?.trim();
    if monitors.len() < 2 || want.is_empty() || want.eq_ignore_ascii_case("auto") {
        return None;
    }
    if want.eq_ignore_ascii_case("primary") {
        return monitors.iter().find(|m| m.primary).map(|m| m.index);
    }
    monitors.iter().find(|m| m.name == want).map(|m| m.index)
}

/// Before starting file `conf`: if the person has not chosen the widget's
/// screen yet (the file has no Rexon settings of its own) and the theme names
/// a screen, record it as the initial choice. From then on it is an ordinary
/// "Screen" setting: the person changes it in the card, and "As the author
/// made it" removes it along with the others (and on the next start the
/// theme's screen is set again — that is the author's choice). On Wayland
/// conky does not choose the screen, so nothing is done.
pub fn seed_monitor(io: &dyn LookIo, ctx: &Ctx, conf: &str) {
    if ctx.wayland {
        return;
    }
    let p = crate::conky_overlay::live_paths(ctx, conf);
    if io.read(&p.settings).is_some() {
        return;
    }
    let Some(name) = widget_of_conf(io, ctx, conf) else { return };
    let Some(meta) = widget_meta(io, ctx, &name) else { return };
    if meta.monitor.is_none() {
        return;
    }
    let Some(head) = monitor_head(&meta, &crate::conky_overlay::monitors(io, ctx.wayland)) else { return };
    let ov = crate::conky_overlay::Overrides { xinerama_head: Some(head), ..Default::default() };
    let env = crate::conky_overlay::Env { version: crate::conky_overlay::conky_version(io), de: ctx.de.clone() };
    if let Err(why) = crate::conky_overlay::save(io, &p, conf, &ov, &env) {
        crate::log_warn!("conky", "экран темы для «{conf}» не записан: {why}");
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  The card
// ════════════════════════════════════════════════════════════════════════════

/// Supplements the widget tile with the theme description: the display name
/// (if the tile is called just by the folder name), author, version and
/// description — where the tile has none of its own. The folder name stays
/// for search (`source_name`).
pub fn decorate(meta: &ThemeMeta, folder_title: &str, item: &mut crate::models::ThemeItem) {
    if let Some(name) = &meta.name {
        if (item.name.is_empty() || item.name == folder_title) && name != folder_title {
            if item.source_name.is_empty() {
                item.source_name = folder_title.to_string();
            }
            item.name = name.clone();
        }
    }
    if item.author.is_empty() {
        if let Some(a) = &meta.author {
            item.author = a.clone();
        }
    }
    if item.version.is_empty() {
        if let Some(v) = &meta.version {
            item.version = v.clone();
        }
    }
    if item.description.is_empty() {
        if let Some(d) = &meta.description {
            item.description = crate::translator::html_escape(d).replace('\n', "<br>");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  start.sh: starting a theme with its script
// ════════════════════════════════════════════════════════════════════════════
//
// In themes of this kind the data (sensors, weather, the player's cover art)
// is collected by background loops from `start.sh` that write cache files,
// and the theme's Lua script reads them. Started as `conky -c conky.conf`,
// such a theme is drawn with empty fields. So a theme with `start.sh` is
// started by it: the working directory is the theme folder, with its own
// process group and the [`MARKER_ENV`] tag in the environment (all theme
// processes inherit it, also after a `setsid` inside the script). Stopping
// works by bookkeeping — the group from the start file and the processes
// tagged with this theme — not by program name. The script is code: before
// starting, it and `scripts/*.sh` are shown by the command check
// (`conky_safety`), and without "Yes" it is not started.

/// The launch script at the root of the theme folder.
pub const START_SCRIPT: &str = "start.sh";

/// The folder of the theme's helper scripts.
pub const SCRIPTS_DIR: &str = "scripts";

/// The tag in the environment of the processes of a theme started by its
/// script: the widget folder path. "Stop" uses it to find the script's loops,
/// and "Running" to find all conky windows of the theme.
pub const MARKER_ENV: &str = "REXON_CONKY_WIDGET";

/// Anything larger is not a launch script.
const SCRIPT_MAX_BYTES: usize = 256 * 1024;

/// The launch script of widget `name`: `<folder>/start.sh`, not empty. A
/// single-file widget has no script.
pub fn start_script(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    let dir = widget_dir(io, ctx, name)?;
    let path = format!("{dir}/{START_SCRIPT}");
    io.read(&path).filter(|t| !t.trim().is_empty() && t.len() <= SCRIPT_MAX_BYTES)?;
    Some(path)
}

/// The shell scripts executed by starting the theme from folder `dir`:
/// `start.sh` first, then `scripts/*.sh` by name. Empty means the theme has
/// no `start.sh`.
pub fn shell_scripts(io: &dyn LookIo, dir: &str) -> Vec<String> {
    let dir = dir.trim_end_matches('/');
    let start = format!("{dir}/{START_SCRIPT}");
    if io.read(&start).is_none_or(|t| t.trim().is_empty()) {
        return Vec::new();
    }
    let mut out = vec![start];
    let sub = format!("{dir}/{SCRIPTS_DIR}");
    let mut names = io.list_dir(&sub);
    names.sort();
    for n in names {
        let lower = n.to_ascii_lowercase();
        if lower.ends_with(".sh") || lower.ends_with(".bash") {
            out.push(format!("{sub}/{n}"));
        }
    }
    out
}

/// The shells a script may be executed with.
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "mksh"];

/// What to execute the script with: the shell from the `#!` line
/// (`#!/bin/bash`, `#!/usr/bin/env bash`), otherwise `sh`. The script is run
/// through it rather than by itself — archive extractors lose the execute bit.
pub fn interpreter(text: &str) -> String {
    let Some(line) = text.lines().next().and_then(|l| l.strip_prefix("#!")) else {
        return "sh".to_string();
    };
    let mut words = line.split_whitespace();
    let mut prog = words.next().unwrap_or("").rsplit('/').next().unwrap_or("");
    if prog == "env" {
        prog = words.find(|w| !w.starts_with('-')).unwrap_or("");
    }
    if SHELLS.contains(&prog) { prog.to_string() } else { "sh".to_string() }
}

/// How to start a theme with its script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptLaunch {
    /// The widget folder: the working directory and the value of the [`MARKER_ENV`] tag.
    pub dir: String,
    pub script: String,
    /// `[shell, script]`.
    pub argv: Vec<String>,
}

/// The plan for starting widget `name` with its script; `None` means the
/// widget has no `start.sh` and goes the old way, `conky -c`.
pub fn script_launch(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<ScriptLaunch> {
    let script = start_script(io, ctx, name)?;
    let text = io.read(&script)?;
    let dir = widget_dir(io, ctx, name)?;
    Some(ScriptLaunch { argv: vec![interpreter(&text), script.clone()], dir, script })
}

/// The widget with a script that the settings file `conf` belongs to.
pub fn script_widget_of_conf(io: &dyn LookIo, ctx: &Ctx, conf: &str) -> Option<String> {
    widget_of_conf(io, ctx, conf).filter(|n| start_script(io, ctx, n).is_some())
}

/// The theme tag in a process environment (`/proc/<pid>/environ`: lines
/// `KEY=value` separated by NUL).
pub fn marker_of_environ(environ: &[u8]) -> Option<String> {
    let key = format!("{MARKER_ENV}=");
    environ
        .split(|b| *b == 0)
        .find_map(|e| e.strip_prefix(key.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .filter(|v| !v.is_empty())
}

/// The tag of process `pid`; `None` means there is no such process or it has no tag.
pub fn marker_of_pid(io: &dyn LookIo, pid: &str) -> Option<String> {
    marker_of_environ(&io.read_bytes(&format!("/proc/{pid}/environ"))?)
}

/// All processes tagged with theme `widget` (the folder path), in ascending PID order.
pub fn marked_pids(io: &dyn LookIo, widget: &str) -> Vec<String> {
    let widget = widget.trim_end_matches('/');
    let mut pids: Vec<u64> = io.list_dir("/proc").iter().filter_map(|p| p.parse().ok()).collect();
    pids.sort_unstable();
    pids.into_iter()
        .map(|p| p.to_string())
        .filter(|p| marker_of_pid(io, p).is_some_and(|m| m.trim_end_matches('/') == widget))
        .collect()
}

/// The directory of theme start files: `$XDG_RUNTIME_DIR/rexon/conky-start`.
fn run_dir() -> Option<String> {
    let rt = std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty())?;
    Some(format!("{}/rexon/conky-start", rt.trim_end_matches('/')))
}

/// The theme start file (the process group number): named by the widget's
/// autostart key, which contains no slashes.
fn group_file(widget: &str) -> Option<String> {
    let key = crate::conky_scan::autostart_key(widget);
    Some(format!("{}/{key}.pgid", run_dir()?))
}

/// The theme's group number from the start file, only if its leader is alive
/// and carries this theme's tag (the number may have gone to another program).
pub fn recorded_group(io: &dyn LookIo, widget: &str) -> Option<String> {
    let text = io.read(&group_file(widget)?)?;
    let pgid = text.trim().to_string();
    if pgid.is_empty() || !pgid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    marker_of_pid(io, &pgid).is_some_and(|m| m.trim_end_matches('/') == widget.trim_end_matches('/')).then_some(pgid)
}

/// What to stop together with the processes `pids`: for a process tagged with
/// a theme, all processes of that theme (the script's loops, the other conky
/// windows) and its group (`-<number>` for `kill`). Without tags, `pids` as is.
pub fn with_companions(io: &dyn LookIo, pids: Vec<String>) -> Vec<String> {
    let mut widgets: Vec<String> = Vec::new();
    for p in &pids {
        if let Some(m) = marker_of_pid(io, p) {
            if !widgets.contains(&m) {
                widgets.push(m);
            }
        }
    }
    let mut out = pids;
    for w in &widgets {
        for p in marked_pids(io, w) {
            if !out.contains(&p) {
                out.push(p);
            }
        }
        if let Some(g) = recorded_group(io, w) {
            let g = format!("-{g}");
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out
}

/// Everything to stop for theme `widget` (the folder path): processes with
/// its tag and its group. Empty means the theme has not been started by its
/// script.
pub fn widget_companions(io: &dyn LookIo, widget: &str) -> Vec<String> {
    let mut out = marked_pids(io, widget);
    if let Some(g) = recorded_group(io, widget) {
        out.push(format!("-{g}"));
    }
    out
}

/// The PIDs of widget `name`'s conky processes right now: by the theme tag or
/// by a settings file in its folder.
pub fn widget_conky_pids(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Vec<String> {
    let path = crate::conky_scan::widget_path(ctx, name);
    crate::conky_procs::all(io)
        .into_iter()
        .filter(|p| {
            p.widget.as_deref().is_some_and(|w| w.trim_end_matches('/') == path)
                || p.conf.as_deref().is_some_and(|c| crate::conky_scan::widget_owns(ctx, name, c))
        })
        .map(|p| p.pid)
        .collect()
}

/// How long the theme must last with a live conky to count as started (as in
/// `conky_procs`: the Wayland output does not crash immediately).
const SURVIVE_MS: u64 = 3000;

/// How long to wait for the first conky from the script: before it, the
/// script starts its pollers.
const APPEAR_MS: u64 = 6000;

/// Starts widget `name` with its `start.sh`. A previous start of the theme is
/// stopped first (one theme per desktop). Returns `(succeeded, message)`.
pub fn start_widget_script(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let title = crate::conky_scan::widget_title(name);
    if !io.have("conky") {
        return (false, crate::de_apply::conky_missing_refusal());
    }
    let Some(plan) = script_launch(io, ctx, name) else {
        return (false, crate::i18n::trf("Couldn't start “%1”: its start script is gone.", &[&title]));
    };
    // The theme is already running (via the button, at login, by hand): stop it entirely.
    let mut old = widget_conky_pids(io, ctx, name);
    old.extend(widget_companions(io, &plan.dir));
    if !old.is_empty() {
        stop_and_wait(io, with_companions(io, old));
    }
    let main_conf = main_conf(io, ctx, name);
    let log = crate::conky_log::live_log_path(ctx, main_conf.as_deref().unwrap_or(&plan.script));
    let needs_x11 = ctx.wayland && main_conf.as_deref().is_some_and(crate::conky_overlay::widget_needs_x11);
    let mut xwayland = crate::conky_overlay::live_via_xwayland(io, ctx, needs_x11);
    let mut result = run_script(io, ctx, name, &plan, &log, xwayland);
    if result.is_err() && !xwayland && ctx.wayland && crate::conky_overlay::live_display() {
        crate::log_warn!("conky", "«{title}»: вывод Wayland не поднялся, второй запуск сценария через XWayland");
        xwayland = true;
        result = run_script(io, ctx, name, &plan, &log, true);
    }
    if let Err(why) = result {
        return (false, why);
    }
    let mut msg = crate::i18n::trf("▶️ Started: %1", &[&title]);
    if !xwayland {
        let version = if ctx.wayland { crate::conky_overlay::conky_version(io) } else { None };
        if let Some(note) = crate::de_apply::conky_wayland_note_for(&ctx.de, ctx.wayland, version) {
            msg.push_str(" — ");
            msg.push_str(&note);
        }
    }
    (true, msg)
}

/// `kill -- <targets>` (PIDs and `-groups`). `kill` returns an error if even
/// one PID has already exited by itself (a short `sleep` in the script's
/// loop) — that is not a failure: `(true, _)` if none of the named PIDs are
/// left. Returns `(succeeded, kill output)`.
pub fn kill_targets(io: &dyn LookIo, targets: &[String]) -> (bool, String) {
    let mut cmd = vec!["kill".to_string(), "--".to_string()];
    cmd.extend(targets.iter().cloned());
    let (ok, out) = io.run(&cmd);
    if ok {
        return (true, out);
    }
    let alive = targets
        .iter()
        .filter(|t| !t.starts_with('-'))
        .any(|p| io.read(&format!("/proc/{p}/cmdline")).is_some_and(|c| !c.is_empty()));
    (!alive, out)
}

/// Stops `targets` (PIDs and `-groups`) and waits for them to exit, otherwise
/// a new start would take the old conky for its own.
fn stop_and_wait(io: &dyn LookIo, targets: Vec<String>) {
    let _ = kill_targets(io, &targets);
    let pids: Vec<&String> = targets.iter().filter(|t| !t.starts_with('-')).collect();
    for _ in 0..30 {
        if pids.iter().all(|p| io.read(&format!("/proc/{p}/cmdline")).is_none_or(|c| c.is_empty())) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// A single script run: `Ok` means the theme's conky appeared and lasted
/// [`SURVIVE_MS`]; otherwise everything the script managed to bring up is
/// stopped, and the cause comes from the log.
fn run_script(io: &dyn LookIo, ctx: &Ctx, name: &str, plan: &ScriptLaunch, log: &str, xwayland: bool) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let title = crate::conky_scan::widget_title(name);
    let out = crate::conky_log::open_for_start(log);
    let err = out.as_ref().and_then(|f| f.try_clone().ok());
    let stdio = |f: Option<std::fs::File>| f.map(std::process::Stdio::from).unwrap_or_else(std::process::Stdio::null);
    let mut cmd = std::process::Command::new(&plan.argv[0]);
    cmd.args(&plan.argv[1..])
        .current_dir(&plan.dir)
        .env(MARKER_ENV, &plan.dir)
        .stdin(std::process::Stdio::null())
        .stdout(stdio(out))
        .stderr(stdio(err))
        .process_group(0);
    if xwayland {
        crate::conky_overlay::apply_xwayland(&mut cmd);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| crate::i18n::trf("Couldn't start “%1”: %2", &[&title, &e.to_string()]))?;
    let leader = child.id().to_string();
    if let Some(file) = group_file(&plan.dir) {
        io.write(&file, &format!("{leader}\n"));
    }
    // The script may exit immediately (`exec setsid "$0"`) or become conky — its
    // end is awaited in a separate thread so that no zombie is left behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let step = std::time::Duration::from_millis(100);
    let mut seen_ms: u64 = 0;
    let mut waited: u64 = 0;
    let ok = loop {
        std::thread::sleep(step);
        waited += 100;
        if widget_conky_pids(io, ctx, name).is_empty() {
            // Appeared and vanished: crashed at startup.
            if seen_ms > 0 || waited >= APPEAR_MS {
                break false;
            }
        } else {
            seen_ms += 100;
            if seen_ms >= SURVIVE_MS {
                break true;
            }
        }
    };
    if ok {
        return Ok(());
    }
    // It did not come up: do not leave the script's loops on the desktop.
    let mut rest = widget_companions(io, &plan.dir);
    rest.push(format!("-{leader}"));
    let _ = kill_targets(io, &rest);
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let why = match crate::conky_log::failure_reason(&text) {
        Some(reason) => crate::i18n::trf("conky closed right after starting: %1", &[&reason]),
        None => crate::i18n::tr("the widget's start script didn't bring up Conky — see what it reported in the card"),
    };
    Err(crate::i18n::trf("Couldn't start “%1”: %2", &[&title, &why]))
}

/// The autostart line for a theme with a script: `sh -c` with a body and
/// arguments (`$0` is the script, `$1` the tag folder, `$2` the shell). Paths
/// are not substituted into the body.
pub const AUTOSTART_SCRIPT: &str = r#"cd "$(dirname "$0")" && export REXON_CONKY_WIDGET="$1" && exec "$2" "$0""#;

/// The launch line for a compositor config (there the compositor expands `$`
/// itself, so the paths are written in quotes; a path with `"`, `$`, `` ` ``
/// or `\` gives `None`).
pub fn compositor_argv(plan: &ScriptLaunch, delay: u32) -> Option<Vec<String>> {
    let bad = |s: &str| s.contains(['"', '$', '`', '\\', '\n']);
    if bad(&plan.dir) || bad(&plan.script) {
        return None;
    }
    let sleep = if delay > 0 { format!("sleep {delay} && ") } else { String::new() };
    Some(vec![
        "sh".into(),
        "-c".into(),
        format!(
            "{sleep}cd \"{dir}\" && export {MARKER_ENV}=\"{dir}\" && exec {sh} \"{script}\"",
            dir = plan.dir,
            sh = plan.argv[0],
            script = plan.script
        ),
    ])
}

// ════════════════════════════════════════════════════════════════════════════
//  A multi-window theme is one widget
// ════════════════════════════════════════════════════════════════════════════
//
// A theme's launch script sometimes brings up several conky instances — one
// window per screen or per part of the drawing (`conky_window_*.conf`,
// copies with `xinerama_head`). For the person it is ONE widget: it is a
// single entry in "Running", in the strip, in sets and in the "Stop all"
// question, and stopping it stops the theme's whole process group
// ([`with_companions`]).

/// The widget name (as on the tile) from theme folder `dir`, the value of the
/// [`MARKER_ENV`] tag: under the Rexon directory, the path relative to it;
/// otherwise the one found on disk (the path); otherwise the folder itself.
pub fn widget_of_dir(ctx: &Ctx, extra: &[String], dir: &str) -> String {
    let dir = dir.trim_end_matches('/');
    let root = format!("{}/", crate::de_apply::conky_root(ctx).trim_end_matches('/'));
    if let Some(rest) = dir.strip_prefix(&root).filter(|r| !r.is_empty()) {
        return rest.to_string();
    }
    crate::conky_scan::owner_of_conf(ctx, extra, &format!("{dir}/{START_SCRIPT}"))
        .filter(|o| o.trim_end_matches('/') == dir)
        .unwrap_or_else(|| dir.to_string())
}

/// The main settings file of widget `name`: a theme with a script is listed
/// under it in sets and in the list of running widgets.
pub fn main_conf(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    crate::conky_scan::widget_confs(io, ctx, name, crate::de_apply::conky_conf(io, ctx, name)).into_iter().next()
}

/// Which file a running conky with file `conf` and tag `marker` is listed
/// under: a window of a theme with a script under the theme's main file (all
/// theme windows are one line), others under their own file.
pub fn unit_conf(io: &dyn LookIo, ctx: &Ctx, extra: &[String], conf: &str, marker: Option<&str>) -> String {
    let name = match marker {
        Some(dir) => Some(widget_of_dir(ctx, extra, dir)),
        None => script_widget_of_conf(io, ctx, conf),
    };
    name.and_then(|n| main_conf(io, ctx, &n)).unwrap_or_else(|| conf.to_string())
}

/// Widget files `confs` (from a set) without repeats of the same theme: the
/// windows of a theme with a script collapse into its main file — otherwise a
/// set would bring the theme up as many times as it has windows.
pub fn unit_confs(io: &dyn LookIo, ctx: &Ctx, extra: &[String], confs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in confs {
        let u = unit_conf(io, ctx, extra, c, None);
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

// ════════════════════════════════════════════════════════════════════════════
//  Conky Studio: the editor the theme was made with
// ════════════════════════════════════════════════════════════════════════════
//
// Rexon does not write an editor itself: for a theme made in Conky Studio
// (the `created_with` mark in `theme.json` or a `.cstudio` project file) the
// card offers to open it there — only if it exists on this machine. Conky
// Studio is installed as a single AppImage file, so it is looked for both
// in the PATH and in the usual places where such files are put.

/// The Conky Studio project file extension.
pub const STUDIO_PROJECT_EXT: &str = "cstudio";

/// Program names in the PATH (wrappers, links to the AppImage).
const STUDIO_PROGRAMS: &[&str] = &["conky-studio", "conkystudio", "ConkyStudio", "conky_studio"];

/// The `created_with` mark is Conky Studio (`conky-studio`, `Conky Studio`,
/// `conkystudio 1.1`).
fn studio_mark(created_with: &str) -> bool {
    let s: String = created_with.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    s.starts_with("conkystudio")
}

/// The Conky Studio project file of the theme in folder `dir`: inside the
/// folder (the first by name) or next to it (`<folder>.cstudio`).
pub fn studio_project(io: &dyn LookIo, dir: &str) -> Option<String> {
    let dir = dir.trim_end_matches('/');
    let ext = format!(".{STUDIO_PROJECT_EXT}");
    let mut names = io.list_dir(dir);
    names.sort();
    if let Some(n) = names.iter().find(|n| n.to_ascii_lowercase().ends_with(&ext) && n.len() > ext.len()) {
        return Some(format!("{dir}/{n}"));
    }
    let (parent, base) = dir.rsplit_once('/')?;
    io.list_dir(if parent.is_empty() { "/" } else { parent })
        .into_iter()
        .find(|n| n.strip_suffix(&ext).is_some_and(|stem| stem == base))
        .map(|n| format!("{parent}/{n}"))
}

/// The theme in folder `dir` was made in Conky Studio.
pub fn is_studio_theme(io: &dyn LookIo, dir: &str) -> bool {
    meta(io, dir).and_then(|m| m.created_with).is_some_and(|c| studio_mark(&c)) || studio_project(io, dir).is_some()
}

/// Whether the file can be executed: the execute bit. If the file is not on a
/// real disk (a fake in tests), it can.
fn executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(true)
}

/// Whether a file name looks like a Conky Studio AppImage
/// (`ConkyStudio-1.1.1-x86_64.AppImage`, `conky-studio.appimage`).
fn studio_appimage_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let flat: String = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    flat.starts_with("conkystudio") && lower.ends_with(".appimage")
}

/// The "Downloads" directory from `user-dirs.dirs` (on a Russian desktop,
/// `~/Загрузки`); `None` means it is not set.
fn download_dir(io: &dyn LookIo, ctx: &Ctx) -> Option<String> {
    let home = ctx.home.trim_end_matches('/');
    let cfg = ctx.xdg_config_home.clone().unwrap_or_else(|| format!("{home}/.config"));
    let text = io.read(&format!("{}/user-dirs.dirs", cfg.trim_end_matches('/')))?;
    text.lines().find_map(|l| {
        let v = l.trim().strip_prefix("XDG_DOWNLOAD_DIR=")?.trim().trim_matches('"').replace("$HOME", home);
        (!v.is_empty() && v.trim_end_matches('/') != home).then(|| v.trim_end_matches('/').to_string())
    })
}

/// Where the Conky Studio AppImage is looked for.
fn studio_dirs(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let home = ctx.home.trim_end_matches('/');
    let mut dirs: Vec<String> = ["Applications", "applications", "AppImages", "Appimages", ".local/bin", "bin", "Downloads"]
        .iter()
        .map(|d| format!("{home}/{d}"))
        .collect();
    if let Some(d) = download_dir(io, ctx) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs.push("/opt".into());
    dirs.push("/opt/conky-studio".into());
    dirs
}

/// How to start Conky Studio on this machine: a program from the PATH or the
/// path to the AppImage (the last one by name, usually the newest release).
/// `None` means Conky Studio was not found.
pub fn find_studio(io: &dyn LookIo, ctx: &Ctx) -> Option<String> {
    if let Some(p) = STUDIO_PROGRAMS.iter().find(|p| io.have(p)) {
        return Some(p.to_string());
    }
    for dir in studio_dirs(io, ctx) {
        let mut names: Vec<String> = io.list_dir(&dir).into_iter().filter(|n| studio_appimage_name(n)).collect();
        names.sort();
        if let Some(n) = names.into_iter().rev().find(|n| executable(&format!("{dir}/{n}"))) {
            return Some(format!("{dir}/{n}"));
        }
    }
    None
}

/// What to open widget `name` with in Conky Studio: the launch line (with the
/// project file, if there is one). `None` means the theme is not from Conky
/// Studio or Conky Studio is not on the machine: there is no button in the card.
pub fn studio_edit_argv(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<Vec<String>> {
    let dir = widget_dir(io, ctx, name)?;
    if !is_studio_theme(io, &dir) {
        return None;
    }
    let mut argv = vec![find_studio(io, ctx)?];
    if let Some(project) = studio_project(io, &dir) {
        argv.push(project);
    }
    Some(argv)
}

/// "Edit in Conky Studio": starts the editor. Returns `(succeeded, message)`.
pub fn edit_in_studio(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let title = crate::conky_scan::widget_title(name);
    let Some(argv) = studio_edit_argv(io, ctx, name) else {
        return (false, crate::i18n::tr("Conky Studio isn't found on this computer."));
    };
    if !io.spawn(&argv) {
        return (false, crate::i18n::trf("Couldn't open Conky Studio: %1", &[&argv[0]]));
    }
    crate::log_info!("conky", "«{title}»: открыт в Conky Studio ({})", argv[0]);
    (true, crate::i18n::trf("✏️ Opening “%1” in Conky Studio. Restart the widget to see your changes.", &[&title]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_reads_known_fields() {
        let m = parse_meta(
            r#"{
              "name": "Night Ops", "author": "you", "version": "1.0.0",
              "description": "Compact strip", "resolution": "1920x1080",
              "requires": ["lua-cairo", "curl", "jq"], "tags": ["dark", "minimal"],
              "created_with": "conky-studio", "store_id": "abc", "sha256": "00ff",
              "monitor": "DP-1"
            }"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("Night Ops"));
        assert_eq!(m.author.as_deref(), Some("you"));
        assert_eq!(m.version.as_deref(), Some("1.0.0"));
        assert_eq!(m.description.as_deref(), Some("Compact strip"));
        assert_eq!(m.resolution.as_deref(), Some("1920x1080"));
        assert_eq!(m.requires, ["lua-cairo", "curl", "jq"]);
        assert_eq!(m.tags, ["dark", "minimal"]);
        assert_eq!(m.created_with.as_deref(), Some("conky-studio"));
        assert_eq!(m.store_id.as_deref(), Some("abc"));
        assert_eq!(m.sha256.as_deref(), Some("00ff"));
        assert_eq!(m.monitor.as_deref(), Some("DP-1"));
    }

    /// Unknown fields and fields of the wrong type are silently skipped; a broken
    /// file and a non-object are treated as if there were no description.
    #[test]
    fn meta_skips_unknown_and_broken() {
        let m = parse_meta(
            r#"{"name": 5, "author": {"x": 1}, "version": 1.5, "description": "  ",
                "requires": ["curl", 3, null, "", "curl"], "tags": "solo",
                "monitor": ["DP-1"], "future_field": true}"#,
        )
        .unwrap();
        assert_eq!(m.name.as_deref(), Some("5"));
        assert_eq!(m.author, None);
        assert_eq!(m.version.as_deref(), Some("1.5"));
        assert_eq!(m.description, None);
        assert_eq!(m.requires, ["curl"]);
        assert_eq!(m.tags, ["solo"]);
        assert_eq!(m.monitor, None);
        assert_eq!(parse_meta("{ not json"), None);
        assert_eq!(parse_meta("[1, 2]"), None);
        assert_eq!(parse_meta("\u{feff}{}"), Some(ThemeMeta::default()));
    }

    #[test]
    fn requires_become_programs() {
        let m = ThemeMeta {
            requires: ["lua-cairo", "conky", "curl", "lm_sensors", "Nerd Font", "jq", "cairo", "playerctl", "curl"]
                .map(String::from)
                .to_vec(),
            ..Default::default()
        };
        assert_eq!(required_programs(&m), ["curl", "sensors", "jq", "playerctl"]);
    }

    fn mon(index: u32, name: &str, primary: bool) -> crate::conky_overlay::Monitor {
        crate::conky_overlay::Monitor { index, name: name.into(), width: 1920, height: 1080, x: 0, y: 0, primary }
    }

    #[test]
    fn monitor_field_picks_head() {
        let two = [mon(0, "eDP-1", false), mon(1, "DP-1", true)];
        let with = |m: &str| ThemeMeta { monitor: Some(m.into()), ..Default::default() };
        assert_eq!(monitor_head(&with("DP-1"), &two), Some(1));
        assert_eq!(monitor_head(&with("primary"), &two), Some(1));
        assert_eq!(monitor_head(&with("auto"), &two), None);
        assert_eq!(monitor_head(&with("HDMI-9"), &two), None, "выхода нет — экран не трогаем");
        assert_eq!(monitor_head(&with("DP-1"), &two[..1]), None, "экран один — выбирать нечего");
        assert_eq!(monitor_head(&ThemeMeta::default(), &two), None);
    }

    #[test]
    fn card_takes_name_author_description() {
        let m = parse_meta(r#"{"name": "Night Ops", "author": "you", "version": "2", "description": "a <b>\nb"}"#).unwrap();
        let mut it = crate::models::ThemeItem { name: "night-ops".into(), ..Default::default() };
        decorate(&m, "night-ops", &mut it);
        assert_eq!(it.name, "Night Ops");
        assert_eq!(it.source_name, "night-ops");
        assert_eq!(it.author, "you");
        assert_eq!(it.version, "2");
        assert_eq!(it.description, "a &lt;b&gt;<br>b");
        // Our own name (from the store) and our own description are not overridden.
        let mut own = crate::models::ThemeItem { name: "Store title".into(), description: "x".into(), ..Default::default() };
        decorate(&m, "night-ops", &mut own);
        assert_eq!(own.name, "Store title");
        assert_eq!(own.description, "x");
    }

    /// The theme's `requires` is part of the common missing programs check.
    #[test]
    fn requires_join_health_check() {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("T")).unwrap();
        std::fs::write(d.join("T/conky.conf"), "conky.config = {}\nconky.text = [[x]]\n").unwrap();
        std::fs::write(d.join("T/theme.json"), r#"{"requires": ["lua-cairo", "rexon-no-such-program-42"]}"#).unwrap();
        let ctx = Ctx { home: d.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() };
        let name = d.join("T").to_string_lossy().into_owned();
        let h = crate::conky_safety::health(&crate::animations::SystemIo, &ctx, &name, "arch");
        let missing: Vec<&str> = h.missing_programs.iter().map(|m| m.program.as_str()).collect();
        assert_eq!(missing, ["rexon-no-such-program-42"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(root: &std::path::Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn ctx_at(home: &std::path::Path) -> Ctx {
        Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() }
    }

    #[test]
    fn interpreter_from_shebang() {
        assert_eq!(interpreter("#!/bin/bash\necho"), "bash");
        assert_eq!(interpreter("#!/usr/bin/env bash\n"), "bash");
        assert_eq!(interpreter("#!/usr/bin/env -S zsh -f\n"), "zsh");
        assert_eq!(interpreter("#!/usr/bin/python3\n"), "sh", "не оболочка — sh");
        assert_eq!(interpreter("echo hi\n"), "sh");
        // The tag in the autostart script is the same one the stop looks for.
        assert!(AUTOSTART_SCRIPT.contains(&format!("export {MARKER_ENV}=")));
    }

    /// The launch plan: a theme with `start.sh` is started by it, from the theme
    /// folder, with the shell from `#!`; without it (or a single-file widget),
    /// the old way.
    #[test]
    fn launch_plan_goes_through_start_script() {
        let d = tmp("plan");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "T/start.sh", "#!/bin/bash\nexec conky -c conky.conf\n");
        put(&d, "Plain/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "Empty/conky.conf", "conky.text = [[x]]\n");
        put(&d, "Empty/start.sh", "  \n");
        let io = crate::animations::SystemIo;
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        let plan = script_launch(&io, &ctx, &s("T")).unwrap();
        assert_eq!(plan.dir, s("T"));
        assert_eq!(plan.argv, ["bash".to_string(), s("T/start.sh")]);
        assert_eq!(script_launch(&io, &ctx, &s("Plain")), None);
        assert_eq!(script_launch(&io, &ctx, &s("Empty")), None, "пустой start.sh — не сценарий");
        assert_eq!(script_launch(&io, &ctx, &s("T/conky.conf")), None, "виджет-файл — без сценария");
        let a = compositor_argv(&plan, 5).unwrap();
        assert_eq!(a[2], format!("sleep 5 && cd \"{0}\" && export {MARKER_ENV}=\"{0}\" && exec bash \"{0}/start.sh\"", s("T")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The dangerous command scanner sees `start.sh` and `scripts/*.sh`: without
    /// permission the theme is not started; the permission is remembered as long
    /// as the files do not change. The script's own functions do not count as
    /// missing programs.
    #[test]
    fn safety_scanner_sees_start_script() {
        crate::i18n::set_language(Some("en"));
        let d = tmp("scan");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[${time}]]\n");
        put(
            &d,
            "T/start.sh",
            "#!/bin/bash\n# комментарий\nDIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncd \"$DIR\"\n\
             poll_weather() {\n  curl -s https://wttr.in/?format=3 > .runtime-cache/weather.cache\n}\n\
             poll_weather &\nwhile true; do sleep 60; scripts/gpu.sh; done &\nexec conky -c \"$DIR/conky.conf\"\n",
        );
        put(&d, "T/scripts/gpu.sh", "#!/bin/sh\nnvidia-smi --query-gpu=temperature.gpu --format=csv > .runtime-cache/gpu.cache\n");
        let io = crate::animations::SystemIo;
        let ctx = ctx_at(&d);
        let name = d.join("T").to_string_lossy().into_owned();
        let risks = crate::conky_safety::scan_widget(&io, &ctx, &name);
        let what: Vec<&str> = risks.iter().map(|r| r.what.as_str()).collect();
        assert!(what.contains(&"start.sh"), "{what:?}");
        assert!(what.contains(&"scripts/gpu.sh"), "{what:?}");
        assert!(what.iter().any(|w| w.starts_with("curl -s https://wttr.in")), "{what:?}");
        assert!(what.iter().any(|w| w.starts_with("nvidia-smi")), "{what:?}");
        assert!(what.contains(&"while true; do sleep 60; scripts/gpu.sh; done &"), "{what:?}");
        assert!(!what.iter().any(|w| w.starts_with("cd ") || w.starts_with("exec conky")), "обвязка не показывается: {what:?}");
        assert!(risks.iter().any(|r| r.kind == crate::conky_safety::RiskKind::Network));
        let (title, text) = crate::conky_safety::start_question(&io, &ctx, &name).expect("без разрешения — вопрос");
        assert_eq!(title, "Allow the widget to run commands?");
        assert!(text.contains("• start.sh\n"), "{text}");
        assert!(crate::conky_safety::start_refusal(&io, &ctx, &name).is_some());
        // The script's functions and the programs in its lines are not "program missing".
        let h = crate::conky_safety::health(&io, &ctx, &name, "arch");
        assert!(h.missing_programs.is_empty(), "{:?}", h.missing_programs);
        assert!(crate::conky_safety::remember(&io, &ctx, &name));
        assert!(crate::conky_safety::start_question(&io, &ctx, &name).is_none());
        put(&d, "T/scripts/gpu.sh", "#!/bin/sh\nrm -rf ~/x\n");
        let (_, text) = crate::conky_safety::start_question(&io, &ctx, &name).expect("файлы изменились — вопрос снова");
        assert!(text.contains("rm -rf ~/x"), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A fake `/proc`: the environment of processes tagged with a theme.
    struct FakeProcs {
        files: std::collections::HashMap<String, String>,
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeProcs {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            if path != "/proc" {
                return Vec::new();
            }
            let mut v: Vec<String> =
                self.files.keys().filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string())).collect();
            v.sort();
            v.dedup();
            v
        }
    }

    fn fake_proc(f: &mut std::collections::HashMap<String, String>, pid: u32, cmd: &[&str], comm: &str, marker: Option<&str>) {
        f.insert(format!("/proc/{pid}/cmdline"), cmd.join("\0") + "\0");
        f.insert(format!("/proc/{pid}/comm"), format!("{comm}\n"));
        let mut env = "HOME=/h\0LANG=C\0".to_string();
        if let Some(m) = marker {
            env.push_str(&format!("{MARKER_ENV}={m}\0"));
        }
        f.insert(format!("/proc/{pid}/environ"), env);
    }

    /// Stopping a theme with a script: together with its conky the script's loops
    /// are stopped (by tag), other processes are not.
    #[test]
    fn stop_takes_the_whole_theme() {
        let mut f = std::collections::HashMap::new();
        let t = "/h/.config/conky/Night";
        fake_proc(&mut f, 200, &["bash", "/h/.config/conky/Night/start.sh"], "bash", Some(t));
        fake_proc(&mut f, 201, &["sleep", "60"], "sleep", Some(t));
        fake_proc(&mut f, 202, &["conky", "-c", "/h/.config/conky/Night/conky.conf"], "conky", Some(t));
        fake_proc(&mut f, 203, &["conky", "-c", "/h/.config/conky/Night/conky_w2.conf"], "conky", Some(t));
        fake_proc(&mut f, 300, &["conky", "-c", "/h/.config/conky/Other/conky.conf"], "conky", None);
        fake_proc(&mut f, 301, &["sleep", "60"], "sleep", Some("/h/.config/conky/Night2"));
        let io = FakeProcs { files: f, ran: Default::default() };
        assert_eq!(marker_of_environ(b"A=1\0REXON_CONKY_WIDGET=/x/y\0B=2\0").as_deref(), Some("/x/y"));
        assert_eq!(marker_of_environ(b"A=1\0"), None);
        assert_eq!(marked_pids(&io, t), ["200", "201", "202", "203"]);
        assert_eq!(with_companions(&io, vec!["202".into()]), ["202", "200", "201", "203"]);
        assert_eq!(with_companions(&io, vec!["300".into()]), ["300"], "без метки — только сам процесс");
        let ctx = Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() };
        let (ok, _) = crate::conky_procs::stop_pids(&io, "Night", vec!["203".into()]);
        assert!(ok);
        assert_eq!(*io.ran.borrow(), [["kill", "--", "203", "200", "201", "202"].map(String::from).to_vec()]);
        let procs = crate::conky_procs::all(&io);
        assert_eq!(procs.iter().filter(|p| p.widget.as_deref() == Some(t)).count(), 2);
        let _ = ctx;
    }

    /// A live run with a script on a fake "conky" (a copy of `tail` under that
    /// name — the real conky is not started): the tag is in the environment, the
    /// working directory is the theme folder, there is its own group; stopping
    /// stops the whole group.
    #[test]
    fn start_script_runs_and_stops_as_group() {
        struct Io;
        impl LookIo for Io {
            fn run(&self, argv: &[String]) -> (bool, String) {
                crate::animations::SystemIo.run(argv)
            }
            fn have(&self, _: &str) -> bool {
                true
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
                crate::animations::SystemIo.read_bytes(path)
            }
            fn write(&self, path: &str, text: &str) -> bool {
                crate::animations::SystemIo.write(path, text)
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let Some(tail) = crate::utils::which("tail", std::env::var("PATH").ok().as_deref()) else { return };
        let d = tmp("run");
        put(&d, "T/conky.conf", "conky.config = {}\nconky.text = [[x]]\n");
        put(&d, "T/start.sh", "#!/bin/sh\nwhile true; do sleep 1; done &\nexec ./conky -c +1 -f /dev/null\n");
        std::fs::copy(tail, d.join("T/conky")).unwrap();
        let ctx = ctx_at(&d);
        let name = d.join("T").to_string_lossy().into_owned();
        let (ok, msg) = start_widget_script(&Io, &ctx, &name);
        assert!(ok, "{msg}");
        let conky = widget_conky_pids(&Io, &ctx, &name);
        assert_eq!(conky.len(), 1, "{conky:?}");
        let all = marked_pids(&Io, &name);
        assert!(all.len() >= 2, "цикл сценария тоже с меткой: {all:?}");
        let cwd = std::fs::read_link(format!("/proc/{}/cwd", conky[0])).unwrap();
        assert_eq!(cwd, d.join("T"));
        let (ok, _) = crate::conky_procs::stop_pids(&Io, "T", conky);
        assert!(ok);
        for _ in 0..30 {
            if marked_pids(&Io, &name).is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(marked_pids(&Io, &name).is_empty(), "после остановки от темы ничего не осталось");
        if let Some(g) = group_file(&name) {
            let _ = std::fs::remove_file(g);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn preview_png_only_next_to_theme_json() {
        let d = std::env::temp_dir().join(format!("rexon-conky-theme-pv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        for dir in ["a", "b"] {
            std::fs::write(d.join(dir).join("preview.png"), b"p").unwrap();
        }
        std::fs::write(d.join("a/theme.json"), "{}").unwrap();
        let io = crate::animations::SystemIo;
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        assert_eq!(preview(&io, &s("a")), Some(s("a/preview.png")));
        assert_eq!(preview(&io, &s("b")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A theme from Conky Studio: the `created_with` mark or a `.cstudio` file;
    /// the button appears only if Conky Studio is found (an AppImage with the
    /// execute bit in the usual places, or a program in the PATH).
    #[test]
    fn studio_theme_and_editor_lookup() {
        use std::os::unix::fs::PermissionsExt;
        struct NoPath;
        impl LookIo for NoPath {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, _: &str) -> bool {
                false
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        struct InPath;
        impl LookIo for InPath {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, p: &str) -> bool {
                p == "conky-studio"
            }
            fn read(&self, path: &str) -> Option<String> {
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let d = tmp("studio");
        let conf = "conky.config = {}\nconky.text = [[x]]\n";
        put(&d, ".config/conky/Night/conky.conf", conf);
        put(&d, ".config/conky/Night/theme.json", r#"{"name": "Night", "created_with": "Conky Studio"}"#);
        put(&d, ".config/conky/Proj/conky.conf", conf);
        put(&d, ".config/conky/Proj/Proj.cstudio", "{}");
        put(&d, ".config/conky/Side/conky.conf", conf);
        put(&d, ".config/conky/Side.cstudio", "{}");
        put(&d, ".config/conky/Plain/conky.conf", conf);
        put(&d, ".config/conky/Plain/theme.json", r#"{"created_with": "hand"}"#);
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Night")));
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Proj")));
        assert!(is_studio_theme(&NoPath, &s(".config/conky/Side")), ".cstudio рядом с папкой");
        assert!(!is_studio_theme(&NoPath, &s(".config/conky/Plain")));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Night"), None, "Conky Studio не найден — кнопки нет");
        // An AppImage without the execute bit does not qualify; with it, the last one by name.
        put(&d, "Applications/ConkyStudio-1.0.0-x86_64.AppImage", "x");
        assert_eq!(find_studio(&NoPath, &ctx), None);
        put(&d, "Загрузки/ConkyStudio-1.1.0-x86_64.AppImage", "x");
        put(&d, "Загрузки/ConkyStudio-1.1.1-x86_64.AppImage", "x");
        put(&d, "Загрузки/notes.txt", "x");
        put(&d, ".config/user-dirs.dirs", "XDG_DOWNLOAD_DIR=\"$HOME/Загрузки\"\n");
        for f in ["Загрузки/ConkyStudio-1.1.0-x86_64.AppImage", "Загрузки/ConkyStudio-1.1.1-x86_64.AppImage"] {
            std::fs::set_permissions(d.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let app = s("Загрузки/ConkyStudio-1.1.1-x86_64.AppImage");
        assert_eq!(find_studio(&NoPath, &ctx).as_deref(), Some(app.as_str()));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Night"), Some(vec![app.clone()]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Proj"), Some(vec![app.clone(), s(".config/conky/Proj/Proj.cstudio")]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Side"), Some(vec![app, s(".config/conky/Side.cstudio")]));
        assert_eq!(studio_edit_argv(&NoPath, &ctx, "Plain"), None, "тема не из Conky Studio — кнопки нет");
        assert_eq!(studio_edit_argv(&InPath, &ctx, "Night"), Some(vec!["conky-studio".to_string()]), "PATH — первым");
        // The fake starts nothing: an honest refusal.
        assert!(!edit_in_studio(&NoPath, &ctx, "Night").0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A multi-window theme (the script brought up several conky instances, one
    /// window from a copy of the file outside the theme folder): in the strip, in
    /// sets, in the question and in the "Stop all" count it is one widget; the
    /// strip's cross and a set stop all of the theme's windows.
    #[test]
    fn multi_window_theme_is_one_widget() {
        crate::i18n::set_language(Some("en"));
        struct Mixed {
            procs: std::collections::HashMap<String, String>,
            ran: std::cell::RefCell<Vec<Vec<String>>>,
        }
        impl LookIo for Mixed {
            fn run(&self, argv: &[String]) -> (bool, String) {
                self.ran.borrow_mut().push(argv.to_vec());
                (true, String::new())
            }
            fn have(&self, _: &str) -> bool {
                true
            }
            fn read(&self, path: &str) -> Option<String> {
                if path.starts_with("/proc/") {
                    return self.procs.get(path).cloned();
                }
                crate::animations::SystemIo.read(path)
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                if path == "/proc" {
                    let mut v: Vec<String> = self
                        .procs
                        .keys()
                        .filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string()))
                        .collect();
                    v.sort();
                    v.dedup();
                    return v;
                }
                crate::animations::SystemIo.list_dir(path)
            }
        }
        let d = tmp("multi");
        let conf = "conky.config = {}\nconky.text = [[x]]\n";
        put(&d, ".config/conky/Night/conky.conf", conf);
        put(&d, ".config/conky/Night/conky_window_2.conf", conf);
        put(&d, ".config/conky/Night/start.sh", "#!/bin/sh\nconky -c conky.conf &\nexec conky -c conky_window_2.conf\n");
        put(&d, ".config/conky/Other/conky.conf", conf);
        let ctx = ctx_at(&d);
        let s = |p: &str| d.join(p).to_string_lossy().into_owned();
        let night = s(".config/conky/Night");
        let (main, w2, other) = (s(".config/conky/Night/conky.conf"), s(".config/conky/Night/conky_window_2.conf"), s(".config/conky/Other/conky.conf"));
        let mut f = std::collections::HashMap::new();
        fake_proc(&mut f, 200, &["sh", &s(".config/conky/Night/start.sh")], "sh", Some(&night));
        fake_proc(&mut f, 201, &["conky", "-c", &main], "conky", Some(&night));
        fake_proc(&mut f, 202, &["conky", "-c", &w2], "conky", Some(&night));
        fake_proc(&mut f, 203, &["conky", "-c", "/run/user/1000/Night-DP-1.conf"], "conky", Some(&night));
        fake_proc(&mut f, 300, &["conky", "-c", &other], "conky", None);
        let io = Mixed { procs: f, ran: Default::default() };
        assert_eq!(widget_of_dir(&ctx, &[], &night), "Night");
        assert_eq!(crate::conky_procs::running_widget_confs(&io, &ctx), [main.clone(), other.clone()]);
        assert_eq!(unit_confs(&io, &ctx, &[], &[w2.clone(), main.clone(), other.clone()]), [main.clone(), other.clone()]);
        let ribbon = crate::conky_procs::ribbon(&io, &ctx, &[]);
        assert_eq!(ribbon.len(), 2, "{ribbon:?}");
        assert_eq!(ribbon[0].item_id, "local:conky:Night");
        assert_eq!(ribbon[0].confs, [main.clone(), w2.clone(), "/run/user/1000/Night-DP-1.conf".to_string()]);
        let ids = crate::de_apply::conky_running_ids(&io, &ctx);
        assert_eq!(ids.iter().filter(|i| *i == "Night").count(), 1, "{ids:?}");
        let procs = crate::conky_procs::all(&io);
        let (_, text) = crate::conky_procs::stop_all_question(&ctx, &[], &procs).unwrap();
        assert_eq!(text.matches("• Night").count(), 1, "{text}");
        // A set stops the theme by its main file, with all its windows.
        let pids = crate::conky_procs::pids_of_units(&io, &ctx, std::slice::from_ref(&main));
        assert_eq!(pids, ["201", "202", "203"]);
        let pids: Vec<String> = procs.iter().map(|p| p.pid.clone()).collect();
        let (ok, msg) = crate::conky_procs::stop_all(&io, &pids);
        assert!(ok);
        assert_eq!(msg, "⏹️ Stopped 2 Conky widgets");
        let _ = std::fs::remove_dir_all(&d);
    }

}
