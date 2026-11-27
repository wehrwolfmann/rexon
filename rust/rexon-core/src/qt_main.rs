//! Skeleton of the Rexon main window: title, size and the set of sections.
//!
//! Why a separate module. Until now ONLY Python knew the set of tabs of the main
//! window: the `_TABS` table in `rexon/qt_main/window.py` held their order,
//! labels and icons, while the core only had label translations for the tray
//! menu (`crate::tray`, keys `tab.*`). While the window has one owner this is
//! tolerable; as soon as there are two owners (the PySide6 widget one and the
//! Rust skeleton on QML), two copies of the order silently drift apart — exactly
//! the class of problem rule #12 forbids. What lives here is the ORDER and
//! nothing else: labels and icons come from `crate::tray` (`LABELS` and the icon
//! table), no second copy of them is made.
//!
//! Warning: the module does NOT know what a section is filled with: which QML
//! page or which widget goes into a section is up to the window's owner. Only
//! what is the same for any owner is here.

use crate::i18n::{tr, trf};
use crate::tray;

// ============================================================================
// Window
// ============================================================================

/// Main window title in the interface language. The Russian is the same text as
/// in `RexonMainWindow.__init__` («Rexon — менеджер рабочих столов»).
pub fn window_title() -> String {
    tr("Rexon — Desktop Environment Manager")
}

/// Minimum size of the main window (width, height).
///
/// 1100×750 is not a number pulled out of thin air: at a smaller width the card
/// grid of "Appearance" collapses into a single column and the tab bar of six
/// sections spills into scroll arrows.
pub const WINDOW_MIN_SIZE: (i32, i32) = (1100, 750);

// ============================================================================
// Tab bar
// ============================================================================

/// Fill of an INACTIVE tab — a share of neutral gray
/// ([`crate::ui_base::neutral_rgba`]).
///
/// Warning: this is not decoration but a SEPARATOR. Without their own fill and
/// gaps, inactive tabs merge into a solid strip and you cannot see where one
/// ends and the next begins (remark by Wehrwolfmann on 18.09.2026 about the QML
/// window). The widget window has had the plate since 2026-06-11, and its
/// numbers live here so that the second window owner does not grow its own.
pub const TAB_IDLE_FILL_ALPHA: f64 = 0.10;

/// Fill of an inactive tab UNDER THE CURSOR — a share of the same gray.
pub const TAB_HOVER_FILL_ALPHA: f64 = 0.20;

/// Corner radius of the tab plate, in points.
pub const TAB_RADIUS: i32 = 6;

/// Gap to the left and right of each tab, in points — the very distance by which
/// the eye sees the boundary between neighbors.
pub const TAB_MARGIN_H: i32 = 3;

/// Top and bottom padding of the tab plate, in points.
pub const TAB_MARGIN_V: i32 = 5;

/// Paint of an inactive tab as a QSS `rgba(...)` notation.
pub fn tab_idle_fill() -> String {
    crate::ui_base::neutral_rgba(TAB_IDLE_FILL_ALPHA)
}

/// Paint of an inactive tab under the cursor as a QSS `rgba(...)` notation.
pub fn tab_hover_fill() -> String {
    crate::ui_base::neutral_rgba(TAB_HOVER_FILL_ALPHA)
}

// ============================================================================
// Sections
// ============================================================================

/// Main window sections IN WINDOW ORDER.
///
/// The single source of truth about the ORDER. There are no labels or icons
/// here, and there must not be: both live in [`crate::tray`] — labels in
/// `LABELS`, icons in the icon table — and a second copy would silently drift
/// from the first (rule #12).
///
/// A new section is one line here plus a `tab.<id>` label and an icon in
/// `crate::tray`; the window tab bar, the "Sections" submenu in the tray and the
/// onboarding pick it up on their own.
///
/// "DE comparison" is NOT a separate section: since 08.07.2026 it lives inside
/// "Desktops" — and that is how the product opens it.
pub const SECTIONS: &[&str] = &[
    "desktops",
    "appearance",
    "extensions",
    "wallpaper_engine",
    "steam",
    "settings",
];

/// Section identifiers in window order — the input of `tray::menu`.
pub fn section_ids() -> Vec<&'static str> {
    SECTIONS.to_vec()
}

/// Tab label — exactly the string the buyer sees in the section bar
/// ("🖥 Desktops"), in their language.
///
/// There is no separate assembly of icon and text here: both are assembled by
/// `tray::titled` — the same function the skeleton is checked against
/// (`tests/qt_main_parity.rs`). The only difference from the tray submenu is
/// that the latter gets `tray::section_label` without an icon.
pub fn section_tab_label(id: &str) -> String {
    let titled = tray::titled(&format!("tab.{id}"));
    if titled.is_empty() {
        // An unknown section is labeled with its own id rather than nothing: that way a
        // new tab will at least be visible somehow.
        id.to_string()
    } else {
        titled
    }
}

// ============================================================================
// The Rust window
// ============================================================================
//
// The Rust window no longer has a title of its own: the desktop launcher is
// moving to it, and the caption has to be the product one (`WINDOW_TITLE`). The
// former suffix "(Rust skeleton)" was developer jargon.

/// Text in place of a section this window does not show yet.
///
/// An empty page reads as breakage (rule #13), so the window says it directly
/// and in simple words — without "skeletons" or names of programming languages.
pub fn not_ported_notice(id: &str) -> String {
    let name = tray::section_label(id);
    // TRANSLATORS: %1 is the name of a tab of the main window.
    trf("The “%1” section is not available in this window yet.", &[&name])
}

/// Label of a subsection the skeleton already shows for real.
///
/// A section and a subsection are different things: "DE comparison" lives
/// INSIDE "Desktops", and labeling it as a separate tab would be lying about
/// the structure of the window.
pub fn subsection_label(id: &str) -> String {
    match id {
        "compare" => tr("Compare Desktops"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Russian "as it was" is in `tests/qt_main_parity.rs` against `po/ru.po`.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    #[test]
    fn every_section_has_a_label_and_an_icon() {
        for id in SECTIONS {
            // `section_label` returns the identifier itself when there is no label: that
            // is exactly what we catch — a section without a label in the tray menu.
            assert!(
                tray::label_msgid(&format!("tab.{id}")).is_some(),
                "у раздела «{id}» нет подписи tab.{id}"
            );
            assert!(
                !tray::icon(&format!("tab.{id}")).is_empty(),
                "у раздела «{id}» нет значка в tray::ICONS"
            );
        }
    }

    #[test]
    fn section_ids_are_unique() {
        let mut ids = section_ids();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(total, ids.len(), "идентификаторы разделов повторяются");
    }

    /// The tab label and the tray item label are the same text; the only difference
    /// is the icon. If these two strings drifted apart, one section would be called
    /// differently in the window and in the tray.
    #[test]
    fn tab_label_is_the_tray_label_plus_icon() {
        english();
        for id in SECTIONS {
            let tab = section_tab_label(id);
            let tray_text = tray::section_label(id);
            assert!(
                tab.ends_with(&tray_text),
                "подпись вкладки «{tab}» не содержит подписи лотка «{tray_text}»"
            );
        }
    }

    #[test]
    fn tab_label_carries_icon_and_name() {
        english();
        assert_eq!(section_tab_label("desktops"), "🖥️ Desktops");
        assert_eq!(section_tab_label("steam"), "🎮 Steam");
        // An unknown section does not invent an icon for itself.
        assert_eq!(section_tab_label("nope"), "nope");
    }
}
