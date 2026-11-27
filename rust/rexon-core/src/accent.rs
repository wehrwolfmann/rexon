//! "Accent color" of the "Color schemes" section — as in KDE "Colors": from the color
//! scheme, from the wallpaper or a custom one (10 preset colors and picking any).
//!
//! Where the color lives (checked against the sources and the schemes on the system, 30.10.2026):
//!
//! * Rexon's storage on ALL desktops — the same keys as KDE's:
//!   `kdeglobals [General] AccentColor` (no key — "from the scheme"),
//!   `LastUsedCustomAccentColor` and `accentColorFromWallpaper`
//!   (plasma-workspace 6.8.2 `kcms/colors/colorssettings.kcfg`);
//! * KDE — first the keys, then `plasma-apply-colorscheme --accent-color
//!   <color>` (the same `applyScheme` as the "Colors" module: recolors
//!   selection, links and the focus frame and sends "the palette changed");
//!   "from the scheme" — `--accent-color transparent` (`applyScheme` with a transparent
//!   accent copies the scheme as is). "From the wallpaper" — the color Plasma
//!   computes from the wallpaper itself (`org.kde.PlasmaShell.color`), plus a flag: from then on
//!   the Plasma service `plasma_accentcolor_service` follows the wallpaper;
//! * outside Plasma — Rexon's layer for KDE and Qt programs
//!   ([`crate::shell_palette::plan_scheme_layer`]): it mixes the accent
//!   from `kdeglobals` into the scheme colors the same way `applyScheme` does
//!   ([`scheme_accent_edits`]), and the qt6ct/qt5ct or LXQt palette gets
//!   the selection in this color. No layer (Rexon has not applied a scheme outside Plasma
//!   yet) — the color takes effect together with the scheme, and the person is told so;
//! * GNOME and niri (the GNOME portal) — `org.gnome.desktop.interface
//!   accent-color` (GNOME 47+): nine names, the nearest is picked by libadwaita's
//!   own rule (`adw_accent_color_nearest_from_rgba`, OKLCH);
//! * Cinnamon and Xfce — `org.x.apps.portal accent-rgb` (`#rrggbb`): the xapp
//!   portal hands it to GTK 4 programs if the GTK theme does not set its own
//!   `accent_color` (the `xdg-desktop-portal-xapp` lines); an empty string —
//!   "as in the theme";
//! * COSMIC — the accent lives in `com.system76.CosmicTheme.*.Builder/v2/accent`,
//!   and only cosmic-settings itself builds the theme from it (libcosmic
//!   `ThemeBuilder::build`; cosmic-settings-daemon does not rebuild it).
//!   Hence — with its own commands: `cosmic-settings appearance export <file>.ron`
//!   (the builder of the current mode, dark or light), the
//!   `accent` field is changed in it (`Some("#RRGGBBFF")`, `None` — the palette color), and
//!   `cosmic-settings appearance import` builds and applies the theme
//!   (`pages/desktop/appearance/commands.rs`, cosmic-settings 1.11.0;
//!   checked in a separate HOME on 31.10.2026);
//! * Deepin — `org.deepin.dde.Appearance1.SetActiveColors("#light,#dark")`
//!   on the session bus (dde-appearance: writes `Active_Colors` and the Qt keys
//!   `qt-active-color`/`qt-dark-active-color`); the colors section is closed there,
//!   the block sits above the explanation;
//! * Pantheon — `io.elementary.settings-daemon.interface accent-color`: one
//!   of 11 names (the nearest) or `automatic` — then the elementary service itself
//!   takes the color from the wallpaper and follows its changes (`AccentColorManager`);
//! * LXQt without a layer — its own keys `lxqt.conf [Palette] highlight_color` and
//!   `highlighted_text_color` (lxqt-qtplugin reads them and rereads them itself);
//! * Hyprland, sway, niri, labwc — the "accent" of tiling compositors: the active window border
//!   (Hyprland `general:col.active_border`, sway `client.focused`, niri
//!   `focus-ring`/`border active-color`, labwc `window.active.border.color`)
//!   — via our file with a managed block in the config ([`crate::animations::wm_include`]);
//! * MATE and Budgie — there is no shared accent color: GTK 3 takes colors from the theme, and
//!   their portal (`xdg-desktop-portal-gtk`) does not hand the accent to GTK 4 programs
//!   (its `settings.c` has only `color-scheme` and `contrast`). KDE and Qt programs
//!   get the color through the layer, the rest — through the GTK theme.
//!
//! "From the wallpaper" outside Plasma: the color is taken from the wallpaper picture; when Rexon
//! itself changes the wallpaper ([`crate::wallpaper_exec`]), the accent is recomputed
//! ([`follow_wallpaper`]). Rexon does not track wallpaper changes made by others.
//!
//! "Restore as it was" — from the memo `~/.local/state/rexon/accent.json`:
//! the previous values of all keys Rexon touched the first time.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::animations::{argv, de_label, run_steps, without_block, wm_include, wm_targets, LookCtx, LookIo, LookStep};
use crate::themes::WmSyntax;
use crate::i18n::{n_, tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit, Rgb, SchemeCtx, SchemeWay};

/// The "Appearance" section above whose grid the block sits.
pub const SECTION_KEY: &str = "colors";

/// Preset colors — the same ten as KDE's (`AccentColorUI.qml`).
pub const PRESETS: [&str; 10] =
    ["#e93a9a", "#e93d58", "#e9643a", "#e8cb2d", "#3dd425", "#00d3b8", "#3daee9", "#b875dc", "#926ee4", "#686b6f"];

const KDE_ACCENT: &str = "AccentColor";
const KDE_LAST_CUSTOM: &str = "LastUsedCustomAccentColor";
const KDE_FROM_WALLPAPER: &str = "accentColorFromWallpaper";

const GNOME_SCHEMA: &str = "org.gnome.desktop.interface";
const GNOME_KEY: &str = "accent-color";
const XAPP_SCHEMA: &str = "org.x.apps.portal";
const XAPP_KEY: &str = "accent-rgb";
const PANTHEON_SCHEMA: &str = "io.elementary.settings-daemon.interface";
const PANTHEON_KEY: &str = "accent-color";

const COSMIC_TARGET: &str = "cosmic:accent";
const DEEPIN_TARGET: &str = "deepin:activecolors";
const DEEPIN_DEST: &str = "org.deepin.dde.Appearance1";
const DEEPIN_PATH: &str = "/org/deepin/dde/Appearance1";
const LXQT_KEYS: [&str; 2] = ["highlight_color", "highlighted_text_color"];
const WM_TARGET: &str = "wm:accent";
/// The managed block and our border file for tiling compositors.
pub const WM_TAG: &str = "accent color";
pub const WM_STEM: &str = "rexon-accent";
const LABWC_KEY: &str = "window.active.border.color";

// ═══════════════════════════════════════════════════════════════════════════
// Color math (as in Qt/KDE)
// ═══════════════════════════════════════════════════════════════════════════

fn chan(c: u8) -> f64 {
    c as f64 / 255.0
}

/// A `QColor::fromRgbF` channel → 8 bits: Qt stores 16 bits (`qRound(x·65535)`),
/// and `red()` divides by 257 with rounding (`qt_div_257`).
fn from_f(x: f64) -> u8 {
    let v = (x.clamp(0.0, 1.0) * 65535.0).round() as u32;
    ((v - (v >> 8) + 0x80) >> 8) as u8
}

fn srgb_to_linear(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// OKLab from sRGB (Björn Ottosson's matrices — the same in KDE and libadwaita).
fn oklab(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (srgb_to_linear(chan(c.0)), srgb_to_linear(chan(c.1)), srgb_to_linear(chan(c.2)));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )
}

fn from_oklab(lab: (f64, f64, f64)) -> Rgb {
    let (big_l, a, b) = lab;
    let l = (big_l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (big_l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (big_l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let bl = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    Rgb(from_f(linear_to_srgb(r)), from_f(linear_to_srgb(g)), from_f(linear_to_srgb(bl)))
}

/// OKLCH: lightness, chroma, hue in degrees.
fn oklch(c: Rgb) -> (f64, f64, f64) {
    let (l, a, b) = oklab(c);
    let mut h = b.atan2(a).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    (l, (a * a + b * b).sqrt(), h)
}

/// `tintColor` from `colorsapplicator.cpp`: the hue of `with` at the share `factor`,
/// the base's lightness stays.
pub fn tint(base: Rgb, with: Rgb, factor: f64) -> Rgb {
    let (l, a, b) = oklab(base);
    let (_, wa, wb) = oklab(with);
    from_oklab((l, a + (wa - a) * factor, b + (wb - b) * factor))
}

/// Alpha as `QColor::alphaF()` returns it: 16 bits, a `float` number.
fn qalpha(a: f64) -> f32 {
    ((a * 65535.0).round() / 65535.0) as f32
}

/// `alphaBlend` from `colorsapplicator.h` for an opaque background. Precision is
/// as in Qt: the foreground share is computed in `float`, the rest in
/// `double`, the sum is truncated to an integer (`QColor::fromRgb(int…)`). Otherwise
/// an extra unit appears at the boundaries (172 vs KDE's 173).
fn alpha_blend(fg: Rgb, alpha: f64, bg: Rgb) -> Rgb {
    let a = qalpha(alpha);
    let inv = 1.0 - a as f64;
    let m = |f: u8, b: u8| ((a * f as f32) as f64 + inv * b as f64) as u8;
    Rgb(m(fg.0, bg.0), m(fg.1, bg.1), m(fg.2, bg.2))
}

/// `accentBackground`: the accent at 0.7 opacity over the background.
pub fn accent_background(accent: Rgb, background: Rgb) -> Rgb {
    alpha_blend(accent, 0.7, background)
}

/// `accentForeground`: white or black text over the accent (WCAG 4.5),
/// for the inactive one — at 0.6 opacity.
pub fn accent_foreground(accent: Rgb, active: bool) -> Rgb {
    let white = Rgb(255, 255, 255);
    let c = if contrast_ratio(accent, white) < 4.5 { Rgb(0, 0, 0) } else { white };
    alpha_blend(c, if active { 1.0 } else { 0.6 }, accent)
}

// KHCY from kguiaddons (`kcolorspaces.cpp`, Rec.709, gamma 2.2).
const YC: [f64; 3] = [0.2126, 0.7152, 0.0722];

fn kgamma(n: f64) -> f64 {
    n.clamp(0.0, 1.0).powf(2.2)
}

fn kigamma(n: f64) -> f64 {
    n.clamp(0.0, 1.0).powf(1.0 / 2.2)
}

/// `KColorUtils::luma`.
pub fn luma(c: Rgb) -> f64 {
    kgamma(chan(c.0)) * YC[0] + kgamma(chan(c.1)) * YC[1] + kgamma(chan(c.2)) * YC[2]
}

/// `KColorUtils::contrastRatio`.
pub fn contrast_ratio(c1: Rgb, c2: Rgb) -> f64 {
    let (y1, y2) = (luma(c1), luma(c2));
    if y1 > y2 {
        (y1 + 0.05) / (y2 + 0.05)
    } else {
        (y2 + 0.05) / (y1 + 0.05)
    }
}

struct Khcy {
    h: f64,
    c: f64,
    y: f64,
}

impl Khcy {
    fn new(color: Rgb) -> Khcy {
        let (r, g, b) = (kgamma(chan(color.0)), kgamma(chan(color.1)), kgamma(chan(color.2)));
        let y = r * YC[0] + g * YC[1] + b * YC[2];
        let p = r.max(g).max(b);
        let n = r.min(g).min(b);
        let d = 6.0 * (p - n);
        let h = if n == p {
            0.0
        } else if r == p {
            (g - b) / d
        } else if g == p {
            (b - r) / d + 1.0 / 3.0
        } else {
            (r - g) / d + 2.0 / 3.0
        };
        let c = if r == g && g == b { 0.0 } else { ((y - n) / y).max((p - y) / (1.0 - y)) };
        Khcy { h, c, y }
    }

    fn color(&self) -> Rgb {
        let h = {
            let r = self.h % 1.0;
            if r < 0.0 {
                1.0 + r
            } else {
                r
            }
        };
        let c = self.c.clamp(0.0, 1.0);
        let y = self.y.clamp(0.0, 1.0);
        let hs = h * 6.0;
        let (th, tm) = if hs < 1.0 {
            (hs, YC[0] + YC[1] * hs)
        } else if hs < 2.0 {
            (2.0 - hs, YC[1] + YC[0] * (2.0 - hs))
        } else if hs < 3.0 {
            (hs - 2.0, YC[1] + YC[2] * (hs - 2.0))
        } else if hs < 4.0 {
            (4.0 - hs, YC[2] + YC[1] * (4.0 - hs))
        } else if hs < 5.0 {
            (hs - 4.0, YC[2] + YC[0] * (hs - 4.0))
        } else {
            (6.0 - hs, YC[0] + YC[2] * (6.0 - hs))
        };
        let (tp, to, tn) = if tm >= y {
            (y + y * c * (1.0 - tm) / tm, y + y * c * (th - tm) / tm, y - y * c)
        } else {
            (y + (1.0 - y) * c, y + (1.0 - y) * c * (th - tm) / (1.0 - tm), y - (1.0 - y) * c * tm / (1.0 - tm))
        };
        let f = |v: f64| from_f(kigamma(v));
        if hs < 1.0 {
            Rgb(f(tp), f(to), f(tn))
        } else if hs < 2.0 {
            Rgb(f(to), f(tp), f(tn))
        } else if hs < 3.0 {
            Rgb(f(tn), f(tp), f(to))
        } else if hs < 4.0 {
            Rgb(f(tn), f(to), f(tp))
        } else if hs < 5.0 {
            Rgb(f(to), f(tn), f(tp))
        } else {
            Rgb(f(tp), f(tn), f(to))
        }
    }
}

/// `KColorUtils::lighten(color)` (amount 0.5, chroma as is).
fn lighten(color: Rgb) -> Rgb {
    let mut c = Khcy::new(color);
    c.y = 1.0 - ((1.0 - c.y) * 0.5).clamp(0.0, 1.0);
    c.c = 1.0 - (1.0 - c.c).clamp(0.0, 1.0);
    c.color()
}

/// `KColorUtils::darken(color)` (amount 0.5, chroma as is).
fn darken(color: Rgb) -> Rgb {
    let mut c = Khcy::new(color);
    c.y = (c.y * 0.5).clamp(0.0, 1.0);
    c.c = c.c.clamp(0.0, 1.0);
    c.color()
}

// ═══════════════════════════════════════════════════════════════════════════
// The accent in scheme colors — KDE's `applyScheme`
// ═══════════════════════════════════════════════════════════════════════════

const ACCENT_SETS: &[&str] = &[
    "Colors:View",
    "Colors:Window",
    "Colors:Button",
    "Colors:Selection",
    "Colors:Tooltip",
    "Colors:Complementary",
    "Colors:Header",
];

const ACCENT_SET_KEYS: &[&str] = &[
    "BackgroundNormal",
    "BackgroundAlternate",
    "ForegroundNormal",
    "ForegroundInactive",
    "ForegroundActive",
    "ForegroundLink",
    "ForegroundVisited",
    "ForegroundNegative",
    "ForegroundNeutral",
    "ForegroundPositive",
    "DecorationFocus",
    "DecorationHover",
];

/// Keys the accent replaces entirely.
const ACCENT_KEYS: &[&str] = &["ForegroundActive", "ForegroundLink", "DecorationFocus", "DecorationHover"];

fn color_of(scheme: &str, group: &str, key: &str) -> Option<Rgb> {
    ini_get(scheme, group, key).and_then(|v| Rgb::parse(&v))
}

fn set(group: &str, key: &str, c: Rgb) -> IniEdit {
    IniEdit { group: group.to_string(), key: key.to_string(), value: Some(c.kde()) }
}

fn truthy(v: Option<String>) -> bool {
    v.is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
}

/// Edits on top of the applied scheme with which KDE's `applyScheme`
/// (`colorsapplicator.cpp`, plasma-workspace 6.8.2) brings in the accent: links,
/// focus and hover get the accent color (a link gets a readable variant), selection
/// gets the accent at 0.7 opacity over the background, schemes with `TintFactor` get a light
/// tint of all colors, schemes with an accented title get the window title.
/// Keys missing from the scheme are not tinted (KDE would write the color
/// of an "invalid" `QColor` for them).
pub fn scheme_accent_edits(scheme: &str, accent: Rgb) -> Vec<IniEdit> {
    let titlebar = ini_get(scheme, "General", "TitlebarIsAccentColored")
        .map(|v| truthy(Some(v)))
        .unwrap_or_else(|| truthy(ini_get(scheme, "General", "accentActiveTitlebar")));
    let tint_factor = ini_get(scheme, "General", "TintFactor");
    let tints = tint_factor.is_some();
    let factor = tint_factor.and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.15);
    let black = Rgb(0, 0, 0);
    let window_bg = color_of(scheme, "Colors:Window", "BackgroundNormal").unwrap_or(black);
    let mut out = Vec::new();
    for group in ACCENT_SETS {
        let has = scheme.lines().any(|l| l.trim() == format!("[{group}]"));
        if *group == "Colors:Header" && !has {
            continue;
        }
        for key in ACCENT_SET_KEYS {
            if ACCENT_KEYS.contains(key) {
                let mut c = accent;
                if *key == "ForegroundLink" {
                    let bg = color_of(scheme, group, "BackgroundNormal").unwrap_or(black);
                    if contrast_ratio(bg, c) < 5.0 {
                        c = if luma(c) > luma(bg) { lighten(c) } else { darken(c) };
                    }
                }
                out.push(set(group, key, c));
            } else if tints {
                if let Some(base) = color_of(scheme, group, key) {
                    out.push(set(group, key, tint(base, accent, factor)));
                }
            }
        }
        if *group == "Colors:Selection" {
            let view_bg = color_of(scheme, "Colors:View", "BackgroundNormal").unwrap_or(black);
            let bg = accent_background(accent, view_bg);
            out.push(set(group, "BackgroundNormal", bg));
            out.push(set(group, "BackgroundAlternate", bg));
            out.push(set(group, "ForegroundNormal", accent_foreground(bg, true)));
            out.push(set(group, "ForegroundInactive", accent_foreground(bg, true)));
        }
        if *group == "Colors:Button" {
            let button_bg = color_of(scheme, "Colors:Button", "BackgroundNormal").unwrap_or(black);
            out.push(set(group, "BackgroundAlternate", accent_background(accent, button_bg)));
        }
        let inactive = format!("{group}][Inactive");
        if tints && scheme.lines().any(|l| l.trim() == format!("[{inactive}]")) {
            for key in ACCENT_SET_KEYS {
                if let Some(base) = color_of(scheme, &inactive, key) {
                    out.push(set(&inactive, key, tint(base, accent, factor)));
                }
            }
        }
        if *group == "Colors:Header" && titlebar {
            let active = accent_background(accent, window_bg);
            let idle = tint(window_bg, accent, factor);
            out.push(set(group, "BackgroundNormal", active));
            out.push(set(group, "ForegroundNormal", accent_foreground(active, true)));
            out.push(set(&inactive, "BackgroundNormal", idle));
            out.push(set(&inactive, "ForegroundNormal", accent_foreground(idle, false)));
        }
    }
    if tints || titlebar {
        if tints {
            let tinted = tint(window_bg, accent, factor);
            if !titlebar {
                out.push(set("WM", "activeBackground", tinted));
                out.push(set("WM", "activeForeground", accent_foreground(tinted, true)));
            }
            out.push(set("WM", "inactiveBackground", tinted));
            out.push(set("WM", "inactiveForeground", accent_foreground(tinted, false)));
        }
        if titlebar {
            let active = accent_background(accent, window_bg);
            out.push(set("WM", "activeBackground", active));
            out.push(set("WM", "activeForeground", accent_foreground(active, true)));
        }
    }
    out
}

/// Scheme text with the accent mixed in — the layer builds the Qt palette from it.
pub fn accented_scheme(scheme: &str, accent: Rgb) -> String {
    ini_apply(Some(scheme), &scheme_accent_edits(scheme, accent))
}

/// The custom accent from `kdeglobals` (`None` — "from the scheme": no key, or it is
/// transparent, as KDE writes the default value).
pub fn accent_from_kdeglobals(kdeglobals: &str) -> Option<Rgb> {
    let v = ini_get(kdeglobals, "General", KDE_ACCENT)?;
    let t = v.trim();
    if t.eq_ignore_ascii_case("transparent") {
        return None;
    }
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() == 4 && parts[3] == "0" {
        return None;
    }
    if t.starts_with('#') && t.len() == 9 && t[1..3].eq_ignore_ascii_case("00") {
        return None; // #00rrggbb — transparent too
    }
    Rgb::parse(t)
}

// ═══════════════════════════════════════════════════════════════════════════
// GNOME: the nearest of nine
// ═══════════════════════════════════════════════════════════════════════════

/// GNOME accents (`adw-accent-color.c`): key name, label, color.
pub const GNOME_ACCENTS: &[(&str, &str, &str)] = &[
    ("blue", n_("blue"), "#3584e4"),
    ("teal", n_("teal"), "#2190a4"),
    ("green", n_("green"), "#3a944a"),
    ("yellow", n_("yellow"), "#c88800"),
    ("orange", n_("orange"), "#ed5b00"),
    ("red", n_("red"), "#e62d42"),
    ("pink", n_("pink"), "#d56199"),
    ("purple", n_("purple"), "#9141ac"),
    ("slate", n_("slate"), "#6f8396"),
];

/// The nearest GNOME accent — libadwaita's own rule
/// (`adw_accent_color_nearest_from_rgba`).
pub fn nearest_gnome(c: Rgb) -> &'static str {
    let (_, chroma, h) = oklch(c);
    if chroma < 0.04 {
        return "slate";
    }
    match h {
        h if h > 345.0 => "pink",
        h if h > 280.0 => "purple",
        h if h > 230.0 => "blue",
        h if h > 175.0 => "teal",
        h if h > 115.0 => "green",
        h if h > 75.5 => "yellow",
        h if h > 35.0 => "orange",
        h if h > 10.0 => "red",
        _ => "pink",
    }
}

fn gnome_label(id: &str) -> String {
    GNOME_ACCENTS.iter().find(|(k, _, _)| *k == id).map(|(_, l, _)| tr(l)).unwrap_or_else(|| id.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// Pantheon: the nearest of eleven
// ═══════════════════════════════════════════════════════════════════════════

/// elementary accents (`io.elementary.settings-daemon.interface
/// accent-color`, the colors are the ones the elementary service itself uses to find
/// the nearest): key name, label, color.
pub const PANTHEON_ACCENTS: &[(&str, &str, &str)] = &[
    ("red", n_("red"), "#ed5353"),
    ("orange", n_("orange"), "#ffa154"),
    ("yellow", n_("yellow"), "#f9c440"),
    ("green", n_("green"), "#68b723"),
    ("mint", n_("mint"), "#28bca3"),
    ("blue", n_("blue"), "#3689e6"),
    ("purple", n_("purple"), "#a56de2"),
    ("pink", n_("pink"), "#de3e80"),
    ("brown", n_("brown"), "#8a715e"),
    ("gray", n_("gray"), "#667885"),
    ("latte", n_("latte"), "#e7c591"),
];

/// The nearest elementary accent: the smallest distance in RGB — the same measure
/// the elementary service uses to match an accent to a solid-color background.
pub fn nearest_pantheon(c: Rgb) -> &'static str {
    let dist = |hex: &str| {
        let p = Rgb::parse(hex).unwrap_or(Rgb(0, 0, 0));
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(c.0, p.0) + d(c.1, p.1) + d(c.2, p.2)
    };
    PANTHEON_ACCENTS.iter().min_by_key(|(_, _, hex)| dist(hex)).map(|(id, _, _)| *id).unwrap_or("blue")
}

fn pantheon_label(id: &str) -> String {
    PANTHEON_ACCENTS.iter().find(|(k, _, _)| *k == id).map(|(_, l, _)| tr(l)).unwrap_or_else(|| id.to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// COSMIC: the `accent` field of the theme builder (RON)
// ═══════════════════════════════════════════════════════════════════════════

/// Where in the RON text the value of the top-level field `field` sits:
/// `(start, end)` bytes of the value without surrounding whitespace. Fields of nested
/// structures (`palette: (… accent_blue: …)`) do not count.
fn ron_field_span(text: &str, field: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ if depth == 1 && text[i..].starts_with(field) => {
                let before_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                let rest = text[i + field.len()..].trim_start();
                if before_ok && rest.starts_with(':') {
                    let start_raw = text.len() - rest.len() + 1;
                    let start = start_raw + (text[start_raw..].len() - text[start_raw..].trim_start().len());
                    // The end is a comma or a closing parenthesis at the same level.
                    let (mut d, mut s, mut j) = (0i32, false, start);
                    while j < bytes.len() {
                        let b = bytes[j];
                        if s {
                            if b == b'\\' {
                                j += 2;
                                continue;
                            }
                            if b == b'"' {
                                s = false;
                            }
                        } else {
                            match b {
                                b'"' => s = true,
                                b'(' | b'[' | b'{' => d += 1,
                                b')' | b']' | b'}' if d == 0 => break,
                                b')' | b']' | b'}' => d -= 1,
                                b',' if d == 0 => break,
                                _ => {}
                            }
                        }
                        j += 1;
                    }
                    let end = start + text[start..j].trim_end().len();
                    return Some((start, end));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The value of a top-level field as RON text (`Some("#63D0DFFF")`).
pub fn ron_field(text: &str, field: &str) -> Option<String> {
    ron_field_span(text, field).map(|(a, b)| text[a..b].to_string())
}

/// RON text with the value of a top-level field replaced; if there is no field, it is
/// appended before the last parenthesis.
pub fn ron_with_field(text: &str, field: &str, value: &str) -> Option<String> {
    if let Some((a, b)) = ron_field_span(text, field) {
        return Some(format!("{}{value}{}", &text[..a], &text[b..]));
    }
    let close = text.rfind(')')?;
    let head = text[..close].trim_end();
    let comma = if head.ends_with(',') || head.ends_with('(') { "" } else { "," };
    Some(format!("{head}{comma}\n    {field}: {value},\n{}", &text[close..]))
}

/// A color in COSMIC builder notation (`Some("#RRGGBBFF")`).
pub fn cosmic_accent_value(c: Rgb) -> String {
    format!("Some(\"#{:02X}{:02X}{:02X}FF\")", c.0, c.1, c.2)
}

/// A string from a `dbus-send --print-reply` answer (`string "…"`).
pub fn parse_dbus_string(reply: &str) -> Option<String> {
    let line = reply.lines().rev().find(|l| l.contains("string \""))?;
    let start = line.find('"')? + 1;
    let end = line.rfind('"')?;
    (end >= start).then(|| line[start..end].to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// Tiling compositors: the active window border
// ═══════════════════════════════════════════════════════════════════════════

/// Our file for the active window border in the accent color.
pub fn wm_border_content(syntax: WmSyntax, accent: Rgb) -> String {
    let c = syntax.comment();
    let hex = accent.hex();
    let bare = &hex[1..];
    let mut out = format!("{c} Rexon: accent color. This file is rewritten by Rexon; change the accent color in Rexon instead.\n");
    out.push_str(&match syntax {
        WmSyntax::HyprLua => format!("hl.config({{ general = {{ col = {{ active_border = \"rgb({bare})\" }} }} }})\n"),
        WmSyntax::HyprConf => format!("general {{\n    col.active_border = rgb({bare})\n}}\n"),
        WmSyntax::Kdl => format!(
            "layout {{\n    focus-ring {{\n        active-color \"{hex}\"\n    }}\n    border {{\n        active-color \"{hex}\"\n    }}\n}}\n"
        ),
        WmSyntax::Sway => {
            let text = accent_foreground(accent, true).hex();
            format!("client.focused {hex} {hex} {text} {hex} {hex}\n")
        }
    });
    out
}

/// The last `client.focused` line in the sway config (without it — sway's
/// default colors, sway(5)).
pub fn sway_focused_colors(config: Option<&str>) -> String {
    config
        .unwrap_or("")
        .lines()
        .filter_map(|l| l.trim().strip_prefix("client.focused "))
        .last()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "#4c7899 #285577 #ffffff #2e9ef4 #285577".to_string())
}

// ═══════════════════════════════════════════════════════════════════════════
// Color from the wallpaper
// ═══════════════════════════════════════════════════════════════════════════

/// Accent color from a picture: the "weightiest" hue (24 buckets around the
/// OKLCH circle, weight is chroma), the average color of its bucket; a gray picture
/// yields its average color. Lightness is clamped so text stays readable on the accent.
pub fn wallpaper_accent(img: &image::RgbImage) -> Option<Rgb> {
    const BINS: usize = 24;
    let mut weight = [0.0f64; BINS];
    let mut sum = [(0.0f64, 0.0f64, 0.0f64); BINS];
    let (mut gl, mut ga, mut gb, mut n) = (0.0, 0.0, 0.0, 0.0);
    for p in img.pixels() {
        let c = Rgb(p[0], p[1], p[2]);
        let (l, a, b) = oklab(c);
        gl += l;
        ga += a;
        gb += b;
        n += 1.0;
        let chroma = (a * a + b * b).sqrt();
        if chroma < 0.03 || !(0.15..=0.95).contains(&l) {
            continue;
        }
        let mut h = b.atan2(a).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        let i = ((h / 360.0 * BINS as f64) as usize).min(BINS - 1);
        weight[i] += chroma;
        sum[i].0 += l * chroma;
        sum[i].1 += a * chroma;
        sum[i].2 += b * chroma;
    }
    if n == 0.0 {
        return None;
    }
    let total: f64 = weight.iter().sum();
    let (l, a, b) = if total > n * 0.01 {
        let i = (0..BINS).max_by(|x, y| weight[*x].total_cmp(&weight[*y])).unwrap_or(0);
        let w = weight[i];
        (sum[i].0 / w, sum[i].1 / w, sum[i].2 / w)
    } else {
        (gl / n, ga / n, gb / n)
    };
    Some(from_oklab((l.clamp(0.45, 0.75), a, b)))
}

/// Color from a picture file (scaled down to 64 pixels — fast and noise-free).
pub fn wallpaper_accent_file(path: &str) -> Option<Rgb> {
    let img = image::open(path).ok()?;
    wallpaper_accent(&img.thumbnail(64, 64).to_rgb8())
}

/// The `org.kde.PlasmaShell.color` answer (`dbus-send --print-reply`: `uint32
/// N`, ARGB). Transparent — Plasma has not computed the color yet.
pub fn parse_plasma_color(reply: &str) -> Option<Rgb> {
    let v: u32 = reply.split_whitespace().last()?.parse().ok()?;
    if v >> 24 == 0 {
        return None;
    }
    Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

// ═══════════════════════════════════════════════════════════════════════════
// Desktops
// ═══════════════════════════════════════════════════════════════════════════

/// The desktop's native key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Native {
    /// Plasma: the `kdeglobals` keys and `plasma-apply-colorscheme`.
    Plasma,
    /// `org.gnome.desktop.interface accent-color` (nine names).
    Gnome,
    /// `org.x.apps.portal accent-rgb` (the xapp portal).
    Xapp,
    /// COSMIC: `cosmic-settings appearance export/import` with the `accent` field.
    Cosmic,
    /// Deepin: `org.deepin.dde.Appearance1.SetActiveColors`.
    Deepin,
    /// Pantheon: `io.elementary.settings-daemon.interface accent-color`.
    Pantheon,
    /// LXQt: `lxqt.conf [Palette]` (when there is no scheme layer).
    Lxqt,
    /// No shared accent (MATE, Budgie, others).
    None,
}

/// The desktop's native path by identifier (`system::detect_current_de`).
pub fn native_way(de: &str) -> Native {
    match de {
        "kde" => Native::Plasma,
        "gnome" | "niri" => Native::Gnome,
        "cinnamon-mint" | "xfce" => Native::Xapp,
        "cosmic" => Native::Cosmic,
        "deepin" => Native::Deepin,
        "pantheon" => Native::Pantheon,
        "lxqt" => Native::Lxqt,
        _ => Native::None,
    }
}

/// A compositor whose accent is the active window border.
pub fn border_wm(de: &str) -> Option<&'static str> {
    match de {
        "hyprland" => Some("hyprland"),
        "sway" => Some("sway"),
        "niri" => Some("niri"),
        "labwc" => Some("labwc"),
        _ => None,
    }
}

/// Where to look and where to write.
#[derive(Debug, Clone)]
pub struct AccentCtx {
    pub de: String,
    pub scheme: SchemeCtx,
}

impl AccentCtx {
    pub fn live() -> AccentCtx {
        let de = crate::animations::LookCtx::from_env().de;
        AccentCtx { scheme: SchemeCtx::live(&de), de }
    }

    /// For the "Accent" section of the "Appearance" tab: the desktop is the one selected in
    /// "Current desktop" ([`crate::animations::LookCtx::for_look`]).
    pub fn for_look() -> AccentCtx {
        let de = crate::animations::LookCtx::for_look().de;
        AccentCtx { scheme: SchemeCtx::live(&de), de }
    }

    fn kdeglobals(&self) -> PathBuf {
        self.scheme.home.kdeglobals()
    }

    pub fn memo_file(&self) -> PathBuf {
        self.scheme.home.state_dir().join("rexon/accent.json")
    }

    /// Working exchange file with cosmic-settings (it requires the `.ron` extension).
    fn cosmic_file(&self) -> PathBuf {
        self.scheme.home.state_dir().join("rexon/accent-cosmic.ron")
    }

    fn config_dir(&self) -> PathBuf {
        self.kdeglobals().parent().map(PathBuf::from).unwrap_or_default()
    }

    fn lxqt_conf(&self) -> PathBuf {
        self.config_dir().join("lxqt/lxqt.conf")
    }

    /// The same directories for the shared compositor plans.
    fn look(&self) -> LookCtx {
        let config = self.config_dir().to_string_lossy().into_owned();
        LookCtx::new(&self.de, &self.scheme.home.home, Some(&config))
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// The "as it was" memo
// ═══════════════════════════════════════════════════════════════════════════

/// The previous value of one key: `kde:<key>` or
/// `gsettings:<schema>:<key>`; `prev = None` — the key did not exist.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoEntry {
    pub target: String,
    pub prev: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccentMemo {
    #[serde(default)]
    pub entries: Vec<MemoEntry>,
}

impl AccentMemo {
    fn remember(&mut self, target: &str, prev: Option<String>) {
        if !self.entries.iter().any(|e| e.target == target) {
            self.entries.push(MemoEntry { target: target.to_string(), prev });
        }
    }
    fn prev(&self, target: &str) -> Option<Option<String>> {
        self.entries.iter().find(|e| e.target == target).map(|e| e.prev.clone())
    }
}

pub fn read_memo(io: &dyn LookIo, ctx: &AccentCtx) -> AccentMemo {
    io.read(&ctx.memo_file().to_string_lossy())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_memo(io: &dyn LookIo, ctx: &AccentCtx, memo: &AccentMemo) -> bool {
    let path = ctx.memo_file();
    if memo.entries.is_empty() {
        return std::fs::remove_file(&path).is_ok() || !path.exists();
    }
    let text = serde_json::to_string_pretty(memo).unwrap_or_default() + "\n";
    io.write(&path.to_string_lossy(), &text)
}

fn gsettings_target(schema: &str, key: &str) -> String {
    format!("gsettings:{schema}:{key}")
}

/// The key exists in this system's schemas (`accent-color` — only from GNOME 47).
fn gsettings_has(io: &dyn LookIo, schema: &str, key: &str) -> bool {
    io.have("gsettings") && io.run(&argv(&["gsettings", "range", schema, key])).0
}

fn gsettings_read(io: &dyn LookIo, schema: &str, key: &str) -> Option<String> {
    let (ok, out) = io.run(&argv(&["gsettings", "get", schema, key]));
    ok.then(|| out.trim().trim_matches('\'').to_string())
}

fn gsettings_write(io: &dyn LookIo, schema: &str, key: &str, value: &str) -> Result<(), String> {
    let quoted = format!("'{}'", value.replace('\'', ""));
    let (ok, out) = io.run(&argv(&["gsettings", "set", schema, key, &quoted]));
    if ok {
        Ok(())
    } else {
        Err(out)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Snapshot for the window
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize)]
pub struct ModeOption {
    pub id: String,
    pub label: String,
}

/// Block snapshot (JSON for `AccentSection.qml`).
#[derive(Debug, Clone, Serialize)]
pub struct AccentState {
    /// The "Appearance" section where the block sits.
    pub section: String,
    /// The block is shown (not in Deepin: the colors section is closed there).
    pub available: bool,
    pub title: String,
    /// `scheme`, `wallpaper` or `custom`.
    pub mode: String,
    pub modes: Vec<ModeOption>,
    /// The current accent color `#rrggbb` (empty — from the scheme).
    pub color: String,
    /// The last custom color (for the "Custom" tile).
    pub custom: String,
    pub presets: Vec<String>,
    pub pick_label: String,
    pub pick_title: String,
    /// Where the color takes effect on this desktop — honestly.
    pub note: String,
    pub can_revert: bool,
    pub revert_label: String,
    /// Label of the "open desktop settings" button (COSMIC), empty — none.
    pub open_label: String,
    /// The block also sits above the section's explanation (Deepin: there are no schemes there, but
    /// there is an accent).
    pub on_note: bool,
}

fn kdeglobals_text(io: &dyn LookIo, ctx: &AccentCtx) -> String {
    io.read(&ctx.kdeglobals().to_string_lossy()).unwrap_or_default()
}

/// The scheme Rexon's layer applied outside Plasma (`None` — no layer).
fn layer_scheme(ctx: &AccentCtx) -> Option<String> {
    crate::shell_palette::read_layer_memo(&ctx.scheme.home).scheme
}

fn qt_note(ctx: &AccentCtx) -> String {
    match crate::shell_palette::scheme_way(&ctx.de) {
        SchemeWay::Layer { lxqt } => match layer_scheme(ctx) {
            Some(s) => trf("KDE and Qt apps get it together with the color scheme “%1”.", &[&s]),
            // LXQt without a scheme — the selection color is written into its palette directly.
            None if lxqt => tr("LXQt and its Qt apps get it as the selection color."),
            None => tr("KDE and Qt apps get it once you apply a color scheme in this section."),
        },
        _ => String::new(),
    }
}

/// Where the color takes effect here — in a sentence or two.
pub fn note(ctx: &AccentCtx) -> String {
    let label = de_label(&ctx.de);
    let native = match native_way(&ctx.de) {
        Native::Plasma | Native::Lxqt => String::new(),
        Native::Gnome => tr("GNOME offers nine accent colors, so GTK 4 apps get the closest of them."),
        Native::Xapp => tr("GTK 4 apps get it unless the GTK theme sets its own accent color."),
        Native::Cosmic => tr("COSMIC rebuilds its theme with this accent color (for the current light or dark mode)."),
        Native::Deepin => tr("Deepin gets it as its accent color in light and dark mode."),
        Native::Pantheon => tr("Pantheon offers eleven accent colors, so it gets the closest of them."),
        Native::None if border_wm(&ctx.de).is_some() => String::new(),
        Native::None => trf(
            "%1 has no shared accent color: GTK apps take their colors from the GTK theme. To recolor them, pick a GTK theme in that color in “GTK Themes”.",
            &[&label],
        ),
    };
    let border = border_wm(&ctx.de)
        .map(|_| trf("In %1 the accent color is the border of the active window.", &[&label]))
        .unwrap_or_default();
    [native, border, qt_note(ctx)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

pub fn read_state(io: &dyn LookIo, ctx: &AccentCtx) -> AccentState {
    let kg = kdeglobals_text(io, ctx);
    let color = accent_from_kdeglobals(&kg);
    let from_wallpaper = truthy(ini_get(&kg, "General", KDE_FROM_WALLPAPER));
    let mode = if from_wallpaper && color.is_some() {
        "wallpaper"
    } else if color.is_some() {
        "custom"
    } else {
        "scheme"
    };
    let custom = ini_get(&kg, "General", KDE_LAST_CUSTOM)
        .and_then(|v| accent_from_kdeglobals(&format!("[General]\n{KDE_ACCENT}={v}\n")))
        .map(|c| c.hex())
        .unwrap_or_default();
    let plasma = native_way(&ctx.de) == Native::Plasma;
    let mut note = note(ctx);
    if mode == "wallpaper" && !plasma {
        let again = if native_way(&ctx.de) == Native::Pantheon {
            tr("Pantheon follows the wallpaper itself; Rexon updates the color for KDE and Qt apps when it changes the wallpaper.")
        } else {
            tr("Rexon updates the color when it changes the wallpaper itself; after changing the wallpaper elsewhere, choose “From the wallpaper” again.")
        };
        note = if note.is_empty() { again } else { format!("{note} {again}") };
    }
    let deepin = native_way(&ctx.de) == Native::Deepin;
    let available = deepin || !matches!(crate::shell_palette::scheme_way(&ctx.de), SchemeWay::Unavailable);
    AccentState {
        section: SECTION_KEY.to_string(),
        available,
        title: tr("Accent color"),
        mode: mode.to_string(),
        modes: vec![
            ModeOption {
                id: "scheme".into(),
                label: if plasma { tr("From the color scheme") } else { tr("From the theme") },
            },
            ModeOption { id: "wallpaper".into(), label: tr("From the wallpaper") },
            ModeOption { id: "custom".into(), label: tr("Custom color") },
        ],
        color: color.map(|c| c.hex()).unwrap_or_default(),
        custom,
        presets: PRESETS.iter().map(|s| s.to_string()).collect(),
        pick_label: tr("Choose a color…"),
        pick_title: tr("Accent color"),
        note,
        can_revert: !read_memo(io, ctx).entries.is_empty(),
        revert_label: tr("Restore as it was"),
        open_label: if native_way(&ctx.de) == Native::Cosmic && io.have("cosmic-settings") {
            tr("Open COSMIC Settings")
        } else {
            String::new()
        },
        on_note: deepin,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Applying
// ═══════════════════════════════════════════════════════════════════════════

/// What the person chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Scheme,
    Wallpaper,
    Custom(Rgb),
}

impl Change {
    /// From the window: `scheme`, `wallpaper` or a color `#rrggbb`.
    pub fn parse(text: &str) -> Option<Change> {
        match text.trim() {
            "scheme" => Some(Change::Scheme),
            "wallpaper" => Some(Change::Wallpaper),
            t => Rgb::parse(t).map(Change::Custom),
        }
    }
}

/// The color Plasma itself computed from the wallpaper.
fn plasma_wallpaper_color(io: &dyn LookIo) -> Option<Rgb> {
    if !io.have("dbus-send") {
        return None;
    }
    let (ok, out) = io.run(&argv(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.kde.plasmashell",
        "/PlasmaShell",
        "org.kde.PlasmaShell.color",
    ]));
    if ok {
        parse_plasma_color(&out)
    } else {
        None
    }
}

/// `kdeglobals [General]` edits for the choice (Rexon's storage everywhere).
fn kde_edits(change: Change, color: Option<Rgb>) -> Vec<IniEdit> {
    let e = |key: &str, value: Option<String>| IniEdit { group: "General".into(), key: key.into(), value };
    let mut out = vec![e(KDE_ACCENT, color.map(|c| c.kde()))];
    if let Change::Custom(c) = change {
        out.push(e(KDE_LAST_CUSTOM, Some(c.kde())));
    }
    out.push(e(KDE_FROM_WALLPAPER, (change == Change::Wallpaper).then(|| "true".to_string())));
    out
}

fn kde_target(key: &str) -> String {
    format!("kde:{key}")
}

/// Reapply the scheme with Rexon's layer (outside Plasma) — it picks up the accent.
fn reapply_layer(io: &dyn LookIo, ctx: &AccentCtx) -> Option<Result<(), String>> {
    let SchemeWay::Layer { lxqt } = crate::shell_palette::scheme_way(&ctx.de) else { return None };
    let scheme = layer_scheme(ctx)?;
    Some(crate::shell_palette::apply_scheme_layer(io, &ctx.scheme, lxqt, &scheme).map(|_| ()))
}

/// `plasma-apply-colorscheme --accent-color` (transparent — "from the scheme").
fn plasma_apply(io: &dyn LookIo, color: Option<Rgb>) -> Result<(), String> {
    let value = color.map(|c| c.hex()).unwrap_or_else(|| "transparent".into());
    let (ok, out) = io.run(&argv(&["plasma-apply-colorscheme", "--accent-color", &value]));
    if ok {
        Ok(())
    } else {
        Err(out)
    }
}

fn cmd(io: &dyn LookIo, args: &[&str]) -> Result<(), String> {
    let (ok, out) = io.run(&argv(args));
    if ok {
        Ok(())
    } else {
        Err(out)
    }
}

// ── COSMIC ───────────────────────────────────────────────────────────────

/// The theme builder of the current mode — as RON text.
fn cosmic_export(io: &dyn LookIo, ctx: &AccentCtx) -> Result<String, String> {
    if !io.have("cosmic-settings") {
        return Err(trf("The program %1 is not installed.", &[&"cosmic-settings"]));
    }
    let path = ctx.cosmic_file().to_string_lossy().into_owned();
    cmd(io, &["cosmic-settings", "appearance", "export", &path])?;
    io.read(&path).ok_or_else(|| trf("Could not read %1.", &[&path]))
}

/// The builder's `accent` field — and cosmic-settings builds the theme itself.
fn cosmic_set(io: &dyn LookIo, ctx: &AccentCtx, value: &str) -> Result<(), String> {
    let text = cosmic_export(io, ctx)?;
    let path = ctx.cosmic_file().to_string_lossy().into_owned();
    let Some(new) = ron_with_field(&text, "accent", value) else {
        return Err(trf("Could not read %1.", &[&path]));
    };
    if !io.write(&path, &new) {
        return Err(trf("Could not write %1.", &[&path]));
    }
    let res = cmd(io, &["cosmic-settings", "appearance", "import", &path]);
    let _ = std::fs::remove_file(&path);
    res
}

// ── Deepin ───────────────────────────────────────────────────────────────

fn deepin_call(io: &dyn LookIo, method: &str, args: &[&str]) -> (bool, String) {
    let dest = format!("--dest={DEEPIN_DEST}");
    let mut a = vec!["dbus-send", "--session", "--print-reply", dest.as_str(), DEEPIN_PATH, method];
    a.extend_from_slice(args);
    io.run(&argv(&a))
}

/// The current colors `#light,#dark`; if not set — the current Qt color for both.
fn deepin_colors(io: &dyn LookIo) -> Option<String> {
    let get = format!("{DEEPIN_DEST}.GetActiveColors");
    let (ok, out) = deepin_call(io, &get, &[]);
    let colors = if ok { parse_dbus_string(&out).filter(|s| !s.trim().is_empty()) } else { None };
    colors.or_else(|| {
        let iface = format!("string:{DEEPIN_DEST}");
        let (ok, out) =
            deepin_call(io, "org.freedesktop.DBus.Properties.Get", &[iface.as_str(), "string:QtActiveColor"]);
        let one = if ok { parse_dbus_string(&out).filter(|s| !s.trim().is_empty()) } else { None };
        one.map(|c| format!("{c},{c}"))
    })
}

fn deepin_set(io: &dyn LookIo, colors: &str) -> Result<(), String> {
    let set = format!("{DEEPIN_DEST}.SetActiveColors");
    let arg = format!("string:{colors}");
    let (ok, out) = deepin_call(io, &set, &[arg.as_str()]);
    if ok {
        Ok(())
    } else {
        Err(out)
    }
}

// ── LXQt and tiling compositors ──────────────────────────────────────────

fn lxqt_target(key: &str) -> String {
    format!("lxqt:{key}")
}

fn labwc_target() -> String {
    format!("labwc:{LABWC_KEY}")
}

/// Active window border steps: a color — our file with the block, `None` — remove
/// the block (the person keeps their colors). `labwc_prev` — the previous labwc key.
fn wm_steps(io: &dyn LookIo, ctx: &AccentCtx, color: Option<Rgb>, labwc_prev: Option<String>) -> Vec<LookStep> {
    let Some(wm) = border_wm(&ctx.de) else { return Vec::new() };
    let look = ctx.look();
    let notify = |args: &[&str]| LookStep::Notify(vec![argv(args)]);
    let mut steps = Vec::new();
    if wm == "labwc" {
        let path = crate::window_buttons::labwc_override_path(&look);
        let before = io.read(&path).unwrap_or_default();
        let value = color.map(|c| c.hex()).or(labwc_prev);
        let after = crate::window_buttons::themerc_with(&before, LABWC_KEY, value.as_deref());
        if after != before {
            steps.push(LookStep::WriteFile { path, content: after });
            steps.push(notify(&["labwc", "--reconfigure"]));
        }
        return steps;
    }
    match color {
        Some(c) => {
            steps.push(LookStep::Wm(wm_include(&look, wm, WM_TAG, WM_STEM, &move |s| wm_border_content(s, c))));
            if wm == "sway" {
                // sway does not reread its config by itself, and `reload` restarts
                // its swaybg over Rexon's wallpaper — so we tell it the color with a command.
                let text = accent_foreground(c, true).hex();
                let h = c.hex();
                steps.push(notify(&["swaymsg", &format!("client.focused {h} {h} {text} {h} {h}")]));
            }
        }
        None => {
            let targets = wm_targets(&look, wm, WM_STEM, &|_| String::new());
            for t in &targets {
                if let Some(text) = io.read(&t.config) {
                    let back = without_block(&text, t.syntax, WM_TAG);
                    if back != text {
                        steps.push(LookStep::WriteFile { path: t.config.clone(), content: back });
                    }
                }
            }
            if wm == "sway" {
                let config = targets.iter().find_map(|t| io.read(&t.config));
                let colors = sway_focused_colors(config.as_deref());
                steps.push(notify(&["swaymsg", &format!("client.focused {colors}")]));
            }
        }
    }
    // Hyprland rereads its config by itself, but not the included files.
    if wm == "hyprland" && !steps.is_empty() {
        steps.push(notify(&["hyprctl", "reload"]));
    }
    steps
}

/// Record the choice. `wallpaper` — the current desktop wallpaper picture (for "from
/// the wallpaper" outside Plasma and if Plasma gave no color). `(succeeded, text)`.
pub fn apply(io: &dyn LookIo, ctx: &AccentCtx, change: Change, wallpaper: &dyn Fn() -> Option<String>) -> (bool, String) {
    let native = native_way(&ctx.de);
    let way = crate::shell_palette::scheme_way(&ctx.de);
    if native != Native::Deepin && matches!(way, SchemeWay::Unavailable) {
        return (false, note(ctx));
    }
    let color = match change {
        Change::Scheme => None,
        Change::Custom(c) => Some(c),
        Change::Wallpaper => {
            let from_plasma = if native == Native::Plasma { plasma_wallpaper_color(io) } else { None };
            match from_plasma.or_else(|| wallpaper().and_then(|p| wallpaper_accent_file(&p))) {
                Some(c) => Some(c),
                None => {
                    return (
                        false,
                        tr("Couldn't take a color from the wallpaper: the desktop has no picture wallpaper right now."),
                    )
                }
            }
        }
    };
    // TRANSLATORS: %1 is the technical reason.
    let failed = |why: String| (false, trf("Couldn't apply the accent color: %1", &[&why]));

    // The memo — before the first write.
    let mut memo = read_memo(io, ctx);
    let kg_path = ctx.kdeglobals();
    let kg = kdeglobals_text(io, ctx);
    let edits = kde_edits(change, color);
    for e in &edits {
        memo.remember(&kde_target(&e.key), ini_get(&kg, "General", &e.key));
    }
    let gnome_key = native == Native::Gnome && gsettings_has(io, GNOME_SCHEMA, GNOME_KEY);
    let xapp_key = native == Native::Xapp && gsettings_has(io, XAPP_SCHEMA, XAPP_KEY);
    let pantheon_key = native == Native::Pantheon && gsettings_has(io, PANTHEON_SCHEMA, PANTHEON_KEY);
    for (on, schema, key) in [
        (gnome_key, GNOME_SCHEMA, GNOME_KEY),
        (xapp_key, XAPP_SCHEMA, XAPP_KEY),
        (pantheon_key, PANTHEON_SCHEMA, PANTHEON_KEY),
    ] {
        if on {
            memo.remember(&gsettings_target(schema, key), gsettings_read(io, schema, key));
        }
    }
    if native == Native::Cosmic && memo.prev(COSMIC_TARGET).is_none() {
        match cosmic_export(io, ctx) {
            Ok(text) => memo.remember(COSMIC_TARGET, ron_field(&text, "accent")),
            Err(why) => return failed(why),
        }
    }
    if native == Native::Deepin {
        if !io.have("dbus-send") {
            return failed(trf("The program %1 is not installed.", &[&"dbus-send"]));
        }
        if memo.prev(DEEPIN_TARGET).is_none() {
            memo.remember(DEEPIN_TARGET, deepin_colors(io));
        }
    }
    // LXQt without a scheme layer — its own palette.
    let lxqt_direct = native == Native::Lxqt && layer_scheme(ctx).is_none();
    let lxqt_path = ctx.lxqt_conf();
    let lxqt_text = io.read(&lxqt_path.to_string_lossy()).unwrap_or_default();
    if lxqt_direct {
        for key in LXQT_KEYS {
            memo.remember(&lxqt_target(key), ini_get(&lxqt_text, "Palette", key));
        }
    }
    if let Some(wm) = border_wm(&ctx.de) {
        memo.remember(WM_TARGET, None);
        if wm == "labwc" {
            let path = crate::window_buttons::labwc_override_path(&ctx.look());
            let prev = io.read(&path).and_then(|t| crate::window_buttons::themerc_value(&t, LABWC_KEY));
            memo.remember(&labwc_target(), prev);
        }
    }
    if !save_memo(io, ctx, &memo) {
        return (false, trf("Couldn't save the accent color: %1 could not be written.", &[&ctx.memo_file().to_string_lossy()]));
    }

    let new_kg = ini_apply(Some(&kg), &edits);
    if new_kg != kg && !io.write(&kg_path.to_string_lossy(), &new_kg) {
        return (false, trf("Couldn't save the accent color: %1 could not be written.", &[&kg_path.to_string_lossy()]));
    }

    let mut parts: Vec<String> = Vec::new();
    match native {
        Native::Plasma => {
            if let Err(why) = plasma_apply(io, color) {
                return failed(why);
            }
            parts.push(tr("Accent color applied."));
        }
        Native::Gnome if gnome_key => {
            let value = match color {
                Some(c) => Some(nearest_gnome(c).to_string()),
                // "As in the theme": GNOME has no "from the scheme" of its own — what was there before Rexon.
                None => memo.prev(&gsettings_target(GNOME_SCHEMA, GNOME_KEY)).flatten(),
            };
            if let Some(v) = value {
                if let Err(why) = gsettings_write(io, GNOME_SCHEMA, GNOME_KEY, &v) {
                    return failed(why);
                }
                if color.is_some() {
                    // TRANSLATORS: %1 is a color name such as “blue”.
                    parts.push(trf("GTK 4 apps got the closest GNOME accent color: %1.", &[&gnome_label(&v)]));
                }
            }
        }
        Native::Xapp if xapp_key => {
            let v = color.map(|c| c.hex()).unwrap_or_default();
            if let Err(why) = gsettings_write(io, XAPP_SCHEMA, XAPP_KEY, &v) {
                return failed(why);
            }
            if color.is_some() {
                parts.push(tr("GTK 4 apps get it unless the GTK theme sets its own accent color."));
            }
        }
        Native::Pantheon if pantheon_key => {
            // "From the wallpaper" — the native `automatic`: from then on Pantheon itself
            // follows the wallpaper; "as in the theme" — what was there before Rexon.
            let value = match change {
                Change::Wallpaper => Some("automatic".to_string()),
                Change::Custom(c) => Some(nearest_pantheon(c).to_string()),
                Change::Scheme => memo.prev(&gsettings_target(PANTHEON_SCHEMA, PANTHEON_KEY)).flatten(),
            };
            if let Some(v) = value {
                if let Err(why) = gsettings_write(io, PANTHEON_SCHEMA, PANTHEON_KEY, &v) {
                    return failed(why);
                }
                match change {
                    Change::Wallpaper => parts.push(tr("Pantheon takes its accent color from the wallpaper itself.")),
                    Change::Custom(_) => {
                        // TRANSLATORS: %1 is a color name such as “mint”.
                        parts.push(trf("Pantheon got the closest elementary accent color: %1.", &[&pantheon_label(&v)]))
                    }
                    Change::Scheme => {}
                }
            }
        }
        Native::Cosmic => {
            let value = match color {
                Some(c) => cosmic_accent_value(c),
                None => memo.prev(COSMIC_TARGET).flatten().unwrap_or_else(|| "None".into()),
            };
            if let Err(why) = cosmic_set(io, ctx, &value) {
                return failed(why);
            }
            parts.push(tr("COSMIC rebuilt its theme with the accent color."));
        }
        Native::Deepin => {
            let value = match color {
                Some(c) => Some(format!("{0},{0}", c.hex().to_uppercase())),
                None => memo.prev(DEEPIN_TARGET).flatten(),
            };
            if let Some(v) = value {
                if let Err(why) = deepin_set(io, &v) {
                    return failed(why);
                }
                parts.push(tr("Deepin got the accent color."));
            }
        }
        Native::Lxqt if lxqt_direct => {
            let mut values = [None, None];
            match color {
                Some(c) => {
                    values = [Some(c.hex()), Some(accent_foreground(c, true).hex())];
                }
                None => {
                    for (i, key) in LXQT_KEYS.iter().enumerate() {
                        values[i] = memo.prev(&lxqt_target(key)).flatten();
                    }
                }
            }
            let edits: Vec<IniEdit> = LXQT_KEYS
                .iter()
                .zip(values)
                .map(|(k, v)| IniEdit { group: "Palette".into(), key: k.to_string(), value: v })
                .collect();
            let new = ini_apply(Some(&lxqt_text), &edits);
            if new != lxqt_text && !io.write(&lxqt_path.to_string_lossy(), &new) {
                return failed(trf("Could not write %1.", &[&lxqt_path.to_string_lossy()]));
            }
            parts.push(tr("LXQt and its Qt apps got it."));
        }
        _ => {}
    }
    if border_wm(&ctx.de).is_some() {
        let labwc_prev = memo.prev(&labwc_target()).flatten();
        if let Err(why) = run_steps(io, &ctx.look(), &wm_steps(io, ctx, color, labwc_prev)) {
            return failed(why);
        }
        if color.is_some() {
            parts.push(trf("The border of the active window in %1 got it.", &[&de_label(&ctx.de)]));
        }
    }
    match reapply_layer(io, ctx) {
        Some(Ok(())) => parts.push(tr("KDE and Qt apps got it; apps that are already open pick it up after a restart.")),
        Some(Err(why)) => return (false, why),
        None if matches!(way, SchemeWay::Layer { .. }) && !lxqt_direct => {
            parts.push(tr("KDE and Qt apps get it once you apply a color scheme in this section."));
        }
        None => {}
    }
    (true, parts.join(" "))
}

/// "Restore as it was": keys back to how they were before Rexon, colors reapplied.
pub fn revert(io: &dyn LookIo, ctx: &AccentCtx) -> (bool, String) {
    let memo = read_memo(io, ctx);
    if memo.entries.is_empty() {
        return (true, tr("Nothing to return: Rexon has not changed the accent color."));
    }
    let failed = |why: String| (false, trf("Couldn't return the accent color: %1", &[&why]));
    let kg_path = ctx.kdeglobals();
    let kg = kdeglobals_text(io, ctx);
    let lxqt_path = ctx.lxqt_conf();
    let lxqt_text = io.read(&lxqt_path.to_string_lossy()).unwrap_or_default();
    let mut edits = Vec::new();
    let mut lxqt_edits = Vec::new();
    for e in &memo.entries {
        if let Some(key) = e.target.strip_prefix("kde:") {
            edits.push(IniEdit { group: "General".into(), key: key.into(), value: e.prev.clone() });
        } else if let Some(key) = e.target.strip_prefix("lxqt:") {
            lxqt_edits.push(IniEdit { group: "Palette".into(), key: key.into(), value: e.prev.clone() });
        } else if let Some(rest) = e.target.strip_prefix("gsettings:") {
            if let Some((schema, key)) = rest.split_once(':') {
                let res = match &e.prev {
                    Some(v) => gsettings_write(io, schema, key, v),
                    None => cmd(io, &["gsettings", "reset", schema, key]),
                };
                if let Err(why) = res {
                    return failed(why);
                }
            }
        } else if e.target == COSMIC_TARGET {
            if let Err(why) = cosmic_set(io, ctx, e.prev.as_deref().unwrap_or("None")) {
                return failed(why);
            }
        } else if e.target == DEEPIN_TARGET {
            if let Some(v) = &e.prev {
                if let Err(why) = deepin_set(io, v) {
                    return failed(why);
                }
            }
        } else if e.target == WM_TARGET {
            let labwc_prev = memo.prev(&labwc_target()).flatten();
            if let Err(why) = run_steps(io, &ctx.look(), &wm_steps(io, ctx, None, labwc_prev)) {
                return failed(why);
            }
        }
    }
    let new_kg = ini_apply(Some(&kg), &edits);
    if new_kg != kg && !io.write(&kg_path.to_string_lossy(), &new_kg) {
        return (false, trf("Couldn't save the accent color: %1 could not be written.", &[&kg_path.to_string_lossy()]));
    }
    if !lxqt_edits.is_empty() {
        let new = ini_apply(Some(&lxqt_text), &lxqt_edits);
        if new != lxqt_text && !io.write(&lxqt_path.to_string_lossy(), &new) {
            return (false, trf("Couldn't save the accent color: %1 could not be written.", &[&lxqt_path.to_string_lossy()]));
        }
    }
    if native_way(&ctx.de) == Native::Plasma {
        if let Err(why) = plasma_apply(io, accent_from_kdeglobals(&new_kg)) {
            return failed(why);
        }
    }
    if let Some(Err(why)) = reapply_layer(io, ctx) {
        return (false, why);
    }
    if !save_memo(io, ctx, &AccentMemo::default()) {
        return (false, trf("Couldn't save the accent color: %1 could not be written.", &[&ctx.memo_file().to_string_lossy()]));
    }
    (true, tr("The accent color is back as it was before Rexon."))
}

/// Rexon itself changed the desktop wallpaper: with "from the wallpaper" outside Plasma the color is taken
/// again from the new picture (Plasma handles it itself). `None` — nothing to do.
pub fn follow_wallpaper(io: &dyn LookIo, ctx: &AccentCtx, picture: &str) -> Option<(bool, String)> {
    if native_way(&ctx.de) == Native::Plasma {
        return None;
    }
    let kg = kdeglobals_text(io, ctx);
    let current = accent_from_kdeglobals(&kg)?;
    if !truthy(ini_get(&kg, "General", KDE_FROM_WALLPAPER)) {
        return None;
    }
    let fresh = wallpaper_accent_file(picture)?;
    if fresh == current {
        return None;
    }
    let path = picture.to_string();
    Some(apply(io, ctx, Change::Wallpaper, &move || Some(path.clone())))
}

/// Open COSMIC "Appearance".
pub fn open_native(io: &dyn LookIo, ctx: &AccentCtx) -> bool {
    native_way(&ctx.de) == Native::Cosmic && io.spawn(&argv(&["cosmic-settings", "appearance"]))
}

// ── Live machine ──────────────────────────────────────────────────────────

/// The current desktop wallpaper picture (the same probe as Conky sets use).
fn live_wallpaper() -> Option<String> {
    crate::conky_sets::live_current_wallpaper()
}

pub fn live_state() -> AccentState {
    read_state(&crate::animations::SystemIo, &AccentCtx::for_look())
}

pub fn live_apply(change: &str) -> (bool, String) {
    let Some(change) = Change::parse(change) else {
        return (false, tr("Couldn't apply the accent color: unknown color."));
    };
    let ctx = AccentCtx::for_look();
    let (ok, mut msg) = apply(&crate::animations::SystemIo, &ctx, change, &live_wallpaper);
    // The accent changes the theme colors: terminals that take them get a readable pair
    // (under the colors section's record: its "Restore as it was" restores the terminal too).
    if ok {
        if let Some(line) = crate::terminal_guard::guard_live(&ctx.de, "colors", &tr("Accent color"), None) {
            msg.push('\n');
            msg.push_str(&line);
        }
    }
    (ok, msg)
}

pub fn live_revert() -> (bool, String) {
    revert(&crate::animations::SystemIo, &AccentCtx::for_look())
}

/// Rexon set the picture `picture` as the wallpaper — see [`follow_wallpaper`].
pub fn live_follow_wallpaper(picture: &str) {
    if let Some((ok, msg)) = follow_wallpaper(&crate::animations::SystemIo, &AccentCtx::live(), picture) {
        let level = if ok { crate::applog::Level::Info } else { crate::applog::Level::Warn };
        crate::applog::write(level, "accent", &format!("цвет акцента по новым обоям: {msg}"));
    }
}

pub fn live_open_native() -> bool {
    open_native(&crate::animations::SystemIo, &AccentCtx::for_look())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;

    #[test]
    fn gnome_nearest_follows_libadwaita() {
        for (id, _, hex) in GNOME_ACCENTS {
            assert_eq!(nearest_gnome(Rgb::parse(hex).unwrap()), *id, "{hex}");
        }
        assert_eq!(nearest_gnome(Rgb(128, 128, 128)), "slate");
        assert_eq!(nearest_gnome(Rgb(255, 0, 0)), "red");
        assert_eq!(nearest_gnome(Rgb::parse("#3dd425").unwrap()), "green");
    }

    #[test]
    fn kde_helpers_match_qt() {
        // accentBackground: accent 0.7 over white — the channels are truncated.
        assert_eq!(accent_background(Rgb(61, 174, 233), Rgb(255, 255, 255)), Rgb(119, 198, 239));
        // A light background under the accent — black text, a dark one — white.
        assert_eq!(accent_foreground(Rgb(232, 203, 45), true), Rgb(0, 0, 0));
        assert_eq!(accent_foreground(Rgb(20, 20, 60), true), Rgb(255, 255, 255));
        // Tinting with a share of 0 — the same color.
        assert_eq!(tint(Rgb(49, 54, 59), Rgb(233, 58, 154), 0.0), Rgb(49, 54, 59));
        assert!((contrast_ratio(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 1e-9);
    }

    #[test]
    fn transparent_accent_means_from_scheme() {
        assert_eq!(accent_from_kdeglobals("[General]\nAccentColor=0,0,0,0\n"), None);
        assert_eq!(accent_from_kdeglobals("[General]\nAccentColor=#00000000\n"), None);
        assert_eq!(accent_from_kdeglobals("[General]\nColorScheme=X\n"), None);
        assert_eq!(accent_from_kdeglobals("[General]\nAccentColor=233,58,154\n"), Some(Rgb(233, 58, 154)));
        assert_eq!(accent_from_kdeglobals("[General]\nAccentColor=#e93a9a\n"), Some(Rgb(233, 58, 154)));
    }

    #[test]
    fn plasma_color_reply_is_argb() {
        let reply = "method return time=1.0 sender=:1.5 -> destination=:1.9 serial=7 reply_serial=2\n   uint32 4282232554\n";
        assert_eq!(parse_plasma_color(reply), Some(Rgb(0x3d, 0xae, 0xea)));
        assert_eq!(parse_plasma_color("   uint32 0\n"), None);
        assert_eq!(parse_plasma_color(""), None);
    }

    #[test]
    fn wallpaper_color_takes_the_dominant_hue() {
        // Three quarters blue sky, a quarter gray asphalt.
        let img = image::RgbImage::from_fn(40, 40, |_, y| if y < 30 { image::Rgb([40, 110, 220]) } else { image::Rgb([90, 90, 90]) });
        let c = wallpaper_accent(&img).unwrap();
        assert_eq!(nearest_gnome(c), "blue", "{c:?}");
        // A gray picture — a gray accent.
        let grey = image::RgbImage::from_pixel(8, 8, image::Rgb([128, 128, 128]));
        assert_eq!(nearest_gnome(wallpaper_accent(&grey).unwrap()), "slate");
    }

    const PLAIN: &str = "[Colors:Button]\nBackgroundNormal=41,44,48\nForegroundNormal=252,252,252\n\n\
[Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=252,252,252\n\n\
[Colors:View]\nBackgroundNormal=20,22,24\nForegroundLink=29,153,243\nForegroundNormal=252,252,252\n\n\
[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\n\n[General]\nName=Plain\n";

    #[test]
    fn accent_edits_recolor_selection_links_and_focus() {
        let accent = Rgb(233, 58, 154);
        let edits = scheme_accent_edits(PLAIN, accent);
        let get = |g: &str, k: &str| edits.iter().rev().find(|e| e.group == g && e.key == k).and_then(|e| e.value.clone());
        assert_eq!(get("Colors:View", "DecorationFocus").as_deref(), Some("233,58,154"));
        assert_eq!(get("Colors:Window", "ForegroundActive").as_deref(), Some("233,58,154"));
        let sel = Rgb::parse(&get("Colors:Selection", "BackgroundNormal").unwrap()).unwrap();
        assert_eq!(sel, accent_background(accent, Rgb(20, 22, 24)));
        // Values written by the real `plasma-apply-colorscheme
        // --accent-color '#e93a9a'` (Plasma 6.8.2) over Breeze Dark: the same
        // view and selection backgrounds as PLAIN.
        assert_eq!(get("Colors:Selection", "BackgroundNormal").as_deref(), Some("169,47,115"));
        assert_eq!(get("Colors:Selection", "BackgroundAlternate").as_deref(), Some("169,47,115"));
        assert_eq!(get("Colors:Selection", "ForegroundNormal").as_deref(), Some("255,255,255"));
        assert_eq!(get("Colors:Selection", "ForegroundLink").as_deref(), Some("170,42,112"));
        assert_eq!(get("Colors:View", "ForegroundLink").as_deref(), Some("233,58,154"));
        assert_eq!(get("Colors:Button", "BackgroundAlternate").as_deref(), Some("175,53,122"));
        assert!(get("Colors:Header", "DecorationFocus").is_none(), "без своего заголовка группа не появляется");
        assert!(edits.iter().all(|e| e.group != "WM"), "без подкраски и акцентного заголовка WM не трогается");
        // A scheme with tinting (like Oxygen Dark): window titles — the KDE reference.
        let tinted = "[Colors:Window]\nBackgroundNormal=38,36,35\n\n[General]\nTintFactor=0.15\n";
        let edits = scheme_accent_edits(tinted, accent);
        let wm = |k: &str| edits.iter().rev().find(|e| e.group == "WM" && e.key == k).and_then(|e| e.value.clone());
        assert_eq!(wm("inactiveBackground").as_deref(), Some("50,29,37"));
        assert_eq!(wm("inactiveForeground").as_deref(), Some("172,164,167"));
        // Qt palette from the tinted scheme — selection in the accent color.
        let qt = crate::shell_palette::qtct_palette_text(&accented_scheme(PLAIN, accent));
        assert!(qt.contains(&format!("#ff{}", &sel.hex()[1..])), "{qt}");
    }

    /// A fake machine. `cosmic` — the theme builder that cosmic-settings "exports" and
    /// "imports"; `deepin` — the `GetActiveColors` answer.
    #[derive(Default)]
    struct FakeIo {
        tools: Vec<&'static str>,
        calls: RefCell<Vec<Vec<String>>>,
        gnome_value: String,
        cosmic: RefCell<String>,
        deepin: String,
    }

    impl LookIo for FakeIo {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.calls.borrow_mut().push(argv.to_vec());
            let a = |i: usize| argv.get(i).map(String::as_str).unwrap_or("");
            if a(0) == "cosmic-settings" && a(2) == "export" {
                return (self.write(a(3), &self.cosmic.borrow()), String::new());
            }
            if a(0) == "cosmic-settings" && a(2) == "import" {
                *self.cosmic.borrow_mut() = std::fs::read_to_string(a(3)).unwrap();
                return (true, String::new());
            }
            if a(0) == "dbus-send" && a(5).ends_with("GetActiveColors") {
                return (true, format!("method return time=1.0\n   string \"{}\"\n", self.deepin));
            }
            if a(1) == "get" {
                return (true, format!("'{}'\n", self.gnome_value));
            }
            (true, String::new())
        }
        fn have(&self, tool: &str) -> bool {
            self.tools.contains(&tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            let p = Path::new(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).is_ok()
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    fn home(tag: &str, de: &str) -> (PathBuf, AccentCtx) {
        let root = std::env::temp_dir().join(format!("rexon-accent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".config")).unwrap();
        let get = |k: &str| if k == "HOME" { root.to_string_lossy().into_owned() } else { String::new() };
        let scheme = SchemeCtx {
            de_id: de.into(),
            home: crate::shell_palette::PaletteHome::from_env(get),
            data_dirs: vec![root.join("none").to_string_lossy().into_owned()],
            session_platform_theme: String::new(),
            qt_desktop_installed: false,
        };
        (root, AccentCtx { de: de.into(), scheme })
    }

    #[test]
    fn plasma_writes_kde_keys_then_asks_plasma_and_reverts() {
        let (root, ctx) = home("kde", "kde");
        let kg = root.join(".config/kdeglobals");
        std::fs::write(&kg, "[General]\nColorScheme=BreezeDark\n").unwrap();
        let io = FakeIo { tools: vec!["dbus-send"], calls: RefCell::new(Vec::new()), gnome_value: String::new(), ..Default::default() };
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        let text = std::fs::read_to_string(&kg).unwrap();
        assert_eq!(ini_get(&text, "General", "AccentColor").as_deref(), Some("233,58,154"));
        assert_eq!(ini_get(&text, "General", "LastUsedCustomAccentColor").as_deref(), Some("233,58,154"));
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["plasma-apply-colorscheme", "--accent-color", "#e93a9a"]));
        let st = read_state(&io, &ctx);
        assert_eq!((st.mode.as_str(), st.color.as_str(), st.can_revert), ("custom", "#e93a9a", true));

        let (ok, _) = apply(&io, &ctx, Change::Scheme, &|| None);
        assert!(ok);
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["plasma-apply-colorscheme", "--accent-color", "transparent"]));
        let st = read_state(&io, &ctx);
        assert_eq!((st.mode.as_str(), st.custom.as_str()), ("scheme", "#e93a9a"), "свой цвет запомнен");

        let (ok, msg) = revert(&io, &ctx);
        assert!(ok, "{msg}");
        assert_eq!(std::fs::read_to_string(&kg).unwrap(), "[General]\nColorScheme=BreezeDark\n");
        assert!(!ctx.memo_file().exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gnome_gets_nearest_name_and_qt_waits_for_a_scheme() {
        let (root, ctx) = home("gnome", "gnome");
        let io = FakeIo { tools: vec!["gsettings"], calls: RefCell::new(Vec::new()), gnome_value: "purple".into(), ..Default::default() };
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(61, 212, 37)), &|| None);
        assert!(ok, "{msg}");
        assert!(
            io.calls.borrow().iter().any(|c| *c == argv(&["gsettings", "set", GNOME_SCHEMA, GNOME_KEY, "'green'"])),
            "{:?}",
            io.calls.borrow()
        );
        assert!(io.calls.borrow().iter().all(|c| c[0] != "plasma-apply-colorscheme"));
        assert!(msg.contains("color scheme"), "без слоя честно сказано, когда получат Qt: {msg}");
        // "As in the theme" — what was there before Rexon.
        let (ok, _) = apply(&io, &ctx, Change::Scheme, &|| None);
        assert!(ok);
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["gsettings", "set", GNOME_SCHEMA, GNOME_KEY, "'purple'"]));
        let (ok, _) = revert(&io, &ctx);
        assert!(ok);
        assert!(!root.join(".config/kdeglobals").exists() || std::fs::read_to_string(root.join(".config/kdeglobals")).unwrap().trim().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn wallpaper_outside_plasma_needs_a_picture() {
        let (root, ctx) = home("wp", "mate");
        let io = FakeIo { tools: vec![], calls: RefCell::new(Vec::new()), gnome_value: String::new(), ..Default::default() };
        let (ok, msg) = apply(&io, &ctx, Change::Wallpaper, &|| None);
        assert!(!ok && msg.contains("wallpaper"), "{msg}");
        assert!(!ctx.memo_file().exists(), "отказ до записи");
        let pic = root.join("wall.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 40, 60])).save(&pic).unwrap();
        let p = pic.to_string_lossy().into_owned();
        let (ok, msg) = apply(&io, &ctx, Change::Wallpaper, &|| Some(p.clone()));
        assert!(ok, "{msg}");
        let st = read_state(&io, &ctx);
        assert_eq!(st.mode, "wallpaper");
        assert!(st.note.contains("MATE"), "{}", st.note);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn calls_with(io: &FakeIo, needle: &str) -> Vec<Vec<String>> {
        io.calls.borrow().iter().filter(|c| c.iter().any(|a| a.contains(needle))).cloned().collect()
    }

    /// Real output of `cosmic-settings appearance export` (1.11.0, a separate
    /// HOME, the default dark theme).
    const COSMIC_EXPORT: &str = include_str!("../tests/golden/accent_cosmic_export.ron");

    #[test]
    fn ron_accent_field_is_the_top_level_one() {
        assert_eq!(ron_field(COSMIC_EXPORT, "accent").as_deref(), Some("Some(\"#63D0DFFF\")"));
        // The palette's `accent_blue` and `accent_text` are other fields.
        assert_eq!(ron_field(COSMIC_EXPORT, "accent_blue"), None);
        let new = ron_with_field(COSMIC_EXPORT, "accent", &cosmic_accent_value(Rgb(233, 58, 154))).unwrap();
        assert_eq!(ron_field(&new, "accent").as_deref(), Some("Some(\"#E93A9AFF\")"));
        assert_eq!(new.lines().count(), COSMIC_EXPORT.lines().count(), "прочее не тронуто");
        assert!(new.contains("accent_blue: \"#63D0DFFF\""));
        assert_eq!(ron_field(&new, "success").as_deref(), Some("Some(\"#5EDB8CFF\")"));
        // A multi-line value and a field that does not exist.
        let multi = "(\n    a: 1,\n    accent: Some((\n        red: 0.5,\n        green: 0.1,\n        blue: 0.2,\n    )),\n    b: \"x)\",\n)";
        assert_eq!(ron_with_field(multi, "accent", "None").unwrap(), "(\n    a: 1,\n    accent: None,\n    b: \"x)\",\n)");
        let added = ron_with_field("(\n    a: 1,\n)", "accent", "None").unwrap();
        assert_eq!(ron_field(&added, "accent").as_deref(), Some("None"));
    }

    #[test]
    fn cosmic_rebuilds_its_theme_through_cosmic_settings_and_reverts() {
        let (root, ctx) = home("cosmic", "cosmic");
        let io = FakeIo { tools: vec!["cosmic-settings"], ..Default::default() };
        *io.cosmic.borrow_mut() = COSMIC_EXPORT.to_string();
        let st = read_state(&io, &ctx);
        assert!(st.available && st.note.contains("COSMIC"), "{}", st.note);
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        assert_eq!(ron_field(&io.cosmic.borrow(), "accent").as_deref(), Some("Some(\"#E93A9AFF\")"));
        assert_eq!(calls_with(&io, "import").len(), 1);
        assert!(!ctx.cosmic_file().exists(), "рабочий файл убран");
        // "As in the theme" — the accent that was there before Rexon.
        let (ok, _) = apply(&io, &ctx, Change::Custom(Rgb(61, 212, 37)), &|| None);
        assert!(ok);
        let (ok, _) = apply(&io, &ctx, Change::Scheme, &|| None);
        assert!(ok);
        assert_eq!(ron_field(&io.cosmic.borrow(), "accent").as_deref(), Some("Some(\"#63D0DFFF\")"));
        let (ok, _) = apply(&io, &ctx, Change::Custom(Rgb(1, 2, 3)), &|| None);
        assert!(ok);
        let (ok, msg) = revert(&io, &ctx);
        assert!(ok, "{msg}");
        assert_eq!(*io.cosmic.borrow(), COSMIC_EXPORT, "построитель — ровно как был");
        // Without cosmic-settings — a refusal before writing.
        let (root2, ctx2) = home("cosmic-none", "cosmic");
        let bare = FakeIo::default();
        let (ok, msg) = apply(&bare, &ctx2, Change::Custom(Rgb(1, 2, 3)), &|| None);
        assert!(!ok && msg.contains("cosmic-settings"), "{msg}");
        assert!(!ctx2.memo_file().exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn deepin_sets_active_colors_over_dbus() {
        let (root, ctx) = home("dde", "deepin");
        let io = FakeIo { tools: vec!["dbus-send"], deepin: "#0081FF,#0081FF".into(), ..Default::default() };
        let st = read_state(&io, &ctx);
        assert!(st.available && st.on_note, "раздела схем нет — блок над объяснением");
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        assert!(!msg.contains("color scheme"), "слоя схем в Deepin нет: {msg}");
        let set = calls_with(&io, "SetActiveColors");
        assert_eq!(set.last().unwrap().last().unwrap(), "string:#E93A9A,#E93A9A");
        assert_eq!(set.last().unwrap()[3], "--dest=org.deepin.dde.Appearance1");
        let (ok, _) = revert(&io, &ctx);
        assert!(ok);
        assert_eq!(calls_with(&io, "SetActiveColors").last().unwrap().last().unwrap(), "string:#0081FF,#0081FF");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pantheon_gets_the_closest_of_eleven_or_follows_the_wallpaper() {
        for (id, _, hex) in PANTHEON_ACCENTS {
            assert_eq!(nearest_pantheon(Rgb::parse(hex).unwrap()), *id, "{hex}");
        }
        assert_eq!(nearest_pantheon(Rgb::parse("#e93a9a").unwrap()), "pink");
        assert_eq!(nearest_pantheon(Rgb::parse("#00d3b8").unwrap()), "mint");
        let (root, ctx) = home("pantheon", "pantheon");
        let io = FakeIo { tools: vec!["gsettings"], gnome_value: "blue".into(), ..Default::default() };
        let set = |v: &str| argv(&["gsettings", "set", PANTHEON_SCHEMA, PANTHEON_KEY, &format!("'{v}'")]);
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb::parse("#00d3b8").unwrap()), &|| None);
        assert!(ok, "{msg}");
        assert_eq!(io.calls.borrow().last().unwrap(), &set("mint"));
        let pic = root.join("wall.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 40, 60])).save(&pic).unwrap();
        let p = pic.to_string_lossy().into_owned();
        let (ok, _) = apply(&io, &ctx, Change::Wallpaper, &|| Some(p.clone()));
        assert!(ok);
        assert!(io.calls.borrow().contains(&set("automatic")));
        assert!(read_state(&io, &ctx).note.contains("Pantheon follows the wallpaper itself"));
        let (ok, _) = apply(&io, &ctx, Change::Scheme, &|| None);
        assert!(ok);
        assert_eq!(io.calls.borrow().last().unwrap(), &set("blue"), "как было до Rexon");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lxqt_without_a_scheme_gets_its_own_palette_keys() {
        let (root, ctx) = home("lxqt", "lxqt");
        let conf = ctx.lxqt_conf();
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "[General]\ntheme=frost\n\n[Palette]\nhighlight_color=#3daee9\n").unwrap();
        let io = FakeIo::default();
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        assert!(!msg.contains("once you apply"), "{msg}");
        let text = std::fs::read_to_string(&conf).unwrap();
        assert_eq!(ini_get(&text, "Palette", "highlight_color").as_deref(), Some("#e93a9a"));
        // On light pink — black text (the same WCAG rule as KDE's).
        assert_eq!(ini_get(&text, "Palette", "highlighted_text_color").as_deref(), Some("#000000"));
        let (ok, _) = revert(&io, &ctx);
        assert!(ok);
        let text = std::fs::read_to_string(&conf).unwrap();
        assert_eq!(ini_get(&text, "Palette", "highlight_color").as_deref(), Some("#3daee9"));
        assert_eq!(ini_get(&text, "Palette", "highlighted_text_color"), None);
        assert_eq!(ini_get(&text, "General", "theme").as_deref(), Some("frost"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tiling_compositors_get_the_active_window_border() {
        // Hyprland: our file and a `source` line in its config; "as in the theme" —
        // the block is removed, the person's config is as it was.
        let (root, ctx) = home("hypr", "hyprland");
        let conf = root.join(".config/hypr/hyprland.conf");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        let mine = "general {\n    col.active_border = rgba(33ccffee)\n}\n";
        std::fs::write(&conf, mine).unwrap();
        // Notify only what is "in place" (`LookStep::Notify`).
        let io = FakeIo { tools: vec!["hyprctl", "swaymsg", "labwc"], ..Default::default() };
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        assert!(msg.contains("border of the active window"), "{msg}");
        let own = std::fs::read_to_string(root.join(".config/hypr/rexon-accent.conf")).unwrap();
        assert!(own.contains("col.active_border = rgb(e93a9a)"), "{own}");
        let text = std::fs::read_to_string(&conf).unwrap();
        assert!(text.starts_with(mine) && text.contains("source = "), "{text}");
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["hyprctl", "reload"]));
        let (ok, _) = apply(&io, &ctx, Change::Scheme, &|| None);
        assert!(ok);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&root);

        // sway: the file, the include and the color to the running sway via a command; rollback —
        // with the `client.focused` line from the person's config.
        let (root, ctx) = home("sway", "sway");
        let conf = root.join(".config/sway/config");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "client.focused #111111 #222222 #ffffff\n").unwrap();
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(232, 203, 45)), &|| None);
        assert!(ok, "{msg}");
        let own = std::fs::read_to_string(root.join(".config/sway/rexon-accent.conf")).unwrap();
        assert!(own.contains("client.focused #e8cb2d #e8cb2d #000000 #e8cb2d #e8cb2d"), "{own}");
        let (ok, _) = revert(&io, &ctx);
        assert!(ok);
        assert_eq!(std::fs::read_to_string(&conf).unwrap(), "client.focused #111111 #222222 #ffffff\n");
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["swaymsg", "client.focused #111111 #222222 #ffffff"]));
        let _ = std::fs::remove_dir_all(&root);

        // niri: both the GNOME accent (portal) and the focus ring.
        let (root, ctx) = home("niri", "niri");
        let conf = root.join(".config/niri/config.kdl");
        std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
        std::fs::write(&conf, "layout {\n}\n").unwrap();
        let gio = FakeIo { tools: vec!["gsettings"], gnome_value: "blue".into(), ..Default::default() };
        let (ok, msg) = apply(&gio, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        let own = std::fs::read_to_string(root.join(".config/niri/rexon-accent.kdl")).unwrap();
        assert!(own.contains("active-color \"#e93a9a\""), "{own}");
        assert!(gio.calls.borrow().iter().any(|c| *c == argv(&["gsettings", "set", GNOME_SCHEMA, GNOME_KEY, "'pink'"])));
        let _ = std::fs::remove_dir_all(&root);

        // labwc: the theme overrides key, rollback — to the previous value.
        let (root, ctx) = home("labwc", "labwc");
        let over = root.join(".config/labwc/themerc-override");
        std::fs::create_dir_all(over.parent().unwrap()).unwrap();
        std::fs::write(&over, "border.width: 2\nwindow.active.border.color: #aaaaaa\n").unwrap();
        let (ok, msg) = apply(&io, &ctx, Change::Custom(Rgb(233, 58, 154)), &|| None);
        assert!(ok, "{msg}");
        let text = std::fs::read_to_string(&over).unwrap();
        assert!(text.contains("window.active.border.color: #e93a9a") && text.contains("border.width: 2"), "{text}");
        assert_eq!(io.calls.borrow().last().unwrap(), &argv(&["labwc", "--reconfigure"]));
        let (ok, _) = revert(&io, &ctx);
        assert!(ok);
        let text = std::fs::read_to_string(&over).unwrap();
        assert!(text.contains("window.active.border.color: #aaaaaa"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn mate_and_budgie_say_why_and_where_to_go() {
        for de in ["mate", "budgie"] {
            let (root, ctx) = home(de, de);
            let st = read_state(&FakeIo::default(), &ctx);
            assert!(st.available);
            assert!(st.note.contains("GTK Themes") && st.note.contains(&de_label(de)), "{}", st.note);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn rexon_wallpaper_change_recolors_the_accent_outside_plasma() {
        let (root, ctx) = home("follow", "mate");
        let io = FakeIo::default();
        let red = root.join("red.png");
        let blue = root.join("blue.png");
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 40, 60])).save(&red).unwrap();
        image::RgbImage::from_pixel(16, 16, image::Rgb([40, 90, 220])).save(&blue).unwrap();
        let (red, blue) = (red.to_string_lossy().into_owned(), blue.to_string_lossy().into_owned());
        // A custom color — the wallpaper does not touch it.
        let (ok, _) = apply(&io, &ctx, Change::Custom(Rgb(1, 2, 3)), &|| None);
        assert!(ok);
        assert!(follow_wallpaper(&io, &ctx, &blue).is_none());
        // "From the wallpaper" — a new color from the new picture.
        let r = red.clone();
        let (ok, _) = apply(&io, &ctx, Change::Wallpaper, &move || Some(r.clone()));
        assert!(ok);
        let before = read_state(&io, &ctx).color;
        assert!(follow_wallpaper(&io, &ctx, &red).is_none(), "та же картинка — делать нечего");
        let (ok, msg) = follow_wallpaper(&io, &ctx, &blue).unwrap();
        assert!(ok, "{msg}");
        let st = read_state(&io, &ctx);
        assert_eq!(st.mode, "wallpaper");
        assert_ne!(st.color, before);
        assert_eq!(nearest_gnome(Rgb::parse(&st.color).unwrap()), "blue");
        assert!(st.note.contains("Rexon updates the color"), "{}", st.note);
        // Plasma handles the color itself.
        let (root2, kde) = home("follow-kde", "kde");
        std::fs::write(root2.join(".config/kdeglobals"), "[General]\nAccentColor=1,2,3\naccentColorFromWallpaper=true\n").unwrap();
        assert!(follow_wallpaper(&io, &kde, &blue).is_none());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }
}
