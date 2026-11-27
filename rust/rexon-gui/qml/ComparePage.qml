// The "DE comparison" tab in QML. Data and decisions come from the Rust
// object CompareModel (rust/rexon-gui/src/compare_page.rs), which decides
// nothing itself but asks the core (`rexon_core::qt_compare`). Only MARKUP
// lives here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from
// `ui_base::skin_colors("steam_dark")` — the default skin of the whole
// product (rule #17). The page defines no colours of its own: a second
// palette in .qml would silently set the tab apart from the rest of Rexon.
//
// Side actions (installation via pkexec, session switching) live outside:
// the page host puts an object with install/switchTo methods into
// `root.actions`, and they return "did it work". For now these steps are
// in Python (rexon/qt_compare/window.py); after moving away from PySide6 a
// Rust object with the same two methods will take their place — the page
// will not change.
//
// The focus chain is built in from the very start (the controller
// decision: controller support itself is postponed, focus in QML is
// always there): all buttons are reachable with Tab, the focused one shows
// a frame, and the page scrolls to it by itself.

// Delegates refer to ids of outer elements (root, page). Without this
// pragma, QML lint rightly complains about unqualified access, and Qt
// resolves such names only through the deprecated delegate context.
// (A "// qmllint …" line must not be written here: it is read as a
// directive.)
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Bridge to the outside ─────────────────────────────────────────────

    // Side-actions object; set by the page host.
    // Methods: install(deId) -> bool, switchTo(deId) -> bool.
    property var actions: null

    // For the window host: the title and minimum size live in the core, not in
    // a Python or QML copy.
    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    // The type name is CompareModel, not ComparePage: the latter is taken by
    // this very file (in a QML module every .qml is a type named after the
    // file).
    CompareModel {
        id: page
        // The name is needed by checks: from the page host the model is fetched
        // with `root.findChild(QObject, "compareModel")` and queried without a
        // single change in the page itself.
        // Warning: only invokables WITHOUT numbers in their signature are called
        // from outside (`refreshInstalled`, `pairView`, `displayName`): for `i32`
        // the metaobject has the name `::std::int32_t`, there is no metatype with
        // that name, and `QMetaObject.invokeMethod("toggle", …)` silently returns
        // False. Expanding a card has to be checked with keys (Tab to the header,
        // Space) — that is, the same way a customer does it.
        objectName: "compareModel"
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    // In the widgets the card is painted not by the palette but by a
    // translucent style sheet overlay (`qt_compare::card_qss`); the core lays
    // it over the page background and hands out the ready colour. The
    // `bg_card` token was similar but not the same: (30,45,61) versus
    // (36,48,64) over 1 712 261 pixels.
    readonly property color colCard: root.pal.surface_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // ── Side actions ──────────────────────────────────────────────────────

    // Installation runs synchronously: confirmation, progress and result are
    // shown by the host, and only "did it work" comes back here. The page
    // recomputes the set of desktops — otherwise the card would keep offering
    // "Install" for an already installed desktop (a fix carried over from the
    // widget version).
    function runInstall(deId) {
        if (!root.actions)
            return;
        if (root.actions.install(deId))
            page.refreshInstalled();
    }

    function runSwitch(deId) {
        if (!root.actions)
            return;
        root.actions.switchTo(deId);
    }

    // Scroll the page to the element that has just received focus: without
    // this Tab takes focus past the bottom edge, and from the keyboard (and
    // tomorrow from a gamepad) the tab looks frozen.
    function ensureVisible(item) {
        if (!item || !flick.contentItem)
            return;
        // The window became active again (the screenshot dialog from Print was
        // closed, the user returned from another program) — Qt gives focus back
        // to the same element. This is not a Tab move: the page must not scroll
        // to it, otherwise a tab scrolled down jumps to the top.
        if (item.focusReason === Qt.ActiveWindowFocusReason)
            return;
        const pos = item.mapToItem(flick.contentItem, 0, 0);
        const top = pos.y;
        const bottom = pos.y + item.height;
        const maxY = Math.max(0, flick.contentHeight - flick.height);
        if (top < flick.contentY)
            flick.contentY = Math.max(0, top - 12);
        else if (bottom > flick.contentY + flick.height)
            flick.contentY = Math.min(maxY, bottom - flick.height + 12);
    }

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Page ──────────────────────────────────────────────────────────────

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: pageColumn.implicitHeight + 48
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {}

        // Warning: a layout inside an ORDINARY Item (not inside another layout)
        // must get its height by hand: by itself it stays at zero, and the page
        // collapses into a strip.
        ColumnLayout {
            id: pageColumn
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: 24
            y: 24
            height: implicitHeight
            spacing: 14

            // ── Header: title and quick pairs ─────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }

                // The widget tab pushes the quick pairs to the RIGHT edge of the window
                // instead of placing them right next to the title: the spacer keeps the
                // same look.
                Item {
                    Layout.fillWidth: true
                    Layout.preferredWidth: 0
                }

                RexonText {
                    Layout.leftMargin: 16
                    text: page.pairsLabel
                    color: root.colTextSecondary
                    // Secondary text in the widget tab is set bolder (`font-weight: 600` from
                    // `secondary_text_qss`); the number arrives from the core with the
                    // palette, the page defines none of its own.
                    font.weight: root.pal.text_secondary_weight
                }

                // The pair buttons are in a Flow, not in the row itself: at the minimum
                // window width (900, a number from the core) four buttons do not fit in
                // one row, and the last one slid past the right edge. Flow wraps the
                // extra one onto a second line instead of cutting it.
                Flow {
                    id: pairsFlow

                    // Width of the button row if it fitted on one line. Computed from the
                    // buttons rather than asked from the Flow itself: a positioner's
                    // implicitWidth depends on its own width, and binding one through the
                    // other would loop.
                    property real naturalWidth: 0

                    function measure() {
                        let total = 0;
                        let shown = 0;
                        for (let i = 0; i < pairsFlow.children.length; i++) {
                            const item = pairsFlow.children[i];
                            // Warning: the Repeater itself also counts as a child of the Flow, and it
                            // is invisible with zero width. While it was counted along with the
                            // buttons, an EXTRA gap went into the width cap, and the whole pairs block
                            // moved 8 pixels away from the right edge of the window, although in the
                            // original it is flush (measurement 02.09.2026: right edge 1367 versus
                            // 1374).
                            if (!item.visible || item.implicitWidth <= 0)
                                continue;
                            // Each width is rounded UP separately. A button width is fractional, and
                            // Flow wraps by comparing fractional sums: a cap computed exactly turned
                            // out a hundredth of a pixel short, and the last pair moved to the second
                            // line for no reason.
                            total += Math.ceil(item.implicitWidth);
                            shown += 1;
                        }
                        pairsFlow.naturalWidth = shown > 1
                            ? total + pairsFlow.spacing * (shown - 1)
                            : total;
                    }

                    Layout.fillWidth: true
                    // The width cap is the button row itself: the spacer on the left takes
                    // the extra room, and the block stays on the right. Below the cap the
                    // Flow shrinks and wraps a button.
                    Layout.maximumWidth: pairsFlow.naturalWidth
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 8

                    Repeater {
                        id: pairRepeater
                        model: page.quickPairs

                        onItemAdded: pairsFlow.measure()
                        onItemRemoved: pairsFlow.measure()

                        delegate: RexonButton {
                            id: pairButton
                            required property var modelData
                            pal: root.pal
                            text: pairButton.modelData.label
                            onClicked: root.openPair(pairButton.modelData.a,
                                                     pairButton.modelData.b)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(pairButton)
                        }
                    }

                    Component.onCompleted: pairsFlow.measure()
                }
            }

            // SteamOS (Steam Deck and Steam Machine): which desktops can be installed
            // is shown as a separate line under the header, across the full width (in
            // the header row the phrase overlapped the quick pairs). Why the button
            // of the others is disabled is a line in their card.
            RexonText {
                Layout.fillWidth: true
                visible: page.installNoteText !== ""
                text: page.installNoteText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
            }

            // ── "How to read the table" legend ────────────────────────────
            Rectangle {
                Layout.fillWidth: true
                color: root.colCard
                border.color: root.colBorder
                border.width: 1
                radius: 6
                // A Rectangle's border is drawn INSIDE, while a QSS widget border is
                // outside the padding. So its pixel at the top and bottom has to be added
                // by hand, otherwise the block is two pixels lower than the original, and
                // by the bottom of the page the error adds up as many times as there are
                // blocks (measurement 02.09.2026).
                implicitHeight: legendColumn.implicitHeight + 22

                ColumnLayout {
                    id: legendColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: 13
                    y: 11
                    height: implicitHeight
                    // The gap between the legend label and its body in the widget tab is set
                    // not by code but by the style (Breeze gives 6): the `QVBoxLayout` there
                    // is left with its default.
                    spacing: 6

                    RexonText {
                        Layout.fillWidth: true
                        text: page.legendTitle
                        textFormat: Text.RichText
                        color: root.colText
                    }

                    RexonText {
                        Layout.fillWidth: true
                        text: page.legendBody
                        textFormat: Text.RichText
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }
                }
            }

            // ── Desktop table header ──────────────────────────────────────
            RexonText {
                text: page.tableHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // ── Desktop cards ─────────────────────────────────────────────
            Repeater {
                model: page

                delegate: Rectangle {
                    id: card

                    required property int index
                    required property string deId
                    required property bool installed
                    required property bool expanded
                    required property string headerText
                    required property string ramLabel
                    required property int ramMax
                    required property int ramValue
                    required property string ramColor
                    required property var starsLabels
                    required property var specs
                    required property string description
                    required property string actionText
                    required property bool actionAccent
                    required property string actionKind
                    // SteamOS: why "Install" of this desktop is disabled; empty means it is
                    // enabled (or the button is not "Install").
                    readonly property string installBlocked: page.installBlockedText(card.deId)

                    Layout.fillWidth: true
                    color: root.colCard
                    border.color: root.colBorder
                    border.width: 1
                    radius: 6
                    // Plus two pixels of border — the same correction as for the legend: in
                    // the original the card border lies outside the content, in a Rectangle
                    // inside.
                    implicitHeight: cardColumn.implicitHeight + 2

                    ColumnLayout {
                        id: cardColumn
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: 1
                        y: 1
                        height: implicitHeight
                        spacing: 0

                        // The card header is a whole clickable row, as in the original (there it
                        // was a full-width QPushButton).
                        Button {
                            id: cardHeader
                            // The name is needed by checks: there is no other way to expand the card
                            // from outside — `toggle(i32)` cannot be called through
                            // `QMetaObject.invokeMethod` (see the note at the model), and the header
                            // is clicked by sending the same signal a human press sends.
                            objectName: "cardHeader"
                            Layout.fillWidth: true
                            flat: true
                            // The expand arrow is set by the page, not the core: a collapsed one
                            // points to the end of the line. For right to left the header starts with
                            // an RLM mark: otherwise a desktop name in Latin letters would make the
                            // line left-to-right, and "◀" would end up in the middle of the card
                            // instead of at its start.
                            readonly property RexonArrows arrows: RexonArrows {
                                mirrored: cardHeader.mirrored
                            }
                            text: (cardHeader.mirrored ? "\u200f" : "")
                                  + (card.expanded ? "▼" : arrows.collapsedRow)
                                  + "  " + card.headerText
                            // The hover highlight and the focus frame are a pair of skin colours, not
                            // a hard-coded black: on a dark theme that darkened the row instead of
                            // highlighting it (a core fix, rule #9).
                            background: Rectangle {
                                radius: 6
                                color: cardHeader.hovered || cardHeader.down
                                       ? root.colCardHover : "transparent"
                                border.color: cardHeader.activeFocus
                                              ? root.colAccent : "transparent"
                                border.width: 1
                            }
                            contentItem: RexonText {
                                text: cardHeader.text
                                color: root.colText
                                verticalAlignment: Text.AlignVCenter
                                elide: Text.ElideRight
                                leftPadding: 14
                                rightPadding: 14
                                topPadding: 10
                                bottomPadding: 10
                            }
                            onClicked: page.toggle(card.index)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(cardHeader)
                        }

                        // Card body: shown only when expanded, as in the original (there it was
                        // setVisible on the widget).
                        ColumnLayout {
                            id: cardBody
                            Layout.fillWidth: true
                            Layout.leftMargin: 14
                            Layout.rightMargin: 14
                            Layout.topMargin: 4
                            Layout.bottomMargin: card.expanded ? 12 : 0
                            spacing: 8
                            visible: card.expanded

                            // Memory: label and bar
                            RexonText {
                                Layout.fillWidth: true
                                text: card.ramLabel
                                textFormat: Text.RichText
                                color: root.colText
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: 8
                                radius: 4
                                // The bar groove is light on a dark theme (the same colour pair fix). The
                                // colour comes from the core with the palette: in the widget tab it is the
                                // same, only wrapped in QSS.
                                color: root.pal.ram_track

                                Rectangle {
                                    height: parent.height
                                    radius: 4
                                    width: card.ramMax > 0
                                           ? parent.width * card.ramValue / card.ramMax
                                           : 0
                                    color: card.ramColor
                                }
                            }

                            // Stars: customization and gaming
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 24

                                Repeater {
                                    model: card.starsLabels

                                    delegate: RexonText {
                                        id: starsText
                                        required property string modelData
                                        text: starsText.modelData
                                        textFormat: Text.RichText
                                        color: root.colText
                                    }
                                }

                                Item {
                                    Layout.fillWidth: true
                                }
                            }

                            // Specs
                            Repeater {
                                model: card.specs

                                delegate: RowLayout {
                                    id: specRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 10

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: specRow.modelData.label
                                        color: root.colText
                                    }

                                    RexonText {
                                        text: specRow.modelData.valueHtml
                                        textFormat: Text.RichText
                                        // An empty colour from the core means "do not paint", and then the value
                                        // goes as ordinary text.
                                        color: specRow.modelData.color === ""
                                               ? root.colText
                                               : specRow.modelData.color
                                    }
                                }
                            }

                            // Description
                            RexonText {
                                Layout.fillWidth: true
                                Layout.topMargin: 6
                                text: card.description
                                wrapMode: Text.WordWrap
                                color: root.colTextSecondary
                                // The same weight as the legend: in the original the description is
                                // `secondary_text_qss(...)`. Without it the line came out 9 pixels
                                // narrower and thinner (measurement 03.09.2026, the KDE card).
                                font.weight: root.pal.text_secondary_weight
                            }

                            // Action button: install or switch
                            RowLayout {
                                Layout.fillWidth: true

                                Item {
                                    Layout.fillWidth: true
                                }

                                // Warning: in the widget original this button looks like bare text: the
                                // reset `QFrame#deCard QWidget { background: transparent }` in `card_qss`
                                // overrides the QPushButton style (a defect of the class from rule #9).
                                // Here it is a real skin button — the defect is not carried into the
                                // port. The blue "Install" is painted with a tone from the core (the same
                                // colours as `INSTALL_BUTTON_QSS`), not with `highlighted` — that took the
                                // skin accent with a different opacity.
                                RexonButton {
                                    id: cardAction
                                    pal: root.pal
                                    text: card.actionText
                                    // SteamOS: "Install" of an unverified desktop is visible but disabled (the
                                    // reason is a line under the button).
                                    enabled: card.actionKind !== "install"
                                             || card.installBlocked === ""
                                    tone: card.actionAccent
                                          ? root.pal.install_tone : null
                                    onClicked: {
                                        if (card.actionKind === "install")
                                            root.runInstall(card.deId);
                                        else
                                            root.runSwitch(card.deId);
                                    }
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardAction)
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                visible: card.actionKind === "install"
                                         && card.installBlocked !== ""
                                text: card.installBlocked
                                wrapMode: Text.WordWrap
                                color: root.pal.status_unavailable
                            }
                        }
                    }
                }
            }

            // ── Quick recommendations ─────────────────────────────────────
            RexonText {
                text: page.recHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            Repeater {
                model: page.recRows

                delegate: Rectangle {
                    id: recRow
                    required property var modelData

                    Layout.fillWidth: true
                    color: root.colCard
                    border.color: root.colBorder
                    border.width: 1
                    radius: 6
                    // Plus two pixels of border — see the correction at the legend.
                    implicitHeight: recLayout.implicitHeight + 18

                    RowLayout {
                        id: recLayout
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: 13
                        y: 9
                        height: implicitHeight
                        spacing: 10

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            RexonText {
                                Layout.fillWidth: true
                                text: recRow.modelData.labelHtml
                                textFormat: Text.RichText
                                color: root.colText
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: recRow.modelData.sub
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                            }
                        }

                        // Desktop already installed — a mark; not installed — a button; "none"
                        // (the desktop is unknown to the core) — a line without an action.
                        RexonText {
                            visible: recRow.modelData.mark === "badge"
                            text: recRow.modelData.text
                            color: recRow.modelData.color === ""
                                   ? root.colText : recRow.modelData.color
                        }

                        RexonButton {
                            id: recButton
                            pal: root.pal
                            visible: recRow.modelData.mark === "install"
                            enabled: page.installBlockedText(recRow.modelData.deId) === ""
                            text: recRow.modelData.text
                            onClicked: root.runInstall(recRow.modelData.deId)
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(recButton)
                        }
                    }
                }
            }
        }
    }

    // ── "X versus Y" dialog ───────────────────────────────────────────────

    function openPair(aId, bId) {
        const view = page.pairView(aId, bId);
        // An empty answer means the pair is unknown to the core. The widget tab
        // simply did not open the dialog in this case; the same here.
        if (!view)
            return;
        pairDialog.view = view;
        pairDialog.open();
    }

    Popup {
        id: pairDialog

        property var view: null
        // Reasons for the pair's disabled install buttons, without repetition.
        readonly property string blockedText: {
            const out = [];
            for (const b of (pairDialog.view ? pairDialog.view.buttons : [])) {
                const why = page.installBlockedText(b.deId);
                if (why !== "" && out.indexOf(why) < 0)
                    out.push(why);
            }
            return out.join("\n");
        }

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        width: Math.min(root.width - 40,
                        pairDialog.view ? pairDialog.view.minWidth : 720)
        // Height follows the content, but no less than the core's minimum and no
        // more than the window. A constant 520 pixels cut the table in the middle
        // of the last row even in a 1440×920 window where there was enough room,
        // and the scroll bar was hidden meanwhile — the cut-off part looked like
        // the end of the window (01.11.2026).
        height: Math.min(root.height - 40,
                         Math.max(pairDialog.view ? pairDialog.view.minHeight : 520,
                                  pairDialog.implicitHeight))
        padding: 16

        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Dimming of the page under the modal window. The default style does not
        // draw it, so the window was modal but did not look it: the user pokes at
        // the cards under it and does not understand why they do not respond
        // (foolproofing, rule #13). The shade is product-wide
        // (`ui_base::MODAL_DIM_RGBA`, arriving in the palette under the key
        // `modal_dim`): the tutorial dims the window with the same one; the tab
        // has no number of its own and must not have one.
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        // The dialog is redrawn on every opening: the content arrives from the
        // core in full, and nothing of the previous pair remains in it.
        contentItem: ColumnLayout {
            spacing: 10

            RexonText {
                Layout.fillWidth: true
                text: pairDialog.view ? pairDialog.view.heading : ""
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: pairDialog.view
                                ? pairDialog.view.titlePointSize : 16
                font.bold: true
            }

            // SteamOS: why the install buttons at the bottom of the window are
            // disabled — the same reasons as in the cards of these desktops.
            RexonText {
                Layout.fillWidth: true
                visible: pairDialog.blockedText !== ""
                text: pairDialog.blockedText
                wrapMode: Text.WordWrap
                color: root.pal.status_unavailable
            }

            // The "parameter — X — Y" table arrives as ready HTML from the core: the
            // same text that the QTextBrowser of the widget tab showed.
            Flickable {
                id: pairFlick
                Layout.fillWidth: true
                Layout.fillHeight: true
                // How much the table needs in full: the window height comes from here.
                Layout.preferredHeight: pairBody.implicitHeight
                contentWidth: width
                contentHeight: pairBody.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                // In a low window the table scrolls — and then the bar is always visible,
                // not only under the mouse: otherwise there is no way to tell there are
                // more rows below.
                ScrollBar.vertical: ScrollBar {
                    policy: pairFlick.contentHeight > pairFlick.height
                            ? ScrollBar.AlwaysOn : ScrollBar.AsNeeded
                }

                RexonText {
                    id: pairBody
                    width: pairFlick.width
                    text: pairDialog.view ? pairDialog.view.html : ""
                    textFormat: Text.RichText
                    wrapMode: Text.WordWrap
                    color: root.colText
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item {
                    Layout.fillWidth: true
                }

                Repeater {
                    model: pairDialog.view ? pairDialog.view.buttons : []

                    delegate: RexonButton {
                        id: pairInstall
                        required property var modelData
                        pal: root.pal
                        text: pairInstall.modelData.text
                        enabled: page.installBlockedText(pairInstall.modelData.deId) === ""
                        // The same blue core button as in the card.
                        tone: pairInstall.modelData.accent
                              ? root.pal.install_tone : null
                        onClicked: {
                            const deId = pairInstall.modelData.deId;
                            pairDialog.close();
                            root.runInstall(deId);
                        }
                    }
                }

                RexonButton {
                    id: pairClose
                    pal: root.pal
                    text: pairDialog.view ? pairDialog.view.closeText
                                          : page.pairCloseText
                    onClicked: pairDialog.close()
                }
            }
        }

        // Focus on opening is on the close button: the dialog must be operable
        // from the keyboard from the first frame, not only with the mouse.
        onOpened: pairClose.forceActiveFocus()
    }

    // ── Focus from the first frame ────────────────────────────────────────

    // Warning: focus goes to the FIRST CONTROL, not to the page root.
    // `forceActiveFocus` on the root looks right but makes `activeFocusItem`
    // a nameless Item: no button receives focus, and the page is dead both
    // from the keyboard and from a gamepad. Caught with a mock-up of the tab
    // frame, not by guessing.
    property int focusAttempts: 0

    function focusFirstControl() {
        const first = pairRepeater.count > 0 ? pairRepeater.itemAt(0) : null;
        if (first) {
            root.focusAttempts = 0;
            first.forceActiveFocus();
            return;
        }
        // The repeater does not fill instantly. We retry on the next event loop
        // pass, but not forever: an empty list of pairs is a core breakage, and
        // endless self-calls would only hide it.
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка сравнения: кнопок пар нет, фокус взять не с чего");
        }
    }

    // ── Desktop operation window ──────────────────────────────────────────
    //
    // The same window as in "Desktops" (`DeOperationDialog.qml`) and the same
    // actions object: there is no second executor and no second dialog.

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
        onDesktopsChanged: page.refreshInstalled()
    }

    Component.onCompleted: root.focusFirstControl()

    // In the embedded tab the page is created in advance and comes alive when
    // switched to — focus must be taken there too, otherwise the first Tab
    // after switching tabs goes nowhere.
    onVisibleChanged: if (root.visible) root.focusFirstControl()
}
