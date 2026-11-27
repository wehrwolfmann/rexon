// Text for Rexon QML tabs with alignment that is also correct for
// right-to-left languages.
//
// Why a type of our own: a `Text` without explicit alignment takes it from
// the string ITSELF, not from the window layout. With an Arabic interface,
// a string without Arabic letters ("Rexon", "KDE Plasma", a number) aligns
// left in a plain `Text`, while its Arabic neighbour aligns right, and the
// column comes out ragged. Mirroring (`LayoutMirroring`, enabled on the
// window root) flips only EXPLICITLY set alignment: an explicit `AlignLeft`
// becomes `AlignRight` for any string in a right-to-left language and stays
// left in a left-to-right one. Verified by running with `-reverse` on
// offscreen, 06.11.2026: `effectiveHorizontalAlignment` of a `Text` with
// Latin text is left, of this type it is right; `AlignHCenter` and other
// explicit values work as in `Text`.
//
// Without a set width, single-line text takes exactly its own line, and
// alignment has no effect — its space is given by anchors and layouts,
// which mirror themselves.
//
// Where the layout is physical (window buttons, screen layout), mirroring
// is switched off with a local `LayoutMirroring.enabled: false` — then
// `AlignLeft` here stays left too.

import QtQuick

Text {
    horizontalAlignment: Text.AlignLeft
}
