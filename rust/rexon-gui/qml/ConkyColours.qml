// Палитра цветов виджета Conky в карточке раздела «Виджеты Conky»: все
// цвета, найденные в файлах виджета (настройки, текст, графики, сценарии
// Lua), кружками со счётчиком мест. Нажатие — стандартный выбор цвета, и
// цвет меняется во всех его местах сразу; «Вернуть цвета» — файлы как до
// первой смены.
//
// Только разметка: найденное, подписи и запись — ядро
// (rexon_core::conky_colors) через объект действий страницы
// (`conkyColours`, `setConkyColour`, `revertConkyColours` в
// rust/rexon-gui/src/appearance_actions.rs). Снимок для плитки ядро
// переснимает само — карточка узнаёт о нём сигналом `conkyPreviewsReady`.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: box

    property var pal: null
    property var actions: null
    property string itemId: ""
    property bool busy: false

    property var view: ({})
    readonly property var t: box.view.t || ({})
    /// Цвет, который сейчас меняется (`#rrggbb`).
    property string picking: ""

    readonly property color colText: box.pal ? box.pal.text_primary : "#ffffff"
    readonly property color colHint: box.pal ? box.pal.text_secondary : "#8f98a0"
    readonly property color colAccent: box.pal ? box.pal.accent : "#3daee9"

    spacing: 6

    function reload() {
        if (!box.actions || !box.actions.conkyColours || box.itemId === "") {
            box.view = ({});
            return;
        }
        try {
            box.view = JSON.parse(box.actions.conkyColours(box.itemId));
        } catch (e) {
            box.view = ({});
        }
    }

    function change(from, to) {
        if (!box.actions || !box.actions.setConkyColour)
            return;
        try {
            box.view = JSON.parse(box.actions.setConkyColour(box.itemId, from, to));
        } catch (e) {
            box.reload();
        }
    }

    onItemIdChanged: reload()
    Component.onCompleted: reload()

    ColorDialog {
        id: dialog
        title: box.t.pick_title || ""
        onAccepted: {
            const to = dialog.selectedColor.toString().substring(0, 7).toLowerCase();
            if (box.picking !== "" && to !== box.picking)
                box.change(box.picking, to);
            box.picking = "";
        }
        onRejected: box.picking = ""
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: 4
        visible: box.view.available === true
        RexonText {
            Layout.fillWidth: true
            text: box.t.title || ""
            color: box.colHint
            font.bold: true
        }
        RexonButton {
            objectName: "conkyColoursRevert"
            pal: box.pal
            visible: box.view.can_revert === true
            enabled: !box.busy
            text: box.t.revert || ""
            ToolTip.text: box.t.revert_tooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onClicked: {
                if (!box.actions || !box.actions.revertConkyColours)
                    return;
                try {
                    box.view = JSON.parse(box.actions.revertConkyColours(box.itemId));
                } catch (e) {
                    box.reload();
                }
            }
        }
    }

    RexonText {
        Layout.fillWidth: true
        visible: box.view.available === true && (box.view.colours || []).length > 0
        text: box.t.hint || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }

    Flow {
        objectName: "conkyColoursFlow"
        Layout.fillWidth: true
        visible: box.view.available === true
        spacing: 6
        Repeater {
            model: box.view.colours || []
            delegate: AbstractButton {
                id: chip
                required property var modelData
                objectName: "conkyColour_" + chip.modelData.hex.substring(1)
                implicitWidth: 64
                implicitHeight: 62
                enabled: !box.busy
                focusPolicy: Qt.StrongFocus
                hoverEnabled: true
                background: Item {}
                contentItem: ColumnLayout {
                    spacing: 2
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 40
                        radius: 20
                        color: chip.modelData.hex
                        border.color: chip.activeFocus || chip.hovered ? box.colAccent : box.colHint
                        border.width: chip.activeFocus ? 3 : (chip.hovered ? 2 : 1)
                    }
                    RexonText {
                        Layout.alignment: Qt.AlignHCenter
                        text: chip.modelData.count_label
                        color: box.colHint
                        font.pointSize: 9
                    }
                }
                ToolTip.text: chip.modelData.tip
                ToolTip.visible: chip.hovered
                ToolTip.delay: 400
                onClicked: {
                    box.picking = chip.modelData.hex;
                    dialog.selectedColor = chip.modelData.hex;
                    dialog.open();
                }
            }
        }
    }

    RexonText {
        Layout.fillWidth: true
        visible: box.view.available === true && (box.view.colours || []).length === 0
        text: box.t.empty || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }

    RexonText {
        Layout.fillWidth: true
        visible: box.view.available === true && (box.view.note || "") !== ""
        text: box.view.note || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }

    RexonText {
        Layout.fillWidth: true
        visible: box.view.available === true && (box.view.picture_note || "") !== ""
        text: box.view.picture_note || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }
}
