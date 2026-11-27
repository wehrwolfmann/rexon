// The "MATE panel" section: ready panel layouts with one button ("Like
// Windows", classic, like in Mint…), a mark on the current one, restoring
// the previous panel. Shown only in a MATE session and only if mate-panel
// is present.
//
// The section is self-contained — its own actions object, its own
// confirmation window; from outside it needs only `pal` and `sty`. It sits in
// the "Appearance" tab as the "MATE panel" section (screen `matepanel`) and
// fills the height it is given: the title, the "Apply" / "Restore previous
// panel" buttons and the result line with "Retry" / "Open log" stay pinned at
// the top, only the list of layouts scrolls (Wehrwolfmann 27.11.2026: the
// buttons and the result at the end of a long list were out of sight).
//
// Markup only: snapshot, labels, applying and restoring — `MatePanelActions`
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

    property var st: ({})

    /// Layout selected in the list (the current one by default).
    property string selected: ""

    /// The last result: the text, whether to offer "Retry", the build log.
    property string resultText: ""
    property bool resultRetry: false
    property string resultLog: ""
    /// What "Retry" repeats: the layout and whether parts are built from AUR.
    property string lastId: ""
    property bool lastInstall: false

    function reload() {
        section.st = JSON.parse(actions.state());
        const layouts = section.st.layouts || [];
        if (!layouts.some(l => l.id === section.selected)) {
            const current = layouts.find(l => l.current);
            section.selected = current ? current.id : (layouts.length ? layouts[0].id : "");
        }
    }

    function show(text, retry, log) {
        section.resultText = text || "";
        section.resultRetry = retry === true;
        section.resultLog = log || "";
    }

    /// Applying in the background (parts of the layout are being installed) —
    /// the buttons are locked.
    property bool busy: false

    /// Ask for confirmation: `id` is the layout, empty means restoring the
    /// previous panel.
    function ask(id) {
        const texts = JSON.parse(actions.confirm(id));
        confirmDialog.parts = false;
        confirmDialog.layoutId = id;
        confirmDialog.heading = texts.title;
        confirmDialog.question = texts.question;
        confirmDialog.open();
    }

    /// Apply a layout: if parts that can be installed are missing, first ask
    /// about that ("No" applies with substitution, as before).
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
        section.lastId = id;
        section.lastInstall = install;
        section.show(install
                     ? qsTranslate("rexon", "Installing the missing parts of the layout — a build from AUR can take a few minutes…")
                     : "", false, "");
        actions.applyAsync(id, install);
    }

    MatePanelActions {
        id: actions
        objectName: "matePanelActions"
        onApplied: function(result) {
            section.busy = false;
            let r = {};
            try {
                r = JSON.parse(result);
            } catch (e) {
                r = { text: result };
            }
            section.show(r.text, r.retry, r.log);
            section.reload();
        }
        onRestored: function(text) {
            section.busy = false;
            section.show(text, false, "");
            section.reload();
        }
    }

    Component.onCompleted: section.reload()

    /// The screen shows the section (the tab decides); the section itself also
    /// needs a MATE session with mate-panel.
    property bool shown: true
    visible: section.shown && section.st.available === true
    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius

    // ── Pinned: title, buttons, result ────────────────────────────────────

    ColumnLayout {
        id: head
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        spacing: section.sty.group.spacing

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                RexonText {
                    Layout.fillWidth: true
                    text: qsTranslate("rexon", "MATE panel")
                    color: section.pal.text_primary
                    font.bold: true
                    font.pointSize: 13
                    wrapMode: Text.WordWrap
                }

                RexonText {
                    Layout.fillWidth: true
                    text: qsTranslate("rexon", "Ready-made panel layouts: pick one and press “Apply”. The panel changes right away, without logging out.")
                    color: section.pal.text_secondary
                    wrapMode: Text.WordWrap
                }
            }

            RexonButton {
                id: restoreButton
                objectName: "matePanelRestore"
                Layout.alignment: Qt.AlignTop
                visible: section.st.can_restore === true
                enabled: !section.busy
                pal: section.pal
                text: qsTranslate("rexon", "Restore previous panel")
                onClicked: section.ask("")
            }

            RexonButton {
                id: applyButton
                objectName: "matePanelApply"
                Layout.alignment: Qt.AlignTop
                pal: section.pal
                highlighted: true
                enabled: section.selected !== "" && !section.busy
                text: qsTranslate("rexon", "Apply")
                onClicked: section.ask(section.selected)
            }
        }

        // The result of the last "Apply" / "Restore" — next to the buttons, so
        // it is seen whatever the list is scrolled to.
        RowLayout {
            Layout.fillWidth: true
            visible: section.resultText !== ""
            spacing: 8

            RexonText {
                id: resultLine
                objectName: "matePanelResult"
                Layout.fillWidth: true
                Layout.maximumHeight: section.height / 3
                text: section.resultText
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
                elide: Text.ElideRight
            }

            RexonButton {
                objectName: "matePanelRetry"
                Layout.alignment: Qt.AlignTop
                visible: section.resultRetry && section.lastId !== ""
                enabled: !section.busy
                pal: section.pal
                text: qsTranslate("rexon", "Retry")
                onClicked: section.start(section.lastId, section.lastInstall)
            }

            RexonButton {
                objectName: "matePanelOpenLog"
                Layout.alignment: Qt.AlignTop
                visible: section.resultLog !== ""
                pal: section.pal
                text: qsTranslate("rexon", "Open log")
                onClicked: actions.openLog(section.resultLog)
            }
        }
    }

    // ── The list of layouts: the only part that scrolls ───────────────────

    ScrollView {
        id: listView
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: head.bottom
        anchors.bottom: parent.bottom
        anchors.topMargin: section.sty.group.spacing
        anchors.leftMargin: section.sty.group.marginH + section.border.width
        anchors.rightMargin: section.sty.group.marginH + section.border.width
        anchors.bottomMargin: section.sty.group.marginV + section.border.width
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            id: body
            width: listView.availableWidth
            spacing: section.sty.group.spacing

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
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: 12
                        y: 8
                        spacing: 2

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            RexonText {
                                text: (row.chosen ? "◉ " : "○ ") + row.modelData.title
                                color: section.pal.text_primary
                                font.bold: true
                            }

                            RexonText {
                                visible: row.modelData.current === true
                                text: qsTranslate("rexon", "(current)")
                                color: section.pal.accent
                            }

                            Item { Layout.fillWidth: true }
                        }

                        RexonText {
                            Layout.fillWidth: true
                            text: row.modelData.description
                            color: section.pal.text_secondary
                            wrapMode: Text.WordWrap
                        }

                        Repeater {
                            model: row.modelData.notes || []

                            delegate: RexonText {
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
        }
    }

    // ── Confirmation ──────────────────────────────────────────────────────
    //
    // Inside the window, not a system window (the Big Picture canon); focus on
    // opening is on "No": an accidental Enter will not replace the panel.

    Popup {
        id: confirmDialog

        property string layoutId: ""
        /// Second question — about the missing parts: "No" applies with substitution.
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

            RexonText {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            RexonText {
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
