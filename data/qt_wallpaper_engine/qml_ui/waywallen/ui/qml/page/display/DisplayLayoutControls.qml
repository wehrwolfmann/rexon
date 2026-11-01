pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W

ColumnLayout {
    id: control

    required property var displayLayout
    required property bool wallpaperOverride
    required property bool resetEnabled
    required property var fillModeValues
    required property var fillModeLabels
    required property var rotationValues
    required property var rotationLabels

    readonly property int currentX: clampPercent(displayLayout.locationX ?? 50)
    readonly property int currentY: clampPercent(displayLayout.locationY ?? 50)
    readonly property bool locationEnabled: (displayLayout.fillmode || 0) !== 1

    signal fillModeRequested(int value)
    signal locationRequested(real x, real y)
    signal rotationRequested(int value)
    signal flipRequested(int value)
    signal resetRequested

    spacing: 8

    function clampPercent(value) {
        return Math.max(0, Math.min(100, Math.round(Number(value) || 0)));
    }

    function fillModeIndex(value) {
        const index = fillModeValues.indexOf(value);
        return index < 0 ? 0 : index;
    }

    MD.Divider {
        Layout.fillWidth: true
        Layout.topMargin: 8
        Layout.bottomMargin: 4
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        MD.Text {
            Layout.fillWidth: true
            text: qsTr("Layout")
            typescale: MD.Token.typescale.title_small
            color: MD.Token.color.on_surface
        }

        MD.AssistChip {
            visible: control.wallpaperOverride
            text: qsTr("Wallpaper override")
        }

        Item {
            implicitWidth: children[0].implicitWidth

            MD.IconButton {
                anchors.verticalCenter: parent.verticalCenter
                mdState.size: MD.Enum.XS
                enabled: control.resetEnabled
                icon.name: MD.Token.icon.settings_backup_restore
                MD.ToolTip.visible: mdState.interactions.hovered
                MD.ToolTip.text: qsTr("Revert to global default")
                onClicked: control.resetRequested()
            }
        }
    }

    Flow {
        id: layoutFlow

        Layout.fillWidth: true
        spacing: 12

        ColumnLayout {
            width: Math.min(layoutFlow.width, 220)
            spacing: 4

            MD.Text {
                text: qsTr("Fill mode")
                typescale: MD.Token.typescale.label_medium
                color: MD.Token.color.on_surface_variant
            }

            MD.ComboBox {
                Layout.fillWidth: true
                mdState.size: MD.Enum.S
                model: control.fillModeLabels
                currentIndex: control.fillModeIndex(control.displayLayout.fillmode || 0)
                onSelectionChangeRequested: index => control.fillModeRequested(control.fillModeValues[index])
            }
        }

        ColumnLayout {
            width: Math.min(layoutFlow.width, 260)
            spacing: 4
            enabled: control.locationEnabled
            opacity: enabled ? 1.0 : 0.4

            MD.Text {
                text: qsTr("Horizontal")
                typescale: MD.Token.typescale.label_medium
                color: MD.Token.color.on_surface_variant
            }

            W.ValueSlider {
                id: horizontalLocation

                Layout.fillWidth: true
                from: 0
                to: 100
                stepSize: 1
                value: control.currentX
                valueText: control.clampPercent(value)
                valueMaxText: control.clampPercent(to).toString()
                valueHorizontalAlignment: Text.AlignLeft
                onValueChangeRequested: value => control.locationRequested(value, verticalLocation.value)
            }
        }

        ColumnLayout {
            width: Math.min(layoutFlow.width, 260)
            spacing: 4
            enabled: control.locationEnabled
            opacity: enabled ? 1.0 : 0.4

            MD.Text {
                text: qsTr("Vertical")
                typescale: MD.Token.typescale.label_medium
                color: MD.Token.color.on_surface_variant
            }

            W.ValueSlider {
                id: verticalLocation

                Layout.fillWidth: true
                from: 0
                to: 100
                stepSize: 1
                value: control.currentY
                valueText: control.clampPercent(value)
                valueMaxText: control.clampPercent(to).toString()
                valueHorizontalAlignment: Text.AlignLeft
                onValueChangeRequested: value => control.locationRequested(horizontalLocation.value, value)
            }
        }

        ColumnLayout {
            width: Math.min(layoutFlow.width, implicitWidth)
            spacing: 4

            MD.Text {
                text: qsTr("Rotation")
                typescale: MD.Token.typescale.label_medium
                color: MD.Token.color.on_surface_variant
            }

            MD.SegmentedButtonGroup {
                id: rotationGroup

                size: MD.Enum.XS

                // A Repeater becomes an extra content-model slot and shifts the first button.
                function applyRotation(rotationValue) {
                    control.rotationRequested(rotationValue);
                }
                function isChecked(rotationValue) {
                    return (control.displayLayout.rotation || 0) === rotationValue;
                }

                MD.SegmentedButton {
                    text: control.rotationLabels[0]
                    checked: rotationGroup.isChecked(control.rotationValues[0])
                    onCheckedChangeRequested: rotationGroup.applyRotation(control.rotationValues[0])
                }
                MD.SegmentedButton {
                    text: control.rotationLabels[1]
                    checked: rotationGroup.isChecked(control.rotationValues[1])
                    onCheckedChangeRequested: rotationGroup.applyRotation(control.rotationValues[1])
                }
                MD.SegmentedButton {
                    text: control.rotationLabels[2]
                    checked: rotationGroup.isChecked(control.rotationValues[2])
                    onCheckedChangeRequested: rotationGroup.applyRotation(control.rotationValues[2])
                }
                MD.SegmentedButton {
                    text: control.rotationLabels[3]
                    checked: rotationGroup.isChecked(control.rotationValues[3])
                    onCheckedChangeRequested: rotationGroup.applyRotation(control.rotationValues[3])
                }
            }

        }

        ColumnLayout {
            width: Math.min(layoutFlow.width, implicitWidth)
            spacing: 4

            MD.Text {
                text: qsTr("Flip")
                typescale: MD.Token.typescale.label_medium
                color: MD.Token.color.on_surface_variant
            }
            W.FlipButtons {
                value: control.displayLayout.flip || 1
                onSelected: value => control.flipRequested(value)
            }
        }
    }
}
