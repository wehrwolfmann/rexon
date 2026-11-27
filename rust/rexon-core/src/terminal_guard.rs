//! "Don't let themes spoil the terminal colors" — a guard after any change of
//! looks that changes colors (GTK theme, global theme, color scheme,
//! light/dark, «Distro Looks», accent).
//!
//! Why: some terminals by default draw text and background with GTK theme
//! colors (the theme's input field: `theme_text_color` on `theme_base_color`).
//! A dark theme with an unlucky pair — and the terminal becomes unreadable: this
//! is how on 06.11.2026, after WhiteSur-Dark-nord in MATE, the terminal showed
//! dark gray text on black. The guard pins OWN colors on such a profile: it
//! turns off "colors from the theme" and writes a contrasting pair (dark theme —
//! light text on dark, light theme — dark on light; WCAG contrast ≥ 4.5).
//!
//! Who takes the theme colors (checked against sources and schemas 06.11.2026):
//!
//! | terminal         | theme colors           | where                                                    |
//! |------------------|------------------------|----------------------------------------------------------|
//! | mate-terminal    | yes, by default        | dconf `/org/mate/terminal/profiles/<p>/use-theme-colors` |
//! | gnome-terminal   | yes, by default        | dconf `/org/gnome/terminal/legacy/profiles:/:<uuid>/…`   |
//! | Tilix            | yes, by default        | dconf `/com/gexperts/Tilix/profiles/<uuid>/…`            |
//! | xfce4-terminal   | only if enabled        | xfconf `xfce4-terminal /color-use-theme` (default false) |
//! | Terminator       | only if enabled        | `~/.config/terminator/config` `use_theme_colors`         |
//! | Console, Ptyxis  | no: own pair of palettes for light/dark (libadwaita)              |
//! | Konsole          | no: profile scheme; "Sync profile with system theme" — profiles   |
//! | deepin-terminal  | no: own Light/Dark schemes by DDE mode                            |
//! | cosmic-term      | no: COSMIC Dark/Light                                              |
//! | LXTerminal       | no: own `fgcolor`/`bgcolor`                                        |
//! | QTerminal        | no: own `colorScheme`                                              |
//! | kitty, Alacritty, foot, WezTerm, xterm | no: own config only                         |
//!
//! Decision rules ([`decide`]):
//! * the profile takes theme colors → "theme colors" are turned off; the
//!   profile's own colors stay if they are set and readable, otherwise our pair
//!   is written;
//! * a profile with its own colors (theme colors off) is not touched;
//! * the exception is OUR pair pinned earlier for a different lightness: it is
//!   switched to the pair of the new theme (otherwise after a light theme the
//!   terminal would have stayed dark).
//!
//! Writes go through `dconf write`/`xfconf-query` commands and a file write of
//! the shared plan ([`crate::install_plan::exec_undoable`]): the shared undo
//! takes the previous values, and they go into the section's record of the
//! shared journal — the section's «Restore as it was» also restores the
//! terminal colors. The switch is the [`SETTING_KEY`] key in `settings.json`
//! (no key — enabled).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::ext_manager::ExtStep;
use crate::i18n::trf;
use crate::install_plan::{Ctx, Step};

/// Switch key in `settings.json`. Without it the guard is enabled.
pub const SETTING_KEY: &str = "terminal_colors_guard";

/// Color pair: text, background.
pub type Pair = ((u8, u8, u8), (u8, u8, u8));

/// Our pair for a dark theme: Tango light gray on almost black.
pub const DARK_PAIR: Pair = ((0xD3, 0xD7, 0xCF), (0x1E, 0x1E, 0x1E));
/// Our pair for a light theme: Tango dark gray on white.
pub const LIGHT_PAIR: Pair = ((0x2E, 0x34, 0x36), (0xFF, 0xFF, 0xFF));

/// Readability threshold for normal text (WCAG 2.x, level AA).
pub const MIN_CONTRAST: f64 = 4.5;

/// The pair for the theme's lightness.
pub fn pair_for(dark: bool) -> Pair {
    if dark { DARK_PAIR } else { LIGHT_PAIR }
}

/// `#RRGGBB`.
pub fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// A color from a terminal record: `#RGB`, `#RRGGBB`, `#RRRRGGGGBBBB` (as
/// mate-terminal writes it), `rgb(r,g,b)`; GVariant and config quotes are
/// stripped.
pub fn parse_color(raw: &str) -> Option<(u8, u8, u8)> {
    let s = raw.trim().trim_matches(|c| c == '\'' || c == '"').trim();
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<u8> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        return (parts.len() == 3).then(|| (parts[0], parts[1], parts[2]));
    }
    let digits = s.strip_prefix('#')?;
    if digits.len() == 12 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        let ch = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
        return Some((ch(0)?, ch(4)?, ch(8)?));
    }
    crate::ui_base::parse_hex(s)
}

/// WCAG 2.x relative luminance (sRGB).
fn luminance(c: (u8, u8, u8)) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG 2.x contrast: from 1 to 21.
pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

// ============================================================================
// Decision per profile
// ============================================================================

/// What is known about a terminal profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// Whether the profile takes theme colors (with the terminal's default if the key is not set).
    pub use_theme: bool,
    /// The profile's own colors, if set.
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
}

/// What to write to the profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// Turn off "colors from the theme".
    pub theme_off: bool,
    /// Write the pair (text, background); `None` — the profile's own colors stay.
    pub colors: Option<Pair>,
}

/// Decision for a profile under a theme of lightness `dark`; `None` — don't touch.
pub fn decide(p: &Profile, dark: bool) -> Option<Pin> {
    let want = pair_for(dark);
    let own = p.fg.zip(p.bg);
    let ours = own.filter(|pair| *pair == DARK_PAIR || *pair == LIGHT_PAIR);
    if p.use_theme {
        let colors = match own {
            // The person's own colors are readable — they stay.
            Some(pair) if ours.is_none() && contrast(pair.0, pair.1) >= MIN_CONTRAST => None,
            Some(pair) if pair == want => None,
            _ => Some(want),
        };
        return Some(Pin { theme_off: true, colors });
    }
    // Own colors: we touch only our pair pinned for a different lightness.
    match ours {
        Some(pair) if pair != want => Some(Pin { theme_off: false, colors: Some(want) }),
        _ => None,
    }
}

// ============================================================================
// Terminals on dconf
// ============================================================================

/// A terminal whose profiles live in dconf.
#[derive(Debug, Clone, Copy)]
pub struct DconfTerminal {
    /// Executable (if present, the terminal is installed).
    pub binary: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Root of `dconf dump`.
    pub root: &'static str,
    /// Dump section with the profile list and the list key.
    pub list_section: &'static str,
    pub list_key: &'static str,
    /// Default profile list (key not set).
    pub default_list: &'static [&'static str],
    /// Profile section in the dump and the profile directory under the root: prefixes to the name.
    pub profile_prefix: &'static str,
    /// The schema's default of `use-theme-colors`.
    pub theme_default: bool,
    /// The "bold in the same color" key — written together with the pair.
    pub bold_same_key: Option<&'static str>,
}

/// Schemas: `org.mate.terminal.gschema.xml` (mate-terminal 1.28),
/// `org.gnome.Terminal.gschema.xml` (gnome-terminal 3.60),
/// `com.gexperts.Tilix.gschema.xml` (Tilix, master branch).
pub const DCONF_TERMINALS: &[DconfTerminal] = &[
    DconfTerminal {
        binary: "mate-terminal",
        name: "MATE Terminal",
        root: "/org/mate/terminal/",
        list_section: "global",
        list_key: "profile-list",
        default_list: &["default"],
        profile_prefix: "profiles/",
        theme_default: true,
        bold_same_key: Some("bold-color-same-as-fg"),
    },
    DconfTerminal {
        binary: "gnome-terminal",
        name: "GNOME Terminal",
        root: "/org/gnome/terminal/legacy/profiles:/",
        list_section: "/",
        list_key: "list",
        default_list: &["b1dcc9dd-5262-4d8d-a863-c897e6d979b9"],
        profile_prefix: ":",
        theme_default: true,
        bold_same_key: Some("bold-color-same-as-fg"),
    },
    DconfTerminal {
        binary: "tilix",
        name: "Tilix",
        root: "/com/gexperts/Tilix/profiles/",
        list_section: "/",
        list_key: "list",
        default_list: &["2b7c4080-0ddd-46c5-8f23-563fd3ba789d"],
        profile_prefix: "",
        theme_default: true,
        bold_same_key: None,
    },
];

/// Parsing `dconf dump`: section → key → value (GVariant text).
pub fn parse_dump(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let s = line.trim();
        if s.starts_with('[') && s.ends_with(']') {
            section = s[1..s.len() - 1].to_string();
            out.entry(section.clone()).or_default();
        } else if let Some((k, v)) = s.split_once('=') {
            out.entry(section.clone()).or_default().insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

/// GVariant string list: `['a', 'b']`, `@as []`.
fn parse_str_list(raw: &str) -> Vec<String> {
    let s = raw.trim().trim_start_matches("@as").trim();
    let inner = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or("");
    inner
        .split(',')
        .map(|p| p.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().trim_matches(|c| c == '\'' || c == '"').to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}

fn run_step(parts: &[&str]) -> Step {
    Step::from(ExtStep::Run { argv: argv(parts), timeout_secs: Some(10) })
}

/// Pinning steps for terminal `t` from its dump `dump`.
pub fn dconf_steps(t: &DconfTerminal, dump: &str, dark: bool) -> Vec<Step> {
    let tree = parse_dump(dump);
    let list = tree
        .get(t.list_section)
        .and_then(|s| s.get(t.list_key))
        .map(|raw| parse_str_list(raw))
        .unwrap_or_else(|| t.default_list.iter().map(|s| s.to_string()).collect());
    let mut steps = Vec::new();
    for name in list {
        let section = format!("{}{name}", t.profile_prefix);
        let keys = tree.get(&section);
        let get = |k: &str| keys.and_then(|m| m.get(k));
        let profile = Profile {
            use_theme: get("use-theme-colors").and_then(|v| parse_bool(v)).unwrap_or(t.theme_default),
            fg: get("foreground-color").and_then(|v| parse_color(v)),
            bg: get("background-color").and_then(|v| parse_color(v)),
        };
        let Some(pin) = decide(&profile, dark) else { continue };
        let base = format!("{}{section}/", t.root);
        if let Some((fg, bg)) = pin.colors {
            steps.push(run_step(&["dconf", "write", &format!("{base}foreground-color"), &format!("'{}'", hex(fg))]));
            steps.push(run_step(&["dconf", "write", &format!("{base}background-color"), &format!("'{}'", hex(bg))]));
            if let Some(bold) = t.bold_same_key {
                steps.push(run_step(&["dconf", "write", &format!("{base}{bold}"), "true"]));
            }
        }
        if pin.theme_off {
            steps.push(run_step(&["dconf", "write", &format!("{base}use-theme-colors"), "false"]));
        }
    }
    steps
}

// ============================================================================
// xfce4-terminal (xfconf)
// ============================================================================

/// The xfce4-terminal channel.
pub const XFCE_CHANNEL: &str = "xfce4-terminal";

/// Steps for xfce4-terminal: `use_theme` — `/color-use-theme` (`None` — not
/// set, default false), `fg`/`bg` — `/color-foreground`, `/color-background`.
pub fn xfce_steps(use_theme: Option<&str>, fg: Option<&str>, bg: Option<&str>, dark: bool) -> Vec<Step> {
    let profile = Profile {
        use_theme: use_theme.and_then(parse_bool).unwrap_or(false),
        fg: fg.and_then(parse_color),
        bg: bg.and_then(parse_color),
    };
    let Some(pin) = decide(&profile, dark) else { return Vec::new() };
    let set = |prop: &str, ty: &str, value: &str| {
        run_step(&["xfconf-query", "-c", XFCE_CHANNEL, "-p", prop, "-n", "-t", ty, "-s", value])
    };
    let mut steps = Vec::new();
    if let Some((f, b)) = pin.colors {
        steps.push(set("/color-foreground", "string", &hex(f)));
        steps.push(set("/color-background", "string", &hex(b)));
    }
    if pin.theme_off {
        steps.push(set("/color-use-theme", "bool", "false"));
    }
    steps
}

// ============================================================================
// Terminator (its own ConfigObj config)
// ============================================================================

/// New text of `~/.config/terminator/config`: profiles `[profiles] [[name]]`
/// per the [`decide`] decision (the default of `use_theme_colors` is False,
/// `terminatorlib/config.py`). `None` — nothing to change.
pub fn terminator_config(text: &str, dark: bool) -> Option<String> {
    // Profiles: (name, line range of the body).
    let lines: Vec<&str> = text.lines().collect();
    let depth = |s: &str| {
        let t = s.trim();
        if !t.starts_with('[') || !t.ends_with(']') {
            return None;
        }
        let n = t.chars().take_while(|c| *c == '[').count();
        Some((n, t.trim_matches(|c| c == '[' || c == ']').trim().to_string()))
    };
    let mut top = String::new();
    let mut profiles: Vec<(usize, usize)> = Vec::new(); // (header, end of body)
    for (i, line) in lines.iter().enumerate() {
        if let Some((n, name)) = depth(line) {
            if let Some(last) = profiles.last_mut().filter(|p| p.1 == usize::MAX) {
                last.1 = i;
            }
            if n == 1 {
                top = name;
            } else if n == 2 && top == "profiles" {
                profiles.push((i, usize::MAX));
            }
        }
    }
    if let Some(last) = profiles.last_mut().filter(|p| p.1 == usize::MAX) {
        last.1 = lines.len();
    }
    let key_of = |line: &str| line.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()));
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    let mut changed = false;
    // From the end: insertions don't shift profiles not yet processed.
    for &(head, end) in profiles.iter().rev() {
        let body = head + 1..end;
        let find = |k: &str| body.clone().find(|&i| key_of(lines[i]).is_some_and(|(kk, _)| kk == k));
        let value = |k: &str| find(k).and_then(|i| key_of(lines[i]).map(|(_, v)| v));
        let profile = Profile {
            use_theme: value("use_theme_colors").and_then(|v| parse_bool(&v)).unwrap_or(false),
            fg: value("foreground_color").and_then(|v| parse_color(&v)),
            bg: value("background_color").and_then(|v| parse_color(&v)),
        };
        let Some(pin) = decide(&profile, dark) else { continue };
        let indent = body
            .clone()
            .find(|&i| key_of(lines[i]).is_some())
            .map(|i| lines[i][..lines[i].len() - lines[i].trim_start().len()].to_string())
            .unwrap_or_else(|| "    ".to_string());
        // First in-place replacements (line numbers are still valid), then the
        // missing keys — right under the profile header, in order.
        let mut wanted: Vec<(&str, String)> = Vec::new();
        if pin.theme_off {
            wanted.push(("use_theme_colors", "False".into()));
        }
        if let Some((f, b)) = pin.colors {
            wanted.push(("foreground_color", format!("\"{}\"", hex(f).to_lowercase())));
            wanted.push(("background_color", format!("\"{}\"", hex(b).to_lowercase())));
        }
        let mut missing = Vec::new();
        for (k, v) in wanted {
            let line = format!("{indent}{k} = {v}");
            match find(k) {
                Some(i) => out[i] = line,
                None => missing.push(line),
            }
        }
        for line in missing.into_iter().rev() {
            out.insert(head + 1, line);
        }
        changed = true;
    }
    if !changed {
        return None;
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    Some(s)
}

// ============================================================================
// Theme lightness
// ============================================================================

/// Whether the active GTK theme is dark: the `theme` name, the person's own CSS
/// (`gtk-3.0/gtk.css` and the `colors.css` it imports — this is how Plasma
/// gives the scheme colors to GTK programs) and the theme's own `gtk.css`. The
/// own CSS is loaded after the theme and overrides it, so it goes first
/// ([`crate::themes::gtk_theme_is_dark`] takes the first definition).
pub fn theme_is_dark(theme: &str, user_css: Option<&str>, theme_css: Option<&str>) -> bool {
    let css: String = [user_css, theme_css].into_iter().flatten().collect::<Vec<_>>().join("\n");
    crate::themes::gtk_theme_is_dark(theme, (!css.is_empty()).then_some(css.as_str()))
}

// ============================================================================
// Execution
// ============================================================================

/// Where to look for configs and themes.
#[derive(Debug, Clone)]
pub struct Env {
    /// Rexon desktop (`mate`, `kde`, `xfce`…).
    pub de: String,
    pub home: String,
    /// `$XDG_CONFIG_HOME` or `~/.config`.
    pub config: String,
    /// `$XDG_DATA_HOME` or `~/.local/share`.
    pub data: String,
}

impl Env {
    /// Live machine: paths from the environment.
    pub fn live(de: &str) -> Self {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let home = env("HOME").unwrap_or_default();
        Env {
            de: de.to_string(),
            config: crate::utils::config_home(env("XDG_CONFIG_HOME").as_deref(), &home),
            data: crate::utils::data_home(env("XDG_DATA_HOME").as_deref(), &home),
            home,
        }
    }

    /// Rexon config directory (`settings.json` and the shared journal are there too).
    pub fn rexon_dir(&self) -> PathBuf {
        Path::new(&self.config).join("rexon")
    }
}

/// Whether the guard is enabled: the [`SETTING_KEY`] key in `settings.json` of the Rexon directory.
pub fn enabled(rexon_dir: &Path) -> bool {
    crate::settings::load_settings(&rexon_dir.join("settings.json"))
        .get(SETTING_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

fn read_out(ctx: &Ctx, parts: &[&str]) -> Option<String> {
    let r = (ctx.run)(&argv(parts), Some(10));
    (r.returncode == Some(0)).then(|| crate::utils::decode_text(&r.stdout))
}

/// Name of the active GTK theme on desktop `env.de`.
fn live_theme_name(env: &Env, ctx: &Ctx) -> String {
    use crate::themes::Schemas;
    let from_ini = || {
        std::fs::read_to_string(format!("{}/gtk-3.0/settings.ini", env.config))
            .ok()
            .and_then(|t| crate::de_apply::ini_value(&t, "Settings", "gtk-theme-name"))
            .unwrap_or_default()
    };
    let name = match crate::themes::resolve_schemas(&env.de) {
        Schemas::Gsettings(_) => crate::themes::resolve_schemas(&env.de)
            .role("gtk")
            .and_then(|(schema, key)| read_out(ctx, &["gsettings", "get", schema, key]))
            .map(|o| crate::themes::parse_icon_theme_output(&o)),
        _ if env.de == "xfce" => read_out(ctx, &["xfconf-query", "-c", "xsettings", "-p", "/Net/ThemeName"])
            .map(|o| o.trim().to_string()),
        _ => None,
    };
    name.filter(|n| !n.is_empty()).unwrap_or_else(from_ini)
}

/// Whether the GTK theme in effect now (after applying) is dark.
pub fn live_dark(env: &Env, ctx: &Ctx) -> bool {
    let theme = live_theme_name(env, ctx);
    let gtk3 = format!("{}/gtk-3.0", env.config);
    let user = std::fs::read_to_string(format!("{gtk3}/gtk.css")).ok().map(|css| {
        match std::fs::read_to_string(format!("{gtk3}/colors.css")) {
            Ok(colors) if css.contains("colors.css") => format!("{colors}\n{css}"),
            _ => css,
        }
    });
    let theme_css = crate::gtk_sanitizer::theme_dirs("gtk-theme", &env.home, &env.data)
        .into_iter()
        .map(|base| format!("{base}/{theme}"))
        .find(|dir| Path::new(dir).is_dir())
        .and_then(|dir| {
            ["gtk-3.0", "gtk-4.0"].iter().find_map(|v| std::fs::read_to_string(format!("{dir}/{v}/gtk.css")).ok())
        });
    theme_is_dark(&theme, user.as_deref(), theme_css.as_deref())
}

/// Pinning steps for all installed terminals and their names.
/// If a terminal's state could not be read, it is skipped: writing blindly is
/// not allowed (it could overwrite the person's own colors).
pub fn plan(env: &Env, ctx: &Ctx, dark: bool) -> (Vec<Step>, Vec<&'static str>) {
    let mut steps = Vec::new();
    let mut names = Vec::new();
    let mut take = |name: &'static str, s: Vec<Step>, steps: &mut Vec<Step>| {
        if !s.is_empty() {
            names.push(name);
            steps.extend(s);
        }
    };
    for t in DCONF_TERMINALS {
        if !(ctx.have)(t.binary) {
            continue;
        }
        if let Some(dump) = read_out(ctx, &["dconf", "dump", t.root]) {
            take(t.name, dconf_steps(t, &dump, dark), &mut steps);
        }
    }
    if (ctx.have)("xfce4-terminal") && (ctx.have)("xfconf-query") {
        let get = |p: &str| read_out(ctx, &["xfconf-query", "-c", XFCE_CHANNEL, "-p", p]).map(|o| o.trim().to_string());
        let (u, f, b) = (get("/color-use-theme"), get("/color-foreground"), get("/color-background"));
        take("Xfce Terminal", xfce_steps(u.as_deref(), f.as_deref(), b.as_deref(), dark), &mut steps);
    }
    let path = format!("{}/terminator/config", env.config);
    if (ctx.have)("terminator") {
        if let Some(text) = std::fs::read_to_string(&path).ok().and_then(|t| terminator_config(&t, dark)) {
            take("Terminator", vec![Step::from(ExtStep::WriteFile { path: path.clone(), content: text })], &mut steps);
        }
    }
    (steps, names)
}

/// Pin the terminal colors after applying the looks of section `section`
/// (theme `theme`, journal record `item`) and write the previous values into the
/// section's record of the shared journal — «Restore as it was» restores them
/// too. Disabled, nothing to pin or it failed — an empty list; otherwise a line
/// for the person.
pub fn guard(env: &Env, ctx: &Ctx, section: &str, theme: &str, item: Option<String>) -> Option<String> {
    use crate::install_journal as journal;
    let rexon = env.rexon_dir();
    if !enabled(&rexon) {
        return None;
    }
    let dark = live_dark(env, ctx);
    let (steps, names) = plan(env, ctx, dark);
    if steps.is_empty() {
        return None;
    }
    let mut undo = crate::install_plan::Undo::new().keeping_values();
    if let Err(f) = crate::install_plan::exec_undoable(&steps, &mut Vec::new(), ctx, &mut undo) {
        undo.finish(false);
        crate::log_warn!("appearance", "цвета терминалов не закреплены: {}", crate::install_plan::failure_text(&f));
        return None;
    }
    // The section record already exists (the click itself left it) — we don't lose
    // its link to the card and the name.
    let prev = journal::view_at(&rexon).applied.get(section).cloned();
    let item = item.or_else(|| prev.as_ref().and_then(|a| a.item.clone()));
    let record_theme = prev.as_ref().map_or_else(|| theme.to_string(), |a| a.theme.clone());
    let dir = journal::applied_snapshot_dir(&rexon, section);
    let first = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.parse::<usize>().ok()))
                .max()
                .map_or(1, |n| n + 1)
        })
        .unwrap_or(1);
    let values = undo.take_values();
    let touched = undo.keep_into(&dir, first);
    let text = |p: &Path| p.to_string_lossy().into_owned();
    let applied = journal::Applied {
        section: section.to_string(),
        theme: record_theme,
        item,
        applied_at: Some(crate::qt_app::Timestamp::now_local().map(|n| crate::qt_app::Timestamp { microsecond: 0, ..n }.isoformat()).unwrap_or_default()),
        snapshot: text(&dir),
        files: touched
            .iter()
            .map(|t| journal::AppliedFile { path: text(&t.path), before: t.before.as_deref().map(text), dir: t.dir })
            .collect(),
    };
    let values = values.into_iter().map(journal::AppliedValue::from).collect();
    if let Err(e) = journal::record_applied_values_at(&rexon, applied, values) {
        crate::log_warn!("appearance", "закреплённые цвета терминалов не записаны в общий журнал ({}) — {e}", rexon.display());
    }
    let list = names.join(", ");
    crate::log_info!(
        "appearance",
        "цвета терминалов закреплены после «{theme}» (раздел {section}, тема {}): {list}",
        if dark { "тёмная" } else { "светлая" }
    );
    Some(trf(
        // TRANSLATORS: %1 is a list of terminal apps (MATE Terminal, GNOME Terminal…).
        "Terminal colors were set so the text stays readable with the new theme: %1.",
        &[&list],
    ))
}

/// [`guard`] on the live machine of desktop `de`.
pub fn guard_live(de: &str, section: &str, theme: &str, item: Option<String>) -> Option<String> {
    let ctx = Ctx::live(true, false, None, false);
    guard(&Env::live(de), &ctx, section, theme, item)
}

// ============================================================================
// Checks
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[test]
    fn our_pairs_are_readable() {
        for pair in [DARK_PAIR, LIGHT_PAIR] {
            assert!(contrast(pair.0, pair.1) >= MIN_CONTRAST, "{pair:?}");
        }
        assert!(luminance(DARK_PAIR.1) < luminance(DARK_PAIR.0));
        assert!(luminance(LIGHT_PAIR.1) > luminance(LIGHT_PAIR.0));
        // The 06.11 case: dark gray on black — unreadable.
        assert!(contrast((0x4C, 0x56, 0x6A), (0, 0, 0)) < MIN_CONTRAST);
    }

    #[test]
    fn colors_parse_in_every_form() {
        assert_eq!(parse_color("'#D3D3D7D7CFCF'"), Some((0xD3, 0xD7, 0xCF)));
        assert_eq!(parse_color("\"#ffffff\""), Some((255, 255, 255)));
        assert_eq!(parse_color("#abc"), Some((0xAA, 0xBB, 0xCC)));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some((1, 2, 3)));
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn decide_rules() {
        let theme = Profile { use_theme: true, ..Default::default() };
        assert_eq!(decide(&theme, true), Some(Pin { theme_off: true, colors: Some(DARK_PAIR) }));
        assert_eq!(decide(&theme, false), Some(Pin { theme_off: true, colors: Some(LIGHT_PAIR) }));
        // Own readable colors under "theme colors" — they stay, the theme is turned off.
        let own = Profile { use_theme: true, fg: Some((0, 0, 0)), bg: Some((0xFF, 0xFF, 0xDD)) };
        assert_eq!(decide(&own, true), Some(Pin { theme_off: true, colors: None }));
        // Own unreadable ones under "theme colors" — our pair.
        let bad = Profile { use_theme: true, fg: Some((0x33, 0x33, 0x33)), bg: Some((0, 0, 0)) };
        assert_eq!(decide(&bad, true), Some(Pin { theme_off: true, colors: Some(DARK_PAIR) }));
        // The person's own colors (theme off) — never touch.
        let mine = Profile { use_theme: false, fg: Some((0x33, 0x33, 0x33)), bg: Some((0, 0, 0)) };
        assert_eq!(decide(&mine, true), None);
        assert_eq!(decide(&Profile::default(), true), None);
        // Our pair is switched to the new lightness, the person's own is not.
        let pinned = Profile { use_theme: false, fg: Some(DARK_PAIR.0), bg: Some(DARK_PAIR.1) };
        assert_eq!(decide(&pinned, true), None);
        assert_eq!(decide(&pinned, false), Some(Pin { theme_off: false, colors: Some(LIGHT_PAIR) }));
    }

    fn cmds(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .map(|s| match s {
                Step::File(ExtStep::Run { argv, .. }) => argv.join(" "),
                Step::File(ExtStep::WriteFile { path, .. }) => format!("write {path}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// The owner's case: an empty mate-terminal dump (the `default` profile takes
    /// theme colors by the schema default) → the theme is turned off, the pair is
    /// written.
    #[test]
    fn mate_default_profile_is_pinned() {
        let mate = &DCONF_TERMINALS[0];
        assert_eq!(
            cmds(&dconf_steps(mate, "", true)),
            [
                "dconf write /org/mate/terminal/profiles/default/foreground-color '#D3D7CF'",
                "dconf write /org/mate/terminal/profiles/default/background-color '#1E1E1E'",
                "dconf write /org/mate/terminal/profiles/default/bold-color-same-as-fg true",
                "dconf write /org/mate/terminal/profiles/default/use-theme-colors false",
            ]
        );
        // Own colors (as the owner fixed them) — not touched.
        let own = "[profiles/default]\nuse-theme-colors=false\nforeground-color='#D3D3D7D7CFCF'\nbackground-color='#1E1E1E1E1E1E'\n";
        assert!(!dconf_steps(mate, own, false).is_empty(), "наша пара под светлую тему переводится");
        let mine = "[profiles/default]\nuse-theme-colors=false\nforeground-color='#00FF00'\nbackground-color='#000000'\n";
        assert!(dconf_steps(mate, mine, true).is_empty());
        assert!(dconf_steps(mate, mine, false).is_empty());
        // Profile list — from [global].
        let two = "[global]\nprofile-list=['default', 'Work']\n\n[profiles/default]\nuse-theme-colors=false\n";
        assert_eq!(
            cmds(&dconf_steps(mate, two, true)).last().map(String::as_str),
            Some("dconf write /org/mate/terminal/profiles/Work/use-theme-colors false")
        );
    }

    #[test]
    fn gnome_and_tilix_profiles() {
        let gnome = &DCONF_TERMINALS[1];
        let steps = cmds(&dconf_steps(gnome, "", false));
        assert_eq!(
            steps.last().map(String::as_str),
            Some("dconf write /org/gnome/terminal/legacy/profiles:/:b1dcc9dd-5262-4d8d-a863-c897e6d979b9/use-theme-colors false")
        );
        assert!(steps[0].ends_with("foreground-color '#2E3436'"), "{steps:?}");
        let listed = "[/]\nlist=['abc']\ndefault='abc'\n\n[:abc]\nuse-theme-colors=false\n";
        assert!(dconf_steps(gnome, listed, true).is_empty());
        let tilix = &DCONF_TERMINALS[2];
        let steps = cmds(&dconf_steps(tilix, "", true));
        assert_eq!(steps.len(), 3, "у Tilix нет ключа жирного: {steps:?}");
        assert_eq!(
            steps[2],
            "dconf write /com/gexperts/Tilix/profiles/2b7c4080-0ddd-46c5-8f23-563fd3ba789d/use-theme-colors false"
        );
    }

    #[test]
    fn xfce_only_when_theme_colors_are_on() {
        assert!(xfce_steps(None, None, None, true).is_empty(), "умолчание xfce4-terminal — свои цвета");
        assert!(xfce_steps(Some("false"), Some("#ffffff"), Some("#000000"), true).is_empty());
        assert_eq!(
            cmds(&xfce_steps(Some("true"), None, None, true)),
            [
                "xfconf-query -c xfce4-terminal -p /color-foreground -n -t string -s #D3D7CF",
                "xfconf-query -c xfce4-terminal -p /color-background -n -t string -s #1E1E1E",
                "xfconf-query -c xfce4-terminal -p /color-use-theme -n -t bool -s false",
            ]
        );
    }

    #[test]
    fn terminator_profiles() {
        let cfg = "[global_config]\n  title_hide_sizetext = True\n[profiles]\n  [[default]]\n    use_theme_colors = True\n    font = Mono 11\n  [[mine]]\n    foreground_color = \"#00ff00\"\n[layouts]\n";
        let out = terminator_config(cfg, true).unwrap();
        assert_eq!(
            out,
            "[global_config]\n  title_hide_sizetext = True\n[profiles]\n  [[default]]\n    foreground_color = \"#d3d7cf\"\n    background_color = \"#1e1e1e\"\n    use_theme_colors = False\n    font = Mono 11\n  [[mine]]\n    foreground_color = \"#00ff00\"\n[layouts]\n"
        );
        // Already pinned for dark — nothing to change for dark, for light — the pair.
        assert_eq!(terminator_config(&out, true), None);
        assert!(terminator_config(&out, false).unwrap().contains("foreground_color = \"#2e3436\""));
        assert_eq!(terminator_config("[profiles]\n  [[default]]\n    font = Mono\n", true), None);
    }

    #[test]
    fn darkness_follows_user_css_then_theme_then_name() {
        let dark_css = "@define-color theme_bg_color #202020;";
        let light_css = "@define-color theme_bg_color #f0f0f0;";
        assert!(theme_is_dark("Breeze", Some(dark_css), Some(light_css)), "colors.css Plasma перекрывает тему");
        assert!(!theme_is_dark("Foo-Dark", None, Some(light_css)), "цвет важнее имени");
        assert!(theme_is_dark("WhiteSur-Dark-nord", None, None));
        assert!(!theme_is_dark("Adwaita", None, None));
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-termguard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Fake dconf: `dump`, `read`, `write`, `reset`; command log.
    #[derive(Default)]
    struct FakeDconf {
        keys: RefCell<HashMap<String, String>>,
        ran: RefCell<Vec<String>>,
    }

    impl FakeDconf {
        fn run(&self, a: &[String]) -> crate::utils::RunReport {
            self.ran.borrow_mut().push(a.join(" "));
            let ok = |out: String| crate::utils::RunReport { returncode: Some(0), stdout: out.into_bytes(), ..Default::default() };
            match a.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
                ["dconf", "dump", root] => {
                    let mut sections: BTreeMap<String, Vec<String>> = BTreeMap::new();
                    for (k, v) in self.keys.borrow().iter() {
                        if let Some(rest) = k.strip_prefix(root) {
                            let (sec, key) = rest.rsplit_once('/').unwrap_or(("/", rest));
                            let sec = if sec.is_empty() { "/" } else { sec };
                            sections.entry(sec.to_string()).or_default().push(format!("{key}={v}"));
                        }
                    }
                    ok(sections.iter().map(|(s, l)| format!("[{s}]\n{}\n", l.join("\n"))).collect::<Vec<_>>().join("\n"))
                }
                ["dconf", "read", key] => ok(self.keys.borrow().get(*key).cloned().map(|v| format!("{v}\n")).unwrap_or_default()),
                ["dconf", "write", key, value] => {
                    self.keys.borrow_mut().insert(key.to_string(), value.to_string());
                    ok(String::new())
                }
                ["dconf", "reset", key] => {
                    self.keys.borrow_mut().remove(*key);
                    ok(String::new())
                }
                ["gsettings", "get", ..] => ok("'WhiteSur-Dark-nord'\n".into()),
                _ => crate::utils::RunReport::default(),
            }
        }
    }

    fn no_spawn(_: &[String]) {}
    fn no_sync(_: &mut crate::install_plan::Undo) {}

    /// The whole path on the fake: pinning, journal record, «Restore as it was»
    /// restores the previous state; a disabled setting — not a single write.
    #[test]
    fn guard_records_and_reverts() {
        let dir = scratch("guard");
        let env = Env {
            de: "mate".into(),
            home: dir.to_string_lossy().into_owned(),
            config: dir.join(".config").to_string_lossy().into_owned(),
            data: dir.join(".local/share").to_string_lossy().into_owned(),
        };
        let fake = FakeDconf::default();
        let run = |a: &[String], _: Option<u64>| fake.run(a);
        let have = |b: &str| b == "mate-terminal";
        let ctx = Ctx { install: true, steamos: false, privileged: None, run: &run, have: &have, spawn: &no_spawn, wayland: false, sync_env: &no_sync };

        let msg = guard(&env, &ctx, "gtk", "WhiteSur-Dark-nord", None).expect("закрепление");
        assert!(msg.contains("MATE Terminal"), "{msg}");
        let k = |s: &str| fake.keys.borrow().get(&format!("/org/mate/terminal/profiles/default/{s}")).cloned();
        assert_eq!(k("use-theme-colors").as_deref(), Some("false"));
        assert_eq!(k("foreground-color").as_deref(), Some("'#D3D7CF'"));
        assert_eq!(k("background-color").as_deref(), Some("'#1E1E1E'"));
        let rec = crate::install_journal::view_at(&env.rexon_dir());
        assert!(rec.applied.contains_key("gtk"), "запись раздела в журнале");

        // A repeat under the same theme — already own (our) colors, nothing to write.
        assert_eq!(guard(&env, &ctx, "gtk", "WhiteSur-Dark-nord", None), None);

        // «Restore as it was» — the keys are reset (they did not exist).
        let failed = crate::install_journal::revert_applied_with(&env.rexon_dir(), "gtk", &|_| false, &run).unwrap();
        assert_eq!(failed, Some(Vec::new()));
        assert_eq!(k("use-theme-colors"), None);

        // Disabled — not a single command.
        std::fs::create_dir_all(env.rexon_dir()).unwrap();
        std::fs::write(env.rexon_dir().join("settings.json"), format!("{{\"{SETTING_KEY}\": false}}")).unwrap();
        fake.ran.borrow_mut().clear();
        assert_eq!(guard(&env, &ctx, "gtk", "WhiteSur-Dark-nord", None), None);
        assert!(fake.ran.borrow().is_empty(), "{:?}", fake.ran.borrow());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
