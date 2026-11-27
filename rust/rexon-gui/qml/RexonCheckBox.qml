// Check box for Rexon QML tabs in the application's standard skin.
//
// Same reason as for RexonButton: the default QtQuick Controls style
// (Basic) does NOT follow the application palette — its colours are
// hard-coded inside the style itself, and the check box would stay light on
// a dark page. Changing the style for the whole process is not possible:
// the Wallpaper Engine tab with vendored QML lives in the same process.
//
// Colours come from outside through the `pal` property — a map from
// `ui_base::skin_colors("steam_dark")`. The control defines no colours of
// its own (rule #17: one skin for the whole product).

import QtQuick
import QtQuick.Controls

CheckBox {
    id: control

    /// Skin palette; without it the control falls back to safe steam_dark colours.
    property var pal: null

    readonly property color colBox: control.pal ? control.pal.border : "#2a4055"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colText: control.pal ? control.pal.text_primary : "#ffffff"

    // Control minimum from the canon (Big Picture / Steam Deck): tabs must be
    // operable not only with the mouse.
    implicitHeight: 40
    padding: 6

    indicator: Rectangle {
        implicitWidth: 20
        implicitHeight: 20
        // The square goes at the start of the line: on the left, on the right for
        // right-to-left languages.
        x: control.mirrored ? control.width - width - control.rightPadding
                            : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: 3
        // A checked box is the "✅" glyph itself: it already is a green square
        // with a check mark, and our own fill with a border under it would give a
        // "square in a square". So the checked state drops the border and fill,
        // and the glyph takes the whole indicator area. An empty box keeps the
        // old border.
        color: "transparent"
        border.color: control.checked ? "transparent"
                      : (control.activeFocus || control.hovered
                         ? control.colAccent : control.colBox)
        border.width: control.activeFocus ? 2 : 1

        // The check mark itself: a font glyph, not an image — otherwise an
        // external file would be needed, and the page must stay self-contained.
        RexonText {
            anchors.centerIn: parent
            visible: control.checked
            text: "✅"
            // A colour glyph ignores the text colour, so a disabled box is dimmed
            // through the opacity of the glyph itself.
            opacity: control.enabled ? 1.0 : 0.4
            font.pixelSize: 21
        }

        // The focus and hover frame of a checked box goes over the glyph:
        // underneath it would not be visible.
        Rectangle {
            anchors.fill: parent
            visible: control.checked && (control.activeFocus || control.hovered)
            radius: parent.radius
            color: "transparent"
            border.color: control.colAccent
            border.width: control.activeFocus ? 2 : 1
        }
    }

    // Space for the square on the side where it sits.
    contentItem: RexonText {
        leftPadding: control.mirrored ? 0 : control.indicator.width + control.spacing
        rightPadding: control.mirrored ? control.indicator.width + control.spacing : 0
        text: control.text
        color: control.colText
        font: control.font
        textFormat: Text.RichText
        wrapMode: Text.WordWrap
        verticalAlignment: Text.AlignVCenter
    }
}
