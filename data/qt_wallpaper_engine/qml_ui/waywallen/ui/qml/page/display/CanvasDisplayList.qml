pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W

ColumnLayout {
    id: control

    required property var editor
    property bool busy: false

    spacing: 8

    function configureWidth(index, current, text) {
        const value = Math.round(Number(text));
        if (control.busy || !Number.isFinite(value) || value <= 0 || value === current)
            return;
        editor.setMemberWidth(index, value);
    }

    function configureHeight(index, current, text) {
        const value = Math.round(Number(text));
        if (control.busy || !Number.isFinite(value) || value <= 0 || value === current)
            return;
        editor.setMemberHeight(index, value);
    }

    function syncDisplays() {
        const rows = editor.members || [];
        let rebuild = displayModel.count !== rows.length;
        if (!rebuild) {
            for (let index = 0; index < rows.length; ++index) {
                if (displayModel.get(index).settingsKey !== rows[index].settingsKey) {
                    rebuild = true;
                    break;
                }
            }
        }
        if (rebuild) {
            displayModel.clear();
            for (const member of rows) {
                const minimum = member.minimumScaleTo || ({});
                displayModel.append({
                    settingsKey: String(member.settingsKey || ""),
                    label: String(member.label || member.settingsKey || ""),
                    scaleWidth: Math.max(1, Number(member.width || 1)),
                    scaleHeight: Math.max(1, Number(member.height || 1)),
                    minimumWidth: Number(minimum.width || 0),
                    minimumHeight: Number(minimum.height || 0),
                    aspectLocked: member.aspectLocked ?? true
                });
            }
            return;
        }
        for (let index = 0; index < rows.length; ++index) {
            const member = rows[index];
            const minimum = member.minimumScaleTo || ({});
            displayModel.set(index, {
                label: String(member.label || member.settingsKey || ""),
                scaleWidth: Math.max(1, Number(member.width || 1)),
                scaleHeight: Math.max(1, Number(member.height || 1)),
                minimumWidth: Number(minimum.width || 0),
                minimumHeight: Number(minimum.height || 0),
                aspectLocked: member.aspectLocked ?? true
            });
        }
    }

    Component.onCompleted: syncDisplays()

    Connections {
        target: control.editor
        function onMembersChanged() {
            control.syncDisplays();
        }
    }

    ListModel {
        id: displayModel
        dynamicRoles: true
    }

    MD.Divider {
        Layout.fillWidth: true
        Layout.topMargin: 4
        Layout.bottomMargin: 4
    }

    MD.Text {
        Layout.fillWidth: true
        Layout.topMargin: 4
        text: qsTr("Displays")
        typescale: MD.Token.typescale.title_small
        color: MD.Token.color.on_surface
    }

    Flow {
        id: displayFlow

        Layout.fillWidth: true
        spacing: 8

        Repeater {
            model: displayModel

            delegate: MD.Pane {
                id: displayPanel

                required property int index
                required property string settingsKey
                required property string label
                required property real scaleWidth
                required property real scaleHeight
                required property real minimumWidth
                required property real minimumHeight
                required property bool aspectLocked

                width: displayFlow.width > 0 ? Math.min(implicitWidth, displayFlow.width) : implicitWidth
                padding: 12
                radius: 12
                backgroundColor: MD.Token.color.surface_container

                contentItem: MD.Column {
                    spacing: 8

                    MD.Row {
                        MD.Layout.fillWidth: true
                        spacing: 8
                        alignment: Qt.AlignVCenter

                        MD.Text {
                            MD.Layout.fillWidth: true
                            text: displayPanel.label
                            typescale: MD.Token.typescale.body_large
                            color: MD.Token.color.on_surface
                            elide: Text.ElideRight
                            maximumLineCount: 1
                        }

                        W.Tag {
                            text: displayPanel.minimumWidth > 0 && displayPanel.minimumHeight > 0
                                ? qsTr("Original %1 × %2").arg(displayPanel.minimumWidth).arg(displayPanel.minimumHeight)
                                : qsTr("Offline")
                            bgColor: MD.Token.color.surface_container_highest
                            fgColor: MD.Token.color.on_surface_variant
                        }
                    }

                    MD.Row {
                        spacing: 6
                        alignment: Qt.AlignVCenter

                        MD.TextField {
                            id: widthField

                            property string draftText: String(displayPanel.scaleWidth)

                            enabled: !control.busy
                            mdState.size: MD.Enum.XS
                            placeholderText: qsTr("Width")
                            horizontalAlignment: TextInput.AlignHCenter
                            inputMethodHints: Qt.ImhDigitsOnly
                            maximumLength: 5
                            validator: IntValidator {
                                bottom: 1
                                top: 99999
                            }
                            text: activeFocus ? draftText : String(displayPanel.scaleWidth)
                            onActiveFocusChanged: {
                                if (activeFocus)
                                    draftText = String(displayPanel.scaleWidth);
                            }
                            onTextEdited: {
                                draftText = text;
                            }
                            onEditingFinished: control.configureWidth(displayPanel.index, displayPanel.scaleWidth, draftText)
                        }

                        MD.Text {
                            id: dimensionSeparator

                            text: "×"
                            typescale: MD.Token.typescale.body_large
                            color: MD.Token.color.on_surface_variant
                        }

                        MD.TextField {
                            id: heightField

                            property string draftText: String(displayPanel.scaleHeight)

                            enabled: !control.busy
                            mdState.size: MD.Enum.XS
                            placeholderText: qsTr("Height")
                            horizontalAlignment: TextInput.AlignHCenter
                            inputMethodHints: Qt.ImhDigitsOnly
                            maximumLength: 5
                            validator: IntValidator {
                                bottom: 1
                                top: 99999
                            }
                            text: activeFocus ? draftText : String(displayPanel.scaleHeight)
                            onActiveFocusChanged: {
                                if (activeFocus)
                                    draftText = String(displayPanel.scaleHeight);
                            }
                            onTextEdited: {
                                draftText = text;
                            }
                            onEditingFinished: control.configureHeight(displayPanel.index, displayPanel.scaleHeight, draftText)
                        }

                        MD.IconToggleButton {
                            id: lockButton

                            enabled: !control.busy
                            checked: displayPanel.aspectLocked
                            mdState.size: MD.Enum.XS
                            icon.name: displayPanel.aspectLocked ? MD.Token.icon.lock : MD.Token.icon.lock_open
                            MD.ToolTip.visible: mdState.interactions.hovered
                            MD.ToolTip.text: displayPanel.aspectLocked ? qsTr("Unlock aspect ratio") : qsTr("Lock aspect ratio")
                            onCheckedChangeRequested: checked => control.editor.setMemberAspectLocked(displayPanel.index, checked)
                        }

                        MD.IconButton {
                            id: resetButton

                            enabled: !control.busy && displayPanel.minimumWidth > 0 && displayPanel.minimumHeight > 0 && (displayPanel.scaleWidth !== displayPanel.minimumWidth || displayPanel.scaleHeight !== displayPanel.minimumHeight)
                            mdState.size: MD.Enum.XS
                            icon.name: MD.Token.icon.settings_backup_restore
                            MD.ToolTip.visible: mdState.interactions.hovered
                            MD.ToolTip.text: qsTr("Reset to original size")
                            onClicked: control.editor.resetMemberSize(displayPanel.index)
                        }
                    }
                }
            }
        }
    }
}
