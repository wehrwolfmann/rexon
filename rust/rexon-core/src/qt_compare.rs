//! Port of `rexon/qt_compare/window.py` — the "Desktop comparison" tab.
//!
//! The tab consists entirely of DATA AND DECISIONS: which pairs to offer in the
//! header, how to read the table, what to show in an expanded desktop card,
//! what color the memory bar is, what to recommend to whom, what the "X vs Y"
//! table says. None of these questions needs a widget, so the core answers
//! them, and the Qt side is left with building the cards.
//!
//! The desktops' specifications themselves do NOT live here: they already exist
//! in [`crate::system::DE_META`] (memory, load, toolkit, compositor, panel, file
//! manager, terminal, stars) — the module only reads and formats them. There is
//! no second copy of the comparison table in the project, and none may be
//! created: the buyer would notice diverging numbers in the storefront before
//! we did.
//!
//! Desktop icons ([`DE_ICONS`]) are also a single source: before the port the
//! same table existed in TWO copies, in `qt_compare/window.py` and in
//! `qt_desktops/window.py` (rule #12). Now both windows ask the core.
//!
//! Warning — fixes to the original (rule #16 — bugs are fixed during the port,
//! not preserved), each marked `Fix:` in place:
//!   1. the value "X11 (Wayland WIP)" was colored green like real Wayland: the
//!      check looked for the substring "Wayland" without regard for "WIP". The
//!      buyer was shown Budgie and LXQt as ready for Wayland;
//!   2. the highlight of the row under the cursor and the memory bar's
//!      background were hard-wired to `rgba(0,0,0,…)` in any theme — on a dark
//!      one (Rexon's default) this goes the wrong way; rule #9 requires a pair
//!      of colors;
//!   3. in the "X vs Y" dialog the install button of the FIRST desktop was
//!      highlighted while the second one was ordinary: buttons with the same
//!      meaning looked different.
//!
//! The check is `rust/rexon-core/tests/qt_compare_parity.rs` against a snapshot
//! of the live Python path (`tests/golden/qt_compare_cases.json`).

use serde::Serialize;

use crate::i18n::{n_, tr, trc, trf};
use crate::system::{CpuLoad, DeMeta, DE_META};

// ============================================================================
// Desktop icons
// ============================================================================

/// The emoji icon of each desktop: the list on the left in "Desktops", the card
/// header in "Comparison".
///
/// `cinnamon` is an alias of `cinnamon-mint`: under this name the desktop
/// arrives from the session variables, while in the package database it is
/// called by its full name.
pub const DE_ICONS: &[(&str, &str)] = &[
    ("cinnamon-mint", "🍃"),
    ("cinnamon", "🍃"),
    ("xfce", "🐭"),
    ("mate", "🦣"),
    ("gnome", "🦶"),
    ("kde", "💎"),
    ("budgie", "🐦"),
    ("pantheon", "🏛️"),
    ("deepin", "🌊"),
    ("cosmic", "🪐"),
    ("hyprland", "🪟"),
    ("lxqt", "🪶"),
    ("niri", "🎞️"),
    ("sway", "🧩"),
    ("river", "🏞️"),
    ("wayfire", "🔥"),
];

/// Icon of an unknown desktop.
pub const DE_ICON_FALLBACK: &str = "🖥️";

/// Icon of a desktop by its identifier.
pub fn de_icon(de_id: &str) -> &'static str {
    DE_ICONS
        .iter()
        .find(|(k, _)| *k == de_id)
        .map(|(_, v)| *v)
        .unwrap_or(DE_ICON_FALLBACK)
}

fn meta(de_id: &str) -> Option<&'static DeMeta> {
    DE_META.iter().find(|m| m.id == de_id)
}

// ============================================================================
// Fixed labels and sizes of the tab
// ============================================================================

// Labels are functions rather than constants: the interface language is chosen
// at start-up, and a function, unlike a constant, cannot be forgotten when
// translating.

pub fn window_title() -> String {
    tr("Rexon — Desktop Comparison")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

pub fn header_title() -> String {
    tr("📊 Desktop Comparison")
}
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
pub fn pairs_label() -> String {
    tr("Quick pairs:")
}

pub fn legend_title() -> String {
    tr("<b>How to read the table</b>")
}
pub fn legend_body() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("<b>Memory</b> — how much it takes right after login (depends on the computer)<br>\
<b>Customization ⭐</b> — from 1 (minimal) to 5 (most flexible)<br>\
<b>Gaming 🎮</b> — how well it suits games<br>\
<b>Expand a row</b> → what it is built on, what draws the windows, panel, terminal")
}

pub fn table_header() -> String {
    tr("All Desktops")
}
pub fn rec_header() -> String {
    tr("🎯 Quick Recommendations")
}
pub const SECTION_POINT_SIZE: i32 = 14;

/// Quick pairs in the header: `(button label, left, right)`.
///
/// The set is a product decision: the most frequent questions of a buyer coming
/// from Windows ("which is lighter", "which is more familiar", "which tiling
/// one"). That the identifiers exist in the desktop database is checked by
/// `qt_compare_parity.rs`.
pub const QUICK_PAIRS: &[(&str, &str, &str)] = &[
    ("XFCE ↔ KDE", "xfce", "kde"),
    ("GNOME ↔ Cinnamon", "gnome", "cinnamon-mint"),
    ("MATE ↔ Budgie", "mate", "budgie"),
    ("Hyprland ↔ Sway", "hyprland", "sway"),
];

/// Quick recommendations: `(for whom, the reasoning, which desktop to suggest)`.
///
/// The texts are catalog `msgid`s; the place that shows them translates them
/// ([`rec_rows`]).
pub const RECOMMENDATIONS: &[(&str, &str, &str)] = &[
    (
        n_("🎮 For gamers"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Minimal overhead → XFCE or KDE"),
        "xfce",
    ),
    (
        n_("💻 Low-end hardware"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Under 2 GB of RAM → MATE or XFCE"),
        "mate",
    ),
    (
        n_("🎨 For eye candy fans"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Animations and widgets → KDE Plasma"),
        "kde",
    ),
    (
        n_("🟢 For newcomers"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Simple out of the box → Cinnamon or GNOME"),
        "cinnamon-mint",
    ),
    (
        n_("🔧 For power users"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Full customization → KDE Plasma"),
        "kde",
    ),
    (
        n_("⚡ Maximum speed"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("The fastest one → XFCE"),
        "xfce",
    ),
    (
        n_("⌨️ For keyboard lovers"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Keyboard-driven window management → Hyprland"),
        "hyprland",
    ),
    (
        n_("🧩 Tiling out of the box"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Automatic window layout → Sway, niri or River"),
        "sway",
    ),
    (
        n_("🎞️ Scrollable tiling"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Windows on an endless strip → niri"),
        "niri",
    ),
    (
        n_("✨ Effects like in Compiz"),
        // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
        n_("Wobbly windows and the desktop cube → Wayfire"),
        "wayfire",
    ),
];

// ============================================================================
// Card styling
// ============================================================================

/// Style of a frame card by its `objectName`, taking the theme into account.
///
/// Resetting the frame and background is needed for both `QLabel` and nested
/// `QWidget`s: otherwise the Qt theme draws gray frames inside the card.
pub fn card_qss(object_name: &str, dark: bool) -> String {
    // The fill is the product's shared surface (`ui_base::CARD_FILL_*`), see
    // `qt_steam::card_colors`: QML gets the same paint ready-made (`surface_card`).
    let (bg, border) = if dark {
        (crate::ui_base::CARD_FILL_DARK, "rgba(255,255,255,0.12)")
    } else {
        (crate::ui_base::CARD_FILL_LIGHT, "rgba(0,0,0,0.15)")
    };
    format!(
        "QFrame#{name} {{ background-color: {bg}; \
border: 1px solid {border}; border-radius: 6px; }} \
QFrame#{name} QLabel {{ border: none; background: transparent; }} \
QFrame#{name} QWidget {{ border: none; background: transparent; }}",
        name = object_name
    )
}

/// Style of the clickable card header.
///
/// Fix: the hover highlight was `rgba(0,0,0,0.04)` in ANY theme — on a dark one
/// the row under the cursor got darker instead of lighting up (rule #9: a color
/// is chosen as a pair).
pub fn card_header_qss(dark: bool) -> String {
    let hover = if dark {
        "rgba(255,255,255,0.06)"
    } else {
        "rgba(0,0,0,0.04)"
    };
    format!(
        "QPushButton {{ text-align: left; padding: 10px 14px; \
border: none; border-radius: 6px; }}\
QPushButton:hover {{ background-color: {hover}; }}"
    )
}

/// Style of the highlighted install button (blue, with a hover highlight).
pub const INSTALL_BUTTON_QSS: &str = "QPushButton { background-color: rgba(59,130,246,0.3); \
padding: 6px 14px; border-radius: 4px; font-weight: bold; }\
QPushButton:hover { background-color: rgba(59,130,246,0.45); }";

/// The same install button paints as a bare `#AARRGGBB` color — for QML, where
/// QSS does not work: the background and the hover background. A second number
/// in the markup would split two tabs on the same data; agreement with the QSS
/// above is guarded by `qt_compare_parity::widget_qss_and_qml_colors_agree`.
pub const INSTALL_BUTTON_COLOR: &str = "#4d3b82f6";
pub const INSTALL_BUTTON_HOVER_COLOR: &str = "#733b82f6";
/// Corner radius of the same button (`border-radius: 4px` in QSS).
pub const INSTALL_BUTTON_RADIUS: i32 = 4;

// ============================================================================
// Memory bar
// ============================================================================

/// Top of the memory scale: we have no desktops heavier than a gigabyte and a
/// bit, and a fixed top makes it possible to compare bars of different cards by
/// eye.
pub const RAM_SCALE_MAX: u32 = 1200;
/// Bar height in points.
pub const RAM_BAR_HEIGHT: u32 = 8;

/// The "green → yellow" and "yellow → red" thresholds as fractions of the scale.
const RAM_GREEN_UPTO: f64 = 0.35;
const RAM_YELLOW_UPTO: f64 = 0.65;

/// Memory consumption bar: what to set on the widget.
#[derive(Debug, Clone, Serialize)]
pub struct RamBar {
    pub max: u32,
    pub value: u32,
    pub height: u32,
    pub text_visible: bool,
    pub color: &'static str,
    pub qss: String,
}

/// Bar color by memory consumption: green — light, yellow — medium, red — heavy.
pub fn ram_color(ram_mb: u32) -> &'static str {
    let frac = (ram_mb as f64 / RAM_SCALE_MAX as f64).min(1.0);
    if frac < RAM_GREEN_UPTO {
        "#22c55e"
    } else if frac < RAM_YELLOW_UPTO {
        "#eab308"
    } else {
        "#ef4444"
    }
}

/// The memory bar groove as a QSS string — for the widget tab.
const RAM_TRACK_QSS_DARK: &str = "rgba(255,255,255,0.10)";
const RAM_TRACK_QSS_LIGHT: &str = "rgba(0,0,0,0.1)";

/// The same groove as a bare `#AARRGGBB` color — for QML: QSS does not work
/// there, and a second number in the markup would split two tabs on the same
/// data. Agreement of the wrappers is guarded by
/// `qt_compare_parity::widget_qss_and_qml_colors_agree`.
pub fn ram_track_color(dark: bool) -> &'static str {
    if dark { "#1affffff" } else { "#1a000000" }
}

/// The whole memory bar.
///
/// Fix: the bar background was `rgba(0,0,0,0.1)` in any theme (rule #9) — on a
/// dark one the groove merged with the card background.
pub fn ram_bar(ram_mb: u32, dark: bool) -> RamBar {
    let color = ram_color(ram_mb);
    let track = if dark {
        RAM_TRACK_QSS_DARK
    } else {
        RAM_TRACK_QSS_LIGHT
    };
    RamBar {
        max: RAM_SCALE_MAX,
        value: ram_mb.min(RAM_SCALE_MAX),
        height: RAM_BAR_HEIGHT,
        text_visible: false,
        color,
        qss: format!(
            "QProgressBar {{ background: {track}; border-radius: 4px; }}\
QProgressBar::chunk {{ background: {color}; border-radius: 4px; }}"
        ),
    }
}

// ============================================================================
// Stars and the color of a specification value
// ============================================================================

/// A rating out of five: `n` filled icons plus empty stars up to five.
pub fn stars(n: u8, emoji: &str) -> String {
    let filled = n.min(5) as usize;
    let mut s = emoji.repeat(filled);
    s.push_str(&"☆".repeat(5 - filled));
    s
}

/// "Good" specification values — we highlight them in green.
///
/// Only technology names are here: they are not translated, so comparing their
/// text is safe. CPU load is a translatable label, and its color is decided by
/// a flag ([`cpu_load_color`]), not by the text: this list used to contain
/// «Низкое» ("Low"), and the English label would have been left without a
/// color.
const GOOD_VALUES: &[&str] = &["Qt6", "GTK4", "iced (Rust)"];

pub const GOOD_COLOR: &str = "#22c55e";
pub const BAD_COLOR: &str = "#ef4444";

/// CPU load color: low — green, high — red.
pub fn cpu_load_color(load: CpuLoad) -> &'static str {
    match load {
        CpuLoad::Low => GOOD_COLOR,
        CpuLoad::Medium => "",
        CpuLoad::High => BAD_COLOR,
    }
}

/// Color of a name value (toolkit, session, compositor…) in a specification row;
/// an empty string — do not change the color. Do not pass the load here — it has
/// [`cpu_load_color`].
///
/// The color decision is separate from the way it is applied: QSS is understood
/// only by QtWidgets, while the QML tab (`rexon-gui::compare_page`) paints the
/// value with the `color` property. Rule #12 — one engine, different wrappers.
///
/// Fix: any value containing the word "Wayland" was colored green, including
/// "X11 (Wayland WIP)" for Budgie and LXQt — to the buyer they looked ready for
/// Wayland, although the session there is X11 and Wayland is in progress.
pub fn spec_value_color(value: &str) -> &'static str {
    let wayland_ready = value.contains("Wayland") && !value.contains("WIP");
    if GOOD_VALUES.contains(&value) || wayland_ready {
        return GOOD_COLOR;
    }
    ""
}

/// Style by value color for QtWidgets; an empty string — do not change.
pub fn color_qss(color: &str) -> String {
    match color {
        "" => String::new(),
        color => format!("color: {color};"),
    }
}

/// Style of a name value in a specification row for QtWidgets; an empty string
/// — do not change.
pub fn spec_value_qss(value: &str) -> String {
    color_qss(spec_value_color(value))
}

// ============================================================================
// Card of one desktop
// ============================================================================

/// Labels of the specification rows — in the order the buyer sees them.
const SPEC_LABELS: [&str; 7] = [
    n_("🧰 Toolkit"),
    n_("🖥️ Session"),
    n_("🪟 Compositor"),
    n_("📊 Panel"),
    n_("📁 File manager"),
    n_("⌨️ Terminal"),
    n_("⚡ CPU load"),
];

/// A specification row: the label, the ready value, its color and style.
#[derive(Debug, Clone, Serialize)]
pub struct SpecRow {
    pub label: String,
    pub value: String,
    pub value_html: String,
    /// Value color as a bare `#RRGGBB` (for QML); empty — do not change.
    pub color: &'static str,
    pub qss: String,
}

/// The action button in a card: install or switch.
#[derive(Debug, Clone, Serialize)]
pub struct CardAction {
    pub de_id: String,
    /// `"install"` or `"switch"` — which signal to send.
    pub kind: &'static str,
    pub text: String,
    pub qss: &'static str,
}

/// The expanded card body: everything written and drawn in it.
#[derive(Debug, Clone, Serialize)]
pub struct CardBody {
    pub ram_label: String,
    pub ram_bar: RamBar,
    /// The «Настройка: ⭐…» ("Customization: ⭐…") and «Игры: 🎮…» ("Games: 🎮…")
    /// labels.
    pub stars_labels: Vec<String>,
    pub specs: Vec<SpecRow>,
    pub description: String,
    pub action: CardAction,
}

/// The card header line: the arrow, the icon, the name, the installed mark and
/// the short specifications on one line.
pub fn card_header(de_id: &str, expanded: bool, installed: bool) -> Option<String> {
    let arrow = if expanded { "▼" } else { "▶" };
    Some(format!("{arrow}  {}", card_header_tail(de_id, installed)?))
}

/// Card header without the expand arrow. The page puts the arrow: a collapsed
/// one points to the end of the line, and in a right-to-left language that is
/// "◀", and the core has no way to know about layout mirroring.
pub fn card_header_tail(de_id: &str, installed: bool) -> Option<String> {
    let m = meta(de_id)?;
    let mark = if installed { "  ✅" } else { "" };
    Some(format!(
        "{icon}  {name}{mark}   ·   {mem_word} {ram}   ·   {cpu_word} {cpu}   ·   {toolkit}   ·   {comp}",
        mem_word = trc("desktop spec", "Memory"),
        cpu_word = trc("desktop spec", "Processor"),
        icon = de_icon(de_id),
        name = m.display_name,
        ram = megabytes(m.ram_mb),
        cpu = m.cpu_usage.label(),
        toolkit = m.toolkit,
        comp = m.compositor_text(),
    ))
}

/// Memory amount with a unit: «700 MB» / «700 МБ».
fn megabytes(mb: u32) -> String {
    trf("%1 MB", &[&mb])
}

/// Specification rows of one desktop.
pub fn specs(de_id: &str) -> Vec<SpecRow> {
    let Some(m) = meta(de_id) else {
        return Vec::new();
    };
    // Program names are compared as text, the load by a flag.
    let values: [(String, &'static str); 7] = [
        (m.toolkit.to_string(), spec_value_color(m.toolkit)),
        // The session label comes from the table of variants, the color from the
        // default session: experimental Wayland next to X11 is not yet ready Wayland,
        // so it is not colored green (the same idea as in fix 1).
        (m.session_text(), spec_value_color(m.session_type)),
        (m.compositor_text(), spec_value_color(m.compositor)),
        (m.panel_type.to_string(), spec_value_color(m.panel_type)),
        (m.file_manager.to_string(), spec_value_color(m.file_manager)),
        (m.terminal.to_string(), spec_value_color(m.terminal)),
        (m.cpu_usage.label(), cpu_load_color(m.cpu_usage)),
    ];
    SPEC_LABELS
        .iter()
        .zip(values)
        .map(|(label, (value, color))| SpecRow {
            label: tr(label),
            value_html: format!("<b>{value}</b>"),
            value,
            color,
            qss: color_qss(color),
        })
        .collect()
}

/// The card's action button: the desktop is not installed — we offer to install
/// it; it is installed — to switch to it.
pub fn card_action(de_id: &str, installed: bool) -> CardAction {
    if installed {
        CardAction {
            de_id: de_id.to_string(),
            kind: "switch",
            text: tr("🔄 Switch"),
            qss: "",
        }
    } else {
        CardAction {
            de_id: de_id.to_string(),
            kind: "install",
            text: tr("📦 Install"),
            qss: INSTALL_BUTTON_QSS,
        }
    }
}

/// The whole card body.
pub fn card_body(de_id: &str, installed: bool, dark: bool) -> Option<CardBody> {
    let m = meta(de_id)?;
    Some(CardBody {
        ram_label: format!("<b>{}:</b>  {}", trc("desktop spec", "Memory"), megabytes(m.ram_mb)),
        ram_bar: ram_bar(m.ram_mb, dark),
        stars_labels: vec![
            trf("<b>Customization:</b>  %1", &[&stars(m.customizable, "⭐")]),
            trf("<b>Gaming:</b>  %1", &[&stars(m.gaming_friendly, "🎮")]),
        ],
        specs: specs(de_id),
        description: m.description_text(),
        action: card_action(de_id, installed),
    })
}

// ============================================================================
// Quick recommendations
// ============================================================================

/// The mark on the right of a recommendation row.
#[derive(Debug, Clone, Serialize)]
pub struct RecRow {
    pub label: String,
    /// The ready row label — bold, as the buyer sees it.
    pub label_html: String,
    pub sub: String,
    pub de_id: &'static str,
    /// `"badge"` — the desktop is already installed, `"install"` — offer the
    /// installation, `"none"` — the desktop is unknown to the core (the row stays
    /// without an action).
    pub mark: &'static str,
    pub text: String,
    pub qss: &'static str,
}

pub const REC_BADGE_TEXT: &str = "✅";
/// Color of the "desktop already installed" mark. Kept separate from the QSS
/// for the same reason as [`spec_value_color`]: QML paints with a property, not
/// a stylesheet. That the two notations have not drifted apart is guarded by
/// `qt_compare_parity.rs`.
pub const REC_BADGE_COLOR: &str = "#4ade80";
pub const REC_BADGE_QSS: &str = "color: #4ade80;";

/// All rows of the recommendations block for the given set of installed desktops.
pub fn rec_rows(installed: &[String]) -> Vec<RecRow> {
    RECOMMENDATIONS
        .iter()
        .map(|(label, sub, de_id)| {
            let (mark, text, qss) = if meta(de_id).is_none() {
                ("none", String::new(), "")
            } else if installed.iter().any(|i| i == de_id) {
                ("badge", REC_BADGE_TEXT.to_string(), REC_BADGE_QSS)
            } else {
                ("install", tr("📦 Install"), "")
            };
            let label = tr(label);
            RecRow {
                label_html: format!("<b>{label}</b>"),
                label,
                sub: tr(sub),
                de_id,
                mark,
                text,
                qss,
            }
        })
        .collect()
}

// ============================================================================
// The "X vs Y" dialog
// ============================================================================

pub const PAIR_MIN_SIZE: (u32, u32) = (720, 520);
pub const PAIR_TITLE_POINT_SIZE: i32 = 16;
pub fn pair_close_text() -> String {
    tr("Close")
}

/// The install button in the comparison dialog.
#[derive(Debug, Clone, Serialize)]
pub struct PairButton {
    pub de_id: String,
    pub text: String,
    pub qss: &'static str,
}

/// The whole pairwise comparison dialog.
#[derive(Debug, Clone, Serialize)]
pub struct PairView {
    /// Window title.
    pub window_title: String,
    /// The large caption inside the window.
    pub heading: String,
    pub html: String,
    pub buttons: Vec<PairButton>,
    pub close_text: String,
    pub min_size: (u32, u32),
    pub title_point_size: i32,
}

fn pair_table(a: &DeMeta, b: &DeMeta) -> String {
    // Short words of the table rows — with context: "Session" or "Panel" may mean
    // something else elsewhere in the product.
    let rows: [(String, String, String); 10] = [
        (trc("desktop spec", "Memory"), megabytes(a.ram_mb), megabytes(b.ram_mb)),
        (trc("desktop spec", "CPU load"), a.cpu_usage.label(), b.cpu_usage.label()),
        // Ratings use the same icons as in the legend above the table and in the
        // cards (`card_body`). The former bar of "█" and "░" was drawn in a fallback
        // font: cells with gaps, the empty part hatched, and it did not match the
        // legend "⭐ from 1 to 5" (01.11.2026).
        (
            trc("desktop spec", "Customization"),
            stars(a.customizable, "⭐"),
            stars(b.customizable, "⭐"),
        ),
        (
            trc("desktop spec", "Gaming"),
            stars(a.gaming_friendly, "🎮"),
            stars(b.gaming_friendly, "🎮"),
        ),
        (trc("desktop spec", "Toolkit"), a.toolkit.to_string(), b.toolkit.to_string()),
        (
            trc("desktop spec", "Session"),
            a.session_text(),
            b.session_text(),
        ),
        (trc("desktop spec", "Compositor"), a.compositor_text(), b.compositor_text()),
        (
            trc("desktop spec", "Panel"),
            a.panel_type.to_string(),
            b.panel_type.to_string(),
        ),
        (
            trc("desktop spec", "Files"),
            a.file_manager.to_string(),
            b.file_manager.to_string(),
        ),
        (trc("desktop spec", "Terminal"), a.terminal.to_string(), b.terminal.to_string()),
    ];
    let mut html = String::from("<table cellpadding=\"6\" cellspacing=\"0\" border=\"0\">");
    html.push_str(&format!(
        "<tr><th align='left'>{}</th><th align='left'>{}</th><th align='left'>{}</th></tr>",
        trc("desktop spec", "Feature"),
        a.display_name,
        b.display_name
    ));
    for (label, va, vb) in rows {
        html.push_str(&format!(
            "<tr><td><b>{label}</b></td><td>{va}</td><td>{vb}</td></tr>"
        ));
    }
    html.push_str("</table>");
    html.push_str(&format!(
        "<p><b>{}:</b> {}</p><p><b>{}:</b> {}</p>",
        a.display_name,
        a.description_text(),
        b.display_name,
        b.description_text()
    ));
    html
}

/// Dialog comparing two desktops; `None` — at least one is unknown to the core.
///
/// Fix: the install buttons of both desktops now look the same — in the original
/// only the first was highlighted.
pub fn pair_view(
    a_id: &str,
    b_id: &str,
    a_installed: bool,
    b_installed: bool,
) -> Option<PairView> {
    let a = meta(a_id)?;
    let b = meta(b_id)?;
    let mut buttons = Vec::new();
    for (m, installed) in [(a, a_installed), (b, b_installed)] {
        if !installed {
            buttons.push(PairButton {
                de_id: m.id.to_string(),
                text: trf("📦 Install %1", &[&m.display_name]),
                qss: INSTALL_BUTTON_QSS,
            });
        }
    }
    Some(PairView {
        window_title: format!("⚔️ {} ↔ {}", a.display_name, b.display_name),
        heading: format!(
            "<b>⚔️ {}  ↔  {}</b>",
            a.display_name, b.display_name
        ),
        html: pair_table(a, b),
        buttons,
        close_text: pair_close_text(),
        min_size: PAIR_MIN_SIZE,
        title_point_size: PAIR_TITLE_POINT_SIZE,
    })
}

// ============================================================================
// Installing and switching: texts of the flow
// ============================================================================

/// Desktop installation texts: the confirmation, the progress title and the result.
#[derive(Debug, Clone, Serialize)]
pub struct InstallTexts {
    pub confirm_title: String,
    pub confirm_html: String,
    pub operation_title: String,
    pub done_title: String,
    pub done_text: String,
}

/// The installation texts of both tabs are THE SAME: they are taken from
/// `qt_desktops::install_texts`. The former copy here differed by a word
/// («Установить KDE Plasma?» versus «Установить рабочий стол KDE Plasma?» —
/// "Install KDE Plasma?" versus "Install the KDE Plasma desktop?"), i.e. it had
/// already started drifting apart (rule #12).
pub fn install_texts(display_name: &str) -> InstallTexts {
    let t = crate::qt_desktops::install_texts(display_name);
    InstallTexts {
        confirm_title: t.confirm_title.to_string(),
        confirm_html: t.confirm_html,
        operation_title: t.operation_title,
        done_title: t.done_title.to_string(),
        done_text: t.done_text,
    }
}

/// Texts for switching to a desktop.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchTexts {
    pub confirm_title: String,
    pub confirm_text: String,
    pub operation_title: String,
}

pub fn switch_texts(display_name: &str) -> SwitchTexts {
    SwitchTexts {
        confirm_title: tr("Switch Desktops?"),
        confirm_text: trf(
            "Switch to <b>%1</b>?\n\n\
Rexon will back up the current desktop's settings and log you out.",
            &[&display_name],
        ),
        operation_title: trf("Switching to %1", &[&display_name]),
    }
}
