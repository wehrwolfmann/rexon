//! The "Appearance" tab: layout numbers, labels and style sheets.
//!
//! This is where what the tab KNOWS about its look lives: the tile and
//! details panel sizes, the preview loader thresholds, the debounce pauses,
//! the list of sorts and ready style sheets. The Python side remains a Qt
//! layer: it builds widgets and plugs in what it is told; it has no numbers
//! or colours of its own.
//!
//! ⚠️ Colours come from the ACTIVE skin (`ui_base::skin_colors`), not from
//! the baked-in Steam palette. A copy of that palette lived in the tab as a
//! separate dictionary and was not updated when the skin changed: the
//! details labels, the install path, the comments and the suggestions
//! drop-down stayed white and became invisible on a light skin.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::appearance_cache;
use crate::i18n::{fill, format_decimal, n_, tr, trc, trf};
use crate::appearance_installer;
use crate::categories;
use crate::installed_scan;
use crate::ocs;
use crate::ocs_store;
use crate::translator;
use crate::models::{CatalogSource, ThemeCategory, ThemeItem};
use crate::ui_base;
use crate::wallpaper_screens::WallpaperOrientation;

// ═══════════════════════════════════════════════════════════════════════
//  Catalog geometry
// ═══════════════════════════════════════════════════════════════════════

/// How many works we ask OCS for per server page. The server actually
/// returns about ten: this is taken into account when counting pages.
///
/// This is also the LOWER bound of a Rexon page: a grid page is assembled
/// from whole rows ([`grid_page_size`]) and may be slightly larger, but not
/// smaller. The server page does not depend on the number of columns;
/// otherwise the store cache (`ocs_cache`, a key without the size) would
/// return slices of a different length.
pub const PAGE_SIZE: u32 = 20;

/// The grid page size with `cols` columns: whole rows, no fewer than
/// [`PAGE_SIZE`] works. Otherwise, with nine columns, a page of twenty
/// ended with a row of two tiles and seven empty places.
pub fn grid_page_size(cols: u32) -> u32 {
    let cols = cols.max(1);
    ui_base::grid_page_size(cols, PAGE_SIZE.div_ceil(cols))
}

/// The page number after its size changes: the first work of the previous
/// page stays on screen.
pub fn page_after_resize(page: i64, old_size: u32, new_size: u32) -> i64 {
    let first = page.max(0).saturating_mul(i64::from(old_size.max(1)));
    first / i64::from(new_size.max(1))
}

/// The catalog tile width. Constant: the common grid engine computes the
/// number of columns from it, and a "floating" tile would break the
/// calculation.
pub const CARD_W: i32 = 300;
/// The full height of a catalog tile.
pub const CARD_H: i32 = 285;
/// The height of the picture in a tile.
pub const PREVIEW_H: i32 = 170;
/// The tile body margins (12 + 12) plus a reserve for the rounding: the
/// labels inside a tile are narrower than the tile itself by this much.
pub const CARD_TEXT_MARGIN: i32 = 30;

/// The width of labels inside a tile.
pub fn card_text_width() -> i32 {
    CARD_W - CARD_TEXT_MARGIN
}

/// The tile body side margins.
pub const CARD_BODY_MARGIN: i32 = 12;
/// The tile body top margin.
///
/// ⚠️ The number is taken from the original's RENDERED tile, not from its
/// source, and differs from it deliberately. The source declares 14, but
/// the tile has a fixed height (`CARD_H`), and the body content does not
/// fit: `QVBoxLayout` eats the margins and gaps in such a squeeze, and the
/// top margin degenerates almost to zero in the live rendering (screenshot
/// measurement on 01.09.2026: the label starts 2 points below the picture
/// border, not 14). A QML layout does not shrink margins: it overflows the
/// bottom edge, and the tile button got cut off. So what is written here is
/// what the person SEES.
pub const CARD_BODY_TOP_MARGIN: i32 = 2;
/// The tile body bottom margin.
pub const CARD_BODY_BOTTOM_MARGIN: i32 = 10;
/// The gap between lines inside a tile.
pub const CARD_BODY_SPACING: i32 = 4;
/// The tile button height (measured on the original's rendered button).
pub const CARD_BUTTON_HEIGHT: i32 = 32;
/// The offset of the button row from the stats line.
pub const CARD_BUTTON_TOP_MARGIN: i32 = 4;
/// The font size of the theme name in a tile. Larger than the other lines:
/// the name is what the person finds the theme by in the grid.
pub const CARD_NAME_POINT_SIZE: i32 = 11;
/// The font size of secondary tile lines (author, source, statistics).
pub const CARD_META_POINT_SIZE: i32 = 10;

/// The gap between grid tiles.
pub const GRID_SPACING: i32 = 14;
/// The grid side margins: the same 16 as the header and the status line;
/// otherwise the left edge of the tiles does not line up with the left edge
/// of the labels above them.
pub const GRID_MARGIN_H: i32 = 16;
/// The grid top and bottom margins.
pub const GRID_MARGIN_V: i32 = 12;

/// How many tile columns fit into width `usable` (grid margins already
/// subtracted; the gap after the last column is part of the width): how
/// many tiles of the smallest width [`CARD_W`] with a gap fit, at least
/// one. One calculation for all "Appearance" tile grids (the store catalog,
/// the Razer Axon library); otherwise one stretches and the other leaves an
/// empty strip on the right.
pub fn grid_columns(usable: i32) -> i32 {
    (usable / (CARD_W + GRID_SPACING)).max(1)
}

/// The cell width (tile plus gap) at width `usable`: the remaining width is
/// divided between the [`grid_columns`] columns, but never narrower than
/// the smallest tile.
pub fn grid_cell_width(usable: i32) -> i32 {
    (usable / grid_columns(usable)).max(CARD_W + GRID_SPACING)
}
/// The catalog header side margins.
pub const HEADER_MARGIN_H: i32 = 16;
/// The catalog header top and bottom margins.
pub const HEADER_MARGIN_V: i32 = 12;

/// The width of the categories sidebar.
pub const SIDEBAR_W: i32 = 210;
/// The height of a sidebar row.
pub const SIDEBAR_ROW_H: i32 = 40;
/// The width of the search line.
pub const SEARCH_W: i32 = 240;

/// The size of the picture on the details panel.
pub const DETAIL_PREVIEW_W: i32 = 600;
/// The height of the picture on the details panel.
pub const DETAIL_PREVIEW_H: i32 = 340;

// ── Overlay buttons on top of the details picture ──────────────────────
/// The side of a round button (magnifier, gallery arrows). At least 40: the
/// Big Picture requirement for the size of a tap target.
pub const OVERLAY_BTN: i32 = 44;
/// The offset of an overlay button from the picture edge.
pub const OVERLAY_MARGIN: i32 = 10;
/// The height of the "2/5" counter badge.
pub const COUNTER_H: i32 = 24;
/// The smallest width of the counter badge.
pub const COUNTER_MIN_W: i32 = 48;
/// The padding inside the counter badge (left and right together).
pub const COUNTER_PAD: i32 = 20;

/// The width of the counter badge for text of a given width.
pub fn counter_width(text_w: i32) -> i32 {
    COUNTER_MIN_W.max(text_w + COUNTER_PAD)
}

// ── Search suggestions drop-down ───────────────────────────────────────
/// The height of a suggestion row.
pub const SUGGEST_ROW_H: i32 = 54;
/// The width of the thumbnail in a suggestion (16:9).
pub const SUGGEST_THUMB_W: i32 = 72;
/// The height of the thumbnail in a suggestion.
pub const SUGGEST_THUMB_H: i32 = 40;
/// The maximum number of suggestions shown.
pub const SUGGEST_MAX_ROWS: i32 = 12;
/// The smallest width of the drop-down: a thumbnail plus two lines of text.
pub const SUGGEST_MIN_W: i32 = 360;
/// The gap from the right edge of the page that the drop-down does not cross.
pub const SUGGEST_EDGE: i32 = 12;
/// The smallest offset of the drop-down from the left edge of the page.
pub const SUGGEST_LEFT: i32 = 8;
/// The gap between the search line and the drop-down.
pub const SUGGEST_GAP: i32 = 3;
/// The drop-down border, top and bottom together.
pub const SUGGEST_FRAME: i32 = 2;
/// The width at which the name and the author are cut in a suggestion row.
pub const SUGGEST_TEXT_W: i32 = 300;
/// From how many typed characters we ask for suggestions at all.
pub const SUGGEST_MIN_CHARS: usize = 2;

/// The width of the suggestions drop-down: no narrower than the search line
/// and no narrower than a reasonable minimum.
pub fn suggest_width(anchor_w: i32) -> i32 {
    anchor_w.max(SUGGEST_MIN_W)
}

/// The height of the drop-down for a given number of rows.
pub fn suggest_height(rows: i32) -> i32 {
    rows.clamp(0, SUGGEST_MAX_ROWS) * SUGGEST_ROW_H + SUGGEST_FRAME
}

/// The left edge of the drop-down: under the search line, but not past the page edge.
pub fn suggest_x(anchor_x: i32, width: i32, page_w: i32) -> i32 {
    let right_limit = page_w - SUGGEST_EDGE;
    if anchor_x + width > right_limit {
        SUGGEST_LEFT.max(right_limit - width)
    } else {
        anchor_x
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Pauses
// ═══════════════════════════════════════════════════════════════════════

/// The typing pause before requesting suggestions: we do not query the store per character.
pub const SUGGEST_DEBOUNCE_MS: u32 = 220;
/// The pause before a network catalog request: while the person runs the
/// mouse over the category list, the network is not touched.
pub const CATALOG_DEBOUNCE_MS: u32 = 200;
/// The typing pause before a store search: the request goes out when the
/// person stops typing, not on every letter (Enter is immediate).
pub const SEARCH_DEBOUNCE_MS: u32 = 300;
/// How soon after the tab is shown the first category is loaded.
pub const FIRST_LOAD_MS: u32 = 120;
/// How soon the background warm-up of the other categories starts: it must
/// not compete for the network with the open category.
pub const PREFETCH_START_MS: u32 = 900;
/// How soon stale translations are cleaned up (once, not on the hot path).
pub const PURGE_TRANSLATIONS_MS: u32 = 1500;
/// The pause between background category warm-ups, so as not to hammer the server.
pub const PREFETCH_PAUSE_MS: u32 = 450;
/// The pause before rebuilding the grid after a theme is applied: applying
/// a Global Theme triggers a Qt style change in our own process, and
/// rebuilding widgets at that very moment crashed the application.
pub const STYLE_STORM_MS: u32 = 600;
/// The pause before the background warm-up of the shown grid's gallery: the
/// tiles themselves must arrive first, and the category must "settle".
pub const GALLERY_WARM_DELAY_MS: u32 = 800;

// ═══════════════════════════════════════════════════════════════════════
//  Preview loader
// ═══════════════════════════════════════════════════════════════════════

/// How many pictures are downloaded at the same time.
pub const PREVIEW_MAX_INFLIGHT: usize = 6;
/// How many of them the BACKGROUND warm-up may occupy: live tiles always
/// have room left.
pub const PREVIEW_MAX_INFLIGHT_LOW: usize = 2;
/// How many extra attempts are given for a transient network failure.
pub const PREVIEW_RETRIES: u32 = 2;
/// The pause before a retry (grows with the attempt number).
pub const PREVIEW_RETRY_DELAY_MS: u32 = 700;
/// A response larger than this is treated as a failure, not a picture.
pub const PREVIEW_MAX_BYTES: usize = 20 * 1024 * 1024;

/// Server responses for which retrying is pointless: the file does not
/// exist or access is denied. Everything else (a dropped connection, a
/// timeout, 5xx) is transient.
pub const PREVIEW_FATAL_HTTP: &[u16] = &[400, 401, 403, 404, 405, 410, 451];

/// Whether to retry a preview download after such a server response.
pub fn preview_retry_allowed(status: Option<u16>, tries: u32) -> bool {
    if let Some(code) = status {
        if PREVIEW_FATAL_HTTP.contains(&code) {
            return false;
        }
    }
    tries <= PREVIEW_RETRIES
}

// ═══════════════════════════════════════════════════════════════════════
//  Catalog sorts
// ═══════════════════════════════════════════════════════════════════════

/// The order of sorts in the drop-down list: `(label, OCS key)`. The label
/// is a `msgid`, translated where shown ([`sort_label`]).
/// The default is "New": by rating the same works have sat at the top for
/// years, and the showcase looks dead on every visit.
pub const SORTS: &[(&str, &str)] = &[
    (n_("🆕 Newest"), "new"),
    (n_("⬇️ Most Downloaded"), "down"),
    (n_("⭐ Top Rated"), "high"),
    (n_("🔤 Alphabetical"), "alpha"),
];

/// The label of a sort from [`SORTS`] in the interface language.
pub fn sort_label(label: &str) -> String {
    tr(label)
}

/// The default sort.
pub const DEFAULT_SORT: &str = "new";

/// The position of the default sort in the list (for setting it in the combo box).
pub fn default_sort_index() -> usize {
    SORTS
        .iter()
        .position(|(_, key)| *key == DEFAULT_SORT)
        .expect("сортировка по умолчанию обязана быть в таблице")
}

// ═══════════════════════════════════════════════════════════════════════
//  Numbers for people
// ═══════════════════════════════════════════════════════════════════════

/// A download/vote counter in short form: 1234 -> "1.2K".
pub fn human_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{}M", format_decimal(n as f64 / 1_000_000.0, 1))
    } else if n >= 1000 {
        format!("{}K", format_decimal(n as f64 / 1000.0, 1))
    } else {
        n.to_string()
    }
}

/// The item archive size for people: "13 KiB", "1.4 MiB".
///
/// ⚠️ The store returns `downloadsize1` in KIBIBYTES, not bytes: a live
/// response on 04.10.2026: the "Aemeath" cursors have `downloadsize1=13`
/// with a 13 KiB archive, "ba-click-fx" `1419` with 1.4 MiB (Discover shows
/// them the same way). Empty means the size is unknown (0 or less).
pub fn human_size_kib(kib: i64) -> String {
    if kib <= 0 {
        return String::new();
    }
    // The separator and digits follow the interface language: Russian and
    // German use a comma ([`crate::i18n::format_decimal`]).
    let one_decimal = |v: f64| format_decimal(v, 1);
    let kib_f = kib as f64;
    if kib < 1024 {
        // TRANSLATORS: file size in kibibytes, %1 is the number.
        fill(&trc("file size", "%1 KiB"), &[&format_decimal(kib_f, 0)])
    } else if kib_f < 1024.0 * 1024.0 {
        // TRANSLATORS: file size in mebibytes, %1 is the number.
        fill(&trc("file size", "%1 MiB"), &[&one_decimal(kib_f / 1024.0)])
    } else {
        // TRANSLATORS: file size in gibibytes, %1 is the number.
        fill(&trc("file size", "%1 GiB"), &[&one_decimal(kib_f / (1024.0 * 1024.0))])
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Style sheets
// ═══════════════════════════════════════════════════════════════════════

fn token<'a>(colors: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    colors
        .get(key)
        .map(String::as_str)
        .or_else(|| {
            ui_base::APPEARANCE_STEAM_COLORS
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
        })
        .unwrap_or("#ffffff")
}

/// The tab tokens for the active skin. The full set of keys is guaranteed
/// by the core, so substitution into the style sheet never falls apart.
pub fn colors() -> BTreeMap<String, String> {
    ui_base::active_skin_colors()
}

/// The style sheet of the whole tab, assembled from the tokens of skin `c`.
///
/// For the `system`/`steam_dark` skins the tokens match the historical
/// Steam palette, so the tab looks the same as before.
pub fn page_qss(c: &BTreeMap<String, String>) -> String {
    let bg_primary = token(c, "bg_primary");
    let bg_secondary = token(c, "bg_secondary");
    let bg_card = token(c, "bg_card");
    let bg_card_hover = token(c, "bg_card_hover");
    let accent = token(c, "accent");
    let accent_green = token(c, "accent_green");
    let accent_green_hover = token(c, "accent_green_hover");
    let text_primary = token(c, "text_primary");
    let text_secondary = token(c, "text_secondary");
    let text_dim = token(c, "text_dim");
    let border = token(c, "border");
    let sb = token(c, "scrollbar");
    let rating_star = token(c, "rating_star");
    let author_self = token(c, "author_self");

    format!(
        r#"
#RightPane, #GridHost, #GridScroll, #TopBar, #Pagination, #DetailView,
#DetailHost {{
    background: {bg_primary};
}}
QScrollArea {{ background: {bg_primary}; border: none; }}

/* Scrollbar: a visible handle by default (not only on hover).
   The colour is the skin-dependent RGB base c['scrollbar'] (white on dark skins,
   black on the light one): 0.28 at rest -> 0.45 hover -> 0.60 pressed. */
QScrollBar:vertical {{
    background: transparent;
    width: 11px;
    margin: 0;
    border: none;
}}
QScrollBar::handle:vertical {{
    background: rgba({sb},0.28);
    border-radius: 5px;
    min-height: 28px;
}}
QScrollBar::handle:vertical:hover   {{ background: rgba({sb},0.45); }}
QScrollBar::handle:vertical:pressed {{ background: rgba({sb},0.60); }}
QScrollBar:horizontal {{
    background: transparent;
    height: 11px;
    margin: 0;
    border: none;
}}
QScrollBar::handle:horizontal {{
    background: rgba({sb},0.28);
    border-radius: 5px;
    min-width: 28px;
}}
QScrollBar::handle:horizontal:hover   {{ background: rgba({sb},0.45); }}
QScrollBar::handle:horizontal:pressed {{ background: rgba({sb},0.60); }}
QScrollBar::add-line, QScrollBar::sub-line {{
    width: 0; height: 0; background: none; border: none;
}}
QScrollBar::add-page, QScrollBar::sub-page {{ background: none; }}

#Sidebar {{
    background: {bg_secondary};
    border: none;
    outline: 0;
    color: {text_primary};
    font-size: 11pt;
    font-weight: 600;
}}
#Sidebar::item {{
    padding: 8px 4px;
    border-left: 3px solid transparent;
    color: {text_secondary};
}}
#Sidebar::item:selected {{
    background: {bg_card_hover};
    border-left: 3px solid {accent};
    color: {accent};
}}
#Sidebar::item:hover {{
    background: {bg_card};
    color: {text_primary};
}}

#PageTitle, #DetailName {{ color: {text_primary}; }}
#StatusBar {{ color: {text_secondary}; font-size: 10pt; font-weight: 600; }}
/* The explanation "this section does not exist in your desktop" instead of an empty
   grid. Large, centred, readable in Big Picture too (font >= 14pt). */
#DeNote {{ color: {text_secondary}; font-size: 14pt; line-height: 150%; }}

#SrcLabel {{ color: {text_secondary}; padding-right: 2px; font-weight: 600; }}
#SrcChip {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 5px 10px;
    color: {text_secondary};
}}
#SrcChip:hover {{
    background: {bg_card};
    color: {text_primary};
}}
#SrcChip:checked {{
    background: {bg_card_hover};
    border: 1px solid {accent};
    color: {accent};
    font-weight: bold;
}}

#SearchBox {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 6px 8px;
    color: {text_primary};
}}
#SearchBox:focus {{ border: 1px solid {accent}; }}
#SortCombo {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 5px 8px;
    color: {text_primary};
    min-width: 130px;
}}
#SortCombo QAbstractItemView {{
    background: {bg_secondary};
    color: {text_primary};
    selection-background-color: {bg_card_hover};
}}
#RefreshBtn, #PageBtn, #BackBtn {{
    background: {bg_card};
    border: 1px solid {border};
    border-radius: 4px;
    padding: 6px 12px;
    color: {text_primary};
}}
#RefreshBtn:hover, #PageBtn:hover, #BackBtn:hover {{
    background: {bg_card_hover};
    border: 1px solid {accent};
}}
/* An inactive page button is a dimmed version of THE SAME colour as the scrollbar.
   Previously this was white with its own transparency, and on the light skin
   the "← Back" label vanished against the light background. */
#PageBtn:disabled {{ color: rgba({sb},0.55); }}
#PageLabel {{ color: {text_secondary}; padding: 0 12px; font-weight: 600; }}

#ThemeCard {{
    background: {bg_card};
    border: 1px solid {border};
    border-radius: 4px;
}}
#ThemeCard:hover {{
    border: 1px solid {accent};
    background: {bg_card_hover};
}}
#ThemeCard QLabel {{ background: transparent; border: none; }}
#CardPreview {{
    background: {bg_secondary};
    border-top-left-radius: 4px;
    border-top-right-radius: 4px;
    color: {text_dim};
}}
#CardName {{ color: {text_primary}; }}
#CardAuthor {{ color: {text_primary}; font-size: 10pt; font-weight: 600; }}
#CardStats {{ color: {text_primary}; font-size: 10pt; font-weight: bold; }}
/* The rating icon (★ in the appearance catalog, 👍 in the AUR) is the only coloured
   spot of the stats line. It used to be a <span style='color:…'> inside the same
   label: the colour was baked into the text when the tile was built and did not
   follow a skin change. A separate label with its own objectName is painted by the
   style sheet for free, by the same setStyleSheet as the whole tab. The size and
   weight repeat #CardStats: the icon lived in its font. */
#CardStar {{ color: {rating_star}; font-size: 10pt; font-weight: bold; }}

#InstallBtn, #InstallApplyBtn {{
    background: {accent_green};
    border: none;
    border-radius: 3px;
    padding: 6px 10px;
    color: white;
    font-weight: bold;
}}
#InstallBtn:hover, #InstallApplyBtn:hover {{ background: {accent_green_hover}; }}
/* "✓ Applied" is a muted grey-green (the item is already installed/applied).
   It stays clickable for reapplying; it is visible on a dark background but does
   not shout like a bright green one. */
#InstallApplyBtn[applied="true"] {{
    background: #3a5a3f;
    color: #cfe8d2;
    font-weight: bold;
}}
#InstallApplyBtn[applied="true"]:hover {{ background: #46704c; }}
/* "✓ Installed" on a catalog card is NOT a fill but an outline: a filled
   green button = an offer of an action, an outlined teal one = a state.
   One shade is not enough: "Install" and "Installed" in the same green
   merged together. */
#InstallBtn[installed="true"] {{
    background: transparent;
    border: 1px solid #4fd1c5;
    color: #7fe3da;
    font-weight: bold;
}}
#InstallBtn[installed="true"]:hover {{
    background: rgba(79, 209, 197, 0.16);
    border: 1px solid #7fe3da;
    color: #a8efe8;
}}
/* "✓ Applied" on a catalog card is a FILLED teal one. The third state
   must differ from both neighbours, so both features change at once:
   the fill (against the "installed" outline) and the shade (against the green
   "Install" button). The border has the same colour as the background so that
   the button does not "jump" in size when switching from the outlined look. */
#InstallBtn[installed="applied"] {{
    background: #2f9e94;
    border: 1px solid #2f9e94;
    color: #ffffff;
    font-weight: bold;
}}
#InstallBtn[installed="applied"]:hover {{
    background: #3ab8ac;
    border: 1px solid #4fd1c5;
    color: #ffffff;
}}
/* Details: "✓ Installed" has the same outlined look as the card, so that
   the grid and the details do not tell different stories about the item. */
#InstallApplyBtn[applied="installed"] {{
    background: transparent;
    border: 1px solid #4fd1c5;
    color: #7fe3da;
    font-weight: bold;
}}
#InstallApplyBtn[applied="installed"]:hover {{
    background: rgba(79, 209, 197, 0.16);
    border: 1px solid #7fe3da;
    color: #a8efe8;
}}
#DetailsBtn, #DownloadOnlyBtn, #CommentsMoreBtn {{
    background: {bg_card_hover};
    border: 1px solid {border};
    border-radius: 3px;
    padding: 6px 10px;
    color: {text_primary};
}}
#DetailsBtn:hover, #DownloadOnlyBtn:hover, #CommentsMoreBtn:hover {{
    border: 1px solid {accent};
}}
/* The same fix as for "← Back": dimming comes from the skin colour, not white. */
#CommentsMoreBtn:disabled {{ color: rgba({sb},0.55); }}
#UninstallBtn {{
    background: #5a2230;
    border: 1px solid #803040;
    border-radius: 3px;
    padding: 6px 12px;
    color: #ffc0cb;
}}
#UninstallBtn:hover {{ background: #803040; }}

#DetailView QLabel {{ background: transparent; }}
#DetailPreview {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 4px;
    color: {text_dim};
}}
#DetailDesc {{ color: {text_primary}; font-size: 11pt; }}
/* The item meta line ("author • licence • updated"), the counters and the
   "Category / install path" line are also painted from here rather than
   assembled as HTML at display time: otherwise a skin change left an ALREADY OPEN
   details view in the previous skin's colours while the window around it was repainted. */
#DetailMeta {{ color: {text_secondary}; }}
#DetailStars {{ color: {rating_star}; font-size: 14pt; }}
#DetailDownloads {{ color: {text_secondary}; }}
#DetailPath {{ color: {text_dim}; font-weight: 600; font-size: 10.5pt; }}
#CommentsHeader {{ color: {text_primary}; margin-top: 10px; }}
#CommentsHost {{ background: transparent; }}
#CommentItem {{ color: {text_primary}; font-size: 10.5pt; }}
/* The comment nick and date. The nick of the item author HIMSELF is bright (author_self):
   the flag is set by a dynamic property so that the rule lives in the style
   sheet rather than in the label text. */
#CommentAuthor {{ color: {text_primary}; font-size: 10.5pt; font-weight: bold; }}
#CommentAuthor[own="true"] {{ color: {author_self}; }}
#CommentDate {{ color: {text_dim}; font-size: 9.5pt; font-weight: 600; }}
#CommentsEmpty {{ color: {text_dim}; font-weight: 600; }}
#DetailProgress {{
    background: {bg_secondary};
    border: 1px solid {border};
    border-radius: 3px;
    text-align: center;
    color: {text_primary};
}}
#DetailProgress::chunk {{ background: {accent}; border-radius: 3px; }}
"#
    )
}

/// The style sheet of the search suggestions drop-down.
///
/// Previously the colour set here was its own and baked for Steam: on the
/// light skin a dark blue panel unfolded in the middle of a light page, and
/// the grey-blue author broke the rule "only white on a dark background".
/// Now the panel is assembled from the same tokens as the whole tab.
pub fn suggestions_qss(c: &BTreeMap<String, String>) -> String {
    let bg_secondary = token(c, "bg_secondary");
    let bg_card = token(c, "bg_card");
    let bg_card_hover = token(c, "bg_card_hover");
    let accent = token(c, "accent");
    let text_primary = token(c, "text_primary");
    let text_dim = token(c, "text_dim");
    format!(
        "#SuggestPopup {{ background:{bg_secondary};\
 border:1px solid {accent}; border-radius:6px; }}\
#SuggestList {{ background:transparent; border:none;\
 outline:none; }}\
#SuggestList::item {{ border:none; padding:0px;\
 border-radius:4px; }}\
#SuggestList::item:selected, #SuggestList::item:hover {{\
 background:{bg_card_hover}; }}\
#SuggTitle {{ color:{text_primary}; font-weight:700; background:transparent; }}\
#SuggAuthor {{ color:{text_dim}; background:transparent; }}\
#SuggThumb {{ background:{bg_card};\
 border-radius:3px; color:{accent}; }}"
    )
}

/// An overlay button on top of the picture: round, semi-transparent black.
///
/// The colour here is deliberately NOT skin-dependent: the button lies on a
/// photo, not on the page background, and reads on any picture only by
/// darkening. `point_size` tells the magnifier (large) from the gallery
/// arrows.
pub fn image_overlay_qss(point_size: f32) -> String {
    let radius = OVERLAY_BTN / 2;
    format!(
        "QPushButton {{ background-color: rgba(0,0,0,0.55); \
color: white; border: none; border-radius: {radius}px; \
font-size: {point_size}pt; }}\
QPushButton:hover, QPushButton:focus \
{{ background-color: rgba(0,0,0,0.85); }}"
    )
}

/// The font size of the "enlarge to full screen" magnifier.
pub const OVERLAY_ZOOM_PT: f32 = 20.0;
/// The font size of the gallery arrows.
pub const OVERLAY_NAV_PT: f32 = 16.0;

/// The style sheet of the "2/5" counter badge over the picture.
pub fn gallery_counter_qss() -> String {
    let radius = COUNTER_H / 2;
    format!(
        "QLabel {{ background-color: rgba(0,0,0,0.55); color: white; \
border: none; border-radius: {radius}px; \
font-size: 10.5pt; font-weight: 600; }}"
    )
}

// ═══════════════════════════════════════════════════════════════════════
//  The installed log: "installed" and "applied" are different states
// ═══════════════════════════════════════════════════════════════════════

/// The `rexon_type` of a log entry -> the `apply_kind` of the active theme:
/// the reverse side of `appearance_installer::kind_to_rexon_type`. By the
/// type from the log we learn in which row of the active themes map to
/// look for the item name.
///
/// ⚠️ COMPATIBILITY WITH OLD ENTRIES. Until 2026-08-22 the core dumped
/// wallpapers, sounds, fonts and widgets into one type "wallpaper-pack",
/// and the splash, the lock screen, GTK and the application style into
/// "plugin". Customers already have such entries in the log, and they must
/// not be broken. So both former dumps are NOT mentioned here at all: the
/// kind cannot be determined from them, and the entry honestly goes to the
/// second pass, by OCS category (the log always has `category_id`).
pub fn rexon_type_to_apply_kind(rexon_type: &str) -> &'static str {
    match rexon_type.trim() {
        "theme-kde-global" => "global",
        "theme-kde-plasma" => "plasma",
        "theme-kde-colors" => "colors",
        "theme-kde-aurorae" => "aurorae",
        "theme-icons" => "icons",
        "theme-cursors" => "cursor",
        "theme-kvantum" => "kvantum",
        "theme-sddm" => "sddm",
        "theme-plymouth" => "plymouth",
        // Kinds that have had their own type instead of the "plugin" dump since 2026-08-22.
        "theme-gtk" => "gtk",
        "theme-widget-style" => "widgetstyle",
        "theme-kde-splash" => "splash",
        "theme-kde-lockscreen" => "lockscreen",
        // Sound is a full-fledged kind: it has "applied now".
        "theme-sounds" => "sounds",
        // Fonts, widgets and wallpaper plugins also got their own types; they
        // have no applying, but the type now shows that it is them and not
        // wallpapers.
        "font-pack" => "fonts",
        "theme-plasmoid" => "plasmoids",
        "kde-wallpaper-plugin" => "wallpaper_plugins",
        _ => "",
    }
}

/// Theme names of a log entry: the basename of each installed path.
///
/// A colour scheme lies in a "<Name>.colors" file, while in the active
/// themes map it is named without the extension: we strip the suffix,
/// otherwise the scheme would never match.
pub fn log_theme_names(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths {
        let trimmed = p.trim_end_matches('/');
        let base = trimmed.rsplit('/').next().unwrap_or("");
        let base = if base.to_ascii_lowercase().ends_with(".colors") {
            &base[..base.len() - ".colors".len()]
        } else {
            base
        };
        if !base.is_empty() {
            out.push(base.to_string());
        }
    }
    out
}

/// The `apply_kind` of a log entry ("" means the kind has no notion of
/// "applied").
///
/// First by `rexon_type`, then by OCS category. The second pass is
/// mandatory both for old entries (both former type dumps) and for
/// "wallpaper-pack", which marks both ordinary wallpapers and the lock
/// screen background.
///
/// Whether a kind has a notion of "applied now" is decided by
/// `appearance_installer::kind_has_active_state`: a list next to the
/// active themes map, not a second such list in the tab (rule #12).
pub fn log_apply_kind(home: &Path, rec: &Map<String, Value>, item_category_id: i64) -> String {
    let rexon_type = rec.get("rexon_type").and_then(Value::as_str).unwrap_or("");
    let mut kind = rexon_type_to_apply_kind(rexon_type).to_string();
    if kind.is_empty() {
        // Zero in an entry is not "category 0" but "no field": then we take the
        // category from the card itself, as the original did.
        let mut cid = rec.get("category_id").and_then(Value::as_i64).unwrap_or(0);
        if cid == 0 {
            cid = item_category_id;
        }
        kind = categories::category_by_id(home, cid)
            .map(|c| c.apply_kind.trim().to_string())
            .unwrap_or_default();
    }
    if kind.is_empty() || !appearance_installer::kind_has_active_state(&kind) {
        return String::new();
    }
    kind
}

/// What to do with the active themes map when stamping a batch of cards.
///
/// The map is fetched by `installed_scan`, and it calls `kreadconfig6`, a
/// process. We call it ONLY when an applicable item was actually found in
/// the log, and exactly once per batch. Outside KDE the utility may not
/// exist at all: the grid must neither crash nor wait for a process on
/// every card.
pub struct LazyActiveMap<F: FnMut() -> Map<String, Value>> {
    fetch: F,
    cached: Option<Map<String, Value>>,
}

impl<F: FnMut() -> Map<String, Value>> LazyActiveMap<F> {
    pub fn new(fetch: F) -> Self {
        Self { fetch, cached: None }
    }

    /// The active themes map; the first call fetches it.
    pub fn get(&mut self) -> &Map<String, Value> {
        if self.cached.is_none() {
            self.cached = Some((self.fetch)());
        }
        self.cached.as_ref().expect("карта только что заполнена")
    }

    /// Whether the source was queried at all (for tests and measurements).
    pub fn was_fetched(&self) -> bool {
        self.cached.is_some()
    }
}

/// Set `installed` and `applied` on a batch of OCS cards from the log.
///
/// The log of installed OCS themes (`~/.config/rexon/ocs_installed.json`,
/// key = `content_id` = `item.id`) is the only source of truth about
/// whether an item is installed: when the catalog is loaded OCS returns
/// `installed=false`, and without this check the button showed "Install
/// and apply" again after a restart.
///
/// `applied` is a SEPARATE state: a theme can be downloaded with the
/// "Download only" button and not enabled. It is "applied" only if its
/// name matched the active theme of its kind.
pub fn stamp_installed_from_log<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log: &Map<String, Value>,
    items: &mut [ThemeItem],
    active: &mut LazyActiveMap<F>,
) {
    if items.is_empty() || log.is_empty() {
        return;
    }
    for it in items.iter_mut() {
        let Some(rec) = log.get(&it.id) else { continue };
        it.installed = true;
        it.applied = false;
        let Some(rec) = rec.as_object() else { continue };
        let mut kind = log_apply_kind(home, rec, it.category_id);
        // A Conky widget and an add-on: "applied" means running or enabled (the
        // active map can hold any number of them, newline-separated).
        let switch = kind.is_empty();
        if switch {
            kind = log_switch_kind(home, rec, it.category_id);
        }
        if kind.is_empty() {
            continue; // a kind without the notion of "applied" (fonts, plugins)
        }
        let cur = active
            .get()
            .get(&kind)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if cur.is_empty() {
            continue; // the appearance kind cannot be determined in this environment
        }
        let paths: Vec<String> = rec
            .get("paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|v| v.as_str().unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        let on: Vec<&str> = if switch { cur.split('\n').map(str::trim).collect() } else { vec![cur.as_str()] };
        it.applied = log_theme_names(&paths)
            .iter()
            .any(|n| on.contains(&n.trim().to_lowercase().as_str()));
    }
}

/// The item section kind from a log entry, if it is enabled by starting or
/// by a toggle ([`store_item_switch`]); otherwise empty.
fn log_switch_kind(home: &Path, rec: &Map<String, Value>, item_category_id: i64) -> String {
    let mut cid = rec.get("category_id").and_then(Value::as_i64).unwrap_or(0);
    if cid == 0 {
        cid = item_category_id;
    }
    categories::category_by_id(home, cid)
        .map(|c| c.apply_kind.trim().to_string())
        .filter(|k| switch_of_apply_kind(k).is_some())
        .unwrap_or_default()
}

/// The same stamping, but the log is read from disk at path `log_path`.
///
/// No file, broken JSON, a non-object at the top: the log is empty and the
/// cards are not touched (`ocs_store::parse_installed_log`). An empty batch
/// does not read the file.
pub fn stamp_installed_from_log_file<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log_path: &str,
    items: &mut [ThemeItem],
    active: &mut LazyActiveMap<F>,
) {
    if items.is_empty() {
        return;
    }
    // Read through the common install log (step 3 of merging the installers).
    let log = crate::install_journal::store_log_view(log_path);
    stamp_installed_from_log(home, &log, items, active);
}

/// The installation result on the cards of one item.
///
/// Success: the item is "installed" right away, even if writing the log
/// failed (the theme is on disk). "Applied" only by fact: by the log and
/// the active themes map, not by the intention "install and apply". For
/// kinds without applying the core answers "applying is not required", and
/// applying could also have failed: in both cases the item is only
/// installed. Failure: the previous state is left alone.
pub fn mark_installed_from_log_file<F: FnMut() -> Map<String, Value>>(
    home: &Path,
    log_path: &str,
    items: &mut [ThemeItem],
    ok: bool,
    active: &mut LazyActiveMap<F>,
) {
    if !ok {
        return;
    }
    for it in items.iter_mut() {
        it.installed = true;
        it.applied = false;
    }
    stamp_installed_from_log_file(home, log_path, items, active);
}

/// The installed log path honouring `$XDG_CONFIG_HOME`: the same path the
/// installer writes it to.
fn live_install_log_path(home: &str) -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    ocs_store::install_log_path_xdg(xdg.as_deref(), home)
}

/// The live active themes map for stamping (fetched by `installed_scan`).
///
/// The environment is the one running now: without it the map has no
/// environment sections (Marco/xfwm4 frames, shells), and their items from
/// the store did not become "applied".
fn live_active_map() -> Map<String, Value> {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let de = crate::system::detect_current_de(
        &var("XDG_CURRENT_DESKTOP"),
        &var("DESKTOP_SESSION"),
        !var("WAYLAND_DISPLAY").is_empty(),
    )
    .unwrap_or_default();
    installed_scan::LiveScan::new(Vec::new(), std::collections::HashMap::new())
        .with_de(&de)
        .active_map()
        .into_iter()
        .map(|(k, v)| (k, Value::from(v)))
        .collect()
}

/// Live stamping of a batch of OCS cards: the log from disk, the active
/// themes map lazily and no more than once per batch. It is called by all
/// places of the window where store items go on screen: the grid,
/// "Favourites", full details.
pub fn stamp_installed_live(home: &str, items: &mut [ThemeItem]) {
    let mut active = LazyActiveMap::new(live_active_map);
    stamp_installed_from_log_file(
        Path::new(home),
        &live_install_log_path(home),
        items,
        &mut active,
    );
}

/// The live variant of [`mark_installed_from_log_file`]: the installation result.
pub fn mark_installed_live(home: &str, items: &mut [ThemeItem], ok: bool) {
    let mut active = LazyActiveMap::new(live_active_map);
    mark_installed_from_log_file(
        Path::new(home),
        &live_install_log_path(home),
        items,
        ok,
        &mut active,
    );
}


// ═══════════════════════════════════════════════════════════════════════
//  Preview loader: queue, priority, dedup and retries
// ═══════════════════════════════════════════════════════════════════════

/// The preview cache directory of AUR cards: next to the ordinary one, a separate folder.
pub fn aur_preview_dir(home: &str) -> String {
    format!("{}/aur_previews", appearance_cache::cache_dir(home))
}

/// The directory of drawn placeholders for local theme tiles.
///
/// A directory of its own rather than shared with the store previews: that
/// one holds downloads, this one what we drew, and they will one day have
/// to be cleaned differently.
pub fn placeholder_dir(home: &str) -> String {
    format!("{}/local_placeholders", appearance_cache::cache_dir(home))
}

/// The placeholder file of a local theme that has no picture of its own on
/// disk.
///
/// The key includes EVERYTHING the frame depends on: the theme kind, the
/// install path, the name, the label and the file timestamp. The last one
/// is not for show: for a colour scheme the frame is drawn from ITS
/// content, swatches of the real colours, and after the scheme is edited it
/// must be redrawn rather than served stale from the cache.
pub fn placeholder_cache_path(
    home: &str,
    category_id: i64,
    install_path: &str,
    name: &str,
    author: &str,
    stamp: i64,
) -> String {
    // The separator is a character that never occurs in a path or a theme
    // name: without it "name A + label B" and "name AB + empty label" would
    // give one key and one frame for two different tiles.
    let key = [
        category_id.to_string().as_str(),
        install_path,
        name,
        author,
        stamp.to_string().as_str(),
    ]
    .join("\u{0}");
    format!(
        "{}/{}.png",
        placeholder_dir(home),
        crate::translator::sha1_hex(key.as_bytes())
    )
}

/// The host GitHub OpenGraph cards come from (AUR package previews).
pub const GITHUB_OPENGRAPH_HOST: &str = "opengraph.githubassets.com";

/// The local preview path by URL.
///
/// The naming scheme is shared with the store client
/// (`ocs::preview_cache_name`), and that is essential: if the two rules
/// diverged even in choosing the extension, the whole preview cache would
/// count as empty, and the pictures would be downloaded again on top of
/// files already lying nearby. The original kept a SECOND copy of this rule
/// in the tab (its own sha1 and its own extension parsing); there is no
/// copy here (rule #12).
///
/// GitHub OpenGraph cards (AUR previews) go to their own directory and are
/// always PNG: their links have no extension in the path at all.
pub fn preview_cache_path(home: &str, url: &str) -> String {
    if ocs::url_netloc(url) == GITHUB_OPENGRAPH_HOST {
        return format!(
            "{}/{}.png",
            aur_preview_dir(home),
            crate::translator::sha1_hex(url.as_bytes())
        );
    }
    format!(
        "{}/{}",
        appearance_cache::preview_dir(home),
        ocs::preview_cache_name(url)
    )
}

/// One preview request: whose card, which link and whether it is a background one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewJob {
    pub content_id: String,
    pub url: String,
    /// A background warm-up: such requests are not shown and give way to live tiles.
    pub low: bool,
}

/// What to do after a failed preview download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRetry {
    /// Retry after `delay_ms`; `attempt` is which attempt failed.
    Again { delay_ms: u32, attempt: u32 },
    /// Give up. `fatal` means the server answered "no file / access denied":
    /// that goes to the log as a calm line, not a warning.
    GiveUp { fatal: bool, attempts: u32 },
}

/// The preview loader queue: everything it KNOWS, without a single network
/// call.
///
/// The Qt layer stays outside: it only asks "what to start", reports "this
/// answer arrived" and hands ready paths upwards. The rules (how many
/// pictures to fetch at once, whom to let ahead, what counts as a duplicate
/// and when to retry) live here and are covered by acceptance tests.
///
/// ⚠ A request failure does NOT mean "no picture": images.pling.com
/// periodically closes the connection and drops the WHOLE batch of
/// simultaneous answers AT ONCE. So transient failures are retried, and
/// final ones (404/403/410) are not.
#[derive(Debug, Default)]
pub struct PreviewQueue {
    queue: Vec<PreviewJob>,
    /// One URL, one download: without dedup returning to a category sent the
    /// same requests again and flooded the channel.
    wanted: BTreeSet<String>,
    /// URL -> attempts made.
    tries: BTreeMap<String, u32>,
    inflight: usize,
    /// How many BACKGROUND downloads are in flight: the background may not take all slots.
    low_inflight: usize,
    started: u64,
    /// The generation: `cancel_all` increments it, and deferred retries of a
    /// past generation (the category has already changed) are silently
    /// dropped.
    gen: u64,
    /// Ready previews are held back and handed out one at a time: decoding six
    /// JPEGs in one pass of the event loop gives a noticeable stutter.
    ready: VecDeque<(String, String)>,
}

impl PreviewQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// The current generation (deferred retries carry it).
    pub fn generation(&self) -> u64 {
        self.gen
    }

    /// How many downloads have been started over the queue's whole life: this
    /// measures whether the warm-up "flooded" the channel.
    pub fn started(&self) -> u64 {
        self.started
    }

    pub fn inflight(&self) -> usize {
        self.inflight
    }

    pub fn low_inflight(&self) -> usize {
        self.low_inflight
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// One picture: a cache hit goes straight up (or silently), otherwise into
    /// the queue.
    ///
    /// Dedup is kept only for BACKGROUND requests. A live one must not be
    /// skipped by the same key: two catalog cards occasionally share one URL,
    /// and then the second would stay without a picture, since there would be
    /// nobody to send its "done" to.
    pub fn want(
        &mut self,
        content_id: &str,
        url: &str,
        low: bool,
        cached: Option<&str>,
        emit_cached: bool,
    ) {
        if url.is_empty() || (low && self.wanted.contains(url)) {
            return;
        }
        if let Some(local) = cached {
            if emit_cached && !low {
                self.ready.push_back((content_id.to_string(), local.to_string()));
            }
            return;
        }
        self.wanted.insert(url.to_string());
        self.push(content_id, url, low);
    }

    /// Put into the queue taking priority into account: live tiles before the
    /// background.
    ///
    /// The queue is kept as "all live first, then the background"; otherwise
    /// fifty background frames push back the tiles of the grid the person is
    /// looking at.
    fn push(&mut self, content_id: &str, url: &str, low: bool) {
        let job = PreviewJob {
            content_id: content_id.to_string(),
            url: url.to_string(),
            low,
        };
        if low {
            self.queue.push(job);
            return;
        }
        let pos = self.queue.iter().position(|j| j.low).unwrap_or(self.queue.len());
        self.queue.insert(pos, job);
    }

    /// What to start right now. The background gets no more than
    /// `PREVIEW_MAX_INFLIGHT_LOW` slots out of `PREVIEW_MAX_INFLIGHT`: the rest
    /// are always free for live cards; otherwise the gallery warm-up would
    /// take the whole channel and the "speed-up" would turn into a sluggish
    /// grid.
    pub fn pump(&mut self) -> Vec<PreviewJob> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.queue.len() && self.inflight < PREVIEW_MAX_INFLIGHT {
            if self.queue[i].low && self.low_inflight >= PREVIEW_MAX_INFLIGHT_LOW {
                i += 1;
                continue;
            }
            let job = self.queue.remove(i);
            self.started += 1;
            self.inflight += 1;
            if job.low {
                self.low_inflight += 1;
            }
            out.push(job);
        }
        out
    }

    /// The answer to a request arrived (any: successful, failed, cancelled).
    ///
    /// Dedup is removed HERE, not when queueing: while a picture is in flight,
    /// a repeated request for it must not spawn a second request.
    pub fn finished(&mut self, url: &str, low: bool) {
        self.inflight = self.inflight.saturating_sub(1);
        if low {
            self.low_inflight = self.low_inflight.saturating_sub(1);
        }
        self.wanted.remove(url);
    }

    /// Is the response body fit to be a picture? An empty or implausibly large
    /// one is a failure, not a preview, and must be retried.
    pub fn body_is_image(len: usize) -> bool {
        len > 0 && len <= PREVIEW_MAX_BYTES
    }

    /// The picture is written to the cache: reset the attempt counter and, if
    /// the request is live, put it into the output.
    pub fn succeeded(&mut self, content_id: &str, url: &str, low: bool, path: &str) {
        self.tries.remove(url);
        if !low {
            self.ready.push_back((content_id.to_string(), path.to_string()));
        }
    }

    /// A download failure: retry (transient) or give up (permanent).
    ///
    /// A connection drop by the server takes down the WHOLE batch of
    /// simultaneous requests: without a retry that is `PREVIEW_MAX_INFLIGHT`
    /// cards with a placeholder forever.
    pub fn failed(&mut self, url: &str, status: Option<u16>) -> PreviewRetry {
        let attempt = self.tries.get(url).copied().unwrap_or(1);
        if !preview_retry_allowed(status, attempt) {
            self.tries.remove(url);
            let fatal = status.is_some_and(|c| PREVIEW_FATAL_HTTP.contains(&c));
            return PreviewRetry::GiveUp { fatal, attempts: attempt };
        }
        self.tries.insert(url.to_string(), attempt + 1);
        PreviewRetry::Again {
            delay_ms: PREVIEW_RETRY_DELAY_MS * attempt,
            attempt,
        }
    }

    /// Return a preview to the queue if the category has not changed since.
    /// `false` means the request is stale or the same picture is already being
    /// requested again.
    pub fn requeue(&mut self, gen: u64, content_id: &str, url: &str, low: bool) -> bool {
        if gen != self.gen || self.wanted.contains(url) {
            return false;
        }
        self.wanted.insert(url.to_string());
        self.push(content_id, url, low);
        true
    }

    /// Cancel everything active and queued (a category change). The in-flight
    /// counters are cleared by the answers themselves: a cancelled request
    /// still comes back to `finished`.
    pub fn cancel_all(&mut self) {
        self.gen += 1;
        self.queue.clear();
        self.ready.clear();
        self.tries.clear();
        self.wanted.clear();
    }

    /// The next ready preview, one per pass of the event loop.
    pub fn take_ready(&mut self) -> Option<(String, String)> {
        self.ready.pop_front()
    }

    /// Whether there is anything to hand out (the Qt layer keeps a timer only while there is).
    pub fn has_ready(&self) -> bool {
        !self.ready.is_empty()
    }
}


// ═══════════════════════════════════════════════════════════════════════
//  Catalog state: what the tab shows and where it goes for data
// ═══════════════════════════════════════════════════════════════════════

/// The category's data source.
pub const SOURCE_OCS: &str = "ocs";
pub const SOURCE_AUR: &str = "aur";
pub const SOURCE_BOTH: &str = "both";
/// Cinnamon Spices: the only store of the five Cinnamon sections
/// (`crate::cinnamon_spices`).
pub const SOURCE_SPICES: &str = crate::cinnamon_spices::SOURCE_KEY;
/// Wallhaven: the second store of "HD wallpapers" next to KDE Store
/// (`crate::wallhaven`).
pub const SOURCE_WALLHAVEN: &str = crate::wallhaven::SOURCE_KEY;
/// extensions.gnome.org: the only store of "GNOME extensions"
/// (`crate::gnome_ego`).
pub const SOURCE_EGO: &str = crate::gnome_ego::SOURCE_KEY;
/// Razer Axon: the Axon library on this computer, a chip of "HD wallpapers"
/// next to KDE Store and Wallhaven (`crate::axon_window`). Present only if
/// Axon was found ([`CatalogView::axon`]).
pub const SOURCE_AXON: &str = crate::axon_window::SOURCE_KEY;

/// The keys of the sources present in section `category_key` (chip order).
/// What is not here does not exist in the section: the choice is
/// remembered when moving between sections, but KDE Store is in effect
/// ([`CatalogView::effective_source`]). Removed sources, "vsthemes"
/// (04.10.2026) and "github" in "Cursors" (25.10.2026), are not included
/// and by the same path silently become KDE Store.
///
/// AUR ("AUR" and "All") only where the AUR has something to show
/// ([`crate::aur::has_category`]): otherwise the chip would lead to a shelf
/// known to be empty.
pub fn source_keys_for(category_key: &str) -> Vec<&'static str> {
    // Cinnamon sections have their own store, and only one: KDE Store and
    // the AUR do not carry spices.
    if crate::cinnamon_spices::type_for_section(category_key).is_some() {
        return vec![SOURCE_SPICES];
    }
    // GNOME extensions come only from the extensions site: KDE Store and the
    // AUR do not carry them, and the shell itself installs them.
    if crate::gnome_ego::has_section(category_key) {
        return vec![SOURCE_EGO];
    }
    let mut keys = vec![SOURCE_OCS];
    if crate::aur::has_category(category_key) {
        keys.extend([SOURCE_AUR, SOURCE_BOTH]);
    }
    // Spices themes carry both an application theme and window frames: in
    // these sections they are one more store, selected by theme composition
    // (`cinnamon_spices::PART_SECTIONS`).
    if crate::cinnamon_spices::is_part_section(category_key) {
        keys.push(SOURCE_SPICES);
    }
    if crate::wallhaven::has_section(category_key) {
        keys.push(SOURCE_WALLHAVEN);
    }
    keys
}

/// [`source_keys_for`] together with the "Razer Axon" source: it is the
/// last chip in the `axon_window::SECTIONS` sections, and only if Axon was
/// found (`axon` is `axon_window::show_source`, set by the window). Without
/// Axon there is no such source at all: neither a chip nor an active
/// choice.
pub fn source_keys_in(category_key: &str, axon: bool) -> Vec<&'static str> {
    let mut keys = source_keys_for(category_key);
    if axon && crate::axon_window::has_section(category_key) {
        keys.push(SOURCE_AXON);
    }
    keys
}

/// The "Lock screen" category mode: full themes or lock screen wallpapers.
pub const LOCK_MODE_THEMES: &str = "themes";
pub const LOCK_MODE_WALLPAPER: &str = "wallpaper";

/// The synthetic category "lock screen theme" (apply_kind=lockscreen).
pub const LOCK_CAT_THEMES: i64 = 903;
/// The synthetic category "lock screen wallpaper" (apply_kind=lockwallpaper).
pub const LOCK_CAT_WALLPAPER: i64 = 904;

/// The OCS catalog of full lock themes (Global Themes).
pub const LOCK_STORE_THEMES: i64 = 722;
/// The OCS wallpaper catalog (the lock screen background).
pub const LOCK_STORE_WALLPAPER: i64 = 299;

/// The minimum query length for an AUR search.
pub const AUR_MIN_QUERY: usize = 2;

/// The catalog state: everything from which the tab derives its view and
/// its requests.
///
/// There are no widgets here: the Qt layer does not keep the same fields
/// itself but asks this type for them. This way "what to show" and "where
/// to go for data" stop being two different truths.
#[derive(Debug, Clone)]
pub struct CatalogView {
    /// The open sidebar section.
    pub category: ThemeCategory,
    /// The catalog page, from zero.
    pub page: i64,
    /// How many works the category has in total (per the store's answer).
    pub total: i64,
    /// The sort key chosen by the person.
    pub sort: String,
    /// The search line.
    pub query: String,
    /// Source: `ocs` | `aur` | `both`.
    pub source: String,
    /// The desktop the catalog is filtered for.
    pub de: String,
    /// The "Lock screen" mode.
    pub lock_mode: String,
    /// The "Application style" grid filter: [`STYLE_FILTER_ALL`],
    /// [`STYLE_FILTER_QT`] or [`STYLE_FILTER_KVANTUM`]; `None` means all.
    pub style_sub: Option<String>,
    /// The tone filter of colour schemes and Plasma styles as the person chose
    /// it (`local_preview::tone_filter` decides which one is in effect); empty
    /// means "All".
    pub tone_filter: String,
    /// The "Orientation" filter of a store that knows the picture size
    /// (Wallhaven): a [`WallpaperOrientation`] key, empty means "All".
    pub orientation_filter: String,
    /// The "Running" chip of the "Conky widgets" section.
    pub running_only: bool,
    /// We run on SteamOS (Steam Deck or Steam Machine): the login screen and
    /// the boot splash are not changed there, and their sections show the
    /// reason instead of a shelf (`categories::steamos_block`).
    pub steamos: bool,
    /// The unit of the active login screen (`plasmalogin`, `sddm`, `lightdm`…),
    /// for the explanation "it is currently shown by …". Empty means the probe
    /// did not answer.
    pub login_manager: String,
    /// The Conky program is in the system (for the "Conky widgets" section label).
    pub conky_installed: bool,
    /// Rexon can install the Conky package on this system by itself.
    pub conky_installable: bool,
    /// A Wayland session: Conky on Wayland has its own caveat.
    pub wayland: bool,
    /// A desktop with its own Qt integration (Plasma, LXQt, Deepin) is
    /// installed: the Qt style caveat on sway/river/Wayfire names Rexon's
    /// desktop-bound variables file (`categories::category_condition_on`).
    /// Set by the window; no by default.
    pub qt_desktop_installed: bool,
    /// The catalog section's "⚙️ Settings" view is open (system fonts in
    /// "Fonts", icons and toolbars in "Icons", launch feedback in "Cursors",
    /// program animation in "GTK themes": `categories::SECTION_SETTINGS`)
    /// instead of the tiles. It belongs to the section: it is reset when
    /// leaving it, and the person returns to the section's catalog.
    pub section_settings: bool,
    /// The session kind of the chosen desktop: sections impossible in it (MATE
    /// screen effects on Wayland) are not visible
    /// (`categories::category_available_in`). Set by the window; "any" by
    /// default.
    pub session: categories::Session,
    /// The "Only for your GNOME version" toggle of the extensions.gnome.org
    /// showcase: on means the site returns only compatible ones, off means all,
    /// with incompatible ones marked. On by default.
    pub ego_compatible_only: bool,
    /// Whether Razer Axon was found (`axon_window::show_source`): without it
    /// there is no "Razer Axon" source and no block of its settings. Set by the
    /// window; no by default.
    pub axon: bool,
}

impl CatalogView {
    /// The initial state: the section, the "New" sort, the KDE store.
    ///
    /// The default sort is "New": by rating the same works have sat at the top
    /// for years, and the showcase looks dead on every visit. The default lock
    /// mode is "Wallpapers": there are thousands of them, and the choice is
    /// instant.
    pub fn new(category: ThemeCategory, de: &str) -> Self {
        Self {
            category,
            page: 0,
            total: 0,
            sort: DEFAULT_SORT.to_string(),
            query: String::new(),
            source: SOURCE_OCS.to_string(),
            de: de.to_string(),
            lock_mode: LOCK_MODE_WALLPAPER.to_string(),
            style_sub: None,
            tone_filter: String::new(),
            orientation_filter: String::new(),
            running_only: false,
            steamos: false,
            login_manager: String::new(),
            conky_installed: true,
            conky_installable: false,
            wayland: false,
            qt_desktop_installed: false,
            section_settings: false,
            session: categories::Session::Any,
            ego_compatible_only: true,
            axon: false,
        }
    }

    /// The "Lock screen" mode adjusted for the environment: where there are no
    /// full themes (Deepin), the wallpaper mode stays even if the themes mode
    /// was chosen earlier under KDE.
    pub fn lock_mode_effective(&self) -> &str {
        if self.lock_mode == LOCK_MODE_THEMES && !categories::lock_themes_supported(&self.de) {
            LOCK_MODE_WALLPAPER
        } else {
            &self.lock_mode
        }
    }

    /// The section is closed on this SteamOS (login screen, boot splash).
    pub fn steamos_blocked(&self) -> bool {
        self.steamos && categories::steamos_block(&self.category.key).is_some()
    }

    /// The login screen section under Plasma Login Manager has its own panel
    /// (an explanation and a button to the settings), not a store shelf.
    pub fn is_plasma_login(&self) -> bool {
        self.category.key == categories::PLASMA_LOGIN_KEY
    }

    /// The login screen section (a shelf of SDDM themes or the Plasma Login
    /// Manager panel) has the "Login manager" block above it. On SteamOS the
    /// section is closed entirely, and there is no block.
    pub fn is_login_screen(&self) -> bool {
        matches!(self.category.key.as_str(), "sddm" | categories::PLASMA_LOGIN_KEY) && !self.steamos_blocked()
    }

    /// The "MATE panel" section is its own screen with layouts, not a shelf.
    pub fn is_mate_panel(&self) -> bool {
        self.category.key == categories::MATE_PANEL_KEY
    }

    /// A settings section (animation, fonts, title bar buttons…) is its own
    /// screen with a section of the "Settings" tab, not a shelf
    /// (`categories::SETTINGS_PANELS`).
    pub fn is_settings_panel(&self) -> bool {
        categories::is_settings_panel(&self.category.key) || self.section_settings_open()
    }

    /// The catalog section's "⚙️ Settings" view: a button in the header of
    /// "Fonts", "Icons", "Cursors", window frames and "GTK themes" (08.11.2026:
    /// the catalog and the settings of one kind in one section rather than two
    /// sidebar rows): which settings sections it holds. Empty means no button
    /// (the section has none, there is nothing to configure in this desktop, or
    /// the section is closed).
    pub fn section_settings_panels(&self) -> Vec<&'static str> {
        if self.de_blocked() || self.steamos_blocked() {
            return Vec::new();
        }
        let mut panels = categories::section_settings_panels(&self.category.key, &self.de);
        // The "Razer Axon" block only where the source itself exists.
        if !self.axon {
            panels.retain(|p| *p != categories::AXON_SETTINGS_PANEL);
        }
        panels
    }

    /// Whether the section has a "⚙️ Settings" view.
    pub fn has_section_settings(&self) -> bool {
        !self.section_settings_panels().is_empty()
    }

    /// Whether the section's settings view is open now.
    pub fn section_settings_open(&self) -> bool {
        self.section_settings && self.has_section_settings()
    }

    /// Which sections of the "Settings" tab to show on the [`SCREEN_SETTINGS`]
    /// screen (comma-separated, `SettingsPage.panel`): settings sections have
    /// their own key, a catalog section in settings view has its sections from
    /// `categories::SECTION_SETTINGS`.
    pub fn settings_panel_key(&self) -> String {
        if self.section_settings_open() {
            self.section_settings_panels().join(",")
        } else {
            self.category.key.clone()
        }
    }

    /// An explanation instead of a shelf: the section does not exist in this
    /// desktop or is closed on SteamOS. `None` means the section is open.
    pub fn note(&self, home: &Path) -> Option<String> {
        categories::category_note(home, &self.category.key, &self.de)
            .or_else(|| categories::steamos_note(home, &self.category.key, self.steamos))
    }

    /// Which screen to show after rebuilding the section: `note` is an
    /// explanation, `plasmalogin` the Plasma Login Manager panel, `grid` the
    /// showcase. An item's details are not included: the person opens them.
    pub fn screen(&self, home: &Path) -> &'static str {
        if self.note(home).is_some() {
            SCREEN_NOTE
        } else if self.is_plasma_login() {
            SCREEN_PLASMA_LOGIN
        } else if self.is_mate_panel() {
            SCREEN_MATE_PANEL
        } else if self.is_settings_panel() {
            SCREEN_SETTINGS
        } else if self.axon_mode() {
            SCREEN_AXON
        } else {
            SCREEN_GRID
        }
    }

    /// The section is a local disk overview, a category without a network
    /// store, or a section with an installed grid while the store is not open.
    pub fn is_local(&self) -> bool {
        categories::LOCAL_VIEW_KEYS.contains(&self.category.key.as_str())
            || self.category.local_only()
    }

    /// Whether the section has an installed grid with the store behind a "Get
    /// new…" button, like the KDE "Appearance" kcm pages: any section that has
    /// both a disk scan and a store ([`has_installed_grid_for`]), plus full
    /// lock screen themes.
    ///
    /// Previously the splash and the lock screen glued the installed items on
    /// top of the store's first page. A grid with a button is one path for all
    /// ten: there can be more installed items than a page (there are dozens of
    /// icon themes in the system), and the store below would slide off the
    /// edge, while an "Installed" heading cannot stand between two parts of one
    /// grid.
    pub fn has_installed_view(&self) -> bool {
        let key = self.category.key.as_str();
        has_installed_grid_for(&self.category)
            || (key == "lockscreen" && self.lock_mode_effective() == LOCK_MODE_THEMES)
    }

    /// Whether the section shows installed items: on top, above the section's
    /// store on the same page. There is no "Get new…" button any more (the
    /// owner's decision of 18.10.2026): the store loads by itself.
    pub fn installed_view(&self) -> bool {
        self.has_installed_view()
    }

    /// Whether the "All / Light / Dark" tone filter line is visible
    /// (`local_preview::tone_bar_applies`).
    pub fn tone_bar(&self) -> bool {
        crate::local_preview::tone_bar_applies(&self.category.key, self.installed_view())
    }

    /// The section's tone filter in effect (`local_preview::tone_filter`).
    pub fn active_tone_filter(&self) -> &'static str {
        crate::local_preview::tone_filter(&self.category.key, &self.tone_filter)
    }

    /// Whether the "Orientation: All / Horizontal / Vertical" line is
    /// visible: in a store whose cards carry the picture size and whose
    /// server filters by it (Wallhaven). The Razer Axon source has the same
    /// group in its own filter panel (`axon_window::Filters`); KDE Store
    /// cards carry no size, so it has no such line.
    pub fn orientation_bar(&self) -> bool {
        self.wallhaven_mode()
    }

    /// The orientation filter in effect; `None` — "All" or no such line.
    pub fn active_orientation(&self) -> Option<WallpaperOrientation> {
        if self.orientation_bar() {
            WallpaperOrientation::from_key(&self.orientation_filter)
        } else {
            None
        }
    }

    /// Whether the installed grid is narrowed by a filter that has no store
    /// showcase underneath: only what is on disk is shown.
    ///
    /// ONE rule for all such section filters: "Light / Dark" for colours and
    /// Plasma styles (the tone is known only for themes on disk), "Qt styles /
    /// Kvantum" for the application style (a filter of the installed grid,
    /// while the section's store is Kvantum themes without distinction) and
    /// "Running" for Conky (only installed items have a process). Under such a
    /// filter the store is not queried (`request_key`), there is no pager, sort
    /// or "Store:" (`chrome`), and search goes among the installed items
    /// (`search_placeholder`). A new filter of the installed grid is added
    /// HERE, and it gets everything else by itself. Until 01.11.2026 the rule
    /// lived only in the tone filter, and under "Kvantum" "Found 337" and a
    /// "1 / 17" pager remained.
    pub fn installed_only(&self) -> bool {
        if !self.installed_view() {
            return false;
        }
        let tone = self.tone_bar() && self.active_tone_filter() != crate::local_preview::TONE_ALL;
        let style = self.category.key == "widgetstyle" && self.style_filter() != STYLE_FILTER_ALL;
        let running = self.category.key == "conky" && self.running_only;
        tone || style || running
    }

    /// Whether to highlight the current theme with a frame and a bold label
    /// (as KDE does): in the installed grid and in environment sections. In
    /// the "Active" overview and others many are enabled at once, so a frame
    /// would distinguish nothing there.
    pub fn marks_current(&self) -> bool {
        self.installed_view()
            || (self.is_local() && local_scan_plan(&self.local_scan_which()).0 == "section")
    }

    /// Whether the "Store:" switch applies to the current category.
    ///
    /// Local disk overviews do not know the network at all. The switch exists
    /// where the catalog comes from OCS and there is something else to choose
    /// from besides it ([`source_keys_for`]): the splash (umbrella 716+488),
    /// the lock screen, the window switcher, Conky and environment frames have
    /// no packages in the AUR, and an "AUR" chip would lead to a shelf known
    /// to be empty.
    pub fn source_applicable(&self) -> bool {
        let ocs = matches!(self.category.store, CatalogSource::Ocs(_) | CatalogSource::OcsShowcase);
        !self.is_local() && ocs && self.source_keys().len() > 1
    }

    /// The section's sources, adjusted for whether Axon was found
    /// ([`source_keys_in`]).
    pub fn source_keys(&self) -> Vec<&'static str> {
        source_keys_in(&self.category.key, self.axon)
    }

    /// Whether the pure AUR source is active.
    pub fn aur_mode(&self) -> bool {
        self.source_applicable() && self.source == SOURCE_AUR
    }

    /// Whether the mixed "All" source is active.
    pub fn both_mode(&self) -> bool {
        self.source_applicable() && self.source == SOURCE_BOTH
    }

    /// The source in effect in this section. A choice the section does not
    /// have (or that no longer exists at all, like the removed "vsthemes" and
    /// "github")
    /// is remembered, but the section's first source is in effect (KDE Store,
    /// for Cinnamon sections Cinnamon Spices).
    pub fn effective_source(&self) -> &str {
        let keys = self.source_keys();
        if keys.contains(&self.source.as_str()) {
            &self.source
        } else {
            keys.first().copied().unwrap_or(SOURCE_OCS)
        }
    }

    /// The section's store is Cinnamon Spices: the site's catalog kind
    /// (`applets`, `themes`…). `None` means the section has another store or
    /// none on the network.
    pub fn spices_json(&self) -> Option<&str> {
        match &self.category.store {
            CatalogSource::Spices(json) if !self.is_local() => Some(json.as_str()),
            // "GTK themes" and window frames: the Cinnamon Spices chip is chosen,
            // so the showcase is the site's themes selected by composition.
            _ if self.spices_parts_mode() => crate::cinnamon_spices::type_for_key("theme").map(|t| t.json),
            _ => None,
        }
    }

    /// Whether Cinnamon Spices themes are open in the GTK or window frames
    /// section (`cinnamon_spices::PART_SECTIONS`): there it is not the only
    /// store but a chip next to KDE Store.
    pub fn spices_parts_mode(&self) -> bool {
        self.source_applicable()
            && self.source == SOURCE_SPICES
            && crate::cinnamon_spices::is_part_section(&self.category.key)
    }

    /// Whether the Cinnamon Spices store is open.
    pub fn spices_mode(&self) -> bool {
        self.spices_json().is_some()
    }

    /// Whether the Wallhaven store is open ("HD wallpapers", a chip next to KDE Store).
    pub fn wallhaven_mode(&self) -> bool {
        self.source_applicable()
            && self.source == SOURCE_WALLHAVEN
            && crate::wallhaven::has_section(&self.category.key)
    }

    /// Whether the "Razer Axon" source is open ("HD wallpapers", a chip next to
    /// KDE Store): instead of the store grid, its own [`SCREEN_AXON`] screen
    /// with the Axon library.
    pub fn axon_mode(&self) -> bool {
        self.axon
            && self.source_applicable()
            && self.source == SOURCE_AXON
            && crate::axon_window::has_section(&self.category.key)
    }

    /// Whether the extensions.gnome.org store is open ("GNOME extensions").
    pub fn ego_mode(&self) -> bool {
        !self.is_local() && self.category.store == CatalogSource::GnomeExtensions
    }

    /// Whether a store with its own listing is open (Cinnamon Spices,
    /// Wallhaven, extensions.gnome.org): it has no KDE Store suggestions or
    /// background warm-up.
    pub fn own_site_store(&self) -> bool {
        self.spices_mode() || self.wallhaven_mode() || self.ego_mode() || self.axon_mode()
    }

    /// The drop-down list sorts of the open store: `(label, key)`.
    pub fn sorts(&self) -> &'static [(&'static str, &'static str)] {
        if self.wallhaven_mode() {
            crate::wallhaven::SORTS
        } else if self.ego_mode() {
            crate::gnome_ego::SORTS
        } else {
            SORTS
        }
    }

    /// The section does not exist in the chosen desktop.
    pub fn de_blocked(&self) -> bool {
        !categories::category_available_in(&self.category.key, &self.de, self.session)
    }

    /// `cat_id` (or an "id1,id2" umbrella) for the catalog request.
    ///
    /// A string, not a number: OCS accepts several categories separated by
    /// commas, and umbrella categories use this.
    pub fn query_category_id(&self) -> String {
        if self.category.key == "lockscreen" {
            return if self.lock_mode_effective() == LOCK_MODE_THEMES {
                LOCK_STORE_THEMES.to_string()
            } else {
                LOCK_STORE_WALLPAPER.to_string()
            };
        }
        store_query(&self.category, &self.de)
    }

    /// The sort that will actually go into the request for this category.
    ///
    /// For "Popular" and "New" it is fixed; for ordinary ones it is the
    /// person's choice. The background warm-up computes it the same way;
    /// otherwise the cache key would not match and the warmed-up page would
    /// open again over the network.
    pub fn sort_for(&self, cat: &ThemeCategory) -> String {
        match cat.key.as_str() {
            "new" => "new".to_string(),
            "popular" => "high".to_string(),
            _ => self.sort.clone(),
        }
    }

    /// The sort for the open section. Wallhaven has three sorts of its own: a
    /// choice it does not have ("Alphabetical") acts as "New".
    pub fn active_sort(&self) -> String {
        let sort = self.sort_for(&self.category);
        if self.wallhaven_mode() {
            return crate::wallhaven::effective_sort(&sort).to_string();
        }
        if self.ego_mode() {
            return crate::gnome_ego::effective_sort(&sort).to_string();
        }
        sort
    }

    /// The AUR session cache key: a category list by its key, a search by the
    /// section and the query (search results are filtered for the section,
    /// `aur::search_in_category`). The worker's answer carries the same key,
    /// and answers for categories the person has already left are discarded
    /// by it.
    pub fn aur_cache_key(&self) -> String {
        // A shelf that differs by desktop (`aur::native_shelf`) is cached per desktop.
        let key = match crate::aur::native_shelf(&self.category.key, &self.de) {
            Some(_) => format!("{}@{}", self.category.key, self.de),
            None => self.category.key.clone(),
        };
        if self.query.is_empty() {
            format!("cat:{key}")
        } else {
            format!("search:{key}:{}", self.query)
        }
    }

    /// What to show in the tab header in the current state.
    pub fn chrome(&self) -> Chrome {
        if self.de_blocked() || self.steamos_blocked() || self.is_plasma_login() || self.is_mate_panel()
            || self.is_settings_panel()
        {
            // The section does not exist in this desktop (or is closed on SteamOS,
            // or the login screen has no themes at all): the store, the sort and
            // the pages are not needed: there is nothing to show, and offering to
            // "search" here is dishonest. For Plasma Login Manager the
            // explanation "shared by all desktops" stays: in MATE it would
            // otherwise be unclear why Plasma is here.
            let caveat = if self.is_plasma_login() && !self.steamos_blocked() {
                self.caveat()
            } else {
                None
            };
            return Chrome { caveat, ..Chrome::default() };
        }
        let local = self.is_local();
        // Under the "installed only" filter there is no store, and so no switch,
        // sort or pages for it either (`installed_only`).
        let store = !self.installed_only();
        // The "Razer Axon" source has its own sort and filters, as Axon itself
        // does, on its screen; there is nothing to page through (the whole
        // library), and installing from a file does not belong here. But the
        // search line is THE SAME as for the neighbouring stores, in the same
        // place of the header and with the same word: a separate line on the
        // Axon screen drifted to the bottom left, and the typed word was lost
        // when switching stores (live check 16.11.2026, Cinnamon 6.8). Where
        // to search, in names or in tags, is chosen in the "Filter".
        if self.axon_mode() {
            return Chrome {
                search: true,
                search_placeholder: self.search_placeholder(),
                source_bar: store,
                caveat: self.caveat(),
                ..Chrome::default()
            };
        }
        Chrome {
            search: true,
            search_placeholder: self.search_placeholder(),
            caveat: self.caveat(),
            source_bar: self.source_applicable() && store,
            // The "All / Qt styles / Kvantum" filter is above the installed grid;
            // in the section's store (Kvantum themes) there is nothing to filter.
            style_bar: self.category.key == "widgetstyle" && self.installed_view(),
            lock_bar: self.category.key == "lockscreen",
            cursor_size_bar: self.category.key == "cursor",
            sort_combo: !local && !self.aur_mode() && store,
            pagination: !local && store,
            install_file: crate::local_install::button_for(
                &self.category.key,
                self.category.virtual_,
            ),
        }
    }

    /// The search field placeholder. Under the "installed only" filter
    /// ([`Self::installed_only`]) there is no store (neither AUR nor Spices),
    /// the search goes only among installed items, and a placeholder about the
    /// chosen store would be untrue.
    pub fn search_placeholder(&self) -> String {
        if self.installed_only() {
            tr("🔍 Search installed…")
        } else if self.aur_mode() {
            tr("🔍 Search AUR…")
        } else if self.spices_mode() {
            tr("🔍 Search Cinnamon Spices…")
        } else if self.wallhaven_mode() {
            crate::wallhaven::search_placeholder()
        } else if self.ego_mode() {
            crate::gnome_ego::search_placeholder()
        } else if self.axon_mode() {
            crate::axon_window::search_placeholder()
        } else {
            tr("🔍 Search themes…")
        }
    }

    /// The condition label under the section heading: for the login screen and
    /// the boot splash, that they are shared by all desktops
    /// (`categories::system_section_note`), for the rest the matrix condition.
    fn caveat(&self) -> Option<String> {
        let key = self.category.key.as_str();
        let conky = (key == "conky").then(|| {
            crate::de_apply::conky_section_note(
                self.conky_installed,
                self.conky_installable,
                &self.de,
                self.wayland,
            )
        });
        let parts: Vec<String> = [
            categories::system_section_note(key, &self.login_manager),
            categories::category_condition_on(key, &self.de, self.qt_desktop_installed),
            conky.flatten(),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    /// Which disk scan to run for a local category.
    ///
    /// "Application style" carries a grid filter (`widgetstyle:<filter>`): Qt
    /// styles and Kvantum themes together, only Qt styles or only Kvantum.
    pub fn local_scan_which(&self) -> String {
        if self.category.key != "widgetstyle" {
            return self.category.key.clone();
        }
        format!("widgetstyle:{}", self.style_filter())
    }

    /// The "Application style" filter in effect: an unknown value (including
    /// the former sub-choice `qt:<Name>`) means "All".
    pub fn style_filter(&self) -> &'static str {
        match self.style_sub.as_deref() {
            Some(STYLE_FILTER_QT) => STYLE_FILTER_QT,
            Some(STYLE_FILTER_KVANTUM) => STYLE_FILTER_KVANTUM,
            _ => STYLE_FILTER_ALL,
        }
    }

    /// Accept what the person typed in the search line.
    ///
    /// ⚠️ THERE WAS A HOLE HERE (live check 06.09.2026). Typing a non-empty
    /// word did NOT get into the query at all: the page could only RESET the
    /// search when the field became empty, and silently lost the typed word.
    /// The query went to the store and to the disk scan empty, and no section
    /// was filtered: in "Active" all five themes remained for the word "bree".
    /// The original widgets record the typed text on Enter (`_on_search`), and
    /// the whole difference between it and us was this missing write.
    ///
    /// Returns what the tab should do next: it decides nothing more about
    /// search itself, and so there is one rule for ALL sections of the page:
    /// the store, the AUR and the disk overviews fetch data with the same
    /// `request_key` and the same `query`.
    pub fn edit_query(&mut self, text: &str) -> QueryEdit {
        let text = text.trim();
        if text.is_empty() {
            // An emptied field resets the search RIGHT AWAY: otherwise the query
            // stuck on the previous word with a visually empty field (a fix of
            // the original).
            if self.query.is_empty() {
                return QueryEdit::Idle;
            }
            self.query.clear();
            self.page = 0;
            return QueryEdit::Cleared;
        }
        if self.query == text {
            return QueryEdit::Idle;
        }
        // The typed text is remembered, but we do not go for data: we do not
        // query the store per character; search is applied by Enter
        // (`submit_query`).
        //
        // The difference from the original is deliberate: there the typed
        // text lives only in the input field and gets into the query on
        // Enter, so "Refresh" or a sort change with a typed but unsent word
        // reread the section WITHOUT it: the word is on screen, but the
        // showcase does not reflect it. Here the query is always what the
        // person sees in the field.
        self.query = text.to_string();
        QueryEdit::Kept
    }

    /// How soon to apply the typed word while the person is typing.
    ///
    /// A disk overview (and the grid under the "installed only" filter)
    /// searches the already read list without the network, so right away (0).
    /// The store and the "installed + store" section (one request) wait for
    /// the [`SEARCH_DEBOUNCE_MS`] pause: we do not query the store per
    /// character. Enter applies immediately for any kind of section.
    pub fn search_delay_ms(&self) -> u32 {
        if self.is_local() || self.installed_only() {
            0
        } else {
            SEARCH_DEBOUNCE_MS
        }
    }

    /// The request key for the section's contents: the answer is checked
    /// against it, and a foreign one is discarded.
    ///
    /// ⚠️ The query is part of the key for ALL section kinds, not only for the
    /// store. Disk overviews had the key `local:<kind>` without the query, and
    /// two different queries for one section looked the same to the dedup:
    /// the typed word was not sent at all, and the grid accepted the answer of
    /// an UNFILTERED scan still in flight.
    pub fn request_key(&self) -> String {
        // Under the "installed only" filter the store is not queried: the key
        // is its own, so as not to merge with an "installed + store" request
        // in flight for the same section.
        if self.installed_only() {
            return format!("installed:{}:{}", self.local_scan_which(), self.query);
        }
        // Installed items and the store in one request: the key carries both parts.
        if self.installed_view() {
            return format!("combined:{}:{}", self.local_scan_which(), self.store_request_key());
        }
        self.store_request_key()
    }

    /// The key without the installed part: a disk overview or a store page.
    fn store_request_key(&self) -> String {
        if self.is_local() {
            return format!("local:{}:{}", self.local_scan_which(), self.query);
        }
        if self.aur_mode() {
            return format!("aur:{}", self.aur_cache_key());
        }
        if let Some(json) = self.spices_json() {
            // Themes by composition: each section has its own showcase.
            let json = if self.spices_parts_mode() { format!("{json}@{}", self.category.key) } else { json.to_string() };
            return format!("spices:{json}:{}:{}:{}", self.page, self.active_sort(), self.query);
        }
        if self.wallhaven_mode() {
            let orientation = self.active_orientation().map_or("", WallpaperOrientation::key);
            return format!("wallhaven:{}:{}:{orientation}:{}", self.page, self.active_sort(), self.query);
        }
        // The Axon library is read by the source model, not the catalog: the key
        // is its own, so as not to merge with the KDE Store showcase of the same
        // section.
        if self.axon_mode() {
            return "axon".to_string();
        }
        if self.ego_mode() {
            let only = if self.ego_compatible_only { "mine" } else { "all" };
            return format!("ego:{only}:{}:{}:{}", self.page, self.active_sort(), self.query);
        }
        format!(
            "ocs:{}:{}:{}:{}",
            self.query_category_id(),
            self.page,
            self.active_sort(),
            self.query
        )
    }
}

/// What the tab should do after the search line was edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryEdit {
    /// Nothing changed: nothing to touch.
    Idle,
    /// The typed text is accepted; it is too early to reread the section: search is applied by Enter.
    Kept,
    /// The field became empty: the query is reset, reread the section right away.
    Cleared,
}

/// Visibility and labels of the catalog header.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chrome {
    pub search: bool,
    /// The placeholder in the search line (empty when there is no line).
    pub search_placeholder: String,
    /// "Works, but on a condition": a label under the section heading. The
    /// section stays in place: hiding something that works instead of naming
    /// the condition is not our way (rule #13).
    pub caveat: Option<String>,
    pub source_bar: bool,
    pub style_bar: bool,
    pub lock_bar: bool,
    /// Cursor size chips, in the "Cursors" section.
    pub cursor_size_bar: bool,
    pub sort_combo: bool,
    pub pagination: bool,
    /// The "Install from file…" button (`crate::local_install::button_for`).
    pub install_file: bool,
}

// ── Section screens ────────────────────────────────────────────────────

/// The tile showcase.
pub const SCREEN_GRID: &str = "grid";
/// An explanation of why the section is not here.
pub const SCREEN_NOTE: &str = "note";
/// The login screen panel under Plasma Login Manager.
pub const SCREEN_PLASMA_LOGIN: &str = "plasmalogin";
/// MATE panel layouts (`categories::MATE_PANEL_KEY`).
pub const SCREEN_MATE_PANEL: &str = "matepanel";
/// A settings section (`categories::SETTINGS_PANELS`): the same section as
/// in the "Settings" tab; the section key is the category key.
pub const SCREEN_SETTINGS: &str = "settings";
/// The Razer Axon library, the "Razer Axon" source of the wallpaper section
/// ([`CatalogView::axon_mode`]): its own screen; the catalog grid is not
/// queried.
pub const SCREEN_AXON: &str = "axon";

/// The "System Settings" module that configures Plasma Login Manager
/// (`/usr/share/applications/kcm_plasmalogin.desktop`,
/// `Exec=systemsettings kcm_plasmalogin`).
pub const PLASMA_LOGIN_KCM: &str = "kcm_plasmalogin";

/// What to open the Plasma Login Manager login screen settings with, in
/// order of attempts. First "System Settings" itself on the right page
/// (that is how the menu item opens it), the fallback is the same module
/// in a separate window (`kcmshell6`). `have(name)` tells whether such a
/// program exists. Empty means there is nothing to open it with: do not
/// show the button, and direct the person in words ([`plasma_login_panel`]
/// with `can_open = false`).
///
/// The module cannot be opened directly on the "Appearance" subpage: it
/// opens it itself from inside, and there is no argument for it.
pub fn login_settings_commands(have: &dyn Fn(&str) -> bool) -> Vec<Vec<String>> {
    ["systemsettings", "kcmshell6"]
        .iter()
        .filter(|bin| have(bin))
        .map(|bin| vec![bin.to_string(), PLASMA_LOGIN_KCM.to_string()])
        .collect()
}

/// Labels of the login screen panel under Plasma Login Manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlasmaLoginPanel {
    /// The panel heading.
    pub title: String,
    /// What this is and what can be changed here, in plain language.
    pub body: String,
    /// The button to the login screen settings.
    pub open_label: String,
    /// A hint for those who came for themes from the store.
    pub store_hint: String,
    /// The status line if opening the settings did not work.
    pub open_failed: String,
}

/// Labels of the Plasma Login Manager panel. `can_open` means there is
/// something to open the settings with ([`login_settings_commands`] is not
/// empty); if there is nothing, there is no button, and the text itself
/// says where to go.
pub fn plasma_login_panel(can_open: bool) -> PlasmaLoginPanel {
    let mut body = tr(
        "It comes with KDE Plasma 6.6 and later and doesn't support third-party themes: \
         the login screen always looks like Breeze. You can change its background and \
         clock and bring your desktop's colors, cursor and fonts over to it — that's done \
         in System Settings.",
    );
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    let where_to_go = tr("Open “System Settings → Login Screen”.");
    if !can_open {
        body = format!("{body}\n{where_to_go}");
    }
    PlasmaLoginPanel {
        title: tr("The login screen is drawn by Plasma Login Manager"),
        body,
        open_label: tr("Open Login Screen Settings"),
        store_hint: tr("Looking for login screen themes from the store? They only work with SDDM."),
        open_failed: format!(
            "{} {where_to_go}",
            tr("Couldn't open the login screen settings.")
        ),
    }
}

// ── Event sounds are turned off ────────────────────────────────────────

/// The bar above the "Sounds" section when system sounds are turned off in
/// the environment (in MATE `org.mate.sound event-sounds`, in the others
/// the key from `qt_settings::sound_backend`). In this state any sound
/// theme is mute: it can be applied but not heard, and the person will
/// decide the theme is broken (rule #13: name the condition in advance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventSoundsBanner {
    pub visible: bool,
    pub text: String,
    pub button: String,
}

/// The bar's decision: section `cat_key` and the raw answer of reading the
/// switch (`qt_settings::sound_read_argv_for(.., EventSounds, ..)`).
///
/// An empty answer means reading failed (no schema, no utility): then we
/// stay silent rather than scare with "turned off" where we simply do not
/// know.
pub fn event_sounds_banner(cat_key: &str, raw: &str) -> EventSoundsBanner {
    let off = cat_key == "sounds"
        && !raw.trim().is_empty()
        && !crate::qt_settings::parse_event_sounds(raw);
    EventSoundsBanner {
        visible: off,
        text: tr(
            "System sounds are turned off, so no sound theme will be heard. \
             Turn them on to hear the theme.",
        ),
        button: tr("Turn on"),
    }
}

// ── "Restore as it was" after "Apply" ──────────────────────────────────

/// The section's "Restore as it was" bar: a successful "Apply" left a
/// snapshot of what was there before it (`install_journal::Applied`), and
/// the person can restore it with one button.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AppliedRevertBanner {
    pub visible: bool,
    /// The section that will be restored (the category key).
    pub section: String,
    /// What will be restored, in words.
    pub text: String,
    /// The button label.
    pub button: String,
}

/// When "Apply" happened, briefly, for the label: `25.10, 14:02`. The log
/// timestamp is local time in ISO (`2026-10-25T14:02:33`); if it does not
/// parse, `None`, and the label does without the time.
pub fn applied_when(applied_at: Option<&str>) -> Option<String> {
    let s = applied_at?;
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r.clone()).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    if !(digits(0..4) && b.get(4) == Some(&b'-') && digits(5..7) && digits(8..10) && digits(11..13) && digits(14..16)) {
        return None;
    }
    Some(format!("{}.{}, {}", &s[8..10], &s[5..7], &s[11..16]))
}

/// The bar's decision for section `cat_key`.
///
/// * `applied`: this section's "Apply" record in the common log;
/// * `layer_memo`: colours outside Plasma have their own layer note
///   (`shell_palette::read_layer_memo`) with edits that can be restored;
/// * `shell_revert_shown`: the "Shell palette" bar shows its own "Restore
///   as it was" button (`PaletteBanner::can_revert` with a live shell).
///
/// There is one button per section. For colours, while the shell bar's
/// button is visible, there is none of its own: that one restores both the
/// shell files and the previous scheme.
pub fn applied_revert_banner(
    cat_key: &str,
    applied: Option<&crate::install_journal::Applied>,
    layer_memo: bool,
    shell_revert_shown: bool,
) -> AppliedRevertBanner {
    let colors = cat_key == "colors";
    let applied = applied.filter(|a| !a.files.is_empty());
    let hidden = AppliedRevertBanner { section: cat_key.to_string(), ..Default::default() };
    if colors && shell_revert_shown {
        return hidden;
    }
    let (text, button) = match applied {
        Some(a) => match applied_when(a.applied_at.as_deref()) {
            Some(when) => (
                trf(
                    // TRANSLATORS: %1 is a theme name, %2 is a short date and time (25.10, 14:02).
                    "Before applying “%1” (%2), Rexon saved what was there. You can return it.",
                    &[&a.theme, &when],
                ),
                trf(
                    // TRANSLATORS: button; %1 is a theme name, %2 is a short date and time (25.10, 14:02).
                    "Return as It Was Before “%1” (%2)",
                    &[&a.theme, &when],
                ),
            ),
            None => (
                trf(
                    // TRANSLATORS: %1 is a theme name.
                    "Before applying “%1”, Rexon saved what was there. You can return it.",
                    &[&a.theme],
                ),
                trf(
                    // TRANSLATORS: button; %1 is a theme name.
                    "Return as It Was Before “%1”",
                    &[&a.theme],
                ),
            ),
        },
        None if colors && layer_memo => (
            tr("Rexon changed the colors of applications outside Plasma. You can return the colors that were there before."),
            // TRANSLATORS: button; returns the application colors that were there before Rexon.
            tr("Return Colors as They Were"),
        ),
        None => return hidden,
    };
    AppliedRevertBanner { visible: true, section: cat_key.to_string(), text, button }
}

/// The result of "Restore as it was" by the "Apply" record
/// (`revert_applied_at`): `(success, text for the person)`.
pub fn applied_revert_outcome(theme: &str, result: &std::io::Result<Option<Vec<String>>>, home: &str) -> (bool, String) {
    match result {
        Ok(None) => (false, tr("There is nothing to return: Rexon has no saved copy for this section.")),
        Ok(Some(failed)) if failed.is_empty() => (
            true,
            trf(
                // TRANSLATORS: %1 is a theme name.
                "Returned what was there before “%1”. Programs that are already open may show it only after a restart or the next login.",
                &[&theme],
            ),
        ),
        Ok(Some(failed)) => {
            let short = |p: &String| match p.strip_prefix(home).filter(|r| !home.is_empty() && r.starts_with('/')) {
                Some(rest) => format!("~{rest}"),
                None => p.clone(),
            };
            // Paths are files; the rest are desktop settings keys (gsettings,
            // dconf, xfconf): they get their own line and must not be called
            // files.
            let (values, files): (Vec<&String>, Vec<&String>) =
                failed.iter().partition(|f| crate::install_journal::is_value_key(f));
            let mut lines = Vec::new();
            if !files.is_empty() {
                let list: Vec<String> = files.iter().map(|p| short(p)).collect();
                lines.push(fill(
                    &crate::i18n::ntr(
                        // TRANSLATORS: %n is the number of files, %1 is their list. The copies stay saved, so the button keeps working.
                        "Not everything returned: %n file could not be put back (%1). Its copy is kept — try again.",
                        "Not everything returned: %n files could not be put back (%1). Their copies are kept — try again.",
                        files.len() as u64,
                    ),
                    &[&list.join(", ")],
                ));
            }
            if !values.is_empty() {
                let list: Vec<&str> = values.iter().map(|k| crate::install_journal::value_key_label(k)).collect();
                lines.push(fill(
                    &crate::i18n::ntr(
                        // TRANSLATORS: %n is the number of desktop settings (theme, icons… kept by the desktop itself, not in files), %1 is their list. The old values stay saved, so the button keeps working.
                        "Not everything returned: %n desktop setting could not be put back (%1). Its old value is kept — try again.",
                        "Not everything returned: %n desktop settings could not be put back (%1). Their old values are kept — try again.",
                        values.len() as u64,
                    ),
                    &[&list.join(", ")],
                ));
            }
            (false, lines.join("\n"))
        }
        Err(e) => (false, trf("Could not return: %1", &[&e.to_string()])),
    }
}

/// "Restore as it was" for section `section`: the one button of the
/// [`applied_revert_banner`] bar.
///
/// For colours with a layer note the note works first
/// (`shell_palette::revert_scheme_layer`: by keys, keeping edits made after
/// Rexon), then the "Apply" record, only for paths the note does not touch
/// (and not for the note itself: otherwise its previous copy would come
/// back). Other sections use the whole "Apply" record.
pub fn revert_applied_section(
    rexon_dir: &Path,
    palette: &crate::shell_palette::PaletteHome,
    section: &str,
) -> (bool, String) {
    use crate::install_journal as journal;
    use crate::shell_palette as sp;
    let rec = journal::view_at(rexon_dir).applied.get(section).cloned();
    let theme = rec.as_ref().map(|r| r.theme.clone()).unwrap_or_default();
    let memo = sp::read_layer_memo(palette);
    if section == "colors" && memo.has_changes() {
        let memo_file = sp::layer_memo_path(palette).to_string_lossy().into_owned();
        let mine: BTreeSet<String> = memo.files.iter().map(|f| f.path.clone()).chain([memo_file]).collect();
        let msg = match sp::revert_scheme_layer(palette) {
            Ok(m) => m,
            Err(e) => return (false, trf("Could not return: %1", &[&e])),
        };
        if rec.is_none() {
            return (true, msg);
        }
        let rest = journal::revert_applied_except_at(rexon_dir, section, &|p| mine.contains(p));
        return match rest {
            Ok(Some(failed)) if failed.is_empty() => (true, msg),
            other => applied_revert_outcome(&theme, &other, &palette.home),
        };
    }
    applied_revert_outcome(&theme, &journal::revert_applied_at(rexon_dir, section), &palette.home)
}

// ── Pages ──────────────────────────────────────────────────────────────

/// How many pages the category has in total.
///
/// The store pages by exactly the requested size (`page × page_size`), and
/// works it hides simply drop out of their page: a page may have 18 cards
/// out of twenty, but the next one still starts with the 21st. So the
/// number of pages is counted by the REQUESTED size. The former count by
/// the received page (318 / 18 = 18 instead of 16) showed empty pages at
/// the end: checked on 03.10.2026 on Global Themes: 18, 20 × 14, 18, 0.
pub fn total_pages(total: i64, page_size: u32) -> i64 {
    if total <= 0 {
        return 1;
    }
    let per = i64::from(page_size.max(1));
    (1).max((total + per - 1) / per)
}

/// The state of the "Page N / M" block.
#[derive(Debug, Clone, PartialEq)]
pub struct Pagination {
    pub label: String,
    pub prev_enabled: bool,
    pub next_enabled: bool,
}

/// The pages block for the current catalog answer.
pub fn pagination(page: i64, total: i64, shown: usize, page_size: u32) -> Pagination {
    let pages = total_pages(total, page_size);
    Pagination {
        label: trf("Page %1 / %2", &[&(page + 1), &pages]),
        prev_enabled: page > 0,
        next_enabled: shown > 0 && (page + 1) < pages,
    }
}

/// Whether there is a next page worth warming up in advance.
///
/// ⚠️ A fix of the original (rule #16). The warm-up counted pages by the
/// REQUESTED size (twenty), and the "Forward" button by the actually
/// received one (about ten). Because of this, in the second half of the
/// catalog the button paged on, while the warm-up thought there were no
/// pages left, and that page was opened over the network every time. The
/// count is now shared by both.
pub fn has_page_to_prefetch(page: i64, total: i64, page_size: u32) -> bool {
    total > 0 && (page + 1) < total_pages(total, page_size)
}

/// A Rexon page assembled from the source's stream.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPage<T> {
    /// Exactly `page_size` works in a row (fewer only at the very end of the stream).
    pub items: Vec<T>,
    /// How many works the source named in total (`totalitems`), including the
    /// ones it later hides, so there may be fewer visible ones.
    pub total: i64,
    /// There is at least one work after the page: "Forward" lights up by the
    /// fact, not by `total`.
    pub has_more: bool,
}

/// Filling up a page: Rexon page number `page` is the stream works
/// `[page × size, page × size + size)`, where the stream is server pages in
/// a row of `server_size` works each. The sizes differ: a Rexon page is
/// whole grid rows ([`grid_page_size`]), a server page is always
/// [`PAGE_SIZE`].
///
/// This is how KDE Store (OCS) behaves: it pages strictly by the requested
/// twenty, and the works it hides drop out of their page, leaving a hole on
/// screen (Global Themes 03.10.2026: 18, 20 × 14, 18). Here the shortfall
/// is filled by works from the next server page, and the next Rexon page
/// starts exactly where this one ended, without repeats or gaps.
///
/// `fetch(k)` is server page `k` (from zero, `server_size` works each):
/// the works and `totalitems`.
/// If the source did not answer, `(empty, 0)`. Server pages are fetched
/// from zero in order, because where a Rexon page starts in the stream is
/// known only by adding up the lengths of all previous ones; when paging
/// in sequence they are already in the cache (`ocs_cache`), so the network
/// sees one or two new pages per step. One work more than a page is
/// fetched, to know honestly whether there is a next one (`has_more`). The
/// end of the stream is by the source's own count
/// (`ceil(total / server_size)` server pages), not by a short page: short
/// pages also occur in the middle. If the source went silent in the middle
/// of the stream, we judge the next page by `total`, as before, rather
/// than turn off "Forward".
pub fn stream_page<T>(
    page: i64,
    page_size: u32,
    server_size: u32,
    mut fetch: impl FnMut(i64) -> (Vec<T>, i64),
) -> StreamPage<T> {
    let size = page_size.max(1) as usize;
    let server_size = i64::from(server_size.max(1));
    let start = (page.max(0) as usize).saturating_mul(size);
    let want = start.saturating_add(size + 1);
    let mut items: Vec<T> = Vec::with_capacity(size + 1);
    let mut seen = 0usize;
    let mut total = 0i64;
    let mut broken = false;
    let mut server = 0i64;
    loop {
        let (chunk, reported) = fetch(server);
        if reported <= 0 && chunk.is_empty() {
            // An empty zero page means an empty stream; later ones mean a source failure.
            broken = server > 0;
            break;
        }
        if reported > 0 {
            total = reported;
        }
        for it in chunk {
            if seen >= start && seen < want {
                items.push(it);
            }
            seen += 1;
        }
        server += 1;
        if seen >= want || server.saturating_mul(server_size) >= total {
            break;
        }
    }
    let has_more = if broken {
        (page + 1) < total_pages(total, page_size)
    } else {
        items.len() > size
    };
    items.truncate(size);
    StreamPage { items, total, has_more }
}

/// The pages block for a filled-up stream ([`stream_page`]): "Forward" by
/// the fact (`has_more`), the number of pages by `total` while the stream
/// has not ended, and by the fact once it has (hidden works at the end may
/// add up to a whole page, and we do not promise an empty last one).
pub fn stream_pagination(page: i64, total: i64, has_more: bool, page_size: u32) -> Pagination {
    let pages = if has_more {
        total_pages(total, page_size).max(page + 2)
    } else {
        page + 1
    };
    Pagination {
        label: trf("Page %1 / %2", &[&(page + 1), &pages]),
        prev_enabled: page > 0,
        next_enabled: has_more,
    }
}

/// A slice of the AUR list for the current page: the AUR has no server
/// pages, RPC returns everything at once, and pages are cut on the spot.
pub fn aur_page_range(len: usize, page: i64) -> std::ops::Range<usize> {
    let per = PAGE_SIZE as usize;
    let start = (page.max(0) as usize).saturating_mul(per).min(len);
    let end = start.saturating_add(per).min(len);
    start..end
}

/// The page a fresh AUR list lands on: never beyond the end.
pub fn aur_clamp_page(len: usize, page: i64) -> i64 {
    let last = if len == 0 { 0 } else { (len as i64 - 1) / PAGE_SIZE as i64 };
    page.min(last.max(0)).max(0)
}

/// Whether the AUR list can be paged by `delta` pages.
pub fn aur_can_change_page(len: usize, page: i64, delta: i64) -> bool {
    let new = page + delta;
    if new < 0 {
        return false;
    }
    if delta > 0 && new * PAGE_SIZE as i64 >= len as i64 {
        return false;
    }
    true
}

/// A query too short for an AUR search: RPC returns half the repository on
/// a single letter.
pub fn aur_query_too_short(query: &str) -> bool {
    !query.is_empty() && query.chars().count() < AUR_MIN_QUERY
}

/// The label for a query to the AUR that is too short.
pub fn aur_query_too_short_msg() -> String {
    trf("AUR search needs at least %1 characters", &[&AUR_MIN_QUERY])
}

/// The section and search label when the KDE store did not give a page
/// ([`ocs::StoreFailure`]): an honest "not responding" rather than
/// "nothing found". The section still shows installed items: this is only
/// the status line.
pub fn store_failure_text(failure: &ocs::StoreFailure) -> String {
    match failure {
        ocs::StoreFailure::Refused { code, message } if !message.is_empty() => trf(
            // TRANSLATORS: %1 is the store's error code, %2 its own message (English, from the server).
            "The KDE Store isn't responding right now (error %1: %2). Please try again later.",
            &[code as &dyn std::fmt::Display, message],
        ),
        ocs::StoreFailure::Refused { code, .. } if !code.is_empty() => trf(
            // TRANSLATORS: %1 is the store's error code.
            "The KDE Store isn't responding right now (error %1). Please try again later.",
            &[code as &dyn std::fmt::Display],
        ),
        ocs::StoreFailure::Refused { .. } | ocs::StoreFailure::Malformed => {
            tr("The KDE Store isn't responding right now. Please try again later.")
        }
        ocs::StoreFailure::Network(raw) => ui_base::humanize_network_error(raw, "KDE Store"),
    }
}

// ── Local search and disk scans ────────────────────────────────────────

/// Whether a theme from disk stays in view under the active query.
///
/// One measure for ALL places where the page shows themes from disk:
/// overview sections ("Active", "Downloaded", "System", "Application
/// style", "Favourites"), search line suggestions and installed themes
/// glued on top of the store in "Splash screen" and "Lock screen". The
/// latter used to be ignored by search entirely: the showcase was
/// filtered, while they stayed in place.
pub fn local_matches(name: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q)
}

/// What exactly to scan on disk by the section key.
///
/// The reverse side of [`AppearanceView::local_scan_which`]: the core
/// builds the key (`widgetstyle:<sub-choice>` for "Application style",
/// otherwise the bare section key), and parsing it back is also up to the
/// core. While parsing lived in the executor, two parties knew the key
/// format, and they could diverge silently: the builder would add a
/// prefix, and the parser would not learn about it.
///
/// Returns `(scan kind, Qt style name)`:
///   * `active`, `downloaded`, `system`, `splash`, `lockscreen`: a scan of
///     its own for each, the name is empty;
///   * `widgetstyle`: "Application style" as one grid: Qt styles and
///     Kvantum themes; the second element is the filter (`all`, `qt`,
///     `kvantum`);
///   * `section`: a whole section, the second element is the section key;
///   * `` (empty): there is no such scan, nothing to show.
pub fn local_scan_plan(which: &str) -> (String, String) {
    if let Some(sub) = which.strip_prefix("widgetstyle:") {
        let filter = match sub {
            STYLE_FILTER_QT | STYLE_FILTER_KVANTUM => sub,
            _ => STYLE_FILTER_ALL,
        };
        return ("widgetstyle".to_string(), filter.to_string());
    }
    match which {
        "widgetstyle" => ("widgetstyle".to_string(), STYLE_FILTER_ALL.to_string()),
        "active" | "downloaded" | "system" | "splash" | "lockscreen" => {
            (which.to_string(), String::new())
        }
        // An environment section: everything of it on disk, enabled or not.
        k if categories::DE_SECTIONS.iter().any(|(_, key)| *key == k) => {
            ("section".to_string(), k.to_string())
        }
        // A KDE section with an installed grid (global themes, Plasma styles,
        // colours, frames, icons, cursors, GTK, sounds): the same section scan,
        // current, own, system.
        k if categories::KDE_INSTALLED_SECTIONS.iter().any(|(_, key)| *key == k) => {
            ("section".to_string(), k.to_string())
        }
        _ => (String::new(), String::new()),
    }
}

/// A section with an installed grid in any mode (excluding the lock
/// screen, which has one only in full themes mode). A theme installed into
/// such a section is visible in that section itself (`local_install`), not
/// only in "Downloaded".
///
/// The key is looked up in the section table ([`categories::categories`]);
/// for the open section use [`has_installed_grid_for`].
pub fn has_installed_grid(key: &str) -> bool {
    categories::categories(Path::new(""))
        .iter()
        .find(|c| c.key == key)
        .is_some_and(has_installed_grid_for)
}

/// The installed grid of a section: ANY section that has both a disk scan
/// ([`local_scan_plan`]) and a store has one.
///
/// Until 17.10.2026 this was decided by a hardcoded list of eight KDE
/// sections and the splash, and a section saw either only the disk or only
/// the store: Conky and the KWin sections (switcher, scripts, effects) had
/// a scan, but what was installed was not shown in their own section at
/// all. Now a new section with a store and a scan gets the "Get new…"
/// button by itself.
///
/// The lock screen is not included: it has a grid only in full themes mode
/// ([`CatalogView::has_installed_view`]).
pub fn has_installed_grid_for(cat: &ThemeCategory) -> bool {
    if cat.virtual_ || cat.key == "lockscreen" || categories::LOCAL_VIEW_KEYS.contains(&cat.key.as_str()) {
        return false;
    }
    let has_store = !cat.local_only();
    has_store && !local_scan_plan(&cat.key).0.is_empty()
}

/// Whether a CARD of a theme from disk stays in view under the active query.
///
/// Beyond the shown name ([`local_matches`]) a theme is also found by its
/// original name: the untranslated name (`source_name`, `Name=` without a
/// language) and the directory name from the card id
/// (`local:<kind>:<directory>`). Otherwise system themes with translations
/// in their metadata got lost: "Adwaita" and "HighContrast" were not found
/// because the card showed their Russian names (live check r1434,
/// 16.10.2026).
///
/// For original names the comparison also ignores spaces, hyphens and
/// underscores: the `HighContrast` directory is found by "High Contrast",
/// `adw-gtk3` by "adw gtk3".
pub fn local_item_matches(item: &ThemeItem, query: &str) -> bool {
    if local_matches(&item.name, query) {
        return true;
    }
    let q = query.trim().to_lowercase();
    let q_squeezed = squeeze_name(&q);
    let dir = local_theme_name(&item.id, "");
    for original in [item.source_name.as_str(), dir.as_str()] {
        if original.is_empty() {
            continue;
        }
        let low = original.to_lowercase();
        if low.contains(&q) || (!q_squeezed.is_empty() && squeeze_name(&low).contains(&q_squeezed)) {
            return true;
        }
    }
    false
}

/// A name without spaces and separator characters, for comparing original names.
fn squeeze_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Filtering a list by the search line, for sections without a network.
pub fn local_filter(items: &[ThemeItem], query: &str) -> Vec<ThemeItem> {
    items
        .iter()
        .filter(|it| local_item_matches(it, query))
        .cloned()
        .collect()
}

/// The status label by scan kind; `None` means the kind has no label of its
/// own and the section name is used.
pub fn scan_status(which: &str, n: usize) -> Option<String> {
    match which {
        "active" => Some(trf("Active themes: %1", &[&n])),
        "downloaded" => Some(trf("Downloaded themes: %1", &[&n])),
        "system" => Some(trf("System themes: %1", &[&n])),
        _ => None,
    }
}

/// The "active" marker in the author label of a local theme.
///
/// A label, NOT a flag: the Rust page recognises the enabled theme by
/// `ThemeItem::local_kind`, which the scan itself sets. Looking for the
/// badge in the label ([`scan_local_kind`]) remains only for the Python
/// window.
pub fn local_active_badge() -> String {
    tr("✅ Active")
}

/// Which card kind to set on a scan item.
///
/// "Active" are all with a badge; "Downloaded" and "System" are plain; for
/// the other scans (application style, splash, lock screen) the active item
/// is recognised by the author label.
pub fn scan_local_kind(which: &str, author: &str) -> &'static str {
    match which {
        "active" => "active",
        "downloaded" | "system" => "downloaded",
        _ => {
            if author.contains(&local_active_badge()) {
                "active"
            } else {
                "downloaded"
            }
        }
    }
}

/// The scan answer is stale: the person has already moved to another section.
///
/// "Application style" scans come with a sub-choice in the key
/// (`widgetstyle:<what>`) and are discarded by their own counter, not by
/// the section name. The original expressed exactly this with three nested
/// conditions, one of which was unreachable.
pub fn scan_answer_stale(which: &str, active_cat_key: &str) -> bool {
    !which.starts_with("widgetstyle") && which != active_cat_key
}

// ── Lock screen ────────────────────────────────────────────────────────

/// Rewrite the category of online cards for the "Lock screen" mode.
///
/// The cards come from the Global Themes (722) or Wallpapers (299) catalog,
/// and the installer would apply the WHOLE Global Theme or just copy the
/// wallpaper. The synthetic category makes it touch exactly the lock
/// component (`lockscreen`) or its background (`lockwallpaper`). Installed
/// local cards already come with the right category, so they are left
/// alone.
pub fn stamp_lock_kind(items: &mut [ThemeItem], lock_mode: &str) {
    let forced = if lock_mode == LOCK_MODE_THEMES {
        LOCK_CAT_THEMES
    } else {
        LOCK_CAT_WALLPAPER
    };
    for it in items.iter_mut() {
        if it.id.starts_with("local:") {
            continue;
        }
        it.category_id = forced;
    }
}

// ── Background warm-up of categories ───────────────────────────────────

/// The first pages of all NETWORK categories from top to bottom,
/// `(cat_id, sort)`.
///
/// `cat_id` as a string, the same value that goes into the request
/// ([`CatalogView::query_category_id`]); otherwise the cache key would not
/// match. The virtual "Popular"/"New" are included (they aggregate the
/// environment's common categories with a fixed sort); local disk
/// overviews, sections without a store and sections missing in the
/// environment are skipped. The ids are taken from the section catalog
/// (`store`), not from the section id: a synthetic id above nine hundred
/// never goes to the store. Dedup by pair.
pub fn prefetch_targets(view: &CatalogView, sidebar: &[ThemeCategory]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for cat in sidebar {
        if !prefetchable(cat) || steamos_closed(view, cat) || !shown_here(view, cat) {
            continue;
        }
        let key = (prefetch_cat_id(view, cat), view.sort_for(cat));
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The category id for warming up a section: the same one that goes into
/// the live request: for "Popular" and "New" these are the environment's
/// common categories ([`categories::aggregate_query`]).
fn prefetch_cat_id(view: &CatalogView, cat: &ThemeCategory) -> String {
    store_query(cat, &view.de)
}

/// The section's OCS category ids as a string for the request (an umbrella
/// is comma-separated, as the store understands it): from the `store`
/// field, not from the section id. The combined showcase uses the
/// environment's common categories ([`categories::aggregate_query`]). A
/// non-OCS catalog gives an empty string.
pub fn store_query(cat: &ThemeCategory, de: &str) -> String {
    match &cat.store {
        CatalogSource::OcsShowcase => categories::aggregate_query(de),
        CatalogSource::Ocs(cats) => cats.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(","),
        _ => String::new(),
    }
}

/// The section is visible in the catalog's environment: the person will
/// not open a section missing here, and there is no point warming up its
/// showcase.
fn shown_here(view: &CatalogView, cat: &ThemeCategory) -> bool {
    categories::category_available_in(&cat.key, &view.de, view.session)
}

/// The section is closed on SteamOS: its showcase will not be shown to the
/// person, so there is no point warming it up.
fn steamos_closed(view: &CatalogView, cat: &ThemeCategory) -> bool {
    view.steamos && categories::steamos_block(&cat.key).is_some()
}

/// Whether the section goes to the network for its catalog.
///
/// The catalog comes from the `store` field: environment sections with
/// their own OCS category under a synthetic id (XFCE frames, 138) are
/// warmed up with the same request that goes out live. The lock screen is
/// not: its id depends on the mode, and guessing it here would mean warming
/// up the wrong showcase.
fn prefetchable(cat: &ThemeCategory) -> bool {
    !categories::LOCAL_VIEW_KEYS.contains(&cat.key.as_str())
        && cat.key != "lockscreen"
        && matches!(cat.store, CatalogSource::Ocs(_) | CatalogSource::OcsShowcase)
}

/// Neighbours of the open section (N+1, then N+2…) worth warming up first.
///
/// Progressiveness: open a section and we immediately warm up the next one;
/// when switching it opens instantly from the cache.
pub fn prefetch_neighbours(
    view: &CatalogView,
    sidebar: &[ThemeCategory],
    done: &BTreeSet<(String, String)>,
) -> Vec<(String, String)> {
    let Some(idx) = sidebar.iter().position(|c| c.key == view.category.key) else {
        return Vec::new();
    };
    let mut front: Vec<(String, String)> = Vec::new();
    for cat in &sidebar[idx + 1..] {
        if !prefetchable(cat) || steamos_closed(view, cat) || !shown_here(view, cat) {
            continue;
        }
        let key = (prefetch_cat_id(view, cat), view.sort_for(cat));
        if done.contains(&key) || front.contains(&key) {
            continue;
        }
        front.push(key);
    }
    front
}

/// A new warm-up queue: neighbours first, the rest after them, without duplicates.
pub fn prefetch_reorder(
    front: &[(String, String)],
    queue: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out = front.to_vec();
    for k in queue {
        if !front.contains(k) {
            out.push(k.clone());
        }
    }
    out
}

// ── Desktop choice ─────────────────────────────────────────────────────

/// Which desktop to open the catalog with and what to remember along the way.
#[derive(Debug, Clone, PartialEq)]
pub struct DeChoice {
    /// The catalog's desktop.
    pub de_id: String,
    /// What to write to the settings: `(choice, which desktop was running)`.
    /// `None` means nothing to write.
    pub remember: Option<(String, String)>,
}

/// The ACTUALLY running desktop wins: log into a GNOME session and the
/// catalog becomes GNOME's by itself; remembering the previous choice here
/// would be wrong.
///
/// But a choice made by hand is not erased, and this is how one is told
/// from the other: together with the choice we remember which desktop was
/// running AT THE MOMENT of the choice. If it matches the current one, the
/// person decided so, and we keep their choice; if it does not match, they
/// changed desktops, and the choice was inherited rather than made.
///
/// If nothing is known at all, we show the widest set of sections rather
/// than hide half the catalog.
pub fn initial_de(detected: &str, saved: &str, seen: &str) -> DeChoice {
    if !detected.is_empty() && detected != seen {
        return DeChoice {
            de_id: detected.to_string(),
            remember: Some((detected.to_string(), detected.to_string())),
        };
    }
    if !saved.is_empty() {
        return DeChoice { de_id: saved.to_string(), remember: None };
    }
    let de_id = if detected.is_empty() { "kde" } else { detected };
    DeChoice { de_id: de_id.to_string(), remember: None }
}

/// The "Detect automatically" item in the desktop choice.
pub const DE_AUTO_ID: &str = "auto";

/// The desktop choice items with the manual choice label.
///
/// A manual choice lives between launches: that is intended (`initial_de`).
/// But when it differs from the running session, this must be visible on
/// screen: on 19.10.2026 a MATE session showed just "KDE Plasma", and it
/// looked as if Rexon had confused the desktop. So the chosen item is
/// labelled "chosen manually, now <session>", and the first item is
/// "Detect automatically", which resets this choice.
pub fn de_picker_choices(selected: &str, detected: &str) -> Vec<(String, String)> {
    let mut out = crate::categories::appearance_de_choices(selected);
    // Tiling environments share one common item: sway versus Hyprland is
    // not a manual choice but the same item.
    let tiling = |id: &str| {
        crate::de_customization::de_from_id(id) == Some(crate::de_customization::De::Wlroots)
    };
    if detected.is_empty() || selected == detected || (tiling(selected) && tiling(detected)) {
        return out;
    }
    let session_name = out
        .iter()
        .find(|(id, _)| id == detected)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| {
            crate::categories::appearance_de_choices(detected)
                .into_iter()
                .find(|(id, _)| id == detected)
                .map(|(_, n)| n)
                // A recognised but unsupported desktop (UKUI) goes by its name,
                // not by the raw identifier.
                .or_else(|| crate::system::unsupported_desktop_name(detected).map(str::to_string))
                .unwrap_or_else(|| detected.to_string())
        });
    for (id, name) in out.iter_mut() {
        if id == selected {
            *name = trf("%1 (chosen manually, now %2)", &[&name.clone(), &session_name]);
        }
    }
    out.insert(
        0,
        (DE_AUTO_ID.to_string(), trf("Detect automatically (%1)", &[&session_name])),
    );
    out
}

/// What to do for the chosen item: `(catalog desktop, what to write as the
/// choice)`. "Detect automatically" means the session's desktop and an
/// EMPTY choice: with it `initial_de` follows the session by itself on
/// every launch.
pub fn resolve_de_pick(picked: &str, detected: &str) -> (String, String) {
    if picked == DE_AUTO_ID {
        let de = if detected.is_empty() { "kde" } else { detected };
        return (de.to_string(), String::new());
    }
    (picked.to_string(), picked.to_string())
}

// ═══════════════════════════════════════════════════════════════════════
//  Details panel: gallery, state buttons, comments
// ═══════════════════════════════════════════════════════════════════════

/// ⚠️ The separators between stats line slots are a real NON-BREAKING SPACE,
/// not the `&nbsp;` entity: without tags Qt treats such text as plain and
/// prints the entity as is (noticed by a pixel comparison with the old
/// version).
pub const NBSP: &str = "\u{a0}";

// ── Screenshot gallery ─────────────────────────────────────────────────

/// Carousel screenshots EXACTLY as on the site: without the first picture.
///
/// The first slot on pling is the "face of the item": the site shows it
/// only as an icon in the catalog and in the header; it is NOT in the
/// carousel. The same here: the first stays the tile preview, the gallery
/// starts from the second. If the item has only one picture, we show it.
///
/// The links from the store response are scaled-down copies (770×540); in
/// the full-screen view they were stretched to the monitor and looked
/// blurry. Each one goes through the common conversion to the
/// full-resolution original.
pub fn gallery_urls(preview_urls: &[String], preview_url: &str) -> Vec<String> {
    let mut urls: Vec<String> = preview_urls.to_vec();
    if urls.is_empty() && !preview_url.is_empty() {
        urls.push(preview_url.to_string());
    }
    if urls.len() > 1 {
        urls.remove(0);
    }
    urls.iter().map(|u| ocs::full_preview_url(u)).collect()
}

/// Tile pictures in turn: if the first did not load (network, 404,
/// something other than a picture arrived), we take the next one.
///
/// In the store the first picture is sometimes dead while the others are
/// alive: for "Sirin Cursor" (2370501) `previewpic1` is a GIF with CJK
/// characters in the name, the server answers 404 for it, and the tile
/// stayed empty. Order: the tile preview, then all `previewpicN` by
/// number; the last reserve is the full-size originals (pling's
/// scaled-down copy may fail to build while the original is alive). No
/// duplicates.
pub fn preview_candidates(preview_url: &str, preview_urls: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |u: &str, out: &mut Vec<String>| {
        if !u.is_empty() && !out.iter().any(|x| x == u) {
            out.push(u.to_string());
        }
    };
    push(preview_url, &mut out);
    for u in preview_urls {
        push(u, &mut out);
    }
    let small = out.clone();
    for u in &small {
        push(&ocs::full_preview_url(u), &mut out);
    }
    out
}

/// The next picture after the one that did not load; `None` means the reserve is exhausted.
pub fn next_preview_candidate(candidates: &[String], failed: &str) -> Option<String> {
    let pos = candidates.iter().position(|u| u == failed)?;
    candidates.get(pos + 1).cloned()
}

/// A picture with motion? The decision is by the first bytes of the file,
/// not by the name: the preview cache takes the extension from the store
/// link, and for a link without one the file is stored as `.jpg`, even if
/// it is a GIF. Animation means GIF (`GIF87a`/`GIF89a`) and WebP with the
/// animation flag in the `VP8X` header.
pub fn is_animated_image_header(head: &[u8]) -> bool {
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return true;
    }
    head.len() >= 21
        && &head[0..4] == b"RIFF"
        && &head[8..12] == b"WEBP"
        && &head[12..16] == b"VP8X"
        && head[20] & 0x02 != 0
}

/// Whether the start of a file looks like a picture. The server sometimes
/// answers "success" with an error page instead of a picture: such a file
/// is not put on the tile, and we take the item's next picture.
pub fn looks_like_image(head: &[u8]) -> bool {
    let starts = |sig: &[u8]| head.starts_with(sig);
    if starts(b"\x89PNG\r\n\x1a\n")
        || starts(&[0xFF, 0xD8, 0xFF])
        || starts(b"GIF87a")
        || starts(b"GIF89a")
        || starts(b"BM")
        || starts(&[0, 0, 1, 0])
        || (starts(b"RIFF") && head.get(8..12) == Some(b"WEBP"))
        || head.get(4..8) == Some(b"ftyp")
    {
        return true;
    }
    // SVG is text: skip whitespace and look at the root.
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start().to_ascii_lowercase();
    if t.contains("<html") || t.starts_with("<!doctype html") {
        return false;
    }
    t.starts_with("<svg") || t.starts_with("<?xml") || (t.starts_with("<!--") && t.contains("<svg"))
}

/// A gallery step that skips broken frames: a frame that did not load is
/// skipped when flipping. If all but the current one are broken, we stay
/// in place.
pub fn gallery_step_skipping(index: i64, total: i64, delta: i64, failed: &BTreeSet<i64>) -> i64 {
    if total <= 1 || delta == 0 {
        return index;
    }
    let mut next = index;
    for _ in 0..total {
        next = gallery_step(next, total, delta);
        if next == index {
            break;
        }
        if !failed.contains(&next) {
            return next;
        }
    }
    index
}

/// The "2/4" counter without broken frames: the person sees only the live ones.
pub fn gallery_counter_skipping(index: i64, total: i64, failed: &BTreeSet<i64>) -> Option<String> {
    let dead = failed.iter().filter(|&&i| i >= 0 && i < total).count() as i64;
    let alive = total - dead;
    let before = failed.iter().filter(|&&i| i >= 0 && i < index).count() as i64;
    gallery_counter(index - before, alive)
}

/// Flipping with arrows wraps around (convenient with a gamepad and a keyboard).
pub fn gallery_step(index: i64, total: i64, delta: i64) -> i64 {
    if total <= 1 {
        return index;
    }
    (index + delta).rem_euclid(total)
}

/// How many frames the gallery has: full details bring more links than the
/// list, so the counter only grows; otherwise "2/5" would turn into "2/1".
pub fn gallery_total(known: i64, src_len: usize, loaded_len: usize) -> i64 {
    known.max(src_len as i64).max(loaded_len as i64)
}

/// What to show in place of the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GalleryFrame {
    /// There is a frame: show the file.
    Image(String),
    /// The frame will not arrive: the server refused or the link walk is already over.
    Failed,
    /// The frame is still loading.
    Loading,
    /// A single picture that is not there yet: the panel stays as it was.
    Unchanged,
    /// The item has no picture and will not have one: the link walk ended
    /// empty or the only frame did not arrive. Previously "Loading preview…"
    /// stayed here forever: waiting for something that will not come.
    Unavailable,
}

/// The label in place of a frame that did not load. Previously both a
/// refusal and waiting were shown with one "⏳ Loading screenshot…", and on
/// a refusal the label hung until the window was closed.
pub fn gallery_failed_msg() -> String {
    tr("🚫 The screenshot didn't load.\nUse the ◀️ ▶️ arrows to see the other ones.")
}
/// The label in place of a frame that is still loading.
pub fn gallery_loading_msg() -> String {
    tr("⏳ Loading screenshot…")
}

/// The state of the current gallery frame.
pub fn gallery_frame(
    index: i64,
    loaded: &BTreeMap<i64, String>,
    failed: &BTreeSet<i64>,
    finished: bool,
    total: i64,
) -> GalleryFrame {
    if let Some(path) = loaded.get(&index) {
        if !path.is_empty() {
            return GalleryFrame::Image(path.clone());
        }
    }
    if total <= 1 {
        if (total == 0 && finished) || failed.contains(&index) {
            return GalleryFrame::Unavailable;
        }
        return GalleryFrame::Unchanged;
    }
    // Empty "holes" between successful frames: the store had no links to them
    // at all, so such a frame will not appear either.
    if failed.contains(&index) || finished {
        GalleryFrame::Failed
    } else {
        GalleryFrame::Loading
    }
}

/// The "2/5" counter over the picture: visible only when there is more than one frame.
pub fn gallery_counter(index: i64, total: i64) -> Option<String> {
    if total > 1 {
        Some(format!("{}/{}", index + 1, total))
    } else {
        None
    }
}

/// Which frame to land on when ready pictures were picked up from the cache.
///
/// The background warm-up puts the first frame of every visible tile on
/// disk in advance; when an item is opened it is simply picked up, without
/// the network. If the current frame is not among the ready ones and the
/// person has not flipped by hand yet, we land on the earliest ready one.
/// `None` means leave it as is.
pub fn adopt_gallery_index(
    loaded: &BTreeMap<i64, String>,
    index: i64,
    user_navigated: bool,
) -> Option<i64> {
    if loaded.is_empty() || user_navigated {
        return None;
    }
    if loaded.get(&index).is_some_and(|p| !p.is_empty()) {
        return None;
    }
    loaded.keys().next().copied()
}

// ── The main details button ────────────────────────────────────────────

/// The look of the main button: text, tooltip and the flag for the style sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallButton {
    pub text: String,
    pub tooltip: String,
    /// The value of the `applied` property for the style sheet selectors.
    pub state: &'static str,
}

/// The states of the main button: the same as on a catalog tile
/// ([`card_install_button`]).
///
/// As in KDE: "Install" only installs; for an installed theme the same
/// button is "Apply" (`can_apply` means the kind can be applied,
/// [`store_item_can_apply`]); for an enabled one it is "Applied", and a
/// press reapplies it.
pub fn install_button(installed: bool, applied: bool, can_apply: bool) -> InstallButton {
    if applied {
        return InstallButton {
            text: tr("✅ Applied"),
            tooltip: tr("This theme is on now — click to apply it again"),
            state: "true",
        };
    }
    if installed && can_apply {
        return InstallButton {
            text: tr("🎨 Apply"),
            tooltip: tr("The theme is downloaded but not turned on — click to apply it"),
            state: "installed",
        };
    }
    if installed {
        return InstallButton {
            text: trc("theme", "✅ Installed"),
            tooltip: String::new(),
            state: "installed",
        };
    }
    InstallButton {
        text: tr("Install"),
        tooltip: String::new(),
        state: "false",
    }
}

/// What pressing a store item button does (the tile and the main details
/// button are one entry point).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreCardAction {
    /// Download and install WITHOUT applying.
    Install,
    /// Apply what is already installed, by the log entry, without downloading.
    ApplyInstalled,
}

/// The item button decision: an installed item of a kind that can be
/// applied is applied; everything else is installed (again).
pub fn store_card_action(installed: bool, can_apply: bool) -> StoreCardAction {
    if installed && can_apply {
        StoreCardAction::ApplyInstalled
    } else {
        StoreCardAction::Install
    }
}

/// Whether a store item has "apply", by the kind of its section. Fonts,
/// widgets, wallpaper plugins, add-ons and Conky have no "Apply" button on
/// the tile: they are enabled in their installed grid.
pub fn store_item_can_apply(home: &Path, item: &ThemeItem) -> bool {
    categories::category_by_id(home, item.category_id)
        .is_some_and(|c| appearance_installer::kind_has_active_state(c.apply_kind.trim()))
}

/// Whether a section's item is enabled not by "Apply" but by starting
/// (Conky) or by a toggle (add-ons): `Some(button)`; `None` means a theme
/// or a kind without enabling. Decided by the same section kind as for a
/// local tile ([`crate::de_apply::button_for_apply_kind`]).
pub fn store_item_switch(home: &Path, item: &ThemeItem) -> Option<crate::de_apply::Button> {
    let c = categories::category_by_id(home, item.category_id)?;
    switch_of_apply_kind(c.apply_kind.trim())
}

fn switch_of_apply_kind(apply_kind: &str) -> Option<crate::de_apply::Button> {
    use crate::de_apply::Button;
    if apply_kind.is_empty() {
        return None;
    }
    match crate::de_apply::button_for_apply_kind(apply_kind) {
        Button::Apply => None,
        b => Some(b),
    }
}

/// The installed tile id (`local:<kind>:<name>`) for an installed store
/// item `item` with the on-disk name `name`: by it the item's details panel
/// shows the same as the installed tile: "Start at login", the parts and
/// settings of a Conky widget. `None` means the item is not enabled by
/// starting or by a toggle.
pub fn store_local_id(home: &Path, item: &ThemeItem, name: &str) -> Option<String> {
    let c = categories::category_by_id(home, item.category_id)?;
    let kind = c.apply_kind.trim();
    (switch_of_apply_kind(kind).is_some() && !name.is_empty()).then(|| format!("local:{kind}:{name}"))
}

/// The name under which item `content_id` is on disk, from the install log:
/// the first path of the entry that still exists (`exists`). `None` means
/// there is no entry or the theme files were removed by hand.
pub fn logged_theme_name(
    log: &Map<String, Value>,
    content_id: &str,
    exists: impl Fn(&str) -> bool,
) -> Option<String> {
    let paths: Vec<String> = log
        .get(content_id)?
        .get("paths")?
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|p| exists(p))
        .map(str::to_string)
        .collect();
    log_theme_names(&paths).into_iter().next()
}

/// The live [`logged_theme_name`]: the log from disk.
pub fn live_logged_theme_name(home: &str, content_id: &str) -> Option<String> {
    let log = crate::install_journal::store_log_view(&live_install_log_path(home));
    logged_theme_name(&log, content_id, |p| Path::new(p).exists())
}

/// The colours of the main details button: the QML sibling of
/// `install_button`.
///
/// The same reason as for `card_button_colors`: QSS does not work in QML,
/// and the bare colours are needed separately. The value contract is the
/// same: with a hash it is a ready colour, without a hash a skin token
/// name, empty means no colour.
///
/// ⚠️ "✓ Applied" in the details is a muted grey-green, not teal as on the
/// tile: on the details panel the button is large and full width, and a
/// loud colour on it would read as unfinished business. "✓ Installed"
/// deliberately matches the tile: the grid and the details must not tell
/// different stories about the item.
pub fn detail_button_colors(state: &str) -> (&'static str, &'static str, &'static str) {
    match state {
        "true" => ("#3a5a3f", "#cfe8d2", ""),
        "installed" => ("", "#7fe3da", "#4fd1c5"),
        _ => ("accent_green", "#ffffff", ""),
    }
}

/// The icon before the action result message.
pub fn status_prefix(ok: bool) -> &'static str {
    if ok {
        "✅ "
    } else {
        "❌ "
    }
}

// ── Local themes ───────────────────────────────────────────────────────

/// The theme name for applying: local cards have it baked into the id
/// (`local:<kind>:<name>`), the others use the plain name.
pub fn local_theme_name(id: &str, name: &str) -> String {
    if let Some(rest) = id.strip_prefix("local:") {
        if let Some((_, theme)) = rest.split_once(':') {
            return theme.to_string();
        }
    }
    name.to_string()
}

/// Why an old-format theme will not apply.
pub fn stale_global_msg(name: &str) -> String {
    trf(
        "❌ The theme “%1” uses the old Plasma 5 format (no metadata.json), and \
         Plasma 6 can't apply it. You need a version of the theme made for Plasma 6.",
        &[&name],
    )
}

// ── Details panel labels ───────────────────────────────────────────────

/// The item meta line: author, licence, update date, version.
pub fn detail_meta_line(author: &str, license: &str, updated: &str, version: &str) -> String {
    let author = if author.is_empty() { "—" } else { author };
    let license = if license.is_empty() { "—" } else { license };
    let upd: String = updated.chars().take(10).collect();
    let upd = if upd.is_empty() { "—".to_string() } else { upd };
    // ⚠️ Fix of the original (rule #16): it always prepended the letter "v",
    // while the store returns the version both as "5.0" and as "v5.0"; in
    // the second case the details printed "vv5.0".
    let ver = if version.is_empty() {
        String::new()
    } else if version.starts_with(['v', 'V']) {
        version.to_string()
    } else {
        format!("v{version}")
    };
    // TRANSLATORS: author • license • updated <date> <version>.
    trf("%1 • %2 • updated %3 %4", &[&author, &license, &upd, &ver])
}

/// The meta line with the archive size at the end: "author • licence •
/// updated … • 13 KiB". If the size is unknown, the line stays as it was.
pub fn detail_meta_with_size(meta_line: &str, download_size_kib: i64) -> String {
    let size = human_size_kib(download_size_kib);
    if size.is_empty() {
        meta_line.to_string()
    } else {
        format!("{} • {size}", meta_line.trim_end())
    }
}

/// The address of the item's page in the store: the one the store returns
/// in the `detailpage` field (`https://store.kde.org/p/<id>`). Empty means
/// there is no id.
pub fn store_page_url(item_id: &str) -> String {
    if item_id.is_empty() {
        String::new()
    } else {
        format!("https://store.kde.org/p/{item_id}")
    }
}

/// The heading of the file choice block in the store item details panel
/// (the item has several files).
pub fn detail_files_title() -> String {
    tr("Files of this item")
}
/// The explanation under the file choice block heading.
pub fn detail_files_hint() -> String {
    tr("Rexon has picked the main archive. You can choose another file and press “Install”.")
}
/// The mark on the file the "Install" button will install.
pub fn detail_file_will_install() -> String {
    tr("will be installed")
}
/// The main details button when a file other than the automatic one is
/// chosen: it installs the chosen one even if the item is already
/// installed.
pub fn detail_file_install_btn() -> String {
    tr("Install")
}

/// File choice block rows for the page (a JSON array): `name` is the choice
/// key (the page passes it back to `installById`), `meta` the size and
/// upload day for the person, `chosen` the automatically chosen file. With
/// one file or a non-store item it is `[]`, and there is no block.
pub fn store_file_choices_json(item: &ThemeItem) -> String {
    let rows: Vec<Value> = ocs::file_choices(item)
        .into_iter()
        .map(|c| {
            let meta = [human_size_kib(c.size_kib), c.date]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            serde_json::json!({ "name": c.name, "meta": meta, "chosen": c.chosen })
        })
        .collect();
    Value::Array(rows).to_string()
}

/// The "Copy link" button in the item card.
pub fn detail_copy_link_btn() -> String {
    tr("🔗 Copy Link")
}
pub fn detail_copy_link_tooltip() -> String {
    tr("Copy the address of this item's store page")
}

/// The status line after copying the link.
pub fn link_copied_status(url: &str) -> String {
    // TRANSLATORS: status line after "Copy Link"; %1 is the address.
    trf("Link copied: %1", &[&url])
}

/// The "no votes" label: on the tile instead of the star count and in the
/// item card (`ThemeItem::rated_stars` = `None`).
pub fn no_ratings_label() -> String {
    // TRANSLATORS: store item that nobody has rated yet (shown instead of stars).
    tr("No ratings yet")
}

/// The label with rating stars; `None` means no votes, no star.
pub fn detail_stars_label(rating_stars: Option<f64>) -> String {
    match rating_stars {
        Some(stars) => format!("⭐ {}", format_decimal(stars, 1)),
        None => no_ratings_label(),
    }
}

/// The rating icon on a store tile: without votes it is empty, since a star
/// without a number would read as a rating.
pub fn card_star_glyph(rating_stars: Option<f64>) -> &'static str {
    if rating_stars.is_some() {
        "⭐"
    } else {
        ""
    }
}

/// The label with the download counter.
pub fn detail_downloads_label(downloads: i64) -> String {
    format!("{NBSP}{NBSP}{}", trf("⬇️ %1 downloads", &[&human_count(downloads)]))
}

/// The "Category / Install path" label.
pub fn detail_path_label(category_name: &str, install_path: &str) -> String {
    trf("Category: %1\nInstalled to: %2", &[&category_name, &install_path])
}

/// The placeholder when the item has no description at all.
pub fn no_description_html() -> String {
    format!("<i>{}</i>", tr("No description"))
}

/// The "translation ⇄ original" toggle under the text.
///
/// The link colour is taken from the ACTIVE skin and substituted at build
/// time: this is the only place where the colour stays in the text itself,
/// because the style sheet does not paint a fragment inside a label.
fn toggle_link(accent: &str, label: &str, point_size: &str, lead: &str) -> String {
    format!(
        "{lead}<a href='rexon:toggle' style='color:{accent};\
text-decoration:none;font-size:{point_size}'>{label}</a>"
    )
}

/// The comment body together with the translation toggle.
pub fn comment_body_html(
    original: &str,
    translated: &str,
    was_translated: bool,
    show_original: bool,
    accent: &str,
) -> String {
    let body = if show_original { original } else { translated };
    if !was_translated {
        return body.to_string();
    }
    let label = if show_original { tr("🌐 translation") } else { tr("📄 original") };
    format!("{body}{}", toggle_link(accent, &label, "9pt", "&nbsp;&nbsp;"))
}

/// Store smileys as a character, not as a picture from the network.
///
/// The store puts `<img src="…/media/emoticons/<name>.png">` into
/// descriptions. Qt fetches such a picture itself, and for every card this
/// gave a dozen log lines: the socket closes before the answer
/// (`QSslSocket: device not open`), the data arrives empty, then
/// `Error decoding …` with a localised "unsupported image format". The
/// server is fine meanwhile: the 1 KB PNG is served at the same address by
/// hand. There is no point fixing the network path for a 30×30 picture: a
/// character is typed in a font, always visible and independent of the
/// network.
///
/// ⚠️ The names are taken from the live store cache, not made up: exactly
/// nine occur in descriptions (`happy` 88 times, `wink` 23, `cool` 17,
/// `ecstatic` 10, `tongue` 6, `gah` 4, `heart` 4, `sad` and `kiss` once
/// each). An unknown name means the tag is simply dropped: an empty space
/// is more honest than a broken picture and certainly more honest than a
/// random foreign smiley.
///
/// It also shows where smileys come from: the store read
/// `(default 15&deg;)` as `;)` and substituted a wink. That is its parsing,
/// not ours, and we cannot fix it, but at least the character is readable.
pub fn inline_emoticons(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<img") {
        let Some(end_rel) = rest[start..].find('>') else { break };
        let end = start + end_rel + 1;
        let tag = &rest[start..end];
        out.push_str(&rest[..start]);
        if let Some(name) = emoticon_name(tag) {
            out.push_str(emoticon_char(name));
        } else if !tag.contains("/media/emoticons/") {
            // Not a smiley: a description picture, left alone.
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// The smiley name from an `<img>` tag, if it is one.
fn emoticon_name(tag: &str) -> Option<&str> {
    let after = tag.split("/media/emoticons/").nth(1)?;
    let name = after.split(".png").next()?;
    if name.is_empty() || name.contains('"') || name.contains('/') {
        return None;
    }
    Some(name)
}

/// A character instead of a store smiley; an unknown name gives an empty string.
fn emoticon_char(name: &str) -> &'static str {
    match name {
        "happy" => "🙂",
        "wink" => "😉",
        "cool" => "😎",
        "ecstatic" => "😄",
        "tongue" => "😛",
        "gah" => "😩",
        "heart" => "❤️",
        "sad" => "🙁",
        "kiss" => "😘",
        _ => "",
    }
}

/// The "translation in progress" mark under a description not yet
/// translated. It is attached to [`description_html`] INSTEAD of the toggle
/// while there is no translation, and disappears when the translation
/// arrives or the core decides "do not translate".
pub fn description_translating_html() -> String {
    // TRANSLATORS: shown under an item description in the store panel while
    // the description is being machine-translated into the interface
    // language (it disappears when the translation arrives).
    format!("<br><br><i>{}</i>", tr("Translating the description…"))
}

/// The item description together with the translation toggle.
pub fn description_html(
    original: &str,
    translated: &str,
    was_translated: bool,
    show_original: bool,
    accent: &str,
) -> String {
    let body = if show_original { original } else { translated };
    let empty = no_description_html();
    let body = if body.is_empty() { empty.as_str() } else { body };
    // Whatever the store sent (HTML, Markdown, BBCode, plain text) → safe
    // markup: headings, lists, bold, links; no scripts and no pictures
    // loaded behind the person's back (`rich_text`).
    let body = crate::rich_text::store_description_html(&inline_emoticons(body));
    // Code and commands in a store description go left to right in any language.
    let body = crate::ui_base::pre_left_to_right(&body);
    if !was_translated {
        return body;
    }
    let label = if show_original {
        tr("🌐 show translation")
    } else {
        tr("📄 show original")
    };
    format!("{body}{}", toggle_link(accent, &label, "9.5pt", "<br>"))
}

// ── Comments ───────────────────────────────────────────────────────────

/// How many comments we ask the store for at a time.
pub const COMMENTS_PAGESIZE: usize = 10;

/// How deep replies in a thread are indented.
pub const COMMENT_INDENT_PX: i64 = 28;

/// A comment text from the store -> a safe piece of markup (tags stripped).
pub fn clean_comment_text(raw: &str) -> String {
    let text = translator::html_unescape(raw);
    // re.sub(r"<[^>]+>", " ", …): a "<…>" pair with at least one character
    // inside. An empty "<>" does not match the rule and stays text.
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '>' {
                j += 1;
            }
            if j < chars.len() && j > i + 1 {
                stripped.push(' ');
                i = j + 1;
                continue;
            }
        }
        stripped.push(chars[i]);
        i += 1;
    }
    translator::html_escape(&stripped).replace('\n', "<br>")
}

/// A nick for the comparison "comment author == item author".
///
/// The store returns the item author in one field and the comment author in
/// another. On pling it is usually the same login, but case, surrounding
/// spaces and occasionally a display name versus a login can differ. We
/// compare case-insensitively and without surrounding spaces, collapsing
/// inner ones. The store does not promise an exact match, so this is an
/// honest heuristic: a mistake either way costs only the nick colour.
pub fn norm_author(name: &str) -> String {
    let squeezed = name.split_whitespace().collect::<Vec<_>>().join(" ");
    // The original folds case with `casefold`, not `lower`: it additionally
    // equates "ß" with "ss" and the final sigma with the ordinary one. Rust's
    // standard library has no full folding table, so we handle these two
    // cases by hand: for Latin and Greek there are no other differences from
    // `lower`, and comparing nicks in other alphabets does not depend on
    // them.
    squeezed
        .to_lowercase()
        .replace('\u{00df}', "ss")
        .replace('\u{03c2}', "\u{03c3}")
}

/// Whether the item author HIMSELF is commenting: his nick is coloured separately.
pub fn comment_is_product_author(comment_author: &str, product_author: &str) -> bool {
    !comment_author.is_empty() && norm_author(comment_author) == norm_author(product_author)
}

/// The commenter's nick for display.
pub fn comment_author_display(author: &str) -> String {
    if author.is_empty() {
        "—".to_string()
    } else {
        author.to_string()
    }
}

/// The comment date: only the day, without the time.
pub fn comment_date(raw: &str) -> String {
    raw.chars().take(10).collect()
}

/// Reply depth with a ceiling: deeper than the fifth level the indent would eat the whole width.
pub fn comment_depth(raw: i64) -> i64 {
    raw.clamp(0, 4)
}

/// The indent of a reply thread.
pub fn comment_indent(depth: i64) -> i64 {
    comment_depth(depth) * COMMENT_INDENT_PX
}

/// The reply icon before the nick.
pub fn comment_reply_prefix(depth: i64) -> &'static str {
    if comment_depth(depth) > 0 {
        "↪️ "
    } else {
        ""
    }
}

/// Whether there is another page of comments.
///
/// The store does not return the total, so we count by root comments: a
/// full page arrived, so most likely there is a next one too.
pub fn comments_has_more(depths: &[i64]) -> bool {
    depths.iter().filter(|d| **d == 0).count() >= COMMENTS_PAGESIZE
}

/// The heading of the comments section.
///
/// The exact number comes with the full item details; from the cache it is
/// lost, and then an honest "showing N(+)" rather than made-up precision.
pub fn comments_header_text(total: Option<i64>, shown: usize, has_more: bool) -> String {
    if let Some(n) = total {
        if n != 0 {
            return trf("💬 Comments (%1)", &[&n]);
        }
    }
    let plus = if has_more { "+" } else { "" };
    trf("💬 Comments (%1)", &[&format!("{shown}{plus}")])
}

/// The heading when there are no comments at all.
pub fn comments_empty_header() -> String {
    tr("💬 Comments")
}
/// The label instead of an empty list.
pub fn comments_empty_text() -> String {
    tr("No comments yet")
}

// ═══════════════════════════════════════════════════════════════════════
//  Catalog requests: generations, dedup and background warm-up
// ═══════════════════════════════════════════════════════════════════════

/// What to do with a deferred catalog request when the debounce expires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FireOutcome {
    /// Nothing to send.
    Nothing,
    /// The person has already moved on: the request lost its value, do not send it.
    Stale,
    /// Exactly this request is already in flight: a second one does NOT go to
    /// the network, the one in flight just gets its generation updated, and
    /// its answer will be accepted as fresh.
    Deduplicated { key: String },
    /// Send.
    Send { key: String, generation: u64 },
}

/// A deferred catalog request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCatalog {
    pub generation: u64,
    pub key: String,
}

/// Who is in flight for the catalog now and what is queued for warm-up.
///
/// Before the port the generation only discarded SOMEONE ELSE'S answer,
/// while the request itself lived on and occupied the channel: measurement
/// showed up to thirteen simultaneous requests and the response time
/// growing from one second to twelve. Here a request that has left is
/// honestly cancelled, and a duplicate is not sent at all.
#[derive(Debug, Default)]
pub struct CatalogRequests {
    generation: u64,
    pending: Option<PendingCatalog>,
    /// List key -> the generation of the request in flight for it.
    inflight: BTreeMap<String, u64>,
    /// Pages whose warm-up has already been started: returning to a category
    /// must not send the same request again.
    prefetched_pages: BTreeSet<String>,
    prefetch_queue: Vec<(String, String)>,
    prefetch_done: BTreeSet<(String, String)>,
    prefetch_busy: bool,
    closing: bool,
}

impl CatalogRequests {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A view change (section, source, search, page) invalidates everything in
    /// flight for the previous category. Returns the new generation.
    pub fn bump(&mut self) -> u64 {
        self.generation += 1;
        self.pending = None;
        self.generation
    }

    /// Defer the request until the end of the debounce: while the person runs
    /// the mouse over the category list, the network is not touched.
    pub fn schedule(&mut self, generation: u64, key: &str) {
        self.pending = Some(PendingCatalog {
            generation,
            key: key.to_string(),
        });
    }

    /// The debounce expired: what to do with the deferred request.
    pub fn fire(&mut self) -> FireOutcome {
        let Some(pending) = self.pending.take() else {
            return FireOutcome::Nothing;
        };
        if pending.generation != self.generation {
            return FireOutcome::Stale;
        }
        if let Some(live) = self.inflight.get_mut(&pending.key) {
            *live = pending.generation;
            return FireOutcome::Deduplicated { key: pending.key };
        }
        self.inflight.insert(pending.key.clone(), pending.generation);
        FireOutcome::Send {
            key: pending.key,
            generation: pending.generation,
        }
    }

    /// The answer for `key` arrived. `true` means it is stale and will not get
    /// into the grid.
    ///
    /// Removes the request from those in flight in any case: a cancelled one
    /// comes back here the usual way.
    pub fn answer(&mut self, key: &str) -> bool {
        match self.inflight.remove(key) {
            Some(gen) => gen != self.generation,
            // A direct call (a cache hit) has no sender: such an answer is
            // always fresh.
            None => false,
        }
    }

    /// Is the visible category's catalog loading now and waiting for the network?
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.inflight.is_empty()
    }

    /// Which requests to cancel when leaving a category: all except the one we
    /// came here for. The channel is needed more by the open category.
    pub fn stale_keys(&self, keep: Option<&str>) -> Vec<String> {
        self.inflight
            .keys()
            .filter(|k| Some(k.as_str()) != keep)
            .cloned()
            .collect()
    }

    /// Whether to warm up the next page.
    ///
    /// One warm-up per page per session: without it every return to a
    /// category sent the same request again and flooded the channel. A live
    /// request by the person for the same page also counts as a warm-up.
    pub fn claim_page_prefetch(&mut self, key: &str) -> bool {
        if self.prefetched_pages.contains(key) || self.inflight.contains_key(key) {
            return false;
        }
        self.prefetched_pages.insert(key.to_string());
        true
    }

    /// The list cache was reset by hand: page warm-ups can be started again.
    pub fn forget_page_prefetches(&mut self) {
        self.prefetched_pages.clear();
    }

    /// The tab is closing: no new warm-ups are handed out.
    pub fn close(&mut self) {
        self.closing = true;
    }

    /// Append to the warm-up queue everything not in it yet.
    pub fn enqueue_prefetch(&mut self, targets: &[(String, String)]) {
        for key in targets {
            if !self.prefetch_done.contains(key) && !self.prefetch_queue.contains(key) {
                self.prefetch_queue.push(key.clone());
            }
        }
    }

    /// Move the neighbours of the open section to the front of the queue.
    pub fn prioritize_prefetch(&mut self, front: &[(String, String)]) {
        if front.is_empty() {
            return;
        }
        self.prefetch_queue = prefetch_reorder(front, &self.prefetch_queue);
    }

    /// Pairs already warmed up, for selecting neighbours.
    pub fn prefetch_done(&self) -> &BTreeSet<(String, String)> {
        &self.prefetch_done
    }

    pub fn prefetch_queue_len(&self) -> usize {
        self.prefetch_queue.len()
    }

    /// The next warm-up target, one at a time.
    ///
    /// The person's live request matters more than the background warm-up:
    /// while the category they are looking at is still loading, we do not
    /// occupy the channel. The queue is not touched: it catches up after the
    /// answer.
    pub fn next_prefetch(&mut self) -> Option<(String, String)> {
        if self.closing || self.prefetch_busy || self.busy() {
            return None;
        }
        while let Some(key) = self.prefetch_queue.first().cloned() {
            self.prefetch_queue.remove(0);
            if self.prefetch_done.contains(&key) {
                continue;
            }
            self.prefetch_busy = true;
            return Some(key);
        }
        None
    }

    /// The target's warm-up has finished (successfully or not: the cache just will not fill).
    pub fn prefetch_finished(&mut self, key: (String, String)) {
        self.prefetch_busy = false;
        self.prefetch_done.insert(key);
    }
}

// ── Search suggestions ─────────────────────────────────────────────────

/// What to do with suggestions for the current input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuggestAction {
    /// Remove the drop-down.
    Hide,
    /// No network: filter the already loaded list in place, instantly.
    Local,
    /// Ask the store.
    Network,
}

/// Whether suggestions are needed and where to take them from.
///
/// We do not query the store per character: below the length threshold the
/// drop-down is simply closed. The details panel has no suggestions: it
/// has no grid for them to lead to either.
pub fn suggest_action(query: &str, on_grid: bool, view: &CatalogView) -> SuggestAction {
    if query.trim().chars().count() < SUGGEST_MIN_CHARS || !on_grid {
        return SuggestAction::Hide;
    }
    // Cinnamon Spices gives no suggestions, and KDE Store suggestions would
    // lead to the wrong store: the search goes over its own showcase.
    if view.is_local() || view.aur_mode() || view.own_site_store() {
        SuggestAction::Local
    } else {
        SuggestAction::Network
    }
}

/// Whether to accept an answer with suggestions.
///
/// The person kept typing (the generation grew), left the search line or
/// erased the input: the answer is no longer about what they see.
pub fn suggest_answer_fresh(
    answer_generation: u64,
    current_generation: u64,
    search_focused: bool,
    query: &str,
) -> bool {
    answer_generation == current_generation
        && search_focused
        && query.trim().chars().count() >= SUGGEST_MIN_CHARS
}

// ── Catalog status labels ──────────────────────────────────────────────

/// The line "Found: N · showing M" or an honest "nothing".
pub fn catalog_status(total: i64, shown: usize) -> String {
    if shown == 0 {
        return tr("Nothing found");
    }
    trf("Found: %1 · showing %2", &[&total, &shown])
}

/// The status line while a search is active: it shows WHAT is filtering
/// the section.
///
/// On 19.10.2026 the field still held "MishMty" from a previous search,
/// the section showed one tile, and the line wrote the usual "Found": it
/// looked as if the store had disappeared.
pub fn search_status(query: &str, found: i64) -> String {
    if found <= 0 {
        return trf("Search: “%1” — nothing found", &[&query]);
    }
    trf("Search: “%1” — found %2", &[&query, &found])
}

/// Tooltip of the ✕ button in the search line.
pub fn search_clear_tooltip() -> String {
    tr("Clear search")
}

/// While a catalog page is loading.
pub fn status_loading_catalog() -> String {
    tr("⏳ Loading catalog…")
}
/// While the AUR list is loading.
pub fn status_loading_aur() -> String {
    tr("⏳ Loading AUR…")
}
/// While the disk scan is running.
pub fn status_loading_scan() -> String {
    tr("⏳ Loading…")
}
/// While system themes are being classified (it queries the package manager).
pub fn status_scanning_system() -> String {
    tr("🖥️ Looking for system themes…")
}

/// The label during the scan: system themes honestly warn about the delay.
pub fn scan_busy_status(which: &str) -> String {
    if which == "system" {
        status_scanning_system()
    } else {
        status_loading_scan()
    }
}

/// System themes could not be classified reliably.
///
/// The list is then NOT shown at all: otherwise themes from the AUR would
/// leak into the "System" section, and the person would take them for part
/// of the distribution.
pub fn status_system_scan_unavailable() -> String {
    tr("⚠️ Couldn't tell where packages came from — system themes are hidden (so that \
        AUR themes are not shown as system ones). Try again later.")
}

// ── Installation progress ──────────────────────────────────────────────

/// The first line of the installation log: the work has started, but the
/// installer has not yet said what exactly is being done.
pub fn install_preparing() -> String {
    tr("⬇️ Preparing…")
}

/// Installation refusal: the theme archive did not download.
///
/// The network has its own reasons, and the person does not need them:
/// they need an answer to the button they pressed. One line for both
/// versions of the tab.
pub fn install_download_failed() -> String {
    tr("Couldn't download the archive")
}

/// Kinds of long actions on an item: each has its own label, and not all
/// of them have a progress bar.
pub const ACTION_INSTALL: &str = "install";
pub const ACTION_AUR: &str = "aur";
pub const ACTION_APPLY: &str = "apply";
/// "Install from file…": it has no completed fraction (unpacking and
/// copying go uncounted); the label is the same as for installation.
pub const ACTION_LOCAL: &str = "local";

/// The status line label for the duration of a long action.
pub fn action_busy_status(kind: &str, name: &str) -> String {
    match kind {
        ACTION_AUR => trf("🔨 Building “%1” from AUR…", &[&name]),
        ACTION_APPLY => trf("🎨 Applying “%1”…", &[&name]),
        _ => trf("⬇️ Installing “%1”…", &[&name]),
    }
}

/// Whether to rebuild the grid after the action has finished.
///
/// Only applying a theme: the active theme has changed, and the
/// "✅ Active" badge must move to it. Installation does not touch the grid
/// (one tile changes there), and removal rebuilds it right away, so there
/// is nothing to wait for.
///
/// ⚠ And waiting before the rebuild is MANDATORY (`STYLE_STORM_MS`):
/// applying a full theme triggers a Qt style change over D-Bus in OUR OWN
/// process, and rebuilding at that very moment crashed the application.
pub fn reload_after_action(kind: &str, ok: bool) -> bool {
    ok && kind == ACTION_APPLY
}

/// Whether to show the progress bar.
///
/// Only a store download reports the completed fraction. The AUR helper
/// and theme application do not count it at all, and a bar at zero would
/// lie about "running, this much done": they are left with log lines.
pub fn action_shows_progress(kind: &str) -> bool {
    kind == ACTION_INSTALL
}

/// The completed fraction for the progress bar: the installer occasionally
/// reports more than a hundred and less than zero, and the bar must stay a
/// bar.
pub fn install_percent(pct: i64) -> i64 {
    pct.clamp(0, 100)
}

/// The action result with an icon in front.
///
/// ⚠ The icon is added only if the message does not already start with
/// it. The installer returns ready "✅ Installed: …", and an unconditional
/// second icon gave "✅ ✅ Installed": in the original this was fixed in
/// the status line but not in the details panel, where the double icon
/// remained (rule #12: fix it in one place for everyone).
pub fn action_result_status(ok: bool, msg: &str) -> String {
    let text = msg.trim_start();
    if text.starts_with('✅') || text.starts_with('❌') {
        return msg.to_string();
    }
    format!("{}{msg}", status_prefix(ok))
}

/// Label of the "Favourites" section.
pub fn favorites_status(n: usize) -> String {
    trf("In favorites: %1", &[&n])
}

/// Label of the installed grid: the heading above the tiles and the status line.
pub fn installed_status(n: usize) -> String {
    trf("Installed: %1", &[&n])
}

/// The status line of a section where the installed items and the store
/// are on one page: both parts; with nothing installed, only the store.
pub fn combined_status(installed: usize, total: i64, store_shown: usize) -> String {
    let store = catalog_status(total, store_shown);
    if installed == 0 {
        store
    } else {
        format!("{} · {store}", installed_status(installed))
    }
}

/// Label of a local section that has none of its own: the section name and the count.
pub fn local_category_status(category_name: &str, n: usize) -> String {
    format!("{category_name}: {n}")
}

// ═══════════════════════════════════════════════════════════════════════
//  Catalog tiles: labels, tooltips and button kinds
// ═══════════════════════════════════════════════════════════════════════

/// The name on a tile (an item without a name gets a dash, not an empty string).
pub fn card_name(name: &str) -> &str {
    if name.is_empty() {
        "—"
    } else {
        name
    }
}

/// The author on a tile.
pub fn card_author(author: &str) -> &str {
    card_name(author)
}

/// The kind of a store item, with the same label as the "Active" tiles
/// ("📁 Icons"): by the section of its store category. A kind without a
/// scan line (wallpapers, fonts, widgets) gets the icon and the section
/// name. `None` means the category is unknown.
pub fn store_item_kind_label(home: &Path, item: &ThemeItem) -> Option<String> {
    let cat = categories::category_by_id(home, item.category_id)?;
    let h = home.to_string_lossy();
    let kind = cat.apply_kind.trim();
    let scan = installed_scan::types_for(&h, "/usr")
        .into_iter()
        .chain(installed_scan::de_types(&h, "/usr", ""))
        .find(|t| t.apply_kind == kind);
    Some(match scan {
        Some(t) => format!("{} {}", t.icon, installed_scan::type_label(&t)),
        None => format!("{} {}", cat.icon, cat.name),
    })
}

/// The author line on an item tile: in the "New"/"Popular" showcases
/// (`showcase`) it is "kind · author", otherwise as is.
pub fn showcase_author(home: &Path, item: &ThemeItem, showcase: bool) -> String {
    let author = card_author(&item.author).to_string();
    if !showcase {
        return author;
    }
    match store_item_kind_label(home, item) {
        Some(kind) if author.is_empty() => kind,
        Some(kind) => format!("{kind} · {author}"),
        None => author,
    }
}

/// The stats line of a store tile: rating · downloads · update date.
///
/// The slot set is THE SAME as on the AUR tile (★/⬇/📅 versus 👍/📈/📅):
/// a mix of store styles in one grid is not acceptable.
pub fn card_stats_line(rating_stars: Option<f64>, downloads: i64, updated: &str) -> String {
    let day: String = updated.chars().take(10).collect();
    let upd = if day.is_empty() {
        String::new()
    } else {
        format!("{NBSP}{NBSP}📅 {day}")
    };
    // Without votes there is no star (`card_star_glyph`), and the line starts
    // with the label without the indent for the icon.
    let rating = match rating_stars {
        Some(stars) => format!("{NBSP}{}", format_decimal(stars, 1)),
        None => no_ratings_label(),
    };
    format!("{rating}{NBSP}{NBSP}⬇️ {}{upd}", human_count(downloads))
}

/// Tooltip of the store tile stats line (`None` means the item has no date).
pub fn card_updated_tooltip(updated: &str) -> Option<String> {
    let day: String = updated.chars().take(10).collect();
    if day.is_empty() {
        None
    } else {
        Some(trf("Updated: %1", &[&day]))
    }
}

/// The button kind on a tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardButton {
    pub text: String,
    /// The property value for the style sheet selectors.
    pub state: &'static str,
    pub enabled: bool,
}

/// Three states of a store tile button: action / installed / applied.
///
/// "Install" is a filled GREEN action button. "✓ Installed" is an OUTLINED
/// teal one: a different silhouette, not just a different shade, so that
/// the state does not read as yet another offer to install. "✓ Applied" is
/// a FILLED teal one: the theme is not just lying on disk but is enabled
/// right now. This way all three tiles are distinguishable in one grid
/// without even reading the label.
///
/// The button stays clickable in all states: reinstalling and reapplying
/// are ordinary wishes.
pub fn card_install_button(installed: bool, applied: bool, can_apply: bool) -> CardButton {
    if installed && applied {
        return CardButton { text: tr("✅ Applied"), state: "applied", enabled: true };
    }
    // As in KDE: an installed but not enabled theme gets "Apply".
    if installed && can_apply {
        return CardButton { text: tr("🎨 Apply"), state: "true", enabled: true };
    }
    if installed {
        return CardButton { text: trc("theme", "✅ Installed"), state: "true", enabled: true };
    }
    CardButton { text: tr("Install"), state: "false", enabled: true }
}

/// The local tile button: the enabled theme shows a badge, the others an
/// offer to apply.
pub fn card_local_button(local_kind: &str) -> CardButton {
    if local_kind == "active" {
        // ⚠️ The state is "false", not "applied", and that is not a typo. The
        // teal "applied" belongs to a STORE tile: there it distinguishes an
        // applied item from one merely downloaded. A local tile has no such
        // neighbour (the "Active" section consists entirely of applied ones),
        // and the original widgets draw the badge as a normal green button
        // (`#InstallBtn` without the `installed` property). The teal badge was
        // an invention of the port: screenshot measurement on 01.09.2026, fill
        // (43,129,126) versus (91,163,43) in the original.
        CardButton { text: local_active_badge(), state: "false", enabled: false }
    } else {
        CardButton { text: tr("🎨 Apply"), state: "false", enabled: true }
    }
}

/// The local tile button, taking the section kind into account.
///
/// Themes are like [`card_local_button`]: "🎨 Apply", the enabled one gets
/// a badge. Environment add-ons and Conky widgets can have any number
/// enabled, and an enabled one has not a badge but the reverse action:
/// "⏸ Disable" / "⏹ Stop" (decision of 04.10.2026). The kind comes from
/// `de_apply` by the tile id `local:<kind>:<name>`; there is no separate
/// list of sections here.
pub fn card_local_button_for(local_kind: &str, item_id: &str) -> CardButton {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    let button = apply_kind_of_local_id(item_id)
        .map(button_for_apply_kind)
        .unwrap_or(Button::Apply);
    let on = local_kind == "active";
    match button {
        Button::Apply => card_local_button(local_kind),
        _ => switch_button(button, on),
    }
}

/// The button of an add-on or widget: an enabled one gets an outlined
/// status button with the reverse action, a disabled one a filled action
/// button, like the other tiles. One for the local tile
/// ([`card_local_button_for`]) and the installed store item
/// ([`card_store_button`]).
pub fn switch_button(button: crate::de_apply::Button, on: bool) -> CardButton {
    use crate::de_apply::Button;
    match button {
        Button::Toggle if on => CardButton { text: tr("⏸️ Disable"), state: "true", enabled: true },
        Button::Toggle => CardButton { text: tr("▶️ Enable"), state: "false", enabled: true },
        Button::Run if on => CardButton { text: tr("⏹️ Stop"), state: "true", enabled: true },
        Button::Run => CardButton { text: tr("▶️ Start"), state: "false", enabled: true },
        Button::Apply => card_install_button(true, on, true),
    }
}

/// The button of a store item tile. An installed Conky widget or add-on
/// (`switch` is [`store_item_switch`]) is enabled right from the catalog
/// tile, with the same press as on the installed tile: "▶️ Start" /
/// "⏹️ Stop", "▶️ Enable" / "⏸️ Disable" (`on` means running or enabled
/// now). Previously such a tile froze on "✅ Installed" after installation,
/// and it was impossible to tell where to enable the widget (live check
/// 15.11.2026). The rest is [`card_install_button`].
pub fn card_store_button(installed: bool, on: bool, can_apply: bool, switch: Option<crate::de_apply::Button>) -> CardButton {
    match switch {
        Some(button) if installed => switch_button(button, on),
        _ => card_install_button(installed, on, can_apply),
    }
}

/// The main button of a store item's details: the same states as on the
/// tile ([`card_store_button`]).
pub fn store_detail_button(installed: bool, on: bool, can_apply: bool, switch: Option<crate::de_apply::Button>) -> InstallButton {
    match switch {
        Some(button) if installed => {
            let card = switch_button(button, on);
            InstallButton { text: card.text, tooltip: String::new(), state: if on { "true" } else { "installed" } }
        }
        _ => install_button(installed, on, can_apply),
    }
}

/// An SDDM login screen theme cannot be enabled while Plasma Login Manager
/// draws the login: "Apply" on the tile is dimmed and explains the reason
/// in a tooltip ([`sddm_under_plasma_login_reason`]); the press still
/// reaches the installer, which refuses with the same words before the
/// password window (`appearance_installer::root_preflight`). Such a theme
/// can be removed. `plasma_login`: the login is drawn by Plasma Login
/// Manager (`system_plan::is_plasma_login`).
pub fn sddm_blocked_by_plasma_login(item_id: &str, plasma_login: bool) -> bool {
    plasma_login && crate::de_apply::apply_kind_of_local_id(item_id) == Some("sddm")
}

/// The tooltip on the unavailable "Apply" of an SDDM theme: the panel
/// heading of the "Login screen" section (the same words the person sees
/// there).
pub fn sddm_under_plasma_login_reason() -> String {
    plasma_login_panel(false).title
}

/// The action button on the details panel of a local theme: `(label,
/// clickable)`. The same fork as on the tile ([`card_local_button_for`]):
/// an enabled THEME gets the "✅ Active" badge, an add-on and a widget the
/// reverse action.
pub fn detail_local_button(local_kind: &str, item_id: &str) -> (String, bool) {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    let button = apply_kind_of_local_id(item_id)
        .map(button_for_apply_kind)
        .unwrap_or(Button::Apply);
    if button == Button::Apply {
        return if local_kind == "active" {
            (local_applied_btn(), false)
        } else {
            (detail_apply_local_btn(), true)
        };
    }
    let card = card_local_button_for(local_kind, item_id);
    (card.text, true)
}

/// Whether the "Start at login" toggle is visible on the details panel:
/// only for Conky widgets (their start is separate from autostart).
pub fn detail_autostart_visible(item_id: &str) -> bool {
    use crate::de_apply::{apply_kind_of_local_id, button_for_apply_kind, Button};
    apply_kind_of_local_id(item_id).map(button_for_apply_kind) == Some(Button::Run)
}

/// Label of the widget autostart toggle.
pub fn detail_autostart_text() -> String {
    tr("Start at login")
}

/// Local tiles that make sense in the chosen desktop: the tile's section
/// is available in it (`categories::category_available`). A tile of an
/// unknown section stays: hiding it without a reason is not allowed.
///
/// Without this, "Downloaded" on XFCE would contain Aurorae frames with an
/// "Apply" button that writes KWin settings; it is more honest not to show
/// the button where the type does not apply (decision of 04.10.2026).
pub fn local_items_for_de(items: Vec<ThemeItem>, home: &Path, de: &str) -> Vec<ThemeItem> {
    if de.is_empty() {
        return items;
    }
    let cats = categories::categories(home);
    items
        .into_iter()
        .filter(|it| {
            // A subcategory item id (the clock widget, 708) leads to its own
            // section ("Widgets", 705), as in `categories::category_by_id`.
            let section = categories::section_id_for(it.category_id);
            cats.iter()
                .find(|c| c.id == it.category_id)
                .or_else(|| cats.iter().find(|c| c.id == section))
                .is_none_or(|c| categories::category_available(&c.key, de))
        })
        .collect()
}

/// Tile button colours by its state: the QML sibling of
/// `card_install_button`.
///
/// ⚠️ The style sheet (`page_qss`) is QtWidgets; it does not work in QML at
/// all. So next to the QSS string there is a sibling returning BARE
/// colours, and a parity guard makes sure they do not drift apart
/// (`tests/appearance_page_parity.rs`). The colour decision stays single;
/// only the wrappers differ.
///
/// Order: fill, text, border. A value with a hash is a ready colour,
/// without a hash it is the TOKEN NAME of the active skin (the caller
/// substitutes it from `colors()`); an empty string means no colour at all
/// (transparent / no border).
pub fn card_button_colors(state: &str) -> (&'static str, &'static str, &'static str) {
    match state {
        // "✓ Applied" is a FILLED teal one (the border has the same colour so
        // that the button does not "jump" in size against the outlined kind).
        "applied" => ("#2f9e94", "#ffffff", "#2f9e94"),
        // "✓ Installed" is an OUTLINED teal one: a filled green button reads as
        // an offer of an action, an outline as a state.
        "true" => ("", "#7fe3da", "#4fd1c5"),
        // "Install" / "🎨 Apply" is a filled green action button in the colour
        // of the active skin.
        _ => ("accent_green", "#ffffff", ""),
    }
}

/// The paints of a control that the TAB's style sheet overrides on top of
/// the Qt palette.
///
/// ⚠️ WHY A SEPARATE TABLE rather than the common surfaces from `ui_base`.
/// The common surfaces (`surface_button`, `surface_field`) answer the
/// question "how does Qt paint the control ITSELF by palette role". That
/// holds exactly as long as the page has no style sheet of its own.
/// "Appearance" has one, and the role plays no part in it: the drop-down
/// list is painted with the `bg_secondary` token, the page bar with
/// `bg_card`, a secondary button with `bg_card_hover`. Measured on 05.09
/// (bench, "Application style" section): QML put the `Button` role
/// (42,64,85) in the same places and diverged from the original by 6,475
/// pixels on the drop-down list, 2,339 on the refresh button and 2,121 on
/// "Favourites".
///
/// The values are TOKEN NAMES of the active skin (the caller substitutes
/// them from `colors()`), the same contract as [`card_button_colors`]; an
/// empty string means no colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSkin {
    /// Fill at rest.
    pub bg: &'static str,
    /// Fill under the cursor.
    pub hover: &'static str,
    /// Border at rest.
    pub border: &'static str,
    /// Border under the cursor.
    pub hover_border: &'static str,
    /// Text.
    pub fg: &'static str,
    /// Corner radius, points.
    pub radius: i32,
    /// Left and right padding, points.
    pub pad_h: i32,
    /// Top and bottom padding, points.
    pub pad_v: i32,
}

/// The paints of the tab's controls by kind. An unknown kind is like `nav`.
///
/// The kinds repeat the rule groups from [`page_qss`] one to one:
///   * `combo`: `#SortCombo`;
///   * `nav`: `#RefreshBtn, #PageBtn, #BackBtn`;
///   * `secondary`: `#DetailsBtn, #DownloadOnlyBtn, #CommentsMoreBtn`.
pub fn control_colors(kind: &str) -> ControlSkin {
    match kind {
        // The drop-down list: the style sheet gives it no hover highlight of its
        // own, so the fill and border at rest and under the cursor are the same.
        "combo" => ControlSkin {
            bg: "bg_secondary",
            hover: "bg_secondary",
            border: "border",
            hover_border: "border",
            fg: "text_primary",
            radius: 4,
            pad_h: 8,
            pad_v: 5,
        },
        // A secondary button ("Favourites", "Download only", "Show more"): under
        // the cursor only the border changes, the fill stays put.
        "secondary" => ControlSkin {
            bg: "bg_card_hover",
            hover: "bg_card_hover",
            border: "border",
            hover_border: "accent",
            fg: "text_primary",
            radius: 3,
            pad_h: 10,
            pad_v: 6,
        },
        // Refresh, catalog pages, "Back".
        _ => ControlSkin {
            bg: "bg_card",
            hover: "bg_card_hover",
            border: "border",
            hover_border: "accent",
            fg: "text_primary",
            radius: 4,
            pad_h: 12,
            pad_v: 6,
        },
    }
}

/// The "Favourites" button: the label and the flag for the style sheet.
pub fn favorite_button(on: bool) -> CardButton {
    if on {
        CardButton { text: tr("⭐ Favorite"), state: "on", enabled: true }
    } else {
        CardButton { text: tr("☆ Favorite"), state: "off", enabled: true }
    }
}

// ── AUR tile ───────────────────────────────────────────────────────────

/// The out-of-date package mark in the source line.
pub fn aur_out_of_date() -> String {
    format!("  {}", tr("⚠️ out of date"))
}

/// The source line of an AUR tile: the badge and the version.
///
/// The badge is mandatory: in "All" mode AUR tiles are mixed with store
/// ones, and the person must see where the package will come from.
/// `version` arrives already truncated to the actual width: truncation is
/// measured in pixels and lives in the Qt layer; here only the line's
/// composition.
pub fn aur_meta_line(version: &str, out_of_date: bool) -> String {
    let ver = if version.is_empty() { "—" } else { version };
    let ood = if out_of_date { aur_out_of_date() } else { String::new() };
    format!("🛠️ AUR · {ver}{ood}")
}

/// The mark of an AUR tile where packages are MIXED with single store items
/// (the "All" source): how the package differs from its neighbour: it is
/// installed as a system package and asks for a password, and for
/// wallpapers it is a whole set of pictures rather than one. Empty means
/// the mark is not needed (the "AUR" source: everything there is like
/// that). The AUR does not report the package size (neither search nor
/// `info`), so it is not in the mark.
pub fn aur_mix_badge(view: &CatalogView, pkg_category: &str) -> String {
    if !view.both_mode() {
        return String::new();
    }
    if pkg_category == "wallpapers" || view.category.key == "wallpapers" {
        tr("📦 Set · system package · password")
    } else {
        tr("📦 System package · password")
    }
}

/// Tooltip for the [`aur_mix_badge`] mark.
pub fn aur_mix_badge_tooltip() -> String {
    tr("Installed as a system package from AUR: it asks for the administrator password and is removed the same way.")
}

/// A font file (by extension) as the picture of a font tile: the tile draws
/// a text sample with it rather than showing the file as a picture.
pub fn is_font_file(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".ttf", ".otf", ".woff", ".woff2", ".ttc"].iter().any(|e| p.ends_with(e))
}

/// The start of the file is a font (TrueType, OpenType, collection, WOFF).
pub fn looks_like_font(head: &[u8]) -> bool {
    head.starts_with(&[0, 1, 0, 0])
        || head.starts_with(b"OTTO")
        || head.starts_with(b"true")
        || head.starts_with(b"ttcf")
        || head.starts_with(b"wOFF")
        || head.starts_with(b"wOF2")
}

/// How much width is left for the version in the source line: the full
/// label width minus what the badge and the mark take. We never return
/// less than forty, otherwise nothing readable of the version would
/// remain.
pub fn aur_version_budget(text_width: i32, fixed_width: i32) -> i32 {
    40.max(text_width - fixed_width)
}

/// The skeleton of the source line by which the Qt layer measures the width
/// of the fixed part (the version itself is not in it).
pub fn aur_meta_frame(out_of_date: bool) -> String {
    format!("🛠️ AUR · {}", if out_of_date { aur_out_of_date() } else { String::new() })
}

/// Tooltip of the source line.
pub fn aur_version_tooltip(version: &str) -> String {
    trf("Version: %1", &[&if version.is_empty() { "—" } else { version }])
}

/// The stats line of an AUR tile: votes · popularity · update date.
///
/// `updated_day` is a ready date: converting a timestamp to a calendar day
/// depends on the machine's time zone, and doing it here would mean
/// keeping a time zone table of our own in the core.
pub fn aur_stats_line(votes: i64, popularity: f64, updated_day: &str) -> String {
    let pop = if popularity != 0.0 {
        format!("{NBSP}{NBSP}📈 {}", format_decimal(popularity, 1))
    } else {
        String::new()
    };
    let upd = if updated_day.is_empty() {
        String::new()
    } else {
        format!("{NBSP}{NBSP}📅 {updated_day}")
    };
    format!("{NBSP}{}{pop}{upd}", human_count(votes))
}

/// Tooltip of the AUR tile stats line.
pub fn aur_stats_tooltip(votes: i64, popularity: f64, updated_day: &str) -> String {
    let upd = if updated_day.is_empty() { "—" } else { updated_day };
    trf(
        "👍 %1 votes · 📈 AUR popularity %2 · updated %3",
        &[&votes, &format_decimal(popularity, 2), &upd],
    )
}

/// The install button kind on an AUR tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurButton {
    pub text: String,
    pub enabled: bool,
    pub tooltip: String,
}

/// There is nothing to install an AUR package with: neither paru nor yay is
/// in the system. One string for the button tooltip and for the result of
/// an install attempt, in the same words as the appearance rebuild refusal
/// (`aeroshell::REBUILD_NO_HELPER`), without the word "helper".
pub fn aur_no_builder() -> String {
    tr("No AUR package builder found (paru or yay) — installation is unavailable")
}

/// Tooltip of the AUR package install button.
fn aur_build_hint() -> String {
    tr("Built from source — this can take a while")
}

/// The AUR package install button.
///
/// Without a helper (yay/paru) there is nothing to install with: we say so
/// in a tooltip rather than with a silently dead button (rule #13).
pub fn aur_install_button(installed: bool, can_install: bool) -> AurButton {
    if installed {
        return AurButton {
            text: trc("AUR package", "✅ Installed"),
            enabled: false,
            tooltip: aur_build_hint(),
        };
    }
    if !can_install {
        return AurButton {
            text: tr("Install"),
            enabled: false,
            tooltip: aur_no_builder(),
        };
    }
    AurButton {
        text: tr("Install"),
        enabled: true,
        tooltip: aur_build_hint(),
    }
}

/// The button while the package is being built.
pub fn aur_busy_button(busy: bool) -> AurButton {
    if busy {
        AurButton { text: tr("⏳ Building…"), enabled: false, tooltip: String::new() }
    } else {
        AurButton {
            text: tr("Install"),
            enabled: true,
            tooltip: aur_build_hint(),
        }
    }
}

// ── Details panel labels ───────────────────────────────────────────────
//
// They stayed in the Qt layer until the tab moved to QML: a label is the
// same kind of decision as a colour and an order, and a second copy of it
// in the markup would silently drift from the first. The guard is
// `appearance_page_parity.rs`.

/// Return from the details panel to the grid. The double space is as in the original.
pub fn detail_back_btn() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("←  Back to Catalog")
}
/// Remove the installed theme.
pub fn detail_uninstall_btn() -> String {
    tr("🗑️ Remove")
}
/// Open the item's page in the store.
pub fn detail_open_site_btn() -> String {
    tr("🌐 Open Website")
}
pub fn detail_open_site_tooltip() -> String {
    tr("Official page on pling.com (description, comments)")
}
/// Apply a theme that is already on disk.
pub fn detail_apply_local_btn() -> String {
    tr("🎨 Apply")
}
/// The picture tooltip: clicking it opens the full-screen view.
pub fn detail_preview_tooltip() -> String {
    tr("Click the picture to view it full screen")
}
/// While the picture is loading.
pub fn detail_preview_loading() -> String {
    tr("Loading preview…")
}
/// The item has no picture at all.
pub fn detail_preview_unavailable() -> String {
    tr("No preview available")
}
/// Overlay controls on top of the picture.
pub const OVERLAY_ZOOM_BTN: &str = "🔍";
pub fn overlay_zoom_tooltip() -> String {
    tr("View full screen")
}
pub const OVERLAY_PREV_BTN: &str = "◀️";
pub fn overlay_prev_tooltip() -> String {
    tr("Previous screenshot")
}
pub const OVERLAY_NEXT_BTN: &str = "▶️";
pub fn overlay_next_tooltip() -> String {
    tr("Next screenshot")
}
/// Read the rest of the comment thread.
pub fn comments_more_btn() -> String {
    tr("Show More")
}

/// Label of the "open the package page" button.
pub fn aur_open_btn() -> String {
    tr("🌐 Open in AUR")
}
/// Its tooltip.
pub fn aur_open_tooltip() -> String {
    tr("Package page on aur.archlinux.org")
}

// ── Listening to sound themes ──────────────────────────────────────────
//
// Parity with the KDE "System Sounds" module: every theme has a "Listen:"
// row of five icon buttons (bell, warning, message, battery, device; the
// set and the fallback events are in `sound_preview::PREVIEW_SLOTS`) and
// "▶", which plays all five in turn. For a Store item the row appears
// after "Listen before installing": the archive is downloaded into a
// temporary cache, nothing is installed.

/// The OCS "Sounds" section (`categories::categories`, key `sounds`).
pub const SOUND_CATEGORY_ID: i64 = 316;

/// The sound theme on disk shown by a local tile (`local:sounds:<directory>`);
/// `None` means the tile is not a sound theme.
pub fn local_sound_theme(item_id: &str) -> Option<String> {
    item_id
        .strip_prefix("local:sounds:")
        .filter(|n| !n.is_empty() && !n.contains('/'))
        .map(str::to_string)
}

/// What a tile can show full screen: a KDE splash on disk has
/// "Demonstrate" (`ksplashqml <theme> --test`), the token is the theme
/// name; an SDDM theme has a login screen "Preview" in greeter test mode,
/// the token is `sddm:<theme>`
/// ([`crate::system_plan::login_preview_argv_live`]). The "None" tile and
/// others get `None`.
pub fn local_splash_demo(item_id: &str) -> Option<String> {
    if let Some(theme) = item_id.strip_prefix("local:sddm:") {
        let ok = !theme.is_empty() && !theme.contains('/') && !theme.starts_with('-');
        return ok.then(|| format!("{LOGIN_PREVIEW_PREFIX}{theme}"));
    }
    item_id
        .strip_prefix("local:splash:")
        .filter(|n| crate::appearance_installer::splash_demo_argv(n).is_some())
        .map(str::to_string)
}

/// The token of the show button on an SDDM theme tile (see [`local_splash_demo`]).
pub const LOGIN_PREVIEW_PREFIX: &str = "sddm:";

/// A Store item from the "Sounds" section: it can be listened to before installing.
pub fn is_store_sound_item(item: &ThemeItem) -> bool {
    !item.id.starts_with("local:") && item.category_id == SOUND_CATEGORY_ID
}

/// Label of the listening buttons row.
pub fn sound_preview_label() -> String {
    tr("Preview:")
}
/// Tooltip of a single-sound button; `event` is the freedesktop event name,
/// as in KDE ("battery-caution").
pub fn sound_preview_tooltip(event: &str) -> String {
    // TRANSLATORS: %1 is a freedesktop sound event name such as battery-caution.
    trf("Preview sound “%1”", &[&event])
}
/// Tooltip of "▶".
pub fn sound_play_all_tooltip() -> String {
    tr("Play all five sounds in turn")
}
/// Tooltip of a disabled button: there is no such sound in the theme, in
/// its parents or in `freedesktop`.
pub fn sound_missing_tooltip() -> String {
    tr("This theme has no such sound")
}
/// The panel button of a Store item from the "Sounds" section.
pub fn sound_store_preview_btn() -> String {
    tr("🔊 Preview Before Installing")
}
/// Its tooltip.
pub fn sound_store_preview_tooltip() -> String {
    tr("The theme is downloaded to a temporary folder so you can hear its sounds. Nothing is installed.")
}
/// Download progress for listening: the fraction if the server named the
/// size, otherwise how much has already arrived.
pub fn sound_store_progress(done: i64, total: i64) -> String {
    if total > 0 {
        let pct = (done.max(0) * 100 / total).min(100);
        trf("Downloading the theme to preview it: %1 %", &[&pct])
    } else if done > 0 {
        let mb = format_decimal(done as f64 / (1024.0 * 1024.0), 1);
        trf("Downloading the theme to preview it: %1 MB", &[&mb])
    } else {
        tr("Downloading the theme to preview it…")
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Tab header: labels, chips and overlay controls
// ═══════════════════════════════════════════════════════════════════════

/// The tab title.
pub fn page_title() -> String {
    tr("Appearance")
}

/// The smallest tab window size: the sidebar (210) plus two rows of 300
/// tiles with margins: already at 900 the grid does not collapse into one
/// column. The same number as in "Comparison" and "Steam": one product has
/// one lower limit.
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

/// Font size of the tab title (the original's `_apply_heading_skin`).
pub const PAGE_TITLE_POINT_SIZE: i32 = 15;

/// A sidebar line: the icon and the section name with spacing, as in the original.
pub fn sidebar_row_text(icon: &str, name: &str) -> String {
    format!("  {icon}  {name}")
}

/// The heading of the open section above the grid.
pub fn section_title(icon: &str, name: &str) -> String {
    format!("{icon}  {name}")
}

/// The label before the source chips.
pub fn source_label() -> String {
    tr("Store:")
}
/// The label before the sub-chips (style kind, lock mode).
pub fn submode_label() -> String {
    tr("View:")
}
/// The label before the desktop choice.
pub fn de_label() -> String {
    tr("Current desktop:")
}
/// Tooltip of the desktop choice.
pub fn de_tooltip() -> String {
    tr("The catalog shows themes for this desktop.\nIt is detected automatically \
        from the current session.")
}

/// The catalog refresh button and its tooltip.
pub const REFRESH_BTN: &str = "🔄";
pub fn refresh_tooltip() -> String {
    tr("Refresh catalog")
}


/// The "Settings" view button in the catalog section header
/// ([`CatalogView::section_settings_panel`]) and its tooltips, per settings
/// section.
pub fn section_settings_btn() -> String {
    tr("⚙️ Settings")
}
pub fn font_settings_tooltip() -> String {
    tr("Fonts of the interface, terminal, menus and window titles, text smoothing and font DPI")
}
pub fn app_widgets_settings_tooltip() -> String {
    tr("Icons in menus and on buttons, text on toolbar buttons — in all applications")
}
pub fn launch_feedback_settings_tooltip() -> String {
    tr("How the pointer and the task bar show that an application is starting.")
}
pub fn app_animations_settings_tooltip() -> String {
    tr("Animations inside applications")
}
pub fn window_buttons_settings_tooltip() -> String {
    tr("Title bar buttons: which ones, on which side and in what order")
}
pub fn desktop_effects_settings_tooltip() -> String {
    tr("Turn on and set up the effects KWin already has: magic lamp, translucency, blur and others")
}
/// The button tooltip for `panels` settings sections: one line per section.
pub fn section_settings_tooltip(panels: &[&str]) -> String {
    panels.iter().map(|p| section_settings_tooltip_one(p)).collect::<Vec<_>>().join("\n")
}
/// The explanation of the "Settings" tab frame `id` (`qt_settings::SECTIONS`):
/// no second wording of the same thought is introduced.
fn group_desc(id: &str) -> String {
    crate::qt_settings::section_texts(id, "").map(|(_, desc)| desc).unwrap_or_default()
}
fn section_settings_tooltip_one(panel: &str) -> String {
    match panel {
        categories::WINDOW_BUTTONS_KEY => window_buttons_settings_tooltip(),
        // The explanation of the "Wallpaper blur" frame itself: there is no second wording.
        categories::WALLPAPER_BLUR_PANEL => group_desc("blurGroup"),
        categories::EVENT_SOUNDS_PANEL => group_desc("soundThemeGroup"),
        categories::AMBIENT_SOUNDS_PANEL => group_desc("ambientGroup"),
        categories::APP_WIDGETS_PANEL => app_widgets_settings_tooltip(),
        categories::LAUNCH_FEEDBACK_PANEL => launch_feedback_settings_tooltip(),
        categories::ANIMATIONS_KEY => app_animations_settings_tooltip(),
        categories::DESKTOP_EFFECTS_PANEL => desktop_effects_settings_tooltip(),
        categories::AXON_SETTINGS_PANEL => crate::axon_window::settings_description(),
        categories::WALLPAPER_PLAYLIST_PANEL => crate::wallpaper_playlist::settings_tooltip(),
        _ => font_settings_tooltip(),
    }
}
/// The same button, while the settings view is open, leads back to the
/// section's tiles, with the label for returning from a theme card
/// ([`detail_back_btn`]): "back to the catalog" looks the same everywhere
/// on the tab.

/// "Install from file…" in the section header: a theme that is already on
/// disk (`crate::local_install`).
pub fn install_file_btn() -> String {
    tr("Install from File…")
}
pub fn install_file_tooltip() -> String {
    tr("Install a theme that is already on this computer: an archive, a folder or \
        single files — fonts, cursors and icons from Windows, color schemes, wallpapers. \
        Rexon recognizes the kind by the contents.")
}
/// The button's menu items: files or a folder.
pub fn install_file_files_item() -> String {
    tr("Choose Files…")
}
pub fn install_file_folder_item() -> String {
    tr("Choose a Folder…")
}
/// The title of the picker window: where we install.
pub fn install_file_dialog_title(section: &str) -> String {
    trf("Install into “%1”", &[&section])
}

/// Page flipping buttons.
pub fn page_prev_btn() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("← Previous")
}
pub fn page_next_btn() -> String {
    // TRANSLATORS: arrows ← → point the way they should in your language: in right-to-left languages swap them.
    tr("Next →")
}

/// A switch chip: key, label and tooltip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Chip {
    pub key: &'static str,
    pub label: String,
    pub tooltip: String,
}

/// The "Store:" chips. Shown in every real section; where there is nothing
/// to choose from, there is no switch at all (see `source_applicable`).
pub fn source_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: SOURCE_OCS,
            label: "KDE Store".to_string(),
            tooltip: tr("KDE Store catalog (OCS)"),
        },
        Chip {
            key: SOURCE_AUR,
            label: "🛠️ AUR".to_string(),
            tooltip: tr("AUR packages that follow the section's naming convention (Arch-based)"),
        },
        Chip {
            key: SOURCE_BOTH,
            label: trc("store", "All"),
            tooltip: tr("KDE Store + AUR in one list"),
        },
    ]
}

/// The section's "Store:" chips: exactly the sources it has
/// ([`source_keys_for`]): the common ones plus Cinnamon Spices in theme
/// sections by composition.
pub fn source_chips_for(category_key: &str) -> Vec<Chip> {
    source_chips_in(category_key, false)
}

/// [`source_chips_for`] with the "Razer Axon" chip if Axon was found
/// ([`source_keys_in`]).
pub fn source_chips_in(category_key: &str, axon: bool) -> Vec<Chip> {
    let keys = source_keys_in(category_key, axon);
    let mut chips: Vec<Chip> = source_chips().into_iter().filter(|c| keys.contains(&c.key)).collect();
    if keys.contains(&SOURCE_SPICES) && keys.len() > 1 {
        chips.push(Chip {
            key: SOURCE_SPICES,
            label: "Cinnamon Spices".to_string(),
            tooltip: crate::cinnamon_spices::parts_chip_tooltip(category_key),
        });
    }
    if keys.contains(&SOURCE_WALLHAVEN) {
        chips.push(Chip {
            key: SOURCE_WALLHAVEN,
            label: crate::wallhaven::chip_label(),
            tooltip: crate::wallhaven::chip_tooltip(),
        });
    }
    if keys.contains(&SOURCE_AXON) {
        chips.push(Chip {
            key: SOURCE_AXON,
            label: crate::axon_window::chip_label(),
            tooltip: crate::axon_window::chip_tooltip(),
        });
    }
    chips
}

/// The "Lock screen" mode chips for an environment: where there are no full
/// lock themes (Deepin), only the wallpaper chip remains.
pub fn lock_chips_for(de_id: &str) -> Vec<Chip> {
    let mut chips = lock_chips();
    if !categories::lock_themes_supported(de_id) {
        chips.retain(|c| c.key != LOCK_MODE_THEMES);
    }
    chips
}

/// The "Lock screen" mode chips.
pub fn lock_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: LOCK_MODE_THEMES,
            label: tr("🔒 Full Themes"),
            tooltip: tr("Lock screen themes (the QML greeter) live inside Global Themes. \
                Only the lock screen part is applied, not the whole theme. Not every \
                Global Theme has its own lock screen — then the default look is used."),
        },
        Chip {
            key: LOCK_MODE_WALLPAPER,
            label: tr("🖼️ Lock Screen Wallpaper"),
            tooltip: tr("A picture as the lock screen background (thousands to choose \
                from in the wallpaper catalog); the greeter stays the same."),
        },
    ]
}

/// The label before the cursor size chips.
pub fn cursor_size_label() -> String {
    trc("cursor", "Size:")
}

/// Cursor size chip keys: the same numbers as [`crate::themes::CURSOR_SIZES`]
/// (a guard in the parity check keeps them together).
pub const CURSOR_SIZE_KEYS: &[&str] = &["24", "32", "40", "48", "64", "96"];

/// Cursor size chips ("Cursors"). The size applies to the CURRENT cursor
/// theme and reaches the same places as the theme choice: the
/// environment's native key, GTK, environment variables, the tiling
/// compositor config.
pub fn cursor_size_chips() -> Vec<Chip> {
    let tooltip = tr("Pointer size in pixels. Rexon applies it to the cursor theme in use \
        — in the desktop's own settings, GTK and Qt apps, and the config of \
        sway, niri or Hyprland.");
    CURSOR_SIZE_KEYS
        .iter()
        .map(|key| Chip { key, label: (*key).to_string(), tooltip: tooltip.clone() })
        .collect()
}

/// "Application style" filter: Qt styles and Kvantum themes together.
pub const STYLE_FILTER_ALL: &str = "all";
/// "Application style" filter: only Qt styles.
pub const STYLE_FILTER_QT: &str = "qt";
/// "Application style" filter: only Kvantum themes.
pub const STYLE_FILTER_KVANTUM: &str = "kvantum";

/// Filter chips above the "Application style" grid.
///
/// Until 17.10.2026 every Qt style was a chip, and under the chip stood
/// ONE card of the same style (carried over from the Python window): the
/// section honestly wrote "Application style: 1", and Kvantum themes
/// opened only with the "Kvantum" chip. Now everything is in one grid, and
/// the chips only narrow it.
pub fn style_filter_chips() -> Vec<Chip> {
    vec![
        Chip {
            key: STYLE_FILTER_ALL,
            label: trc("style filter", "All"),
            tooltip: tr("Qt styles and Kvantum themes together"),
        },
        Chip {
            key: STYLE_FILTER_QT,
            label: tr("Qt styles"),
            tooltip: tr("Styles built into Qt and installed as plugins (Breeze, Fusion, Oxygen…)"),
        },
        Chip {
            key: STYLE_FILTER_KVANTUM,
            label: "Kvantum".to_string(),
            tooltip: tr("Kvantum themes (Kvantum engine)"),
        },
    ]
}

/// The label before the orientation chips.
pub fn orientation_bar_label() -> String {
    tr("Orientation:")
}

/// The orientation chips of a store that knows the picture size
/// ([`CatalogView::orientation_bar`]): "All" (empty key), then the shared
/// [`WallpaperOrientation`] items with the same labels as in the Razer Axon
/// filter panel.
pub fn orientation_chips() -> Vec<Chip> {
    let mut chips = vec![Chip {
        key: "",
        label: trc("orientation filter", "All"),
        tooltip: tr("Horizontal and vertical wallpapers together"),
    }];
    for o in WallpaperOrientation::ALL {
        let tooltip = match o {
            WallpaperOrientation::Landscape => tr("Wallpapers wider than tall — for a regular monitor"),
            WallpaperOrientation::Portrait => {
                tr("Wallpapers taller than wide — for a screen turned upright or for a phone")
            }
        };
        chips.push(Chip { key: o.key(), label: o.label(), tooltip });
    }
    chips
}

/// Which Qt styles to show as "Application style" cards.
///
/// The Kvantum engine is not listed among them: its themes are in the grid
/// instead. The proxy styles `qt5ct-style` / `qt6ct-style` are not a look
/// but a qtNct layer on top of the real style: such a card tells the
/// person nothing, and choosing it changes nothing.
pub fn shown_qt_styles(styles: &[String]) -> Vec<String> {
    styles
        .iter()
        .filter(|s| {
            let low = s.to_lowercase();
            !low.starts_with("kvantum") && low != "qt5ct-style" && low != "qt6ct-style"
        })
        .cloned()
        .collect()
}


// ── Overlay controls on top of the details picture ─────────────────────

/// The corner of a widget inside the details picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPos {
    pub x: i32,
    pub y: i32,
}

/// The magnifier position: the top right corner of the picture.
pub fn overlay_zoom_pos(preview_w: i32, btn_w: i32) -> OverlayPos {
    OverlayPos {
        x: 0.max(preview_w - btn_w - OVERLAY_MARGIN),
        y: OVERLAY_MARGIN,
    }
}

/// The "back" arrow position: the left edge, vertically centred.
pub fn overlay_prev_pos(preview_h: i32, btn_h: i32) -> OverlayPos {
    OverlayPos {
        x: OVERLAY_MARGIN,
        y: 0.max((preview_h - btn_h) / 2),
    }
}

/// The "forward" arrow position: the right edge, vertically centred.
pub fn overlay_next_pos(preview_w: i32, preview_h: i32, btn_w: i32, btn_h: i32) -> OverlayPos {
    OverlayPos {
        x: 0.max(preview_w - btn_w - OVERLAY_MARGIN),
        y: 0.max((preview_h - btn_h) / 2),
    }
}

/// The "2/5" counter position: the bottom of the picture, centred.
pub fn overlay_counter_pos(
    preview_w: i32,
    preview_h: i32,
    counter_w: i32,
    counter_h: i32,
) -> OverlayPos {
    OverlayPos {
        x: 0.max((preview_w - counter_w) / 2),
        y: 0.max(preview_h - counter_h - OVERLAY_MARGIN),
    }
}

/// What to open in the full-screen view: all frames already downloaded, in
/// order, and the place of the current one among them.
///
/// There may be a single frame (a local theme or a single preview); then
/// only it is shown: substituting other pictures into the list is
/// dishonest. `exists` asks the filesystem whether the file really exists.
pub fn zoom_frames<F: Fn(&str) -> bool>(
    current: &str,
    loaded: &BTreeMap<i64, String>,
    exists: F,
) -> Option<(Vec<String>, usize)> {
    if current.is_empty() || !exists(current) {
        return None;
    }
    let paths: Vec<String> = loaded
        .values()
        .filter(|p| exists(p))
        .cloned()
        .collect();
    match paths.iter().position(|p| p == current) {
        Some(i) => Some((paths, i)),
        None => Some((vec![current.to_string()], 0)),
    }
}

/// The viewer frames with a theme's tile picture `small` swapped for its
/// full-size picture `big` ([`crate::models::ThemeItem::preview_big`]): the
/// card shows the small one, the full-screen view the large one. Nothing
/// changes when `big` is empty or not on disk.
pub fn zoom_full_size<F: Fn(&str) -> bool>(paths: Vec<String>, small: &str, big: &str, exists: F) -> Vec<String> {
    if small.is_empty() || big.is_empty() || !exists(big) {
        return paths;
    }
    paths.into_iter().map(|p| if p == small { big.to_string() } else { p }).collect()
}

/// The label of the local theme apply button after success.
pub fn local_applied_btn() -> String {
    tr("✅ Active")
}

#[cfg(test)]
mod tests {

    /// The full-screen view swaps the card's small tile picture for the
    /// theme's full-size one, and only when that file is on disk.
    #[test]
    fn zoom_full_size_swaps_small_for_big() {
        let small = "/t/contents/previews/preview.png";
        let big = "/t/contents/previews/fullscreenpreview.jpg";
        let paths = vec![small.to_string(), "/t/other.png".to_string()];
        assert_eq!(super::zoom_full_size(paths.clone(), small, big, |_| true), vec![big.to_string(), "/t/other.png".to_string()]);
        assert_eq!(super::zoom_full_size(paths.clone(), small, big, |p| p != big), paths);
        assert_eq!(super::zoom_full_size(paths.clone(), small, "", |_| true), paths);
        assert_eq!(super::zoom_full_size(paths.clone(), "", big, |_| true), paths);
    }

    /// The file choice block: lines in the order given, the automatic one
    /// marked, the size and the day on one line; with one file there is no
    /// block.
    #[test]
    fn store_file_choices_rows() {
        let item = ThemeItem {
            name: "Theme".into(),
            download_links: vec![
                serde_json::json!({"url": "https://f/Extra 1.4.tar.xz", "name": "Extra 1.4.tar.xz", "size": 13}),
                serde_json::json!({"url": "https://f/Theme-20230519162903.tar.xz", "name": "Theme-20230519162903.tar.xz", "size": 0}),
            ],
            ..ThemeItem::default()
        };
        let rows: Value = serde_json::from_str(&store_file_choices_json(&item)).unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "Extra 1.4.tar.xz");
        assert_eq!(rows[0]["chosen"], false);
        assert_eq!(rows[0]["meta"], human_size_kib(13));
        assert_eq!(rows[1]["chosen"], true);
        assert_eq!(rows[1]["meta"], "2023-05-19", "размер неизвестен — только день");
        let single = ThemeItem { download_links: vec![item.download_links[0].clone()], ..item };
        assert_eq!(store_file_choices_json(&single), "[]");
    }

    /// A live piece of a description from the store cache: the store read
    /// `(default 15&deg;)` as `;)` and substituted a wink picture.
    #[test]
    fn a_store_emoticon_becomes_a_character() {
        let live = "ap<br />🎚️ Configurable minimum altitude (default 15&amp;deg\
<img src=\"https://www.pling.com/media/emoticons/wink.png\" alt=\"\" /><br />🔵 Catalog filter:";
        let out = super::inline_emoticons(live);
        assert!(out.contains("15&amp;deg😉<br />"), "вышло: {out}");
        assert!(!out.contains("<img"), "картинка осталась: {out}");
    }

    /// All nine names found in the live cache, and the host can be anything:
    /// descriptions contain both pling.com and opendesktop.org.
    #[test]
    fn every_emoticon_seen_in_the_store_has_a_character() {
        for (name, ch) in [
            ("happy", "🙂"), ("wink", "😉"), ("cool", "😎"), ("ecstatic", "😄"),
            ("tongue", "😛"), ("gah", "😩"), ("heart", "❤️"), ("sad", "🙁"),
            ("kiss", "😘"),
        ] {
            let tag = format!("<img src=\"https://www.opendesktop.org/media/emoticons/{name}.png\" alt=\"\" />");
            assert_eq!(super::inline_emoticons(&tag), ch, "смайл {name}");
        }
    }

    /// An unknown name gives an empty space, not a random foreign smiley.
    #[test]
    fn an_unknown_emoticon_leaves_nothing_behind() {
        let tag = "до<img src=\"https://www.pling.com/media/emoticons/zzz-unknown.png\" />после";
        assert_eq!(super::inline_emoticons(tag), "допосле");
    }

    /// The description's own pictures (screenshots) are not touched at all.
    #[test]
    fn a_screenshot_in_the_description_survives() {
        let html = "<p>вид</p><img src=\"https://cdn.pling.com/img/preview.png\" alt=\"\" />";
        assert_eq!(super::inline_emoticons(html), html);
    }

    use super::*;

    #[test]
    fn human_count_switches_units() {
        assert_eq!(human_count(0), "0");
        assert_eq!(human_count(999), "999");
        assert_eq!(human_count(1000), "1.0K");
        assert_eq!(human_count(1234), "1.2K");
        assert_eq!(human_count(999_999), "1000.0K");
        assert_eq!(human_count(1_000_000), "1.0M");
        assert_eq!(human_count(12_345_678), "12.3M");
    }

    #[test]
    fn default_sort_is_the_new_arrivals() {
        assert_eq!(SORTS[default_sort_index()].1, "new");
    }

    #[test]
    fn suggest_box_stays_inside_the_page() {
        // At the right edge the drop-down moves left, but not past the left margin.
        assert_eq!(suggest_x(900, 360, 1000), 1000 - SUGGEST_EDGE - 360);
        assert_eq!(suggest_x(10, 360, 1000), 10);
        assert_eq!(suggest_x(0, 5000, 400), SUGGEST_LEFT);
    }

    #[test]
    fn fatal_http_is_not_retried() {
        assert!(!preview_retry_allowed(Some(404), 1));
        assert!(preview_retry_allowed(Some(500), 1));
        assert!(preview_retry_allowed(None, PREVIEW_RETRIES));
        assert!(!preview_retry_allowed(None, PREVIEW_RETRIES + 1));
    }

    #[test]
    fn page_qss_is_built_from_the_palette() {
        let light = ui_base::skin_colors("light");
        let qss = page_qss(&light);
        assert!(qss.contains("background: #f4f5f6;"));
        // Dimming of an inactive button comes from the skin colour, not white.
        assert!(qss.contains("#PageBtn:disabled { color: rgba(0,0,0,0.55); }"));
        assert!(!qss.contains("rgba(255,255,255,0.55)"));
    }

    #[test]
    fn suggestions_follow_the_skin() {
        for id in ["steam_dark", "light", "synthwave"] {
            let c = ui_base::skin_colors(id);
            let qss = suggestions_qss(&c);
            // Not a single colour outside the skin palette.
            assert!(!qss.contains("#aeb9c4"), "{id}");
            assert!(qss.contains(&format!("background:{}", token(&c, "bg_secondary"))), "{id}");
        }
    }

    #[test]
    fn no_skin_leaves_the_page_text_unreadable() {
        // The label must not match the background it is drawn on.
        for id in ["system", "steam_dark", "light", "synthwave"] {
            let c = ui_base::skin_colors(id);
            for key in ["text_primary", "text_secondary", "text_dim"] {
                assert_ne!(token(&c, key), token(&c, "bg_primary"), "{id}/{key}");
                assert_ne!(token(&c, key), token(&c, "bg_card"), "{id}/{key}");
            }
        }
    }

    /// Which tile "sounds": a local sound theme by id, a Store item by the
    /// "Sounds" section; an id with `/` does not count as a theme.
    #[test]
    fn sound_preview_targets() {
        assert_eq!(local_sound_theme("local:sounds:Ocean").as_deref(), Some("Ocean"));
        assert_eq!(local_sound_theme("local:sounds:"), None);
        assert_eq!(local_sound_theme("local:sounds:../x/y"), None);
        assert_eq!(local_sound_theme("local:icons:Breeze"), None);
        let sounds_cat = categories::categories(Path::new("/h"))
            .into_iter()
            .find(|c| c.key == "sounds")
            .unwrap();
        assert_eq!(sounds_cat.id, SOUND_CATEGORY_ID);
        let store = ThemeItem { id: "2131512".into(), category_id: SOUND_CATEGORY_ID, ..Default::default() };
        assert!(is_store_sound_item(&store));
        let local = ThemeItem { id: "local:sounds:Ocean".into(), category_id: SOUND_CATEGORY_ID, ..Default::default() };
        assert!(!is_store_sound_item(&local));
        let icons = ThemeItem { id: "1".into(), category_id: 132, ..Default::default() };
        assert!(!is_store_sound_item(&icons));
        assert!(sound_store_progress(50, 200).contains("25"));
        assert!(sound_store_progress(3 * 1024 * 1024, 0).contains("3.0"));
        assert!(sound_preview_tooltip("battery-caution").contains("battery-caution"));
    }

    // ── "Restore as it was" after "Apply" ──

    fn revert_scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rexon-applied-revert-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn applied_rec(section: &str, theme: &str, at: Option<&str>, files: Vec<crate::install_journal::AppliedFile>) -> crate::install_journal::Applied {
        crate::install_journal::Applied {
            section: section.into(),
            theme: theme.into(),
            item: None,
            applied_at: at.map(str::to_string),
            snapshot: String::new(),
            files,
        }
    }

    fn file(path: &str) -> crate::install_journal::AppliedFile {
        crate::install_journal::AppliedFile { path: path.into(), before: None, dir: false }
    }

    /// The "what can be restored" model: the label names the theme and the
    /// time of the press; no record (or one without files) means no bar; for
    /// colours with the button visible the shell bar has none of its own,
    /// while a layer note without a record does give one.
    #[test]
    fn applied_revert_banner_model() {
        assert_eq!(applied_when(Some("2026-10-25T14:02:33")).as_deref(), Some("25.10, 14:02"));
        assert_eq!(applied_when(Some("вчера")), None);
        assert_eq!(applied_when(None), None);

        let rec = applied_rec("icons", "Papirus", Some("2026-10-25T14:02:33"), vec![file("/x")]);
        let b = applied_revert_banner("icons", Some(&rec), false, false);
        assert!(b.visible);
        assert_eq!(b.section, "icons");
        assert_eq!(b.button, "Return as It Was Before “Papirus” (25.10, 14:02)");
        assert!(b.text.contains("Papirus") && b.text.contains("25.10, 14:02"), "{}", b.text);
        // Another shell bar and the colours note do not concern icons.
        assert!(applied_revert_banner("icons", Some(&rec), true, true).visible);

        let no_time = applied_rec("icons", "Papirus", None, vec![file("/x")]);
        assert_eq!(applied_revert_banner("icons", Some(&no_time), false, false).button, "Return as It Was Before “Papirus”");

        assert!(!applied_revert_banner("icons", None, false, false).visible);
        let empty = applied_rec("icons", "Papirus", None, Vec::new());
        assert!(!applied_revert_banner("icons", Some(&empty), false, false).visible);

        let colors = applied_rec("colors", "Nord", Some("2026-10-25T09:05:00"), vec![file("/x")]);
        assert!(applied_revert_banner("colors", Some(&colors), true, false).visible);
        let shell = applied_revert_banner("colors", Some(&colors), true, true);
        assert!(!shell.visible, "у цветов кнопка уже есть на полосе шелла");
        let memo_only = applied_revert_banner("colors", None, true, false);
        assert!(memo_only.visible);
        assert_eq!(memo_only.button, "Return Colors as They Were");
        assert!(!applied_revert_banner("colors", None, false, false).visible);
    }

    /// The restore result in words: everything came back / what did not come
    /// back (path relative to home, via "~") / no record.
    #[test]
    fn applied_revert_outcome_texts() {
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(Vec::new())), "/home/u");
        assert!(ok && text.contains("Papirus"), "{text}");
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(vec!["/home/u/.config/kdeglobals".into()])), "/home/u");
        assert!(!ok);
        assert!(text.contains("~/.config/kdeglobals") && text.contains("1 file"), "{text}");
        let (ok, _) = applied_revert_outcome("Papirus", &Ok(None), "/home/u");
        assert!(!ok);
        // Desktop settings get their own line, not "files".
        let failed = vec!["/home/u/.config/kdeglobals".into(), "gsettings org.mate.interface gtk-theme".into(), "xfconf xsettings /Net/ThemeName".into()];
        let (ok, text) = applied_revert_outcome("Papirus", &Ok(Some(failed)), "/home/u");
        assert!(!ok);
        assert!(text.contains("1 file could not") && !text.contains("2 files"), "{text}");
        assert!(text.contains("2 desktop settings could not be put back (org.mate.interface gtk-theme, xsettings /Net/ThemeName)"), "{text}");
    }

    /// A restore on a temporary profile: the icons "Apply" record: the files
    /// are as before, the record and the snapshot are removed, there is no
    /// bar any more.
    #[test]
    fn revert_applied_section_icons() {
        use crate::install_journal as journal;
        let dir = revert_scratch("icons");
        let home = dir.join("home");
        let rexon = home.join(".config/rexon");
        let ini = home.join(".config/gtk-3.0/settings.ini");
        let made = home.join(".config/gtk-4.0/settings.ini");
        std::fs::create_dir_all(ini.parent().unwrap()).unwrap();
        std::fs::create_dir_all(made.parent().unwrap()).unwrap();
        std::fs::write(&ini, "new\n").unwrap();
        std::fs::write(&made, "made\n").unwrap();
        let snap = journal::applied_snapshot_dir(&rexon, "icons");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("0001"), "old\n").unwrap();
        let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
        let mut rec = applied_rec(
            "icons",
            "Papirus",
            Some("2026-10-25T14:02:33"),
            vec![
                journal::AppliedFile { path: s(&ini), before: Some(s(&snap.join("0001"))), dir: false },
                file(&s(&made)),
            ],
        );
        rec.snapshot = s(&snap);
        journal::record_applied_at(&rexon, rec).unwrap();
        let view = journal::view_at(&rexon);
        assert!(applied_revert_banner("icons", view.applied.get("icons"), false, false).visible);

        let palette = crate::shell_palette::PaletteHome { home: s(&home), ..Default::default() };
        let (ok, text) = revert_applied_section(&rexon, &palette, "icons");
        assert!(ok, "{text}");
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), "old\n");
        assert!(!made.exists(), "заведённый нажатием файл остался");
        assert!(!snap.exists(), "снимок остался");
        let view = journal::view_at(&rexon);
        assert!(!applied_revert_banner("icons", view.applied.get("icons"), false, false).visible);
        let (ok, _) = revert_applied_section(&rexon, &palette, "icons");
        assert!(!ok, "второй раз возвращать нечего");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Colours outside Plasma: one button. The layer note restores its own
    /// files (a file rewritten after Rexon is left as is), the "Apply" record
    /// only the rest; the note itself does not come back to life.
    #[test]
    fn revert_applied_section_colors_layer_first() {
        use crate::install_journal as journal;
        use crate::shell_palette as sp;
        for edited in [false, true] {
            let dir = revert_scratch(if edited { "colors-edited" } else { "colors" });
            let home = dir.join("home");
            let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
            let palette = sp::PaletteHome { home: s(&home), ..Default::default() };
            let rexon = home.join(".config/rexon");
            let own = home.join(".config/qt6ct/colors/Rexon.conf");
            let other = home.join(".config/gtk-3.0/gtk.css");
            std::fs::create_dir_all(own.parent().unwrap()).unwrap();
            std::fs::create_dir_all(other.parent().unwrap()).unwrap();
            std::fs::write(&own, if edited { "user" } else { "ours" }).unwrap();
            std::fs::write(&other, "css-new").unwrap();
            let memo_file = sp::layer_memo_path(&palette);
            std::fs::create_dir_all(memo_file.parent().unwrap()).unwrap();
            let memo = sp::LayerMemo {
                files: vec![sp::FileMemo {
                    path: s(&own),
                    kind: "own".into(),
                    original: Some("theirs".into()),
                    written: "ours".into(),
                    keys: Vec::new(),
                    created_tables: Vec::new(),
                }],
                scheme: Some("Nord".into()),
            };
            std::fs::write(&memo_file, serde_json::to_string(&memo).unwrap()).unwrap();
            let snap = journal::applied_snapshot_dir(&rexon, "colors");
            std::fs::create_dir_all(&snap).unwrap();
            std::fs::write(snap.join("0001"), "{\"files\":[]}").unwrap();
            std::fs::write(snap.join("0002"), "stale").unwrap();
            std::fs::write(snap.join("0003"), "css-old").unwrap();
            let mut rec = applied_rec(
                "colors",
                "Nord",
                None,
                vec![
                    journal::AppliedFile { path: s(&memo_file), before: Some(s(&snap.join("0001"))), dir: false },
                    journal::AppliedFile { path: s(&own), before: Some(s(&snap.join("0002"))), dir: false },
                    journal::AppliedFile { path: s(&other), before: Some(s(&snap.join("0003"))), dir: false },
                ],
            );
            rec.snapshot = s(&snap);
            journal::record_applied_at(&rexon, rec).unwrap();

            let (ok, text) = revert_applied_section(&rexon, &palette, "colors");
            assert!(ok, "{text}");
            let want_own = if edited { "user" } else { "theirs" };
            assert_eq!(std::fs::read_to_string(&own).unwrap(), want_own, "правка после Rexon не сбережена / не вернулось");
            assert_eq!(std::fs::read_to_string(&other).unwrap(), "css-old");
            assert!(!memo_file.exists(), "памятка воскресла из снимка");
            assert!(journal::view_at(&rexon).applied.is_empty());
            assert!(!applied_revert_banner("colors", None, sp::read_layer_memo(&palette).has_changes(), false).visible);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod grid_width_tests {
    use super::*;

    #[test]
    fn grid_fills_the_width() {
        let cell = CARD_W + GRID_SPACING;
        // Narrower than one tile: one column of the smallest width.
        assert_eq!((grid_columns(100), grid_cell_width(100)), (1, cell));
        assert_eq!((grid_columns(-5), grid_cell_width(-5)), (1, cell));
        // Exactly two tiles and a bit more: the remainder goes into width.
        assert_eq!((grid_columns(2 * cell), grid_cell_width(2 * cell)), (2, cell));
        assert_eq!((grid_columns(3 * cell - 1), grid_cell_width(3 * cell - 1)), (2, (3 * cell - 1) / 2));
        // A full 1920 screen without the sidebar: a row without an empty strip.
        for usable in [500, 1100, 1586, 1904, 2540] {
            let (c, w) = (grid_columns(usable), grid_cell_width(usable));
            assert!(c * w <= usable && usable - c * w < c, "{usable}: {c}×{w}");
        }
    }
}
