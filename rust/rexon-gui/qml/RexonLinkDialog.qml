// Окно вопроса перед внешней ссылкой: «Открыть внешний сайт <домен> в
// браузере?» с флажком «Больше не спрашивать для этого сайта».
//
// Одно на все вкладки. Обычно его создаёт сам `RexonLinkText` на время
// вопроса (`destroyOnClose`); страница, которой нужно спросить без текста
// (карточку товара по ссылке открыть не удалось), держит свой экземпляр.
// Тексты, домен и решение — ядро (`link_policy`) через синглтон `LinkGuard`.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Popup {
    id: dlg
    objectName: "rexonLinkDialog"

    property var pal: null
    /// Ответ `LinkGuard.decide` про спрашиваемую ссылку.
    property var info: ({})
    /// Создано на один вопрос — после закрытия убрать себя.
    property bool destroyOnClose: false

    readonly property color colBg: dlg.pal ? dlg.pal.bg_primary : "#1b2838"
    readonly property color colBorder: dlg.pal ? dlg.pal.border : "#2a4055"
    readonly property color colText: dlg.pal ? dlg.pal.text_primary : "#ffffff"
    readonly property color colTextSecondary: dlg.pal ? dlg.pal.text_secondary : "#8f98a0"

    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 40 : 560, 560)
    modal: true
    focus: true
    padding: 16
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    /// Спросить про `url`; `decided` — уже полученный ответ ядра.
    function ask(url, decided) {
        dlg.info = decided ? decided : JSON.parse(LinkGuard.decide(url));
        remember.checked = false;
        dlg.open();
    }

    onClosed: if (dlg.destroyOnClose) dlg.destroy()

    Overlay.modal: Rectangle { color: dlg.pal ? dlg.pal.modal_dim : "#99000000" }
    background: Rectangle {
        color: dlg.colBg
        border.color: dlg.colBorder
        radius: 4
    }

    contentItem: ColumnLayout {
        spacing: 10

        RexonText {
            Layout.fillWidth: true
            text: dlg.info.title || ""
            color: dlg.colText
            font.bold: true
            font.pointSize: 13
            elide: Text.ElideRight
        }

        RexonText {
            objectName: "linkQuestion"
            Layout.fillWidth: true
            text: dlg.info.text || ""
            color: dlg.colText
            wrapMode: Text.WordWrap
        }

        // Адрес целиком: куда именно ведёт ссылка, видно до нажатия.
        RexonText {
            Layout.fillWidth: true
            text: dlg.info.url || ""
            color: dlg.colTextSecondary
            textFormat: Text.PlainText
            wrapMode: Text.WrapAnywhere
            maximumLineCount: 3
            elide: Text.ElideRight
        }

        RexonText {
            Layout.fillWidth: true
            text: dlg.info.hint || ""
            color: dlg.colTextSecondary
            wrapMode: Text.WordWrap
        }

        RexonCheckBox {
            id: remember
            objectName: "linkRemember"
            // Файл и почта — не сайт: запоминать нечего.
            visible: (dlg.info.site || "") !== ""
            pal: dlg.pal
            text: dlg.info.remember || ""
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Item { Layout.fillWidth: true }

            RexonButton {
                objectName: "linkCancel"
                pal: dlg.pal
                text: dlg.info.cancel || ""
                onClicked: dlg.close()
            }

            RexonButton {
                id: openButton
                objectName: "linkOpen"
                pal: dlg.pal
                highlighted: true
                text: dlg.info.open || ""
                onClicked: {
                    LinkGuard.open(dlg.info.url || "", remember.visible && remember.checked);
                    dlg.close();
                }
            }
        }
    }

    onOpened: openButton.forceActiveFocus()
}
