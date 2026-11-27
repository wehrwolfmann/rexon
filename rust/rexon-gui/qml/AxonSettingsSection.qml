// The "Razer Axon" block in "⚙️ Settings" of the "HD Wallpapers" section:
// our settings for showing wallpapers from the Razer Axon library —
// "Prepare wallpapers in advance" and "Restore original descriptions", the
// library folder. Axon's own settings are not changed here. The block
// exists only if Axon is found
// (`appearance_page::CatalogView::section_settings_panels`).
//
// Markup only: labels and state — the core (`axon_prepare`, `axon_window`)
// through `AxonActions` (rust/rexon-gui/src/axon_actions.rs).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    signal message(string text)

    readonly property var t: JSON.parse(actions.texts())
    /// `{line, running, enabled, dirs}` — `AxonActions.settingsState`.
    property var st: ({ line: "", running: false, enabled: true, dirs: [] })
    property bool confirmRestore: false

    function reload() {
        section.st = JSON.parse(actions.settingsState());
    }

    AxonActions {
        id: actions
        objectName: "axonSettingsActions"
        onFinished: text => {
            section.message(text);
            section.reload();
        }
    }

    Component.onCompleted: section.reload()

    // Background preparation runs by itself — we refresh the summary line
    // while the block is visible.
    Timer {
        interval: 3000
        repeat: true
        running: section.visible
        onTriggered: section.reload()
    }

    FolderDialog {
        id: folderDialog
        title: section.t.pickTitle || ""
        onAccepted: {
            const answer = JSON.parse(actions.addFolder(selectedFolder.toString()));
            section.message(answer.message);
            section.reload();
        }
    }

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        height: implicitHeight
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: section.t.settingsTitle || ""
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            text: section.t.settingsDescription || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonCheckBox {
            id: prepareAhead
            objectName: "axonPrepareAhead"
            pal: section.pal
            text: section.t.prepareAhead || ""
            checked: section.st.enabled === true
            ToolTip.text: section.t.prepareAheadTooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onToggled: {
                actions.setPrepareAhead(prepareAhead.checked);
                section.reload();
                prepareAhead.checked = Qt.binding(function () { return section.st.enabled === true; });
            }
        }

        RexonText {
            objectName: "axonPrepareLine"
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.line
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            RexonButton {
                objectName: "axonRestore"
                pal: section.pal
                text: section.t.restore || ""
                onClicked: section.confirmRestore = true
            }

            RexonButton {
                objectName: "axonPick"
                pal: section.pal
                text: section.t.pick || ""
                ToolTip.text: section.t.pickTooltip || ""
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: folderDialog.open()
            }

            RexonButton {
                objectName: "axonOpenRoot"
                pal: section.pal
                enabled: section.st.dirs.length > 0
                text: section.t.open || ""
                ToolTip.text: section.st.dirs.length > 0 ? section.st.dirs[0] : ""
                ToolTip.visible: hovered && section.st.dirs.length > 0
                onClicked: actions.openFolder(section.st.dirs[0])
            }

            Item { Layout.fillWidth: true }
        }

        // Restoring descriptions is irreversible for wallpapers already decrypted —
        // first a warning in words, then the action.
        Rectangle {
            Layout.fillWidth: true
            visible: section.confirmRestore
            implicitHeight: restoreBox.implicitHeight + 16
            radius: 4
            color: section.pal.bg_card
            border.color: section.pal.border
            border.width: 1

            ColumnLayout {
                id: restoreBox
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    text: section.t.restoreWarning || ""
                    color: section.pal.text_primary
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    spacing: 8

                    RexonButton {
                        objectName: "axonRestoreConfirm"
                        pal: section.pal
                        highlighted: true
                        text: section.t.restoreConfirm || ""
                        onClicked: {
                            section.confirmRestore = false;
                            actions.restoreOriginals();
                            section.reload();
                        }
                    }

                    RexonButton {
                        pal: section.pal
                        text: section.t.cancel || ""
                        onClicked: section.confirmRestore = false
                    }
                }
            }
        }
    }
}
