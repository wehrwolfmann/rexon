// Наборы виджетов Conky в разделе «Виджеты Conky»: несколько виджетов и
// обои — одной кнопкой «Применить»; «Сохранить запущенные как набор».
//
// Только разметка: список, подписи и действия — объект действий страницы
// (`conkySets`, `saveConkySet`, `applyConkySet`, `removeConkySet` в
// rust/rexon-gui/src/appearance_actions.rs; решения — rexon_core::conky_sets).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: box

    property var pal: null
    property var actions: null
    property bool busy: false

    property var view: ({})
    /// Нажато «Применить» — итог придёт сигналом `actionDone` из фона.
    property bool applying: false
    readonly property var t: box.view.texts || ({})
    readonly property var rows: box.view.rows || []

    readonly property color colText: box.pal ? box.pal.text_primary : "#ffffff"
    readonly property color colHint: box.pal ? box.pal.text_secondary : "#8f98a0"
    readonly property color colCard: box.pal ? box.pal.bg_card : "#1b2838"

    /// Что-то применили или сохранили — сетке пора перечитать запущенные.
    signal changed()

    spacing: 6

    function reload() {
        if (!box.actions || !box.actions.conkySets) {
            box.view = ({});
            return;
        }
        try {
            box.view = JSON.parse(box.actions.conkySets());
        } catch (e) {
            box.view = ({});
        }
    }

    Component.onCompleted: reload()

    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Text {
            text: box.t.title || ""
            color: box.colText
            font.bold: true
        }
        Text {
            Layout.fillWidth: true
            text: box.t.hint || ""
            color: box.colHint
            elide: Text.ElideRight
        }
        RexonTextField {
            id: nameField
            objectName: "conkySetName"
            pal: box.pal
            Layout.preferredWidth: 180
            placeholderText: box.t.name_placeholder || ""
            onAccepted: saveButton.clicked()
        }
        RexonButton {
            id: saveButton
            objectName: "conkySetSave"
            pal: box.pal
            enabled: !box.busy && nameField.text.trim() !== ""
            text: box.t.save || ""
            ToolTip.text: box.t.save_tooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onClicked: {
                if (!box.actions || !box.actions.saveConkySet)
                    return;
                if (box.actions.saveConkySet(nameField.text)) {
                    nameField.text = "";
                    box.changed();
                }
                box.reload();
            }
        }
    }

    Text {
        Layout.fillWidth: true
        visible: box.rows.length === 0
        text: box.t.empty || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }

    Flow {
        Layout.fillWidth: true
        spacing: 8
        Repeater {
            model: box.rows
            delegate: Rectangle {
                id: chip
                required property var modelData
                objectName: "conkySet_" + modelData.name
                color: box.colCard
                radius: 4
                implicitWidth: chipRow.implicitWidth + 16
                implicitHeight: chipRow.implicitHeight + 10
                ToolTip.text: chip.modelData.tooltip
                ToolTip.visible: chipHover.hovered && chip.modelData.tooltip !== ""
                ToolTip.delay: 600
                HoverHandler { id: chipHover }

                RowLayout {
                    id: chipRow
                    anchors.centerIn: parent
                    spacing: 8
                    ColumnLayout {
                        spacing: 0
                        Text {
                            text: chip.modelData.name
                                  + (chip.modelData.applied ? "  " + (box.t.applied || "") : "")
                            color: box.colText
                            font.bold: true
                        }
                        Text {
                            Layout.maximumWidth: 260
                            text: chip.modelData.summary
                            color: box.colHint
                            elide: Text.ElideRight
                        }
                    }
                    RexonButton {
                        objectName: "conkySetApply"
                        pal: box.pal
                        enabled: !box.busy && !chip.modelData.empty
                        text: box.t.apply || ""
                        onClicked: {
                            if (box.actions && box.actions.applyConkySet) {
                                box.applying = true;
                                box.actions.applyConkySet(chip.modelData.name);
                            }
                        }
                    }
                    RexonButton {
                        objectName: "conkySetRemove"
                        pal: box.pal
                        visible: chip.modelData.own
                        enabled: !box.busy
                        text: box.t.remove || ""
                        onClicked: {
                            if (box.actions && box.actions.removeConkySet)
                                box.actions.removeConkySet(chip.modelData.name);
                            box.reload();
                        }
                    }
                }
            }
        }
    }

    Connections {
        target: box.actions
        ignoreUnknownSignals: true
        function onActionDone(text, ok) {
            box.reload();
            if (box.applying) {
                box.applying = false;
                box.changed();
            }
        }
    }
}
