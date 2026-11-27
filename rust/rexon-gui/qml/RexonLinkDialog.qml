// Question window before an external link: "Open external site <domain>
// in the browser?" with the check box "Don't ask again for this site".
//
// One for all tabs. Usually `RexonLinkText` creates it itself for the
// duration of the question (`destroyOnClose`); a page that needs to ask
// without text (opening the item card from a link failed) keeps its own
// instance. Texts, domain and decision come from the core (`link_policy`)
// through the `LinkGuard` singleton.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Popup {
    id: dlg
    objectName: "rexonLinkDialog"

    property var pal: null
    /// The `LinkGuard.decide` answer about the link being asked about.
    property var info: ({})
    /// Created for a single question — remove itself after closing.
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

    /// Ask about `url`; `decided` is the answer already received from the core.
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

        // The full address: where exactly the link leads is visible before clicking.
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
            // A file or an e-mail address is not a site: there is nothing to remember.
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
