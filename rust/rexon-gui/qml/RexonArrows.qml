// Directional icons and arrow keys — ONE place for all of our QML.
//
// Mirroring (`LayoutMirroring`) rearranges anchors and rows but does not
// touch the icon itself or the keys: for right-to-left languages the
// "back" button moves to the right, while the "❮" on it still points
// left — into the frame rather than outwards. Qt does not help here: "❮"
// is mirrored in Unicode, but Qt draws a label made of a single icon
// without letters left to right and does not flip it (checked on a
// screenshot with `-reverse`). So the icon, the side of the ← → keys and
// the arrow in the tooltip are taken from here, by ONE criterion — whether
// the item that lays out the buttons is mirrored:
//
//     RexonArrows { id: arrows; mirrored: viewer.mirrored }
//     text: arrows.previous
//     case Qt.Key_Left: viewer.step(arrows.leftStep)
//
// `mirrored` is the effective mirroring of the place (`Control.mirrored`,
// `Popup.mirrored`, `LayoutMirroring.enabled` on the item), not the
// application language: where the layout is locally kept left to right
// (physical zones, the vendored tab), icons and keys must stay as they are
// along with it.
//
// Deliberately not here: playback icons (▶ ■) are by convention not
// flipped for right to left — they are about time, not direction; "▾" and
// "▼" (expanded) point down and do not depend on direction. The same "▶"
// as the triangle of a collapsed row is a direction, so it is here
// (`collapsedRow`).
//
// The guard is `rust/rexon-core/tests/rtl_parity.rs`: directional icons in
// our QML are written only in this file.

import QtQml

QtObject {
    id: arrows

    /// Whether the layout of the item placing the buttons is mirrored.
    property bool mirrored: false

    /// "Back" points to the start of the line: left, and right for right to left.
    readonly property string previous: arrows.mirrored ? "❯" : "❮"
    /// "Forward" points to the end of the line.
    readonly property string next: arrows.mirrored ? "❮" : "❯"
    /// Collapsed section: chevron towards the end of the line.
    readonly property string collapsed: arrows.mirrored ? "◂" : "▸"
    /// Collapsed list row (large triangle "▶", expanded — "▼"): also towards
    /// the end of the line.
    readonly property string collapsedRow: arrows.mirrored ? "◀" : "▶"

    /// List step from the ← key (and its pair A): towards the start of the
    /// line — back for left to right, forward for right to left. As in the
    /// galleries of an Arabic interface: the key leads to the side where the
    /// button is.
    readonly property int leftStep: arrows.mirrored ? 1 : -1
    /// Step from the → key (and its pair D).
    readonly property int rightStep: -arrows.leftStep

    /// A string with a ← → arrow (a label, a tooltip with a key) shown in this
    /// place. The ← → arrows are not mirrored in Unicode; Qt itself will not
    /// flip them.
    ///
    /// There is ONE rule for translators across the whole product: they write
    /// the arrow the way it should look in their language (right to left,
    /// "back" is →, "forward" and "then" are ←). So the string is already
    /// correct where the layout of the place matches the writing direction of
    /// the string, and it only needs flipping when they differ: the English
    /// source (no translation, or a run with `-reverse`) in a mirrored place,
    /// and a right-to-left translation in a place kept left to right. The
    /// direction of the string is determined by its letters (Hebrew, Arabic
    /// script), not by the application language: an incompletely translated
    /// catalogue returns the English source, and it must be placed correctly
    /// too.
    function keyHint(text) {
        const s = String(text);
        const writtenRtl = /[֐-ࣿיִ-﷿ﹰ-﻿]/.test(s);
        if (arrows.mirrored === writtenRtl)
            return s;
        return s.replace(/[←→]/g, c => c === "←" ? "→" : "←");
    }

    /// Left edge of a strip of width `width` placed at `start` from the start
    /// of the line inside a strip of width `outer`: for right to left it is
    /// counted from the right edge. Needed only where an anchor will not do and
    /// the coordinate is set by hand (a popup menu, a core calculation in
    /// window coordinates). An ordinary margin from an edge is written with an
    /// anchor (`anchors.left` + `leftMargin`): mirroring flips it by itself.
    /// The function is its own inverse, so the same function converts a real
    /// coordinate back into an offset from the start of the line.
    function placeX(start, width, outer) {
        return arrows.mirrored ? outer - start - width : start;
    }
}
