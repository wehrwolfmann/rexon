//! Port of `rexon/qt_extensions/window.py` — the "Extensions" tab.
//!
//! The tab was the ONLY large Qt tab not connected to the core at all: its file
//! had not a single call to `rexon_py`, although all the extension machinery has
//! long lived in [`crate::ext_manager`], [`crate::extension`],
//! [`crate::ext_utils`] and [`crate::constants`]. So what had to be moved was
//! not the logic but the INTERFACE — and, along the way, connecting it to what
//! already exists.
//!
//! What this module decides: which labels to show, which filters to build from
//! the set of installed items, what to write in the statistics and in the empty
//! state, what a card, a list row, the detailed view and the "how to build your
//! own extension" help consist of, which files to accept by drag-and-drop and
//! what text to show in the status line after each action.
//!
//! What is NOT here: the extensions themselves ([`installed`] reads them from
//! disk through [`crate::extension::Extension`]) and side actions — installing,
//! removing and toggling go to the system and stay with the executor (currently
//! `rexon/extensions/manager.py`).
//!
//! Warning — fixes to the original (rule #16 — bugs are fixed during the port,
//! not preserved), each marked `Fix:` in place:
//!   1. the name, author, description, tags and README were inserted into HTML
//!      WITHOUT escaping: an extension with `<` in its description broke label
//!      parsing, and a README with angle brackets lost pieces of text right in
//!      the storefront;
//!   2. secondary labels (type, author, statistics, path, technical data, the
//!      status line) were hard-wired to `color: #ffffff` in any theme — on a
//!      light one that is white on white. The color now comes as a pair from
//!      [`crate::ui_base`], as rule #9 requires;
//!   3. in the light theme the disabled button of the "Install" block had
//!      `color: rgba(255,255,255,0.55)` — "Steam Workshop" was indistinguishable;
//!   4. pressing a filter button with an EMPTY extension list hid the "No
//!      extensions yet" hint instead of showing it (in `_refresh` the same
//!      branch worked correctly). There is now a single empty state —
//!      [`empty_text`].
//!
//! The check is `rust/rexon-core/tests/qt_extensions_parity.rs` against a
//! snapshot of the live Python path (`tests/golden/qt_extensions_cases.json`).

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::constants;
use crate::ext_manager;
use crate::extension::Extension;
use crate::i18n::{tr, trc, trf};

// ============================================================================
// Fixed labels and sizes of the tab
// ============================================================================

// Labels are functions rather than constants: the interface language is chosen
// at start-up, and a function, unlike a constant, cannot be forgotten when
// translating.

pub fn window_title() -> String {
    tr("Rexon — Extensions")
}
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 700);

pub fn header_title() -> String {
    tr("🧩 Extensions")
}
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
pub const SECTION_POINT_SIZE: i32 = 14;

/// Width of an extension tile. CONSTANT: the grid engine computes columns from
/// it. A range of "220…280" produced cards of different widths in one row, and
/// the column computation lied — a "card width" simply did not exist.
pub const CARD_W: i32 = 260;
/// Width of the labels inside the tile: minus the layout margins (8+8).
pub const CARD_TEXT_W: i32 = CARD_W - 16;
pub const PREVIEW_H: i32 = 120;

pub fn search_placeholder() -> String {
    tr("Search by name, author, tags…")
}
pub fn install_button_text() -> String {
    tr("＋ Install from File")
}

pub const VIEW_GRID_TEXT: &str = "⊞";
pub fn view_grid_tooltip() -> String {
    trc("extensions view", "Grid")
}
pub const VIEW_LIST_TEXT: &str = "☰";
pub fn view_list_tooltip() -> String {
    trc("extensions view", "List")
}

/// Display mode identifiers. Strings rather than numbers: they are put into the
/// model roles and read by the markup.
pub const VIEW_GRID: &str = "grid";
pub const VIEW_LIST: &str = "list";

pub const FILTER_ALL_ID: &str = "all";
pub fn filter_all_label() -> String {
    tr("All")
}

pub fn my_header_text() -> String {
    tr("My Extensions")
}
pub fn install_section_header() -> String {
    tr("⬇️ Install")
}
pub fn roadmap_note() -> String {
    tr("🚧 Coming soon: Steam Workshop for .rxn appearance packs and extensions.")
}

pub fn empty_no_extensions() -> String {
    tr("📭 No extensions yet. Drag an .rxn file into the window or \
click <b>“+ Install from File”</b>. Windows .themepack, .deskthemepack \
and .msstyles files work too.")
}
pub fn empty_no_match() -> String {
    tr("🔍 Nothing matches the current filters.")
}

pub const UNINSTALL_ICON: &str = "🗑️";
/// Label of the remove button ON THE TILE and in the detailed view.
///
/// Warning: a deliberate departure from the widget reference (rule #13). On the
/// tile the original has a bare trash can 40 points wide, and the live check of
/// 06.09 showed exactly what this rule does not allow: an IRREVERSIBLE action
/// has no label, the buyer has to guess the icon, and the tooltip appears only
/// under the cursor — on a gamepad and in Big Picture it does not exist at all.
/// There is room on the tile: a width of 260 minus the 8+8 margins gives 244,
/// and the tightest pair of buttons ("▶ Enable" and "🗑 Remove") takes about 214.
/// In a LIST ROW the label is not set: the view is compact there, and the
/// toggle next to it is also drawn as a single icon — the tooltip explains both
/// buttons.
pub fn uninstall_text() -> String {
    tr("🗑️ Remove")
}
pub fn uninstall_tooltip() -> String {
    trc("extension action", "Remove")
}
pub fn toggle_on_text() -> String {
    tr("▶️ Enable")
}
pub fn toggle_off_text() -> String {
    tr("⏸️ Disable")
}
pub const TOGGLE_ON_ICON: &str = "▶️";
pub const TOGGLE_OFF_ICON: &str = "⏸️";
pub fn toggle_on_tooltip() -> String {
    trc("extension action", "Enable")
}
pub fn toggle_off_tooltip() -> String {
    trc("extension action", "Disable")
}

pub fn state_enabled_text() -> String {
    trc("extension state", "✅ Active")
}
pub fn state_disabled_text() -> String {
    trc("extension state", "⏸️ Disabled")
}
/// Color of the "active" mark and the light-blue heading of a group/tags. They
/// are written here and not in the .qml: the markup must not have colors of its
/// own.
pub const STATE_ENABLED_COLOR: &str = "#4ade80";
pub const STATE_DISABLED_COLOR: &str = "#888888";
pub const ACCENT_INFO_COLOR: &str = "#6cb4ff";
/// The red background of the remove button (in widgets — `rgba(220,80,80,0.2)`).
pub const DANGER_COLOR: &str = "#dc5050";
pub const DANGER_FILL_ALPHA: f64 = 0.2;
pub const DANGER_FILL_HOVER_ALPHA: f64 = 0.4;

/// Title and filter of the file chooser dialog.
pub fn file_dialog_title() -> String {
    tr("Choose an .rxn file")
}
pub fn file_dialog_filter() -> String {
    // TRANSLATORS: file filter of a file dialog; keep the ";;" separators
    // and the patterns in parentheses exactly as they are.
    tr("Rexon extensions (*.rxn);;\
Windows themes (*.themepack *.deskthemepack *.theme *.msstyles);;\
All files (*)")
}

/// File extensions the tab accepts by drag-and-drop.
pub const DROP_EXTENSIONS: &[&str] = &[
    ".rxn",
    ".themepack",
    ".deskthemepack",
    ".theme",
    ".msstyles",
    ".cur",
    ".ani",
    ".inf",
    ".curxptheme",
    ".7tsp",
];

/// Whether we accept this file by drag-and-drop (case-insensitive suffix comparison).
pub fn is_supported_drop(path: &str) -> bool {
    let lower = path.to_lowercase();
    let suffix = match lower.rfind('.') {
        Some(pos) => &lower[pos..],
        None => return false,
    };
    DROP_EXTENSIONS.contains(&suffix)
}

// ============================================================================
// Layout and styling that the skin palette does not have
// ============================================================================
//
// The numbers and paints of the widget tab, scattered across its layouts and
// stylesheets. The QML markup does not create its own: everything comes from
// here as the `styles` map (rust/rexon-gui/src/extensions_page.rs) — the same
// set the widget version is painted with.

/// Page margins and the spacing between its blocks.
pub const PAGE_MARGIN: i32 = 24;
pub const PAGE_SPACING: i32 = 14;

/// View buttons (⊞ / ☰): icon font size and the minimum content width.
pub const VIEW_BUTTON_POINT_SIZE: i32 = 14;
pub const VIEW_BUTTON_MIN_WIDTH: i32 = 32;
/// Chip padding (view buttons and filters) horizontally and vertically; the
/// rounding is shared with the Steam chips.
pub const CHIP_PADDING_H: i32 = 10;
pub const CHIP_PADDING_V: i32 = 4;
pub const CHIP_RADIUS: i32 = crate::qt_steam::BTN_RADIUS;
/// Gap between filter buttons.
pub const FILTER_SPACING: i32 = 4;

/// Spacing of the tile grid. List rows and group headings follow the same
/// spacing: in the widget tab both views live in one grid.
pub const GRID_SPACING: i32 = 10;
/// Tile: layout margin, row spacing, rounding, gap between buttons. The width of
/// the remove button is no longer here: with a label (`UNINSTALL_TEXT`) the
/// button is sized by its contents, like the neighboring toggle.
pub const CARD_MARGIN: i32 = 8;
pub const CARD_SPACING: i32 = 6;
pub const CARD_RADIUS: i32 = 6;
pub const CARD_BUTTON_SPACING: i32 = 4;
/// Font size of the placeholder icon in place of the preview.
pub const PLACEHOLDER_POINT_SIZE: i32 = 32;

/// List row: layout margins (horizontal and vertical), the stylesheet addition
/// (`padding: 4px` goes ON TOP of the layout margins — a widget row is 8 points
/// taller than the margins alone suggest), the spacing between the icon, the
/// labels and the buttons, the rounding, the width and font size of the icon,
/// the width of the "⏸"/"🗑" buttons.
pub const LIST_ROW_MARGIN_H: i32 = 10;
pub const LIST_ROW_MARGIN_V: i32 = 6;
pub const LIST_ROW_PADDING: i32 = 4;
pub const LIST_ROW_SPACING: i32 = 10;
pub const LIST_ROW_RADIUS: i32 = 4;
pub const LIST_ICON_WIDTH: i32 = 36;
pub const LIST_ICON_POINT_SIZE: i32 = 18;
pub const LIST_BUTTON_WIDTH: i32 = 36;
/// List group heading: top and bottom padding (`padding: 6px 0 2px 0`).
pub const GROUP_HEADER_PADDING_TOP: i32 = 6;
pub const GROUP_HEADER_PADDING_BOTTOM: i32 = 2;

/// Padding around the empty-state hint (`padding: 16px`).
pub const EMPTY_PADDING: i32 = 16;

/// Buttons of the "⬇ Install" block: gap, rounding, padding around the label.
pub const INSTALL_SOURCE_SPACING: i32 = 10;
pub const INSTALL_SOURCE_RADIUS: i32 = 6;
pub const INSTALL_SOURCE_PADDING: i32 = 10;

/// The "background, border, border under the cursor" triple of a tile.
///
/// The same as for the Steam storefront cards: the stylesheets of both tabs
/// were typed with the same numbers (0.04 / 0.12 / 0.30 on dark, 0.03 / 0.15 /
/// 0.35 on light), and a second copy has no place in the core (rule #12). Under
/// the cursor ONLY the tile's border changes, the background stays.
pub fn card_colors(dark: bool) -> (String, String, String) {
    crate::qt_steam::card_colors(false, dark)
}

/// The "background, icon color" pair of the placeholder in place of the preview.
pub fn placeholder_colors(dark: bool) -> (String, String) {
    if dark {
        ("rgba(255,255,255,0.06)".to_string(), "#aaaaaa".to_string())
    } else {
        ("rgba(0,0,0,0.06)".to_string(), "#555555".to_string())
    }
}

/// The "background, background under the cursor" pair of a list row.
///
/// Fix No. 5 (rule #16): the original painted the row white with transparency
/// in ANY theme — on a light one that is "lighter than light", i.e. an
/// invisible row. The light theme gets black with the same transparency, as for
/// the tile.
pub fn list_row_colors(dark: bool) -> (String, String) {
    if dark {
        (
            "rgba(255,255,255,0.03)".to_string(),
            "rgba(255,255,255,0.06)".to_string(),
        )
    } else {
        ("rgba(0,0,0,0.03)".to_string(), "rgba(0,0,0,0.06)".to_string())
    }
}

/// The "background, background under the cursor, border" triple of a large
/// button of the "Install" block.
pub fn install_source_colors(dark: bool) -> (String, String, String) {
    if dark {
        (
            "rgba(255,255,255,0.04)".to_string(),
            "rgba(255,255,255,0.08)".to_string(),
            "rgba(255,255,255,0.15)".to_string(),
        )
    } else {
        (
            "rgba(0,0,0,0.03)".to_string(),
            "rgba(0,0,0,0.07)".to_string(),
            "rgba(0,0,0,0.15)".to_string(),
        )
    }
}

/// The "border, selected background, selected border, background under the
/// cursor" quadruple of a chip — the view buttons and filters.
///
/// There is one chip for the product (`qt_steam::switch_chip_colors`): the
/// selected state in the widget "Extensions" was typed with the same
/// `rgba(59,130,246,0.4)` and `rgba(59,130,246,0.6)` as the Steam subtabs.
pub fn chip_colors() -> (String, String, String, String) {
    crate::qt_steam::switch_chip_colors()
}

// ============================================================================
// Extension types: toggleable, filters, groups
// ============================================================================

/// A type that has a toggle: scripted ones (`plugin`, `desktop-tool`).
///
/// Appearance items have no toggle and cannot have one — they are either
/// installed on the system or not. The answer is taken from the core
/// (`SCRIPTED_TYPES`) rather than repeated as a couple of lines in the UI:
/// there may be more scripted types.
pub fn is_toggleable(ext_type: &str) -> bool {
    constants::is_scripted(ext_type)
        // Environment add-ons (KWin scripts and effects, GNOME extensions, Cinnamon
        // applets/desklets/extensions, Nemo actions) are enabled and disabled by the
        // person: installation no longer enables them (04.10.2026).
        || crate::de_apply::mode(ext_type).map(crate::de_apply::Mode::button)
            == Some(crate::de_apply::Button::Toggle)
}

/// A filter button: the type identifier and the label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FilterItem {
    pub id: String,
    pub label: String,
}

/// Filter buttons by the SET of installed items: "All" plus only the types that
/// are actually installed, in the order of the label table.
///
/// A new type gets into the filter by itself, without editing this place (rule
/// #12).
pub fn filter_items(installed_types: &[String]) -> Vec<FilterItem> {
    let mut items = vec![FilterItem {
        id: FILTER_ALL_ID.to_string(),
        label: filter_all_label(),
    }];
    for (t, label) in constants::type_labels_ordered() {
        if installed_types.iter().any(|x| x == t) {
            items.push(FilterItem {
                id: (*t).to_string(),
                label: format!(
                    "{} {}",
                    constants::type_icon(t).unwrap_or(""),
                    crate::i18n::tr(label)
                ),
            });
        }
    }
    items
}

/// Whether the selected filter stays valid with this set of buttons.
///
/// If no buttons other than "All" are left (the extensions were removed), the
/// choice is reset — otherwise the storefront would silently show emptiness by
/// a filter that is no longer on the screen.
pub fn normalize_filter(filter_type: &str, items: &[FilterItem]) -> String {
    if items.iter().any(|i| i.id == filter_type) {
        filter_type.to_string()
    } else {
        FILTER_ALL_ID.to_string()
    }
}

// ============================================================================
// Statistics line and headings
// ============================================================================

/// Statistics line: "Installed · Active · by type" (non-zero only).
///
/// The input is the ready answer of [`ext_manager::stats`], so that there is a
/// single count for the product: the Python path shows the same map.
pub fn stats_line(stats: &Map<String, Value>) -> String {
    let num = |key: &str| stats.get(key).and_then(Value::as_u64).unwrap_or(0);
    let mut parts = vec![
        trf("Installed: <b>%1</b>", &[&num("total")]),
        trf("Active: <b>%1</b>", &[&num("enabled")]),
    ];
    let by_type = stats.get("by_type").and_then(Value::as_object);
    for (t, label) in constants::type_labels_ordered() {
        let count = by_type
            .and_then(|m| m.get(*t))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if count > 0 {
            parts.push(format!(
                "{} {}: <b>{}</b>",
                constants::type_icon(t).unwrap_or(""),
                crate::i18n::tr(label),
                count
            ));
        }
    }
    parts.join("  •  ")
}

/// The heading "My extensions (filtered/total)".
///
/// `None` — there are no extensions at all: the heading above emptiness is hidden.
pub fn my_header(filtered: usize, total: usize) -> Option<String> {
    if total == 0 {
        return None;
    }
    if filtered == total {
        Some(my_header_text())
    } else {
        Some(format!("{} ({filtered}/{total})", my_header_text()))
    }
}

/// Text of the empty state, or `None` if there is nothing to show.
///
/// Fix No. 4: one function for both branches. In the original the filter
/// handler hid the "No extensions yet" hint although the list was empty.
pub fn empty_text(filtered: usize, total: usize) -> Option<String> {
    if total == 0 {
        Some(empty_no_extensions())
    } else if filtered == 0 {
        Some(empty_no_match())
    } else {
        None
    }
}

// ============================================================================
// The "Install" block
// ============================================================================

/// A button of the "⬇ Install" block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallSource {
    /// What to do on click: `file` | `workshop` | `create`.
    pub kind: String,
    pub icon: String,
    pub label: String,
    pub tooltip: String,
    pub enabled: bool,
}

impl InstallSource {
    /// The whole button label: the icon, a line break, the name.
    pub fn text(&self) -> String {
        format!("{}\n{}", self.icon, self.label)
    }
}

pub const INSTALL_SOURCE_MIN_HEIGHT: i32 = 70;
pub const INSTALL_SOURCE_MIN_WIDTH: i32 = 160;
pub const INSTALL_SOURCE_MAX_WIDTH: i32 = 220;

/// Three sources of extensions: a file, the Steam Workshop (disabled for now),
/// your own.
pub fn install_sources() -> Vec<InstallSource> {
    vec![
        InstallSource {
            kind: "file".into(),
            icon: "📦".into(),
            label: tr("From File"),
            tooltip: tr("Open an .rxn, .themepack or Windows theme"),
            enabled: true,
        },
        InstallSource {
            kind: "workshop".into(),
            icon: "🎮".into(),
            // The button is disabled, and a tooltip on a disabled one cannot be
            // seen: the reason is right in the label, otherwise a gray tile without
            // words would read as breakage.
            label: tr("Steam Workshop — coming soon"),
            tooltip: tr("Coming soon — Steam Workshop integration"),
            enabled: false,
        },
        InstallSource {
            kind: "create".into(),
            icon: "✏️".into(),
            label: tr("Create Your Own"),
            tooltip: tr("How to make your own .rxn extension"),
            enabled: true,
        },
    ]
}

// ============================================================================
// Status line
// ============================================================================

/// Truncation of the error text, as in the original: 200 characters.
///
/// Warning: we count CHARACTERS, not bytes: in Python the slice `msg[:200]` is by
/// characters, while in Rust `&s[..200]` on Russian text would simply crash the
/// process at a multi-byte character boundary.
fn clip(msg: &str) -> String {
    msg.chars().take(200).collect()
}

pub fn status_install_busy() -> String {
    tr("⚠️ An installation is already running")
}
pub fn status_uninstall_busy() -> String {
    tr("⚠️ A removal is already running")
}

pub fn status_installing(file_name: &str) -> String {
    trf("⏳ Installing “%1”…", &[&file_name])
}

pub fn status_installed(name: &str) -> String {
    trf("✅ Installed: %1", &[&name])
}

/// The name of the installed extension PLUS what the person must learn about.
///
/// The installation went through, but some optional steps may have been skipped
/// (no converter, nothing to measure the wallpapers with). Keeping silent about
/// it is not allowed: the buyer would decide that the program just works this
/// way, or that the theme is bad. The status line wraps by words, so each
/// warning goes on its own line. If there are no warnings, only the name is
/// returned, and the message does not change by a single character.
pub fn installed_with_notes(name: &str, notes: &[String]) -> String {
    crate::importers::with_skipped_notes(name, notes)
}

pub fn status_install_error(msg: &str) -> String {
    trf("⚠️ Error: %1", &[&clip(msg)])
}

pub fn status_uninstalling(name: &str) -> String {
    trf("⏳ Removing “%1”…", &[&name])
}

pub fn status_uninstalled(ext_id: &str) -> String {
    trf("🗑️ Removed: %1", &[&ext_id])
}

pub fn status_uninstall_error(msg: &str) -> String {
    trf("⚠️ Couldn't remove: %1", &[&clip(msg)])
}

pub fn status_toggled(enabled: bool, ext_id: &str) -> String {
    if enabled {
        trf("▶️ Enabled: %1", &[&ext_id])
    } else {
        trf("⏸️ Disabled: %1", &[&ext_id])
    }
}

pub fn status_toggle_error(msg: &str) -> String {
    trf("⚠️ Couldn't turn it on or off: %1", &[&msg])
}

// ============================================================================
// Confirmations
// ============================================================================

pub fn confirm_title() -> String {
    tr("Remove Extension?")
}

/// Removal confirmation from the storefront: short, without details.
pub fn confirm_uninstall(name: &str) -> String {
    trf("Remove “%1”?", &[&name])
}

/// Removal confirmation from the detailed view: with the directory named.
pub fn confirm_uninstall_detailed(name: &str, dir: &str) -> String {
    trf(
        "Remove “%1”?\n\nThe files in %2 will be deleted permanently.",
        &[&name, &dir],
    )
}

// ============================================================================
// Escaping
// ============================================================================

/// Escape text for insertion into a label with markup.
///
/// Fix No. 1: the original substituted the name, author, description, tags and
/// README into HTML as is. An extension with `<` in its description broke the
/// label, and a README with angle brackets lost pieces of text in the
/// storefront.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

// ============================================================================
// Flat view of an extension
// ============================================================================

/// An extension broken down into the fields the storefront needs.
///
/// In the core an [`Extension`] is a directory, a manifest and an enabled flag;
/// everything else is obtained from the manifest by queries. The storefront
/// needs a flat set, and exactly the same one Python saw (`get_info`), so it is
/// built from `get_info()` rather than by a second parse of the manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExtView {
    pub ext_id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    /// The author is specified in the manifest. A FLAG, not a comparison with a
    /// label: previously "author unknown" was recognized by the text «Неизвестно»
    /// ("Unknown"), which differs in every language once translated.
    pub author_known: bool,
    pub description: String,
    pub ext_type: String,
    pub type_label: String,
    pub type_icon: String,
    pub tags: Vec<String>,
    pub de_support: Vec<String>,
    pub enabled: bool,
    pub dir: String,
    pub preview_path: String,
    pub icon_path: String,
    pub readme_path: String,
    pub license: String,
    pub homepage: String,
    pub format_version: String,
    pub rexon_min_version: String,
}

fn map_str(info: &Map<String, Value>, key: &str) -> String {
    info.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn map_list(info: &Map<String, Value>, key: &str) -> Vec<String> {
    info.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A manifest value as a string "for display": strings as is, numbers without
/// quotes, a missing one — an empty string.
fn manifest_display(manifest: &Map<String, Value>, key: &str) -> String {
    match manifest.get(key) {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

impl ExtView {
    /// Break a core extension down into storefront fields.
    pub fn from_extension(ext: &Extension) -> Self {
        let info = ext.get_info();
        let manifest = &ext.manifest;
        // The original first asked for `rexon_min_version`, then for
        // `min_rexon_version` — both names occurred in foreign manifests.
        let mut rexon_min = manifest_display(manifest, "rexon_min_version");
        if rexon_min.is_empty() {
            rexon_min = manifest_display(manifest, "min_rexon_version");
        }
        ExtView {
            ext_id: map_str(&info, "id"),
            name: map_str(&info, "name"),
            version: map_str(&info, "version"),
            author: map_str(&info, "author"),
            author_known: !matches!(manifest.get("author"), None | Some(Value::Null)),
            description: map_str(&info, "description"),
            ext_type: map_str(&info, "type"),
            type_label: map_str(&info, "type_label"),
            type_icon: map_str(&info, "type_icon"),
            tags: map_list(&info, "tags"),
            de_support: map_list(&info, "de_support"),
            enabled: info
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            dir: ext.dir.to_string_lossy().into_owned(),
            preview_path: map_str(&info, "preview_path"),
            icon_path: map_str(&info, "icon_path"),
            readme_path: map_str(&info, "readme_path"),
            license: map_str(&info, "license"),
            homepage: map_str(&info, "homepage"),
            format_version: manifest_display(manifest, "format_version"),
            rexon_min_version: rexon_min,
        }
    }

    /// Whether to show the author: an empty or unspecified one tells nothing.
    pub fn has_author(&self) -> bool {
        self.author_known && !self.author.is_empty()
    }

    /// Whether there is a toggle (a scripted type).
    pub fn toggleable(&self) -> bool {
        is_toggleable(&self.ext_type)
    }

    /// The card picture: the preview, otherwise the icon, otherwise nothing.
    pub fn card_image(&self) -> &str {
        if !self.preview_path.is_empty() {
            &self.preview_path
        } else {
            &self.icon_path
        }
    }

    /// Whether the extension matches the search query.
    ///
    /// The query has already been lower-cased and trimmed at the edges by the caller
    /// (see [`normalize_query`]). We search by name, author, description and tags —
    /// exactly the same set of fields as in the original.
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        self.name.to_lowercase().contains(query)
            || self.author.to_lowercase().contains(query)
            || self.description.to_lowercase().contains(query)
            || self
                .tags
                .iter()
                .any(|t| t.to_lowercase().contains(query))
    }
}

/// Bring a search query to the form in which it is compared.
pub fn normalize_query(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Select extensions by type and query — in the order of the source list.
pub fn apply_filters<'a>(exts: &'a [ExtView], filter_type: &str, query: &str) -> Vec<&'a ExtView> {
    let q = normalize_query(query);
    exts.iter()
        .filter(|e| filter_type == FILTER_ALL_ID || e.ext_type == filter_type)
        .filter(|e| e.matches(&q))
        .collect()
}

// ============================================================================
// Card
// ============================================================================

/// Contents of an extension tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card {
    /// Path to the picture (empty — we draw the type icon).
    pub image: String,
    /// Placeholder icon if there is no picture.
    pub placeholder: String,
    /// "icon Type · vVersion".
    pub type_line: String,
    pub name: String,
    /// The full name in the tooltip: the label may be truncated with an ellipsis.
    pub name_tooltip: String,
    /// "👤 Author" or empty.
    pub author_line: String,
    pub has_toggle: bool,
    pub state_text: String,
    pub state_color: String,
    pub toggle_text: String,
}

/// Build the tile.
pub fn card(ext: &ExtView) -> Card {
    Card {
        image: ext.card_image().to_string(),
        placeholder: ext.type_icon.clone(),
        type_line: format!(
            "{} {}  ·  v{}",
            ext.type_icon, ext.type_label, ext.version
        ),
        name: ext.name.clone(),
        name_tooltip: ext.name.clone(),
        author_line: if ext.has_author() {
            format!("👤 {}", ext.author)
        } else {
            String::new()
        },
        has_toggle: ext.toggleable(),
        state_text: if ext.enabled {
            state_enabled_text()
        } else {
            state_disabled_text()
        },
        state_color: if ext.enabled {
            STATE_ENABLED_COLOR.to_string()
        } else {
            STATE_DISABLED_COLOR.to_string()
        },
        toggle_text: if ext.enabled {
            toggle_off_text()
        } else {
            toggle_on_text()
        },
    }
}

// ============================================================================
// A row of the "list" mode
// ============================================================================

/// A compact extension row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListRow {
    pub icon: String,
    /// "<b>Name</b>  <small>vVersion</small>".
    pub title_html: String,
    /// "Type  ·  👤 Author" or empty.
    pub subtitle: String,
    pub has_toggle: bool,
    pub toggle_icon: String,
    pub toggle_tooltip: String,
}

pub fn list_row(ext: &ExtView) -> ListRow {
    ListRow {
        icon: ext.type_icon.clone(),
        title_html: format!(
            "<b>{}</b>  <small>v{}</small>",
            escape_html(&ext.name),
            escape_html(&ext.version)
        ),
        subtitle: if ext.has_author() {
            format!("{}  ·  👤 {}", ext.type_label, ext.author)
        } else {
            String::new()
        },
        has_toggle: ext.toggleable(),
        toggle_icon: if ext.enabled {
            TOGGLE_OFF_ICON.to_string()
        } else {
            TOGGLE_ON_ICON.to_string()
        },
        toggle_tooltip: if ext.enabled {
            toggle_off_tooltip()
        } else {
            toggle_on_tooltip()
        },
    }
}

/// A group of the "list" mode: the type heading and the row numbers inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListGroup {
    pub ext_type: String,
    /// "<b>icon Label (N)</b>".
    pub header_html: String,
    /// Numbers in the passed list — the order within the group is preserved.
    pub rows: Vec<usize>,
}

/// Break extensions down by type in the order of the label table.
///
/// Types missing from the table go to the end — like the original's
/// `index(t) if t in type_order else 99`, but without its ceiling: with 35
/// types in the table "99" just happened to coincide with "at the end", while
/// with more types the sorting would silently shuffle the tail.
pub fn list_groups(exts: &[&ExtView]) -> Vec<ListGroup> {
    let order: Vec<&str> = constants::type_labels_ordered()
        .iter()
        .map(|(t, _)| *t)
        .collect();
    let mut seen: Vec<String> = Vec::new();
    for e in exts {
        if !seen.iter().any(|t| *t == e.ext_type) {
            seen.push(e.ext_type.clone());
        }
    }
    seen.sort_by_key(|t| {
        order
            .iter()
            .position(|o| *o == t.as_str())
            .unwrap_or(order.len())
    });
    seen.into_iter()
        .map(|t| {
            let rows: Vec<usize> = exts
                .iter()
                .enumerate()
                .filter(|(_, e)| e.ext_type == t)
                .map(|(i, _)| i)
                .collect();
            ListGroup {
                header_html: format!(
                    "<b>{} {} ({})</b>",
                    constants::type_icon(&t).unwrap_or(""),
                    constants::type_label(&t).unwrap_or_else(|| t.clone()),
                    rows.len()
                ),
                ext_type: t,
                rows,
            }
        })
        .collect()
}

// ============================================================================
// Detailed view of an extension
// ============================================================================

pub const DETAIL_MIN_SIZE: (u32, u32) = (680, 540);
pub const DETAIL_TITLE_POINT_SIZE: i32 = 16;
pub const DETAIL_PREVIEW_SIZE: (u32, u32) = (620, 260);
pub fn detail_fallback_title() -> String {
    tr("Extension")
}
pub fn detail_open_folder_text() -> String {
    tr("📁 Open Folder")
}
/// The label is the same as on the tile: there must be no second copy of the
/// text here.
pub fn detail_uninstall_text() -> String {
    uninstall_text()
}
pub fn detail_close_text() -> String {
    tr("Close")
}
pub fn detail_no_description() -> String {
    tr("<i>No description</i>")
}
/// How many tags we show: a long tail stretched the window.
pub const DETAIL_TAGS_LIMIT: usize = 20;

// ── Layout of both modal windows of the tab ─────────────────────────────
//
// The detailed view and the "Create your own" help are windows of the same cut:
// a margin of 16, a title, a scrollable body, a row of buttons at the bottom.
// The numbers are the same as in the layouts of the widget windows; the QML
// markup does not create its own and takes them from the `styles` map under the
// `dialog` key.

/// Margin around the contents of both windows (`setContentsMargins(16, 16, 16, 16)`).
pub const DIALOG_MARGIN: i32 = 16;
/// Spacing between the blocks of the detailed view (`layout.setSpacing(10)`).
pub const DETAIL_SPACING: i32 = 10;
/// Spacing between the blocks of the help: it sets none of its own and works on
/// the layout's default spacing — 6 points (measured on a live window under the
/// skin).
pub const CREATE_SPACING: i32 = 6;
/// Top and bottom padding of the detailed view's technical line (`padding: 4px 0`).
pub const DETAIL_TECH_PADDING_V: i32 = 4;
/// Fill opacity of the "🗑 Remove" button IN THE DETAILED VIEW. It has its own,
/// denser than the tile's: 0.25 versus [`DANGER_FILL_ALPHA`] = 0.2. Under the
/// cursor both reach 0.4 ([`DANGER_FILL_HOVER_ALPHA`]).
pub const DETAIL_DANGER_FILL_ALPHA: f64 = 0.25;

/// The description area (README, help): border, rounding and inner padding.
///
/// In the widget windows this is a `QTextBrowser` — a two-point border from the
/// style, a corner rounding of about three (measured from a screenshot) and a
/// document margin of 4. The area's background is the `Base` role of the skin
/// palette (`bg_secondary`), i.e. NOT the window background: without it the
/// text hangs right on the window background and the area's border cannot be
/// seen at all.
pub const BODY_BORDER_WIDTH: i32 = 1;
pub const BODY_RADIUS: i32 = 3;
pub const BODY_PADDING: i32 = 4;
/// The description area does not shrink below this — the `minimumSizeHint` of a
/// `QTextBrowser` under the product skin (measured: 83×83).
///
/// This number is NOT for beauty: in a window shrunk to the minimum, the layout
/// distributes the lack of space along this boundary. In the widget window the
/// description stays readable while the preview gives way; without the boundary
/// the whole shortage fell on the description, and it collapsed into a strip
/// (measured from a screenshot on 04.09.2026: 21 points versus 85 in the
/// widgets).
pub const BODY_MIN_HEIGHT: i32 = 83;

/// Detailed view of one extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detail {
    /// Window title.
    pub window_title: String,
    /// "<b>icon Name</b>".
    pub heading_html: String,
    /// "📦 Type · vVersion · 👤 Author · 🖥 desktops".
    pub meta_line: String,
    /// Preview picture (empty — not shown).
    pub preview: String,
    /// README, description or "No description" — already as markup.
    pub body_html: String,
    /// "#tag #tag" or empty.
    pub tags_line: String,
    /// Technical information as one line of markup.
    pub tech_html: String,
    /// "📁 path" with the home directory collapsed to `~`.
    pub path_html: String,
    /// The extension directory — for the "Open folder" button.
    pub dir: String,
    pub ext_id: String,
}

/// Collapse the home directory to `~`, as the original showed it.
pub fn shorten_home(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    path.replace(home, "~")
}

/// Build the detailed view. `readme` — the already read text or `None`.
pub fn detail(ext: &ExtView, readme: Option<&str>, home: &str) -> Detail {
    let mut meta = vec![
        format!("📦 {}", ext.type_label),
        format!("v{}", ext.version),
    ];
    if ext.has_author() {
        meta.push(format!("👤 {}", ext.author));
    }
    if !ext.de_support.is_empty() {
        meta.push(format!("🖥️ {}", ext.de_support.join(", ")));
    }

    let body_html = match readme {
        // A wrapping <pre>: otherwise README lines longer than the window were cut off.
        Some(text) if !text.is_empty() => crate::ui_base::dialog_pre(&escape_html(text)),
        _ if !ext.description.is_empty() => {
            format!("<p>{}</p>", escape_html(&ext.description))
        }
        _ => detail_no_description(),
    };

    let tags_line = if ext.tags.is_empty() {
        String::new()
    } else {
        ext.tags
            .iter()
            .take(DETAIL_TAGS_LIMIT)
            .map(|t| format!("#{t}"))
            .collect::<Vec<_>>()
            .join("  ")
    };

    // The internal package number and the `.rxn` format version do not go here:
    // they say nothing to a person, and "Format: v3" read as the version of the
    // theme itself (Wehrwolfmann's question of 10.10.2026). The theme version is
    // in the line under the title.
    let mut tech: Vec<String> = Vec::new();
    if !ext.rexon_min_version.is_empty() {
        tech.push(trf(
            "<b>Minimum Rexon:</b> %1",
            &[&escape_html(&ext.rexon_min_version)],
        ));
    }
    if !ext.homepage.is_empty() {
        let safe = escape_html(&ext.homepage);
        tech.push(trf("<b>Website:</b> %1", &[&format!("<a href='{safe}'>{safe}</a>")]));
    }
    if !ext.license.is_empty() {
        tech.push(trf("<b>License:</b> %1", &[&escape_html(&ext.license)]));
    }

    Detail {
        window_title: if ext.name.is_empty() {
            detail_fallback_title()
        } else {
            ext.name.clone()
        },
        heading_html: format!(
            "<b>{} {}</b>",
            ext.type_icon,
            escape_html(&ext.name)
        ),
        meta_line: meta.join("  ·  "),
        preview: ext.preview_path.clone(),
        body_html,
        tags_line,
        tech_html: tech.join("  ·  "),
        path_html: format!(
            "📁 <tt>{}</tt>",
            escape_html(&shorten_home(&ext.dir, home))
        ),
        dir: ext.dir.clone(),
        ext_id: ext.ext_id.clone(),
    }
}

// ============================================================================
// The "Create your own" help
// ============================================================================

pub fn create_title() -> String {
    tr("Create Your Own .rxn Extension")
}
pub const CREATE_MIN_SIZE: (u32, u32) = (720, 580);
pub fn create_header() -> String {
    tr("<b>📦 .rxn v3 Structure</b>")
}
pub fn create_close_text() -> String {
    tr("Close")
}

/// Help on building your own extension — the same one the widget tab showed.
/// The core holds the markup: it is product text, not styling.
pub fn create_body_html() -> String {
    // TRANSLATORS: help page in HTML. Keep the tags, the file names, the
    // JSON keys and the type names (theme-gtk, plugin…) as they are;
    // translate the prose and the sample values of "name", "author" and
    // "description". The <pre> blocks stay left-to-right in every language,
    // so keep their arrows ← as they are.
    // No leading line break: xgettext strips it from the msgid, and the
    // string in the code would diverge from the catalog key.
    let body = tr(r#"<p>A Rexon extension is a plain <b>ZIP archive</b> with the <code>.rxn</code> extension.
Every type has the same layout:</p>

<pre>
extension.rxn (ZIP):
├── manifest.json     ← REQUIRED (format_version: 3)
├── preview.png       ← 620×348 (for the catalog and cards)
├── icon.png          ← 128×128 (lists in Rexon)
├── install.sh        ← for desktop-tool / plugin
├── uninstall.sh      ← for desktop-tool / plugin
├── app.desktop       ← for desktop-tool
├── README.md         ← optional, the description
└── data/             ← the extension's files
</pre>

<h3>📋 Minimal manifest.json</h3>
<pre>
{
  "format_version": 3,
  "id": "my-cool-theme",
  "name": "My Cool Theme",
  "version": "1.0.0",
  "author": "Me",
  "description": "What the extension does",
  "type": "theme-gtk",
  "tags": ["dark", "modern"],
  "license": "MIT",
  "homepage": "https://github.com/me/my-theme",
  "de_support": ["any"]
}
</pre>

<h3>🎨 Categories (the <code>type</code> field)</h3>
<p><b>Appearance</b> — Rexon installs the files automatically:<br>
<code>theme-gtk, theme-icons, theme-cursors, theme-sounds,
theme-kde-global, theme-kde-colors, theme-kde-plasma, theme-kvantum,
theme-window-decoration, theme-login-screen, theme-plymouth, theme-conky,
theme-bundle, theme-plasmoid, wallpaper-pack, font-pack</code></p>

<p><b>Desktop customization</b> — Rexon puts it into that desktop's own
folder and turns it on with the desktop's own tools:<br>
<code>kde-kwin-script, kde-kwin-effect, kde-window-switcher,
kde-wallpaper-plugin, theme-kde-splash, theme-kde-lockscreen,
gnome-shell-extension, theme-gnome-shell, cinnamon-applet, cinnamon-desklet,
cinnamon-extension, theme-cinnamon, nemo-action, theme-xfwm4, theme-marco,
theme-lxqt</code></p>

<p><b>Extensions</b> (you get a warning on install — they run code):<br>
<code>desktop-tool</code>, <code>plugin</code></p>

<h3>📦 Packing</h3>
<pre>
cd my-extension/
zip -r ../my-extension.rxn .
</pre>

<p>Drag the finished <code>.rxn</code> into the Rexon window or click
<b>「📦 From File」</b>.</p>

<h3>🔗 Full documentation</h3>
<p>In the Rexon sources: <code>extensions/constants.py</code> has the full list
of types and install paths.</p>
"#);
    // The archive tree, JSON and commands — left to right in any language.
    crate::ui_base::pre_left_to_right(&body)
}

// ============================================================================
// The live disk: where extensions lie and what is installed there
// ============================================================================

// Where extensions lie is decided by the SINGLE point of the core
// ([`crate::utils::rexon_config_dir`]), not by a custom gluing of `~/.config`:
// that would cost the storefront its contents. The executor
// (`rexon/extensions/manager.py`) computes its path via `utils.expand_user`,
// i.e. HONORING `$XDG_CONFIG_HOME` — and on a machine with a non-standard
// config root the installation went into one directory while the storefront
// read another and stayed empty (rule #12).

/// `$XDG_CONFIG_HOME/rexon/extensions` — the same directory as the Python manager's.
pub fn extensions_dir() -> PathBuf {
    crate::utils::rexon_config_dir().join("extensions")
}

/// `$XDG_CONFIG_HOME/rexon/extensions_state.json` — the "is it enabled" map.
pub fn state_file() -> PathBuf {
    crate::utils::rexon_config_dir().join("extensions_state.json")
}

/// Read the state map from disk (a missing file — an empty map).
pub fn load_state() -> Map<String, Value> {
    let text = std::fs::read_to_string(state_file()).ok();
    ext_manager::load_state(text.as_deref())
}

/// Read the installed extensions FROM DISK.
///
/// The sibling of [`ext_manager::discover`] for the live file system — just as
/// [`Extension::load_from_dir`] is a sibling of `from_manifest`. The semantics
/// are the same: a directory with a parsable `manifest.json`, duplicates by `id`
/// are overwritten by the last one, the order is the order of the first
/// appearance of the `id`.
///
/// Warning: the directory names are sorted: `read_dir` returns them in file
/// system order, and Python's `os.scandir` in its own. Without sorting the
/// storefront would change the order of cards from machine to machine.
pub fn installed_in(dir: &Path, state: &Map<String, Value>) -> Vec<Extension> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    names.sort();

    let mut order: Vec<String> = Vec::new();
    let mut by_id: std::collections::HashMap<String, Extension> = std::collections::HashMap::new();
    for name in names {
        let path = dir.join(&name);
        if !path.is_dir() {
            continue;
        }
        let Ok(ext) = Extension::load_from_dir(&path, state) else {
            continue;
        };
        let id = ext.ext_id().to_string();
        if !by_id.contains_key(&id) {
            order.push(id.clone());
        }
        by_id.insert(id, ext);
    }
    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// Installed extensions from the standard directory, already broken down into fields.
pub fn installed() -> Vec<ExtView> {
    let state = load_state();
    installed_in(&extensions_dir(), &state)
        .iter()
        .map(|ext| {
            let mut view = ExtView::from_extension(ext);
            // For an environment add-on "enabled" is the environment's truth, not a
            // Rexon mark: the person may have toggled it in the desktop's settings.
            if !crate::ext_exec::de_addon_objects(ext).is_empty() {
                view.enabled = crate::ext_exec::de_addon_enabled(ext);
            }
            view
        })
        .collect()
}

/// Read the extension's README (for the detailed view).
///
/// The core had only the path (`readme_path`), and Python read the text itself.
pub fn read_readme(ext: &ExtView) -> Option<String> {
    if ext.readme_path.is_empty() {
        return None;
    }
    std::fs::read_to_string(&ext.readme_path).ok()
}

/// Statistics over the broken-down extensions — a single count for the product.
pub fn stats(exts: &[ExtView]) -> Map<String, Value> {
    let pairs: Vec<(String, bool)> = exts
        .iter()
        .map(|e| (e.ext_type.clone(), e.enabled))
        .collect();
    ext_manager::stats(&pairs)
}

/// The types that are actually installed — the input of [`filter_items`].
pub fn installed_types(exts: &[ExtView]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in exts {
        if !out.iter().any(|t| *t == e.ext_type) {
            out.push(e.ext_type.clone());
        }
    }
    out
}
