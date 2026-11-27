//! "Start at login" for a Conky widget: delay, environment, compositors.
//!
//! WHY. A widget started in the very first second after login ends up under
//! the panel or is drawn without a background: the panel and the wallpaper
//! are not up yet (a complaint from users of conky manager programs, which
//! default to a 20 s delay). A person with two desktop environments wants the
//! widget in only one of them. And bare compositors (sway, Hyprland without
//! uwsm, river, labwc, Wayfire, niri without `niri-session`) do not run
//! `~/.config/autostart` entries at all — the checkbox was on, yet the widget
//! did not appear at login.
//!
//! How it works:
//! * the delay is a `sleep N` in the launch line itself
//!   ([`crate::ext_manager::conky_desktop_entry_with`]): not every
//!   environment honors the `X-GNOME-Autostart-Delay` key;
//! * "only in this environment" is `OnlyShowIn=` with names from
//!   `XDG_CURRENT_DESKTOP` (for a compositor, its names);
//! * compositors use Rexon's common path for its own programs
//!   ([`crate::compositor_autostart`]): a launch line in a managed block in
//!   the compositor's native config, plus `NotShowIn=` in the entry, so the
//!   widget does not start twice where the compositor runs both. A systemd
//!   unit with `PartOf=graphical-session.target` would add nothing here: on
//!   bare sway and Hyprland that target is never reached (it is brought up by
//!   uwsm, `niri-session`, sway-systemd — and there XDG entries run anyway).
//!   The line is written to a config when "Start at login" is clicked, and
//!   ONLY to the config of the current session's compositor (Wehrwolfmann's
//!   decision, 23.10.2026): no new line appears in the configs of other
//!   installed compositors, and in full desktop environments (KDE, MATE,
//!   Cinnamon…) in none of them. A line left there by earlier enabling is
//!   only refreshed together with the entry (otherwise it would call an
//!   overlay that has been removed). Disabling and removing take the line out
//!   everywhere. If it could not be placed, the person is told what to insert
//!   by hand and where.
//!
//! The entry's settings are stored in the entry itself ([`options_of`]):
//! rewriting the entry (switching to the overlay, updating the old form) does
//! not lose them.

use crate::compositor_autostart::{self as ca, Compositor, CurrentSession, SessionEnv};
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};

/// The entry key holding the delay in seconds.
pub const DELAY_KEY: &str = "X-Rexon-Conky-Delay";

/// The delay of a new autostart: the panel and the wallpaper have time to come up.
pub const DEFAULT_DELAY: u32 = 10;

/// The largest delay that can be chosen.
pub const MAX_DELAY: u32 = 600;

/// Autostart entry settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Options {
    /// Seconds before start.
    pub delay: u32,
    /// `OnlyShowIn=`; empty means in all environments.
    pub only_in: Vec<String>,
}

/// The settings stored in entry `text`. The delay comes from [`DELAY_KEY`], and
/// for an entry without that key, from the `sleep N &&` of the launch line.
pub fn options_of(text: &str) -> Options {
    let value = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
    let delay = value(&format!("{DELAY_KEY}="))
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            let exec = value("Exec=")?;
            let at = exec.find("sleep ")? + 6;
            exec[at..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
        })
        .unwrap_or(0);
    let only_in = value("OnlyShowIn=")
        .map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    Options { delay: delay.min(MAX_DELAY), only_in }
}

/// The block tag in compositor configs, derived from part of the autostart
/// file name (`rexon-conky-<key>.desktop`). The tag goes into a comment and
/// into a Wayfire key, so characters outside `[A-Za-z0-9_-]` are replaced by
/// a fingerprint.
pub fn compositor_id(key: &str) -> String {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return format!("conky-{key}");
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("conky-{h:016x}")
}

/// The launch line for a compositor config. The entry's script (`$0`, `$1`)
/// does not work there: sway and Hyprland expand `$` themselves. Here the
/// paths are written into the script in double quotes; a path with `"`, `$`,
/// `` ` `` or `\` yields no such line — the compositor refusal then shows what
/// to insert by hand.
pub fn compositor_argv(conf: &str, launch: Option<&str>, delay: u32) -> Option<Vec<String>> {
    let bad = |s: &str| s.contains(['"', '$', '`', '\\', '\n']);
    let dir = crate::de_apply::conky_work_dir(conf);
    let file = launch.unwrap_or(conf);
    if bad(dir) || bad(file) {
        return None;
    }
    let sleep = if delay > 0 { format!("sleep {delay} && ") } else { String::new() };
    Some(vec!["sh".into(), "-c".into(), format!("{sleep}cd \"{dir}\" && exec conky -c \"{file}\"")])
}

/// Environment names for "only here": `XDG_CURRENT_DESKTOP`, or for a
/// compositor without it, its names. Empty means the session was not recognized.
pub fn current_names(env: &SessionEnv) -> Vec<String> {
    if !env.current_desktop.is_empty() {
        return env.current_desktop.clone();
    }
    match ca::current_session(env) {
        CurrentSession::Compositor(c) => c.desktop_names().iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// The environment for editing compositor configs. Only for this process's
/// home directory: a foreign tree (comparison runs) has no compositors, and
/// configs are not touched.
pub fn session_env(ctx: &Ctx) -> SessionEnv {
    let own = std::env::var("HOME").ok().is_some_and(|h| h.trim_end_matches('/') == ctx.home.trim_end_matches('/'));
    if own {
        return SessionEnv::live();
    }
    SessionEnv { home: ctx.home.clone(), config_home: ctx.config(""), ..Default::default() }
}

/// The result of building the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// The final entry text.
    pub desktop: String,
    /// Why the widget will not start at login in the CURRENT session (a compositor
    /// whose config the line could not be written to); `None` means it will start.
    pub refusal: Option<String>,
    /// The compositor configs where the line was written.
    pub configs: Vec<String>,
}

/// Builds the autostart entry `key` and places (or removes) the compositor
/// lines. Writing it to disk is up to [`Built::desktop`]. `xwayland` is the
/// launch file via XWayland ([`crate::conky_overlay::autostart_xwayland`]).
pub fn build(
    env: &SessionEnv,
    key: &str,
    title: &str,
    conf: &str,
    launch: Option<&str>,
    xwayland: Option<&str>,
    opts: &Options,
) -> Built {
    let entry = crate::ext_manager::conky_desktop_entry_with(title, conf, launch, xwayland, opts.delay, &opts.only_in);
    build_entry(env, key, entry, compositor_argv(conf, launch, opts.delay), opts)
}

/// [`build`] for a theme that starts with its own `start.sh`
/// (`conky_theme::ScriptLaunch`): at login the same script runs, from the
/// theme folder and with its process tag. `main_conf` is the main settings
/// file (it decides whether the widget needs X11).
pub fn build_script(
    env: &SessionEnv,
    key: &str,
    title: &str,
    plan: &crate::conky_theme::ScriptLaunch,
    main_conf: Option<&str>,
    opts: &Options,
) -> Built {
    use crate::installer::desktop_exec_arg;
    let session = crate::conky_overlay::xwayland_shell(main_conf.is_some_and(crate::conky_overlay::widget_needs_x11), None);
    let args = [&plan.script, &plan.dir, &plan.argv[0]].map(|a| desktop_exec_arg(a)).to_vec();
    let entry = crate::ext_manager::conky_desktop_entry_exec(
        title,
        &plan.dir,
        &format!("{session}{}", crate::conky_theme::AUTOSTART_SCRIPT),
        &args,
        opts.delay,
        &opts.only_in,
    );
    build_entry(env, key, entry, crate::conky_theme::compositor_argv(plan, opts.delay), opts)
}

/// The part shared by [`build`] and [`build_script`]: compositor lines for the
/// finished entry `entry` and the line `argv` (`None` means the path could not
/// be put into it).
fn build_entry(env: &SessionEnv, key: &str, entry: String, argv: Option<Vec<String>>, opts: &Options) -> Built {
    let id = compositor_id(key);
    // The line is placed only in the config of the CURRENT session's compositor
    // ([`ca::Scope::Current`]); in the configs of the others, only a line already
    // there from earlier enabling is refreshed, so that it does not call the old one.
    // "Only here": the line has no place in compositors outside the list.
    let wanted = |c: Compositor| opts.only_in.is_empty() || c.desktop_names().iter().any(|n| opts.only_in.iter().any(|o| o == n));
    for c in ca::compositors_with_block(env, &id) {
        if !wanted(c) {
            ca::remove_from(env, c, &id);
        }
    }
    let current = match ca::current_session(env) {
        CurrentSession::Compositor(c) => Some(c),
        _ => None,
    };
    let Some(argv) = argv else {
        ca::remove_everywhere(env, &id);
        let refusal = current.filter(|&c| wanted(c)).map(|c| {
            trf(
                // TRANSLATORS: %1 is a window manager name (Sway, Hyprland…).
                "%1 doesn't start programs from the common startup list, and the path of this widget has characters \
                 its settings file can't hold.",
                &[&c.display_name()],
            )
        });
        return Built { desktop: entry, refusal, configs: Vec::new() };
    };
    // The current session does not need the line — do not add a new one.
    let scope = if current.is_some_and(|c| !wanted(c)) { ca::Scope::Present } else { ca::Scope::Current };
    let report = ca::sync_command_in(env, &id, Some(&argv), scope);
    let names = report.not_show_in();
    let configs = report.outcomes.iter().filter(|o| o.result.is_ok()).map(|o| o.path.clone()).collect();
    Built { desktop: ca::with_not_show_in(&entry, &names), refusal: report.current_refusal(env), configs }
}

/// Removes the compositor lines of entry `key`.
pub fn remove(env: &SessionEnv, key: &str) {
    ca::remove_everywhere(env, &compositor_id(key));
}

/// Part of the file name from the entry path (`…/rexon-conky-<key>.desktop`).
pub fn key_of_path(path: &str) -> Option<&str> {
    path.rsplit('/').next()?.strip_prefix("rexon-conky-")?.strip_suffix(".desktop")
}

/// Labels of the autostart settings for the card.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Texts {
    pub delay: String,
    pub delay_tooltip: String,
    pub only_here: String,
    pub only_here_tooltip: String,
}

pub fn texts(names: &[String]) -> Texts {
    let here = names.first().cloned().unwrap_or_default();
    Texts {
        delay: tr("Wait before starting, seconds"),
        delay_tooltip: tr("Gives the panel and the wallpaper time to appear, so the widget doesn't end up under the panel."),
        // TRANSLATORS: %1 is a desktop name (KDE, MATE, sway…).
        only_here: trf("Only in %1", &[&here]),
        only_here_tooltip: tr("Don't start this widget when you log in to other desktops."),
    }
}

/// What the card shows under the "Start at login" switch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct View {
    pub on: bool,
    pub delay: u32,
    pub only_here: bool,
    /// Whether it can be restricted to an environment (the session is recognized).
    pub can_limit: bool,
    pub max_delay: u32,
    pub t: Texts,
}

pub fn view(text: Option<&str>, env: &SessionEnv) -> View {
    let names = current_names(env);
    let opts = text.map(options_of).unwrap_or(Options { delay: DEFAULT_DELAY, only_in: Vec::new() });
    View {
        on: text.is_some(),
        delay: opts.delay,
        only_here: !opts.only_in.is_empty(),
        can_limit: !names.is_empty(),
        max_delay: MAX_DELAY,
        t: texts(&names),
    }
}

/// Autostart settings of widget `id` for the card.
pub fn widget_view(io: &dyn crate::animations::LookIo, ctx: &Ctx, id: &str) -> View {
    let text = io.read(&crate::de_apply::conky_autostart_path(ctx, id));
    view(text.as_deref(), &session_env(ctx))
}

/// Changes the delay and "only here" of an enabled autostart.
/// Returns `(succeeded, message)`.
pub fn set_widget(io: &dyn crate::animations::LookIo, ctx: &Ctx, id: &str, delay: u32, only_here: bool) -> (bool, String) {
    let env = session_env(ctx);
    let only_in = if only_here { current_names(&env) } else { Vec::new() };
    let opts = Options { delay: delay.min(MAX_DELAY), only_in };
    crate::de_apply::set_conky_autostart_options(io, ctx, id, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_survive_in_the_entry() {
        let e = crate::ext_manager::conky_desktop_entry_with("w", "/h/w/c.conf", None, None, 25, &["MATE".into()]);
        assert_eq!(options_of(&e), Options { delay: 25, only_in: vec!["MATE".into()] });
        let plain = crate::ext_manager::conky_desktop_entry("w", "/h/w/c.conf");
        assert_eq!(options_of(&plain), Options::default());
        // An entry without the key: the delay comes from the launch line.
        assert_eq!(options_of("Exec=sh -c \"sleep 7 && cd x\" a\n").delay, 7);
    }

    #[test]
    fn compositor_ids_are_safe() {
        assert_eq!(compositor_id("Jhalak"), "conky-Jhalak");
        let odd = compositor_id("Мой виджет");
        assert!(odd.starts_with("conky-") && odd[6..].chars().all(|c| c.is_ascii_hexdigit()), "{odd}");
        assert_eq!(key_of_path("/h/.config/autostart/rexon-conky-a b.desktop"), Some("a b"));
    }

    #[test]
    fn compositor_line_has_no_dollar() {
        let a = compositor_argv("/h/w x/c.conf", Some("/s/o.rexon.conf"), 10).unwrap();
        assert_eq!(a[2], "sleep 10 && cd \"/h/w x\" && exec conky -c \"/s/o.rexon.conf\"");
        assert!(compositor_argv("/h/$w/c.conf", None, 0).is_none());
    }
}
