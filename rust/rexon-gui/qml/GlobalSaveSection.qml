// «Сохранить текущее оформление…» над сеткой «Глобальных тем» — как кнопка
// модуля KDE «Глобальная тема»: то, что стоит на столе сейчас, становится
// своей глобальной темой (имя, автор, что сохранить, превью — снимок
// экрана, свой рисунок или плитка цветами темы).
//
// Только разметка: снимок, подписи, проверка имени, пакет и превью —
// `GlobalSaveActions` (rust/rexon-gui/src/global_save_actions.rs →
// rexon_core::global_save). Сохранили — сигнал `saved`: страница
// перечитывает сетку, и плитка новой темы появляется сразу.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: section

    property var pal: ({})
    property var sty: ({})

    /// Тема сохранена: `message` — для строки состояния.
    signal saved(string message)

    property var st: ({})

    implicitHeight: openButton.implicitHeight

    GlobalSaveActions {
        id: actions
        objectName: "globalSaveActions"
        onScreenshotReady: function(path, error) {
            dialog.shooting = false;
            if (path !== "") {
                dialog.preview = path;
                dialog.message = "";
            } else {
                dialog.message = error;
            }
        }
    }

    Component.onCompleted: {
        try {
            section.st = JSON.parse(actions.state());
        } catch (e) {
            section.st = ({});
        }
    }

    RowLayout {
        anchors.right: parent.right
        spacing: 8

        RexonButton {
            id: openButton
            objectName: "globalSaveOpen"
            pal: section.pal
            text: section.st.button || ""
            ToolTip.text: section.st.buttonTooltip || ""
            ToolTip.visible: hovered && (section.st.buttonTooltip || "") !== ""
            ToolTip.delay: 600
            onClicked: dialog.start()
        }
    }

    FileDialog {
        id: imageDialog
        fileMode: FileDialog.OpenFile
        nameFilters: ["*.png *.jpg *.jpeg *.webp *.bmp"]
        onAccepted: {
            const url = selectedFile.toString();
            dialog.preview = decodeURIComponent(url.replace(/^file:\/\//, ""));
        }
    }

    Popup {
        id: dialog
        objectName: "globalSaveDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min((parent ? parent.width : 640) - 40, 600)
        height: Math.min((parent ? parent.height : 700) - 40, implicitHeight)
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape

        property var q: ({})
        /// Отмеченные части: id → true.
        property var picked: ({})
        /// Картинка превью; пусто — плитка `tile`.
        property string preview: ""
        property string tile: ""
        property string problem: ""
        property string message: ""
        property bool shooting: false

        readonly property bool anyPicked: (q.parts || []).some(p => p.enabled && picked[p.id] === true)

        function start() {
            try {
                q = JSON.parse(actions.state());
            } catch (e) {
                q = ({});
            }
            const m = ({});
            for (const p of (q.parts || []))
                m[p.id] = p.checked === true;
            picked = m;
            preview = "";
            tile = actions.tile();
            message = "";
            nameField.text = "";
            authorField.text = q.author || "";
            problem = actions.nameProblem("");
            open();
            nameField.forceActiveFocus();
        }

        function setPicked(id, on) {
            const m = Object.assign({}, picked);
            m[id] = on;
            picked = m;
        }

        function save() {
            const parts = (q.parts || []).filter(p => p.enabled && picked[p.id] === true).map(p => p.id);
            let r = ({});
            try {
                r = JSON.parse(actions.save(JSON.stringify({
                    name: nameField.text,
                    author: authorField.text,
                    parts: parts,
                    preview: preview
                })));
            } catch (e) {
                r = ({});
            }
            if (r.ok === true) {
                close();
                section.saved(r.message || "");
            } else {
                message = r.message || "";
            }
        }

        Overlay.modal: Rectangle { color: section.pal.modal_dim !== undefined ? section.pal.modal_dim : "#80000000" }
        background: Rectangle {
            color: section.pal.bg_secondary
            border.color: section.sty.group ? section.sty.group.border : "#2a4055"
            border.width: 1
            radius: 8
        }

        contentItem: ScrollView {
            id: scroll
            clip: true
            contentWidth: availableWidth

            ColumnLayout {
                width: scroll.availableWidth
                spacing: 10

                Text {
                    Layout.fillWidth: true
                    text: dialog.q.title || ""
                    color: section.pal.text_primary
                    font.bold: true
                    font.pointSize: 14
                    wrapMode: Text.WordWrap
                }

                Text {
                    Layout.fillWidth: true
                    text: dialog.q.intro || ""
                    color: section.pal.text_secondary
                    wrapMode: Text.WordWrap
                }

                GridLayout {
                    Layout.fillWidth: true
                    columns: 2
                    columnSpacing: 10
                    rowSpacing: 6

                    Text {
                        text: dialog.q.nameLabel || ""
                        color: section.pal.text_primary
                    }
                    RexonTextField {
                        id: nameField
                        objectName: "globalSaveName"
                        Layout.fillWidth: true
                        pal: section.pal
                        placeholderText: dialog.q.namePlaceholder || ""
                        onTextChanged: dialog.problem = actions.nameProblem(text)
                        onAccepted: if (dialog.problem === "" && dialog.anyPicked) dialog.save()
                    }

                    Item { implicitWidth: 1; implicitHeight: 1; visible: problemText.visible }
                    Text {
                        id: problemText
                        Layout.fillWidth: true
                        // Пустое имя — без предупреждения: поле и так ждёт ввода.
                        visible: dialog.problem !== "" && nameField.text.trim() !== ""
                        text: dialog.problem
                        color: section.pal.state_warn !== undefined ? section.pal.state_warn : section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }

                    Text {
                        text: dialog.q.authorLabel || ""
                        color: section.pal.text_primary
                    }
                    RexonTextField {
                        id: authorField
                        objectName: "globalSaveAuthor"
                        Layout.fillWidth: true
                        pal: section.pal
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: dialog.q.partsHeading || ""
                    color: section.pal.text_primary
                    font.bold: true
                }

                Repeater {
                    model: dialog.q.parts || []

                    delegate: ColumnLayout {
                        id: partRow
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 0

                        RexonCheckBox {
                            objectName: "globalSavePart_" + partRow.modelData.id
                            Layout.fillWidth: true
                            pal: section.pal
                            enabled: partRow.modelData.enabled === true
                            checked: dialog.picked[partRow.modelData.id] === true
                            text: partRow.modelData.text
                                  + ((partRow.modelData.value || "") !== "" ? " — " + partRow.modelData.value : "")
                            onToggled: dialog.setPicked(partRow.modelData.id, checked)
                        }
                        Text {
                            Layout.fillWidth: true
                            Layout.leftMargin: 32
                            visible: (partRow.modelData.reason || "") !== ""
                            text: partRow.modelData.reason || ""
                            color: section.pal.text_secondary
                            font.pointSize: 9
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: dialog.q.previewHeading || ""
                    color: section.pal.text_primary
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12

                    Rectangle {
                        Layout.preferredWidth: 240
                        Layout.preferredHeight: 136
                        color: "transparent"
                        border.width: 1
                        border.color: section.sty.group ? section.sty.group.border : "#2a4055"
                        radius: 4

                        Image {
                            objectName: "globalSavePreview"
                            anchors.fill: parent
                            anchors.margins: 1
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            cache: false
                            source: dialog.preview !== "" ? "file://" + dialog.preview
                                    : (dialog.tile !== "" ? "file://" + dialog.tile : "")
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6

                        RexonButton {
                            objectName: "globalSaveShot"
                            pal: section.pal
                            enabled: !dialog.shooting
                            text: dialog.q.shotText || ""
                            onClicked: {
                                dialog.shooting = true;
                                dialog.message = dialog.q.shotWaiting || "";
                                actions.takeScreenshot();
                            }
                        }
                        RexonButton {
                            objectName: "globalSavePick"
                            pal: section.pal
                            text: dialog.q.pickText || ""
                            onClicked: imageDialog.open()
                        }
                        RexonButton {
                            objectName: "globalSaveTile"
                            pal: section.pal
                            visible: dialog.preview !== ""
                            text: dialog.q.tileText || ""
                            onClicked: dialog.preview = ""
                        }
                        Text {
                            Layout.fillWidth: true
                            visible: dialog.preview === ""
                            text: dialog.q.previewNote || ""
                            color: section.pal.text_secondary
                            font.pointSize: 9
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    visible: dialog.message !== ""
                    text: dialog.message
                    color: section.pal.text_secondary
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    Layout.alignment: Qt.AlignRight
                    spacing: 8

                    RexonButton {
                        objectName: "globalSaveConfirm"
                        pal: section.pal
                        enabled: dialog.problem === "" && dialog.anyPicked && !dialog.shooting
                        text: dialog.q.saveText || ""
                        onClicked: dialog.save()
                    }
                    RexonButton {
                        objectName: "globalSaveCancel"
                        pal: section.pal
                        text: dialog.q.cancelText || ""
                        onClicked: dialog.close()
                    }
                }
            }
        }
    }
}
