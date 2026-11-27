// The "Extensions" tab in QML. Data and decisions come from the Rust
// object ExtensionsModel (rust/rexon-gui/src/extensions_page.rs), which
// decides nothing itself but asks the core (`rexon_core::qt_extensions`).
// Only MARKUP lives here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from
// `ui_base::skin_colors("steam_dark")` — the default skin of the whole
// product (rule #17). The page defines no colours of its own; this is
// guarded by the core test `qt_extensions_parity::page_colors_come_from_core`.
//
// Side actions (installing an .rxn, removal, enabling/disabling) live
// outside, in an object set as the `root.actions` property: installDialog /
// installPath / busy / confirm / uninstall / toggle / openFolder plus the
// signals installFinished / uninstallFinished. The promise "the page will
// not change" was kept: with the arrival of the Rust object
// (`ExtensionsActions`, rust/rexon-gui/src/extensions_actions.rs) not a
// single call here was rewritten; only the default below was added.
//
// The default is a Rust object created by the page itself. That way the
// tab is complete in a process with no Python at all; the PySide6 host
// (`rexon/qt_extensions/window.py`) can still substitute its own object —
// assigning the property removes the binding. An empty `actions` no longer
// means "the buttons do nothing".
//
// The focus chain is built in from the very start: all actions are on
// controls, the focused one shows a frame, and the page scrolls to it.

// Delegates refer to ids of outer elements (root, page). Without this
// pragma, QML lint rightly complains about unqualified access.
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

    // Side-actions object. Our own Rust one by default; the page host may
    // substitute another.
    // Methods: installDialog() -> string, installPath(path) -> bool,
    //         busy() -> bool, confirm(title, text) -> bool,
    //         uninstall(extId) -> bool, toggle(extId) -> string,
    //         openFolder(dir) -> bool.
    property var actions: ownActions

    ExtensionsActions {
        id: ownActions
        // The name is needed by checks — the same approach as for the model below.
        objectName: "extensionsActions"
    }

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    ExtensionsModel {
        id: page
        // The name is needed by checks: the model is fetched with
        // `root.findChild(QObject, "extensionsModel")` and queried without a
        // single change in the page itself.
        objectName: "extensionsModel"
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colInfo: root.pal.accent_info
    readonly property color colDanger: root.pal.danger

    // Tab styling that is not in the skin palette: chips, the tile, the
    // preview placeholder, the list row, the "Install" block, the layout.
    // Everything comes ready from the core (`qt_extensions`) — the same set of
    // numbers and colours that the layouts and style sheets of the widget tab
    // are built from.
    readonly property var st: page.styles
    // Chip (view buttons and filters): a border without fill; the selected one
    // has a blue fill with a border. The same chip as in the Steam sub-tabs.
    readonly property var toneChip: ({
        "background": "transparent",
        "border": root.st.chip.border,
        "hover": root.st.chip.hoverBackground,
        "radius": root.st.chip.radius
    })
    readonly property var toneChipOn: ({
        "background": root.st.chip.checkedBackground,
        "border": root.st.chip.checkedBorder,
        "hover": root.st.chip.checkedBackground,
        "radius": root.st.chip.radius
    })
    // The "🗑 Delete" button of the detailed view: it has its own fill, denser
    // than on the tile (0.25 versus 0.2 — that is how the original's style
    // sheet is written).
    readonly property var toneDetailDelete: ({
        "fillAlpha": root.st.dialog.dangerFillAlpha
    })
    // Large button of the "Install" block: a border and a light fill,
    // rounding 6.
    readonly property var toneSource: ({
        "background": root.st.source.background,
        "border": root.st.source.border,
        "hover": root.st.source.hover,
        "radius": root.st.source.radius
    })

    // ── Side actions ──────────────────────────────────────────────────────

    // Installation runs in the host's background: only "did it start the job"
    // comes back here. The result arrives as a signal (see Connections below)
    // — so the status line and the showcase update at the same moment as in
    // the widget tab.
    // What the button of the "Install" block does. The action kind is
    // dispatched here, not in the delegate: that way it can be called both
    // from the keyboard and from a check, without imitating a mouse click at
    // coordinates.
    function runSource(kind) {
        if (kind === "file")
            root.runInstallDialog();
        else if (kind === "create")
            createDialog.open();
        // "workshop" is disabled: the button is inactive and cannot be pressed.
    }

    function runInstallDialog() {
        if (!root.actions)
            return;
        const path = root.actions.installDialog();
        if (path)
            root.runInstallPath(path);
    }

    function runInstallPath(path) {
        if (!root.actions)
            return;
        if (root.actions.busy()) {
            page.noteInstallBusy();
            return;
        }
        page.noteInstalling(path);
        root.actions.installPath(path);
    }

    function runUninstall(extId, detailed) {
        if (!root.actions)
            return;
        const question = detailed ? page.confirmUninstallDetailed(extId)
                                  : page.confirmUninstall(extId);
        if (!root.actions.confirm(page.confirmTitle, question))
            return;
        if (root.actions.busy()) {
            page.noteUninstallBusy();
            return;
        }
        page.noteUninstalling(page.displayName(extId));
        root.actions.uninstall(extId);
    }

    function runToggle(extId) {
        if (!root.actions)
            return;
        // The answer is "on" / "off" / an error text prefixed with "!". A pair
        // "did it work + the new state" cannot be expressed as one value, and two
        // calls in a row could disagree with the state on disk.
        const answer = root.actions.toggle(extId);
        if (answer === "on")
            page.noteEnabled(extId);
        else if (answer === "off")
            page.noteDisabled(extId);
        else
            page.noteToggleError(answer.substring(1));
        page.refresh();
    }

    Connections {
        target: root.actions

        function onInstallFinished(ok, message) {
            if (ok)
                page.noteInstalled(message);
            else
                page.noteInstallError(message);
            page.refresh();
        }

        function onUninstallFinished(ok, message) {
            if (ok)
                page.noteUninstalled(message);
            else
                page.noteUninstallError(message);
            page.refresh();
        }

        function onApplyFinished(message) {
            page.noteText(message);
        }
    }

    // "Apply" on a wallpaper set: puts the wallpaper on the desktop right from
    // here. Previously the tile had only "Delete", and something installed
    // from a file could not be turned on without going to "Settings".
    function runApply(extId) {
        if (root.actions && root.actions.apply)
            page.noteText(root.actions.apply(extId));
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

    // ── Dragging files ────────────────────────────────────────────────────
    //
    // The same set of extensions the widget tab accepted; the core decides
    // about it (`qt_extensions::is_supported_drop`), not a list in the markup.
    DropArea {
        id: dropZone
        anchors.fill: parent
        keys: ["text/uri-list"]

        onEntered: (drag) => {
            drag.accepted = root.acceptableDrop(drag.urls).length > 0;
        }
        onDropped: (drop) => {
            const good = root.acceptableDrop(drop.urls);
            if (good.length === 0)
                return;
            drop.acceptProposedAction();
            root.runInstallPath(good[0]);
        }
    }

    function acceptableDrop(urls) {
        const out = [];
        for (let i = 0; i < urls.length; ++i) {
            const path = String(urls[i]).replace(/^file:\/\//, "");
            if (page.acceptsDrop(path))
                out.push(path);
        }
        return out;
    }

    // Window highlight while dragging: without it the user cannot tell
    // whether the program will take the file (foolproofing, rule #13).
    Rectangle {
        anchors.fill: parent
        visible: dropZone.containsDrag
        color: "transparent"
        border.color: root.colAccent
        border.width: 3
        z: 5
    }

    // ── Page ──────────────────────────────────────────────────────────────

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: pageColumn.implicitHeight + 2 * root.st.page.margin
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
            anchors.margins: root.st.page.margin
            y: root.st.page.margin
            height: implicitHeight
            spacing: root.st.page.spacing

            // ── Header ────────────────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }

                Item {
                    Layout.fillWidth: true
                }

                // View buttons are the original's chips (`QToolButton:checked` in blue),
                // not the skin's highlighted buttons: icon size 14 and minimum width 32
                // come from the core. The selection is drawn with a tone, not `checked`:
                // pressing the selected one again must not "deselect" it.
                RexonButton {
                    id: gridButton
                    pal: root.pal
                    text: page.viewGridText
                    tone: page.viewMode === "grid" ? root.toneChipOn : root.toneChip
                    font.pointSize: root.st.viewButton.pointSize
                    leftPadding: root.st.chip.paddingH
                    rightPadding: root.st.chip.paddingH
                    implicitWidth: Math.max(implicitContentWidth, root.st.viewButton.minWidth)
                                   + leftPadding + rightPadding
                    ToolTip.visible: hovered
                    ToolTip.text: page.viewGridTooltip
                    onClicked: page.setViewMode("grid")
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(gridButton)
                }

                RexonButton {
                    id: listButton
                    pal: root.pal
                    text: page.viewListText
                    tone: page.viewMode === "list" ? root.toneChipOn : root.toneChip
                    font.pointSize: root.st.viewButton.pointSize
                    leftPadding: root.st.chip.paddingH
                    rightPadding: root.st.chip.paddingH
                    implicitWidth: Math.max(implicitContentWidth, root.st.viewButton.minWidth)
                                   + leftPadding + rightPadding
                    ToolTip.visible: hovered
                    ToolTip.text: page.viewListTooltip
                    onClicked: page.setViewMode("list")
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(listButton)
                }

                RexonButton {
                    id: installButton
                    pal: root.pal
                    text: page.installButtonText
                    highlighted: true
                    // Original: `font-weight: bold` in the button's style sheet.
                    font.bold: true
                    onClicked: root.runInstallDialog()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(installButton)
                }
            }

            // ── Search ────────────────────────────────────────────────────
            RexonTextField {
                id: searchField
                Layout.fillWidth: true
                pal: root.pal
                placeholderText: page.searchPlaceholder
                text: page.searchText
                onTextEdited: page.setSearch(text)
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(searchField)
            }

            // ── Filter buttons ────────────────────────────────────────────
            //
            // Warning: a Flow, not a RowLayout: there are thirty-five extension types,
            // and at the minimum window width (900, a number from the core) the
            // buttons do not fit into one line. RowLayout cannot wrap and would simply
            // cut off the tail — the same defect the "Compare" header had.
            Flow {
                Layout.fillWidth: true
                spacing: root.st.page.filterSpacing

                Repeater {
                    model: page.filters

                    delegate: RexonButton {
                        id: filterButton
                        required property var modelData
                        pal: root.pal
                        text: filterButton.modelData.label
                        tone: filterButton.modelData.checked ? root.toneChipOn
                                                             : root.toneChip
                        leftPadding: root.st.chip.paddingH
                        rightPadding: root.st.chip.paddingH
                        onClicked: page.setFilter(filterButton.modelData.id)
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(filterButton)
                    }
                }
            }

            // ── Statistics ────────────────────────────────────────────────
            RexonText {
                Layout.fillWidth: true
                text: page.statsLine
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                // Original: `font-weight: 600` on all secondary labels — a thin weight is
                // unreadable on dark (the owner's rule, 2026-06-11). The number arrives
                // from the core with the palette (`ui_base::SECONDARY_TEXT_WEIGHT`), as in
                // "Compare".
                font.weight: root.pal.text_secondary_weight
            }

            // ── "My extensions" ───────────────────────────────────────────
            RexonText {
                text: page.myHeader
                visible: page.myHeader !== ""
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // ── Showcase: tile grid ───────────────────────────────────────
            //
            // The number of columns is computed from the strip width and the CONSTANT
            // tile width (260 from the core) — just as the shared grid engine of the
            // widget tab did. Its own steps "<700 → 2, <1100 → 3" had already been
            // found wrong there: above 1500 px the number of columns froze at five
            // and an empty margin remained on the right.
            // Warning: a GridLayout, not a Flow: tiles in ONE ROW must have the same
            // height, otherwise the "Disable" and "🗑" buttons sit at different
            // heights — extensions have different sets of lines (not all have an
            // author, the "Active" status only scripted ones have). In the widget tab
            // this was done by the `card_actions_spacer` spacer, which removed the
            // card's vertical fixing and stretched it to the row height. A Flow does
            // not equalize row heights — that would be a degradation, not a port.
            //
            // Left alignment (without AlignTop) also comes from the original: with
            // AlignTop a tile is clamped to its own height, and the row falls apart
            // again.
            GridLayout {
                id: cardsGrid
                Layout.alignment: Qt.AlignLeft
                visible: page.viewMode === "grid"
                columnSpacing: root.st.page.gridSpacing
                rowSpacing: root.st.page.gridSpacing
                columns: Math.max(1, Math.floor((pageColumn.width + columnSpacing)
                                                / (page.cardWidth + columnSpacing)))

                Repeater {
                    model: page.viewMode === "grid" ? page : null

                    delegate: Rectangle {
                        id: card

                        required property int index
                        required property string extId
                        required property string name
                        required property string nameTooltip
                        required property string typeLine
                        required property string authorLine
                        required property string imageUrl
                        required property string placeholder
                        required property bool hasToggle
                        required property string stateText
                        required property string stateColor
                        required property string toggleText
                        /// The tile picture was read — it can be opened large (there is no point
                        /// opening the placeholder icon).
                        readonly property bool imageReady: cardImage.visible
                        /// Tile entry for the large view.
                        readonly property var viewerItem: card.imageReady ? ({
                            "key": card.extId,
                            "title": card.name,
                            "subtitle": card.typeLine,
                            "thumb": card.imageUrl,
                            "source": card.imageUrl
                        }) : null
                        onViewerItemChanged: root.registerViewerItem(card.index, card.viewerItem)
                        Component.onCompleted: root.registerViewerItem(card.index, card.viewerItem)
                        Component.onDestruction: root.registerViewerItem(card.index, null)

                        Layout.preferredWidth: page.cardWidth
                        Layout.maximumWidth: page.cardWidth
                        Layout.fillHeight: true
                        Layout.alignment: Qt.AlignLeft
                        implicitHeight: cardColumn.implicitHeight + 2 * root.st.card.margin
                        // Tile colours come from the core (the original's `QFrame#extCard`): under
                        // the cursor only the border changes, the background stays.
                        color: root.st.card.background
                        border.color: cardHover.hovered ? root.st.card.hover
                                                        : root.st.card.border
                        border.width: 1
                        radius: root.st.card.radius

                        // A click on the tile opens the detailed view, as in the original (there
                        // it was a mousePressEvent of the whole card).
                        HoverHandler {
                            id: cardHover
                        }
                        TapHandler {
                            onTapped: root.openDetail(card.extId)
                        }

                        ColumnLayout {
                            id: cardColumn
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.margins: root.st.card.margin
                            y: root.st.card.margin
                            // The height is taken from the tile, not from its own implicitHeight: the
                            // row stretched the tile, and the button row below must use that to press
                            // against the bottom edge.
                            height: parent.height - 2 * root.st.card.margin
                            spacing: root.st.card.spacing

                            // Preview or type icon. The placeholder is shown exactly when the picture
                            // was not read — the widget made the same choice based on an empty pixmap.
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: page.previewHeight
                                radius: root.st.card.radius
                                color: root.st.placeholder.background
                                clip: true

                                Image {
                                    id: cardImage
                                    anchors.fill: parent
                                    source: card.imageUrl
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    visible: status === Image.Ready
                                }

                                RexonText {
                                    anchors.centerIn: parent
                                    visible: !cardImage.visible
                                    text: card.placeholder
                                    color: root.st.placeholder.color
                                    font.pointSize: root.st.placeholder.pointSize
                                }

                                // A click on the tile is taken by the description, so the large view is
                                // opened with the magnifier.
                                RexonZoomArea {
                                    anchors.fill: parent
                                    pal: root.pal
                                    badgeOnly: true
                                    radius: root.st.card.radius
                                    enabled: card.imageReady
                                    visible: enabled
                                    onActivated: root.openViewer(card.extId)
                                }
                            }

                            // Type and author are the original's secondary labels: the colour from the
                            // `ui_base` pair (fix no. 2 in the core), weight 600 — as there.
                            RexonText {
                                Layout.fillWidth: true
                                text: card.typeLine
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: card.name
                                color: root.colText
                                font.bold: true
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                                ToolTip.visible: nameHover.hovered
                                                 && truncated
                                ToolTip.text: card.nameTooltip
                                HoverHandler {
                                    id: nameHover
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                visible: card.authorLine !== ""
                                text: card.authorLine
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                            }

                            RexonText {
                                visible: card.hasToggle
                                text: card.stateText
                                color: card.stateColor
                            }

                            // The button row takes the rest of the row height and presses the buttons
                            // to the bottom — this is the original's `card_actions_spacer`: extensions
                            // have different sets of lines (not all have an author, only scripted ones
                            // have a status), and without it the buttons in a row would sit at
                            // different heights.
                            // A separate spacer Item does not work here: the layout added its own
                            // spacing to it, and above the buttons there came out 10 pixels more than
                            // in the original (measurement 03.09: 20 versus 8).
                            // Warning: `fillHeight` alone is not enough, and this is not nitpicking:
                            // the row also has a height CEILING, which it computes from its children.
                            // The buttons are pressed down with `Qt.AlignBottom`, an aligned cell does
                            // not stretch, and the row's ceiling becomes equal to the button height.
                            // The tile has nowhere to put the remaining height — the layout spreads it
                            // over ALL the gaps: an extra margin appeared above the preview, and the
                            // buttons still did not reach the bottom edge (measurement 18.09 on the
                            // test bench: top margin 19 versus 7 pixels, buttons 4 higher than their
                            // neighbours). The ceiling is removed — the button row takes the whole
                            // remainder, as the original's spacer did.
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                Layout.maximumHeight: Number.POSITIVE_INFINITY
                                spacing: root.st.card.buttonSpacing

                                RexonButton {
                                    id: cardToggle
                                    Layout.alignment: Qt.AlignBottom
                                    visible: card.hasToggle
                                    pal: root.pal
                                    text: card.toggleText
                                    onClicked: root.runToggle(card.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardToggle)
                                }

                                RexonButton {
                                    id: cardApply
                                    Layout.alignment: Qt.AlignBottom
                                    readonly property string label:
                                        (root.actions && root.actions.applyText)
                                        ? root.actions.applyText(card.extId) : ""
                                    visible: cardApply.label !== ""
                                    pal: root.pal
                                    text: cardApply.label
                                    onClicked: root.runApply(card.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardApply)
                                }

                                RexonDangerButton {
                                    id: cardDelete
                                    Layout.alignment: Qt.AlignBottom
                                    pal: root.pal
                                    // A label, not just a bin icon: removal is irreversible, and the customer
                                    // should not have to guess the icon (rule #13). Details of the departure
                                    // from the reference are at the `UNINSTALL_TEXT` key in the core. The
                                    // button measures its width by its label itself, like the neighbouring
                                    // toggle.
                                    text: page.uninstallText
                                    ToolTip.visible: hovered
                                    ToolTip.text: page.uninstallTooltip
                                    onClicked: root.runUninstall(card.extId, false)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(cardDelete)
                                }

                                Item {
                                    Layout.fillWidth: true
                                }
                            }
                        }
                    }
                }
            }

            // ── Showcase: list grouped by type ────────────────────────────
            // In the widget tab the group headers and the rows are neighbouring rows
            // of ONE grid with the tile spacing (10), so here too the grid spacing is
            // used both between blocks and between a header and its first row.
            ColumnLayout {
                Layout.fillWidth: true
                visible: page.viewMode === "list"
                spacing: root.st.page.gridSpacing

                Repeater {
                    model: page.viewMode === "list" ? page : null

                    delegate: ColumnLayout {
                        id: rowBlock

                        required property string extId
                        required property string typeLine
                        required property string titleHtml
                        required property string subtitle
                        required property string placeholder
                        required property bool hasToggle
                        required property string toggleIcon
                        required property string toggleTooltip
                        required property string groupHeader

                        Layout.fillWidth: true
                        spacing: root.st.page.gridSpacing

                        RexonText {
                            Layout.fillWidth: true
                            Layout.topMargin: root.st.group.paddingTop
                            Layout.bottomMargin: root.st.group.paddingBottom
                            visible: rowBlock.groupHeader !== ""
                            text: rowBlock.groupHeader
                            textFormat: Text.RichText
                            color: root.colInfo
                        }

                        // Row padding is the layout margins plus the `padding` of its style sheet:
                        // in the widgets they add up.
                        Rectangle {
                            id: rowFrame
                            readonly property int padH: root.st.row.marginH + root.st.row.padding
                            readonly property int padV: root.st.row.marginV + root.st.row.padding
                            Layout.fillWidth: true
                            implicitHeight: rowLayout.implicitHeight + 2 * rowFrame.padV
                            radius: root.st.row.radius
                            color: rowHover.hovered ? root.st.row.hover : root.st.row.background

                            HoverHandler {
                                id: rowHover
                            }
                            TapHandler {
                                onTapped: root.openDetail(rowBlock.extId)
                            }

                            RowLayout {
                                id: rowLayout
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.margins: rowFrame.padH
                                y: rowFrame.padV
                                height: implicitHeight
                                spacing: root.st.row.spacing

                                RexonText {
                                    Layout.preferredWidth: root.st.row.iconWidth
                                    text: rowBlock.placeholder
                                    color: root.colText
                                    font.pointSize: root.st.row.iconPointSize
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: rowBlock.titleHtml
                                        textFormat: Text.RichText
                                        color: root.colText
                                        elide: Text.ElideRight
                                    }

                                    RexonText {
                                        Layout.fillWidth: true
                                        visible: rowBlock.subtitle !== ""
                                        text: rowBlock.subtitle
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        elide: Text.ElideRight
                                    }
                                }

                                RexonButton {
                                    id: rowToggle
                                    visible: rowBlock.hasToggle
                                    pal: root.pal
                                    text: rowBlock.toggleIcon
                                    implicitWidth: root.st.row.buttonWidth
                                    leftPadding: 0
                                    rightPadding: 0
                                    ToolTip.visible: hovered
                                    ToolTip.text: rowBlock.toggleTooltip
                                    onClicked: root.runToggle(rowBlock.extId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(rowToggle)
                                }

                                RexonDangerButton {
                                    id: rowDelete
                                    pal: root.pal
                                    text: page.uninstallIcon
                                    implicitWidth: root.st.row.buttonWidth
                                    leftPadding: 0
                                    rightPadding: 0
                                    ToolTip.visible: hovered
                                    ToolTip.text: page.uninstallTooltip
                                    onClicked: root.runUninstall(rowBlock.extId, false)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(rowDelete)
                                }
                            }
                        }
                    }
                }
            }

            // ── Empty state ───────────────────────────────────────────────
            RexonText {
                Layout.fillWidth: true
                Layout.margins: root.st.page.emptyPadding
                visible: page.emptyText !== ""
                text: page.emptyText
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── "⬇ Install" block ─────────────────────────────────────────
            RexonText {
                text: page.installSectionHeader
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            Flow {
                Layout.fillWidth: true
                spacing: root.st.source.spacing

                Repeater {
                    model: page.installSources

                    delegate: RexonButton {
                        id: sourceButton
                        required property var modelData
                        pal: root.pal
                        text: sourceButton.modelData.text
                        enabled: sourceButton.modelData.enabled
                        tone: root.toneSource
                        padding: root.st.source.padding
                        leftPadding: root.st.source.padding
                        rightPadding: root.st.source.padding
                        implicitWidth: Math.max(page.sourceMinWidth,
                                                Math.min(page.sourceMaxWidth,
                                                         implicitContentWidth
                                                         + leftPadding + rightPadding))
                        implicitHeight: page.sourceMinHeight
                        ToolTip.visible: hovered
                        ToolTip.text: sourceButton.modelData.tooltip
                        onClicked: root.runSource(sourceButton.modelData.kind)
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(sourceButton)
                    }
                }
            }

            // ── Status line ───────────────────────────────────────────────
            //
            // Room for the line is ALWAYS taken, even when there is nothing to report:
            // in the original this is a permanent `QLabel` that is hidden never, not
            // on empty text, and the empty line keeps its 21 pixels between the
            // "Install" block and the plans. While `visible: statusText !== ""` stood
            // here, the line collapsed, the plans rose 30 pixels above the original,
            // and on the very first installation the whole page jerked down.
            RexonText {
                Layout.fillWidth: true
                text: page.statusText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Plans ─────────────────────────────────────────────────────
            RexonText {
                Layout.fillWidth: true
                text: page.roadmapNote
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ── Detailed view of an extension ─────────────────────────────────────

    // ── Large view of an extension picture ────────────────────────────────
    //
    // The layer is shared across the whole product (`RexonImageViewer.qml`).
    // Paging goes over the showcase tiles that have a picture.
    property var viewerRegistry: ({})

    function registerViewerItem(row, info) {
        if (info)
            root.viewerRegistry[row] = info;
        else
            delete root.viewerRegistry[row];
    }

    function openViewer(extId) {
        const items = [];
        let at = 0;
        // What to show is said by the tiles themselves (`viewerRegistry`): tile
        // index on the showcase → its entry. Tiles with a placeholder instead of
        // a picture do not get into the list.
        const rows = Object.keys(root.viewerRegistry).map(Number).sort((x, y) => x - y);
        for (const row of rows) {
            const info = root.viewerRegistry[row];
            if (info.key === extId)
                at = items.length;
            items.push(info);
        }
        viewer.show(items, at);
    }

    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
    }

    function openDetail(extId) {
        const view = page.detailFor(extId);
        // An empty answer means there is no such extension on the showcase. The
        // widget tab simply did not open the window in this case; the same here.
        if (!view)
            return;
        detailDialog.view = view;
        detailDialog.open();
    }

    Popup {
        id: detailDialog
        // The name is needed by the acceptance test bench: the window is found by
        // it in order to capture it separately from the page.
        objectName: "detailDialog"

        property var view: null

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        // The numbers from the core are the window MINIMUM, not its size: for the
        // widget window `setMinimumSize` did not stop it from taking more, and on
        // a large monitor the README could be read in full. A QML modal window has
        // no size of its own at all, so we take the minimum as a lower bound, not
        // as a ceiling.
        width: Math.min(root.width - 40,
                        Math.max(page.detailMinWidth, root.width * 0.6))
        height: Math.min(root.height - 40,
                         Math.max(page.detailMinHeight, root.height * 0.8))
        padding: root.st.dialog.margin

        background: Rectangle {
            // Window background — the same as the widget dialog's: the `Window` role
            // of the skin palette. The secondary background here was darker than the
            // original across the whole dialog, while it belongs to the description
            // area (see below).
            color: root.pal.bg_primary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Dimming of the page under the modal window. The default style does not
        // draw it, so the window would be modal but not look it: the user pokes at
        // the tiles under it and does not understand why they do not respond
        // (foolproofing, rule #13). The shade is product-wide
        // (`ui_base::MODAL_DIM_RGBA`, arriving in the palette under the key
        // `modal_dim`).
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: root.st.dialog.detailSpacing

            RexonText {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.headingHtml : ""
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: page.detailTitlePointSize
                font.bold: true
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.metaLine : ""
                color: root.colTextSecondary
                // Original: `font-weight: 600` on the meta line, the technical details and
                // the path — the same set of secondary labels as on the showcase.
                font.weight: root.pal.text_secondary_weight
                wrapMode: Text.WordWrap
            }

            Image {
                Layout.fillWidth: true
                // The preview takes its height while there is room and GIVES IT UP when
                // the window is shrunk to the minimum. Without this the picture kept its
                // natural height as a lower bound and squeezed the description out
                // entirely: in a 680×540 window (the allowed minimum) not a single line of
                // text was left, whereas in the widget window both blocks shrink
                // (rule #13).
                Layout.fillHeight: true
                Layout.preferredHeight: page.detailPreviewHeight
                Layout.minimumHeight: 0
                Layout.maximumHeight: page.detailPreviewHeight
                source: detailDialog.view ? detailDialog.view.previewUrl : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                visible: status === Image.Ready

                RexonZoomArea {
                    anchors.fill: parent
                    pal: root.pal
                    onActivated: viewer.show([{
                        "key": detailDialog.view.extId,
                        "title": page.displayName(detailDialog.view.extId),
                        "subtitle": detailDialog.view.metaLine,
                        "thumb": detailDialog.view.previewUrl,
                        "source": detailDialog.view.previewUrl
                    }], 0)
                }
            }

            // Backing of the description area. In the widget window this is a
            // `QTextBrowser`: its own border, its own rounding and a `Base` role
            // background different from the window background. A bare Flickable did
            // not give that, and the text hung directly on the window backing — the
            // area did not read as a field.
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                // Below this bound the description does not shrink — just like the widget
                // window's `QTextBrowser`. The lack of room in a shrunk window is taken by
                // the preview, and the text stays readable.
                Layout.minimumHeight: root.st.dialog.bodyMinHeight
                color: root.pal.bg_secondary
                border.color: root.colBorder
                border.width: root.st.dialog.bodyBorderWidth
                radius: root.st.dialog.bodyRadius

                Flickable {
                    id: bodyFlick
                    anchors.fill: parent
                    anchors.margins: root.st.dialog.bodyPadding
                                     + root.st.dialog.bodyBorderWidth
                    contentWidth: width
                    contentHeight: bodyText.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds

                    // The scroll bar TAKES width from the text rather than lying over it: that
                    // is how the QTextBrowser of the widget windows works, and without it the
                    // lines ran some fifteen pixels longer, wrapped in other places and the
                    // whole text shifted (measurement of the help: a shift of −19 pixels on
                    // every bar). The width is taken from the bar itself; there is no number
                    // of our own.
                    ScrollBar.vertical: ScrollBar { id: bodyBar }

                    // The README and the description arrive as ready markup from the core —
                    // the same text the QTextBrowser of the widget tab showed, only with
                    // escaped angle brackets (fix #1: the original lost chunks of text on
                    // them).
                    RexonLinkText {
                        id: bodyText
                        // Width by anchors, room for the bar by a margin on its side: for right to
                        // left the bar moves to the left, and mirroring moves the margin there
                        // too.
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.rightMargin: bodyBar.visible ? bodyBar.width : 0
                        // Description of a third-party extension: links go through the shared path
                        // (↗ and a question before an external site).
                        html: detailDialog.view ? detailDialog.view.bodyHtml : ""
                        pal: root.pal
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: detailDialog.view && detailDialog.view.tagsLine !== ""
                text: detailDialog.view ? detailDialog.view.tagsLine : ""
                wrapMode: Text.WordWrap
                color: root.colInfo
            }

            RexonLinkText {
                Layout.fillWidth: true
                Layout.topMargin: root.st.dialog.techPaddingV
                Layout.bottomMargin: root.st.dialog.techPaddingV
                html: detailDialog.view ? detailDialog.view.techHtml : ""
                pal: root.pal
                visible: html !== ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonText {
                Layout.fillWidth: true
                text: detailDialog.view ? detailDialog.view.pathHtml : ""
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonButton {
                    id: detailOpen
                    pal: root.pal
                    text: page.detailOpenText
                    onClicked: {
                        if (root.actions && detailDialog.view)
                            root.actions.openFolder(detailDialog.view.dir);
                    }
                }

                Item {
                    Layout.fillWidth: true
                }

                RexonButton {
                    id: detailApply
                    readonly property string label:
                        (root.actions && root.actions.applyText && detailDialog.view)
                        ? root.actions.applyText(detailDialog.view.extId) : ""
                    visible: detailApply.label !== ""
                    pal: root.pal
                    text: detailApply.label
                    onClicked: {
                        const extId = detailDialog.view.extId;
                        detailDialog.close();
                        root.runApply(extId);
                    }
                }

                RexonDangerButton {
                    id: detailDelete
                    pal: root.pal
                    tone: root.toneDetailDelete
                    text: page.detailUninstallText
                    onClicked: {
                        const extId = detailDialog.view.extId;
                        detailDialog.close();
                        root.runUninstall(extId, true);
                    }
                }

                RexonButton {
                    id: detailClose
                    pal: root.pal
                    text: page.detailCloseText
                    onClicked: detailDialog.close()
                }
            }
        }

        // Focus on opening is on the close button: the window must be operable
        // from the keyboard from the first frame, not only with the mouse.
        onOpened: detailClose.forceActiveFocus()
    }

    // ── "Create your own" help ────────────────────────────────────────────

    Popup {
        id: createDialog
        objectName: "createDialog"

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        anchors.centerIn: parent
        width: Math.min(root.width - 40,
                        Math.max(page.createMinWidth, root.width * 0.6))
        height: Math.min(root.height - 40,
                         Math.max(page.createMinHeight, root.height * 0.85))
        padding: root.st.dialog.margin

        background: Rectangle {
            color: root.pal.bg_primary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            // The help has no spacing of its own: the default layout spacing applies,
            // and it is narrower than the detailed view's (6 versus 10).
            spacing: root.st.dialog.createSpacing

            RexonText {
                Layout.fillWidth: true
                text: page.createHeader
                textFormat: Text.RichText
                color: root.colText
                font.pointSize: page.sectionPointSize
                font.bold: true
            }

            // The same backing as the detailed view's description: in the widget help
            // the text also lies in a `QTextBrowser`.
            Rectangle {
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.minimumHeight: root.st.dialog.bodyMinHeight
                color: root.pal.bg_secondary
                border.color: root.colBorder
                border.width: root.st.dialog.bodyBorderWidth
                radius: root.st.dialog.bodyRadius

                Flickable {
                    id: createFlick
                    anchors.fill: parent
                    anchors.margins: root.st.dialog.bodyPadding
                                     + root.st.dialog.bodyBorderWidth
                    contentWidth: width
                    contentHeight: createText.implicitHeight
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds

                    ScrollBar.vertical: ScrollBar { id: createBar }

                    RexonLinkText {
                        id: createText
                        // Width by anchors, room for the bar by a margin on its side: for right to
                        // left the bar moves to the left, and mirroring moves the margin there
                        // too.
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.rightMargin: createBar.visible ? createBar.width : 0
                        html: page.createBody
                        pal: root.pal
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true

                Item {
                    Layout.fillWidth: true
                }

                RexonButton {
                    id: createClose
                    pal: root.pal
                    text: page.createCloseText
                    onClicked: createDialog.close()
                }
            }
        }

        onOpened: createClose.forceActiveFocus()
    }

    // ── Focus from the first frame ────────────────────────────────────────

    // Warning: focus goes to the FIRST CONTROL, not to the page root.
    // `forceActiveFocus` on the root looks right but makes `activeFocusItem`
    // a nameless Item: no button receives focus, and the page is dead both
    // from the keyboard and from a gamepad.
    // On a cold first frame the scene may not be in the window yet, and
    // `forceActiveFocus()` goes nowhere: a measurement caught a run where
    // focus was not taken at all and the tab stayed dead from both the
    // keyboard and the gamepad. So the attempt is repeated on the next pass
    // of the event loop — just as in "DE comparison" and "Desktops" — but not
    // forever.
    property int focusAttempts: 0

    function focusFirstControl() {
        gridButton.forceActiveFocus();
        if (gridButton.activeFocus) {
            root.focusAttempts = 0;
            return;
        }
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка расширений: фокус взять не удалось");
        }
    }

    Component.onCompleted: root.focusFirstControl()

    // In the embedded tab the page is created in advance and comes alive when
    // switched to — focus must be taken there too, otherwise the first Tab
    // after switching tabs goes nowhere.
    onVisibleChanged: {
        if (!root.visible)
            return;
        // An extension may have been installed bypassing this tab ("Install from
        // file…" in "Appearance") — the list is re-read on every show, otherwise
        // the new one is not visible until a restart.
        page.refresh();
        root.focusFirstControl();
    }
}
