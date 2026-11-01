// Раздел «Панель MATE»: готовые раскладки панели одной кнопкой («Как
// Windows», классическая, как в Mint…), отметка текущей, возврат прежней
// панели. Показывается только в сеансе MATE и только если mate-panel есть.
//
// Раздел самостоятельный — свой объект действий, своё окно подтверждения,
// снаружи нужны только `pal`, `sty` и обработчик `message`. Стоит во
// вкладке «Оформление», разделом «Панель MATE» (экран `matepanel`).
//
// Только разметка: снимок, подписи, применение и возврат — `MatePanelActions`
// (rust/rexon-gui/src/mate_panel_actions.rs → rexon_core::mate_panel).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    signal message(string text)

    property var st: ({})

    /// Выбранная в списке раскладка (по умолчанию — текущая).
    property string selected: ""

    function reload() {
        section.st = JSON.parse(actions.state());
        const layouts = section.st.layouts || [];
        if (!layouts.some(l => l.id === section.selected)) {
            const current = layouts.find(l => l.current);
            section.selected = current ? current.id : (layouts.length ? layouts[0].id : "");
        }
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    /// Идёт применение в фоне (ставятся части раскладки) — кнопки заперты.
    property bool busy: false

    /// Спросить подтверждение: `id` — раскладка, пусто — возврат прежней панели.
    function ask(id) {
        const texts = JSON.parse(actions.confirm(id));
        confirmDialog.parts = false;
        confirmDialog.layoutId = id;
        confirmDialog.heading = texts.title;
        confirmDialog.question = texts.question;
        confirmDialog.open();
    }

    /// Применить раскладку: не хватает частей, которые можно поставить, —
    /// сперва спросить об этом («Нет» — применить с заменой, как раньше).
    function applyLayout(id) {
        let q = {};
        try {
            q = JSON.parse(actions.partsQuestion(id));
        } catch (e) {
            q = {};
        }
        if (q.title) {
            confirmDialog.parts = true;
            confirmDialog.layoutId = id;
            confirmDialog.heading = q.title;
            confirmDialog.question = q.question;
            confirmDialog.open();
            return;
        }
        section.start(id, false);
    }

    function start(id, install) {
        section.busy = true;
        if (install)
            section.message(qsTranslate("rexon", "Installing the missing parts of the layout — a build from AUR can take a few minutes…"));
        actions.applyAsync(id, install);
    }

    MatePanelActions {
        id: actions
        objectName: "matePanelActions"
        onApplied: function(text) {
            section.busy = false;
            section.did(text);
        }
        onRestored: function(text) {
            section.busy = false;
            section.did(text);
        }
    }

    Component.onCompleted: section.reload()

    visible: section.st.available === true
    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: section.visible ? body.implicitHeight + 2 * (section.sty.group.marginV + border.width) : 0

    ColumnLayout {
        id: body
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "MATE panel")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Ready-made panel layouts: pick one and press “Apply”. The panel changes right away, without logging out.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Repeater {
            model: section.st.layouts || []

            delegate: Rectangle {
                id: row
                required property var modelData
                readonly property bool chosen: row.modelData.id === section.selected

                objectName: "matePanelLayout_" + row.modelData.id
                Layout.fillWidth: true
                implicitHeight: rowColumn.implicitHeight + 16
                radius: 6
                color: rowMouse.containsMouse ? section.pal.bg_card_hover : section.pal.bg_card
                border.width: row.chosen || row.activeFocus ? 2 : 1
                border.color: row.chosen || row.activeFocus ? section.pal.accent : section.pal.border
                activeFocusOnTab: true

                Keys.onReturnPressed: section.selected = row.modelData.id
                Keys.onSpacePressed: section.selected = row.modelData.id

                MouseArea {
                    id: rowMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        section.selected = row.modelData.id;
                        row.forceActiveFocus();
                    }
                }

                ColumnLayout {
                    id: rowColumn
                    x: 12
                    y: 8
                    width: parent.width - 24
                    spacing: 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Text {
                            text: (row.chosen ? "◉ " : "○ ") + row.modelData.title
                            color: section.pal.text_primary
                            font.bold: true
                        }

                        Text {
                            visible: row.modelData.current === true
                            text: qsTranslate("rexon", "(current)")
                            color: section.pal.accent
                        }

                        Item { Layout.fillWidth: true }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: row.modelData.description
                        color: section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: row.modelData.notes || []

                        delegate: Text {
                            required property string modelData
                            Layout.fillWidth: true
                            text: "• " + modelData
                            color: section.pal.state_warn
                            wrapMode: Text.WordWrap
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            RexonButton {
                id: applyButton
                objectName: "matePanelApply"
                pal: section.pal
                highlighted: true
                enabled: section.selected !== "" && !section.busy
                text: qsTranslate("rexon", "Apply")
                onClicked: section.ask(section.selected)
            }

            RexonButton {
                id: restoreButton
                objectName: "matePanelRestore"
                visible: section.st.can_restore === true
                enabled: !section.busy
                pal: section.pal
                text: qsTranslate("rexon", "Restore previous panel")
                onClicked: section.ask("")
            }

            Item { Layout.fillWidth: true }
        }
    }

    // ── Подтверждение ─────────────────────────────────────────────────────
    //
    // Внутри окна, а не окно системы (канон Big Picture); фокус при
    // открытии — на «Нет»: случайный Enter панель не заменит.

    Popup {
        id: confirmDialog

        property string layoutId: ""
        /// Второй вопрос — о недостающих частях: «Нет» применяет с заменой.
        property bool parts: false
        property string heading: ""
        property string question: ""

        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        width: Math.min((parent ? parent.width : 560) - 40, 560)
        padding: 16

        background: Rectangle {
            color: section.pal.bg_secondary
            border.color: section.pal.border
            border.width: 1
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: section.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            Text {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                text: confirmDialog.question
                color: section.pal.text_primary
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: confirmYes
                    objectName: "matePanelConfirmYes"
                    pal: section.pal
                    highlighted: true
                    text: qsTranslate("rexon", "Yes")
                    onClicked: {
                        const id = confirmDialog.layoutId;
                        const parts = confirmDialog.parts;
                        confirmDialog.close();
                        if (parts)
                            section.start(id, true);
                        else if (id === "") {
                            section.busy = true;
                            actions.restoreAsync();
                        }
                        else
                            section.applyLayout(id);
                    }
                }

                RexonButton {
                    id: confirmNo
                    objectName: "matePanelConfirmNo"
                    pal: section.pal
                    text: qsTranslate("rexon", "No")
                    onClicked: {
                        const id = confirmDialog.layoutId;
                        const parts = confirmDialog.parts;
                        confirmDialog.close();
                        if (parts)
                            section.start(id, false);
                    }
                }
            }
        }

        onOpened: confirmNo.forceActiveFocus()
    }
}
