// Choosing a file of a store item when there are several files.
//
// Which file is taken automatically is decided by the core
// (`ocs::file_choices`, the same rule as for the tile button); rows arrive
// ready as a JSON array `[{name, meta, chosen}]` from `detailFiles`. Here
// we only show and choose: by default the automatic one is selected, a
// click on a row selects another one, and the panel's main button installs
// the selected one (`differs` means a non-automatic one is selected). With
// one file there is no block.
//
// A separate component for the sake of the `LiveModelsProbe.qml` test bench
// (scenario "storefiles"): the whole details panel cannot be brought up
// without a live store.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: block

    /// Skin palette; without it — safe steam_dark colours.
    property var pal: null
    /// Core rows (a JSON array).
    property string filesJson: "[]"
    property string title: ""
    property string hint: ""
    property string willInstallText: ""

    readonly property var files: block.parse(block.filesJson)
    readonly property string autoName: block.autoOf(block.files)
    /// Name of the selected file — the page passes it to `installById`.
    property string selectedName: ""
    readonly property bool differs: block.selectedName !== ""
                                    && block.selectedName !== block.autoName

    readonly property color colText: block.pal ? block.pal.text_primary : "#ffffff"
    readonly property color colTextSecondary: block.pal ? block.pal.text_secondary : "#8f98a0"
    readonly property color colAccent: block.pal ? block.pal.accent : "#66c0f4"
    readonly property color colBorder: block.pal ? block.pal.border : "#2a4055"
    readonly property color colHover: block.pal ? block.pal.bg_card_hover : "#2a3f5a"

    visible: block.files.length > 1
    spacing: 4

    function parse(json) {
        try {
            const rows = JSON.parse(json);
            return Array.isArray(rows) ? rows : [];
        } catch (e) {
            return [];
        }
    }

    function autoOf(rows) {
        for (let i = 0; i < rows.length; i++) {
            if (rows[i].chosen)
                return rows[i].name;
        }
        return "";
    }

    function select(name) {
        block.selectedName = name;
    }

    // A different item (a different list) — the choice is automatic again.
    // The list is computed straight from the string: the `files` binding may
    // not have been re-evaluated yet at this moment.
    onFilesJsonChanged: block.selectedName = block.autoOf(block.parse(block.filesJson))
    Component.onCompleted: block.selectedName = block.autoOf(block.parse(block.filesJson))

    RexonText {
        text: block.title
        color: block.colTextSecondary
        font.bold: true
    }

    RexonText {
        Layout.fillWidth: true
        text: block.hint
        color: block.colTextSecondary
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
    }

    Repeater {
        model: block.files

        delegate: Rectangle {
            id: row

            required property var modelData
            required property int index
            readonly property bool picked: row.modelData.name === block.selectedName

            objectName: "storeFile" + row.index
            Layout.fillWidth: true
            // Control minimum from the canon (Big Picture / Steam Deck).
            implicitHeight: Math.max(40, rowLine.implicitHeight + 12)
            radius: 3
            color: rowMouse.containsMouse ? block.colHover : "transparent"
            border.color: row.activeFocus ? block.colAccent
                                          : (row.picked ? block.colAccent : block.colBorder)
            border.width: row.activeFocus ? 2 : 1
            activeFocusOnTab: true
            Keys.onSpacePressed: block.select(row.modelData.name)
            Keys.onReturnPressed: block.select(row.modelData.name)

            RowLayout {
                id: rowLine
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                spacing: 8

                RexonText {
                    text: row.picked ? "◉" : "○"
                    color: row.picked ? block.colAccent : block.colTextSecondary
                    font.pixelSize: 18
                }

                RexonText {
                    Layout.fillWidth: true
                    text: row.modelData.name
                    color: block.colText
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                }

                RexonText {
                    visible: row.modelData.meta !== ""
                    text: row.modelData.meta
                    color: block.colTextSecondary
                    textFormat: Text.PlainText
                }

                RexonText {
                    objectName: "storeFileMark"
                    visible: row.picked
                    text: block.willInstallText
                    color: block.colAccent
                    font.bold: true
                    textFormat: Text.PlainText
                }
            }

            MouseArea {
                id: rowMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: block.select(row.modelData.name)
            }
        }
    }
}
