//! "Stop all" in the "Conky widgets" section: all conky processes on the
//! machine — our own, those found on disk, and those the person started
//! bypassing Rexon.
//!
//! Never use `killall conky` / `pkill conky`: only what Rexon has first found
//! in `/proc` and shown to the person as a list is stopped, by process ID. A
//! process counts as conky only if both the program name in its command line
//! and the kernel process name (`/proc/<pid>/comm`) are `conky`: a script with
//! "conky" in its arguments (`sh start.sh conky`) or another program with that
//! word in its path is not affected. Before `kill` every PID is checked once
//! more: while the question was on screen the process may have exited and its
//! PID may have gone to another program.

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// A running conky process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkyProc {
    pub pid: String,
    /// The settings file from the command line; `None` means conky without `-c`
    /// (then it reads `~/.config/conky/conky.conf` or `~/.conkyrc`).
    pub conf: Option<String>,
    /// The theme folder, if conky was brought up by the theme's launch script
    /// (the [`crate::conky_theme::MARKER_ENV`] tag in the environment).
    pub widget: Option<String>,
}

/// Whether this is conky: the program `conky` in the command line, and `conky`
/// as the kernel's process name.
fn is_conky(cmdline: &str, comm: Option<&str>) -> bool {
    let prog = cmdline.split('\0').next().unwrap_or("");
    prog.rsplit('/').next() == Some("conky") && comm.map(str::trim) == Some("conky")
}

fn proc_of(io: &dyn LookIo, pid: &str) -> Option<ConkyProc> {
    let cmd = io.read(&format!("/proc/{pid}/cmdline"))?;
    let comm = io.read(&format!("/proc/{pid}/comm"));
    if !is_conky(&cmd, comm.as_deref()) {
        return None;
    }
    // A copy of a widget on the invisible snapshot screen (`conky_preview`) is
    // not a widget on the desktop: it is not included in "On the desktop now",
    // "Stop all" or "is it running".
    if crate::de_apply::conky_launch_of_cmdline(&cmd).is_some_and(|l| crate::conky_preview::is_preview_launch(&l)) {
        return None;
    }
    Some(ConkyProc {
        pid: pid.to_string(),
        conf: crate::de_apply::conky_conf_of_cmdline(&cmd),
        widget: crate::conky_theme::marker_of_pid(io, pid),
    })
}

/// All conky processes right now, in ascending PID order.
pub fn all(io: &dyn LookIo) -> Vec<ConkyProc> {
    let mut pids: Vec<u64> = io.list_dir("/proc").iter().filter_map(|p| p.parse().ok()).collect();
    pids.sort_unstable();
    pids.into_iter().filter_map(|p| proc_of(io, &p.to_string())).collect()
}

/// How to name a process to the person: the Rexon widget name, the name of a
/// found widget marked "not from Rexon", the settings file name, or "conky
/// without a settings file".
pub fn label(ctx: &Ctx, extra: &[String], p: &ConkyProc) -> String {
    // A window of a theme brought up by a script is named after the theme (all its windows are one line).
    if let Some(dir) = p.widget.as_deref() {
        return crate::conky_scan::widget_title(&crate::conky_theme::widget_of_dir(ctx, extra, dir));
    }
    let Some(conf) = p.conf.as_deref() else {
        return tr("conky with its default settings file");
    };
    if let Some(id) = crate::de_apply::conky_widget_of_conf(&crate::animations::SystemIo, ctx, conf) {
        return crate::conky_scan::widget_title(&id);
    }
    let owner = crate::conky_scan::owner_of_conf(ctx, extra, conf).unwrap_or_else(|| conf.to_string());
    let name = owner.rsplit('/').next().unwrap_or(&owner).to_string();
    // TRANSLATORS: %1 is a Conky widget name.
    trf("%1 (not from Rexon)", &[&name])
}

/// The question before "Stop all": `(title, text)` with a list of what will be
/// stopped. `None` means there is nothing to stop.
pub fn stop_all_question(ctx: &Ctx, extra: &[String], procs: &[ConkyProc]) -> Option<(String, String)> {
    if procs.is_empty() {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for p in procs {
        let l = label(ctx, extra, p);
        if !names.contains(&l) {
            names.push(l);
        }
    }
    let list = names.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n");
    Some((
        tr("Stop all Conky widgets?"),
        trf(
            // TRANSLATORS: %1 is a bulleted list of widget names, one per line.
            "Every running Conky widget will be stopped, including the ones not started from Rexon:\n\n%1",
            &[&list],
        ),
    ))
}

/// Stops the processes `pids` — only those that are still conky now (a PID may
/// have moved to another program while the person was reading the question).
/// Returns `(succeeded, message)`.
pub fn stop_all(io: &dyn LookIo, pids: &[String]) -> (bool, String) {
    let alive: Vec<String> = pids.iter().filter(|p| proc_of(io, p).is_some()).cloned().collect();
    if alive.is_empty() {
        return (true, tr("⏹️ No Conky widgets are running"));
    }
    // A theme brought up by a script is stopped as a whole: with its loops and its group.
    let (ok, out) = crate::conky_theme::kill_targets(io, &crate::conky_theme::with_companions(io, alive.clone()));
    if !ok {
        return (false, trf("Couldn't stop the Conky widgets: %1", &[&out.trim().to_string()]));
    }
    // The windows of one theme with a script are one widget.
    let mut units: Vec<String> = Vec::new();
    for p in &alive {
        let u = crate::conky_theme::marker_of_pid(io, p).unwrap_or_else(|| p.clone());
        if !units.contains(&u) {
            units.push(u);
        }
    }
    (true, ntr("⏹️ Stopped %n Conky widget", "⏹️ Stopped %n Conky widgets", units.len() as u64))
}

// ════════════════════════════════════════════════════════════════════════════
//  Starting and stopping widget parts
// ════════════════════════════════════════════════════════════════════════════

/// The settings files of all running conky instances; for conky without `-c`,
/// the file it reads by itself ([`crate::conky_scan::default_conf`]).
pub fn running_confs(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in all(io) {
        let conf = p.conf.unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
        if !out.contains(&conf) {
            out.push(conf);
        }
    }
    out
}

/// Running widgets, one file per widget: like [`running_confs`], but all
/// windows of a theme with a launch script are listed under one file, the
/// theme's main file ([`crate::conky_theme::unit_conf`]). For sets: a theme in
/// a set is one line, and the set brings it up once.
pub fn running_widget_confs(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let extra = crate::conky_scan::extra_dirs_for(ctx);
    let mut out: Vec<String> = Vec::new();
    for p in all(io) {
        let conf = p.conf.unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
        let unit = crate::conky_theme::unit_conf(io, ctx, &extra, &conf, p.widget.as_deref());
        if !out.contains(&unit) {
            out.push(unit);
        }
    }
    out
}

/// The conky PIDs of widgets `units` (files, as in [`running_widget_confs`]):
/// for a theme with a script, all of its windows.
pub fn pids_of_units(io: &dyn LookIo, ctx: &Ctx, units: &[String]) -> Vec<String> {
    let extra = crate::conky_scan::extra_dirs_for(ctx);
    all(io)
        .into_iter()
        .filter(|p| {
            let conf = p.conf.clone().unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
            units.contains(&conf) || units.contains(&crate::conky_theme::unit_conf(io, ctx, &extra, &conf, p.widget.as_deref()))
        })
        .map(|p| p.pid)
        .collect()
}

/// The PIDs of conky processes whose settings file passes `belongs`.
pub fn pids_where(io: &dyn LookIo, ctx: &Ctx, belongs: &dyn Fn(&str) -> bool) -> Vec<String> {
    all(io)
        .into_iter()
        .filter(|p| {
            let conf = p.conf.clone().unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
            belongs(&conf)
        })
        .map(|p| p.pid)
        .collect()
}

/// Stops the processes `pids` of widget `title`. Returns `(succeeded, message)`.
pub fn stop_pids(io: &dyn LookIo, title: &str, pids: Vec<String>) -> (bool, String) {
    if !pids.is_empty() {
        // For a theme with a launch script: all of its processes and its group.
        let (ok, out) = crate::conky_theme::kill_targets(io, &crate::conky_theme::with_companions(io, pids));
        if !ok {
            return (false, trf("Couldn't stop “%1”: %2", &[&title, &out.trim().to_string()]));
        }
    }
    (true, trf("⏹️ Stopped: %1", &[&title]))
}

/// Starts conky with the file `conf` (a part of widget `title`): from the
/// file's folder, with errors going to the log ([`crate::conky_log`]). The
/// truth is a process in the system after a second; if there is none, the
/// cause from the log in conky's own words.
pub fn start_conf(io: &dyn LookIo, ctx: &Ctx, title: &str, conf: &str) -> (bool, String) {
    // A theme with a launch script (`start.sh`) is brought up only by it — as a
    // whole, with all its windows and background pollers (`conky_theme`).
    if let Some(name) = crate::conky_theme::script_widget_of_conf(io, ctx, conf) {
        return crate::conky_theme::start_widget_script(io, ctx, &name);
    }
    if !io.have("conky") {
        return (false, crate::de_apply::conky_missing_refusal());
    }
    let log = crate::conky_log::live_log_path(ctx, conf);
    let started = start_with(io, ctx, "conky", title, conf, &log);
    // The first start also takes a snapshot of the widget for the tile (in the background, X11).
    if started.0 {
        crate::conky_preview::capture_later(ctx, conf, &log);
    }
    started
}

/// [`start_conf`] with a given program and log (checks substitute their own
/// program for conky).
///
/// In a Wayland session conky goes through XWayland where its Wayland output
/// is not suitable ([`crate::conky_overlay::via_xwayland`]). If the Wayland
/// output failed at once and XWayland is available, there is a second start
/// through it: a widget on the desktop is better than an honest error.
pub fn start_with(io: &dyn LookIo, ctx: &Ctx, program: &str, title: &str, conf: &str, log: &str) -> (bool, String) {
    // The theme names its screen (`theme.json`, `monitor`) and the person has not
    // chosen a screen yet, so it becomes the initial choice.
    crate::conky_theme::seed_monitor(io, ctx, conf);
    // The person changed the widget settings: conky runs with the Rexon overlay,
    // while the working directory stays the folder of the author's file.
    let paths = crate::conky_overlay::live_paths(ctx, conf);
    let launch = crate::conky_overlay::launch_conf(io, ctx, &paths, conf);
    // Through XWayland we use our own file: an X11 window "below all windows"
    // instead of the author's override-redirect, otherwise Muffin/Mutter put the
    // widget above windows ([`crate::conky_overlay::xwayland_path`]).
    let argv_for = |xwayland: bool| {
        let file = if xwayland {
            let env = crate::conky_overlay::Env { version: crate::conky_overlay::conky_version(io), de: ctx.de.clone() };
            crate::conky_overlay::write_xwayland(io, &paths, conf, &env).unwrap_or_else(|| launch.clone())
        } else {
            launch.clone()
        };
        crate::de_apply::conky_start_command(&file).0
    };
    let dir = crate::de_apply::conky_work_dir(conf).to_string();
    let needs_x11 = ctx.wayland && crate::conky_overlay::widget_needs_x11(conf);
    let mut xwayland = crate::conky_overlay::live_via_xwayland(io, ctx, needs_x11);
    let mut result = start_once(io, ctx, program, title, conf, log, &argv_for(xwayland), &dir, xwayland);
    if result.is_err() && !xwayland && ctx.wayland && crate::conky_overlay::live_display() {
        crate::log_warn!("conky", "«{title}»: вывод Wayland не поднялся, второй запуск через XWayland");
        xwayland = true;
        result = start_once(io, ctx, program, title, conf, log, &argv_for(true), &dir, true);
    }
    if let Err(why) = result {
        return (false, why);
    }
    let mut msg = trf("▶️ Started: %1", &[&title]);
    // Through XWayland the widget stays on the desktop as in an X11 session, so
    // the caveat about the Wayland window does not apply to it.
    if !xwayland {
        // The conky version is needed for the caveat only in a Wayland session.
        let version = if ctx.wayland { crate::conky_overlay::conky_version(io) } else { None };
        if let Some(note) = crate::de_apply::conky_wayland_note_for(&ctx.de, ctx.wayland, version) {
            msg.push_str(" — ");
            msg.push_str(&note);
        }
    }
    (true, msg)
}

/// How long conky must live after start to count as started: the Wayland
/// output of 1.25.2 crashes after a second and a half (Jhalak, 15.11.2026),
/// and the old check "the process exists after a second" said "Started".
const SURVIVE_MS: u64 = 3000;

/// A single start: `Ok` means a conky process with this file appeared and
/// lived until [`SURVIVE_MS`]; `Err` is a message with the cause from the log.
#[allow(clippy::too_many_arguments)]
fn start_once(
    io: &dyn LookIo,
    ctx: &Ctx,
    program: &str,
    title: &str,
    conf: &str,
    log: &str,
    argv: &[String],
    dir: &str,
    xwayland: bool,
) -> Result<(), String> {
    // Output goes to a file, not a pipe: conky with `-d` goes into the background
    // and keeps the inherited output, so with a pipe the button would wait for it
    // to end.
    let stderr = match crate::conky_log::open_for_start(log) {
        Some(f) => std::process::Stdio::from(f),
        None => std::process::Stdio::null(),
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(&argv[1..])
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr);
    if xwayland {
        crate::conky_overlay::apply_xwayland(&mut cmd);
    }
    if let Err(e) = cmd.status() {
        return Err(trf("Couldn't start “%1”: %2", &[&title, &e.to_string()]));
    }
    let step = std::time::Duration::from_millis(100);
    let alive = || running_confs(io, ctx).iter().any(|c| c == conf);
    let mut seen = false;
    for i in 0..SURVIVE_MS / 100 {
        std::thread::sleep(step);
        let now = alive();
        // Appeared and disappeared: crashed at startup; did not appear within a
        // second: did not start.
        if seen && !now {
            seen = false;
            break;
        }
        seen |= now;
        if !seen && i >= 9 {
            break;
        }
    }
    if seen {
        return Ok(());
    }
    let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let why = match crate::conky_log::failure_reason(&text) {
        Some(reason) => trf("conky closed right after starting: %1", &[&reason]),
        None => tr("conky closed right after starting — the widget's settings file may be broken"),
    };
    Err(trf("Couldn't start “%1”: %2", &[&title, &why]))
}

// ════════════════════════════════════════════════════════════════════════════
//  For the page: widget parts, the log, section labels
// ════════════════════════════════════════════════════════════════════════════

/// A widget part for the sub-list in the card.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Part {
    pub conf: String,
    /// The file path relative to the widget directory (`variants/small.conf`).
    pub label: String,
    pub running: bool,
}

/// The parts of widget `name` (the tile name): the main one first.
pub fn parts(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Vec<Part> {
    let primary = if crate::conky_scan::is_found_name(name) {
        None
    } else {
        crate::de_apply::conky_conf(io, ctx, name)
    };
    let base = crate::conky_scan::widget_path(ctx, name);
    let running = running_confs(io, ctx);
    crate::conky_scan::widget_confs(io, ctx, name, primary)
        .into_iter()
        .map(|conf| Part {
            label: conf
                .strip_prefix(&format!("{base}/"))
                .map(str::to_string)
                .unwrap_or_else(|| crate::conky_scan::widget_title(&conf)),
            running: running.contains(&conf),
            conf,
        })
        .collect()
}

/// The part button: if running, stop its processes; if not, start it. A
/// foreign file (not a part of this widget) is refused without any action.
pub fn press_part(io: &dyn LookIo, ctx: &Ctx, name: &str, conf: &str) -> (bool, String) {
    let Some(part) = parts(io, ctx, name).into_iter().find(|p| p.conf == conf) else {
        return (false, tr("This file isn't part of the widget."));
    };
    let title = format!("{} — {}", crate::conky_scan::widget_title(name), part.label);
    if part.running {
        stop_pids(io, &title, pids_where(io, ctx, &|c| c == conf))
    } else if let Some(why) = crate::conky_safety::start_refusal(io, ctx, name) {
        // A part of a widget is the same widget: it does not start without permission.
        (false, why)
    } else {
        start_conf(io, ctx, &title, conf)
    }
}

/// The last lines of the widget log for the card; empty means Rexon has not
/// started it yet.
pub fn log_tail(io: &dyn LookIo, ctx: &Ctx, name: &str) -> String {
    let confs: Vec<String> = parts(io, ctx, name).into_iter().map(|p| p.conf).collect();
    crate::conky_log::widget_log(ctx, &confs).map(|(_, text)| text).unwrap_or_default()
}

/// The folder of widget `name` for "Open folder": the theme directory, or for
/// a single-file widget (`~/.conkyrc`, a found standalone file) the folder
/// where that file lives. `None` means the widget is no longer on disk.
pub fn widget_folder(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Option<String> {
    let path = crate::conky_scan::widget_path(ctx, name);
    if !io.list_dir(&path).is_empty() {
        return Some(path);
    }
    io.read(&path)?;
    let parent = path.rsplit_once('/').map(|(dir, _)| dir).filter(|d| !d.is_empty())?;
    Some(parent.to_string())
}

/// The settings file `conf` for "Open settings file", only if it is one of the
/// parts of widget `name`: the page cannot ask to open an arbitrary path.
pub fn part_file(io: &dyn LookIo, ctx: &Ctx, name: &str, conf: &str) -> Option<String> {
    parts(io, ctx, name).into_iter().map(|p| p.conf).find(|c| c == conf)
}

/// Labels of the "Conky widgets" section for the page.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SectionTexts {
    pub running_chip: String,
    pub running_chip_tooltip: String,
    pub stop_all: String,
    pub stop_all_tooltip: String,
    pub parts_title: String,
    pub part_start: String,
    pub part_stop: String,
    pub log_title: String,
    pub ribbon_title: String,
    pub ribbon_open_tooltip: String,
    pub ribbon_stop_tooltip: String,
    pub open_folder: String,
    pub open_folder_tooltip: String,
    pub open_file: String,
    pub open_file_tooltip: String,
    /// The button for a theme made in Conky Studio (`conky_theme::studio_edit_argv`).
    pub edit_in_studio: String,
    pub edit_in_studio_tooltip: String,
}

pub fn section_texts() -> SectionTexts {
    SectionTexts {
        running_chip: tr("▶️ Running"),
        running_chip_tooltip: tr("Show only the widgets that are running now"),
        stop_all: tr("⏹️ Stop all"),
        stop_all_tooltip: tr("Stop every running Conky widget, including the ones not started from Rexon"),
        parts_title: tr("Parts of the widget"),
        // The same words as on the tile button (`appearance_page::card_local_button_for`).
        part_start: tr("▶️ Start"),
        part_stop: tr("⏹️ Stop"),
        log_title: tr("What Conky reported at the last start"),
        ribbon_title: tr("On the desktop now"),
        ribbon_open_tooltip: tr("Open the widget's card: settings, parts, log"),
        ribbon_stop_tooltip: tr("Remove from the desktop (the widget stays installed)"),
        open_folder: tr("📂 Open folder"),
        open_folder_tooltip: tr("Open the widget's folder in the file manager"),
        open_file: tr("📝 Open settings file"),
        open_file_tooltip: tr(
            "Open the widget's settings file in your text editor. Restart the widget to see your changes."
        ),
        edit_in_studio: tr("✏️ Edit in Conky Studio"),
        edit_in_studio_tooltip: tr(
            "This theme was made in Conky Studio, a visual editor for Conky widgets. Open it there; restart the widget to see your changes."
        ),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  The "On the desktop now" strip
// ════════════════════════════════════════════════════════════════════════════

/// A running widget in the strip above the grid, like gadgets on the Windows
/// desktop: you see at once what is there, a click opens the card, the cross
/// removes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RibbonItem {
    /// The tile id in the section (`local:conky:<name or path>`).
    pub item_id: String,
    pub title: String,
    /// The picture: the widget's own or a Rexon snapshot; empty means the 📊 sign.
    pub picture: String,
    /// The running settings files of this widget.
    pub confs: Vec<String>,
}

/// Running widgets, one per widget (parts together), in start order. A conky
/// process without a settings file is a widget too (the file conky reads by
/// itself).
pub fn ribbon(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<RibbonItem> {
    let mut out: Vec<RibbonItem> = Vec::new();
    for p in all(io) {
        let conf = p.conf.unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
        // The windows of a theme with a script belong to its tile, even if a window
        // file lies outside its folder (a copy for a particular screen).
        let name = match p.widget.as_deref() {
            Some(dir) => crate::conky_theme::widget_of_dir(ctx, extra, dir),
            None => crate::conky_sets::widget_name_of(ctx, extra, &conf),
        };
        let id = format!("local:conky:{name}");
        if let Some(it) = out.iter_mut().find(|i| i.item_id == id) {
            if !it.confs.contains(&conf) {
                it.confs.push(conf);
            }
            continue;
        }
        out.push(RibbonItem { item_id: id, title: crate::conky_scan::widget_title(&name), picture: String::new(), confs: vec![conf] });
    }
    for it in &mut out {
        let name = it.item_id.trim_start_matches("local:conky:").to_string();
        let path = crate::conky_scan::widget_path(ctx, &name);
        let mut confs = it.confs.clone();
        for c in crate::conky_scan::widget_confs(io, ctx, &name, None) {
            if !confs.contains(&c) {
                confs.push(c);
            }
        }
        it.picture = crate::conky_scan::widget_picture(io, &path, &confs)
            .or_else(|| crate::conky_preview::cached(ctx, &confs))
            .unwrap_or_default();
    }
    out
}

/// The strip's cross: stops exactly these settings files (by PID; others are
/// not affected).
pub fn stop_confs(io: &dyn LookIo, ctx: &Ctx, title: &str, confs: &[String]) -> (bool, String) {
    stop_pids(io, title, pids_where(io, ctx, &|c| confs.iter().any(|x| x == c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeProc {
        files: HashMap<String, String>,
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeProc {
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
            let mut v: Vec<String> = self
                .files
                .keys()
                .filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string()))
                .collect();
            v.push("self".into());
            v.sort();
            v.dedup();
            v
        }
    }

    fn proc(files: &mut HashMap<String, String>, pid: u32, cmd: &[&str], comm: &str) {
        files.insert(format!("/proc/{pid}/cmdline"), cmd.join("\0") + "\0");
        files.insert(format!("/proc/{pid}/comm"), format!("{comm}\n"));
    }

    fn ctx() -> Ctx {
        Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() }
    }

    fn fixture() -> FakeProc {
        let mut f = HashMap::new();
        proc(&mut f, 120, &["conky", "-d", "-c", "/h/.config/conky/Jhalak/Jhalak.conf"], "conky");
        proc(&mut f, 99, &["/usr/bin/conky", "-c", "/h/.conky/Gotham/Gotham"], "conky");
        proc(&mut f, 300, &["conky"], "conky");
        // A Jhalak copy on the invisible snapshot screen is not a widget on the desktop.
        proc(
            &mut f,
            130,
            &["conky", "-c", "/run/user/1000/rexon/conky-preview/h+s.config+sconky+sJhalak+sJhalak.conf.preview.conf"],
            "conky",
        );
        // Not conky: a launch script with "conky" in its arguments, a file editor.
        proc(&mut f, 400, &["sh", "start.sh", "conky"], "sh");
        proc(&mut f, 401, &["/usr/bin/vim", "/h/.config/conky/x/conky.conf"], "vim");
        // The program name is spoofed, and the kernel knows it as something else.
        proc(&mut f, 402, &["conky"], "python3");
        FakeProc { files: f, ran: RefCell::new(Vec::new()) }
    }

    #[test]
    fn only_real_conky_processes_are_listed() {
        let io = fixture();
        let got = all(&io);
        let pids: Vec<&str> = got.iter().map(|p| p.pid.as_str()).collect();
        assert_eq!(pids, ["99", "120", "300"]);
        assert_eq!(got[2].conf, None);
    }

    #[test]
    fn question_lists_own_and_foreign_widgets() {
        crate::i18n::set_language(Some("en"));
        let io = fixture();
        let (_, text) = stop_all_question(&ctx(), &[], &all(&io)).unwrap();
        assert!(text.contains("• Gotham (not from Rexon)"), "{text}");
        assert!(text.contains("• Jhalak\n"), "{text}");
        assert!(text.contains("• conky with its default settings file"), "{text}");
        assert!(stop_all_question(&ctx(), &[], &[]).is_none());
    }

    /// conky crashed at once: the message names the cause from its output.
    #[test]
    fn failed_start_tells_the_reason_from_log() {
        use std::os::unix::fs::PermissionsExt;
        crate::i18n::set_language(Some("en"));
        let dir = std::env::temp_dir().join(format!("rexon-conky-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("w")).unwrap();
        let fake = dir.join("conky");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho \"[2026-10-19 00:21:46.200] [error] [src/lua/llua.cc:339] lua script './x.lua' not found\" >&2\npwd >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let conf = dir.join("w/conky.conf").to_string_lossy().into_owned();
        std::fs::write(&conf, "conky.text = [[x]]").unwrap();
        let log = dir.join("log/w.log").to_string_lossy().into_owned();
        let io = FakeProc { files: HashMap::new(), ran: RefCell::new(Vec::new()) };
        let (ok, msg) = start_with(&io, &ctx(), &fake.to_string_lossy(), "w", &conf, &log);
        assert!(!ok);
        assert!(msg.contains("lua script './x.lua' not found"), "{msg}");
        // Started from the widget folder, output goes to the log.
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains(&dir.join("w").to_string_lossy().into_owned()), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_confs_cover_default_config() {
        let io = fixture();
        let confs = running_confs(&io, &ctx());
        assert!(confs.contains(&"/h/.conkyrc".to_string()), "{confs:?}");
        assert!(confs.contains(&"/h/.conky/Gotham/Gotham".to_string()));
        let pids = pids_where(&io, &ctx(), &|c| c.starts_with("/h/.conky/"));
        assert_eq!(pids, ["99"]);
    }

    /// Only the shown processes that are still conky are stopped by PID: PID 401
    /// went to an editor while the question was on screen, so it is left alone.
    #[test]
    fn stop_all_kills_by_pid_and_rechecks() {
        let io = fixture();
        let (ok, _) = stop_all(&io, &["99".into(), "120".into(), "401".into(), "777".into()]);
        assert!(ok);
        assert_eq!(*io.ran.borrow(), vec![vec!["kill".to_string(), "--".into(), "99".into(), "120".into()]]);
        let io2 = fixture();
        assert!(stop_all(&io2, &["401".into()]).0);
        assert!(io2.ran.borrow().is_empty(), "некого гасить — kill не зовётся");
    }

    /// Directories and files for "Open folder" without /proc.
    struct DirIo(HashMap<String, String>);

    impl LookIo for DirIo {
        fn run(&self, _: &[String]) -> (bool, String) {
            (false, String::new())
        }
        fn have(&self, _: &str) -> bool {
            false
        }
        fn read(&self, path: &str) -> Option<String> {
            self.0.get(path).cloned()
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{path}/");
            let mut v: Vec<String> = self
                .0
                .keys()
                .filter_map(|k| k.strip_prefix(&pre)?.split('/').next().map(str::to_string))
                .collect();
            v.sort();
            v.dedup();
            v
        }
    }

    /// "Open folder": for a theme, its directory; for a single-file widget, the
    /// file's folder; for a vanished widget, nothing. "Open file": only a widget part.
    #[test]
    fn folder_and_file_to_open() {
        let mut f = HashMap::new();
        f.insert("/h/.config/conky/Jhalak/Jhalak.conf".to_string(), "conky.text = [[x]]".to_string());
        f.insert("/h/.conkyrc".to_string(), "conky.text = [[y]]".to_string());
        let io = DirIo(f);
        assert_eq!(widget_folder(&io, &ctx(), "Jhalak").as_deref(), Some("/h/.config/conky/Jhalak"));
        assert_eq!(widget_folder(&io, &ctx(), "/h/.conkyrc").as_deref(), Some("/h"));
        assert_eq!(widget_folder(&io, &ctx(), "Gone"), None);
        assert_eq!(widget_folder(&io, &ctx(), "/h/.conky/Gone"), None);
        assert_eq!(
            part_file(&io, &ctx(), "Jhalak", "/h/.config/conky/Jhalak/Jhalak.conf").as_deref(),
            Some("/h/.config/conky/Jhalak/Jhalak.conf")
        );
        assert_eq!(part_file(&io, &ctx(), "Jhalak", "/etc/passwd"), None);
        assert_eq!(part_file(&io, &ctx(), "/h/.conkyrc", "/h/.conkyrc").as_deref(), Some("/h/.conkyrc"));
    }

    /// The strip: one item per widget, in start order; the cross stops only the
    /// processes of its own widget.
    #[test]
    fn ribbon_lists_running_widgets_and_stops_only_theirs() {
        let f = fixture();
        let r = ribbon(&f, &ctx(), &[]);
        let ids: Vec<&str> = r.iter().map(|i| i.item_id.as_str()).collect();
        assert_eq!(ids, ["local:conky:/h/.conky/Gotham", "local:conky:Jhalak", "local:conky:/h/.conkyrc"]);
        assert_eq!(r[1].title, "Jhalak");
        assert_eq!(r[1].confs, ["/h/.config/conky/Jhalak/Jhalak.conf"]);
        let (ok, _) = stop_confs(&f, &ctx(), "Jhalak", &r[1].confs);
        assert!(ok);
        assert_eq!(*f.ran.borrow(), [vec!["kill".to_string(), "--".to_string(), "120".to_string()]]);
    }
}
