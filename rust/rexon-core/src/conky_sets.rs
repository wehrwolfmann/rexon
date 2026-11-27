//! Conky widget sets: several widgets and a wallpaper — with one button.
//!
//! WHY. A person builds a desktop from several widgets (a clock, the CPU,
//! the network) for a particular wallpaper and wants to switch between such
//! "layouts" rather than start each widget anew. Conky manager programs
//! keep such a set as a `.cmtheme` file — sets in theme packs ship as the same files
//! (`Green Apple Desktop.cmtheme`). Rexon:
//! * saves the widgets running now and the current wallpaper as its own set
//!   ([`save`]) in `~/.config/rexon/conky-sets.json`;
//! * also shows sets from `.cmtheme` files lying in the widget
//!   folders ([`file_sets`]) — a theme pack gets installed, and its sets are immediately
//!   ready to apply;
//! * applies a set ([`apply`]): stops ONLY the widgets of the previous set
//!   that are not in the new one (it does not touch foreign ones or those started by hand — no
//!   "stop all conky"), starts the missing ones and hands the wallpaper to the caller,
//!   so that Rexon's shared wallpaper executor sets it (any desktop).
//!
//! A `.cmtheme` file is lines: the path to a widget's settings file, the path to a wallpaper
//! picture (`.png`/`.jpg`/`.jpeg`), `wallpaper-scaling:<mode>`; a path may
//! be in quotes and start with `~`. Paths like `~/.conky/<Name>/…` lead
//! to where Rexon keeps such a widget (`~/.config/conky/<Name>/…`) if there is no file
//! at the old place ([`resolve`]).
//!
//! Safety is the same as for the "Start" button: a widget with commands and
//! scripts does not start without permission ([`crate::conky_safety`]); the window
//! asks once for the whole set ([`apply_question`]), the command line
//! skips such widgets and says so. Programs are also as with
//! "Start" ("all or nothing", [`crate::conky_safety::prepare_start`]):
//! missing ones from the repositories are installed without a question with one password window for
//! the whole set, a widget that still lacks them does not start.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// The sets file (under `$XDG_CONFIG_HOME`).
pub const STORE_REL: &str = "rexon/conky-sets.json";

/// The set file extension of conky manager programs.
pub const CMTHEME_EXT: &str = ".cmtheme";

/// A widget set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ConkySet {
    pub name: String,
    /// Widget settings files (paths of the author's files).
    pub widgets: Vec<String>,
    /// The wallpaper picture; `None` — the set does not change the wallpaper.
    #[serde(default)]
    pub wallpaper: Option<String>,
    /// The `.cmtheme` file the set was read from (`None` for our own).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// What lies in the sets file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Store {
    #[serde(default)]
    pub sets: Vec<ConkySet>,
    /// The name of the set applied last — its widgets are stopped when
    /// another one is applied.
    #[serde(default)]
    pub applied: Option<String>,
    /// The widgets the last application started (the contents of a set from a file
    /// may have changed — what is stopped is what was actually started).
    #[serde(default)]
    pub applied_widgets: Vec<String>,
}

pub fn store_path(ctx: &Ctx) -> String {
    ctx.config(STORE_REL)
}

pub fn load(io: &dyn LookIo, ctx: &Ctx) -> Store {
    io.read(&store_path(ctx)).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store(io: &dyn LookIo, ctx: &Ctx, s: &Store) -> bool {
    io.write(&store_path(ctx), &serde_json::to_string_pretty(s).unwrap_or_default())
}

// ============================================================================
// The .cmtheme file
// ============================================================================

/// A parsed `.cmtheme`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cmtheme {
    /// Widget paths as in the file, `~` expanded.
    pub widgets: Vec<String>,
    pub wallpaper: Option<String>,
    /// The wallpaper placement mode (`zoomed`, `stretched`…) — for reference: Rexon
    /// sets the picture with its wallpaper module's mode.
    pub scaling: Option<String>,
}

fn is_image(path: &str) -> bool {
    let l = path.to_ascii_lowercase();
    l.ends_with(".png") || l.ends_with(".jpg") || l.ends_with(".jpeg")
}

/// `~` and `$HOME` at the start of a path are the home directory (in the middle of a path `~` is
/// part of a name, we do not touch it).
fn expand_home(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if path == "~" || path == "$HOME" {
        return home.to_string();
    }
    for p in ["~/", "$HOME/"] {
        if let Some(rest) = path.strip_prefix(p) {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

/// Parse the `.cmtheme` text.
pub fn parse_cmtheme(text: &str, home: &str) -> Cmtheme {
    let mut out = Cmtheme::default();
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix('"').unwrap_or(line);
        let line = line.strip_suffix('"').unwrap_or(line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("wallpaper-scaling") {
            if let Some(v) = v.trim_start().strip_prefix(':') {
                let v = v.trim().to_ascii_lowercase();
                if !v.is_empty() {
                    out.scaling = Some(v);
                }
            }
            continue;
        }
        let path = expand_home(line, home);
        if is_image(&path) {
            out.wallpaper = Some(path);
        } else if !out.widgets.contains(&path) {
            out.widgets.push(path);
        }
    }
    out
}

/// Where the file actually is: at the recorded path, and if nothing is there and the path leads into
/// `.conky/` (`.Conky/`) — in the same widget folder under `~/.config/conky/`.
/// `None` — the file is neither here nor there.
pub fn resolve(ctx: &Ctx, path: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    if exists(path) {
        return Some(path.to_string());
    }
    let root = crate::de_apply::conky_root(ctx);
    for marker in ["/.conky/", "/.Conky/", "/.config/Conky/"] {
        if let Some((_, tail)) = path.split_once(marker) {
            let moved = format!("{root}/{tail}");
            if exists(&moved) {
                return Some(moved);
            }
        }
    }
    None
}

fn live_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

/// `.cmtheme` files in widget folders: the first and second level under
/// `~/.config/conky/` and the "Found on disk" places.
pub fn cmtheme_files(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<String> {
    let mut dirs = vec![crate::de_apply::conky_root(ctx)];
    for r in crate::conky_scan::roots(ctx, extra) {
        if !r.files_only && !dirs.contains(&r.dir) {
            dirs.push(r.dir);
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        for top in io.list_dir(&dir) {
            if top.starts_with('.') {
                continue;
            }
            let path = format!("{dir}/{top}");
            if top.ends_with(CMTHEME_EXT) {
                out.push(path);
                continue;
            }
            for inner in io.list_dir(&path) {
                if inner.ends_with(CMTHEME_EXT) {
                    out.push(format!("{path}/{inner}"));
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// The set name by file: the name without `.cmtheme`.
fn file_set_name(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    base.strip_suffix(CMTHEME_EXT).unwrap_or(base).to_string()
}

/// A set from a `.cmtheme` file: paths — to where the files actually are;
/// widgets that are not on disk are dropped (their count is the second field).
pub fn set_of_cmtheme(ctx: &Ctx, path: &str, text: &str, exists: &dyn Fn(&str) -> bool) -> (ConkySet, usize) {
    let c = parse_cmtheme(text, &ctx.home);
    let mut widgets = Vec::new();
    let mut missing = 0;
    for w in &c.widgets {
        match resolve(ctx, w, exists) {
            Some(p) if !widgets.contains(&p) => widgets.push(p),
            Some(_) => {}
            None => missing += 1,
        }
    }
    // The picture is checked not by the widgets' `exists` (that one looks at text
    // files) but by the same search: the pack's wallpaper lies next to the set.
    let wallpaper = c.wallpaper.as_deref().and_then(|w| resolve(ctx, w, exists));
    (ConkySet { name: file_set_name(path), widgets, wallpaper, file: Some(path.to_string()) }, missing)
}

/// Sets from `.cmtheme` files on disk.
pub fn file_sets(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<(ConkySet, usize)> {
    cmtheme_files(io, ctx, extra)
        .into_iter()
        .filter_map(|p| io.read(&p).map(|t| set_of_cmtheme(ctx, &p, &t, exists)))
        .collect()
}

// ============================================================================
// The list for the page
// ============================================================================

/// A set row for the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetRow {
    pub name: String,
    /// "Gotham, Seamod · with wallpaper".
    pub summary: String,
    /// The set was applied last.
    pub applied: bool,
    /// Rexon's own set (can be deleted); a set from a file — cannot.
    pub own: bool,
    /// Nothing to apply: not a single widget of the set is on disk.
    pub empty: bool,
    /// The row tooltip (where the set is from, what is missing).
    pub tooltip: String,
}

/// The widget name for messages and permissions by the settings file: a folder under
/// `~/.config/conky/` — the tile name, otherwise — found on disk, otherwise the file
/// itself.
pub fn widget_name_of(ctx: &Ctx, extra: &[String], conf: &str) -> String {
    if let Some(id) = crate::de_apply::conky_widget_of_conf(&crate::animations::SystemIo, ctx, conf) {
        return id;
    }
    crate::conky_scan::owner_of_conf(ctx, extra, conf).unwrap_or_else(|| conf.to_string())
}

fn titles(ctx: &Ctx, extra: &[String], widgets: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in widgets {
        let t = crate::conky_scan::widget_title(&widget_name_of(ctx, extra, w));
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// All sets: our own, then from files (a file with the same name as our own is not duplicated).
pub fn all_sets(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> (Store, Vec<(ConkySet, usize)>) {
    let st = load(io, ctx);
    let mut rows: Vec<(ConkySet, usize)> = st.sets.iter().cloned().map(|s| (s, 0)).collect();
    for (s, missing) in file_sets(io, ctx, extra, exists) {
        if !rows.iter().any(|(r, _)| r.name == s.name) {
            rows.push((s, missing));
        }
    }
    (st, rows)
}

pub fn rows(io: &dyn LookIo, ctx: &Ctx, extra: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<SetRow> {
    let (st, sets) = all_sets(io, ctx, extra, exists);
    sets.into_iter()
        .map(|(s, missing)| {
            let names = titles(ctx, extra, &s.widgets);
            let mut summary = names.join(", ");
            if s.wallpaper.is_some() {
                summary = if summary.is_empty() { tr("wallpaper only") } else { trf("%1 · with wallpaper", &[&summary]) };
            }
            let mut tips: Vec<String> = Vec::new();
            if let Some(f) = &s.file {
                tips.push(trf("From the set file %1", &[f]));
            }
            if missing > 0 {
                tips.push(ntr(
                    "%n widget of this set isn't on this computer.",
                    "%n widgets of this set aren't on this computer.",
                    missing as u64,
                ));
            }
            SetRow {
                applied: st.applied.as_deref() == Some(s.name.as_str()),
                own: s.file.is_none(),
                empty: s.widgets.is_empty() && s.wallpaper.is_none(),
                tooltip: tips.join("\n"),
                name: s.name,
                summary,
            }
        })
        .collect()
}

/// Labels of the "Widget sets" block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Texts {
    pub title: String,
    pub hint: String,
    pub name_placeholder: String,
    pub save: String,
    pub save_tooltip: String,
    pub apply: String,
    pub remove: String,
    pub applied: String,
    pub empty: String,
    pub export: String,
    pub export_tooltip: String,
}

pub fn texts() -> Texts {
    Texts {
        title: tr("Widget sets"),
        hint: tr("A set starts several widgets at once and can change the wallpaper too."),
        name_placeholder: tr("Set name"),
        save: tr("Save running as a set"),
        save_tooltip: tr("Remembers the widgets running now and the current wallpaper."),
        apply: tr("Apply"),
        remove: tr("Delete"),
        applied: tr("✓ Applied"),
        empty: tr("No sets yet: start the widgets you like and save them as a set — then the whole set comes back with one button."),
        export: tr("Save set to file…"),
        export_tooltip: tr("Puts the widgets of the set, their places on the screen and the wallpaper into one .rxn file. On another computer open it with “Install from File…”."),
    }
}

// ============================================================================
// Save, delete
// ============================================================================

/// Whether a set name is valid: not empty, without a newline.
fn clean_name(name: &str) -> Option<String> {
    let n = name.trim();
    (!n.is_empty() && !n.contains('\n') && n.chars().count() <= 80).then(|| n.to_string())
}

/// Save the widgets running now (and the wallpaper `wallpaper`) as the set
/// `name`. Our own set with the same name is replaced.
pub fn save(io: &dyn LookIo, ctx: &Ctx, name: &str, wallpaper: Option<String>) -> (bool, String) {
    let Some(name) = clean_name(name) else {
        return (false, tr("Give the set a name."));
    };
    // A multi-window theme with a script — one row of the set.
    let widgets = crate::conky_procs::running_widget_confs(io, ctx);
    if widgets.is_empty() {
        return (false, tr("No widget is running now: start the widgets for the set first."));
    }
    let mut st = load(io, ctx);
    let replaced = st.sets.iter().any(|s| s.name == name);
    st.sets.retain(|s| s.name != name);
    st.sets.push(ConkySet { name: name.clone(), widgets: widgets.clone(), wallpaper, file: None });
    // What is saved is what is running: that is the applied set.
    st.applied = Some(name.clone());
    st.applied_widgets = widgets;
    if !store(io, ctx, &st) {
        return (false, trf("Couldn't save the set “%1”", &[&name]));
    }
    let msg = if replaced { trf("Set “%1” updated", &[&name]) } else { trf("Set “%1” saved", &[&name]) };
    (true, msg)
}

/// The question before deleting a set: `(title, text)`.
pub fn remove_question(name: &str) -> (String, String) {
    (
        trf("Delete the set “%1”?", &[&name]),
        tr("The widgets and the wallpaper stay as they are — only the set is forgotten."),
    )
}

/// Delete our own set (widgets and files are not touched).
pub fn remove(io: &dyn LookIo, ctx: &Ctx, name: &str) -> (bool, String) {
    let mut st = load(io, ctx);
    let before = st.sets.len();
    st.sets.retain(|s| s.name != name);
    if st.sets.len() == before {
        return (false, trf("There's no set “%1” to delete", &[&name]));
    }
    if st.applied.as_deref() == Some(name) {
        st.applied = None;
        st.applied_widgets.clear();
    }
    if !store(io, ctx, &st) {
        return (false, trf("Couldn't delete the set “%1”", &[&name]));
    }
    (true, trf("Set “%1” deleted", &[&name]))
}

// ============================================================================
// Apply
// ============================================================================

/// What applying will do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub set: ConkySet,
    /// Running widgets of the previous set that are not in this one — stop them.
    pub stop: Vec<String>,
    /// Widgets of the set that are not running now — start them.
    pub start: Vec<String>,
    /// Names of widgets from `start` that need permission for commands.
    pub needs_trust: Vec<String>,
}

pub fn find(io: &dyn LookIo, ctx: &Ctx, extra: &[String], name: &str, exists: &dyn Fn(&str) -> bool) -> Option<ConkySet> {
    let (_, sets) = all_sets(io, ctx, extra, exists);
    sets.into_iter().map(|(s, _)| s).find(|s| s.name == name)
}

pub fn plan(io: &dyn LookIo, ctx: &Ctx, extra: &[String], mut set: ConkySet) -> Plan {
    let st = load(io, ctx);
    // The windows of one theme with a script (a set saved before 16.11.2026 or from
    // a file) — one row: the theme is brought up and stopped once.
    set.widgets = crate::conky_theme::unit_confs(io, ctx, extra, &set.widgets);
    let running = crate::conky_procs::running_widget_confs(io, ctx);
    let prev: Vec<String> = if st.applied.is_some() {
        crate::conky_theme::unit_confs(io, ctx, extra, &st.applied_widgets)
    } else {
        Vec::new()
    };
    let stop: Vec<String> = prev.into_iter().filter(|w| running.contains(w) && !set.widgets.contains(w)).collect();
    let start: Vec<String> = set.widgets.iter().filter(|w| !running.contains(w)).cloned().collect();
    let mut needs_trust: Vec<String> = Vec::new();
    for w in &start {
        let name = widget_name_of(ctx, extra, w);
        if !needs_trust.contains(&name) && crate::conky_safety::start_refusal(io, ctx, &name).is_some() {
            needs_trust.push(name);
        }
    }
    Plan { set, stop, start, needs_trust }
}

/// One question for the whole set: which widgets run commands. `None` —
/// nothing to ask about.
pub fn apply_question(io: &dyn LookIo, ctx: &Ctx, p: &Plan) -> Option<(String, String)> {
    if p.needs_trust.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for name in &p.needs_trust {
        if let Some((_, text)) = crate::conky_safety::start_question(io, ctx, name) {
            parts.push(text);
        }
    }
    Some((
        trf("Apply the set “%1”?", &[&p.set.name]),
        format!(
            "{}\n\n{}",
            ntr(
                "%n widget of this set runs commands on your computer:",
                "%n widgets of this set run commands on your computer:",
                p.needs_trust.len() as u64,
            ),
            parts.join("\n\n")
        ),
    ))
}

/// The result of applying.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    pub ok: bool,
    pub message: String,
    /// The set's wallpaper — set it with the shared wallpaper executor.
    pub wallpaper: Option<String>,
}

/// Starting one widget: `(widget name, label, settings file)` →
/// `(succeeded, message)`. Live — [`live_start`]; checks substitute
/// their own so as not to start conky.
pub type Starter<'a> = &'a dyn Fn(&str, &str, &str) -> (bool, String);

/// A live widget start: path repair and `conky` from the widget folder with
/// a log — like the "Start" button.
pub fn live_start(name: &str, title: &str, conf: &str) -> (bool, String) {
    let ctx = Ctx::live();
    crate::de_apply::conky_prepare(&ctx, name);
    crate::conky_procs::start_conf(&crate::animations::SystemIo, &ctx, title, conf)
}

/// The readiness of the set's widgets to start — widget names → one answer for
/// each, in the same order. Live — the shared "all or nothing" path of the
/// "▶️ Start" button ([`crate::conky_safety::live_prepare_start`]: missing
/// programs from the repositories are installed without a question with one password window);
/// checks substitute their own.
pub type Ready<'a> = &'a dyn Fn(&[String]) -> Vec<crate::conky_safety::Readiness>;

/// The live readiness of the set's widgets.
pub fn live_ready(names: &[String]) -> Vec<crate::conky_safety::Readiness> {
    crate::conky_safety::live_prepare_start(names, crate::conky_safety::Retry::Set)
}

/// Apply the plan. `trusted` — the person allowed the commands of widgets from
/// `needs_trust` (the window); `false` — such widgets are skipped with an explanation
/// (the command line, a refusal in the window). The rest go through `ready` — the same
/// path as "▶️ Start": a widget without the needed programs does not start, and
/// the result says which one and why; the other widgets of the set start.
pub fn apply(io: &dyn LookIo, ctx: &Ctx, extra: &[String], p: &Plan, trusted: bool, ready: Ready, start: Starter) -> Applied {
    let mut lines: Vec<String> = Vec::new();
    let mut ok = true;
    if !p.stop.is_empty() {
        // A theme with a script — by any of its windows, not only by the main file.
        let pids = crate::conky_procs::pids_of_units(io, ctx, &p.stop);
        let title = titles(ctx, extra, &p.stop).join(", ");
        let (stopped, msg) = crate::conky_procs::stop_pids(io, &title, pids);
        ok &= stopped;
        lines.push(msg);
    }
    let mut started: Vec<String> = Vec::new();
    // First the permission for commands, then the programs: a widget that was
    // refused permission does not install packages.
    let mut queue: Vec<(&String, String)> = Vec::new();
    for conf in &p.start {
        let name = widget_name_of(ctx, extra, conf);
        if p.needs_trust.contains(&name) {
            if !trusted {
                let text = crate::conky_safety::declined_text(&name);
                if !lines.contains(&text) {
                    lines.push(text);
                }
                continue;
            }
            crate::conky_safety::remember(io, ctx, &name);
        }
        queue.push((conf, name));
    }
    let mut names: Vec<String> = Vec::new();
    for (_, name) in &queue {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let verdicts = if names.is_empty() { Vec::new() } else { ready(&names) };
    let mut told: Vec<String> = Vec::new();
    for (conf, name) in queue {
        let verdict = names.iter().position(|n| *n == name).and_then(|i| verdicts.get(i));
        let note = match verdict {
            Some(Err(why)) => {
                ok = false;
                if !told.contains(&name) {
                    lines.push(why.clone());
                    told.push(name);
                }
                continue;
            }
            Some(Ok(note)) => note.clone(),
            None => None,
        };
        let title = crate::conky_scan::widget_title(&name);
        let (good, msg) = start(&name, &title, conf);
        if good {
            started.push(conf.clone());
        }
        ok &= good;
        lines.push(msg);
        if let Some(n) = note.filter(|_| good && !told.contains(&name)) {
            lines.push(n);
            told.push(name);
        }
    }
    let mut st = load(io, ctx);
    st.applied = Some(p.set.name.clone());
    let running = crate::conky_procs::running_widget_confs(io, ctx);
    st.applied_widgets = p.set.widgets.iter().filter(|w| running.contains(w) || started.contains(w)).cloned().collect();
    store(io, ctx, &st);
    if p.start.is_empty() && p.stop.is_empty() {
        lines.push(trf("The widgets of “%1” are already running.", &[&p.set.name]));
    }
    Applied { ok, message: lines.join("\n"), wallpaper: p.set.wallpaper.clone() }
}

// ============================================================================
// The live machine
// ============================================================================

fn live() -> (crate::animations::SystemIo, Ctx, Vec<String>) {
    let ctx = Ctx::live();
    let extra = crate::conky_scan::extra_dirs_for(&ctx);
    (crate::animations::SystemIo, ctx, extra)
}

pub fn live_rows() -> Vec<SetRow> {
    let (io, ctx, extra) = live();
    rows(&io, &ctx, &extra, &live_exists)
}

/// The desktop wallpaper picture now (for "Save as a set").
pub fn live_current_wallpaper() -> Option<String> {
    let slot = crate::wallpaper_exec::live_slot();
    crate::wallpaper_exec::try_with_slot(&slot, |e| e.desktop_static_image()).flatten()
}

pub fn live_save(name: &str) -> (bool, String) {
    let (io, ctx, _) = live();
    save(&io, &ctx, name, live_current_wallpaper())
}

pub fn live_remove(name: &str) -> (bool, String) {
    let (io, ctx, _) = live();
    remove(&io, &ctx, name)
}

/// The plan and the question for the window. `Err` — there is no such set.
pub fn live_plan(name: &str) -> Result<(Plan, Option<(String, String)>), String> {
    let (io, ctx, extra) = live();
    let Some(set) = find(&io, &ctx, &extra, name, &live_exists) else {
        return Err(trf("There's no set “%1”", &[&name]));
    };
    // Path repair edits the widget files — before the permission fingerprint, not after.
    for w in &set.widgets {
        crate::de_apply::conky_prepare(&ctx, &widget_name_of(&ctx, &extra, w));
    }
    let p = plan(&io, &ctx, &extra, set);
    let q = apply_question(&io, &ctx, &p);
    Ok((p, q))
}

/// Apply the plan and set the set's wallpaper. `(succeeded, message)`.
pub fn live_apply(p: &Plan, trusted: bool) -> (bool, String) {
    let (io, ctx, extra) = live();
    // The Conky program itself is missing — it is installed without a question, as with "▶️
    // Start" (the protection is the password window); if it did not install, the set is not applied.
    if !p.start.is_empty() && !io.have("conky") {
        if crate::de_apply::conky_install_package(false, crate::de_apply::live_conky_package()).is_none() {
            return (false, crate::de_apply::conky_missing_refusal());
        }
        if let Err(why) = crate::de_apply::install_conky_live() {
            crate::log_warn!("conky", "Conky для набора «{}» не встал: {why}", p.set.name);
            // TRANSLATORS: %1 is the reason, %2 the name of a widget set.
            return (false, trf("Couldn't install Conky: %1. The widgets of the set “%2” weren't started.", &[&why, &p.set.name]));
        }
    }
    let done = apply(&io, &ctx, &extra, p, trusted, &live_ready, &live_start);
    let mut ok = done.ok;
    let mut lines = vec![done.message];
    if let Some(wp) = done.wallpaper {
        let slot = crate::wallpaper_exec::live_slot();
        let title = crate::conky_scan::widget_title(&wp);
        let r = crate::wallpaper_exec::with_slot(&slot, |e| e.apply(&wp, "static", &title));
        ok &= r.ok;
        lines.push(r.message);
    }
    (ok, lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"))
}

/// `rexon --conky-set <name>`: apply a set without a window. Widgets with
/// commands without permission are not started (there is nobody to ask).
pub fn cli_route(args: &[String], out: &mut dyn FnMut(&str)) -> i32 {
    let name = args
        .iter()
        .position(|a| a == CLI_FLAG)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .or_else(|| args.iter().find_map(|a| a.strip_prefix(&format!("{CLI_FLAG}=")).map(str::to_string)));
    let Some(name) = name.filter(|n| !n.trim().is_empty()) else {
        out(&tr("Name the set: rexon --conky-set \"<set name>\""));
        for r in live_rows() {
            out(&format!("  {}", r.name));
        }
        return 2;
    };
    match live_plan(&name) {
        Err(why) => {
            out(&why);
            1
        }
        Ok((p, _)) => {
            let (ok, msg) = live_apply(&p, false);
            for line in msg.lines() {
                out(line);
            }
            if ok { 0 } else { 1 }
        }
    }
}

/// The command line flag.
pub const CLI_FLAG: &str = "--conky-set";

// ============================================================================
// A set to an .rxn file and back
// ============================================================================
//
// WHY. A person built a desktop from widgets and wants the same on another machine
// (or to give it to a friend). "Save set to file…" puts into one `.rxn` the folders
// of the set's widgets (configs, Lua, pictures, fonts — everything in the widget
// folder), the set's wallpaper, the position and screen of each widget (the Rexon
// overlay), the list of needed programs and fonts. "Install from file…" on
// another machine lays it out back ([`crate::ext_manager`], the
// `conky_set` branch): each folder becomes its own widget `~/.config/conky/<folder>`, fonts
// — into `~/.local/share/fonts/rexon-conky-<id>`, the set — as a `.cmtheme` file in
// `~/.config/conky/<id>/` ("Widget sets" sees it right away), the position — as
// an overlay ([`after_set_install`]).
//
// The archive layout:
//   manifest.json            — type `theme-conky`, the section [`SET_MANIFEST_KEY`];
//   preview.png              — a widget snapshot from the preview cache (if any);
//   data/<folder>/…          — the folder of each widget;
//   data/<id>/<name>.cmtheme — the set itself; next to it `wallpaper.<ext>`.
//
// Foolproofing (personal data). Absolute home folder paths are rewritten
// to `~/…` (paths into the widget folder — straight to where the widget will land at
// the recipient's), so the user name does not travel with the paths. E-mail, passwords,
// weather service keys, coordinates, Wi-Fi network names, the user and
// machine names outside paths are not fixed automatically (an edit would be guessed wrong):
// saving stops with a "what and where" list ([`ExportError::Private`]),
// and the window asks whether to save anyway. Files outside widget folders are not
// taken at all — their list goes into the result. The permission for a widget's commands
// is NOT carried over: on another machine Rexon will ask again. Autostart
// is recorded, but installation does not turn it on (the rule of 04.10.2026: installation
// only puts the widget in place) — the install result says so.

/// The manifest section describing the set.
pub const SET_MANIFEST_KEY: &str = "conky_set";
/// The start of the set package id.
pub const SET_ID_PREFIX: &str = "conky-set-";
/// No more than this goes into one file (the widget folders together).
const EXPORT_MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Text files larger than this are neither rewritten nor checked.
const EXPORT_TEXT_MAX: u64 = 1024 * 1024;
/// A wallpaper picture larger than this is not put into the set.
const EXPORT_WALLPAPER_MAX: u64 = 64 * 1024 * 1024;

/// A set widget in the file.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExportedWidget {
    /// The folder in `data/` — also the folder under `~/.config/conky/` at the recipient's.
    pub folder: String,
    /// The settings file inside the folder.
    pub conf: String,
    /// Position, screen, background… — the person's choice on top of the author's file.
    #[serde(default, skip_serializing_if = "crate::conky_overlay::Overrides::is_empty")]
    pub settings: crate::conky_overlay::Overrides,
    /// "Start at login" was on — with this delay, seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autostart_delay: Option<u32>,
}

/// The [`SET_MANIFEST_KEY`] section of the manifest.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SetManifest {
    pub name: String,
    pub widgets: Vec<ExportedWidget>,
    /// The wallpaper file in `data/<id>/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// Programs the widgets' commands call (`cava`, `sensors`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<String>,
    /// Fonts from the widget settings (families).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fonts: Vec<String>,
}

/// What is known about the machine being saved from.
#[derive(Debug, Clone, Default)]
pub struct ExportEnv {
    pub user: String,
    pub host: String,
    /// `$XDG_STATE_HOME` (widget overlays); `None` — `~/.local/state`.
    pub state_home: Option<String>,
}

impl ExportEnv {
    pub fn live(ctx: &Ctx) -> Self {
        let user = std::env::var("USER")
            .ok()
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| crate::conky_scan::widget_title(&ctx.home));
        let host = std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim().to_string();
        let state_home = std::env::var("XDG_STATE_HOME").ok().filter(|s| !s.is_empty());
        ExportEnv { user, host, state_home }
    }
}

/// The saving result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exported {
    pub path: String,
    pub widgets: usize,
    /// Files outside widget folders that the widgets reference (`~/…`).
    pub left_out: Vec<String>,
    /// Widgets of the set that are not on disk.
    pub missing: usize,
    pub wallpaper: bool,
}

/// Why the set was not saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// Personal data was found: "file:line — what" lines. It can be saved only
    /// after the person's consent (`allow_private`).
    Private(Vec<String>),
    Failed(String),
}

/// The set package name: `conky-set-<name in Latin letters>`.
pub fn set_package_id(name: &str) -> String {
    let slug = crate::importers::slug60(name);
    format!("{SET_ID_PREFIX}{}", if slug.is_empty() { "set".to_string() } else { slug })
}

/// A valid file/folder name from `name`: without `/`, not hidden, not empty.
fn safe_component(name: &str, fallback: &str) -> String {
    let n: String = name.trim().trim_start_matches('.').replace('/', "-").chars().take(80).collect();
    let n = n.trim().to_string();
    if n.is_empty() || n == ".." { fallback.to_string() } else { n }
}

/// Fonts named in the widget settings: `font = 'Roboto:size=10'`,
/// `xftfont`, `${font Ubuntu Mono:bold:size=9}`.
pub fn config_fonts(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let fam = raw.split(':').next().unwrap_or("").trim().trim_matches(['"', '\'']).trim();
        if !fam.is_empty() && !fam.starts_with('$') && !out.iter().any(|f| f == fam) {
            out.push(fam.to_string());
        }
    };
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with("--") || s.starts_with('#') {
            continue;
        }
        // `font = '…',` (1.10) and `font …` / `xftfont …` (1.9).
        for key in ["font", "xftfont"] {
            let Some(rest) = s.strip_prefix(key) else { continue };
            if !rest.starts_with(|c: char| c.is_whitespace() || c == '=') {
                continue;
            }
            let rest = rest.trim_start();
            let value = rest.strip_prefix('=').map(str::trim_start).unwrap_or(rest);
            let v = value.trim_end_matches(',').trim().trim_matches(['"', '\'']);
            if !v.is_empty() && !v.contains("${") {
                push(v);
            }
        }
        let mut from = 0;
        while let Some(rel) = s[from..].find("${font ") {
            let start = from + rel + "${font ".len();
            let end = s[start..].find('}').map(|e| start + e).unwrap_or(s.len());
            push(&s[start..end]);
            from = end;
        }
    }
    out
}

/// Rewrite the paths of one text: the widget folder `src` (absolute and from
/// `~`) — to `~/.config/conky/<folder>`, the rest of the home folder — to `~`.
fn relocate_text(text: &str, home: &str, src: Option<&str>, folder: &str) -> String {
    let home = home.trim_end_matches('/');
    let target = format!("~/.config/conky/{folder}");
    let mut out = text.to_string();
    if let Some(src) = src.map(|s| s.trim_end_matches('/')).filter(|s| !s.is_empty()) {
        let mut forms = vec![src.to_string()];
        if let Some(rel) = src.strip_prefix(&format!("{home}/")) {
            for head in ["~/", "$HOME/", "${HOME}/"] {
                forms.push(format!("{head}{rel}"));
            }
        }
        for f in forms {
            if out.contains(&f) {
                out = crate::rxn_manifest::replace_path_prefix(&out, &f, &target);
            }
        }
    }
    if !home.is_empty() && out.contains(home) {
        out = crate::rxn_manifest::replace_path_prefix(&out, home, "~");
    }
    out
}

/// `~/…` paths in the text that lead to an existing file outside
/// `~/.config/conky/<folder>`: the widget relies on them, but they will not get
/// into the set.
fn outside_refs(text: &str, home: &str, folder: &str) -> Vec<String> {
    let inside = format!("~/.config/conky/{folder}/");
    let mut out: Vec<String> = Vec::new();
    for (pos, _) in text.match_indices("~/") {
        let prev_ok = text[..pos].chars().next_back().is_none_or(|c| !(c.is_alphanumeric() || "_./-".contains(c)));
        if !prev_ok {
            continue;
        }
        let rest = &text[pos..];
        let end = rest.find(|c: char| c.is_whitespace() || "'\"`}{)(][,;|<>&$\\=".contains(c)).unwrap_or(rest.len());
        let p = rest[..end].trim_end_matches(['.', ':']);
        if p.starts_with(&inside) || p.len() <= 2 {
            continue;
        }
        let abs = format!("{}/{}", home.trim_end_matches('/'), &p[2..]);
        if Path::new(&abs).is_file() && !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// Personal data in the text after translating paths: "file:line — what" lines.
fn private_lines(text: &str, file: &str, env: &ExportEnv) -> Vec<String> {
    use crate::rxn_manifest::{find_secrets, scrub_text, PrivateKind, ScrubContext};
    let mut out: Vec<String> = find_secrets(text)
        .into_iter()
        .map(|f| format!("{file}:{} — {}", f.line, f.kind.label()))
        .collect();
    let sc = ScrubContext { home: String::new(), user: env.user.clone(), host: env.host.clone() };
    for kind in scrub_text(text, &sc).replaced {
        let word = if kind == PrivateKind::UserName { &env.user } else { &env.host };
        let line = text.lines().position(|l| l.contains(word.as_str())).map(|i| i + 1).unwrap_or(1);
        out.push(format!("{file}:{line} — {}", kind.label()));
    }
    out
}

/// Copy the widget folder `src` to `dest`, rewriting paths in text
/// files. Not taken: links, hidden directories (`.git`), the Rexon owner
/// mark, files that are never published
/// ([`crate::rxn_manifest::never_export`]). `budget` — how many bytes are still allowed.
#[allow(clippy::too_many_arguments)]
fn copy_widget_tree(
    src: &Path,
    dest: &Path,
    rel: &str,
    home: &str,
    src_dir: &str,
    folder: &str,
    env: &ExportEnv,
    budget: &mut u64,
    private: &mut Vec<String>,
    left_out: &mut Vec<String>,
) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut entries: Vec<_> = std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_symlink()
            || name == crate::ext_manager::OWNER_MARK
            || name == crate::installed_scan::REXON_PREVIEW_NAME
            || crate::rxn_manifest::never_export(&child_rel)
        {
            continue;
        }
        if ft.is_dir() {
            if !name.starts_with('.') {
                copy_widget_tree(&e.path(), &dest.join(&name), &child_rel, home, src_dir, folder, env, budget, private, left_out)?;
            }
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        if size > *budget {
            return Err(trf("The widgets are too big for one file (more than %1 MB).", &[&(EXPORT_MAX_BYTES / 1024 / 1024)]));
        }
        *budget -= size;
        let bytes = std::fs::read(e.path()).map_err(|e| e.to_string())?;
        let text = (size <= EXPORT_TEXT_MAX && !bytes.contains(&0)).then(|| String::from_utf8(bytes.clone()).ok()).flatten();
        match text {
            Some(t) => {
                let fixed = relocate_text(&t, home, Some(src_dir), folder);
                private.extend(private_lines(&fixed, &format!("{folder}/{child_rel}"), env));
                for r in outside_refs(&fixed, home, folder) {
                    if !left_out.contains(&r) {
                        left_out.push(r);
                    }
                }
                std::fs::write(dest.join(&name), fixed).map_err(|e| e.to_string())?;
            }
            None => std::fs::write(dest.join(&name), &bytes).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

/// Where the widget of the file `conf` lies: `(the source folder, or None — a widget as a single
/// file, the folder name in the set, the settings file inside it)`.
fn widget_source(ctx: &Ctx, extra: &[String], conf: &str) -> (Option<String>, String, String) {
    let name = widget_name_of(ctx, extra, conf);
    let dir = if crate::conky_scan::is_found_name(&name) { name.clone() } else { crate::conky_scan::widget_path(ctx, &name) };
    if dir != conf && Path::new(&dir).is_dir() {
        if let Some(rel) = conf.strip_prefix(&format!("{}/", dir.trim_end_matches('/'))) {
            let folder = safe_component(&crate::conky_scan::widget_title(&dir), "conky");
            return (Some(dir), folder, rel.to_string());
        }
    }
    // A widget as a single file (`~/.conkyrc`, `~/.config/conky/conky.conf`):
    // only it itself is taken — the neighbors in its folder are foreign.
    let base = crate::conky_scan::widget_title(conf);
    let stem = base.trim_start_matches('.');
    let folder = safe_component(stem.split('.').next().unwrap_or(stem), "conky");
    let file = if base.starts_with('.') || base.is_empty() { "conky.conf".to_string() } else { base };
    (None, folder, file)
}

/// Save the set `set` to the file `dest` (`.rxn`). `allow_private` — the person
/// has seen the list of personal data and agrees.
pub fn export(
    io: &dyn LookIo,
    ctx: &Ctx,
    extra: &[String],
    env: &ExportEnv,
    set: &ConkySet,
    dest: &str,
    allow_private: bool,
) -> Result<Exported, ExportError> {
    let fail = |e: String| ExportError::Failed(e);
    let id = set_package_id(&set.name);
    let base = std::env::temp_dir();
    let stage = crate::ext_exec::mkdtemp(&base, "rexon_conky_set_").map_err(fail)?;
    let result = export_into(io, ctx, extra, env, set, &stage, &id, allow_private).and_then(|mut done| {
        crate::ext_utils::pack_rxn(&stage, Path::new(dest)).map_err(|e| fail(e.to_string()))?;
        done.path = dest.to_string();
        Ok(done)
    });
    let _ = std::fs::remove_dir_all(&stage);
    if result.is_err() && !matches!(result, Err(ExportError::Private(_))) {
        let _ = std::fs::remove_file(dest);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn export_into(
    io: &dyn LookIo,
    ctx: &Ctx,
    extra: &[String],
    env: &ExportEnv,
    set: &ConkySet,
    stage: &Path,
    id: &str,
    allow_private: bool,
) -> Result<Exported, ExportError> {
    let fail = |e: String| ExportError::Failed(e);
    let data = stage.join("data");
    let home = ctx.home.trim_end_matches('/').to_string();
    let mut budget = EXPORT_MAX_BYTES;
    let mut private: Vec<String> = Vec::new();
    let mut done = Exported::default();
    let mut manifest = SetManifest { name: set.name.clone(), ..Default::default() };
    // A widget folder — once, even if the set has several of its parts.
    let mut copied: Vec<(String, String)> = Vec::new();
    let mut confs_alive: Vec<String> = Vec::new();
    for conf in &set.widgets {
        if !Path::new(conf).is_file() {
            done.missing += 1;
            continue;
        }
        let (src, folder0, rel) = widget_source(ctx, extra, conf);
        let key = src.clone().unwrap_or_else(|| conf.clone());
        let folder = match copied.iter().find(|(k, _)| *k == key) {
            Some((_, f)) => f.clone(),
            None => {
                let mut folder = folder0.clone();
                let mut n = 2;
                while folder == id || folder == "fonts" || copied.iter().any(|(_, f)| *f == folder) {
                    folder = format!("{folder0}-{n}");
                    n += 1;
                }
                let dest = data.join(&folder);
                match &src {
                    Some(dir) => copy_widget_tree(
                        Path::new(dir),
                        &dest,
                        "",
                        &home,
                        dir,
                        &folder,
                        env,
                        &mut budget,
                        &mut private,
                        &mut done.left_out,
                    )
                    .map_err(fail)?,
                    None => {
                        let text = std::fs::read_to_string(conf).map_err(|e| fail(e.to_string()))?;
                        let fixed = relocate_text(&text, &home, None, &folder);
                        private.extend(private_lines(&fixed, &format!("{folder}/{rel}"), env));
                        for r in outside_refs(&fixed, &home, &folder) {
                            if !done.left_out.contains(&r) {
                                done.left_out.push(r);
                            }
                        }
                        std::fs::create_dir_all(&dest).map_err(|e| fail(e.to_string()))?;
                        std::fs::write(dest.join(&rel), fixed).map_err(|e| fail(e.to_string()))?;
                    }
                }
                copied.push((key, folder.clone()));
                done.widgets += 1;
                // Programs and the permission are computed by the widget tile name.
                let name = widget_name_of(ctx, extra, conf);
                for p in crate::conky_safety::widget_programs(io, ctx, &name) {
                    if !manifest.programs.contains(&p) {
                        manifest.programs.push(p);
                    }
                }
                folder
            }
        };
        let text = std::fs::read_to_string(conf).unwrap_or_default();
        for f in config_fonts(&text) {
            if !manifest.fonts.contains(&f) {
                manifest.fonts.push(f);
            }
        }
        let mut settings = crate::conky_overlay::load(io, &crate::conky_overlay::paths(ctx, env.state_home.as_deref(), conf));
        // Another machine has its own network card names.
        settings.net_ifaces.clear();
        let name = widget_name_of(ctx, extra, conf);
        let auto = crate::conky_autostart::widget_view(io, ctx, &name);
        manifest.widgets.push(ExportedWidget { folder, conf: rel, settings, autostart_delay: auto.on.then_some(auto.delay) });
        confs_alive.push(conf.clone());
    }
    if manifest.widgets.is_empty() && set.wallpaper.is_none() {
        return Err(fail(trf("None of the widgets of “%1” is on this computer — there's nothing to save.", &[&set.name])));
    }
    if !private.is_empty() && !allow_private {
        return Err(ExportError::Private(private));
    }

    // The set itself: a `.cmtheme` with paths to where the widgets will land at the recipient's.
    let set_dir = data.join(id);
    std::fs::create_dir_all(&set_dir).map_err(|e| fail(e.to_string()))?;
    let mut lines: Vec<String> = manifest.widgets.iter().map(|w| format!("~/.config/conky/{}/{}", w.folder, w.conf)).collect();
    if let Some(wp) = set.wallpaper.as_deref().filter(|w| is_image(w)) {
        let size = std::fs::metadata(wp).map(|m| m.len()).unwrap_or(u64::MAX);
        if size <= EXPORT_WALLPAPER_MAX && size <= budget {
            let ext = wp.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
            let file = format!("wallpaper.{ext}");
            std::fs::copy(wp, set_dir.join(&file)).map_err(|e| fail(e.to_string()))?;
            lines.push(format!("~/.config/conky/{id}/{file}"));
            manifest.wallpaper = Some(file);
            done.wallpaper = true;
        }
    }
    let cmtheme = format!("{}{CMTHEME_EXT}", safe_component(&set.name, "set"));
    std::fs::write(set_dir.join(cmtheme), lines.join("\n") + "\n").map_err(|e| fail(e.to_string()))?;

    // The cover is a widget snapshot from the preview cache, if it has already been taken.
    if let Some(shot) = crate::conky_preview::cached(ctx, &confs_alive) {
        let _ = std::fs::copy(shot, stage.join("preview.png"));
    }

    let titles: Vec<String> = manifest.widgets.iter().map(|w| w.folder.clone()).collect();
    // Home folder paths are always rewritten; if the person allowed the found personal data
    // to stay, we do not say "cleaned".
    let removed = vec![crate::rxn_manifest::PrivateKind::HomePath.label()];
    // TRANSLATORS: description of a saved Conky widget set; %1 lists the widgets.
    let description = trf("Conky widget set: %1", &[&titles.join(", ")]);
    let json = serde_json::json!({
        "format_version": 3,
        "id": id,
        "name": set.name,
        "version": "1.0",
        "author": "Rexon",
        "description": description,
        "type": crate::de_apply::CONKY_KIND,
        "tags": ["conky", "widget-set"],
        "de_support": ["any"],
        SET_MANIFEST_KEY: manifest,
        "privacy": {"exported_by": "rexon", "scrubbed": private.is_empty(), "removed": removed},
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| fail(e.to_string()))?;
    std::fs::write(stage.join("manifest.json"), text).map_err(|e| fail(e.to_string()))?;
    Ok(done)
}

/// Labels of the save window: `(title, filter)`.
pub fn export_dialog() -> (String, String) {
    (tr("Save the widget set to a file"), format!("{} (*.rxn)", tr("Rexon package")))
}

/// The suggested path of the set file in the folder `home`.
pub fn export_suggested(home: &str, name: &str) -> String {
    format!("{}/{}.rxn", home.trim_end_matches('/'), safe_component(name, "conky-set"))
}

/// The path from the save window — with the `.rxn` ending.
pub fn export_path(chosen: &str) -> String {
    if chosen.to_ascii_lowercase().ends_with(".rxn") { chosen.to_string() } else { format!("{chosen}.rxn") }
}

/// The question when personal data was found in the widgets: `(title, text)`.
pub fn export_private_question(name: &str, found: &[String]) -> (String, String) {
    const SHOWN: usize = 12;
    let mut list: Vec<String> = found.iter().take(SHOWN).map(|l| format!("• {l}")).collect();
    if found.len() > SHOWN {
        list.push(ntr("…and %n more", "…and %n more", (found.len() - SHOWN) as u64));
    }
    (
        trf("Save “%1” with personal data?", &[&name]),
        format!(
            "{}\n\n{}\n\n{}",
            tr("The widgets of this set contain what looks like personal data:"),
            list.join("\n"),
            tr("Anyone who gets the file will see it. Better remove it from the widget files first. Save anyway?")
        ),
    )
}

/// The result for the person.
pub fn export_message(done: &Exported) -> String {
    let mut lines = vec![trf("Saved: %1", &[&done.path])];
    if done.missing > 0 {
        lines.push(ntr(
            "%n widget of the set isn't on this computer and wasn't saved.",
            "%n widgets of the set aren't on this computer and weren't saved.",
            done.missing as u64,
        ));
    }
    if !done.left_out.is_empty() {
        // TRANSLATORS: %1 lists files like ~/scripts/weather.sh.
        lines.push(trf("Not included (outside the widget folders): %1", &[&done.left_out.join(", ")]));
    }
    lines.join("\n")
}

/// Save the set `name` to `dest` on the live machine.
pub fn live_export(name: &str, dest: &str, allow_private: bool) -> Result<Exported, ExportError> {
    let (io, ctx, extra) = live();
    let Some(set) = find(&io, &ctx, &extra, name, &live_exists) else {
        return Err(ExportError::Failed(trf("There's no set “%1”", &[&name])));
    };
    export(&io, &ctx, &extra, &ExportEnv::live(&ctx), &set, dest, allow_private)
}

/// The set from a package manifest (`None` — the package is not a set).
pub fn set_of_manifest(manifest: &serde_json::Map<String, serde_json::Value>) -> Option<SetManifest> {
    serde_json::from_value(manifest.get(SET_MANIFEST_KEY)?.clone()).ok()
}

/// After installing a set from a file: the position and other choices of each
/// widget — as an overlay (the person had none of their own), notes for the result
/// (autostart is not enabled, which fonts are missing). A widget whose folder was taken by
/// someone else (there is no `id` owner mark) is not touched.
pub fn after_set_install(io: &dyn LookIo, ctx: &Ctx, state_home: Option<&str>, id: &str, set: &SetManifest, have_font: &dyn Fn(&str) -> bool) -> Vec<String> {
    let root = crate::de_apply::conky_root(ctx);
    let mut notes = Vec::new();
    let mut auto = false;
    for w in &set.widgets {
        let dir = format!("{root}/{}", w.folder);
        let mark = io.read(&format!("{dir}/{}", crate::ext_manager::OWNER_MARK));
        if mark.as_deref().map(str::trim) != Some(id) {
            continue;
        }
        auto |= w.autostart_delay.is_some();
        if w.settings.is_empty() {
            continue;
        }
        let conf = format!("{dir}/{}", w.conf);
        let p = crate::conky_overlay::paths(ctx, state_home, &conf);
        if io.read(&p.settings).is_none() {
            io.write(&p.settings, &serde_json::to_string_pretty(&w.settings).unwrap_or_default());
        }
    }
    notes.push(trf("The set “%1” is in “Widget sets” — apply it there.", &[&set.name]));
    if auto {
        notes.push(tr("Some widgets of the set started at login on the other computer: turn on “Start at login” on their cards if you want that here too."));
    }
    let missing: Vec<&str> = set.fonts.iter().map(String::as_str).filter(|f| !have_font(f)).collect();
    if !missing.is_empty() {
        // TRANSLATORS: %1 lists font names.
        notes.push(trf("Fonts the widgets use that aren't installed: %1", &[&missing.join(", ")]));
    }
    notes
}

/// Whether the font is on the system (`fc-list`).
pub fn live_have_font(family: &str) -> bool {
    let out = std::process::Command::new("fc-list").arg(family).arg("family").output();
    out.map(|o| !o.stdout.is_empty()).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    /// A fake machine: files in memory, processes — `/proc/<pid>/cmdline`.
    #[derive(Default)]
    struct Io {
        files: RefCell<BTreeMap<String, String>>,
        runs: RefCell<Vec<Vec<String>>>,
    }

    impl Io {
        fn put(&self, p: &str, t: &str) {
            self.files.borrow_mut().insert(p.to_string(), t.to_string());
        }
        fn proc(&self, pid: u32, conf: &str) {
            self.put(&format!("/proc/{pid}/cmdline"), &format!("conky\0-c\0{conf}\0"));
            self.put(&format!("/proc/{pid}/comm"), "conky\n");
        }
    }

    impl LookIo for Io {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.put(path, text);
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{}/", path.trim_end_matches('/'));
            let mut out: Vec<String> = Vec::new();
            for k in self.files.borrow().keys() {
                if let Some(rest) = k.strip_prefix(&pre) {
                    let top = rest.split('/').next().unwrap_or("").to_string();
                    if !top.is_empty() && !out.contains(&top) {
                        out.push(top);
                    }
                }
            }
            out
        }
    }

    fn ctx() -> Ctx {
        Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: "mate".into() }
    }

    #[test]
    fn cli_flag_is_its_own_route() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(crate::app::route(&a(&[CLI_FLAG, "Работа"])), crate::app::Route::ConkySet);
        assert_eq!(crate::app::route(&a(&["--conky-set=Работа"])), crate::app::Route::ConkySet);
    }

    #[test]
    fn cmtheme_is_parsed_like_the_manager_writes_it() {
        let t = "\"~/.conky/Green Apple Desktop/conky_seamod\"\n~/.conky/Green Apple Desktop/Gotham\n\n\
                 ~/.conky/Green Apple Desktop/wallpaper.jpg\nwallpaper-scaling:stretched\n/opt/w/a~b\n";
        let c = parse_cmtheme(t, "/h");
        assert_eq!(
            c.widgets,
            ["/h/.conky/Green Apple Desktop/conky_seamod", "/h/.conky/Green Apple Desktop/Gotham", "/opt/w/a~b"]
        );
        assert_eq!(c.wallpaper.as_deref(), Some("/h/.conky/Green Apple Desktop/wallpaper.jpg"));
        assert_eq!(c.scaling.as_deref(), Some("stretched"));
    }

    #[test]
    fn old_conky_paths_lead_to_the_rexon_folder() {
        let c = ctx();
        let have = |p: &str| p == "/h/.config/conky/Green Apple Desktop/Gotham" || p == "/h/.conky/Seamod/x";
        assert_eq!(
            resolve(&c, "/h/.conky/Green Apple Desktop/Gotham", &have).as_deref(),
            Some("/h/.config/conky/Green Apple Desktop/Gotham")
        );
        assert_eq!(resolve(&c, "/h/.conky/Seamod/x", &have).as_deref(), Some("/h/.conky/Seamod/x"));
        assert_eq!(resolve(&c, "/h/.conky/Nope/x", &have), None);
    }

    #[test]
    fn sets_from_files_are_listed_and_own_sets_win() {
        let io = Io::default();
        let c = ctx();
        io.put("/h/.config/conky/Green/Green.cmtheme", "~/.conky/Green/Gotham\n~/.conky/Green/Gone\n~/.conky/Green/wallpaper.jpg\n");
        io.put("/h/.config/conky/Green/Gotham", "conky.text = [[x]]\n");
        let have = |p: &str| p == "/h/.config/conky/Green/Gotham" || p == "/h/.config/conky/Green/wallpaper.jpg";
        let r = rows(&io, &c, &[], &have);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Green");
        assert!(!r[0].own && !r[0].empty);
        assert!(r[0].summary.contains("Green"), "{:?}", r[0]);
        assert!(r[0].tooltip.contains('1'), "{:?}", r[0]);
        let set = find(&io, &c, &[], "Green", &have).expect("набор");
        assert_eq!(set.widgets, ["/h/.config/conky/Green/Gotham"]);
        assert_eq!(set.wallpaper.as_deref(), Some("/h/.config/conky/Green/wallpaper.jpg"));
    }

    #[test]
    fn save_needs_a_name_and_running_widgets() {
        let io = Io::default();
        let c = ctx();
        assert!(!save(&io, &c, "  ", None).0);
        assert!(!save(&io, &c, "Work", None).0);
        io.proc(10, "/h/.config/conky/Clock/clock.conf");
        let (ok, msg) = save(&io, &c, "Work", Some("/w.png".into()));
        assert!(ok, "{msg}");
        let st = load(&io, &c);
        assert_eq!(st.sets[0].widgets, ["/h/.config/conky/Clock/clock.conf"]);
        assert_eq!(st.applied.as_deref(), Some("Work"));
        assert!(save(&io, &c, "Work", None).0);
        assert_eq!(load(&io, &c).sets.len(), 1);
        assert!(remove(&io, &c, "Work").0);
        assert!(!remove(&io, &c, "Work").0);
        assert_eq!(load(&io, &c).applied, None);
    }

    /// Applying stops only the widgets of the previous set that are not in the new one;
    /// the person's foreign conky stays.
    #[test]
    fn apply_stops_only_the_previous_set() {
        let io = Io::default();
        let c = ctx();
        let clock = "/h/.config/conky/Clock/clock.conf";
        let cpu = "/h/.config/conky/Cpu/cpu.conf";
        let net = "/h/.config/conky/Net/net.conf";
        for f in [clock, cpu, net] {
            io.put(f, "conky.text = [[x]]\n");
        }
        io.proc(10, clock);
        io.proc(11, cpu);
        assert!(save(&io, &c, "Work", None).0);
        // Started by hand after the set was saved — not its widget.
        io.proc(12, "/home/other/my.conf");
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "Home".into(), widgets: vec![clock.into(), net.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "Home", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        assert_eq!(p.stop, [cpu]);
        assert_eq!(p.start, [net]);
        assert!(p.needs_trust.is_empty());
        let started = RefCell::new(Vec::<String>::new());
        let starter = |_: &str, _: &str, conf: &str| {
            started.borrow_mut().push(conf.to_string());
            (true, String::new())
        };
        let done = apply(&io, &c, &[], &p, false, &all_ready, &starter);
        assert_eq!(*started.borrow(), [net]);
        let kills: Vec<Vec<String>> = io.runs.borrow().iter().filter(|a| a[0] == "kill").cloned().collect();
        assert_eq!(kills, [vec!["kill".to_string(), "--".to_string(), "11".to_string()]], "{}", done.message);
        assert_eq!(load(&io, &c).applied.as_deref(), Some("Home"));
    }

    /// A widget with commands without permission: the window asks once,
    /// the command line skips it.
    #[test]
    fn untrusted_widget_is_asked_once_or_skipped() {
        let io = Io::default();
        let c = ctx();
        let w = "/h/.config/conky/Weather/w.conf";
        io.put(w, "conky.text = [[${execi 600 curl -s wttr.in}]]\n");
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "S".into(), widgets: vec![w.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "S", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        assert_eq!(p.needs_trust, ["Weather"]);
        let (title, text) = apply_question(&io, &c, &p).expect("вопрос");
        assert!(title.contains('S') && text.contains("curl"), "{title}\n{text}");
        let started = RefCell::new(0);
        let starter = |_: &str, _: &str, _: &str| {
            *started.borrow_mut() += 1;
            (true, String::new())
        };
        let asked = RefCell::new(Vec::<String>::new());
        let ready = |names: &[String]| {
            asked.borrow_mut().extend(names.iter().cloned());
            names.iter().map(|_| Ok(None)).collect()
        };
        let done = apply(&io, &c, &[], &p, false, &ready, &starter);
        assert_eq!(*started.borrow(), 0, "{}", done.message);
        assert!(asked.borrow().is_empty(), "без разрешения пакеты не ставятся");
        let done = apply(&io, &c, &[], &p, true, &ready, &starter);
        assert_eq!(*started.borrow(), 1, "{}", done.message);
        assert_eq!(*asked.borrow(), ["Weather"]);
    }

    fn all_ready(names: &[String]) -> Vec<crate::conky_safety::Readiness> {
        names.iter().map(|_| Ok(None)).collect()
    }

    /// "All or nothing", as with "▶️ Start": a set widget that lacks
    /// programs does not start, and the result says which one and why;
    /// the other widgets of the set start, the note about missing
    /// files is appended to the result. Readiness is asked once for
    /// the whole set.
    #[test]
    fn widget_without_programs_is_not_started_by_the_set() {
        let io = Io::default();
        let c = ctx();
        let jhalak = "/h/.config/conky/Jhalak/Jhalak.conf";
        let clock = "/h/.config/conky/Clock/clock.conf";
        for f in [jhalak, clock] {
            io.put(f, "conky.text = [[x]]\n");
        }
        let mut st = load(&io, &c);
        st.sets.push(ConkySet { name: "S".into(), widgets: vec![jhalak.into(), clock.into()], ..Default::default() });
        store(&io, &c, &st);
        let set = find(&io, &c, &[], "S", &|p| io.read(p).is_some()).unwrap();
        let p = plan(&io, &c, &[], set);
        let calls = RefCell::new(0);
        let ready = |names: &[String]| {
            *calls.borrow_mut() += 1;
            assert_eq!(names, ["Jhalak", "Clock"]);
            vec![Err("Jhalak: нет cava".to_string()), Ok(Some("нет ./a.lua".to_string()))]
        };
        let started = RefCell::new(Vec::<String>::new());
        let starter = |_: &str, _: &str, conf: &str| {
            started.borrow_mut().push(conf.to_string());
            (true, "запущен".to_string())
        };
        let done = apply(&io, &c, &[], &p, false, &ready, &starter);
        assert_eq!(*calls.borrow(), 1);
        assert_eq!(*started.borrow(), [clock], "{}", done.message);
        assert!(!done.ok);
        assert!(done.message.contains("Jhalak: нет cava") && done.message.contains("нет ./a.lua"), "{}", done.message);
        // A widget that was not started is not counted in the applied set.
        assert_eq!(load(&io, &c).applied_widgets, [clock]);
    }

    // ── A set to an .rxn file ──────────────────────────────────────────

    #[test]
    fn export_paths_go_where_the_widget_lands() {
        let t = "lua_load = '/home/al/.conky/Weather/w.lua'\n${execi 60 $HOME/.conky/Weather/a.sh}\n\
                 ${image /home/al/Pictures/x.png} ~/.conky/WeatherX/y\n";
        let out = relocate_text(t, "/home/al", Some("/home/al/.conky/Weather"), "Weather");
        assert_eq!(
            out,
            "lua_load = '~/.config/conky/Weather/w.lua'\n${execi 60 ~/.config/conky/Weather/a.sh}\n\
             ${image ~/Pictures/x.png} ~/.conky/WeatherX/y\n"
        );
        // `/home/al` does not touch `/home/alex`.
        assert_eq!(relocate_text("/home/alex/x", "/home/al", None, "W"), "/home/alex/x");
    }

    #[test]
    fn export_finds_fonts_of_both_syntaxes() {
        let t = "conky.config = {\n  font = 'Roboto Mono:size=10',\n  -- font = 'Old',\n}\n\
                 xftfont DejaVu Sans:size=8\nfonts_dir x\nconky.text = [[${font Ubuntu:bold:size=9}a${font}]]\n";
        assert_eq!(config_fonts(t), ["Roboto Mono", "DejaVu Sans", "Ubuntu"]);
    }

    #[test]
    fn export_names_are_safe() {
        assert_eq!(set_package_id("Мой стол"), "conky-set-moy-stol");
        assert_eq!(safe_component(".conkyrc", "x"), "conkyrc");
        assert_eq!(safe_component("a/b", "x"), "a-b");
        assert_eq!(safe_component("..", "x"), "x");
        assert_eq!(export_path("/h/set"), "/h/set.rxn");
        assert_eq!(export_path("/h/set.RXN"), "/h/set.RXN");
    }

    #[test]
    fn export_lists_personal_data() {
        let env = ExportEnv { user: "wehrwolf".into(), host: "machina".into(), state_home: None };
        let t = "${execi 600 curl 'https://api.openweathermap.org/x?appid=0123456789abcdef0123'}\n\
                 # mail me: a.b@example.org\n${exec echo machina}\n";
        let found = private_lines(t, "W/w.conf", &env);
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:1")), "{found:?}");
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:2")), "{found:?}");
        assert!(found.iter().any(|l| l.starts_with("W/w.conf:3")), "{found:?}");
        assert!(private_lines("conky.text = [[${time}]]\n", "W/w.conf", &env).is_empty());
    }

    const ROUNDTRIP_BASE_ENV: &str = "REXON_TEST_CONKY_SET_BASE";

    /// An end-to-end check: a set saved in one home is installed by the same
    /// "Install from file…" code into another (the substituted HOME of a child
    /// run) and gives the same layout; removal takes away its own.
    #[test]
    fn exported_set_installs_into_another_home() {
        let base = std::env::temp_dir().join(format!("rexon_test_conky_set_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("b");
        std::fs::create_dir_all(&home).unwrap();
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "conky_sets::tests::roundtrip_child", "--ignored", "--test-threads=1", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("LANG", "C.UTF-8")
            .env(ROUNDTRIP_BASE_ENV, &base)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&base);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "дочерний прогон не прошёл:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn put(p: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    /// All files of a tree: relative path → contents.
    fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    out.insert(p.strip_prefix(root).unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(root, root, &mut out);
        out
    }

    #[test]
    #[ignore = "run as a child process by exported_set_installs_into_another_home"]
    fn roundtrip_child() {
        let base = PathBuf::from(std::env::var_os(ROUNDTRIP_BASE_ENV).expect("только дочерний прогон"));
        let io = crate::animations::SystemIo;
        // ── Home A: saved from here ──
        let a = base.join("a");
        let a_s = a.to_string_lossy().into_owned();
        let ctx_a = Ctx { home: a_s.clone(), xdg_config_home: Some(format!("{a_s}/.config")), wayland: false, de: String::new() };
        let state_a = format!("{a_s}/.local/state");
        let clock = a.join(".config/conky/Clock");
        let clock_conf = clock.join("clock.conf");
        put(
            &clock_conf,
            format!(
                "conky.config = {{\n  alignment = 'top_right',\n  font = 'Roboto:size=10',\n  lua_load = '{a_s}/.config/conky/Clock/rings.lua',\n}}\n\
                 conky.text = [[${{execi 60 cava -h}}${{image {a_s}/.config/conky/Clock/bg.png}}]]\n"
            )
            .as_bytes(),
        );
        put(&clock.join("rings.lua"), b"function conky_main() end\n");
        put(&clock.join("bg.png"), b"\x89PNG\0\0binary");
        put(&clock.join("fonts/Fake.ttf"), b"ttf\0");
        put(&clock.join(".git/config"), b"secret history");
        put(&clock.join(crate::ext_manager::OWNER_MARK), b"other\n");
        let weather = a.join(".conky/Weather");
        let weather_conf = weather.join("weather.conf");
        put(&weather_conf, b"conky.config = { lua_load = '~/.conky/Weather/w.lua' }\nconky.text = [[${execi 600 ~/scripts/w.sh}]]\n");
        put(&weather.join("w.lua"), b"-- w\n");
        put(&a.join("scripts/w.sh"), b"#!/bin/sh\necho 1\n");
        let wp = a.join("Pictures/my.png");
        put(&wp, b"\x89PNG\r\n\x1a\nwp");
        let clock_s = clock_conf.to_string_lossy().into_owned();
        let weather_s = weather_conf.to_string_lossy().into_owned();
        // The clock's position and screen, the clock's autostart.
        let ov = crate::conky_overlay::Overrides { gap_x: Some(40), xinerama_head: Some(1), ..Default::default() };
        let pa = crate::conky_overlay::paths(&ctx_a, Some(&state_a), &clock_s);
        put(Path::new(&pa.settings), serde_json::to_string(&ov).unwrap().as_bytes());
        put(
            Path::new(&crate::de_apply::conky_autostart_path(&ctx_a, "Clock")),
            crate::ext_manager::conky_desktop_entry_with("Clock", &clock_s, None, None, 25, &[]).as_bytes(),
        );
        let env = ExportEnv { user: "nobody-special".into(), host: String::new(), state_home: Some(state_a.clone()) };
        let set = ConkySet {
            name: "Мой стол".into(),
            widgets: vec![clock_s.clone(), weather_s.clone(), format!("{a_s}/.config/conky/Gone/x.conf")],
            wallpaper: Some(wp.to_string_lossy().into_owned()),
            file: None,
        };
        let rxn = base.join("out/Мой стол.rxn");
        std::fs::create_dir_all(rxn.parent().unwrap()).unwrap();
        let rxn_s = rxn.to_string_lossy().into_owned();
        let done = export(&io, &ctx_a, &[], &env, &set, &rxn_s, false).expect("экспорт");
        assert_eq!((done.widgets, done.missing, done.wallpaper), (2, 1, true), "{done:?}");
        assert_eq!(done.left_out, ["~/scripts/w.sh"], "{done:?}");
        assert!(export_message(&done).contains(&rxn_s));

        // Personal data: a weather key — a refusal with a list, with consent — saved.
        let secret = base.join("a/.config/conky/Secret/s.conf");
        put(&secret, b"conky.text = [[${execi 600 curl 'https://x.org/?appid=0123456789abcdef0123'}]]\n");
        let mut leaky = set.clone();
        leaky.widgets.push(secret.to_string_lossy().into_owned());
        let leak_rxn = base.join("out/leak.rxn").to_string_lossy().into_owned();
        match export(&io, &ctx_a, &[], &env, &leaky, &leak_rxn, false) {
            Err(ExportError::Private(found)) => assert!(found.iter().any(|l| l.starts_with("Secret/s.conf:1")), "{found:?}"),
            other => panic!("личное не замечено: {other:?}"),
        }
        assert!(!Path::new(&leak_rxn).exists());
        assert!(export(&io, &ctx_a, &[], &env, &leaky, &leak_rxn, true).is_ok());

        // ── Home B: installed here (the process HOME) ──
        let b = PathBuf::from(std::env::var("HOME").unwrap());
        let b_s = b.to_string_lossy().into_owned();
        let ctx_b = Ctx { home: b_s.clone(), xdg_config_home: Some(format!("{b_s}/.config")), wayland: false, de: String::new() };
        let installed = crate::ext_exec::install_from_file(&rxn_s).expect("установка");
        eprintln!("установлено: {} {:?}", installed.name, installed.notes);
        let id = set_package_id("Мой стол");
        let conky = b.join(".config/conky");
        let got_clock = tree(&conky.join("Clock"));
        let got_weather = tree(&conky.join("Weather"));
        let names: Vec<&str> = got_clock.keys().map(String::as_str).collect();
        assert_eq!(names, [crate::ext_manager::OWNER_MARK, "bg.png", "clock.conf", "fonts/Fake.ttf", "rings.lua"], "{names:?}");
        assert_eq!(got_clock["bg.png"], b"\x89PNG\0\0binary");
        let conf_b = String::from_utf8(got_clock["clock.conf"].clone()).unwrap();
        assert!(conf_b.contains("lua_load = '~/.config/conky/Clock/rings.lua'"), "{conf_b}");
        assert!(conf_b.contains("${image ~/.config/conky/Clock/bg.png}"), "{conf_b}");
        let weather_b = String::from_utf8(got_weather["weather.conf"].clone()).unwrap();
        assert!(weather_b.contains("lua_load = '~/.config/conky/Weather/w.lua'"), "{weather_b}");
        assert!(got_weather.contains_key("w.lua"));
        // Not a single path of home A in any file.
        for (f, bytes) in got_clock.iter().chain(got_weather.iter()) {
            assert!(!String::from_utf8_lossy(bytes).contains(&a_s), "путь дома A в {f}");
        }
        // The set — as its own folder, without copies of widgets inside.
        let set_dir: Vec<String> = tree(&conky.join(&id)).into_keys().collect();
        assert_eq!(set_dir, ["wallpaper.png", "Мой стол.cmtheme"], "{set_dir:?}");
        assert!(!conky.join("Secret").exists() && !conky.join("fonts").exists());
        assert!(b.join(format!(".local/share/fonts/rexon-conky-{id}/Fake.ttf")).is_file());
        // "Widget sets" of home B see the set with home B paths.
        let sets = file_sets(&io, &ctx_b, &[], &live_exists);
        let (got, missing) = sets.iter().find(|(s, _)| s.name == "Мой стол").expect("набор не виден");
        assert_eq!(*missing, 0);
        assert_eq!(got.widgets, [format!("{b_s}/.config/conky/Clock/clock.conf"), format!("{b_s}/.config/conky/Weather/weather.conf")]);
        assert_eq!(got.wallpaper.as_deref(), Some(format!("{b_s}/.config/conky/{id}/wallpaper.png").as_str()));
        // The clock's position — as an overlay of home B; autostart is not enabled, but this is said.
        let pb = crate::conky_overlay::paths(&ctx_b, Some(&format!("{b_s}/.local/state")), &got.widgets[0]);
        assert_eq!(crate::conky_overlay::load(&io, &pb), ov);
        assert!(!Path::new(&crate::de_apply::conky_autostart_path(&ctx_b, "Clock")).exists());
        assert!(installed.notes.iter().any(|n| n.contains("Widget sets")), "{:?}", installed.notes);
        assert!(installed.notes.iter().any(|n| n.contains("Start at login")), "{:?}", installed.notes);

        // Removing the package takes away its own.
        crate::ext_exec::uninstall(&id).expect("удаление");
        assert!(!conky.join("Clock").exists() && !conky.join("Weather").exists() && !conky.join(&id).exists());
        assert!(!b.join(format!(".local/share/fonts/rexon-conky-{id}")).exists());
        assert!(!Path::new(&pb.settings).exists());
    }
}
