// Entry into the large view from a thumbnail — ONE for the whole product.
//
// Placed over a thumbnail (`anchors.fill: parent`) and makes it
// self-explanatory: under the cursor a magnifier appears in the corner, the
// cursor becomes a hand, and a click opens the large view
// (`RexonImageViewer.qml`). From the keyboard: Tab to the thumbnail, then
// Enter or Space.
//
// `badgeOnly` is for cards where a click on the picture is already taken by
// something else (choosing a clip, opening the description): then only the
// magnifier opens the view, and the rest of the card behaves as before.
//
// The mouse is handled by a `MouseArea`, not a `TapHandler`: a press
// accepted here does not reach the card's own handlers, so one press does
// not do two things.

import QtQuick
import QtQuick.Controls

Item {
    id: area

    /// Page skin palette (the focus frame uses its accent).
    property var pal: null
    /// Only the magnifier opens the view, not the whole thumbnail.
    property bool badgeOnly: false
    /// Tooltip under the cursor.
    property string tooltip: qsTranslate("rexon", "Open large view")
    /// Focus frame rounding — same as the thumbnail underneath.
    property real radius: 4

    /// The thumbnail is asked to be shown large.
    signal activated()

    readonly property color colAccent: area.pal && area.pal.accent !== undefined
                                       ? area.pal.accent : "#66c0f4"
    readonly property bool hot: areaHover.hovered || area.activeFocus

    activeFocusOnTab: area.enabled && area.visible

    Keys.onReturnPressed: area.activated()
    Keys.onEnterPressed: area.activated()
    Keys.onSpacePressed: area.activated()

    HoverHandler { id: areaHover }

    MouseArea {
        id: wholeArea
        anchors.fill: parent
        enabled: !area.badgeOnly
        visible: enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: area.activated()
        ToolTip.text: area.tooltip
        ToolTip.visible: wholeArea.containsMouse && area.tooltip !== ""
        ToolTip.delay: 600
    }

    // Focus frame: from the keyboard it is visible which thumbnail will open.
    Rectangle {
        anchors.fill: parent
        visible: area.activeFocus
        color: "transparent"
        radius: area.radius
        border.color: area.colAccent
        border.width: 2
    }

    Rectangle {
        id: badge
        objectName: "zoomBadge"
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 4
        width: 28
        height: 28
        radius: 14
        visible: area.hot
        color: badgeArea.containsMouse ? "#e6000000" : "#b3000000"
        border.width: 1
        border.color: badgeArea.containsMouse || area.activeFocus ? area.colAccent
                                                                  : "#40ffffff"

        RexonText {
            anchors.centerIn: parent
            text: "🔍"
            font.pixelSize: 14
        }

        MouseArea {
            id: badgeArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: area.activated()
            ToolTip.text: area.tooltip
            ToolTip.visible: badgeArea.containsMouse && area.tooltip !== ""
            ToolTip.delay: 600
        }
    }
}
