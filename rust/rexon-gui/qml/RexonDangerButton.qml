// Button for an irreversible action ("🗑 Delete") in the application skin.
//
// A separate type rather than a `RexonButton` with a tinted background:
// the red button appears on every extension tile, in a list row and in the
// detailed breakdown, and a third copy of the same colour expression would
// silently drift apart (rule #12).
//
// The button defines NO colour of its own: the hue and both opacities
// arrive from the core with the palette (`qt_extensions::DANGER_COLOR` and
// `DANGER_FILL_*_ALPHA`), where they moved from the QSS of the widget
// tab — there it was `rgba(220,80,80,0.2)` and `rgba(220,80,80,0.4)`.

import QtQuick
import QtQuick.Controls

Button {
    id: control

    /// Skin palette; without it the button falls back to safe steam_dark colours.
    property var pal: null

    /// Colours of THIS button from the core when they differ from the shared
    /// palette ones: a map with the keys `color` (hue), `fillAlpha`,
    /// `fillHoverAlpha`, `textColor`, `bold`, `disabledBackground`,
    /// `disabledColor`, `paddingH`, `paddingV`; any of them may be missing.
    ///
    /// WHY. The widget "Settings" has two different red buttons: stopping
    /// wallpapers and sounds — with a bold red label and a grey disabled state —
    /// and "⏹" in the sound row — with the hue of the "Extensions" delete
    /// buttons and its own fill. Both arrive from the core; the button still
    /// defines no colours of its own.
    property var tone: null

    readonly property bool hasDisabledTone: control.tone
                                            && control.tone.disabledBackground !== undefined

    readonly property color colDanger: control.tone && control.tone.color !== undefined
                                       ? control.tone.color
                                       : (control.pal ? control.pal.danger : "#dc5050")
    readonly property real fillAlpha: control.tone && control.tone.fillAlpha !== undefined
                                      ? control.tone.fillAlpha
                                      : (control.pal ? control.pal.danger_fill_alpha : 0.2)
    readonly property real fillHoverAlpha: control.tone
                                           && control.tone.fillHoverAlpha !== undefined
                                           ? control.tone.fillHoverAlpha
                                           : (control.pal ? control.pal.danger_fill_hover_alpha
                                                          : 0.4)
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colTextBtn: control.pal ? control.pal.text_primary : "#ffffff"
    /// Colour of the DISABLED label — from the core palette, as in `RexonButton`.
    readonly property color colTextOff: control.pal ? control.pal.text_disabled
                                                    : "#8cffffff"

    // Height 40 is the control minimum from the canon (Big Picture / Steam Deck).
    implicitHeight: 40
    // A disabled button is dimmed through its LABEL, not through the opacity
    // of the whole button — the same decision for the same reason as in
    // `RexonButton`: opacity changed the fill colour, while the original
    // leaves the fill alone.
    padding: control.tone && control.tone.paddingV !== undefined
             ? control.tone.paddingV : 10
    leftPadding: control.tone && control.tone.paddingH !== undefined
                 ? control.tone.paddingH : 14
    rightPadding: leftPadding
    font.bold: control.tone !== null && control.tone.bold === true

    background: Rectangle {
        radius: 4
        color: !control.enabled && control.hasDisabledTone
               ? control.tone.disabledBackground
               : Qt.rgba(control.colDanger.r, control.colDanger.g, control.colDanger.b,
                         control.hovered || control.down ? control.fillHoverAlpha
                                                         : control.fillAlpha)
        // The focus frame is always visible while the button has focus: it is the
        // focus chain, not a decoration.
        border.color: control.activeFocus ? control.colAccent : "transparent"
        border.width: 2
    }

    // The label sits on the shared baseline of the button font, and a
    // single-character icon is not elided: the same rule for the same reason
    // as in `RexonButton` ("🗑 …" labels in a fallback font sat higher than the
    // neighbouring buttons in the row).
    contentItem: Item {
        implicitWidth: label.implicitWidth
        implicitHeight: labelMetrics.height

        FontMetrics {
            id: labelMetrics
            font: control.font
        }

        RexonText {
            id: label
            width: parent.width
            y: Math.round((parent.height - labelMetrics.height) / 2
                          + labelMetrics.ascent - label.baselineOffset)
            text: control.text
            color: !control.enabled
                   ? (control.tone && control.tone.disabledColor !== undefined
                      ? control.tone.disabledColor : control.colTextOff)
                   : (control.tone && control.tone.textColor !== undefined
                      ? control.tone.textColor : control.colTextBtn)
            font: control.font
            horizontalAlignment: Text.AlignHCenter
            elide: control.text.length <= 3 && control.text.indexOf(" ") < 0
                   ? Text.ElideNone : Text.ElideRight
        }
    }
}
