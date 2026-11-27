// The "Accent colour" block above the "Colour schemes" grid — as in KDE
// "Colours": from the colour scheme, from the wallpaper or a custom one
// (ten ready colours and a picker for any), "Restore as it was".
//
// Markup only: the snapshot, labels, writing and the explanation of where
// the colour applies in this environment — `AccentActions`
// (rust/rexon-gui/src/accent_actions.rs → rexon_core::accent).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    property var st: ({})
    property string message: ""
    property bool failed: false

    function reload() {
        try {
            section.st = JSON.parse(actions.state());
        } catch (e) {
            section.st = ({});
        }
    }

    function did(payload) {
        let r = ({});
        try {
            r = JSON.parse(payload);
        } catch (e) {
            r = ({});
        }
        section.failed = r.ok !== true;
        section.message = r.message || "";
        section.reload();
    }

    AccentActions {
        id: actions
        objectName: "accentActions"
    }

    ColorDialog {
        id: colorDialog
        title: section.st.pick_title || ""
        selectedColor: section.st.custom || section.st.color || "#3daee9"
        onAccepted: section.did(actions.apply(colorDialog.selectedColor.toString().substring(0, 7)))
    }

    Component.onCompleted: section.reload()
    // A scheme may have been applied in the grid below — the "where it
    // applies" label depends on it, so the snapshot is fresh on every show.
    // Re-read after the handler: the section's `visible` on the page itself
    // depends on `st`, and a new `st` in the middle of a `visible` change
    // turned the binding into a loop.
    onVisibleChanged: if (section.visible) Qt.callLater(section.reload)

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
            text: section.st.title || ""
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        // Mode: from the scheme / from the wallpaper / custom.
        Flow {
            Layout.fillWidth: true
            spacing: 8

            Repeater {
                model: section.st.modes || []

                delegate: RexonButton {
                    id: modeButton
                    required property var modelData
                    objectName: "accentMode_" + modeButton.modelData.id
                    pal: section.pal
                    checkable: true
                    checked: section.st.mode === modeButton.modelData.id
                    font.bold: checked
                    text: modeButton.modelData.label
                    onClicked: {
                        const id = modeButton.modelData.id;
                        if (id === "custom")
                            section.did(actions.apply(section.st.custom || (section.st.presets || ["#3daee9"])[0]));
                        else
                            section.did(actions.apply(id));
                    }
                }
            }
        }

        // Ready colours and "Choose colour…".
        Flow {
            Layout.fillWidth: true
            spacing: 8

            Repeater {
                model: section.st.presets || []

                delegate: Rectangle {
                    id: swatch
                    required property var modelData
                    objectName: "accentPreset_" + swatch.modelData.substring(1)
                    readonly property bool current: section.st.mode === "custom"
                                                    && section.st.color === swatch.modelData
                    width: 40
                    height: 40
                    radius: 20
                    color: swatch.modelData
                    border.width: swatch.current ? 3 : (swatchArea.containsMouse ? 2 : 1)
                    border.color: swatch.current || swatchArea.containsMouse
                                  ? section.pal.text_primary : section.sty.group.border

                    MouseArea {
                        id: swatchArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: section.did(actions.apply(swatch.modelData))
                    }
                }
            }

            RexonButton {
                objectName: "accentPick"
                pal: section.pal
                text: section.st.pick_label || ""
                onClicked: colorDialog.open()
            }
        }

        // A colour from the wallpaper or a custom one that is not among the ready
        // ones — show it.
        RowLayout {
            Layout.fillWidth: true
            visible: (section.st.color || "") !== ""
                     && (section.st.mode === "wallpaper"
                         || (section.st.presets || []).indexOf(section.st.color) < 0)
            spacing: 8

            Rectangle {
                Layout.preferredWidth: 24
                Layout.preferredHeight: 24
                radius: 12
                color: section.st.color || "transparent"
                border.width: 1
                border.color: section.sty.group.border
            }

            RexonText {
                Layout.fillWidth: true
                text: section.st.color || ""
                color: section.pal.text_secondary
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.message !== ""
            text: section.message
            color: section.failed && section.pal.state_warn !== undefined ? section.pal.state_warn
                                                                           : section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            spacing: 8

            RexonButton {
                objectName: "accentRevert"
                visible: section.st.can_revert === true
                pal: section.pal
                text: section.st.revert_label || ""
                onClicked: section.did(actions.revert())
            }

            RexonButton {
                objectName: "accentOpenNative"
                visible: (section.st.open_label || "") !== ""
                pal: section.pal
                text: section.st.open_label || ""
                onClicked: actions.openNative()
            }
        }
    }
}
