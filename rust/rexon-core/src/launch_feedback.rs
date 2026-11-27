//! "Launch feedback" in the "Settings" tab: what shows that a program is
//! starting — the cursor and the taskbar button — and after how many seconds
//! the animation stops.
//!
//! The reference is the "Launch Feedback" dialog of the "Cursors" module of KDE
//! 6.8 System Settings (`kcm_cursortheme`, `LaunchFeedbackDialog.qml`). The keys
//! come from its `launchfeedbacksettings.kcfg`, file `klaunchrc`:
//! * `[FeedbackStyle] BusyCursor` (default `true`) — whether the cursor shows
//!   a launch at all;
//! * `[BusyCursorSettings] Blinking` (`false`), `Bouncing` (`true`) — the style:
//!   "none" = `BusyCursor=false`, "static" = both `false`, "blinking",
//!   "bouncing". The KWin effect `startupfeedback` reads them in the same order:
//!   off → bouncing → blinking → static;
//! * `[FeedbackStyle] TaskbarButton` (`true`) — the animation on the taskbar
//!   (read by the Plasma task model);
//! * `[BusyCursorSettings] Timeout` and `[TaskbarButtonSettings] Timeout`
//!   (both 5, 1…60 s) — the KDE module writes one number into both keys, and so
//!   do we.
//!
//! Both KWin and the panel watch `klaunchrc` themselves (`KConfigWatcher`), so
//! nothing needs to be reloaded after `kwriteconfig6 --notify`. A key that
//! matches its default value is removed rather than written.
//!
//! Outside KDE none of the window managers Rexon works with has such a setting:
//! Mutter, Muffin, Marco and xfwm4 track launches via the startup-notification
//! protocol, but none of them has a gsettings key or an xfconf property for its
//! style or duration (checked against the schemas on the system and the xfwm4
//! properties); Hyprland, niri, sway and labwc have no such feedback at all. The
//! section says so plainly ([`note`]).

use serde::Serialize;

use crate::animations::{kconfig_value, kwrite, LookCtx, LookIo, LookStep};
use crate::i18n::{ntr, tr, trc, trf};

const FILE: &str = "klaunchrc";
const STYLE: &str = "FeedbackStyle";
const CURSOR: &str = "BusyCursorSettings";
const TASKBAR: &str = "TaskbarButtonSettings";

/// Default time and limits — the same as the KDE module's spin box.
pub const DEFAULT_TIMEOUT: u32 = 5;
pub const MIN_TIMEOUT: u32 = 1;
pub const MAX_TIMEOUT: u32 = 60;

/// Steps for choosing the time (seconds).
const TIMEOUT_STEPS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 30, 45, 60];

/// Cursor styles — the same as the KDE module's radio buttons.
pub const CURSOR_STYLES: &[&str] = &["none", "static", "blinking", "bouncing"];

/// Label of a cursor style (with a qualifier: "Static" already exists in the
/// catalog with a different meaning).
fn cursor_label(id: &str) -> String {
    match id {
        "none" => trc("launch feedback pointer", "No feedback"),
        "static" => trc("launch feedback pointer", "Static"),
        "blinking" => trc("launch feedback pointer", "Blinking"),
        _ => trc("launch feedback pointer", "Bouncing"),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Opt {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaunchFeedbackState {
    pub de: String,
    /// The setting exists (KDE only).
    pub supported: bool,
    pub cursor: String,
    pub cursor_options: Vec<Opt>,
    pub taskbar: bool,
    /// Seconds (as a string — the option id).
    pub timeout: String,
    pub timeout_options: Vec<Opt>,
    /// The time matters: the cursor or the panel shows the launch.
    pub timeout_enabled: bool,
    /// Why the setting is unavailable (outside KDE).
    pub note: String,
}

/// The setting as it currently stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feedback {
    pub cursor: String,
    pub taskbar: bool,
    pub timeout: u32,
}

fn kbool(io: &dyn LookIo, ctx: &LookCtx, group: &str, key: &str, default: bool) -> bool {
    match kconfig_value(io, ctx, FILE, group, key) {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "on" | "yes"),
        None => default,
    }
}

fn kint(io: &dyn LookIo, ctx: &LookCtx, group: &str, key: &str) -> u32 {
    kconfig_value(io, ctx, FILE, group, key)
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(DEFAULT_TIMEOUT)
        .clamp(MIN_TIMEOUT, MAX_TIMEOUT)
}

/// Read `klaunchrc` the same way the KWin effect reads it.
pub fn read(io: &dyn LookIo, ctx: &LookCtx) -> Feedback {
    let busy = kbool(io, ctx, STYLE, "BusyCursor", true);
    let blinking = kbool(io, ctx, CURSOR, "Blinking", false);
    let bouncing = kbool(io, ctx, CURSOR, "Bouncing", true);
    let cursor = if !busy {
        "none"
    } else if bouncing {
        "bouncing"
    } else if blinking {
        "blinking"
    } else {
        "static"
    };
    Feedback { cursor: cursor.to_string(), taskbar: kbool(io, ctx, STYLE, "TaskbarButton", true), timeout: kint(io, ctx, CURSOR, "Timeout") }
}

pub fn supported(de: &str) -> bool {
    de == "kde"
}

pub fn note(de: &str) -> String {
    if supported(de) {
        return String::new();
    }
    trf(
        "%1 has no launch feedback settings: its window manager doesn't let you choose how the pointer or the task bar shows that an application is starting.",
        &[&crate::animations::de_label(de)],
    )
}

fn timeout_label(s: u32) -> String {
    ntr("%n second", "%n seconds", s as u64)
}

pub fn read_state(io: &dyn LookIo, ctx: &LookCtx) -> LaunchFeedbackState {
    let de = ctx.de.as_str();
    let ok = supported(de);
    let f = read(io, ctx);
    let mut steps: Vec<u32> = TIMEOUT_STEPS.to_vec();
    if !steps.contains(&f.timeout) {
        steps.push(f.timeout);
        steps.sort_unstable();
    }
    LaunchFeedbackState {
        de: ctx.de.clone(),
        supported: ok,
        cursor: f.cursor.clone(),
        cursor_options: CURSOR_STYLES.iter().map(|id| Opt { id: id.to_string(), label: cursor_label(id) }).collect(),
        taskbar: f.taskbar,
        timeout: f.timeout.to_string(),
        timeout_options: steps.into_iter().map(|s| Opt { id: s.to_string(), label: timeout_label(s) }).collect(),
        timeout_enabled: f.cursor != "none" || f.taskbar,
        note: note(de),
    }
}

/// Write a key: a default value is removed.
fn put(steps: &mut Vec<LookStep>, io: &dyn LookIo, ctx: &LookCtx, group: &str, key: &str, value: &str, default: &str, bool_type: bool) {
    let current = kconfig_value(io, ctx, FILE, group, key);
    if value == default {
        if current.is_some() {
            steps.push(kwrite(FILE, group, key, None, false));
        }
    } else if current.as_deref() != Some(value) {
        steps.push(kwrite(FILE, group, key, Some(value), bool_type));
    }
}

fn b(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

/// The whole write plan (the way the KDE module saves it).
pub fn plan(io: &dyn LookIo, ctx: &LookCtx, want: &Feedback) -> Vec<LookStep> {
    if !supported(&ctx.de) || !CURSOR_STYLES.contains(&want.cursor.as_str()) {
        return Vec::new();
    }
    let timeout = want.timeout.clamp(MIN_TIMEOUT, MAX_TIMEOUT).to_string();
    let default_timeout = DEFAULT_TIMEOUT.to_string();
    let mut steps = Vec::new();
    put(&mut steps, io, ctx, STYLE, "BusyCursor", b(want.cursor != "none"), "true", true);
    put(&mut steps, io, ctx, CURSOR, "Blinking", b(want.cursor == "blinking"), "false", true);
    put(&mut steps, io, ctx, CURSOR, "Bouncing", b(want.cursor == "bouncing"), "true", true);
    put(&mut steps, io, ctx, STYLE, "TaskbarButton", b(want.taskbar), "true", true);
    put(&mut steps, io, ctx, CURSOR, "Timeout", &timeout, &default_timeout, false);
    put(&mut steps, io, ctx, TASKBAR, "Timeout", &timeout, &default_timeout, false);
    steps
}

fn finish(io: &dyn LookIo, ctx: &LookCtx, steps: Vec<LookStep>) -> String {
    if steps.is_empty() {
        return String::new();
    }
    match crate::animations::run_steps(io, ctx, &steps) {
        Ok(()) => tr("Launch feedback applied."),
        Err(e) => e,
    }
}

pub fn apply_cursor(io: &dyn LookIo, ctx: &LookCtx, style: &str) -> String {
    let mut want = read(io, ctx);
    want.cursor = style.to_string();
    finish(io, ctx, plan(io, ctx, &want))
}

pub fn apply_taskbar(io: &dyn LookIo, ctx: &LookCtx, on: bool) -> String {
    let mut want = read(io, ctx);
    want.taskbar = on;
    finish(io, ctx, plan(io, ctx, &want))
}

pub fn apply_timeout(io: &dyn LookIo, ctx: &LookCtx, seconds: u32) -> String {
    let mut want = read(io, ctx);
    want.timeout = seconds;
    finish(io, ctx, plan(io, ctx, &want))
}

pub fn apply_defaults(io: &dyn LookIo, ctx: &LookCtx) -> String {
    let want = Feedback { cursor: "bouncing".into(), taskbar: true, timeout: DEFAULT_TIMEOUT };
    finish(io, ctx, plan(io, ctx, &want))
}
