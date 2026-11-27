// The "Login manager" block in the login screen section (above the SDDM
// theme shelf and above the Plasma Login Manager panel): which login
// screens are installed, which is the default and which is running now,
// "Make default" with a choice of LightDM greeter, a confirmation window
// and "How to revert".
//
// Markup only. Data, decisions and texts — the core
// (`rexon_core::system_exec::login_screen_view`), delivered by the
// `loginScreenReady` probe; switching — `makeDefaultLoginManager` of the
// tab's actions object, the answer — `loginManagerSwitched`. The strip
// "the login screen itself opens a deprecated X11 session" (the probe's
// `waylandDefault`) — the `makeWaylandDefaultSession` button, same answer.
//
// Foolproofing (rule #13): the button is locked where pressing it would
// not work, and the reason is shown in grey right away, before the
// password; the core checks again anyway and refuses by itself.
//
// With SDDM and Plasma Login Manager there is a "Login screen background
// and look" block below (like "Change background" and "Apply Plasma
// settings" of the KDE modules): data — `sddmGreeter` or `plmGreeter` of
// the same probe (one view, `system_plan::SddmGreeterView`), actions —
// `setLoginBackground` and `applyLoginLook` (the core picks the path by
// the login screen itself), the answer — `loginLookDone`.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})
    /// The tab's actions object (`AppearanceActions`).
    property var actions: null

    /// Last answer of the probe.
    property var st: ({})
    /// Selected greeter per unit (preselection comes from the core).
    property var greeterFor: ({})
    /// Switching in progress — the buttons are locked.
    property bool busy: false
    /// Result of the last switch.
    property string message: ""
    property bool failed: false
    /// Login screen background and look block: action in progress, result.
    property bool lookBusy: false
    property string lookMessage: ""
    property bool lookFailed: false
    /// Live preview of the login screen is open.
    property bool previewRunning: false

    function take(payload) {
        if (!payload)
            return;
        try {
            section.st = JSON.parse(payload);
        } catch (e) {
            section.st = ({});
        }
        // After a click the check box loses its binding — the state is taken from
        // the probe again (if it did not work out, it goes back to how it was).
        if (section.greeter)
            clockBox.checked = section.greeter.clock.checked === true;
    }

    function togglePreview() {
        if (!section.actions)
            return;
        if (section.previewRunning) {
            section.actions.closeLoginPreview();
            return;
        }
        const err = section.actions.previewLoginScreen("");
        if (err !== "") {
            section.lookFailed = true;
            section.lookMessage = err;
        }
    }

    function chosenGreeter(row) {
        const g = section.greeterFor[row.unit];
        return g !== undefined ? g : (row.greeter || "");
    }

    /// Switch result without repetition: after success, "takes effect after a
    /// reboot" is already said by the `pending` line, and "how to revert" by
    /// the block below; in that case the result is not duplicated.
    function shownMessage() {
        if (section.failed || section.busy || section.message === "")
            return section.message;
        if ((section.st.pending || "") !== "")
            return "";
        return (section.st.rollback || "") !== "" ? section.message.split("\n\n")[0] : section.message;
    }

    function ask(row) {
        confirmDialog.lookAction = "";
        const greeter = section.chosenGreeter(row);
        const withGreeter = (row.greeters || []).find(g => g.id === greeter);
        confirmDialog.unit = row.unit;
        confirmDialog.session = "";
        confirmDialog.yesText = "";
        confirmDialog.greeter = withGreeter ? greeter : "";
        confirmDialog.heading = row.confirmTitle || "";
        confirmDialog.question = withGreeter ? withGreeter.confirm : (row.confirm || "");
        confirmDialog.open();
    }

    function askSession(offer) {
        confirmDialog.lookAction = "";
        confirmDialog.unit = "";
        confirmDialog.greeter = "";
        confirmDialog.session = offer.x11 || "";
        confirmDialog.yesText = offer.button || "";
        confirmDialog.heading = offer.button || "";
        confirmDialog.question = offer.confirm || "";
        confirmDialog.open();
    }

    function startSession(x11) {
        if (!section.actions || !section.actions.makeWaylandDefaultSession || section.busy)
            return;
        section.busy = true;
        section.failed = false;
        section.message = qsTranslate("rexon", "Changing the login screen…");
        section.actions.makeWaylandDefaultSession(x11);
    }

    function start(unit, greeter) {
        if (!section.actions || !section.actions.makeDefaultLoginManager || section.busy)
            return;
        section.busy = true;
        section.failed = false;
        section.message = qsTranslate("rexon", "Changing the login screen…");
        section.actions.makeDefaultLoginManager(unit, greeter);
    }

    Connections {
        target: section.actions
        ignoreUnknownSignals: true

        function onLoginScreenReady(payload) {
            section.take(payload);
        }

        function onLoginManagerSwitched(ok, text) {
            section.busy = false;
            section.failed = !ok;
            section.message = text;
        }

        function onLoginLookDone(ok, text) {
            section.lookBusy = false;
            section.lookFailed = !ok;
            section.lookMessage = text;
        }

        function onLoginPreviewChanged(running, text) {
            section.previewRunning = running;
            if (text !== "") {
                section.lookFailed = true;
                section.lookMessage = text;
            }
        }
    }

    function startLook(kind, path) {
        if (!section.actions || section.lookBusy)
            return;
        section.lookBusy = true;
        section.lookFailed = false;
        section.lookMessage = qsTranslate("rexon", "Changing the login screen…");
        if (kind === "background")
            section.actions.setLoginBackground(path);
        else
            section.actions.applyLoginLook(kind === "lookReset");
    }

    function askLook(kind) {
        const look = section.greeter ? section.greeter.look : null;
        if (!look)
            return;
        confirmDialog.unit = "";
        confirmDialog.greeter = "";
        confirmDialog.session = "";
        confirmDialog.lookAction = kind;
        confirmDialog.yesText = kind === "lookReset" ? qsTranslate("rexon", "Reset") : qsTranslate("rexon", "Apply");
        confirmDialog.heading = kind === "lookReset" ? (look.reset || "").replace("…", "") : (look.confirmTitle || "");
        confirmDialog.question = kind === "lookReset" ? (look.resetConfirm || "") : (look.confirm || "");
        confirmDialog.open();
    }

    Component.onCompleted: {
        // The probe answer may have arrived before the block was connected.
        if (section.actions && section.actions.loginScreen)
            section.take(section.actions.loginScreen());
    }

    readonly property var managers: section.st.managers || []
    /// The login screen itself opens a deprecated X11 session (`null` — no).
    readonly property var sessionOffer: section.st.waylandDefault || null
    /// Login screen background and look block (SDDM or Plasma Login Manager;
    /// `null` — none).
    readonly property var greeter: section.st.sddmGreeter && section.st.sddmGreeter.visible
                                   ? section.st.sddmGreeter
                                   : (section.st.plmGreeter && section.st.plmGreeter.visible
                                      ? section.st.plmGreeter : null)

    visible: section.managers.length > 0 || section.sessionOffer !== null || section.greeter !== null
    color: section.sty.group ? section.sty.group.background : section.pal.bg_card
    border.color: section.sty.group ? section.sty.group.border : section.pal.border
    border.width: 1
    radius: section.sty.group ? section.sty.group.radius : 6
    implicitHeight: section.visible ? body.implicitHeight + 2 * (body.y) : 0

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: (section.sty.group ? section.sty.group.marginH : 12) + section.border.width
        y: (section.sty.group ? section.sty.group.marginV : 10) + section.border.width
        spacing: section.sty.group ? section.sty.group.spacing : 8

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Login manager")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "The program that shows the sign-in screen when the computer starts. A new default one appears after a restart; your current session isn't interrupted.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // Compatibility of the default login screen with the installed desktops
        // (GNOME without GDM — no lock screen, Deepin without DDM — will not
        // start). Texts come from the core (`system_plan::dm_compat_issues`).
        Repeater {
            model: section.st.compat || []

            delegate: RexonText {
                required property string modelData
                objectName: "loginCompat"
                Layout.fillWidth: true
                text: "⚠️ " + modelData
                color: section.pal.state_warn
                wrapMode: Text.WordWrap
            }
        }

        // A deprecated X11 session as the default — a warning and a button.
        RowLayout {
            Layout.fillWidth: true
            visible: section.sessionOffer !== null
            spacing: 10

            RexonText {
                Layout.fillWidth: true
                text: section.sessionOffer ? section.sessionOffer.text : ""
                color: section.pal.state_warn
                wrapMode: Text.WordWrap
            }

            RexonButton {
                objectName: "loginWaylandDefault"
                pal: section.pal
                enabled: !section.busy
                text: section.sessionOffer ? section.sessionOffer.button : ""
                onClicked: section.askSession(section.sessionOffer)
            }
        }

        Repeater {
            model: section.managers

            delegate: Rectangle {
                id: row
                required property var modelData

                objectName: "loginManager_" + row.modelData.unit
                Layout.fillWidth: true
                implicitHeight: rowLayout.implicitHeight + 16
                radius: 6
                color: section.pal.bg_card
                border.width: row.modelData.active ? 2 : 1
                border.color: row.modelData.active ? section.pal.accent : section.pal.border

                RowLayout {
                    id: rowLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 12
                    y: 8
                    spacing: 10

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            RexonText {
                                text: row.modelData.label
                                color: section.pal.text_primary
                                font.bold: true
                            }

                            // "Default" is a label, not a button.
                            RexonText {
                                visible: row.modelData.active === true
                                text: qsTranslate("rexon", "Default", "login manager")
                                color: section.pal.accent
                                font.bold: true
                            }

                            RexonText {
                                visible: row.modelData.running === true && row.modelData.active !== true
                                text: qsTranslate("rexon", "Running now")
                                color: section.pal.text_secondary
                            }

                            Item { Layout.fillWidth: true }
                        }

                        RexonText {
                            Layout.fillWidth: true
                            visible: text !== ""
                            text: row.modelData.reason || ""
                            color: section.pal.text_disabled !== undefined ? section.pal.text_disabled
                                                                          : section.pal.text_secondary
                            wrapMode: Text.WordWrap
                        }

                        Repeater {
                            model: row.modelData.warnings || []

                            delegate: RexonText {
                                required property string modelData
                                Layout.fillWidth: true
                                text: "• " + modelData
                                color: section.pal.state_warn
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    RexonText {
                        visible: greeterCombo.visible
                        text: qsTranslate("rexon", "Greeter:")
                        color: section.pal.text_secondary
                    }

                    RexonComboBox {
                        id: greeterCombo
                        objectName: "loginGreeter_" + row.modelData.unit
                        pal: section.pal
                        visible: row.modelData.active !== true && (row.modelData.greeters || []).length > 0
                        enabled: !section.busy && row.modelData.canSwitch === true
                        model: row.modelData.greeters || []
                        textRole: "name"
                        valueRole: "id"
                        selectedValue: section.chosenGreeter(row.modelData)
                        onActivated: {
                            const next = Object.assign({}, section.greeterFor);
                            next[row.modelData.unit] = currentValue;
                            section.greeterFor = next;
                        }
                    }

                    RexonButton {
                        objectName: "loginMakeDefault_" + row.modelData.unit
                        visible: row.modelData.active !== true
                        enabled: !section.busy && section.st.switchable === true
                                 && row.modelData.canSwitch === true
                        pal: section.pal
                        text: qsTranslate("rexon", "Make default")
                        onClicked: section.ask(row.modelData)
                    }
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.pending || ""
            color: section.pal.accent
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: text !== ""
            text: section.shownMessage()
            color: section.failed ? section.pal.state_warn : section.pal.text_primary
            wrapMode: Text.WordWrap
        }

        // "How to revert" — the text the helper left for the console
        // (`/etc/rexon/previous-display-manager`). Selectable and copyable.
        RexonText {
            Layout.fillWidth: true
            Layout.topMargin: 4
            visible: rollbackText.text !== ""
            text: qsTranslate("rexon", "How to bring back the previous login screen")
            color: section.pal.text_primary
            font.bold: true
            wrapMode: Text.WordWrap
        }

        TextEdit {
            id: rollbackText
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.rollback || ""
            readOnly: true
            selectByMouse: true
            color: section.pal.text_secondary
            selectionColor: section.pal.accent
            wrapMode: TextEdit.Wrap
            // Start of the line: without explicit alignment the field takes it from
            // the text itself, and mirroring flips an explicit `AlignLeft`.
            horizontalAlignment: TextEdit.AlignLeft
        }

        // ── Login screen background and look (SDDM, Plasma Login Manager) ─
        ColumnLayout {
            id: greeterBlock
            objectName: "sddmGreeterBlock"
            Layout.fillWidth: true
            Layout.topMargin: 8
            visible: section.greeter !== null
            spacing: 8

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: 1
                color: section.pal.border
            }

            // The title with "Apply" / "Reset" / "Preview" at the top right and the
            // result right under them, as in "MATE panel" (Wehrwolfmann 27.11.2026):
            // the buttons used to sit at the end of the block, below the picture.
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    text: section.greeter ? section.greeter.title : ""
                    color: section.pal.text_primary
                    font.bold: true
                    font.pointSize: 13
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    Layout.alignment: Qt.AlignTop
                    spacing: 8

                    RexonButton {
                        objectName: "sddmLookApply"
                        pal: section.pal
                        enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                        text: section.greeter ? section.greeter.look.apply : ""
                        onClicked: section.askLook("look")
                    }

                    RexonButton {
                        objectName: "sddmLookReset"
                        pal: section.pal
                        enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                        text: section.greeter ? section.greeter.look.reset : ""
                        onClicked: section.askLook("lookReset")
                    }

                    // Live preview: the greeter in test mode, without leaving the session.
                    // While it is open, the same button closes it.
                    RexonButton {
                        id: previewButton
                        objectName: "loginPreview"
                        pal: section.pal
                        enabled: section.greeter !== null && (section.greeter.preview.available || section.previewRunning)
                        text: section.greeter
                              ? "▶ " + (section.previewRunning ? section.greeter.preview.close : section.greeter.preview.label)
                              : ""
                        onClicked: section.togglePreview()
                        HoverHandler { id: previewHover }
                        ToolTip.text: section.greeter
                                      ? (section.greeter.preview.available ? section.greeter.preview.tooltip
                                                                           : section.greeter.preview.note)
                                      : ""
                        ToolTip.visible: previewHover.hovered && ToolTip.text !== ""
                        ToolTip.delay: 500
                    }
                }
            }

            RexonText {
                objectName: "sddmGreeterMessage"
                Layout.fillWidth: true
                visible: text !== ""
                text: section.lookMessage
                color: section.lookFailed ? section.pal.state_warn : section.pal.text_primary
                wrapMode: Text.WordWrap
            }

            // Theme — only for SDDM: Plasma Login Manager has no themes.
            RexonText {
                Layout.fillWidth: true
                visible: text !== ""
                text: section.greeter ? section.greeter.theme : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                visible: text !== ""
                text: section.greeter ? section.greeter.blocked : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                // Background preview: the theme's picture or a custom one.
                Rectangle {
                    Layout.preferredWidth: 192
                    Layout.preferredHeight: 108
                    radius: 4
                    color: section.pal.bg_secondary
                    border.color: section.pal.border
                    border.width: 1
                    clip: true

                    Image {
                        id: greeterPreview
                        objectName: "sddmGreeterPreview"
                        anchors.fill: parent
                        anchors.margins: 1
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: false
                        sourceSize.width: 384
                        source: section.greeter && section.greeter.background.image !== ""
                                ? "file://" + section.greeter.background.image : ""
                    }

                    RexonText {
                        anchors.centerIn: parent
                        visible: greeterPreview.status !== Image.Ready
                        text: qsTranslate("rexon", "No picture")
                        color: section.pal.text_secondary
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    RexonText {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: section.greeter ? (section.greeter.background.state || section.greeter.background.note) : ""
                        color: section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: 8

                        RexonButton {
                            objectName: "sddmBackgroundChoose"
                            pal: section.pal
                            enabled: !section.lookBusy && section.greeter !== null
                                     && section.greeter.canChange && section.greeter.background.supported
                            text: section.greeter ? section.greeter.background.choose : ""
                            onClicked: backgroundDialog.open()
                        }

                        RexonButton {
                            objectName: "sddmBackgroundReset"
                            pal: section.pal
                            visible: section.greeter !== null && section.greeter.background.canReset
                            enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                            text: section.greeter ? section.greeter.background.reset : ""
                            onClicked: section.startLook("background", "")
                        }
                    }

                    // "Show clock" — as in the KDE modules: for SDDM it is a theme key, for
                    // Plasma Login Manager its own one.
                    RexonCheckBox {
                        id: clockBox
                        objectName: "loginShowClock"
                        pal: section.pal
                        visible: section.greeter !== null && section.greeter.clock.supported
                        enabled: !section.lookBusy && section.greeter !== null && section.greeter.canChange
                        text: section.greeter ? section.greeter.clock.label : ""
                        onToggled: {
                            if (!section.actions || section.lookBusy)
                                return;
                            section.lookBusy = true;
                            section.lookFailed = false;
                            section.lookMessage = qsTranslate("rexon", "Changing the login screen…");
                            section.actions.setLoginClock(clockBox.checked);
                        }
                    }
                }
            }

            // Why there is no preview (a locked button will not show a tooltip).
            RexonText {
                Layout.fillWidth: true
                visible: text !== "" && !section.previewRunning
                text: section.greeter && !section.greeter.preview.available ? section.greeter.preview.note : ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

        }
    }

    FileDialog {
        id: backgroundDialog
        title: section.greeter ? section.greeter.background.dialogTitle : ""
        nameFilters: section.greeter ? [section.greeter.background.filter] : []
        fileMode: FileDialog.OpenFile
        onAccepted: {
            const url = selectedFile.toString();
            const path = decodeURIComponent(url.replace(/^file:\/\//, ""));
            if (path !== "")
                section.startLook("background", path);
        }
    }

    // ── Confirmation ──────────────────────────────────────────────────────
    //
    // Inside the window (the Big Picture canon); focus on opening is on
    // "Cancel": an accidental Enter will not change the login screen.

    Popup {
        id: confirmDialog

        property string unit: ""
        property string greeter: ""
        /// Not empty — a question about the default session (the X11 session from
        /// the probe).
        property string session: ""
        /// Custom "Yes" label (empty means "Make default").
        property string yesText: ""
        property string heading: ""
        property string question: ""
        /// Not empty — a question of the login screen look block (`look`,
        /// `lookReset`).
        property string lookAction: ""

        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        width: Math.min((parent ? parent.width : 600) - 40, 600)
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

            TextEdit {
                Layout.fillWidth: true
                text: confirmDialog.question
                readOnly: true
                selectByMouse: true
                color: section.pal.text_primary
                selectionColor: section.pal.accent
                wrapMode: TextEdit.Wrap
                // Start of the line: without explicit alignment the field takes it from
                // the text itself, and mirroring flips an explicit `AlignLeft`.
                horizontalAlignment: TextEdit.AlignLeft
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "loginConfirmYes"
                    pal: section.pal
                    highlighted: true
                    text: confirmDialog.yesText !== "" ? confirmDialog.yesText : qsTranslate("rexon", "Make default")
                    onClicked: {
                        const unit = confirmDialog.unit;
                        const greeter = confirmDialog.greeter;
                        const session = confirmDialog.session;
                        const lookAction = confirmDialog.lookAction;
                        confirmDialog.lookAction = "";
                        confirmDialog.close();
                        if (lookAction !== "")
                            section.startLook(lookAction, "");
                        else if (session !== "")
                            section.startSession(session);
                        else
                            section.start(unit, greeter);
                    }
                }

                RexonButton {
                    id: confirmNo
                    objectName: "loginConfirmNo"
                    pal: section.pal
                    text: qsTranslate("rexon", "Cancel", "dialog button")
                    onClicked: confirmDialog.close()
                }
            }
        }

        onOpened: confirmNo.forceActiveFocus()
    }
}
