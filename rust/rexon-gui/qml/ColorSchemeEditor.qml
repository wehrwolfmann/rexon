// Свой редактор цветовой схемы «Оформления» — когда редактора KDE
// (kcolorschemeeditor) в системе нет. Основные роли — окно, область
// просмотра, кнопка, выделение, подсказка, заголовок окна: фон и текст.
//
// Только разметка: роли, подписи и запись схемы — ядро
// (rexon_core::color_scheme_edit) через AppearanceModel
// (`editDetailScheme` даёт состояние, `saveSchemeCopy` пишет файл).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Popup {
    id: editor

    property var pal: ({})
    /// Подписи (`color_scheme_edit::EditorTexts`).
    property var texts: ({})

    /// Роли `[{key, label, background, foreground}]`.
    property var roles: []
    property string base: ""
    property string overwrite: ""
    property string error: ""

    signal saveRequested(string name, string roles, string base, string overwrite)

    function show(state) {
        editor.roles = state.roles || [];
        editor.base = state.base || "";
        editor.overwrite = state.overwrite || "";
        editor.error = "";
        nameField.text = state.name || "";
        editor.open();
    }

    /// Ответ записи: при успехе окно закрывается, иначе показывает причину.
    function done(ok, message) {
        if (ok) {
            editor.close();
        } else {
            editor.error = message;
        }
    }

    function setColor(index, field, value) {
        const copy = editor.roles.slice();
        const row = Object.assign({}, copy[index]);
        row[field] = value;
        copy[index] = row;
        editor.roles = copy;
    }

    width: Math.min(parent ? parent.width - 40 : 560, 560)
    modal: true
    focus: true
    padding: 16
    closePolicy: Popup.CloseOnEscape

    Overlay.modal: Rectangle { color: editor.pal.modal_dim || "#80000000" }
    background: Rectangle {
        color: editor.pal.bg_primary || "#1b2838"
        border.color: editor.pal.border || "#2a4055"
        radius: 4
    }

    ColorDialog {
        id: picker
        property int row: -1
        property string field: ""
        title: editor.texts.title || ""
        onAccepted: editor.setColor(picker.row, picker.field,
                                    picker.selectedColor.toString().substring(0, 7))
    }

    ColumnLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 10

        Text {
            Layout.fillWidth: true
            text: editor.texts.title || ""
            color: editor.pal.text_primary || "#ffffff"
            font.bold: true
            font.pointSize: 13
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Text {
                text: editor.texts.name_label || ""
                color: editor.pal.text_primary || "#ffffff"
            }

            RexonTextField {
                id: nameField
                objectName: "schemeNameField"
                Layout.fillWidth: true
                pal: editor.pal
                onTextChanged: editor.error = ""
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 12

            Item { Layout.fillWidth: true; implicitHeight: 1 }
            Text {
                Layout.preferredWidth: 72
                horizontalAlignment: Text.AlignHCenter
                text: editor.texts.background || ""
                color: editor.pal.text_secondary || "#c7d5e0"
                elide: Text.ElideRight
            }
            Text {
                Layout.preferredWidth: 72
                horizontalAlignment: Text.AlignHCenter
                text: editor.texts.foreground || ""
                color: editor.pal.text_secondary || "#c7d5e0"
                elide: Text.ElideRight
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 6

            Repeater {
                model: editor.roles
                delegate: Item {
                    id: roleRow
                    required property var modelData
                    required property int index
                    // Образец роли и две кнопки цвета.
                    Layout.fillWidth: true
                    implicitHeight: 40

                    RowLayout {
                        anchors.fill: parent
                        spacing: 12

                        // Образец: текст роли на её фоне.
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            radius: 3
                            color: roleRow.modelData.background
                            border.color: editor.pal.border || "#2a4055"
                            Text {
                                anchors.fill: parent
                                anchors.leftMargin: 10
                                verticalAlignment: Text.AlignVCenter
                                text: roleRow.modelData.label
                                color: roleRow.modelData.foreground
                                elide: Text.ElideRight
                            }
                        }

                        Repeater {
                            model: ["background", "foreground"]
                            delegate: Button {
                                id: swatch
                                required property string modelData
                                objectName: "swatch_" + roleRow.modelData.key + "_" + modelData
                                implicitWidth: 72
                                implicitHeight: 40
                                ToolTip.text: roleRow.modelData[swatch.modelData]
                                ToolTip.visible: hovered
                                ToolTip.delay: 400
                                background: Rectangle {
                                    radius: 3
                                    color: roleRow.modelData[swatch.modelData]
                                    border.color: swatch.activeFocus ? (editor.pal.accent || "#66c0f4")
                                                                     : (editor.pal.border || "#2a4055")
                                    border.width: swatch.activeFocus ? 2 : 1
                                }
                                onClicked: {
                                    picker.row = roleRow.index;
                                    picker.field = swatch.modelData;
                                    picker.selectedColor = roleRow.modelData[swatch.modelData];
                                    picker.open();
                                }
                            }
                        }
                    }
                }
            }
        }

        Text {
            Layout.fillWidth: true
            text: editor.error !== "" ? editor.error : (editor.texts.hint || "")
            color: editor.error !== "" ? (editor.pal.rating_star || "#ffcc00")
                                       : (editor.pal.text_secondary || "#c7d5e0")
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8

            RexonButton {
                id: saveButton
                objectName: "schemeSaveButton"
                pal: editor.pal
                text: editor.texts.save || ""
                enabled: nameField.text.trim() !== ""
                onClicked: editor.saveRequested(nameField.text, JSON.stringify(editor.roles),
                                                editor.base, editor.overwrite)
            }

            RexonButton {
                pal: editor.pal
                text: editor.texts.cancel || ""
                onClicked: editor.close()
            }
        }
    }

    onOpened: nameField.forceActiveFocus()
}
