// Dialogs behind the main window's menu items: the about box, the
// question before resetting the KDE theme and the Rexon uninstall window.
//
// In the product their place is taken by `QMessageBox` (`window.py`:
// `_on_about`, `_on_recover_kde`, `_on_uninstall`). There are no widgets
// in this process, so here we use a QtQuick Controls `Dialog` in the
// product skin (`MenuDialog` below) — but the text in them is THE SAME
// and comes entirely from `WindowMenuModel`, i.e. from the core. Not a
// single human-readable string is typed in this file: a second copy would
// silently drift away from the product (rule #12).
//
// Warning: THE ORDER OF THE UNINSTALL BUTTONS AND THE DEFAULT BUTTON ARE
// NOT COSMETICS. "Cancel" is the default: an accidental Enter must not
// wipe the program off the customer's machine. That is how it is in the
// product and here, and the decision is made by the core (`defaultId`),
// not by the markup.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Item {
    id: root

    /// The Rust menu object — source of all texts and executor of actions.
    required property var menuModel

    /// Skin palette.
    required property var pal

    /// The user chose "Quit" or the uninstall finished successfully (in the
    /// product the window closes by itself after uninstalling).
    signal quitRequested()

    /// Which dialog is open right now — the window run uses this field to
    /// check that a menu item leads somewhere. Empty means no dialog is open.
    ///
    /// Warning: IT IS SET IN `onAboutToShow`, NOT IN `onOpened`, AND SO IS THE
    /// FOCUS. `opened` arrives AFTER the appearance animation, and not every
    /// style has one: Basic and Fusion do not, `org.kde.desktop` (the style Qt
    /// picks in a Plasma session) does. Measurement on 15.09.2026, a
    /// `--selftest` run under xvfb: with the `org.kde.desktop` style all three
    /// menu items "led nowhere", and the focus in the uninstall window was NOT
    /// on "Cancel"; with Basic and offscreen everything was green. For a
    /// customer on KDE this meant that for the whole animation Enter pressed
    /// something other than "Cancel".
    property string openDialog: ""

    // ── Door from the menu ────────────────────────────────────────────────
    //
    // Dispatch goes by the item identifier, not by the label: the label is
    // translated and changes, the identifier does not.
    function activate(id) {
        if (id === "about") {
            aboutDialog.open();
        } else if (id === "recover_kde") {
            // First — packages replacing parts of Plasma: a theme reset will not
            // remove them, and the user must learn this BEFORE the reset, not from an
            // output stream they cannot see.
            const found = root.menuModel.recoverForks();
            if (found.forks.length > 0) {
                console.info("Plasma replacement packages found: " + found.forks.join(", "));
                forksDialog.title = found.title;
                forksBody.text = found.text;
                forksRemoveButton.text = found.removeLabel;
                forksThemeButton.text = found.themeLabel;
                forksCancelButton.text = found.cancelLabel;
                forksDialog.open();
            } else {
                root.askRecover();
            }
        } else if (id === "uninstall") {
            const prompt = root.menuModel.uninstallPrompt();
            uninstallDialog.title = prompt.title;
            uninstallHead.text = prompt.text;
            uninstallBody.text = prompt.informative;
            keepButton.text = prompt.keepLabel;
            deleteButton.text = prompt.deleteLabel;
            cancelButton.text = prompt.cancelLabel;
            uninstallDialog.open();
        } else if (id === "quit") {
            root.quitRequested();
        } else {
            // An item with nothing behind it is a promise nobody will keep. Let it be
            // visible in the log.
            console.warn("menu item \"" + id + "\" leads nowhere");
        }
    }

    /// Close whichever dialog is open now. Needed by the `--selftest` run: it
    /// checks ONE BY ONE that every menu item leads somewhere, and an unclosed
    /// modal dialog would cover the next one.
    function closeOpenDialog() {
        aboutDialog.close();
        forksDialog.close();
        forksConfirmDialog.close();
        recoverDialog.close();
        uninstallDialog.close();
        noticeDialog.close();
    }

    /// A dialog closed. "Open" may be reset only if NO other dialog is
    /// visible: `onAccepted` fires BEFORE closing, and a message opened from it
    /// would otherwise count as closed (caught by the test bench on 11.09.2026
    /// — "Yes" → "Uninstalling" gave `openDialog === ""`). The same happens in
    /// styles with a closing animation, where `closed` arrives after the next
    /// window's `opened`.
    function dialogClosed() {
        for (const d of [aboutDialog, forksDialog, forksConfirmDialog,
                         recoverDialog, uninstallDialog, noticeDialog])
            if (d.visible)
                return;
        root.openDialog = "";
    }

    /// Question before the theme reset. On a foreign desktop there is an
    /// explanation instead of a question: the route would refuse there, and
    /// promising "started" is not allowed.
    function askRecover() {
        const prompt = root.menuModel.recoverPrompt();
        if (!prompt.allowed) {
            root.say(prompt.title, prompt.refusal);
            return;
        }
        recoverDialog.title = prompt.title;
        recoverBody.text = prompt.text;
        recoverDialog.open();
    }

    /// A button of the window about replacement packages was pressed. What to
    /// do next is decided by the core (`forksNext`) — the same decision as in
    /// the Python window.
    function forksChoice(buttonId) {
        forksDialog.close();
        const next = root.menuModel.forksNext(buttonId);
        if (next === "confirm_removal")
            root.askForksRemoval();
        else if (next === "theme_question")
            root.askRecover();
    }

    /// Last question before removing the replacements: exactly the command
    /// that will be run. If there is nothing to restore the standard packages
    /// with, an explanation is shown instead of a question, and nothing is
    /// started.
    function askForksRemoval() {
        const preview = root.menuModel.forksPreview();
        if (!preview.ok) {
            root.say(preview.title, preview.text);
            return;
        }
        forksConfirmDialog.title = preview.title;
        forksConfirmBody.text = preview.text;
        forksConfirmDialog.runningTitle = preview.runningTitle;
        forksConfirmDialog.runningText = preview.runningText;
        forksConfirmDialog.open();
    }

    /// Whether the default button of the open uninstall window holds the
    /// keyboard focus. The yardstick of foolproofing: an accidental Enter must
    /// not wipe the program off the customer's machine, and this cannot be
    /// checked by pressing — get it wrong and the program is removed for real.
    function uninstallDefaultHasFocus() {
        return cancelButton.activeFocus;
    }

    /// Labels of the three buttons of the uninstall window — as they stand
    /// RIGHT NOW. They are labelled when the window opens, and an empty label
    /// here would mean a nameless button with the program's removal behind it.
    function uninstallButtonTexts() {
        return [keepButton.text, deleteButton.text, cancelButton.text];
    }

    /// Labels of the uninstall window buttons that did NOT fit into their
    /// button and are shown elided. The "keep" label is long on purpose (it
    /// says what will be kept), and "Uninstall, but ke…" would hide exactly
    /// what it is that long for.
    function uninstallClippedLabels() {
        const out = [];
        for (const b of [keepButton, deleteButton, cancelButton])
            if (b.implicitContentWidth > b.availableWidth + 0.5)
                out.push(b.text + " (" + Math.round(b.availableWidth) + " of "
                         + Math.ceil(b.implicitContentWidth) + ")");
        return out;
    }

    /// Show a simple message (the result of a theme reset or of an uninstall).
    function say(title, text) {
        noticeDialog.title = title;
        noticeBody.text = text;
        noticeDialog.open();
    }

    // ── Common look ───────────────────────────────────────────────────────
    //
    // Warning: ALL WINDOWS OF THIS FILE ARE BUILT FROM THE THREE PARTS BELOW,
    // AND THEIR LOOK IS FIXED HERE, NOT IN EVERY WINDOW (rule #12). Caught on
    // 30.09.2026 in the uninstall window: the stock `Dialog` of the Basic
    // style drew a white header and light buttons over the dark application
    // (steam_dark skin, rule #17), and the explanation, put into a
    // `ScrollView` without a width, did not wrap at all — lines were cut on
    // the right ("…and the des"), and the user could not read to the end what
    // would be removed.

    /// Window text. `Text.Wrap` breaks at words, and a word longer than the
    /// line (a path without spaces) wherever necessary: otherwise the path
    /// would run past the edge.
    component Body: RexonLinkText {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        // Product texts are rich text (`<b>`, `<br>`, `<pre>`, `<h2>`), and
        // showing them as plain text would show the markup. Links go through the
        // shared path (↗ and a question before an external site).
        pal: root.pal
        color: root.pal.text_primary
    }

    /// Window body that WRAPS to the window width and scrolls when it does not
    /// fit in height: the window together with its buttons must fit on a
    /// 1280×800 screen (Steam Deck), otherwise "Cancel" is not visible.
    ///
    /// Warning: THE TEXT WIDTH IS SET EXPLICITLY. The content of a `ScrollView`
    /// is the content of a `Flickable`; layouts do not apply to it, and
    /// `Layout.fillWidth` means nothing there: without `width` the text took
    /// the width of its longest line and did not wrap.
    component ScrollBody: ScrollView {
        id: scrollBody

        property alias text: scrollText.text
        property alias textFormat: scrollText.textFormat
        /// Markup with links: `RexonLinkText` puts ↗ on external ones.
        property alias html: scrollText.html

        Layout.fillWidth: true
        Layout.preferredHeight: Math.min(scrollText.implicitHeight,
                                         Math.max(120, root.Window.height - 320))
        contentWidth: availableWidth
        clip: true
        // The bar is visible all the time while there is something to scroll:
        // otherwise the Basic style shows it only during scrolling, and the user
        // does not know there is more text below the last visible line.
        ScrollBar.vertical.policy: contentHeight > height ? ScrollBar.AlwaysOn
                                                          : ScrollBar.AsNeeded

        Body {
            id: scrollText
            // Room under the scroll bar is always kept: the Basic style bar is drawn
            // over the content and would cover the line ends, and a width depending
            // on its visibility would close the loop "width → height → bar → width".
            // The width comes from anchors, the room for the bar from a margin on its
            // side: for right to left the bar moves to the left, and mirroring moves
            // the margin there too.
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.rightMargin: 14
        }
    }

    /// Row of window buttons. Each button gets its own width: without
    /// alignment the Basic and Fusion styles divide the row into equal shares,
    /// and a long label would be cut even though there is room next to it.
    /// Standard buttons (`standardButtons`) are built by a delegate — the same
    /// `RexonButton` as in the rest of the product.
    component Buttons: DialogButtonBox {
        alignment: Qt.AlignRight
        spacing: 8
        padding: 20
        topPadding: 12
        background: Item {}
        delegate: RexonButton { pal: root.pal }
    }

    /// A window in the product skin — the same look as the desktop operation
    /// window (`DeOperationDialog`): a card background, dimming under the
    /// window, a large-font title, skin buttons.
    component MenuDialog: Dialog {
        id: menuDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        padding: 20
        topPadding: 10

        background: Rectangle {
            color: root.pal.bg_card
            border.color: root.pal.border
            border.width: 1
            radius: 8
        }

        // Dimming under a modal window is a product-wide decision
        // (`ui_base::MODAL_DIM_RGBA`, key `modal_dim`). The Basic style does not
        // dim the background, and the window was modal in fact but not in looks.
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        header: RexonText {
            text: menuDialog.title
            color: root.pal.text_primary
            font.pointSize: 14
            font.weight: Font.DemiBold
            wrapMode: Text.WordWrap
            padding: 20
            bottomPadding: 0
        }

        footer: Buttons {}
    }

    // ── "About" ───────────────────────────────────────────────────────────

    MenuDialog {
        id: aboutDialog
        // Title and body come from the core; the body is computed once on opening.
        title: root.menuModel.aboutTitle()
        width: Math.min(root.Window.width - 80, 620)
        standardButtons: Dialog.Ok
        onClosed: root.dialogClosed()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody {
                id: aboutBody
                html: root.menuModel.aboutHtml()
            }

            // The Rexon log: where it is, open it, copy its tail — so it can be sent
            // to the developer without hunting for the file on disk. Texts and path
            // come from the core (`window_menu::log_*`, `applog`).
            Body {
                id: logLocation
                Layout.fillWidth: true
                text: aboutDialog.logInfo.location || ""
                font.pointSize: 9
            }

            RowLayout {
                spacing: 8

                RexonButton {
                    pal: root.pal
                    text: aboutDialog.logInfo.openLabel || ""
                    onClicked: Qt.openUrlExternally(aboutDialog.logInfo.url)
                }

                RexonButton {
                    pal: root.pal
                    text: aboutDialog.logInfo.copyLabel || ""
                    onClicked: {
                        logClipboard.text = root.menuModel.logTail();
                        logClipboard.selectAll();
                        logClipboard.copy();
                        logClipboard.text = "";
                        logLocation.text = aboutDialog.logInfo.copied;
                    }
                }
            }

            // Clipboard from QML goes through an invisible field: QML has no
            // clipboard API of its own, and `TextEdit.copy()` puts the selection
            // there.
            TextEdit {
                id: logClipboard
                visible: false
                Layout.preferredWidth: 0
                Layout.preferredHeight: 0
            }
        }

        /// Log details — once when the about box opens.
        property var logInfo: ({})
        onAboutToShow: {
            root.openDialog = "about";
            logInfo = root.menuModel.logInfo();
            logLocation.text = logInfo.location;
        }
    }

    // ── "Restore KDE theme": packages replacing parts of Plasma ───────────
    //
    // A port of `_maybe_handle_plasma_forks`: three choices — remove the
    // replacements, only reset the theme, cancel. For the window run this is
    // the same "recover_kde" door as the question about the theme reset.

    MenuDialog {
        id: forksDialog
        width: Math.min(root.Window.width - 80, 640)
        onClosed: root.dialogClosed()

        onAboutToShow: {
            root.openDialog = "recover_kde";
            // The default button is removing the replacements, as in the product:
            // there is one more question behind it, and there the default is "No".
            forksRemoveButton.forceActiveFocus();
        }
        // A second time — after the animation: `DialogButtonBox` hands out focus
        // by itself when the window is shown (see the uninstall window).
        onOpened: forksRemoveButton.forceActiveFocus()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: forksBody }
        }

        footer: Buttons {
            RexonButton {
                id: forksRemoveButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
                onClicked: root.forksChoice("remove")
            }

            RexonButton {
                id: forksThemeButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.ActionRole
                onClicked: root.forksChoice("theme")
            }

            RexonButton {
                id: forksCancelButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
                onClicked: root.forksChoice("cancel")
            }
        }
    }

    MenuDialog {
        id: forksConfirmDialog

        /// What to say while the removal is running; arrives together with the
        /// question.
        property string runningTitle: ""
        property string runningText: ""

        width: Math.min(root.Window.width - 80, 640)
        standardButtons: Dialog.Yes | Dialog.No
        onClosed: root.dialogClosed()

        // "No" is the default: behind "Yes" is the removal of system packages.
        onAboutToShow: {
            root.openDialog = "recover_kde";
            const no = forksConfirmDialog.standardButton(Dialog.No);
            if (no !== null)
                no.forceActiveFocus();
        }

        onAccepted: {
            if (root.menuModel.forksRemoveRun())
                root.say(forksConfirmDialog.runningTitle, forksConfirmDialog.runningText);
            else
                console.warn("removing the Plasma replacements is already in progress");
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: forksConfirmBody }
        }
    }

    // ── "Restore KDE theme" ───────────────────────────────────────────────

    MenuDialog {
        id: recoverDialog
        width: Math.min(root.Window.width - 80, 620)
        standardButtons: Dialog.Yes | Dialog.No
        onClosed: root.dialogClosed()

        // "No" is the default: a theme reset stops live wallpapers, and an
        // accidental Enter must not do that.
        onAboutToShow: {
            root.openDialog = "recover_kde";
            const no = recoverDialog.standardButton(Dialog.No);
            if (no !== null)
                no.forceActiveFocus();
        }

        onAccepted: {
            const done = root.menuModel.recoverApply();
            root.say(done.title, done.text);
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody { id: recoverBody }
        }
    }

    // ── "Uninstall Rexon" ─────────────────────────────────────────────────

    MenuDialog {
        id: uninstallDialog
        // Wider than usual if the buttons require it: the "keep" label names what
        // will be kept, and it must not be cut.
        width: Math.min(root.Window.width - 80,
                        Math.max(680, uninstallButtons.implicitWidth + leftPadding + rightPadding))
        onClosed: root.dialogClosed()

        onAboutToShow: {
            root.openDialog = "uninstall";
            // Before the appearance animation (see `openDialog`): an Enter pressed
            // while the window slides in must land on "Cancel".
            cancelButton.forceActiveFocus();
        }

        onOpened: {
            // Warning: FOCUS IS SET EXPLICITLY, AND ANEW ON EVERY OPENING. A single
            // `focus: true` on the button is not enough: `DialogButtonBox` is a
            // container and hands out focus by itself when the window is shown.
            // Caught by a `--selftest` run on 09.09.2026 (the line
            // `REXON-MAINWINDOW-MENU-UNINSTALL-FOCUS`): focus went NOT to "Cancel",
            // i.e. the foolproofing that the product holds with
            // `setDefaultButton(Cancel)` did not work here at all.
            cancelButton.forceActiveFocus();
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            Body {
                id: uninstallHead
                font.bold: true
            }

            // The list of what will be removed may be long, and the question window
            // must fit on the screen together with its buttons: otherwise "Cancel" is
            // not visible.
            ScrollBody { id: uninstallBody }
        }

        footer: Buttons {
            id: uninstallButtons

            RexonButton {
                id: keepButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.AcceptRole
                onClicked: root.startUninstall(true)
            }

            // Irreversible — in red, like "Delete" of the desktop operation window.
            RexonButton {
                id: deleteButton
                pal: root.pal
                tone: root.pal.destructive_tone !== undefined ? root.pal.destructive_tone : null
                DialogButtonBox.buttonRole: DialogButtonBox.DestructiveRole
                onClicked: root.startUninstall(false)
            }

            RexonButton {
                id: cancelButton
                pal: root.pal
                DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
                // Default button: foolproofing.
                focus: true
                onClicked: uninstallDialog.close()
            }
        }
    }

    /// Start the uninstall and close the question. The result will arrive as a
    /// signal — `pkexec` involves a password prompt, and waiting for it in the
    /// interface thread is not allowed. `keepProfile` means "keep settings and
    /// backups".
    function startUninstall(keepProfile) {
        uninstallDialog.close();
        if (!root.menuModel.uninstallRun(keepProfile))
            console.warn("uninstalling is already in progress");
    }

    Connections {
        target: root.menuModel

        function onUninstallFinished(ok, title, message) {
            root.say(title, message);
            if (ok)
                root.quitRequested();
        }

        // Result of removing the replacements. The window does not close: a new
        // login is needed next, and the result itself says so.
        function onForksRemoveFinished(ok, title, message) {
            root.say(title, message);
        }
    }

    // ── Simple message ────────────────────────────────────────────────────

    MenuDialog {
        id: noticeDialog
        width: Math.min(root.Window.width - 80, 560)
        standardButtons: Dialog.Ok
        onAboutToShow: root.openDialog = "notice"
        onClosed: root.dialogClosed()

        ColumnLayout {
            anchors.fill: parent
            spacing: 8

            ScrollBody {
                id: noticeBody
                // Results arrive as plain text with line breaks, not as markup: `\n` in
                // rich text would be glued into a single line.
                textFormat: Text.PlainText
            }
        }
    }
}
