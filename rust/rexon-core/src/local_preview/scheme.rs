//! KDE color schemes and Plasma styles: a scheme's colors as KDE itself sees
//! them, whether it is light or dark, and the scheme tile — a window painted in
//! its colors ("Appearance" tab, sections "Color schemes" and "Plasma styles").
//!
//! Behavior is as in Plasma 6.8 (measured on Plasma itself):
//!   * scheme colors — window background and text, views, selections and
//!     buttons; what is missing from the file takes the KDE default color
//!     (Breeze), not a neighboring color of the scheme;
//!   * window title bar: if there is a `[Colors:Header]` group — its background
//!     and text; if not — `[WM] activeBackground` / `activeForeground`; if those
//!     are missing too — the selection colors;
//!   * light or dark: window background brightness (Qt's `qGray`) below 192 —
//!     dark;
//!   * a Plasma style without its own `colors` file takes colors from the
//!     current scheme — Plasma labels it "Follows color scheme" and counts it as
//!     neither light nor dark;
//!   * the scheme tile is a window in its colors: a title bar, a line of window
//!     text and a button, and below a view with lines — normal, selected and
//!     disabled. Text is drawn as bars: one picture for all languages.
//!
//! Outside Plasma Rexon lays the same `.colors` files down as a layer (qt6ct,
//! `kdeglobals`), so both the split and the tile are the same in every
//! environment.
//!
//! Parsing is checked against KDE (a program on `KColorScheme`, KF6 6.31) on real
//! schemes and edge cases — `tests/scheme_look_parity.rs`.

use std::path::Path;

use super::{DrawOp, Rgb};
use crate::appearance_page::Chip;
use crate::i18n::{tr, trc};

/// Scheme colors from which KDE draws its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeLook {
    pub window: Rgb,
    pub window_text: Rgb,
    pub view: Rgb,
    pub view_text: Rgb,
    pub selection: Rgb,
    pub selection_text: Rgb,
    pub button: Rgb,
    pub button_text: Rgb,
    /// Title bar of the active window.
    pub title: Rgb,
    pub title_text: Rgb,
}

/// KDE default colors (KF6 6.31, Breeze) — for keys missing from the file.
const DEFAULT_TEXT: Rgb = (35, 38, 41);
const DEFAULT_WINDOW: Rgb = (239, 240, 241);
const DEFAULT_VIEW: Rgb = (255, 255, 255);
const DEFAULT_SELECTION: Rgb = (61, 174, 233);
const DEFAULT_SELECTION_TEXT: Rgb = (255, 255, 255);
const DEFAULT_BUTTON: Rgb = (252, 252, 252);
const DEFAULT_HEADER: Rgb = (222, 224, 226);

/// KDE's threshold: if the window background brightness (`qGray`) is below it, the scheme is dark.
pub const DARK_GRAY_BELOW: u8 = 192;

/// Brightness of a color as Qt computes it (`qGray`): (11·r + 16·g + 5·b) / 32.
pub fn q_gray(c: Rgb) -> u8 {
    ((c.0 as u32 * 11 + c.1 as u32 * 16 + c.2 as u32 * 5) / 32) as u8
}

/// Value of a key in a group — the LAST one found, as KConfig reads it.
fn last_value<'t>(text: &'t str, group: &str, key: &str) -> Option<&'t str> {
    let mut inside = false;
    let mut found = None;
    for line in text.lines() {
        let s = line.trim();
        if let Some(name) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inside = name == group;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = s.split_once('=') {
            if k.trim() == key {
                found = Some(v.trim());
            }
        }
    }
    found
}

/// Whether the file has a `[group]` group (then the title bar is taken from it).
fn has_group(text: &str, group: &str) -> bool {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix('[').and_then(|r| r.strip_suffix(']')))
        .any(|name| name == group)
}

/// A color as KConfig reads it: `r,g,b` or `r,g,b,a` (each part 0..=255, spaces
/// allowed), `#rgb`, `#rrggbb`, `white`, `black`. Anything else (two parts, a
/// part outside 0..=255) — no color, KDE takes the default. Other SVG color
/// names are not known here — also the default.
fn kconfig_color(value: &str) -> Option<Rgb> {
    let v = value.trim();
    if v.starts_with('#') || v.chars().all(|c| c.is_ascii_alphabetic()) {
        return super::css_color(v);
    }
    let parts: Vec<&str> = v.split(',').collect();
    if !(3..=4).contains(&parts.len()) {
        return None;
    }
    let byte = |s: &str| super::color_byte(s);
    if parts.len() == 4 {
        byte(parts[3])?;
    }
    Some((byte(parts[0])?, byte(parts[1])?, byte(parts[2])?))
}

/// Scheme colors from the text of its file (`.colors` or a Plasma style's `colors`).
pub fn scheme_look(text: &str) -> SchemeLook {
    let get = |group: &str, key: &str, default: Rgb| {
        last_value(text, group, key).and_then(kconfig_color).unwrap_or(default)
    };
    let selection = get("Colors:Selection", "BackgroundNormal", DEFAULT_SELECTION);
    let selection_text = get("Colors:Selection", "ForegroundNormal", DEFAULT_SELECTION_TEXT);
    let (title, title_text) = if has_group(text, "Colors:Header") {
        (
            get("Colors:Header", "BackgroundNormal", DEFAULT_HEADER),
            get("Colors:Header", "ForegroundNormal", DEFAULT_TEXT),
        )
    } else {
        (get("WM", "activeBackground", selection), get("WM", "activeForeground", selection_text))
    };
    SchemeLook {
        window: get("Colors:Window", "BackgroundNormal", DEFAULT_WINDOW),
        window_text: get("Colors:Window", "ForegroundNormal", DEFAULT_TEXT),
        view: get("Colors:View", "BackgroundNormal", DEFAULT_VIEW),
        view_text: get("Colors:View", "ForegroundNormal", DEFAULT_TEXT),
        selection,
        selection_text,
        button: get("Colors:Button", "BackgroundNormal", DEFAULT_BUTTON),
        button_text: get("Colors:Button", "ForegroundNormal", DEFAULT_TEXT),
        title,
        title_text,
    }
}

/// Whether the scheme is dark — by the window background, as KDE does.
pub fn is_dark(look: &SchemeLook) -> bool {
    q_gray(look.window) < DARK_GRAY_BELOW
}

/// Light, dark, or (for a Plasma style) takes the colors of the current scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Light,
    Dark,
    /// A Plasma style without its own `colors` file.
    FollowsScheme,
}

impl Tone {
    /// Word for the reference data and the log.
    pub fn key(self) -> &'static str {
        match self {
            Tone::Light => "light",
            Tone::Dark => "dark",
            Tone::FollowsScheme => "follows",
        }
    }
}

/// Tone from the text of a colors file.
pub fn tone_of_text(text: &str) -> Tone {
    if is_dark(&scheme_look(text)) {
        Tone::Dark
    } else {
        Tone::Light
    }
}

/// Tone of a Plasma style by its directory: no `colors` file — it takes the
/// scheme's colors; there is a file — by its window background (KDE reads an
/// unreadable file as empty, i.e. light by default).
pub fn plasma_style_tone(dir: &Path) -> Tone {
    let colors = dir.join("colors");
    if !colors.exists() {
        return Tone::FollowsScheme;
    }
    tone_of_text(&super::read_text(&colors).unwrap_or_default())
}

/// Text of the scheme file: a tile path may come with or without the `.colors`
/// ending.
fn scheme_text(install_path: &str) -> Option<String> {
    super::read_text(Path::new(install_path))
        .or_else(|| super::read_text(Path::new(&format!("{install_path}.colors"))))
}

/// Theme kind from the tile id (`local:<kind>:<name>`).
fn item_kind(item_id: &str) -> &str {
    item_id.strip_prefix("local:").and_then(|r| r.split(':').next()).unwrap_or("")
}

/// Tone of a theme on disk; `None` — the theme kind has no tone (neither a
/// scheme nor a Plasma style) or the scheme file cannot be read.
pub fn item_tone(item_id: &str, install_path: &str) -> Option<Tone> {
    if install_path.is_empty() {
        return None;
    }
    match item_kind(item_id) {
        "colors" => scheme_text(install_path).map(|t| tone_of_text(&t)),
        "plasma" => Some(plasma_style_tone(Path::new(install_path))),
        _ => None,
    }
}

// ── "All / Light / Dark" filter ──────────────────────────────────────────

/// Tone filter: all.
pub const TONE_ALL: &str = "all";
/// Tone filter: light.
pub const TONE_LIGHT: &str = "light";
/// Tone filter: dark.
pub const TONE_DARK: &str = "dark";
/// Tone filter: Plasma styles that take the scheme's colors.
pub const TONE_FOLLOWS: &str = "follows";

/// Whether a section has a tone filter: "Color schemes" and "Plasma styles", the
/// installed grid (KDE also has the filter only there; in the store the tone is
/// unknown until download).
pub fn tone_bar_applies(category_key: &str, installed_view: bool) -> bool {
    installed_view && matches!(category_key, "colors" | "plasma")
}

/// Effective filter: an unknown value — "All"; "Follows color scheme" — only
/// for Plasma styles.
pub fn tone_filter(category_key: &str, chosen: &str) -> &'static str {
    match chosen {
        TONE_LIGHT => TONE_LIGHT,
        TONE_DARK => TONE_DARK,
        TONE_FOLLOWS if category_key == "plasma" => TONE_FOLLOWS,
        _ => TONE_ALL,
    }
}

/// Filter chips above the section grid (like the drop-down list in Plasma's
/// "Colors" and "Plasma Style"). Empty — the section has no filter.
pub fn tone_chips(category_key: &str) -> Vec<Chip> {
    let plasma = category_key == "plasma";
    if !plasma && category_key != "colors" {
        return Vec::new();
    }
    let (all_tip, light_tip, dark_tip) = if plasma {
        (
            tr("All Plasma styles"),
            tr("Plasma styles with a light window background"),
            tr("Plasma styles with a dark window background"),
        )
    } else {
        (
            tr("All color schemes"),
            tr("Color schemes with a light window background"),
            tr("Color schemes with a dark window background"),
        )
    };
    let mut chips = vec![
        Chip { key: TONE_ALL, label: trc("tone filter", "All"), tooltip: all_tip },
        Chip { key: TONE_LIGHT, label: trc("tone filter", "Light"), tooltip: light_tip },
        Chip { key: TONE_DARK, label: trc("tone filter", "Dark"), tooltip: dark_tip },
    ];
    if plasma {
        chips.push(Chip {
            key: TONE_FOLLOWS,
            label: tr("Color scheme compatible"),
            tooltip: tr("Plasma styles without their own colors: they take the colors of the current color scheme"),
        });
    }
    chips
}

/// Whether a theme on disk stays in the grid under the filter. A theme whose
/// tone cannot be determined is not hidden by the filter only with "All".
pub fn tone_passes(item_id: &str, install_path: &str, filter: &str) -> bool {
    let want = match filter {
        TONE_LIGHT => Tone::Light,
        TONE_DARK => Tone::Dark,
        TONE_FOLLOWS => Tone::FollowsScheme,
        _ => return true,
    };
    item_tone(item_id, install_path) == Some(want)
}

/// The "Follows color scheme" caption under a Plasma style (like the tile
/// caption in "Plasma Style"): for a style without its own colors, except when
/// only such styles are selected anyway.
pub fn follows_scheme_badge(item_id: &str, install_path: &str, filter: &str) -> Option<String> {
    (item_kind(item_id) == "plasma"
        && filter != TONE_FOLLOWS
        && item_tone(item_id, install_path) == Some(Tone::FollowsScheme))
    .then(|| tr("Follows color scheme"))
}

// ── scheme tile ──────────────────────────────────────────────────────────

/// Scheme tile — a window in its colors filling the whole frame, like a tile in
/// Plasma's "Colors": a title bar with a caption in the middle; a line of
/// window text and a button; a view with lines — normal, selected and disabled.
pub fn scheme_window_sample(l: &SchemeLook) -> Vec<DrawOp> {
    let fill = |x: f64, y: f64, w: f64, h: f64, c: Rgb| DrawOp::Fill {
        rect: (x, y, w, h),
        color: (c.0, c.1, c.2, 255),
    };
    let (w, h) = (super::PREVIEW_WIDTH as f64, super::PREVIEW_HEIGHT as f64);
    // KDE's view border is the text color at 0.3 opacity over the view.
    let frame_edge = super::mix(l.view, l.view_text, 0.3);
    let button_edge = super::mix(l.button, l.button_text, 0.3);
    // KDE renders disabled text with the scheme's effects; here it is text halfway
    // faded into the view background.
    let disabled = super::mix(l.view, l.view_text, 0.45);
    vec![
        fill(0.0, 0.0, w, h, l.window),
        // Title bar.
        fill(0.0, 0.0, w, 26.0, l.title),
        fill(w / 2.0 - 45.0, 10.0, 90.0, 6.0, l.title_text),
        // "Window text" and a button.
        fill(12.0, 42.0, 110.0, 7.0, l.window_text),
        fill(204.0, 33.0, 84.0, 25.0, button_edge),
        fill(205.0, 34.0, 82.0, 23.0, l.button),
        fill(223.0, 42.0, 46.0, 6.0, l.button_text),
        // View with lines.
        fill(10.0, 66.0, w - 20.0, 96.0, frame_edge),
        fill(11.0, 67.0, w - 22.0, 94.0, l.view),
        fill(20.0, 78.0, 128.0, 7.0, l.view_text),
        fill(11.0, 97.0, w - 22.0, 30.0, l.selection),
        fill(20.0, 109.0, 150.0, 7.0, l.selection_text),
        fill(20.0, 140.0, 104.0, 7.0, disabled),
    ]
}

/// Sample of a scheme tile: a window in its colors. `None` — there is no file
/// or it has no color group at all (only default colors would be drawn).
pub fn colors_sample(input: &super::SampleInput) -> Option<Vec<DrawOp>> {
    let text = scheme_text(input.install_path)?;
    if !text.lines().any(|l| l.trim().starts_with("[Colors:") || l.trim() == "[WM]") {
        return None;
    }
    Some(scheme_window_sample(&scheme_look(&text)))
}
