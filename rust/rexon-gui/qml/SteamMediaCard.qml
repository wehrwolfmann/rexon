// One showcase card of the "Steam" tab: a thumbnail, a title with an optional
// star and a source badge. Shared by the «🎬 Video splash» and «🌙 Screensaver»
// sub-tabs so that both grids look and behave the same.
//
// Markup only: colours come from the tab styling (`SteamModel.styles`), sizes
// and texts from the caller. Content placed inside the card (default
// property) is laid over the thumbnail — the screensaver sub-tab puts a live
// preview there.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: card

    property var pal: ({})
    /// Tab styling (`SteamModel.styles`): card, thumb.
    property var sty: ({})
    property int thumbWidth: 176
    property int thumbHeight: 110
    property int titleElideWidth: 154

    property string title: ""
    property string poster: ""
    /// Shown in the thumbnail while there is no picture.
    property string placeholder: ""
    property string badge: ""
    property color badgeBackground: "transparent"
    property color badgeColor: card.pal.text_secondary
    property string badgeTooltip: ""
    /// Empty — no star on the card.
    property string starGlyph: ""
    property color starColor: card.pal.text_secondary
    property string starTooltip: ""
    /// Marks the clip Steam itself plays (empty — no mark).
    property string choiceGlyph: ""
    property string choiceTooltip: ""
    /// Only the person's own clips can be deleted (empty — no delete mark);
    /// the caller asks first (rule #13).
    property string deleteGlyph: ""
    property string deleteTooltip: ""
    property bool selected: false
    /// Second line under the title (empty — none).
    property string note: ""

    /// A live preview lies over the thumbnail — no placeholder then.
    property bool live: false
    /// Content over the thumbnail.
    default property alias overlay: overlayHolder.data

    signal chosen()
    signal zoomed()
    signal starToggled()
    signal deleteRequested()
    signal hoverMoved(bool hovered)

    Rectangle {
        anchors.fill: parent
        anchors.margins: card.sty.card.gap / 2
        radius: card.sty.card.radius
        color: card.selected ? card.sty.card.selectedBackground
                             : card.sty.card.background
        border.width: 1
        border.color: card.selected
                      ? (hover.hovered ? card.sty.card.selectedHover
                                       : card.sty.card.selectedBorder)
                      : (hover.hovered ? card.sty.card.hover
                                       : card.sty.card.border)

        HoverHandler {
            id: hover
            onHoveredChanged: card.hoverMoved(hover.hovered)
        }
        TapHandler {
            onTapped: card.chosen()
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: card.sty.card.margin
            spacing: card.sty.card.spacing

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                implicitWidth: card.thumbWidth
                implicitHeight: card.thumbHeight
                radius: card.sty.thumb.radius
                color: card.sty.thumb.svBackground
                clip: true
                // The frame is FITTED whole, not cropped: a square poster sits in the
                // middle with margins on the sides.
                Image {
                    anchors.fill: parent
                    source: card.poster
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                    visible: source !== ""
                }
                RexonText {
                    anchors.centerIn: parent
                    visible: card.poster === "" && !card.live
                    text: card.placeholder
                    font.pixelSize: 22
                }
                Item {
                    id: overlayHolder
                    anchors.fill: parent
                }
                // A click on the card is taken by the choice, so the large view is
                // opened with the magnifier.
                RexonZoomArea {
                    anchors.fill: parent
                    pal: card.pal
                    badgeOnly: true
                    radius: card.sty.thumb.radius
                    onActivated: card.zoomed()
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 4
                RexonText {
                    // The elision width is set by the core: otherwise a long name would
                    // push the star apart.
                    Layout.fillWidth: true
                    Layout.maximumWidth: card.titleElideWidth
                    text: card.title
                    color: card.pal.text_primary
                    elide: Text.ElideRight
                    ToolTip.text: card.title
                    ToolTip.visible: nameHover.hovered
                    HoverHandler { id: nameHover }
                }
                RexonText {
                    visible: card.choiceGlyph !== ""
                    text: card.choiceGlyph
                    font.pixelSize: 13
                    ToolTip.text: card.choiceTooltip
                    ToolTip.visible: choiceHover.hovered && card.choiceTooltip !== ""
                    HoverHandler { id: choiceHover }
                }
                RexonText {
                    visible: card.deleteGlyph !== ""
                    text: card.deleteGlyph
                    font.pixelSize: 13
                    ToolTip.text: card.deleteTooltip
                    ToolTip.visible: deleteHover.hovered && card.deleteTooltip !== ""
                    HoverHandler { id: deleteHover }
                    TapHandler {
                        onTapped: card.deleteRequested()
                    }
                }
                RexonText {
                    visible: card.starGlyph !== ""
                    text: card.starGlyph
                    color: card.starColor
                    font.pixelSize: 15
                    ToolTip.text: card.starTooltip
                    ToolTip.visible: starHover.hovered && card.starTooltip !== ""
                    HoverHandler { id: starHover }
                    TapHandler {
                        onTapped: card.starToggled()
                    }
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: card.note !== ""
                text: card.note
                color: card.pal.text_secondary
                font.pixelSize: 11
                elide: Text.ElideRight
            }

            Rectangle {
                Layout.fillWidth: false
                visible: card.badge !== ""
                implicitWidth: Math.min(card.thumbWidth, badgeText.implicitWidth + 14)
                implicitHeight: badgeText.implicitHeight + 2
                radius: 7
                color: card.badgeBackground
                RexonText {
                    id: badgeText
                    anchors.centerIn: parent
                    width: parent.width - 14
                    text: card.badge
                    color: card.badgeColor
                    font.pixelSize: 11
                    elide: Text.ElideRight
                    ToolTip.text: card.badgeTooltip
                    ToolTip.visible: badgeHover.hovered && card.badgeTooltip !== ""
                    HoverHandler { id: badgeHover }
                }
            }
            Item { Layout.fillHeight: true }
        }
    }
}
