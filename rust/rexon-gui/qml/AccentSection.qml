// Блок «Цвет акцента» над сеткой «Цветовых схем» — как в KDE «Цвета»: из
// цветовой схемы, из обоев или свой (десять готовых цветов и выбор любого),
// «Вернуть как было».
//
// Только разметка: снимок, подписи, запись и объяснение, где цвет действует
// в этом окружении, — `AccentActions`
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
    // Схему могли применить в сетке ниже — подпись «где действует» зависит
    // от неё, поэтому снимок свежий при каждом показе. Перечитывать — после
    // обработчика: `visible` раздела на странице сам зависит от `st`, и новый
    // `st` посреди смены `visible` замыкал привязку в петлю.
    onVisibleChanged: if (section.visible) Qt.callLater(section.reload)

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    ColumnLayout {
        id: body
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: section.st.title || ""
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        // Способ: из схемы / из обоев / свой.
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

        // Готовые цвета и «Выбрать цвет…».
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

        // Цвет из обоев либо свой, которого нет среди готовых, — показать.
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

            Text {
                Layout.fillWidth: true
                text: section.st.color || ""
                color: section.pal.text_secondary
            }
        }

        Text {
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Text {
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
