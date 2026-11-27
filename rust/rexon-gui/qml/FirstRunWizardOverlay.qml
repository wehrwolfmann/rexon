// First-run wizard — ONE window over the main one.
//
// WHY. A person who unpacked the archive and launched Rexon from the
// folder got neither a launcher in the application menu nor the live
// wallpaper extension for GNOME and Cinnamon — and learned nothing about
// it. Here they are offered installation once, and the price is stated
// honestly: an administrator password or an installation just for
// yourself.
//
// WHOSE TEXTS ARE HERE — NONE OF OUR OWN. The decision "whether to offer",
// the modes, all labels of the welcome and of the consent to the "do not
// ask for a password" permission arrive as a ready map from the
// `FirstRunWizard` bridge (rust/rexon-gui/src/first_run_wizard.rs), which
// takes them from the core (`rexon_core::installer`). Typing a label here
// would create a second copy that silently drifts away from the product
// (rule #12).
//
// Never: NO CHAIN OF WINDOWS (architecture rule #16). The choice "whom to
// install for" and the consent to the permission are in ONE window: in Big
// Picture and on the Steam Deck a sequence of dialogs is awkward. Progress
// and result are shown by the shared operation window
// `DeOperationDialog.qml` — the same as in "Desktops": the wizard bridge
// has an `operation` property and an `operationLog` signal of the same
// shape, and the window is hooked up with the line `actions: wizard`,
// without a single change inside it.
//
// Control without a mouse: focus lands on the selected mode (an accidental
// Enter only reconfirms what is already selected and starts nothing), the
// up/down arrows move between modes, Tab moves on through the window, Esc
// means "Skip" without remembering. Steam Input maps the D-pad to arrows,
// A to Enter, B to Esc. Buttons and check boxes are at least 40 high
// (rule #16).
//
// Warning: THE FILE NAME. The overlay is called `FirstRunWizardOverlay`,
// not `FirstRunWizard`: the name `FirstRunWizard` in the `org.rexon.gui`
// module is already taken by the bridge itself (`#[qml_element]`), and a
// file with the same name would make the type ambiguous — "Ambiguous type
// detected. FirstRunWizard 1.0 is defined multiple times", after which the
// bridge cannot be reached from markup at all. The "bridge + overlay" pair
// is named the same way as for the tutorial: `OnboardingModel` and
// `OnboardingOverlay.qml`.
//
// Hook-up: `FirstRunWizardOverlay { anchors.fill: parent; pal: …;
// onFinished: <release the tutorial> }`, then `begin()`.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    /// Window skin palette (the same map as for the other pages).
    property var pal: ({})

    readonly property color colAccent: root.pal.accent

    /// The wizard has finished: the user installed Rexon, declined, or there
    /// was nothing to offer. The window host holds back the tutorial until
    /// this signal.
    signal finished()

    /// Whether the wizard is open — the welcome or the progress/result window.
    readonly property bool opened: welcomeDialog.visible || root.opPhase !== ""

    // ── Bridge ────────────────────────────────────────────────────────────

    FirstRunWizard {
        id: wizard
        objectName: "firstRunWizard"
    }

    readonly property string opPhase: (wizard.operation
                                       && wizard.operation.phase !== undefined)
                                      ? wizard.operation.phase : ""

    /// Welcome labels from the core: `{windowTitle, title, description,
    /// benefits, modesTitle, modes, installButton, skipButton,
    /// dontAskAgain, dontAskAgainHint}`.
    property var texts: ({})
    /// Labels of the consent "do not ask for a password for Rexon actions".
    property var consent: ({})
    /// Selected mode: "system" or "user".
    property string mode: ""
    /// Dry run: the wizard goes all the way through but does not touch the
    /// machine.
    property string dryNotice: ""
    /// The wizard is guiding the user right now (from the welcome until the
    /// result is closed).
    property bool active: false

    // ── Wizard flow ───────────────────────────────────────────────────────

    /// Ask the core and, if it offers installation, show the welcome. If there
    /// is nothing to offer — `finished()` at once, so the tutorial does not
    /// wait in vain.
    function begin() {
        if (root.active)
            return;
        const decision = wizard.start();
        // To the log: it shows why the wizard appeared or not.
        console.info("мастер установки: " + decision.reason);
        if (decision.kind !== "offer") {
            root.finished();
            return;
        }
        root.texts = decision.welcome;
        root.consent = decision.passwordless;
        root.dryNotice = decision.dryRunNotice !== undefined
                         ? decision.dryRunNotice : "";
        root.mode = decision.defaultMode;
        // Check boxes return to a safe state on every show: the "do not ask for a
        // password" permission is off (`defaultOn` from the core), "do not ask
        // again" is cleared.
        pwCheck.checked = root.consent.defaultOn === true;
        wizard.setPasswordlessRule(pwCheck.checked);
        dontAsk.checked = false;
        root.active = true;
        // The window always opens at the TOP: when shown a second time (the
        // button in "Settings"), the wizard remembered the previous scroll
        // position and greeted the user with the middle of the consent rather
        // than the welcome.
        flick.contentY = 0;
        welcomeDialog.open();
        Qt.callLater(root.settle);
    }

    /// Focus on the selected mode: the first thing the user is asked about.
    function settle() {
        const item = modes.itemAt(root.modeIndex(root.mode));
        if (item && item.enabled)
            item.forceActiveFocus();
        else
            installButton.forceActiveFocus();
    }

    /// Index of the mode in the core list; -1 means there is no such mode.
    function modeIndex(id) {
        const list = root.modeList();
        for (let i = 0; i < list.length; ++i)
            if (String(list[i].mode) === id)
                return i;
        return -1;
    }

    function modeList() {
        return (root.texts && root.texts.modes !== undefined)
               ? root.texts.modes : [];
    }

    /// Mode `id` was selected. An unavailable mode cannot be selected: its
    /// explanation is already shown under it, and there is no dead end to
    /// drive the user into.
    function selectMode(id) {
        const index = root.modeIndex(id);
        const list = root.modeList();
        if (index < 0 || String(list[index].blocked || "") !== "")
            return;
        root.mode = id;
    }

    /// "Install".
    function install() {
        if (!wizard.install(root.mode)) {
            console.warn("мастер установки: режим «" + root.mode
                         + "» не принят мостом");
            return;
        }
        welcomeDialog.close();
    }

    /// "Skip". The "do not ask again" check box goes into the settings.
    function skip() {
        if (!wizard.decline(dontAsk.checked))
            console.warn("мастер установки: отказ не сохранён, "
                         + "мастер предложится снова");
        welcomeDialog.close();
        root.done();
    }

    function done() {
        root.active = false;
        root.finished();
    }

    // The progress window was closed — the wizard has run to completion.
    onOpPhaseChanged: {
        if (root.active && root.opPhase === "" && !welcomeDialog.visible)
            root.done();
    }

    Connections {
        target: wizard

        // The "Install Rexon into the system…" button on the "Settings" page.
        function onShowRequested() { root.begin(); }

        function onInstallFinished(ok) {
            console.info("мастер установки: итог "
                         + (ok ? "успех" : "неудача"));
        }
    }

    // ── Progress and result: the shared operation window ──────────────────

    DeOperationDialog {
        anchors.fill: parent
        actions: wizard
        pal: root.pal
    }

    // ── Welcome ───────────────────────────────────────────────────────────

    Popup {
        id: welcomeDialog
        objectName: "firstRunWizardDialog"

        parent: root
        modal: true
        visible: false
        // Closed only with the buttons: an accidental click outside the window
        // must neither decline the installation nor fall through into the window
        // underneath.
        closePolicy: Popup.NoAutoClose
        focus: true
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 720)
        height: Math.min(root.height - 40, column.implicitHeight + 40)
        padding: 20

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: FocusScope {
            id: scope
            focus: true

            Keys.onEscapePressed: (event) => { event.accepted = true; root.skip(); }

            Flickable {
                id: flick
                anchors.fill: parent
                contentWidth: width
                contentHeight: column.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {
                    policy: flick.contentHeight > flick.height ? ScrollBar.AlwaysOn
                                                               : ScrollBar.AlwaysOff
                }

                // Room for the scroll bar is on its side: for right-to-left languages the
                // bar moves to the left edge, and the anchors with their margin are
                // mirrored along with it (a width of "minus the bar" from x = 0 would
                // leave the text under the bar).
                ColumnLayout {
                    id: column
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.rightMargin: flick.contentHeight > flick.height ? 14 : 0
                    spacing: 10

                    // ── Icon and title ────────────────────────────────────
                    //
                    // The icon is drawn with shapes, not with a picture: the page must stay
                    // self-contained, and icon files may not exist at all before
                    // installation.

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 14

                        Rectangle {
                            Layout.alignment: Qt.AlignTop
                            implicitWidth: 56
                            implicitHeight: 56
                            radius: 10
                            color: root.pal.bg_primary
                            border.color: root.pal.accent
                            border.width: 2

                            // A stylized desktop window: title bar and body.
                            Rectangle {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.margins: 12
                                y: 14
                                height: 7
                                radius: 2
                                color: root.pal.accent
                            }

                            Rectangle {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.margins: 12
                                y: 25
                                height: parent.height - 39
                                radius: 2
                                color: "transparent"
                                border.color: root.pal.accent
                                border.width: 1
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4

                            RexonText {
                                objectName: "firstRunWizardTitle"
                                Layout.fillWidth: true
                                text: root.texts.title !== undefined ? root.texts.title : ""
                                color: root.pal.text_primary
                                font.pointSize: 15
                                font.weight: Font.DemiBold
                                wrapMode: Text.WordWrap
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: root.texts.description !== undefined
                                      ? root.texts.description : ""
                                color: root.pal.text_primary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    // ── What installation gives ───────────────────────────

                    Repeater {
                        model: root.texts.benefits !== undefined ? root.texts.benefits : []

                        delegate: RowLayout {
                            id: benefitRow
                            required property string modelData

                            Layout.fillWidth: true
                            spacing: 8

                            Rectangle {
                                Layout.alignment: Qt.AlignTop
                                Layout.topMargin: 7
                                implicitWidth: 6
                                implicitHeight: 6
                                radius: 3
                                color: root.pal.accent
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: benefitRow.modelData
                                color: root.pal.text_secondary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }
                        }
                    }

                    // Dry run: otherwise "done" without a single write to disk would read as
                    // a real installation.
                    RexonText {
                        Layout.fillWidth: true
                        Layout.topMargin: 4
                        visible: root.dryNotice !== ""
                        text: root.dryNotice
                        color: root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                 : root.pal.accent
                        font.pointSize: 11
                        wrapMode: Text.WordWrap
                    }

                    // ── Whom to install for ───────────────────────────────

                    RexonText {
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        text: root.texts.modesTitle !== undefined ? root.texts.modesTitle : ""
                        color: root.pal.text_primary
                        font.pointSize: 12
                        font.weight: Font.DemiBold
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        id: modes
                        model: root.modeList()

                        delegate: Button {
                            id: modeCard
                            objectName: "firstRunWizardMode_" + modeCard.modeId

                            required property int index
                            required property var modelData

                            readonly property string modeId: String(modeCard.modelData.mode)
                            readonly property string blocked: String(modeCard.modelData.blocked || "")
                            readonly property bool chosen: root.mode === modeCard.modeId

                            Layout.fillWidth: true
                            // The mode choice is not a toggle button: the state is held by
                            // `root.mode`, otherwise a click would break the binding and two modes
                            // would remain selected.
                            enabled: modeCard.blocked === ""
                            implicitHeight: Math.max(40, modeBody.implicitHeight + 24)
                            padding: 12
                            onClicked: root.selectMode(modeCard.modeId)

                            Keys.onUpPressed: (event) => {
                                event.accepted = true;
                                const prev = modes.itemAt(modeCard.index - 1);
                                if (prev && prev.enabled)
                                    prev.forceActiveFocus();
                            }
                            Keys.onDownPressed: (event) => {
                                event.accepted = true;
                                const next = modes.itemAt(modeCard.index + 1);
                                if (next && next.enabled)
                                    next.forceActiveFocus();
                                else if (pwCheck.visible)
                                    pwCheck.forceActiveFocus();
                                else
                                    installButton.forceActiveFocus();
                            }

                            background: Rectangle {
                                radius: 6
                                color: modeCard.chosen
                                       ? Qt.rgba(root.colAccent.r, root.colAccent.g,
                                                 root.colAccent.b, 0.18)
                                       : (modeCard.hovered ? root.pal.bg_card_hover
                                                           : root.pal.bg_primary)
                                border.width: (modeCard.activeFocus || modeCard.chosen) ? 2 : 1
                                border.color: modeCard.activeFocus
                                              ? root.colAccent
                                              : (modeCard.chosen ? root.colAccent
                                                                 : root.pal.border)
                            }

                            contentItem: ColumnLayout {
                                id: modeBody
                                spacing: 4

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 10

                                    // Selection dot: it shows that the option is one of two.
                                    Rectangle {
                                        Layout.alignment: Qt.AlignTop
                                        Layout.topMargin: 3
                                        implicitWidth: 18
                                        implicitHeight: 18
                                        radius: 9
                                        color: "transparent"
                                        border.width: 2
                                        border.color: modeCard.enabled ? root.colAccent
                                                                       : root.pal.border

                                        Rectangle {
                                            anchors.centerIn: parent
                                            visible: modeCard.chosen
                                            width: 8
                                            height: 8
                                            radius: 4
                                            color: root.colAccent
                                        }
                                    }

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: String(modeCard.modelData.title)
                                        color: modeCard.enabled ? root.pal.text_primary
                                                                : root.pal.text_disabled
                                        font.pointSize: 12
                                        font.weight: Font.DemiBold
                                        wrapMode: Text.WordWrap
                                    }
                                }

                                RexonText {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 28
                                    text: String(modeCard.modelData.description)
                                    color: modeCard.enabled ? root.pal.text_secondary
                                                            : root.pal.text_disabled
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }

                                // Why a mode is unavailable — right under it, not through the silence of
                                // a disabled button.
                                RexonText {
                                    Layout.fillWidth: true
                                    Layout.leftMargin: 28
                                    visible: modeCard.blocked !== ""
                                    text: modeCard.blocked
                                    color: root.pal.state_warn !== undefined
                                           ? root.pal.state_warn : root.pal.accent
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }
                            }

                            // The path to the files is a detail, not a label: its place is in a
                            // tooltip (the core decided so).
                            ToolTip.visible: modeCard.hovered && modeCard.hint !== ""
                            ToolTip.text: modeCard.hint
                            ToolTip.delay: 600
                            readonly property string hint: String(modeCard.modelData.hint || "")
                        }
                    }

                    // ── Do not ask for a password for Rexon actions ───────
                    //
                    // A separate consent, OFF by default: the permission removes password
                    // protection, and only the user can turn it on, after reading what
                    // exactly is being allowed. The step exists only for installation for all
                    // users — with installation "just for you" the permission is not set.

                    Rectangle {
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        visible: root.mode === "system" && root.consent.title !== undefined
                        implicitHeight: consentBody.implicitHeight + 24
                        radius: 6
                        color: root.pal.bg_primary
                        border.color: root.pal.border
                        border.width: 1

                        ColumnLayout {
                            id: consentBody
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 12
                            spacing: 4

                            RexonText {
                                Layout.fillWidth: true
                                text: root.consent.title !== undefined ? root.consent.title : ""
                                color: root.pal.text_primary
                                font.pointSize: 12
                                font.weight: Font.DemiBold
                                wrapMode: Text.WordWrap
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: root.consent.description !== undefined
                                      ? root.consent.description : ""
                                color: root.pal.text_secondary
                                font.pointSize: 11
                                wrapMode: Text.WordWrap
                            }

                            Repeater {
                                model: [root.consent.what, root.consent.who,
                                        root.consent.risk, root.consent.undo]

                                delegate: RexonText {
                                    id: consentLine
                                    required property var modelData

                                    Layout.fillWidth: true
                                    visible: consentLine.modelData !== undefined
                                             && String(consentLine.modelData) !== ""
                                    text: consentLine.modelData !== undefined
                                          ? String(consentLine.modelData) : ""
                                    color: root.pal.text_secondary
                                    font.pointSize: 11
                                    wrapMode: Text.WordWrap
                                }
                            }

                            RexonCheckBox {
                                id: pwCheck
                                objectName: "firstRunWizardPasswordless"
                                Layout.fillWidth: true
                                Layout.topMargin: 4
                                pal: root.pal
                                text: root.consent.checkbox !== undefined
                                      ? root.consent.checkbox : ""
                                onToggled: wizard.setPasswordlessRule(pwCheck.checked)
                                ToolTip.visible: pwCheck.hovered
                                                 && root.consent.hint !== undefined
                                                 && root.consent.hint !== ""
                                ToolTip.text: root.consent.hint !== undefined
                                              ? root.consent.hint : ""
                                ToolTip.delay: 600
                                Keys.onDownPressed: (event) => {
                                    event.accepted = true;
                                    dontAsk.forceActiveFocus();
                                }
                            }
                        }
                    }

                    // ── Answer ────────────────────────────────────────────

                    RexonCheckBox {
                        id: dontAsk
                        objectName: "firstRunWizardDontAsk"
                        Layout.fillWidth: true
                        Layout.topMargin: 6
                        pal: root.pal
                        text: root.texts.dontAskAgain !== undefined
                              ? root.texts.dontAskAgain : ""
                        ToolTip.visible: dontAsk.hovered
                                         && root.texts.dontAskAgainHint !== undefined
                        ToolTip.text: root.texts.dontAskAgainHint !== undefined
                                      ? root.texts.dontAskAgainHint : ""
                        ToolTip.delay: 600
                    }

                    RexonText {
                        Layout.fillWidth: true
                        text: root.texts.dontAskAgainHint !== undefined
                              ? root.texts.dontAskAgainHint : ""
                        color: root.pal.text_secondary
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.topMargin: 10
                        spacing: 8

                        Item { Layout.fillWidth: true }

                        RexonButton {
                            id: skipButton
                            objectName: "firstRunWizardSkip"
                            pal: root.pal
                            text: root.texts.skipButton !== undefined
                                  ? root.texts.skipButton : ""
                            onClicked: root.skip()
                            KeyNavigation.right: installButton
                            Keys.onReturnPressed: clicked()
                        }

                        RexonButton {
                            id: installButton
                            objectName: "firstRunWizardInstall"
                            pal: root.pal
                            tone: root.pal.primary_tone !== undefined
                                  ? root.pal.primary_tone : null
                            text: root.texts.installButton !== undefined
                                  ? root.texts.installButton : ""
                            onClicked: root.install()
                            KeyNavigation.left: skipButton
                            Keys.onReturnPressed: clicked()
                        }
                    }
                }
            }
        }
    }
}
