//! Port of the pure part of `rexon/ui_base.py` — the UI look contract.
//!
//! In Python the module is half Qt code (the application palette,
//! QGraphicsDropShadowEffect, font registration, a guard over QMessageBox).
//! Everything that does NOT depend on Qt moved here: color and skin tables,
//! building QSS fragments, rich-text detection and escaping, the indicator
//! SVG check mark. Environment observations (whether the active theme is
//! dark) come in as a parameter, writing the check mark file stays with the
//! executor.
//!
//! Single-source motivation: the `steam_dark` skin is the look standard of
//! ALL the project's applications (rule #17), and its palette must not
//! diverge between Rexon and the others.

use std::collections::BTreeMap;

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Text colors
// ============================================================================

/// Main text of the dark theme. White only: the hierarchy is set by font
/// weight and size, not color (grey/beige shades blended into the background).
pub const DARK_PRIMARY: &str = "#ffffff";
/// Secondary text of the dark theme.
pub const DARK_SECONDARY: &str = "#ffffff";
/// Muted text of the dark theme.
pub const DARK_MUTED: &str = "#ffffff";
/// Main text of the light theme.
pub const LIGHT_PRIMARY: &str = "#1f2326";
/// Secondary text of the light theme.
pub const LIGHT_SECONDARY: &str = "#3d4147";
/// Muted text of the light theme.
pub const LIGHT_MUTED: &str = "#5d6166";

/// Main text color; `dark` — whether the active theme is dark (an executor
/// observation: in Python it is the brightness of `palette(Window)`).
pub fn primary_text_color(dark: bool) -> &'static str {
    if dark { DARK_PRIMARY } else { LIGHT_PRIMARY }
}

/// Secondary text color.
pub fn secondary_text_color(dark: bool) -> &'static str {
    if dark { DARK_SECONDARY } else { LIGHT_SECONDARY }
}

/// Muted text color.
pub fn muted_text_color(dark: bool) -> &'static str {
    if dark { DARK_MUTED } else { LIGHT_MUTED }
}

/// Weight of secondary and muted text on the CSS/Qt6 scale (400 — regular,
/// 600 — semibold). A thin face is unreadable on a dark background even in
/// white. One number for both wrappers: widgets get it as a QSS string
/// (`font-weight: 600`), QML tabs — as the `font.weight` property from the
/// page palette; a second number in markup would silently set the tabs apart.
pub const SECONDARY_TEXT_WEIGHT: i32 = 600;

/// Shared assembly of a text QSS fragment: base + optional properties.
fn text_qss(base: &str, extra: &str) -> String {
    if extra.is_empty() {
        base.to_string()
    } else {
        format!("{base} {extra}")
    }
}

/// QSS of secondary text. The color goes through `palette(window-text)`, not
/// a baked hex: otherwise on a LIVE skin change the color got stuck in the
/// previous one (dark text on a dark background = invisible).
pub fn secondary_text_qss(extra: &str) -> String {
    text_qss(
        &format!("color: palette(window-text); font-weight: {SECONDARY_TEXT_WEIGHT};"),
        extra,
    )
}

/// QSS of muted text (same reasoning — see [`secondary_text_qss`]).
pub fn muted_text_qss(extra: &str) -> String {
    text_qss(
        &format!("color: palette(window-text); font-weight: {SECONDARY_TEXT_WEIGHT};"),
        extra,
    )
}

/// QSS of main text (page and section headings).
pub fn primary_text_qss(extra: &str) -> String {
    text_qss("color: palette(window-text);", extra)
}

/// Neutral semi-transparent grey — hover, borders, separators. A mid grey with
/// alpha reads equally well on dark and light themes.
///
/// Alpha format — like `%g` in Python: no trailing zeros, an integer without
/// a point.
pub fn neutral_rgba(alpha: f64) -> String {
    format!("rgba(127,127,127,{})", format_g(alpha))
}

/// A CSS color from QSS (`rgba(r,g,b,a)` or `rgb(r,g,b)`) as a `#AARRGGBB` string.
///
/// WHY. The product's colors are written the way widget QSS understands them,
/// and in this same form they were handed to QML pages until now. QML does not
/// know such notation at all: `color: "rgba(59,130,246,0.85)"` it silently
/// reads as an INVALID color and paints it BLACK. Silently — that is why it
/// lived so long: the page builds, there is no error, but the badges, chips
/// and stars on it are black (caught on the «Steam» tab 01.09.2026 by a
/// pixel comparison with the original: the card badge (0,0,0) vs
/// (57,119,219)).
///
/// This must not be fixed in each bridge separately (rule #12): the color is
/// the same, and there must be one translator. Everything that cannot be
/// translated (`#rrggbb`, `white`, `transparent`) is returned as is — QML
/// understands such notation by itself.
pub fn css_color_to_qml(css: &str) -> String {
    match parse_css_rgba(css) {
        Some((r, g, b, a)) => format!("#{a:02x}{r:02x}{g:02x}{b:02x}"),
        None => css.trim().to_string(),
    }
}

/// Parse a QSS `rgba(r,g,b,fraction)` or `rgb(r,g,b)` notation into ready
/// 8-bit channels. `None` — wrong notation (a named color, `#rrggbb`,
/// garbage).
///
/// ⚠️ THE ALPHA FRACTION IS TRUNCATED, NOT ROUNDED. This is not a matter of
/// taste: that is how Qt's own style sheet parser reads the fraction, and the
/// widget side is the reference. Experiment 04.09 (one overlay on a `#1b2838`
/// background, fraction vs integer alpha «down» and «up»): 0.85 gives
/// (220,222,225) — like alpha 216, not 217; 0.45 → (129,136,145) — like 114,
/// not 115; 0.14 → (58,70,83) — like 35, not 36; 0.25 → (83,93,105) — like
/// 63, not 64; 0.03 → (33,46,61) — like 7, not 8; 0.12 → (54,65,79) — like
/// 30, not 31. Six fractions of seven tell the methods apart, and all six
/// vote for truncation.
///
/// While rounding stood here, EVERY such color arrived in QML one step denser
/// than the widget one, i.e. on a dark background one step lighter. The test
/// bench threshold (24 levels) did not show it, and the class lived long:
/// twice it was caught by name (the strip backing 2916fcc, the selected rail
/// item 5ac5d42), each time removing the fraction from one markup — while
/// the root was shared and lay in the translator.
pub fn parse_css_rgba(css: &str) -> Option<(u8, u8, u8, u8)> {
    let text = css.trim();
    let body = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<&str> = body.split(',').map(str::trim).collect();
    if parts.len() < 3 || parts.len() > 4 {
        return None;
    }
    let mut rgb = [0u8; 3];
    for (i, part) in parts[..3].iter().enumerate() {
        match part.parse::<f64>() {
            // Fractional values in QSS are legal for alpha but not for the
            // components; we round the same way Qt would.
            Ok(v) if (0.0..=255.0).contains(&v) => rgb[i] = v.round() as u8,
            _ => return None,
        }
    }
    let alpha = match parts.get(3) {
        Some(a) => match a.parse::<f64>() {
            Ok(v) if (0.0..=1.0).contains(&v) => (v * 255.0) as u8,
            _ => return None,
        },
        None => 255,
    };
    Some((rgb[0], rgb[1], rgb[2], alpha))
}

/// Python's `%g` for alpha values (0..1): up to 6 significant digits,
/// trailing zeros and the point are removed.
fn format_g(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let mut s = format!("{v:.6}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}

// ============================================================================
// Rich text: detection and escaping
// ============================================================================

/// Whether the text has an HTML tag — then the author counted on rich text.
///
/// Port of the regular expression `<\s*/?\s*[a-zA-Z][\w-]*(\s[^<>]*)?/?>`
/// without a regex engine: character-by-character comparison. Unlike the Qt
/// heuristic `mightBeRichText()` it does NOT stop at the first line break.
pub fn looks_like_rich_text(text: &str) -> bool {
    let b: Vec<char> = text.chars().collect();
    let n = b.len();
    let mut i = 0;
    while i < n {
        if b[i] != '<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && b[j].is_whitespace() {
            j += 1;
        }
        if j < n && b[j] == '/' {
            j += 1;
        }
        while j < n && b[j].is_whitespace() {
            j += 1;
        }
        // tag name: first an ASCII letter, then [\w-]
        if j < n && b[j].is_ascii_alphabetic() {
            j += 1;
            while j < n && (b[j].is_alphanumeric() || b[j] == '_' || b[j] == '-') {
                j += 1;
            }
            // optional attributes: a space, then anything except < >
            let mut k = j;
            if k < n && b[k].is_whitespace() {
                k += 1;
                while k < n && b[k] != '<' && b[k] != '>' {
                    k += 1;
                }
                if k < n && b[k] == '>' {
                    j = k;
                }
            }
            if j < n && b[j] == '/' {
                j += 1;
            }
            if j < n && b[j] == '>' {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// A monospace rich-text block of a dialog (paths, package lists, README)
/// that WRAPS to the window width.
///
/// A bare `<pre>` in Qt does not wrap at all (`white-space: pre`): a line
/// longer than the window runs past the right edge, and its tail is cut off
/// by the frame. That is how the removal window hid the end of the path to be
/// removed (screenshot 30.09.2026, «…/home-en-appearance/.config/rex»), and
/// the extension window — long README lines. `pre-wrap` keeps line breaks and
/// indents like `<pre>`, but wraps the excess. A path without spaces wraps
/// only if the text has `Text.Wrap` (by words, and a word longer than the
/// line — wherever it falls).
///
/// `inner` — already escaped markup: the function does not touch it.
pub fn dialog_pre(inner: &str) -> String {
    format!("<pre {PRE_LTR_ATTRS} style=\"white-space: pre-wrap\">{inner}</pre>")
}

/// Attributes of a monospace block: code, paths, file trees and JSON read
/// left to right and stand at the left edge in ANY interface language.
///
/// Qt rich text («Supported HTML Subset») understands `dir="ltr|rtl"` and
/// `align` on block elements, `<pre>` included. Checked with a build against
/// Qt 6.12 (06.11.2026): `dir="ltr"` gives the block a left-to-right
/// direction (character order), but the block's place in a document aligned
/// for right-to-left script stays at the right edge; `align="left"` Qt sets
/// as `AlignLeft | AlignAbsolute` — physically on the left. Both are needed.
pub const PRE_LTR_ATTRS: &str = "dir=\"ltr\" align=\"left\"";

/// Append [`PRE_LTR_ATTRS`] to every `<pre>` of ready markup where the
/// direction is not set: in translated help (`<pre>` sits in a catalog
/// string, and the translator has no reason to remember the attributes), in
/// store descriptions. A `<pre>` with `dir` already set is not touched.
pub fn pre_left_to_right(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() + 64);
    let mut from = 0;
    let mut search = 0;
    while let Some(pos) = lower[search..].find("<pre").map(|p| search + p) {
        let after = pos + 4;
        let next = lower.as_bytes().get(after).copied();
        let Some(end) = lower[after..].find('>').map(|p| after + p) else { break };
        search = end;
        if !matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r' | b'/')) {
            continue; // `<prefix>` and other tags starting with «pre…»
        }
        if lower[after..end].contains("dir=") {
            continue;
        }
        out.push_str(&html[from..after]);
        out.push(' ');
        out.push_str(PRE_LTR_ATTRS);
        from = after;
    }
    out.push_str(&html[from..]);
    out
}

/// Escape dynamic text for insertion into a rich-text dialog.
///
/// Port of `html.escape(text, quote=False)` + replacing line breaks with
/// `<br>`: pacman output, paths and error messages contain `<`, `&`, `>`,
/// which would otherwise be eaten by markup, and rich text collapses line
/// breaks into a space. Quotes are NOT escaped (quote=False in the original).
pub fn escape_dialog_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Scrollbar
// ============================================================================

/// QSS of a visible scrollbar (single-mechanism rule): the handle is visible
/// at rest, brighter on hover and press. The color base comes from the theme,
/// so it reads equally on dark and light. `track` — the track background.
pub fn scrollbar_qss(track: &str, dark: bool) -> String {
    let base = if dark { "255,255,255" } else { "0,0,0" };
    format!(
        "\nQScrollBar:vertical {{\n\
         \x20   background: {track};\n\
         \x20   width: 11px;\n\
         \x20   margin: 0;\n\
         \x20   border: none;\n\
         }}\n\
         QScrollBar::handle:vertical {{\n\
         \x20   background: rgba({base},0.28);\n\
         \x20   border-radius: 5px;\n\
         \x20   min-height: 28px;\n\
         }}\n\
         QScrollBar::handle:vertical:hover   {{ background: rgba({base},0.45); }}\n\
         QScrollBar::handle:vertical:pressed {{ background: rgba({base},0.60); }}\n\
         QScrollBar:horizontal {{\n\
         \x20   background: {track};\n\
         \x20   height: 11px;\n\
         \x20   margin: 0;\n\
         \x20   border: none;\n\
         }}\n\
         QScrollBar::handle:horizontal {{\n\
         \x20   background: rgba({base},0.28);\n\
         \x20   border-radius: 5px;\n\
         \x20   min-width: 28px;\n\
         }}\n\
         QScrollBar::handle:horizontal:hover   {{ background: rgba({base},0.45); }}\n\
         QScrollBar::handle:horizontal:pressed {{ background: rgba({base},0.60); }}\n\
         QScrollBar::add-line, QScrollBar::sub-line {{\n\
         \x20   width: 0; height: 0; background: none; border: none;\n\
         }}\n\
         QScrollBar::add-page, QScrollBar::sub-page {{ background: none; }}\n"
    )
}

/// SVG check mark of check/radio indicators. The core builds the file text,
/// writing it to `~/.cache/rexon/ui` stays with the executor (I/O boundary).
///
/// Needed because Qt QSS does not draw a check mark in an indicator the skin
/// recolored with a fill — without it the selected item looks like a colored
/// blot.
pub fn indicator_check_svg(color: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 14 14\">\
         <path d=\"M3 7.6l2.7 2.7L11.2 4.2\" fill=\"none\" stroke=\"{color}\" \
         stroke-width=\"2.6\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>\
         </svg>"
    )
}

/// Check mark file name for the given color (a leading `#` is dropped).
pub fn indicator_check_filename(color: &str) -> String {
    format!("indicator-check-{}.svg", color.trim_start_matches('#'))
}

// ============================================================================
// Interface skins
// ============================================================================

/// Synthwave / Neon Space Retro tokens.
///
/// ⚠️ Qt QSS does not support box-shadow and text-shadow — there is no real
/// glow in QSS. «Neon» is approximated here by a bright border and rounding;
/// the executor attaches a targeted programmatic glow.
pub const SW_TOKENS: &[(&str, &str)] = &[
        ("violet", "#1a0533"),
        ("navy", "#0d1b2a"),
        ("pink", "#ff2d95"),
        ("green", "#39ff14"),
        ("cyan", "#00f0ff"),
        ("card", "rgba(20,8,40,0.55)"),
        ("card_hi", "rgba(40,12,70,0.75)"),
        ("field", "rgba(13,27,42,0.75)"),
        ("text", "#eaf6ff"),
];

/// Default «Appearance» tab tokens (the historical Steam palette). They
/// guarantee a full key set: a partial skin set must not break substitution
/// into the tab's local QSS.
pub const APPEARANCE_STEAM_COLORS: &[(&str, &str)] = &[
        ("bg_primary", "#1b2838"),
        ("bg_secondary", "#16202d"),
        ("bg_card", "#1e2d3d"),
        ("bg_card_hover", "#263d52"),
        ("accent", "#66c0f4"),
        ("accent_green", "#5ba32b"),
        ("accent_green_hover", "#6fc733"),
        ("text_primary", "#ffffff"),
        ("text_secondary", "#ffffff"),
        ("text_dim", "#ffffff"),
        ("rating_star", "#ffc300"),
        ("border", "#2a4055"),
        ("author_self", "#ff5252"),
        ("scrollbar", "255,255,255"),
];

/// Default skin — the look standard of the project's applications.
pub const DEFAULT_SKIN: &str = "steam_dark";

/// Dimming of the background under whatever grabbed attention, RGBA — ONE for
/// the whole product. Two subsystems dim the background: onboarding
/// (`onboarding`, an overlay with a cut-out around the highlighted spot) and
/// the QML tabs' modal windows. Keeping a shade of its own for each would mean
/// showing the customer two different «disabled» backgrounds in one window,
/// so the number here is one, and both sides only take it:
/// `onboarding::DIM_COLOR` and the QML tab palette (key `modal_dim`).
pub const MODAL_DIM_RGBA: (u8, u8, u8, u8) = (0, 0, 0, 170);

/// The same dimming as a `#AARRGGBB` string — QML understands a color in this
/// form (QtWidgets have `QColor(r, g, b, a)` at hand, QML a string).
pub fn modal_dim_argb_hex() -> String {
    let (r, g, b, a) = MODAL_DIM_RGBA;
    format!("#{a:02x}{r:02x}{g:02x}{b:02x}")
}


/// Template of the Synthwave QSS overlay. Substitutions are marked `%TOKEN%`
/// instead of formatting: QSS itself is full of curly braces.
const SYNTHWAVE_TEMPLATE: &str = r#"
/* ── Synthwave / Neon Space Retro (application overlay) ── */
QWidget {
    color: %TEXT%;
}
QMainWindow, QDialog, QScrollArea > QWidget > QWidget {
    background-color: qlineargradient(x1:0, y1:0, x2:0.4, y2:1,
        stop:0 %VIOLET%, stop:1 %NAVY%);
}
QTabWidget::pane {
    border: 1px solid %CYAN%;
    border-radius: 8px;
    background: transparent;
}
QTabBar::tab {
    background: %CARD%;
    color: %TEXT%;
    border: 1px solid rgba(0,240,255,0.35);
    border-bottom: none;
    border-top-left-radius: 8px;
    border-top-right-radius: 8px;
    padding: 8px 16px;
    margin-right: 2px;
    font-weight: bold;
}
QTabBar::tab:selected {
    background: %CARD_HI%;
    color: %CYAN%;
    border: 2px solid %CYAN%;
    border-bottom: none;
}
QTabBar::tab:hover { color: %PINK%; }
QPushButton {
    background: %CARD%;
    color: %CYAN%;
    border: 2px solid %CYAN%;
    border-radius: 10px;
    padding: 7px 16px;
    font-weight: bold;
}
QPushButton:hover {
    background: %CARD_HI%;
    border: 2px solid %PINK%;
    color: %PINK%;
}
QPushButton:pressed { background: rgba(255,45,149,0.25); }
QPushButton:disabled {
    border: 2px solid rgba(120,120,140,0.4);
    color: rgba(180,180,200,0.5);
}
QLineEdit, QComboBox, QSpinBox, QPlainTextEdit, QTextEdit {
    background: %FIELD%;
    color: %TEXT%;
    border: 1px solid %CYAN%;
    border-radius: 8px;
    padding: 5px 8px;
    selection-background-color: %PINK%;
}
QLineEdit:focus, QComboBox:focus { border: 2px solid %PINK%; }
QComboBox QAbstractItemView {
    background: %NAVY%;
    color: %TEXT%;
    border: 1px solid %CYAN%;
    selection-background-color: rgba(255,45,149,0.45);
}
QCheckBox, QRadioButton, QLabel { color: %TEXT%; background: transparent; }
/* Полная стилизация индикаторов: если задать только :checked, Qt рисует
   невыбранные как пустой бокс, а радио без border-radius выходит КВАДРАТНЫМ
   (баг 2026-06-22: зелёный квадрат вместо круга). Описываем обе стадии +
   форму. */
QCheckBox::indicator, QRadioButton::indicator {
    width: 14px;
    height: 14px;
    border: 2px solid %CYAN%;
    background: %FIELD%;
}
QRadioButton::indicator { border-radius: 9px; }   /* круг */
QCheckBox::indicator { border-radius: 4px; }      /* скруглённый квадрат */
QCheckBox::indicator:checked, QRadioButton::indicator:checked {
    background: %GREEN%;
    border: 2px solid %GREEN%;
    /* галочка внутри — иначе «просто зелёное пятно», не видно что
       применилось (общий хелпер indicator_check_url, правило #12) */
    image: url(%CHECK%);
}
QGroupBox {
    border: 1px solid %CYAN%;
    border-radius: 8px;
    margin-top: 8px;
}
QProgressBar {
    background: %FIELD%;
    border: 1px solid %CYAN%;
    border-radius: 6px;
    text-align: center;
    color: %TEXT%;
}
QProgressBar::chunk {
    background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
        stop:0 %PINK%, stop:1 %CYAN%);
    border-radius: 6px;
}
QScrollBar:vertical, QScrollBar:horizontal {
    background: transparent; width: 11px; height: 11px; margin: 0; border: none;
}
QScrollBar::handle:vertical, QScrollBar::handle:horizontal {
    background: rgba(0,240,255,0.35); border-radius: 5px;
    min-height: 28px; min-width: 28px;
}
QScrollBar::handle:vertical:hover, QScrollBar::handle:horizontal:hover {
    background: rgba(255,45,149,0.55);
}
QScrollBar::add-line, QScrollBar::sub-line {
    width: 0; height: 0; background: none; border: none;
}
QScrollBar::add-page, QScrollBar::sub-page { background: none; }
"#;

/// QSS overlay of the Synthwave skin. `check_url` — path of the indicator SVG
/// check mark (the executor writes its file, see [`indicator_check_svg`]).
pub fn synthwave_qss(check_url: &str) -> String {
    let mut out = SYNTHWAVE_TEMPLATE.to_string();
    for (key, value) in SW_TOKENS {
        out = out.replace(&format!("%{}%", key.to_uppercase()), value);
    }
    out.replace("%CHECK%", check_url)
}


/// Skin description: the Qt role palette, an optional QSS overlay and the
/// «Appearance» tab tokens.
pub struct Skin {
    /// Skin identifier: the `ui_skin` setting stores it, code compares by it.
    /// It is not translated and not changed — otherwise the saved choice is lost.
    pub id: &'static str,
    /// Label in the list — a catalog `msgid`; show it through `tr`
    /// ([`available_skins`]).
    pub label: &'static str,
    /// Description under the list — a catalog `msgid` ([`skin_description`]).
    pub description: &'static str,
    /// `None` — reset to the application palette (the «System» skin).
    pub palette: Option<&'static [(&'static str, &'static str)]>,
    /// `None` — the tab tokens match [`APPEARANCE_STEAM_COLORS`].
    pub colors_override: Option<&'static [(&'static str, &'static str)]>,
    /// Whether the skin has an application-level QSS overlay.
    pub has_qss: bool,
    /// File of the bundled heading font (in `rexon/assets/fonts`), if the skin
    /// draws headings with its own font. The executor registers the file.
    pub heading_font: Option<&'static str>,
    /// Programmatic heading glow `(color, blur radius)`: Qt QSS cannot do
    /// text-shadow, the executor attaches the glow as an effect.
    pub glow: Option<(&'static str, u32)>,
    /// Stroke color of the indicator SVG check mark for the QSS overlay. `None` —
    /// no overlay, no need to draw the check mark (nor to write the file to cache).
    pub check_color: Option<&'static str>,
}

/// All skins in the order shown in the settings combo box.
pub const SKINS: &[Skin] = &[
    Skin {
        id: "system",
        label: n_("System (follow the system theme)"),
        description: n_("Colors come from the system Plasma/Qt theme."),
        palette: None,
        colors_override: None,
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "steam_dark",
        // TRANSLATORS: skin name; “Steam” is the Steam client whose look it copies.
        label: n_("Dark (Steam)"),
        description: n_("Dark blue colors in the style of the Steam client. The default."),
        palette: Some(&[
            ("Window", "#1b2838"),
            ("WindowText", "#ffffff"),
            ("Base", "#16202d"),
            ("AlternateBase", "#1e2d3d"),
            ("Text", "#ffffff"),
            ("Button", "#2a4055"),
            ("ButtonText", "#ffffff"),
            ("Highlight", "#66c0f4"),
            ("HighlightedText", "#0b1722"),
            ("ToolTipBase", "#1e2d3d"),
            ("ToolTipText", "#ffffff"),
            ("PlaceholderText", "#9fb4c6"),
            ("Link", "#66c0f4"),
        ]),
        colors_override: None,
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "light",
        // TRANSLATORS: skin name.
        label: n_("Light"),
        description: n_("Light neutral colors (dark text on a white background)."),
        palette: Some(&[
            ("Window", "#f4f5f6"),
            ("WindowText", "#1f2326"),
            ("Base", "#ffffff"),
            ("AlternateBase", "#eceef0"),
            ("Text", "#1f2326"),
            ("Button", "#e3e6e8"),
            ("ButtonText", "#1f2326"),
            ("Highlight", "#3b82f6"),
            ("HighlightedText", "#ffffff"),
            ("ToolTipBase", "#ffffff"),
            ("ToolTipText", "#1f2326"),
            ("PlaceholderText", "#7a8086"),
            ("Link", "#2563eb"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#f4f5f6"),
            ("bg_secondary", "#eceef0"),
            ("bg_card", "#ffffff"),
            ("bg_card_hover", "#e3eaf2"),
            ("accent", "#2563eb"),
            ("accent_green", "#2e9e3f"),
            ("accent_green_hover", "#37b84a"),
            ("text_primary", "#1f2326"),
            ("text_secondary", "#3d4147"),
            ("text_dim", "#5d6166"),
            ("rating_star", "#e0a800"),
            ("border", "#cdd3da"),
            ("author_self", "#d22d2d"),
            ("scrollbar", "0,0,0"),
        ]),
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
    Skin {
        id: "synthwave",
        // The style name is in Latin script in all languages, hence no `n_`.
        label: "Synthwave / Neon Space Retro",
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        description: n_("Neon retro: a purple→blue gradient, pink/green/cyan accents, neon borders and rounded corners. The glow is approximated (Qt QSS doesn't support box-shadow)."),
        palette: Some(&[
            ("Window", "#1a0533"),
            ("WindowText", "#eaf6ff"),
            ("Base", "#0d1b2a"),
            ("AlternateBase", "#160a2e"),
            ("Text", "#eaf6ff"),
            ("Button", "#1f0a3a"),
            ("ButtonText", "#00f0ff"),
            ("Highlight", "#ff2d95"),
            ("HighlightedText", "#0d1b2a"),
            ("ToolTipBase", "#0d1b2a"),
            ("ToolTipText", "#00f0ff"),
            ("PlaceholderText", "#8aa6c0"),
            ("Link", "#00f0ff"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#1a0533"),
            ("bg_secondary", "#0d1b2a"),
            ("bg_card", "#1c0a38"),
            ("bg_card_hover", "#2a0f52"),
            ("accent", "#00f0ff"),
            ("accent_green", "#1f7a30"),
            ("accent_green_hover", "#39ff14"),
            ("text_primary", "#eaf6ff"),
            ("text_secondary", "#00f0ff"),
            ("text_dim", "#9fb4c6"),
            ("rating_star", "#ffd400"),
            ("border", "#00f0ff"),
            ("author_self", "#ff2d95"),
            ("scrollbar", "0,240,255"),
        ]),
        has_qss: true,
        // Tektur (OFL) — a square sci-fi face WITH CYRILLIC. The former
        // Orbitron had no Cyrillic: Russian headings were drawn by the
        // system fallback font, faded and with letters run together.
        heading_font: Some("Tektur-VariableFont_wdth_wght.ttf"),
        glow: Some(("#00f0ff", 18)),
        // Dark stroke: the indicator is filled with neon green.
        check_color: Some("#0d1b2a"),
    },
    // waywallen's own scheme. The Wallpaper Engine tab is painted with
    // the Rexon skin by default (`we_window::vendor_theme`), and this
    // skin gives it back waywallen's native colors and its own «Theme»
    // and «Accent» settings. The rest of the window gets the same
    // Material colors: the dark QmlMaterial scheme (palette type
    // «neutral», like waywallen) from its default accent #6750A4 —
    // taken from the `Qcm.Material` plugin itself (`MD.Token.color`
    // with these settings).
    Skin {
        id: VENDOR_SCHEME_SKIN,
        // TRANSLATORS: skin name, a pair to “Dark (Steam)”; Material is the design style of the waywallen interface.
        label: n_("Dark (Material)"),
        description: n_("Material colors of the waywallen interface. The Wallpaper Engine tab follows waywallen's own theme and accent settings."),
        palette: Some(&[
            ("Window", "#201f21"),
            ("WindowText", "#e6e1e3"),
            ("Base", "#1c1b1d"),
            ("AlternateBase", "#2b292b"),
            ("Text", "#e6e1e3"),
            ("Button", "#363436"),
            ("ButtonText", "#e6e1e3"),
            ("Highlight", "#cbc3d5"),
            ("HighlightedText", "#322e3c"),
            ("ToolTipBase", "#363436"),
            ("ToolTipText", "#e6e1e3"),
            ("PlaceholderText", "#c9c5c7"),
            ("Link", "#cbc3d5"),
        ]),
        colors_override: Some(&[
            ("bg_primary", "#201f21"),
            ("bg_secondary", "#1c1b1d"),
            ("bg_card", "#2b292b"),
            ("bg_card_hover", "#363436"),
            ("accent", "#cbc3d5"),
            ("text_primary", "#e6e1e3"),
            ("text_secondary", "#c9c5c7"),
            ("text_dim", "#c9c5c7"),
            ("border", "#484648"),
            ("author_self", "#ffb4ab"),
        ]),
        has_qss: false,
        heading_font: None,
        glow: None,
        check_color: None,
    },
];

/// The skin with which the Wallpaper Engine tab shows waywallen's own scheme
/// rather than Rexon's colors (see `we_window::vendor_theme`).
pub const VENDOR_SCHEME_SKIN: &str = "waywallen";

/// Skin by id; an unknown id — as in the original, falls back to the default skin.
pub fn skin(skin_id: &str) -> &'static Skin {
    SKINS
        .iter()
        .find(|s| s.id == skin_id)
        .or_else(|| SKINS.iter().find(|s| s.id == DEFAULT_SKIN))
        .expect("скин по умолчанию обязан быть в таблице")
}

/// A list of `(id, label)` for the settings combo box. The label is in the
/// interface language; the choice is saved by `id`, so a language change does
/// not lose the saved skin.
pub fn available_skins() -> Vec<(&'static str, String)> {
    SKINS.iter().map(|s| (s.id, tr(s.label))).collect()
}

/// Short skin description in the interface language.
pub fn skin_description(skin_id: &str) -> String {
    tr(skin(skin_id).description)
}

/// Whether the skin is known.
pub fn is_valid_skin(skin_id: &str) -> bool {
    SKINS.iter().any(|s| s.id == skin_id)
}

/// «Appearance» tab tokens for the skin: the base plus the skin's overrides.
/// A full key set is guaranteed.
pub fn skin_colors(skin_id: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = APPEARANCE_STEAM_COLORS
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    if let Some(over) = skin(skin_id).colors_override {
        for (k, v) in over {
            out.insert((*k).to_string(), (*v).to_string());
        }
    }
    out
}

/// The effective skin from the SAVED value of the `ui_skin` setting.
///
/// One decision for the whole product: an unknown or missing name (a settings
/// file from a future version, a hand edit, a removed skin) falls back to
/// [`DEFAULT_SKIN`]. Previously this fork lived as a separate copy in
/// `rexon/ui_base.py::_current_skin_id`, and the second copy would stay silent
/// on divergence — the window would be painted with one skin and the tab with
/// another.
pub fn active_skin_id_from(saved: Option<&str>) -> &'static str {
    match saved {
        Some(id) => skin(id).id,
        None => DEFAULT_SKIN,
    }
}

/// The effective skin: reads `ui_skin` from settings and applies
/// [`active_skin_id_from`]. The settings directory is NOT created — this is a read.
pub fn active_skin_id() -> &'static str {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    active_skin_id_from(settings.get("ui_skin").and_then(|v| v.as_str()))
}

/// Color tokens of the EFFECTIVE skin — what QML pages are painted with.
///
/// ⚠️ Exactly this, not `skin_colors(DEFAULT_SKIN)`: widget tabs are painted
/// with the chosen skin (`apply_skin`), and a baked name of the standard skin
/// would leave the migrated tabs dark blue in a window where the customer
/// chose «Light» or «Synthwave».
pub fn active_skin_colors() -> BTreeMap<String, String> {
    skin_colors(active_skin_id())
}

/// Qt role palette for the skin (`None` — reset to the application palette).
pub fn skin_palette(skin_id: &str) -> Option<&'static [(&'static str, &'static str)]> {
    skin(skin_id).palette
}

/// Color of a Qt palette ROLE for the skin (`Base`, `Window`, `Button`…), if
/// the skin sets a palette. `None` — the system skin: roles stay system ones,
/// and the core has no way to know them.
pub fn skin_role_color(skin_id: &str, role: &str) -> Option<&'static str> {
    skin_palette(skin_id)?
        .iter()
        .find(|(name, _)| *name == role)
        .map(|(_, hex)| *hex)
}

/// Link color in formatted text (the `Link` and `LinkVisited` roles of the
/// application palette) for the skin: its `Link` role. `None` — the system
/// skin, links keep the system theme's color.
///
/// ⚠️ Why the application palette and not the element's `linkColor`. For a QML
/// `Text`, `linkColor` only affects `Text.StyledText`; in `Text.RichText`
/// (theme descriptions and comments, «About», extension windows) the link is
/// painted by Qt's own HTML parser — with the `Link` role of the APPLICATION
/// palette, at the moment the text is parsed. An unset role is `#0000ff`,
/// unreadable on dark skins (defect 01.11.2026, commit 504655a9 did not cure
/// it).
pub fn skin_link_rgb(skin_id: &str) -> Option<(u8, u8, u8)> {
    skin_role_color(skin_id, "Link").and_then(parse_hex)
}

/// The same for the EFFECTIVE skin.
pub fn active_skin_link_rgb() -> Option<(u8, u8, u8)> {
    skin_link_rgb(active_skin_id())
}

// ============================================================================
// Skin surfaces: a READY color for QML
// ============================================================================
//
// WHY A WHOLE SECTION. A surface is a fill that on the widget side is NOT
// laid by the tab's markup: Qt itself paints an input field by a palette role
// (`Base`), a card — by a semi-transparent overlay from the style sheet. QML
// has neither palette roles nor a style sheet cascade, and pages picked a
// similar skin token by eye. Similar is not the same: on the standard skin
// the field came out `bg_card` (#1e2d3d, (30,45,61)) where Qt paints the
// `Base` role (#16202d, (22,32,45)), i.e. one step lighter than the
// original, and that on four tabs at once. It is cured not by rewriting the
// markup one by one (rule #12), but by the CORE assembling the surface color
// entirely and handing it over as a ready string, while the markup only lays
// it.

/// Card fill over the page background — THE SAME notation the widget style
/// sheet paints the card with (`qt_steam::card_colors`,
/// `qt_compare::card_qss`, `qt_extensions::_card_qss`).
pub const CARD_FILL_DARK: &str = "rgba(255,255,255,0.04)";
pub const CARD_FILL_LIGHT: &str = "rgba(0,0,0,0.03)";
/// Border of a normal button and input field: the Qt style ITSELF draws it,
/// not a palette role — exactly like the splitter handle
/// (`qt_desktops::split_handle_qss`). Measured 05.09 on live Qt (breeze
/// style, steam_dark skin): the «Quick pairs» button on «Compare» is filled
/// (42,64,85) with the `Button` role and bordered (85,102,119) — that is
/// exactly a white 0.20 overlay over the fill (42+0.2*213 = 85,
/// 64+0.2*191 = 102, 85+0.2*170 = 119). No palette role gives such a color.
/// For an input FIELD the same overlay lies on the page background, not on
/// the field's fill: the border stands at the edge where the fill does not
/// reach.
pub const CONTROL_OUTLINE_DARK: &str = "rgba(255,255,255,0.20)";
pub const CONTROL_OUTLINE_LIGHT: &str = "rgba(0,0,0,0.20)";

/// Lay a semi-transparent QSS overlay over an opaque color and return a READY
/// `#rrggbb` — exactly what the customer will see on screen.
///
/// Computes the same way Qt does: the style sheet parser TRUNCATES the alpha
/// fraction (see [`parse_css_rgba`]), and the rasterizer rounds blending to
/// the nearest level. Checked against the screen: a white 0.04 overlay on
/// `#1b2838` gives (36,48,64) in both. An unparsed overlay is returned as is.
pub fn blend_css_over(base: &str, overlay: &str) -> String {
    let (Some((br, bg, bb)), Some((r, g, b, a))) = (parse_hex(base), parse_css_rgba(overlay))
    else {
        return overlay.trim().to_string();
    };
    let k = f64::from(a) / 255.0;
    let mix = |dst: u8, src: u8| (f64::from(dst) + (f64::from(src) - f64::from(dst)) * k).round();
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(br, r) as u8,
        mix(bg, g) as u8,
        mix(bb, b) as u8
    )
}

/// Surfaces the core hands to QML as a ready color: a key for markup and the
/// Qt palette role by which Qt paints the same surface for widgets. The
/// fallback token is needed for the system skin, whose roles the core cannot
/// see.
pub const QML_SURFACES: &[(&str, &str, &str)] = &[
    // Input field, combo box and its item list: `QLineEdit`, `QComboBox`,
    // `QAbstractItemView` — Qt paints all of these with the `Base` role.
    ("surface_field", "Base", "bg_secondary"),
    // Alternating list row.
    ("surface_field_alt", "AlternateBase", "bg_card"),
    // Button and NON-EDITABLE combo box: Qt paints both with the `Button`
    // role, not `Base`. Measured 04.09 on live Qt with steam_dark roles:
    // `QLineEdit` gives (22,32,45) of the `Base` role, and `QComboBox` —
    // (42,64,85) of the `Button` role, over its whole height.
    ("surface_button", "Button", "border"),
    // Page background.
    ("surface_page", "Window", "bg_primary"),
];

/// Ready colors of all the skin's surfaces.
///
/// The card is not among the roles here: its color is given not by the
/// palette but by an overlay over the page background — so it is computed by
/// [`blend_css_over`] from the same literals as in the widget card's style
/// sheet.
pub fn skin_surface_colors(skin_id: &str) -> BTreeMap<String, String> {
    let tokens = skin_colors(skin_id);
    let token = |name: &str| tokens.get(name).cloned().unwrap_or_default();
    let mut out = BTreeMap::new();
    for (key, role, fallback) in QML_SURFACES {
        let value = match skin_role_color(skin_id, role) {
            Some(hex) => hex.to_string(),
            None => token(fallback),
        };
        out.insert((*key).to_string(), value);
    }
    let page = out
        .get("surface_page")
        .cloned()
        .unwrap_or_else(|| token("bg_primary"));
    let dark = parse_hex(&page)
        .map(|(r, g, b)| is_dark_rgb(r, g, b))
        .unwrap_or(true);
    let fill = if dark { CARD_FILL_DARK } else { CARD_FILL_LIGHT };
    out.insert("surface_card".to_string(), blend_css_over(&page, fill));
    // Button and field border: an overlay over THEIR fill, not over the page
    // background — the style borders each control by its own color.
    let outline = if dark {
        CONTROL_OUTLINE_DARK
    } else {
        CONTROL_OUTLINE_LIGHT
    };
    let button = out
        .get("surface_button")
        .cloned()
        .unwrap_or_else(|| token("border"));
    out.insert(
        "surface_button_outline".to_string(),
        blend_css_over(&button, outline),
    );
    // ⚠️ The FIELD border is computed over the page background, not over the
    // field's fill: the fill does not go under the frame. Measured 05.09 (the
    // «Extensions» search field, steam_dark skin): page background (27,40,56),
    // field fill (22,32,45), frame (73,83,96) — that is 0.20 white over the
    // BACKGROUND (27+0.2*228 = 73), while over the fill it would be (69,77,87).
    out.insert(
        "surface_field_outline".to_string(),
        blend_css_over(&page, outline),
    );
    out
}

/// Color of INACTIVE text — exactly what the skin writes into the `Disabled`
/// group of the Qt palette (see [`palette_plan`]), as `#aarrggbb`.
///
/// ⚠️ WHY IT IS IN QML. The widget side does NOT dim a disabled button as a
/// whole: the fill and frame stay the same, and Qt mutes only the label with
/// the `Disabled` group color. Tab style sheets say exactly the same where
/// they write this rule by hand: `#PageBtn:disabled` on «Appearance» and
/// `QPushButton:disabled` in the «Install» block of «Extensions» — both set
/// opacity 0.55, which the style sheet parser truncates to 140, i.e. to
/// [`DISABLED_TEXT_ALPHA`]. The QML markup instead dimmed the WHOLE button
/// with opacity 0.45 and changed the fill color: measured 05.09 on
/// «Extensions» — 10,108 pixels.
pub fn disabled_text_color(skin_id: &str) -> String {
    // The same source as `palette_plan`: the skin's `WindowText` role, and for
    // a skin without its own palette — the forced white of the dark system theme.
    let hex = skin_role_color(skin_id, "WindowText").unwrap_or("#ffffff");
    let (r, g, b) = parse_hex(hex).unwrap_or((255, 255, 255));
    format!("#{DISABLED_TEXT_ALPHA:02x}{r:02x}{g:02x}{b:02x}")
}

/// The palette the core hands to a QML page: skin tokens PLUS ready surface
/// colors. One door for all pages — otherwise a new tab will again pick a
/// token by eye.
pub fn qml_palette(skin_id: &str) -> BTreeMap<String, String> {
    let mut out = skin_colors(skin_id);
    out.extend(skin_surface_colors(skin_id));
    // Not a surface but a TEXT COLOR, so it is placed alongside rather than
    // in the surface table: it has opacity, and `parse_hex` deliberately does
    // not parse it.
    out.insert("text_disabled".to_string(), disabled_text_color(skin_id));
    // Backing of the slider's inactive part (`RexonSlider` reads `pal.track`):
    // the neutral fill of widget cards, readable on any skin. Here rather than
    // in individual pages: previously only «Settings» and «Steam» set the key,
    // and the «Appearance» sliders (Conky, «Screen effects», «Animation») got
    // `undefined` — Qt complained «Unable to assign [undefined] to QColor».
    // ⚠️ Through the translator: QML reads `rgba(...)` as an invalid color and
    // paints it BLACK ([`css_color_to_qml`]).
    out.insert("track".to_string(), css_color_to_qml(&neutral_rgba(SLIDER_TRACK_ALPHA)));
    out
}

/// Density of the slider backing (fraction of [`neutral_rgba`]).
pub const SLIDER_TRACK_ALPHA: f64 = 0.18;

/// The same for the EFFECTIVE skin.
pub fn active_qml_palette() -> BTreeMap<String, String> {
    qml_palette(active_skin_id())
}

/// The skin's QSS overlay; an empty string if there is no overlay.
pub fn skin_qss(skin_id: &str, check_url: &str) -> String {
    let s = skin(skin_id);
    if !s.has_qss {
        return String::new();
    }
    synthwave_qss(check_url)
}

/// Toast plate corner radius, points.
pub const TOAST_RADIUS: i32 = 8;
/// Plate border width, points.
pub const TOAST_BORDER_WIDTH: i32 = 1;
/// Left and right padding inside the plate, points.
pub const TOAST_PADDING_H: i32 = 14;
/// Top and bottom padding inside the plate, points.
pub const TOAST_PADDING_V: i32 = 10;

/// Pop-up toast colors as QSS `rgba(...)` notation.
///
/// The field names are not skin tokens but ready colors: the plate is laid
/// over the content and must stay readable on ANY skin, so its tone is set
/// directly and depends only on how dark the theme is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastSkin {
    /// Plate fill (semi-transparent).
    pub background: &'static str,
    /// Plate border.
    pub border: &'static str,
    /// Label.
    pub text: &'static str,
}

/// Toast colors for a dark or light theme.
///
/// ⚠️ ONE DOOR FOR BOTH PATHS. Both the widget style sheet ([`toast_qss`])
/// and the QML markup (through the tab's `styles` map) take them from here.
/// While the numbers live here, the plate cannot acquire a second set of
/// colors «by eye»: in QML the toast was drawn with the
/// `bg_secondary`/`border`/`text_primary` tokens and came out OPAQUE, with a
/// different corner radius and in a different corner than the widget version.
pub fn toast_colors(dark: bool) -> ToastSkin {
    if dark {
        ToastSkin {
            background: "rgba(30,30,30,0.94)",
            border: "rgba(255,255,255,0.18)",
            text: "rgba(255,255,255,0.92)",
        }
    } else {
        ToastSkin {
            background: "rgba(250,250,250,0.96)",
            border: "rgba(0,0,0,0.18)",
            text: "rgba(0,0,0,0.88)",
        }
    }
}

/// QSS of the pop-up overlay toast — a semi-transparent plate with a border.
/// `dark` — whether the active theme is dark (an executor observation, like
/// for [`primary_text_color`]).
pub fn toast_qss(dark: bool) -> String {
    let c = toast_colors(dark);
    let (bg, border, fg) = (c.background, c.border, c.text);
    let (bw, radius) = (TOAST_BORDER_WIDTH, TOAST_RADIUS);
    let (pad_v, pad_h) = (TOAST_PADDING_V, TOAST_PADDING_H);
    format!(
        "QLabel {{ background-color: {bg}; color: {fg}; \
         border: {bw}px solid {border}; border-radius: {radius}px; \
         padding: {pad_v}px {pad_h}px; }}"
    )
}

/// `(outline, fill, width)` of the skin's heading, or `None`.
///
/// The outline is thin and light on purpose: at the real heading size a thick
/// dark outline ate the letter and the text looked unreadable.
pub fn heading_outline_colors(skin_id: &str) -> Option<(&'static str, &'static str, u32)> {
    if skin_id == "synthwave" {
        Some(("#2bd83a", "#9dff7a", 1))
    } else {
        None
    }
}

/// Default heading outline `(outline, fill, width)` — the way the heading
/// widget itself sets it up before a skin turns it on.
pub fn heading_default_outline() -> (&'static str, &'static str, u32) {
    ("#ff2d95", "#eaf6ff", 2)
}

/// File of the skin's bundled heading font (in `rexon/assets/fonts`) or
/// `None`. The executor registers the file in the Qt font database.
pub fn skin_heading_font(skin_id: &str) -> Option<&'static str> {
    skin(skin_id).heading_font
}

/// Glow of the skin's heading `(color, blur radius)` or `None` — the heavy
/// effect is attached only where it is part of the look.
pub fn skin_glow(skin_id: &str) -> Option<(&'static str, u32)> {
    skin(skin_id).glow
}

/// Stroke color of the indicator check mark for the skin's QSS overlay, or `None`.
///
/// `None` also means «no overlay»: no need to write the check mark file to
/// cache. Previously the color was written in Python next to the call, and an
/// SVG file was prepared even for a skin without an overlay.
pub fn skin_indicator_check_color(skin_id: &str) -> Option<&'static str> {
    skin(skin_id).check_color
}

// ============================================================================
// Whether the active theme is dark
// ============================================================================

/// Brightness threshold below which a theme counts as dark.
pub const DARK_LEVEL: u32 = 128;

/// Whether the theme is dark with such a window background color.
///
/// The only place this criterion lives: the average channel brightness is
/// below half. Before the port the tabs decided differently — some computed
/// the channel average, some asked Qt for `lightness()` (half the sum of the
/// extreme channels), and on a saturated color the answers diverged: yellow
/// `#ffff00` gives an average of 170 (light) and `lightness` 127 (dark). Rule
/// #12 — one criterion for the whole product, the observation (the color
/// itself) comes in as a parameter.
pub fn is_dark_rgb(r: u8, g: u8, b: u8) -> bool {
    u32::from(r) + u32::from(g) + u32::from(b) < 3 * DARK_LEVEL
}

/// Whether the EFFECTIVE skin is dark — by its main background and the shared
/// criterion [`is_dark_rgb`], not by name: the customer picks the skin, and
/// «Light» must count as light. One place for all QML pages that need the
/// tab colors as a «dark/light» pair («Steam», «Extensions»); before that
/// each kept its own copy in the bridge.
pub fn active_skin_is_dark() -> bool {
    active_skin_colors()
        .get("bg_primary")
        .and_then(|c| parse_hex(c))
        .map(|(r, g, b)| is_dark_rgb(r, g, b))
        .unwrap_or(true)
}

/// The skin's placeholder color (`QPalette::PlaceholderText`) — widget tabs
/// paint unselectable list section headings with it. For a skin with a
/// palette the role is set explicitly; for the system skin the role stays a
/// system one, and the core's closest answer is secondary text with the
/// opacity the palette plan paints the placeholder with on a dark base
/// (`SYSTEM_PLACEHOLDER_ALPHA`). One place for all QML pages: before that the
/// page took secondary text with opacity 0.7 — its own number instead of the
/// core's.
pub fn placeholder_text_color(skin_id: &str) -> String {
    if let Some(spec) = skin_palette(skin_id) {
        if let Some((_, hex)) = spec.iter().find(|(role, _)| *role == "PlaceholderText") {
            return (*hex).to_string();
        }
    }
    let colors = skin_colors(skin_id);
    let (r, g, b) = colors
        .get("text_secondary")
        .and_then(|c| parse_hex(c))
        .unwrap_or((255, 255, 255));
    format!("#{SYSTEM_PLACEHOLDER_ALPHA:02x}{r:02x}{g:02x}{b:02x}")
}

/// The same for the EFFECTIVE skin.
pub fn active_placeholder_text_color() -> String {
    placeholder_text_color(active_skin_id())
}

// ============================================================================
// Application palette: assignment plan
// ============================================================================
//
// The QPalette itself is assembled by the Qt executor, but WHAT to write
// into it is a decision, and it lives here: which roles, in which groups,
// with what opacity. Otherwise the rule «mute inactive text» and the forced
// white on a dark system theme would lie in Python as a second copy of the
// knowledge about skins.

/// Qt palette groups the skin writes colors into.
pub const PALETTE_GROUPS: &[&str] = &["Active", "Inactive", "Disabled"];

/// Text roles muted for the inactive (disabled) state.
pub const TEXT_ROLES: &[&str] = &["WindowText", "Text", "ButtonText"];

/// Opacity of muted text: a readable indication of inactivity.
pub const DISABLED_TEXT_ALPHA: u8 = 140;

/// Placeholder opacity with forced white on a dark system theme.
pub const SYSTEM_PLACEHOLDER_ALPHA: u8 = 190;

/// One color assignment in the palette: group, role, color with opacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteAssign {
    pub group: &'static str,
    pub role: &'static str,
    pub rgba: (u8, u8, u8, u8),
}

/// Parse `#rgb` or `#rrggbb` into channels. Other forms (named colors,
/// `#aarrggbb`) are deliberately unsupported: skin tables do not have them,
/// and the guard [`skin_palettes_parse`] makes sure they do not appear.
pub fn parse_hex(value: &str) -> Option<(u8, u8, u8)> {
    let s = value.strip_prefix('#')?;
    let d = |c: char| c.to_digit(16).map(|v| v as u8);
    let cs: Vec<char> = s.chars().collect();
    match cs.len() {
        3 => {
            let (r, g, b) = (d(cs[0])?, d(cs[1])?, d(cs[2])?);
            Some((r * 17, g * 17, b * 17))
        }
        6 => {
            let hx = |a: char, b: char| Some(d(a)? * 16 + d(b)?);
            Some((hx(cs[0], cs[1])?, hx(cs[2], cs[3])?, hx(cs[4], cs[5])?))
        }
        _ => None,
    }
}

/// Lighten a color by mixing in white — the shared trick for hovering over
/// an element in the accent color.
///
/// Skins have no «hovered» shade of their own, and adding it as four manual
/// values would mean a fifth skin forgets it again. `k` — the share of white,
/// 0.0 leaves the color as is, 1.0 gives white; an unparsed color is returned
/// unchanged, so that hover does not fade the button into nothing.
pub fn lighten_hex(value: &str, k: f64) -> String {
    let Some((r, g, b)) = parse_hex(value) else {
        return value.to_string();
    };
    let k = k.clamp(0.0, 1.0);
    let mix = |c: u8| -> u8 {
        (f64::from(c) + (255.0 - f64::from(c)) * k).round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", mix(r), mix(g), mix(b))
}

/// What the skin writes into the application palette on top of the system one.
///
/// `base_is_dark` — whether the palette is dark the way the system handed it
/// over at startup (an executor observation; see [`is_dark_rgb`]). The order
/// of assignments matters: the last one for a role and group wins.
///
/// The «System» skin (without its own palette) imposes nothing except the
/// historical forced white text on a dark system theme: Breeze Dark gives a
/// greyish `WindowText`, and labels blended into the background. On a light
/// system theme the plan is empty — the palette stays as the system gave it.
pub fn palette_plan(skin_id: &str, base_is_dark: bool) -> Vec<PaletteAssign> {
    let mut out: Vec<PaletteAssign> = Vec::new();
    let mut push = |group: &'static str, role: &'static str, rgba| {
        out.push(PaletteAssign { group, role, rgba })
    };
    match skin(skin_id).palette {
        None => {
            if !base_is_dark {
                return out;
            }
            for role in TEXT_ROLES {
                push("Active", role, (255, 255, 255, 255));
                push("Inactive", role, (255, 255, 255, 255));
                push("Disabled", role, (255, 255, 255, DISABLED_TEXT_ALPHA));
            }
            // The placeholder is set in all groups at once (in Qt that is the
            // `setColor(role, color)` form without a group).
            for group in PALETTE_GROUPS {
                push(
                    group,
                    "PlaceholderText",
                    (255, 255, 255, SYSTEM_PLACEHOLDER_ALPHA),
                );
            }
        }
        Some(spec) => {
            for (role, hex) in spec {
                let Some((r, g, b)) = parse_hex(hex) else {
                    continue;
                };
                for group in PALETTE_GROUPS {
                    push(group, role, (r, g, b, 255));
                }
            }
            // Unset roles stay system ones — the skin lists only what it
            // changes.
            let win = spec
                .iter()
                .find(|(k, _)| *k == "WindowText")
                .map(|(_, v)| *v)
                .unwrap_or("#ffffff");
            let (r, g, b) = parse_hex(win).unwrap_or((255, 255, 255));
            for role in TEXT_ROLES {
                push("Disabled", role, (r, g, b, DISABLED_TEXT_ALPHA));
            }
        }
    }
    out
}

// ============================================================================
// Pop-up toast
// ============================================================================
//
// Where and how big the plate should be is arithmetic, and all of it is
// here: Qt only measures the text and moves the widget. Before the port the
// calculation lay in Python inside `_reposition`, and each of its edges («not
// above the visible area», «not narrower than 160») was a separate line
// without a test.

/// Toast margin from the parent's edges.
pub const TOAST_MARGIN: i32 = 12;
/// How long a toast stays by default, milliseconds (0 — until dismissed by hand).
pub const TOAST_TIMEOUT_MS: u32 = 5000;
/// How many ms per character we give to read a long toast beyond the usual
/// time. An adult reads about 15 characters a second; 60 ms leaves a margin.
pub const TOAST_READ_MS_PER_CHAR: u32 = 60;
/// An informational toast does not stay longer than this even with a long
/// text: the plate lies over buttons and must not cover them for long.
pub const TOAST_MAX_TIMEOUT_MS: u32 = 12_000;

/// Lifetime of an informational toast with this text, ms.
///
/// A short one gets the shared [`TOAST_TIMEOUT_MS`], like all Rexon plates; a
/// long one gets time to be read (characters × [`TOAST_READ_MS_PER_CHAR`]
/// plus a second to notice it), but no more than [`TOAST_MAX_TIMEOUT_MS`].
/// Never 0: zero means «stay until closed», and that is reserved for errors
/// the person can do something about (defect 30.10.2026 — a message about a
/// wallpaper rollback hung for over a minute over the filter).
pub fn toast_timeout_for_text(text: &str) -> u32 {
    let chars = u32::try_from(text.chars().count()).unwrap_or(u32::MAX);
    let read = chars.saturating_mul(TOAST_READ_MS_PER_CHAR).saturating_add(1000);
    read.clamp(TOAST_TIMEOUT_MS, TOAST_MAX_TIMEOUT_MS)
}

/// Minimum width below which the plate is not squeezed.
pub const TOAST_MIN_WIDTH: i32 = 160;
/// Maximum plate width: a long answer wraps rather than stretching the toast
/// across the whole page.
pub const TOAST_MAX_WIDTH: i32 = 520;
/// Margin added to the measured text width, points.
///
/// ⚠️ Not a matter of taste. Both `QTextDocument::idealWidth` of a widget
/// label and `implicitWidth` of formatted text in QML return a FRACTIONAL
/// number, and it enters the calculation as an integer (by truncation).
/// Without the margin a line that fit entirely wrapped onto a second one:
/// measured 08.09.2026 — the text «✅ Panel copy saved» measured 173.59, the
/// plate came out at 173, and the last word moved down.
pub const TOAST_TEXT_SLACK: i32 = 4;
/// Minimum height of the parent's visible area we count on.
pub const TOAST_MIN_VISIBLE_HEIGHT: i32 = 48;
/// Minimum height of the plate itself.
pub const TOAST_MIN_HEIGHT: i32 = 24;

/// Plate width: the width of the text in one line, but no wider than the
/// maximum and no wider than the parent minus margins.
///
/// `ideal_w` — the width of the text in one line, `hint_w` — the widget's
/// hint (for a wrapping label it is «square», so it alone is not enough).
pub fn toast_width(parent_w: i32, ideal_w: i32, hint_w: i32, max_w: i32) -> i32 {
    let avail = (parent_w - 2 * TOAST_MARGIN).max(TOAST_MIN_WIDTH);
    ideal_w.max(hint_w).min(max_w).min(avail)
}

/// Height of the VISIBLE part of the parent.
///
/// `chain` — a chain of `(node offset inside the parent, parent height)` from
/// the toast's parent up to the window. Counting by one parent's height is
/// not possible: a tab can be taller than the space allotted to it (its
/// window enforces its minimum size), the bottom pixels are clipped, and a
/// two-line toast went off the window's edge.
pub fn toast_visible_height(parent_h: i32, chain: &[(i32, i32)]) -> i32 {
    let mut view = parent_h;
    let mut offset = 0;
    for (node_y, parent_height) in chain {
        offset += node_y;
        view = view.min(parent_height - offset);
    }
    view.max(TOAST_MIN_VISIBLE_HEIGHT)
}

/// Position and height of the plate: `(x, y, height)`.
///
/// `height_for_width` — the text height at the chosen width (Qt measures it
/// taking line wrapping into account); a non-positive value means «nothing
/// to measure with», then the widget's hint `hint_h` is taken. A long text is
/// better clipped than moving the toast past the window border, so the
/// height is clamped by the visible area.
pub fn toast_placement(visible_h: i32, height_for_width: i32, hint_h: i32) -> (i32, i32, i32) {
    let mut h = if height_for_width > 0 {
        height_for_width
    } else {
        hint_h
    };
    h = h.min((visible_h - 2 * TOAST_MARGIN).max(TOAST_MIN_HEIGHT));
    (
        TOAST_MARGIN,
        TOAST_MARGIN.max(visible_h - h - TOAST_MARGIN),
        h,
    )
}

// ============================================================================
// Qt messages to the log
// ============================================================================
//
// ⛔ There is no Python message handler here and there cannot be one:
// PySide6 takes the GIL in the thread that prints, and Qt also prints from
// QQmlThread — the application deadlocks. The stock Qt C++ handler prints;
// we only set its format and destination through environment variables.

/// Qt message format — 1:1 the line format of the shared Rexon log
/// (`crate::applog::format_line`: «time.ms [process] LEVEL area: text», the
/// level exactly five characters), so that Qt lines do not stand out in the
/// file. The area is `qt`; the Qt category (`qt.qml`, `qt.quick`…) goes
/// before the text if set. The process number is mandatory: the window and
/// the wallpaper host write to the same file, and Qt lines could not be told
/// apart otherwise.
pub const QT_MESSAGE_PATTERN: &str = "%{time yyyy-MM-dd hh:mm:ss.zzz} [%{pid}] \
     %{if-debug}DEBUG%{endif}%{if-info}INFO %{endif}\
     %{if-warning}WARN %{endif}%{if-critical}ERROR%{endif}\
     %{if-fatal}ERROR%{endif} qt: \
     %{if-category}%{category}: %{endif}%{message}";

/// Environment variables that route Qt into the Rexon log.
///
/// `QT_FORCE_STDERR_LOGGING` is needed because on many distributions (CachyOS
/// among them) Qt is built with journald and by default sends messages to the
/// system journal bypassing stderr.
pub fn qt_logging_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("QT_FORCE_STDERR_LOGGING", "1"),
        ("QT_MESSAGE_PATTERN", QT_MESSAGE_PATTERN),
    ]
}

/// Whether to redirect descriptor 2 into the log file.
///
/// Yes — only when launched without a console (a «Start» menu shortcut, a
/// systemd unit): stderr there is either a journald socket or the character
/// device `/dev/null`. A terminal, a file and a pipe are an explicit choice of
/// whoever launched it; we do not intercept them, otherwise a developer would
/// stop seeing messages where they expect them.
pub fn should_redirect_stderr(
    stderr_is_tty: bool,
    fd_is_socket: bool,
    fd_is_chardev: bool,
    fd_is_tty: bool,
) -> bool {
    if stderr_is_tty {
        return false;
    }
    fd_is_socket || (fd_is_chardev && !fd_is_tty)
}

// ============================================================================
// Network failures in human language
// ============================================================================

/// A raw network error → a phrase the customer understands (rule #13).
///
/// Showing a person `<urlopen error https://aur.archlinux.org/rpc/v5/...:
/// Network Error: Unexpected EOF>` is not allowed: they see brackets, a URL
/// and English jargon and decide the program broke. The technical line
/// belongs in the log, and on screen — what happened and what to do about it.
///
/// `source` — the human name of the catalog («AUR», «KDE Store»), so it is
/// clear WHAT exactly is unavailable: other sections may be working. The
/// store name is not inflected; the word «catalog» is.
pub fn humanize_network_error(raw: &str, source: &str) -> String {
    let low = raw.to_lowercase();
    // Whole phrases, two per cause — with the catalog name and without: the
    // grammatical case of the word «catalog» depends on its place in the
    // phrase, and gluing it from pieces would force Russian word order on
    // all languages.
    let named = !source.is_empty();

    const DNS: &[&str] = &[
        "name or service not known",
        "nodename nor servname",
        "temporary failure in name resolution",
        "getaddrinfo",
        "no address associated",
    ];
    const BROKEN: &[&str] = &[
        "unexpected eof",
        "connection reset",
        "connection refused",
        "connection aborted",
        "ssl",
        "tls",
        "certificate",
    ];

    if low.contains("timed out") || low.contains("timeout") {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ The %1 catalog didn't respond in time. The service may be overloaded — try again.", &[&source])
        } else {
            tr("⚠️ The catalog didn't respond in time. The service may be overloaded — try again.")
        };
    }
    if DNS.iter().any(|s| low.contains(s)) {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ Couldn't find the address of the %1 catalog. Check your internet connection and DNS settings.", &[&source])
        } else {
            tr("⚠️ Couldn't find the catalog's address. Check your internet connection and DNS settings.")
        };
    }
    if BROKEN.iter().any(|s| low.contains(s)) {
        return if named {
            trf(
                // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
                "⚠️ The connection to the %1 catalog was interrupted. This happens when the \
                 connection drops, or when access to it is blocked by a firewall, VPN or \
                 internet provider.",
                &[&source],
            )
        } else {
            tr("⚠️ The connection to the catalog was interrupted. This happens when the \
                connection drops, or when access to it is blocked by a firewall, VPN or \
                internet provider.")
        };
    }
    if low.contains("unreachable") || low.contains("no route to host") {
        return if named {
            // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
            trf("⚠️ The network is unreachable — couldn't contact the %1 catalog.", &[&source])
        } else {
            tr("⚠️ The network is unreachable — couldn't contact the catalog.")
        };
    }
    if let Some(code) = http_status_code(raw) {
        return if named {
            // TRANSLATORS: %1 is a catalog name (AUR, KDE Store…), %2 an HTTP status code.
            trf("⚠️ The %1 catalog returned error %2. This is most likely temporary — try again later.", &[&source, &code])
        } else {
            // TRANSLATORS: %1 is an HTTP status code.
            trf("⚠️ The catalog returned error %1. This is most likely temporary — try again later.", &[&code])
        };
    }
    if named {
        // TRANSLATORS: %1 is a catalog name: AUR, KDE Store…
        trf("⚠️ Couldn't contact the %1 catalog. Check your internet connection and try again.", &[&source])
    } else {
        tr("⚠️ Couldn't contact the catalog. Check your internet connection and try again.")
    }
}

/// The three-digit HTTP response code in the raw error text: «HTTP Error
/// 503: ...», «status 429», «status code 429».
///
/// Port of the regex `\b(?:HTTP\s*Error|status(?:\s*code)?)\s*[:=]?\s*(\d{3})\b`
/// without a regex engine. Word boundaries are computed as in Python: a
/// letter of any alphabet, a digit and an underscore are part of a word.
pub fn http_status_code(raw: &str) -> Option<String> {
    let cs: Vec<char> = raw.chars().collect();
    let n = cs.len();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    // Case-insensitive keyword; returns the position after it.
    let eat = |mut i: usize, kw: &str| -> Option<usize> {
        for k in kw.chars() {
            if i >= n || cs[i].to_lowercase().next() != Some(k) {
                return None;
            }
            i += 1;
        }
        Some(i)
    };
    let spaces = |mut i: usize| -> usize {
        while i < n && cs[i].is_whitespace() {
            i += 1;
        }
        i
    };
    for start in 0..n {
        if start > 0 && word(cs[start - 1]) {
            continue; // not at a word boundary
        }
        // Prefix variants; for «status» we first try with «code» (like the
        // regex's greedy group), then without it.
        let mut heads: Vec<usize> = Vec::new();
        if let Some(i) = eat(start, "http") {
            if let Some(j) = eat(spaces(i), "error") {
                heads.push(j);
            }
        }
        if let Some(i) = eat(start, "status") {
            if let Some(j) = eat(spaces(i), "code") {
                heads.push(j);
            }
            heads.push(i);
        }
        for head in heads {
            let mut i = spaces(head);
            if i < n && (cs[i] == ':' || cs[i] == '=') {
                i += 1;
            }
            i = spaces(i);
            if i + 3 > n || !cs[i..i + 3].iter().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if i + 3 < n && word(cs[i + 3]) {
                continue; // «5031» is not a response code
            }
            return Some(cs[i..i + 3].iter().collect());
        }
    }
    None
}

// ============================================================================
// Adaptive card grid
// ============================================================================
//
// How many fixed-width cards fit in a strip is pure arithmetic, and it is one
// for all tabs with grids (rule #12). Previously each tab computed it its own
// way: «Appearance» divided the width by the card width, «Extensions» — in
// steps by window width, the Steam tab kept the column count a constant
// altogether. Observations (viewport width, card width, spacing) come in as
// parameters — Qt is not visible from here.

/// How many columns of cards `card_width` wide fit in the strip
/// `viewport_width` with spacing `spacing` and total margins `margins`.
///
/// The result is clamped to `[min_cols, max_cols]` and is never less than one:
/// an empty grid is useless, and a card wider than the strip is shown anyway
/// (the scrollbar clips it, but the user sees the contents).
///
/// Degenerate observations are not counted as an error — the window asks for
/// the width even before it is first shown, when there is none yet:
/// * `viewport_width <= 0` (the window is not laid out) → `min_cols`;
/// * `card_width <= 0` (a card without a size) → `max_cols`, because there is
///   nothing to divide by, and pinning to one column is surely wrong.
pub fn grid_columns(
    viewport_width: i32,
    card_width: i32,
    spacing: i32,
    margins: i32,
    min_cols: u32,
    max_cols: u32,
) -> u32 {
    let lo = min_cols.max(1);
    let hi = max_cols.max(lo);
    if card_width <= 0 {
        return hi;
    }
    if viewport_width <= 0 {
        return lo;
    }
    let gap = spacing.max(0) as i64;
    let usable = (viewport_width as i64) - (margins.max(0) as i64);
    if usable < card_width as i64 {
        return lo;
    }
    // n cards fit if n*card + (n-1)*gap <= usable.
    let fit = (usable + gap) / (card_width as i64 + gap);
    (fit.clamp(lo as i64, hi as i64)) as u32
}

/// Grid page size with `cols` columns and `rows` rows.
///
/// Pagination is set by rows, not by the number of items: otherwise with five
/// columns a page of 25 items is exactly five rows, and with four — six rows
/// and a one-item «tail».
pub fn grid_page_size(cols: u32, rows: u32) -> u32 {
    cols.max(1) * rows.max(1)
}

/// Page number (from zero) that holds element `index` at page size
/// `page_size`.
///
/// Needed when the column count changes: the page changes size, and so the
/// user does not lose their place, the number is recomputed from the index of
/// the first element of the previous page.
pub fn grid_page_for_index(index: u32, page_size: u32) -> u32 {
    index / page_size.max(1)
}

/// How many pages in total for `total` elements at page size `page_size`. An
/// empty list is one (empty) page, so that the label «Page 1 / 1» does not
/// turn into «1 / 0».
pub fn grid_page_count(total: u32, page_size: u32) -> u32 {
    let size = page_size.max(1);
    if total == 0 {
        1
    } else {
        (total + size - 1) / size
    }
}

#[cfg(test)]
mod grid_tests {
    use super::*;

    #[test]
    fn fits_by_exact_arithmetic() {
        // 3 cards of 180 + 2 gaps of 12 = 564, plus margins 16 = 580.
        assert_eq!(grid_columns(580, 180, 12, 16, 1, 12), 3);
        // One pixel less — the third no longer fits.
        assert_eq!(grid_columns(579, 180, 12, 16, 1, 12), 2);
        // One pixel more — still three, the fourth needs another 192.
        assert_eq!(grid_columns(581, 180, 12, 16, 1, 12), 3);
        // Exactly at the fourth's boundary: 4*180 + 3*12 + 16 = 772.
        assert_eq!(grid_columns(772, 180, 12, 16, 1, 12), 4);
    }

    #[test]
    fn zero_and_negative_width_give_minimum() {
        assert_eq!(grid_columns(0, 180, 12, 16, 1, 12), 1);
        assert_eq!(grid_columns(-1000, 180, 12, 16, 1, 12), 1);
        assert_eq!(grid_columns(0, 180, 12, 16, 2, 12), 2);
    }

    #[test]
    fn card_wider_than_viewport_still_shows_one() {
        assert_eq!(grid_columns(100, 180, 12, 16, 1, 12), 1);
        // A card exactly the strip width without margins — already fits.
        assert_eq!(grid_columns(180, 180, 12, 0, 1, 12), 1);
        // ...and with margins it does not, but there is still one column.
        assert_eq!(grid_columns(180, 180, 12, 16, 1, 12), 1);
    }

    #[test]
    fn degenerate_card_width_gives_maximum() {
        assert_eq!(grid_columns(900, 0, 12, 16, 1, 6), 6);
        assert_eq!(grid_columns(900, -50, 12, 16, 1, 6), 6);
    }

    #[test]
    fn clamped_to_range() {
        assert_eq!(grid_columns(4000, 180, 12, 16, 1, 5), 5);
        assert_eq!(grid_columns(300, 180, 12, 16, 3, 8), 3);
        // Swapped bounds do not break the calculation: max is pulled up to min.
        assert_eq!(grid_columns(4000, 180, 12, 16, 4, 2), 4);
        // A zero minimum means «at least one».
        assert_eq!(grid_columns(0, 180, 12, 16, 0, 5), 1);
    }

    #[test]
    fn zero_spacing_is_plain_division() {
        assert_eq!(grid_columns(900, 180, 0, 0, 1, 99), 5);
        assert_eq!(grid_columns(899, 180, 0, 0, 1, 99), 4);
        // Negative spacing is treated as zero.
        assert_eq!(grid_columns(900, 180, -20, 0, 1, 99), 5);
    }

    #[test]
    fn page_size_follows_columns() {
        assert_eq!(grid_page_size(5, 5), 25);
        assert_eq!(grid_page_size(4, 5), 20);
        assert_eq!(grid_page_size(0, 5), 5);
        assert_eq!(grid_page_size(7, 0), 7);
    }

    #[test]
    fn page_recomputed_from_first_visible_item() {
        // It was page 3 of 25 (elements 50..74) — the first element is 50.
        // With four columns the page became 20, element 50 landed on page 2.
        assert_eq!(grid_page_for_index(50, 20), 2);
        // Back to five columns — element 40 (the first of the second page).
        assert_eq!(grid_page_for_index(40, 25), 1);
        assert_eq!(grid_page_for_index(0, 25), 0);
        assert_eq!(grid_page_for_index(7, 0), 7);
    }

    #[test]
    fn page_count_never_zero() {
        assert_eq!(grid_page_count(0, 25), 1);
        assert_eq!(grid_page_count(25, 25), 1);
        assert_eq!(grid_page_count(26, 25), 2);
        assert_eq!(grid_page_count(202, 25), 9);
        assert_eq!(grid_page_count(202, 20), 11);
        assert_eq!(grid_page_count(10, 0), 10);
    }
}

#[cfg(test)]
mod skin_extras_tests {
    use super::*;

    #[test]
    fn skin_palettes_parse() {
        // Format guard: only #rgb/#rrggbb are allowed in the tables, otherwise
        // the role would silently drop out of the plan and the color stay system.
        for s in SKINS {
            if let Some(spec) = s.palette {
                for (role, hex) in spec {
                    assert!(
                        parse_hex(hex).is_some(),
                        "скин {}: роль {role} задана как {hex}",
                        s.id
                    );
                }
            }
        }
    }

    #[test]
    fn hex_parsed_both_forms() {
        assert_eq!(parse_hex("#1b2838"), Some((27, 40, 56)));
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("#0d1b2a"), Some((13, 27, 42)));
        assert_eq!(parse_hex("1b2838"), None);
        assert_eq!(parse_hex("#xyzxyz"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn dark_criterion_is_channel_average() {
        assert!(is_dark_rgb(0x1b, 0x28, 0x38)); // steam_dark
        assert!(is_dark_rgb(0x1a, 0x05, 0x33)); // synthwave
        assert!(!is_dark_rgb(0xf4, 0xf5, 0xf6)); // light
        // Exactly at the threshold: 128,128,128 — average 128, NOT dark anymore.
        assert!(!is_dark_rgb(128, 128, 128));
        assert!(is_dark_rgb(127, 128, 128));
        // Saturated yellow — where the two former criteria diverged.
        assert!(!is_dark_rgb(255, 255, 0));
    }

    /// Every skin with its own palette sets the link color, and it is
    /// distinguishable from the background: otherwise links in formatted text
    /// come out as the system `#0000ff` (defect 01.11.2026 on «Dark (Steam)»).
    #[test]
    fn every_palette_skin_has_readable_link() {
        assert_eq!(skin_link_rgb("system"), None);
        for s in SKINS.iter().filter(|s| s.palette.is_some()) {
            let link = skin_link_rgb(s.id).unwrap_or_else(|| panic!("{}: нет роли Link", s.id));
            let bg = skin_colors(s.id)
                .get("bg_primary")
                .and_then(|c| parse_hex(c))
                .unwrap_or_else(|| panic!("{}: нет bg_primary", s.id));
            assert_ne!(
                is_dark_rgb(link.0, link.1, link.2),
                is_dark_rgb(bg.0, bg.1, bg.2),
                "{}: ссылка {link:?} сливается с фоном {bg:?}",
                s.id
            );
        }
    }

    #[test]
    fn system_skin_forces_white_only_on_dark() {
        assert!(palette_plan("system", false).is_empty());
        let plan = palette_plan("system", true);
        // Three text roles × three groups + the placeholder in three groups.
        assert_eq!(plan.len(), 12);
        let dis: Vec<_> = plan
            .iter()
            .filter(|a| a.group == "Disabled" && a.role == "Text")
            .collect();
        assert_eq!(dis.len(), 1);
        assert_eq!(dis[0].rgba, (255, 255, 255, DISABLED_TEXT_ALPHA));
        let ph: Vec<_> = plan
            .iter()
            .filter(|a| a.role == "PlaceholderText")
            .collect();
        assert_eq!(ph.len(), 3);
        assert_eq!(ph[0].rgba, (255, 255, 255, SYSTEM_PLACEHOLDER_ALPHA));
    }

    #[test]
    fn explicit_skin_writes_all_groups_and_dims_disabled() {
        let plan = palette_plan("steam_dark", true);
        let spec = skin_palette("steam_dark").unwrap();
        assert_eq!(plan.len(), spec.len() * 3 + TEXT_ROLES.len());
        // Window — in all three groups exactly the color from the table.
        let win: Vec<_> = plan.iter().filter(|a| a.role == "Window").collect();
        assert_eq!(win.len(), 3);
        assert!(win.iter().all(|a| a.rgba == (0x1b, 0x28, 0x38, 255)));
        // Muted text — last, over the entry from the table.
        let last = plan
            .iter()
            .rev()
            .find(|a| a.group == "Disabled" && a.role == "ButtonText")
            .unwrap();
        assert_eq!(last.rgba, (255, 255, 255, DISABLED_TEXT_ALPHA));
        assert_eq!(plan.last().unwrap().role, "ButtonText");
        // The light skin mutes with ITS OWN text color, not white.
        let light = palette_plan("light", true);
        let dim = light
            .iter()
            .rev()
            .find(|a| a.group == "Disabled" && a.role == "Text")
            .unwrap();
        assert_eq!(dim.rgba, (0x1f, 0x23, 0x26, DISABLED_TEXT_ALPHA));
        // Unknown skin — as everywhere, fall back to the default.
        assert_eq!(palette_plan("нет-такого", true), plan);
    }

    #[test]
    fn skin_extras_only_where_declared() {
        assert_eq!(skin_glow("synthwave"), Some(("#00f0ff", 18)));
        assert_eq!(skin_glow("steam_dark"), None);
        assert_eq!(
            skin_heading_font("synthwave"),
            Some("Tektur-VariableFont_wdth_wght.ttf")
        );
        assert_eq!(skin_heading_font("system"), None);
        assert_eq!(skin_indicator_check_color("synthwave"), Some("#0d1b2a"));
        assert_eq!(skin_indicator_check_color("light"), None);
        assert_eq!(heading_default_outline(), ("#ff2d95", "#eaf6ff", 2));
        // The check mark is needed exactly by the skins that have an overlay.
        for s in SKINS {
            assert_eq!(s.has_qss, s.check_color.is_some(), "скин {}", s.id);
        }
    }

    /// An informational toast always goes out: short — the shared time, long —
    /// time to read, but no longer than the limit.
    #[test]
    fn info_toast_timeout_reads_and_never_sticks() {
        assert_eq!(toast_timeout_for_text(""), TOAST_TIMEOUT_MS);
        assert_eq!(toast_timeout_for_text("Сохранено"), TOAST_TIMEOUT_MS);
        let revert = "The live wallpaper could not start on this computer, so Rexon put your \
                      previous desktop wallpaper back.";
        let t = toast_timeout_for_text(revert);
        assert!(t > TOAST_TIMEOUT_MS && t < TOAST_MAX_TIMEOUT_MS, "{t}");
        assert_eq!(toast_timeout_for_text(&"ж".repeat(10_000)), TOAST_MAX_TIMEOUT_MS);
    }

    #[test]
    fn toast_width_clamped_by_parent_and_maximum() {
        // The usual case: one-line text shorter than both the maximum and the strip.
        assert_eq!(toast_width(900, 400, 300, 520), 400);
        // The widget hint is wider than the ideal width — take it.
        assert_eq!(toast_width(900, 200, 300, 520), 300);
        // The maximum cuts.
        assert_eq!(toast_width(900, 800, 300, 520), 520);
        // A narrow window cuts more than the maximum.
        assert_eq!(toast_width(300, 800, 300, 520), 276);
        // A very narrow window — not narrower than the minimum.
        assert_eq!(toast_width(50, 800, 300, 520), TOAST_MIN_WIDTH);
    }

    #[test]
    fn toast_visible_height_takes_the_smallest_ancestor() {
        // One parent — its height.
        assert_eq!(toast_visible_height(400, &[]), 400);
        // The tab is taller than its allotted space: parent 800, the window gives 500.
        assert_eq!(toast_visible_height(800, &[(0, 500)]), 500);
        // The node offset inside the parent is subtracted.
        assert_eq!(toast_visible_height(800, &[(40, 500)]), 460);
        // The chain adds up offsets.
        assert_eq!(toast_visible_height(800, &[(40, 700), (10, 500)]), 450);
        // Degenerate window — not below the minimum.
        assert_eq!(toast_visible_height(10, &[]), TOAST_MIN_VISIBLE_HEIGHT);
        assert_eq!(toast_visible_height(800, &[(0, 0)]), TOAST_MIN_VISIBLE_HEIGHT);
    }

    #[test]
    fn toast_placement_keeps_plate_inside_window() {
        // A two-line toast in a normal window.
        assert_eq!(toast_placement(400, 56, 30), (12, 332, 56));
        // Nothing to measure with — take the widget hint.
        assert_eq!(toast_placement(400, 0, 30), (12, 358, 30));
        assert_eq!(toast_placement(400, -1, 30), (12, 358, 30));
        // Text taller than the visible area: clip it rather than going off the edge.
        let (x, y, h) = toast_placement(100, 400, 30);
        assert_eq!((x, y, h), (12, 12, 76));
        assert!(y + h + TOAST_MARGIN <= 100);
        // A very flat area — the plate is not below the minimum, but not above the edge either.
        assert_eq!(toast_placement(40, 400, 30), (12, 12, TOAST_MIN_HEIGHT));
    }

    #[test]
    fn stderr_redirected_only_without_console() {
        // A «Start» menu shortcut: /dev/null is a character device, not a terminal.
        assert!(should_redirect_stderr(false, false, true, false));
        // systemd unit: a journald socket.
        assert!(should_redirect_stderr(false, true, false, false));
        // Terminal — not intercepted.
        assert!(!should_redirect_stderr(true, false, true, false));
        assert!(!should_redirect_stderr(false, false, true, true));
        // A file or a pipe (neither a socket nor a device) — an explicit choice of whoever launched it.
        assert!(!should_redirect_stderr(false, false, false, false));
    }

    #[test]
    fn qt_logging_env_is_pattern_and_stderr() {
        let env = qt_logging_env();
        assert_eq!(env.len(), 2);
        assert_eq!(env[0], ("QT_FORCE_STDERR_LOGGING", "1"));
        assert_eq!(env[1].0, "QT_MESSAGE_PATTERN");
        assert!(env[1].1.starts_with("%{time yyyy-MM-dd hh:mm:ss.zzz} [%{pid}] "));
        assert!(env[1].1.contains("%{endif} qt: "));
        assert!(env[1].1.ends_with("%{if-category}%{category}: %{endif}%{message}"));
        assert!(!env[1].1.contains('\n'));
    }

    #[test]
    fn network_error_classified_by_cause() {
        // Source language: the Russian text is guarded by `tests/ui_base_parity.rs`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let t = |s: &str| humanize_network_error(s, "AUR");
        assert!(t("<urlopen error timed out>").starts_with("⚠️ The AUR catalog didn't respond"));
        assert!(t("TIMEOUT while reading").contains("didn't respond in time"));
        assert!(t("Name or service not known").contains("DNS settings"));
        assert!(t("getaddrinfo failed").contains("DNS settings"));
        assert!(t("[SSL: CERTIFICATE_VERIFY_FAILED]").contains("was interrupted"));
        assert!(t("Network Error: Unexpected EOF").contains("was interrupted"));
        assert!(t("Network is unreachable").contains("network is unreachable"));
        assert!(t("No route to host").contains("network is unreachable"));
        assert!(t("HTTP Error 503: Service Unavailable").contains("returned error 503"));
        assert!(t("status code 429").contains("returned error 429"));
        assert!(t("нечто непонятное").contains("Couldn't contact the AUR catalog"));
        // Without a catalog name — a phrase of its own, not a hole where the name was.
        assert!(humanize_network_error("", "").contains("contact the catalog."));
        assert!(humanize_network_error("timed out", "").starts_with("⚠️ The catalog didn't respond"));
    }

    #[test]
    fn http_code_found_like_the_original_regex() {
        assert_eq!(http_status_code("HTTP Error 503: nope").as_deref(), Some("503"));
        assert_eq!(http_status_code("http error 404").as_deref(), Some("404"));
        // The space between words is optional and may be double — like the
        // original's regex (`HTTP\s*Error`).
        assert_eq!(http_status_code("HTTPError: 500").as_deref(), Some("500"));
        assert_eq!(http_status_code("HTTP  Error  301").as_deref(), Some("301"));
        assert_eq!(http_status_code("status 429").as_deref(), Some("429"));
        assert_eq!(http_status_code("status code 429").as_deref(), Some("429"));
        assert_eq!(http_status_code("status=418").as_deref(), Some("418"));
        // Four digits are not a response code (the original has `\d{3}\b`).
        assert_eq!(http_status_code("HTTP Error 5031").as_deref(), None);
        // Two digits are not enough.
        assert_eq!(http_status_code("status 42").as_deref(), None);
        // The word must start at a boundary.
        assert_eq!(http_status_code("substatus 429").as_deref(), None);
        // «codes» is not «code»: there are no digits after it, and falling back to
        // the short variant does not find a code either (checked on the original's
        // regex).
        assert_eq!(http_status_code("status codes 429").as_deref(), None);
        // Russian text around does not interfere and does not break word boundaries.
        assert_eq!(
            http_status_code("ответ сервера: HTTP Error 502 — плохо").as_deref(),
            Some("502")
        );
        assert_eq!(http_status_code("ошибка без кода").as_deref(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_blocks_read_left_to_right() {
        assert_eq!(pre_left_to_right("<p>a</p><pre>\n{ }\n</pre>"), "<p>a</p><pre dir=\"ltr\" align=\"left\">\n{ }\n</pre>");
        assert_eq!(pre_left_to_right("<PRE class=\"cmd\">x</PRE>"), "<PRE dir=\"ltr\" align=\"left\" class=\"cmd\">x</PRE>");
        // Already with a direction, foreign tags starting with «pre» and text without tags — as is.
        for same in ["<pre dir=\"rtl\">x</pre>", "<prefix>x</prefix>", "<preview/>", "текст", "", "<pre"] {
            assert_eq!(pre_left_to_right(same), same);
        }
        // A repeated pass adds nothing.
        let once = pre_left_to_right("<pre>a</pre><pre>b</pre>");
        assert_eq!(once.matches("dir=").count(), 2);
        assert_eq!(pre_left_to_right(&once), once);
        assert_eq!(dialog_pre("x"), "<pre dir=\"ltr\" align=\"left\" style=\"white-space: pre-wrap\">x</pre>");
        assert_eq!(pre_left_to_right(&dialog_pre("x")), dialog_pre("x"));
    }
}
