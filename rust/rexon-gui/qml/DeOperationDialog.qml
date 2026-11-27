// Desktop operation window — ONE for all tabs that install, check, change
// and remove a desktop ("Desktops" and "DE comparison").
//
// One wizard dialog per operation (architecture rule #16: chains of
// windows are awkward in Big Picture): question or refusal → (SteamOS
// password step) → progress → result. The state and all labels come from
// the `actions.operation` map of the Rust object `DesktopsActions` (the
// executor is `rexon_core::system_de_exec`); there are no texts of our own
// here.
//
// The Python tab host has no such property — then the window is simply
// not shown, and the host draws the questions itself
// (`qt_main.de_progress`).
//
// Hook-up: `DeOperationDialog { anchors.fill: parent; actions: …;
// pal: …; onDesktopsChanged: <recompute the set of desktops> }`.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    /// The page's side-actions object.
    property var actions: null

    /// Page skin palette (button tones are taken if they are in it).
    property var pal: ({})

    /// The operation succeeded — the set of installed desktops may have
    /// changed; the page will recompute itself.
    signal desktopsChanged()

    readonly property var op: (root.actions && root.actions.operation !== undefined)
                              ? root.actions.operation : null
    readonly property string opPhase: (root.op && root.op.phase !== undefined)
                                      ? root.op.phase : ""

    readonly property var primaryTone: root.pal.primary_tone !== undefined
                                       ? root.pal.primary_tone : null
    readonly property var dangerTone: root.pal.destructive_tone !== undefined
                                      ? root.pal.destructive_tone : null

    Connections {
        target: root.actions
        ignoreUnknownSignals: true
        function onOperationLog(line) { root.queueLogLine(line); }
        function onDesktopsChanged() { root.desktopsChanged(); }
    }

    // Operation log: pacman sends dozens of lines per second. Appending one
    // line at a time re-laid out the whole text of the field on every line
    // and pulled the view down — scrolling was wrenched out of the user's
    // hands, the main thread could not keep up, and the shell offered to close
    // the "frozen" window (16.11.2026, Cinnamon). So lines accumulate and go
    // into the field as a batch once per logFlush.interval, the field keeps
    // only the tail (the full log is in rexon.log), and the view moves down
    // only if the user was already at the bottom.
    readonly property int logKeepLines: 2000
    property var logPending: []
    property int logLines: 0

    function queueLogLine(line) {
        root.logPending.push(line);
        if (!logFlush.running)
            logFlush.start();
    }

    function flushLog() {
        if (root.logPending.length === 0)
            return;
        const chunk = root.logPending.join("\n");
        const added = root.logPending.length;
        root.logPending = [];
        const flick = logScroll.contentItem as Flickable;
        const follow = !flick || flick.contentHeight <= flick.height
            || flick.contentY + flick.height >= flick.contentHeight - 24;
        opLog.append(chunk);
        root.logLines += added;
        if (root.logLines > root.logKeepLines + 500) {
            // Trimming is rare (once per 500 lines) — re-laying out the whole text is
            // acceptable here.
            const all = opLog.text.split("\n");
            opLog.text = all.slice(all.length - root.logKeepLines).join("\n");
            root.logLines = root.logKeepLines;
        }
        if (follow)
            opLog.cursorPosition = opLog.length;
    }

    function resetLog() {
        logFlush.stop();
        root.logPending = [];
        root.logLines = 0;
        opLog.clear();
    }

    Timer {
        id: logFlush
        interval: 100
        onTriggered: root.flushLog()
    }

    readonly property bool passwordBusy: (root.op && root.op.passwordBusy === true)

    onOpPhaseChanged: {
        if (root.opPhase === "running")
            root.resetLog();
        else if (root.opPhase === "done")
            root.flushLog();
        // The password does not live in the fields longer than the step.
        if (root.opPhase !== "password") {
            pwNew.clear();
            pwRepeat.clear();
        }
        if (root.opPhase !== "")
            Qt.callLater(root.focusOperation);
    }

    function focusOperation() {
        // The safe answer has focus: an accidental Enter starts nothing.
        if (root.opPhase === "confirm")
            opNo.forceActiveFocus();
        else if (root.opPhase === "password")
            pwNew.forceActiveFocus();
        else if (root.opPhase === "blocked")
            opOk.forceActiveFocus();
        else if (opClose.enabled)
            opClose.forceActiveFocus();
    }

    function dismissOperation() {
        if (root.actions && root.opPhase !== "running" && !root.passwordBusy)
            root.actions.dismissOperation();
    }

    function submitPassword() {
        if (root.actions && root.opPhase === "password" && !root.passwordBusy)
            root.actions.submitPassword(pwNew.text, pwRepeat.text);
    }

    Popup {
        id: opDialog
        objectName: "desktopsOperationDialog"

        parent: root
        modal: true
        // A page hidden by its host (the second page of a section) does not show
        // its window: otherwise two pages of the same actions object would put
        // up two identical windows on top of each other.
        visible: root.opPhase !== "" && root.visible
        closePolicy: Popup.NoAutoClose
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 760)
        height: (root.opPhase === "running" || root.opPhase === "done")
                ? Math.min(root.height - 40, 580) : implicitHeight
        padding: 20

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            Keys.onEscapePressed: root.dismissOperation()

            RexonText {
                Layout.fillWidth: true
                text: root.op ? root.op.title : ""
                color: root.pal.text_primary
                font.pointSize: 14
                font.weight: Font.DemiBold
                wrapMode: Text.WordWrap
            }

            // Question, refusal or explanation of the password step.
            RexonText {
                Layout.fillWidth: true
                visible: root.opPhase === "confirm" || root.opPhase === "blocked"
                         || root.opPhase === "password"
                text: root.op ? root.op.text : ""
                textFormat: Text.RichText
                color: root.pal.text_primary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            // Question option switches (style of the "Like SteamOS" look: Vapor or
            // VGUI2): label and options come from the map, the selected one is
            // highlighted. A click rebuilds the question — the text above changes.
            Repeater {
                model: (root.opPhase === "confirm" && root.op && root.op.choices !== undefined)
                       ? root.op.choices : []

                delegate: ColumnLayout {
                    id: choiceRow
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: 4

                    RexonText {
                        text: choiceRow.modelData.label
                        color: root.pal.text_primary
                        font.pointSize: 11
                        font.weight: Font.DemiBold
                    }

                    // Options with an explanation (the desktop session: X11 / Wayland / both)
                    // as cards one below another: a label button and the explanation under
                    // it. An unavailable option is visible but cannot be pressed — why is
                    // said in the explanation.
                    readonly property bool described: choiceRow.modelData.options.length > 0
                        && choiceRow.modelData.options[0].description !== undefined
                        && choiceRow.modelData.options[0].description !== ""

                    Repeater {
                        model: choiceRow.described ? choiceRow.modelData.options : []

                        delegate: Rectangle {
                            id: choiceCard
                            required property var modelData
                            readonly property bool picked: choiceRow.modelData.current === choiceCard.modelData.value
                            readonly property bool available: choiceCard.modelData.enabled !== false
                            Layout.fillWidth: true
                            implicitHeight: cardColumn.implicitHeight + 16
                            radius: 6
                            color: "transparent"
                            border.width: choiceCard.picked ? 2 : 1
                            border.color: choiceCard.picked ? root.pal.accent : root.pal.border
                            opacity: choiceCard.available ? 1.0 : 0.6

                            ColumnLayout {
                                id: cardColumn
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.top: parent.top
                                anchors.margins: 8
                                spacing: 4

                                RexonButton {
                                    id: cardButton
                                    objectName: "desktopsOperationChoice_" + choiceCard.modelData.value
                                    Layout.fillWidth: true
                                    pal: root.pal
                                    checkable: true
                                    enabled: choiceCard.available
                                    checked: choiceCard.picked
                                    checkedTone: root.primaryTone
                                    text: (choiceCard.picked ? "◉  " : "○  ") + choiceCard.modelData.label
                                    onClicked: {
                                        // The selection comes from the map, not from the click.
                                        cardButton.checked = Qt.binding(function () {
                                            return choiceCard.picked;
                                        });
                                        if (root.actions && root.actions.setOperationChoice)
                                            root.actions.setOperationChoice(choiceRow.modelData.name,
                                                                            choiceCard.modelData.value);
                                    }
                                }

                                RexonText {
                                    Layout.fillWidth: true
                                    text: choiceCard.modelData.description
                                    textFormat: Text.RichText
                                    wrapMode: Text.WordWrap
                                    color: root.pal.text_secondary
                                    font.pointSize: 10
                                }
                            }
                        }
                    }

                    Flow {
                        Layout.fillWidth: true
                        visible: !choiceRow.described
                        spacing: 8

                        Repeater {
                            model: choiceRow.modelData.options

                            delegate: RexonButton {
                                id: choiceButton
                                required property var modelData
                                objectName: "desktopsOperationChoice_" + choiceButton.modelData.value
                                pal: root.pal
                                checkable: true
                                checked: choiceRow.modelData.current === choiceButton.modelData.value
                                checkedTone: root.primaryTone
                                text: choiceButton.modelData.label
                                onClicked: {
                                    // The selection comes from the map, not from the click.
                                    choiceButton.checked = Qt.binding(function () {
                                        return choiceRow.modelData.current === choiceButton.modelData.value;
                                    });
                                    if (root.actions && root.actions.setOperationChoice)
                                        root.actions.setOperationChoice(choiceRow.modelData.name,
                                                                        choiceButton.modelData.value);
                                }
                            }
                        }
                    }
                }
            }

            // Optional check box of the question (the game mode of the "Like SteamOS"
            // look): present only when its label came in the map. Toggling rebuilds
            // the question — the text above changes.
            RexonCheckBox {
                id: opOption
                objectName: "desktopsOperationOption"
                Layout.fillWidth: true
                visible: root.opPhase === "confirm" && root.op
                         && root.op.optionText !== undefined && root.op.optionText !== ""
                pal: root.pal
                text: (root.op && root.op.optionText !== undefined) ? root.op.optionText : ""
                checked: root.op ? root.op.optionChecked === true : false
                onToggled: {
                    const want = opOption.checked;
                    // The check box state comes from the map, not from the click: a click
                    // breaks the binding, and the next question would show the old state.
                    opOption.checked = Qt.binding(function () {
                        return root.op ? root.op.optionChecked === true : false;
                    });
                    if (root.actions && root.actions.setOperationOption)
                        root.actions.setOperationOption(want);
                }
            }
            RexonText {
                Layout.fillWidth: true
                visible: opOption.visible && root.op && root.op.optionHint !== undefined
                         && root.op.optionHint !== ""
                text: (root.op && root.op.optionHint !== undefined) ? root.op.optionHint : ""
                textFormat: Text.RichText
                color: root.pal.status_unavailable !== undefined ? root.pal.status_unavailable
                                                                 : root.pal.text_secondary
                font.pointSize: 10
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                visible: root.opPhase === "blocked" && root.op && root.op.informative !== ""
                text: root.op ? root.op.informative : ""
                textFormat: Text.RichText
                color: root.pal.text_secondary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            // SteamOS password step: the "deck" user has no password, and without one
            // the system password window cannot be passed. Two fields, the core's
            // words below them.
            RexonTextField {
                id: pwNew
                objectName: "desktopsPasswordNew"
                Layout.fillWidth: true
                visible: root.opPhase === "password"
                enabled: !root.passwordBusy
                pal: root.pal
                echoMode: TextInput.Password
                placeholderText: (root.op && root.op.passwordNewText !== undefined)
                                 ? root.op.passwordNewText : ""
                onAccepted: pwRepeat.forceActiveFocus()
            }
            RexonTextField {
                id: pwRepeat
                objectName: "desktopsPasswordRepeat"
                Layout.fillWidth: true
                visible: root.opPhase === "password"
                enabled: !root.passwordBusy
                pal: root.pal
                echoMode: TextInput.Password
                placeholderText: (root.op && root.op.passwordRepeatText !== undefined)
                                 ? root.op.passwordRepeatText : ""
                onAccepted: root.submitPassword()
            }
            RexonText {
                objectName: "desktopsPasswordError"
                Layout.fillWidth: true
                visible: root.opPhase === "password" && root.op && root.op.passwordError !== ""
                text: root.op ? root.op.passwordError : ""
                color: root.pal.danger !== undefined ? root.pal.danger : "#dc5050"
                font.pointSize: 10
                wrapMode: Text.WordWrap
            }

            // Progress.
            RexonText {
                Layout.fillWidth: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                text: root.op ? root.op.status : ""
                color: root.pal.text_primary
                font.pointSize: 11
                font.weight: Font.DemiBold
            }

            // The bar uses our own markup: the Basic style does not follow the palette.
            Rectangle {
                Layout.fillWidth: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                implicitHeight: 10
                radius: 5
                color: root.pal.border

                Rectangle {
                    width: (root.op && root.op.barMax > 0)
                           ? parent.width * Math.max(0, Math.min(1, root.op.bar / root.op.barMax)) : 0
                    height: parent.height
                    radius: parent.radius
                    color: root.pal.accent
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: (root.opPhase === "running" || root.opPhase === "done")
                         && root.op && root.op.current !== ""
                text: root.op ? root.op.current : ""
                color: root.pal.text_secondary
                font.pointSize: 9
                elide: Text.ElideRight
            }

            ScrollView {
                id: logScroll
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: root.opPhase === "running" || root.opPhase === "done"
                clip: true

                TextArea {
                    id: opLog
                    objectName: "desktopsOperationLog"
                    readOnly: true
                    // By words; anywhere only for a long path.
                    wrapMode: TextEdit.WrapAtWordBoundaryOrAnywhere
                    // The operation log — program output and paths — reads left to right in
                    // any interface language, so the field is physically on the left:
                    // mirroring would flip an explicit `AlignLeft` to the right, while
                    // `AlignRight` under mirroring gives the left edge (like the PKGBUILD
                    // window in "Appearance").
                    horizontalAlignment: LayoutMirroring.enabled ? TextEdit.AlignRight
                                                                 : TextEdit.AlignLeft
                    color: root.pal.text_primary
                    font.family: "monospace"
                    font.pointSize: 9
                    background: Rectangle {
                        color: root.pal.bg_primary
                        border.color: root.pal.border
                    }
                    // The view moves to the tail only from flushLog and only if the user has
                    // not scrolled the log up.
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: root.opPhase === "done" && root.op && root.op.resultText !== ""
                text: root.op ? root.op.resultText : ""
                textFormat: Text.RichText
                color: root.pal.text_primary
                font.pointSize: 11
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 6
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: opYes
                    objectName: "desktopsOperationYes"
                    visible: root.opPhase === "confirm" || root.opPhase === "password"
                    enabled: !root.passwordBusy
                    pal: root.pal
                    // Irreversible (removal) in red, everything else in blue.
                    tone: (root.op && root.op.danger) ? root.dangerTone : root.primaryTone
                    text: root.op ? root.op.yesText : ""
                    onClicked: {
                        if (root.opPhase === "password")
                            root.submitPassword();
                        else
                            root.actions.confirmOperation();
                    }
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opNo
                    visible: root.opPhase === "confirm" || root.opPhase === "password"
                    enabled: !root.passwordBusy
                    pal: root.pal
                    text: root.op ? root.op.noText : ""
                    onClicked: root.dismissOperation()
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opOk
                    visible: root.opPhase === "blocked"
                    pal: root.pal
                    text: root.op ? root.op.okText : ""
                    onClicked: root.dismissOperation()
                    Keys.onReturnPressed: clicked()
                }
                RexonButton {
                    id: opCancel
                    visible: root.opPhase === "running" && root.op && root.op.cancellable
                    pal: root.pal
                    tone: root.dangerTone
                    text: root.op ? root.op.cancelText : ""
                    onClicked: root.actions.cancelOperation()
                }
                RexonButton {
                    id: opClose
                    visible: root.opPhase === "running" || root.opPhase === "done"
                    enabled: root.op ? root.op.closeEnabled : false
                    pal: root.pal
                    text: root.op ? root.op.closeText : ""
                    onClicked: root.dismissOperation()
                    onEnabledChanged: if (enabled) forceActiveFocus()
                    Keys.onReturnPressed: clicked()
                }
            }
        }
    }
}
