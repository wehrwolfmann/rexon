// The "Desktops" tab in QML. Data and decisions come from the Rust object
// DesktopsModel (rust/rexon-gui/src/desktops_page.rs), which decides
// nothing itself but asks the core (`rexon_core::qt_desktops`). Only
// MARKUP lives here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from
// `ui_base::skin_colors("steam_dark")` — the default skin of the whole
// product (rule #17). The page defines no colours of its own.
//
// Never: the side actions of this tab INSTALL AND REMOVE desktops through
// pacman and pkexec. The page takes not a single such step by itself: it
// calls the `root.actions` object put in by the window host
// (rexon/qt_desktops/window.py). The confirmations, both removal
// safeguards and the progress windows live there too — exactly what was in
// the widget tab. The page knows only the method names and "did it work".
//
// The focus chain is built in from the start: all actions are controls,
// the focused one shows a frame, and the page scrolls to it.

// Delegates refer to ids of outer elements (root, page). Without this
// pragma, QML lint rightly complains about unqualified access.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Bridge to the outside ─────────────────────────────────────────────

    // Side-actions object; set by the page host. Methods:
    //   install(deId) -> bool      install a desktop (confirmation + progress)
    //   repair(deId) -> bool       check and repair
    //   switchTo(deId) -> bool     switch the session
    //   remove(deId) -> bool       removal with both safeguards
    //   repairCurrent() -> bool    "check my desktop"
    //   panelBackup() / panelRestore() -> bool
    //   openCompare()              show the embedded "DE comparison"
    //   requestVersion(deId)       asynchronous version query from repositories
    //   shellInstall / shellSwitch / shellRemove(deId, shellId)
    //                              the "Shells" block (Noctalia, DMS)
    //   lookApply / lookRevert(lookId)
    //                              the "Like in the distribution" block
    // Signal versionReady(deId, version) — the answer to requestVersion.
    property var actions: null

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight
    // Label of the back button from the embedded "DE comparison": the page
    // host draws it (the comparison is a separate window and does not go into
    // the scene), but the text still comes from the core rather than from a
    // second copy in Python.
    readonly property string compareBackText: page.compareBackText
    /// The ← → arrows in the page's labels follow its mirroring.
    readonly property RexonArrows arrows: RexonArrows {
        mirrored: root.LayoutMirroring.enabled
    }
    // The selected desktop and the session's desktop — for the main window's
    // `--selftest` run: the tab must open on the CURRENT desktop, not on the
    // first one in the list.
    readonly property string selectedDe: page.selectedDe
    readonly property string currentDe: page.currentDe

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    DesktopsModel {
        id: page
        // The name is needed by checks: from the page host the model is fetched
        // with `root.findChild(QObject, "desktopsModel")`.
        // Warning: only invokables WITHOUT numbers in their signature are called
        // from outside (`refreshInstalled`, `select`, `removePlan`, `displayName`):
        // for `i32` the metaobject has the name `::std::int32_t`, there is no
        // metatype with that name, and `QMetaObject.invokeMethod` silently returns
        // False.
        objectName: "desktopsModel"
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    // Markup sizes — paddings, font sizes, button sides — from the same core
    // map.
    readonly property var m: page.metrics
    readonly property color colBg: root.pal.bg_primary
    readonly property color colCard: root.pal.bg_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // ── View state ────────────────────────────────────────────────────────

    readonly property var card: page.card
    readonly property var carouselTexts: page.carousel

    // Current carousel slide. Reset when the desktop changes: otherwise a
    // desktop with a single screenshot would keep the index from the previous
    // one, and the carousel would show emptiness.
    property int slide: 0
    // Package version from the repositories for the shown desktop: "" means
    // not asked yet or there is no package, and then the line is hidden.
    property string versionText: ""
    property bool versionPending: false
    // The "packages" block from the answer of the trial transaction: "" means
    // no answer yet (or asking failed), and then the card's line is visible.
    property string packagesLine: ""
    property string packagesNotice: ""
    // Header replacement from the probe answer ("Partially installed"); empty
    // means the card's header.
    property string statusOverride: ""
    // The set will not install because of a conflict with something installed:
    // "Install" and "Check / Repair" are visible but disabled (the reason is
    // in `packagesNotice` right above them).
    property bool packagesBlocked: false
    // A package of the set is waiting to be rebuilt against a new library — it
    // can be built from the Arch sources: the button label and an explanation
    // ("" — nothing to do).
    property string rebuildButton: ""
    property string rebuildHint: ""

    onCardChanged: {
        root.slide = 0;
        root.versionText = "";
        root.versionPending = false;
        root.packagesLine = "";
        root.statusOverride = "";
        root.packagesNotice = "";
        root.packagesBlocked = false;
        root.rebuildButton = "";
        root.rebuildHint = "";
        root.requestVersion();
        root.requestPackages();
    }

    // ── Side actions ──────────────────────────────────────────────────────

    // All of them are SYNCHRONOUS and return "did it work": confirmation,
    // progress and result are shown by the host, and from the answer the page
    // decides whether to recompute the set of installed desktops.
    function runAction(kind, deId) {
        if (!root.actions)
            return;
        let changed = false;
        if (kind === "install")
            changed = root.actions.install(deId);
        else if (kind === "repair")
            changed = root.actions.repair(deId);
        else if (kind === "switch")
            changed = root.actions.switchTo(deId);
        else if (kind === "remove")
            changed = root.actions.remove(deId);
        if (changed)
            page.refreshInstalled();
    }

    // The "Shells" block: install, log in with the shell, remove. Progress and
    // result go through the shared operation window; the set of desktops is
    // recomputed by the `desktopsChanged` signal.
    function runShellAction(kind, deId, shellId) {
        if (!root.actions)
            return;
        if (kind === "shell_install")
            root.actions.shellInstall(deId, shellId);
        else if (kind === "shell_switch")
            root.actions.shellSwitch(deId, shellId);
        else if (kind === "shell_remove")
            root.actions.shellRemove(deId, shellId);
    }

    // The "Like in the distribution" block: "Apply" and "Restore as it was".
    // Question, progress and result go through the shared operation window;
    // the tiles are re-read by the `desktopsChanged` signal
    // (→ `refreshInstalled`).
    function runLook(kind, lookId) {
        if (!root.actions)
            return;
        if (kind === "look_apply")
            root.actions.lookApply(lookId);
        else if (kind === "look_revert")
            root.actions.lookRevert(lookId);
    }

    function runTool(kind) {
        if (!root.actions)
            return;
        if (kind === "compare")
            root.actions.openCompare();
        else if (kind === "repair_current") {
            if (root.actions.repairCurrent())
                page.refreshInstalled();
        } else if (kind === "panel_backup")
            root.actions.panelBackup();
        else if (kind === "panel_restore")
            root.actions.panelRestore();
    }

    // The package version from the distribution repositories is requested
    // asynchronously: `pacman -Si` takes seconds, and a synchronous call would
    // stall the tab. The line is needed only where there is an "Install"
    // button.
    function requestVersion() {
        if (!root.actions || !root.card || root.card.deId === "")
            return;
        if (!root.card.available)
            return;
        // A request is already on its way — the package manager is not bothered a
        // second time: we get here both when the card changes and when the actions
        // object appears, and these can happen in any order.
        if (root.versionPending)
            return;
        root.versionPending = true;
        // For an installed desktop — its version (and, if the repository has a
        // newer one, that one next to it); for a non-installed one — what will
        // arrive.
        if (root.card.installed)
            root.actions.requestInstalledVersion(root.card.deId);
        else
            root.actions.requestVersion(root.card.deId);
    }

    // How many packages will ACTUALLY be installed and whether the set is
    // broken — by a trial transaction in the background; the answer is
    // `packagesReady`.
    function requestPackages() {
        if (!root.actions || !root.card || root.card.deId === "" || !root.card.packagesProbe)
            return;
        root.actions.requestPackages(root.card.deId, root.card.installed);
    }

    // The actions object is put in by the page host, and it does so AFTER the
    // page has been built: in `Component.onCompleted` it is not there yet,
    // `root.actions` is null there. Without this handler the version query to
    // the repositories went into the void — the "Available in your
    // distribution" line NEVER appeared, however long you waited. The same
    // approach as on the settings page: everything that needs the host is
    // asked here.
    onActionsChanged: {
        root.requestVersion();
        root.requestPackages();
    }

    Connections {
        target: root.actions

        function onVersionReady(deId, version) {
            if (!root.card || deId !== root.card.deId)
                return;
            if (root.card.installed)
                return;
            root.versionPending = false;
            root.versionText = page.versionLine(version);
        }

        function onInstalledVersionReady(deId, line) {
            if (!root.card || deId !== root.card.deId || !root.card.installed)
                return;
            root.versionPending = false;
            root.versionText = line;
        }

        function onPackagesReady(deId, installed, text, notice, blocked, status) {
            if (!root.card || deId !== root.card.deId || installed !== root.card.installed)
                return;
            root.statusOverride = status;
            root.packagesLine = text;
            root.packagesNotice = notice;
            root.packagesBlocked = blocked;
        }

        function onRebuildOfferReady(deId, installed, button, hint) {
            if (!root.card || deId !== root.card.deId || installed !== root.card.installed)
                return;
            root.rebuildButton = button;
            root.rebuildHint = hint;
        }
    }

    // Scroll the card to the element that received focus: without this Tab
    // takes focus past the bottom edge, and the tab looks frozen.
    function ensureVisible(item) {
        if (!item || !cardFlick.contentItem)
            return;
        // The window became active again (the screenshot dialog from Print was
        // closed, the user returned from another program) — Qt gives focus back
        // to the same element. This is not a Tab move: the page must not scroll
        // to it, otherwise a tab scrolled down jumps to the top.
        if (item.focusReason === Qt.ActiveWindowFocusReason)
            return;
        const pos = item.mapToItem(cardFlick.contentItem, 0, 0);
        const maxY = Math.max(0, cardFlick.contentHeight - cardFlick.height);
        if (pos.y < cardFlick.contentY)
            cardFlick.contentY = Math.max(0, pos.y - 12);
        else if (pos.y + item.height > cardFlick.contentY + cardFlick.height)
            cardFlick.contentY = Math.min(maxY,
                                          pos.y + item.height - cardFlick.height + 12);
    }

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Three panels: list, "Like in the distribution", card ──────────────

    SplitView {
        id: split
        anchors.fill: parent
        orientation: Qt.Horizontal

        // Handle: width and colour come from the core. The QtQuick Basic style
        // does not paint its own handle with the application palette and draws a
        // light grey strip (198,200,201) over the full height of the tab; in the
        // original the same place has a barely noticeable overlay (73,83,96 on the
        // standard skin).
        handle: Rectangle {
            implicitWidth: root.m.splitHandleWidth
            implicitHeight: root.m.splitHandleWidth
            color: root.pal.split_handle
        }

        // Right-to-left language: `SplitView` ignores mirroring (Qt lays the
        // panels out left to right in child order — screenshot with `-reverse`:
        // the list stayed on the left), so we reverse the panel order ourselves —
        // the list at the start of the line, on the right.
        readonly property bool panelsMirrored: split.LayoutMirroring.enabled
        onPanelsMirroredChanged: split.orderPanels()
        Component.onCompleted: split.orderPanels()

        function orderPanels() {
            const want = split.panelsMirrored ? [cardPanel, middlePanel, listPanel]
                                              : [listPanel, middlePanel, cardPanel];
            for (let i = 0; i < want.length; ++i) {
                for (let j = i; j < split.count; ++j) {
                    if (split.itemAt(j) === want[i]) {
                        if (j !== i)
                            split.moveItem(j, i);
                        break;
                    }
                }
            }
        }

        // ── Left panel: list of desktops ──────────────────────────────────
        Item {
            id: listPanel
            SplitView.preferredWidth: root.m.listInitialWidth
            SplitView.minimumWidth: root.m.listMinWidth
            SplitView.maximumWidth: root.m.listMaxWidth

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: root.m.listMarginLeft
                anchors.rightMargin: root.m.listMarginRight
                anchors.topMargin: root.m.listMarginTop
                anchors.bottomMargin: root.m.listMarginBottom
                spacing: root.m.listSpacing

                RexonText {
                    Layout.fillWidth: true
                    text: page.listHeader
                    color: root.colText
                    font.pointSize: page.listHeaderPointSize
                    font.bold: true
                }

                ListView {
                    id: deList
                    // The name is needed by the acceptance test bench: it selects the
                    // requested desktop by changing `currentIndex` — the same way as an arrow
                    // key does.
                    objectName: "deList"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: page
                    // There is no gap between the original's items: the item padding
                    // (`padding: 8px 12px`) is all the air it has.
                    spacing: 0
                    boundsBehavior: Flickable.StopAtBounds
                    // Focus is driven by the list itself: the button of the CURRENT row takes
                    // it through ListView.isCurrentItem. That way nobody from outside pokes
                    // into the delegate's innards — and that is exactly where focus used to
                    // get silently lost on any markup edit.
                    focus: true
                    currentIndex: -1
                    keyNavigationEnabled: true

                    ScrollBar.vertical: ScrollBar {}

                    // Where we went last time — by this we step over a section header in THE
                    // SAME direction.
                    property int lastRow: -1

                    // The list row and the shown card are one whole, as in the widget tab:
                    // there `itemSelectionChanged` redrew the card right on the arrow key, and
                    // the user did not have to confirm the choice after every step. Plus a
                    // section header is visible but not selectable (`NoItemFlags` in the
                    // original), so focus must not stay on it either — otherwise the keyboard
                    // and the gamepad get a "dead" step without a highlight.
                    onCurrentIndexChanged: {
                        if (deList.currentIndex < 0)
                            return;
                        if (page.roleValue(deList.currentIndex, "selectable") !== true) {
                            const dir = deList.currentIndex > deList.lastRow ? 1 : -1;
                            const next = deList.currentIndex + dir;
                            deList.currentIndex = (next >= 0 && next < deList.count)
                                                  ? next : deList.lastRow;
                            return;
                        }
                        deList.lastRow = deList.currentIndex;
                        page.select(page.roleValue(deList.currentIndex, "deId"));
                    }

                    delegate: Item {
                        id: listRow

                        required property int index
                        required property string kind
                        required property string deId
                        required property string text
                        required property string tooltip
                        required property bool selectable

                        width: ListView.view.width
                        // Both rows have the original's item padding: a section header has the
                        // same `padding: 8px 12px` as a desktop.
                        implicitHeight: listRow.selectable
                                        ? deButton.implicitHeight
                                        : sectionLabel.implicitHeight
                                          + 2 * root.m.listItemPaddingV

                        // Section header: visible but not selectable — in the original it had
                        // `NoItemFlags` and was painted with the PlaceholderText role of the skin
                        // palette.
                        RexonText {
                            id: sectionLabel
                            visible: !listRow.selectable
                            anchors.left: parent.left
                            anchors.leftMargin: root.m.listItemPaddingH
                            anchors.verticalCenter: parent.verticalCenter
                            text: listRow.text
                            color: root.pal.placeholder_text
                            font.pointSize: page.sectionPointSize
                            font.bold: true
                        }

                        Button {
                            id: deButton
                            visible: listRow.selectable
                            anchors.fill: parent
                            flat: true
                            text: listRow.text

                            readonly property bool current: page.selectedDe === listRow.deId

                            // Focus comes from the list together with "currentness".
                            focus: listRow.ListView.isCurrentItem

                            // The item padding belongs to the button, not to the text: that way the
                            // row height adds up exactly as in the original (text plus
                            // `padding: 8px 12px`).
                            padding: 0
                            topPadding: root.m.listItemPaddingV
                            bottomPadding: root.m.listItemPaddingV
                            leftPadding: root.m.listItemPaddingH
                            rightPadding: root.m.listItemPaddingH

                            background: Rectangle {
                                radius: root.m.listItemRadius
                                // List colours come from the core: a blue backing for the selected item
                                // and a light one under the cursor.
                                color: deButton.current
                                       ? root.pal.list_selected
                                       : (deButton.hovered || deButton.down
                                          ? root.pal.list_hover : "transparent")
                                border.color: deButton.activeFocus ? root.colAccent
                                                                   : "transparent"
                                border.width: 1
                            }

                            contentItem: RexonText {
                                text: deButton.text
                                color: deButton.current ? root.pal.list_selected_text
                                                        : root.colText
                                font.pointSize: page.listItemPointSize
                                font.weight: root.pal.text_secondary_weight
                                elide: Text.ElideRight
                                verticalAlignment: Text.AlignVCenter
                            }

                            ToolTip.visible: deButton.hovered && listRow.tooltip !== ""
                            ToolTip.text: listRow.tooltip

                            onClicked: {
                                deList.currentIndex = listRow.index;
                                page.select(listRow.deId);
                            }
                        }
                    }
                }
            }
        }

        // ── Second panel: "Like in the distribution" and "Tools" ──────────
        // The owner's decision 25.10.2026: in place of the former "Tools" — whole
        // looks "like in the distribution", one tile per look. The tiles, the mark
        // of the applied one and the buttons are assembled by the core
        // (`qt_desktops::look_tiles`); the progress of "Apply" and "Restore as it
        // was" goes through the shared operation window.
        // The owner's decision 28.10.2026: "Tools" came back into the same panel —
        // TO THE RIGHT of "Like in the distribution", buttons in a column, as
        // before 25.10; there is no strip at the bottom of the card any more. On a
        // narrow panel (a 1280 window) the two blocks do not fit side by side, and
        // "Tools" go UNDER "Like in the distribution" in the same column — the
        // core decides (`qt_desktops::looks_tools_side_by_side`).
        Rectangle {
            id: middlePanel
            // Width — sharing the window's extra room between the panels, as the
            // original's splitter does (stretch 0:1:4 with a cap for the card): the
            // core computes it. Otherwise QML gave all the extra to the card, and in a
            // 1400 window the screenshot frame came out 40 pixels wider than in the
            // original.
            SplitView.preferredWidth: page.looksPanelWidth(split.width, root.height)
            // Warning: the panel's MINIMUM width comes from its CONTENT, not from a
            // single number: in a standard 1100×750 window the splitter gives the panel
            // exactly 160 pixels, and the title would be cut by the edge of the
            // neighbouring card in the middle of a word. On a narrow panel the blocks
            // stand one under another, so the floor is the wider of the two. Qt
            // measures the label widths by the font; the floor, the cap and the
            // arithmetic are held by the core.
            SplitView.minimumWidth: page.looksPanelMinWidth(
                Math.max(looksHeaderText.implicitWidth, toolsHeaderText.implicitWidth),
                Math.max(looksColumn.implicitWidth, toolColumn.implicitWidth))
            // A light background and a right border: without them the stretched panel
            // reads as an empty hole between the list and the card.
            color: root.pal.tools_bg

            Rectangle {
                anchors.right: parent.right
                width: 1
                height: parent.height
                color: root.pal.tools_border
            }

            Flickable {
                id: looksFlick
                anchors.fill: parent
                anchors.leftMargin: root.m.toolsMarginLeft
                anchors.rightMargin: root.m.toolsMarginRight
                anchors.topMargin: root.m.toolsMarginTop
                anchors.bottomMargin: root.m.toolsMarginBottom
                contentWidth: width
                contentHeight: middleBody.implicitHeight
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {}

                Item {
                    id: middleBody
                    objectName: "middleBody"

                    // Side by side or in a column, and the block width — the core. The blocks
                    // ask for their UNcompressed width (title and column), which does not
                    // depend on the width of the block itself — there is no layout loop.
                    readonly property bool sideBySide: page.looksToolsSideBySide(
                        looksFlick.width,
                        Math.max(looksHeaderText.implicitWidth, looksColumn.implicitWidth),
                        Math.max(toolsHeaderText.implicitWidth, toolColumn.implicitWidth))
                    readonly property int blockWidth: page.looksToolsBlockWidth(
                        looksFlick.width, middleBody.sideBySide)
                    readonly property int gap: root.m.looksToolsGap

                    width: looksFlick.width
                    implicitHeight: middleBody.sideBySide
                                    ? Math.max(looksBody.implicitHeight, toolsBody.implicitHeight)
                                    : looksBody.implicitHeight + middleBody.gap
                                      + toolsBody.implicitHeight

                    // ── "Like in the distribution" ────────────────────────
                    ColumnLayout {
                        id: looksBody
                        // Margins from the edge are anchors: for right to left the blocks swap
                        // sides by themselves (`LayoutMirroring`).
                        anchors.left: parent.left
                        y: 0
                        width: middleBody.blockWidth
                        spacing: root.m.toolsSpacing

                        RexonText {
                            id: looksHeaderText
                            Layout.fillWidth: true
                            text: page.looksHeader
                            color: root.colText
                            font.pointSize: page.listHeaderPointSize
                            font.bold: true
                        }

                        // The explanation spans the block width (we do not put it into the tile
                        // column: a long line would stretch the column to its own width).
                        RexonText {
                            Layout.fillWidth: true
                            Layout.preferredWidth: looksBody.width
                            text: page.looksHint
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.packagesPointSize
                        }

                        // Tile column: by the longest label, with the core's cap and NEVER wider
                        // than the block itself.
                        ColumnLayout {
                            id: looksColumn
                            Layout.fillWidth: true
                            Layout.maximumWidth: looksBody.width
                            spacing: root.m.toolsSpacing

                            Repeater {
                                model: page.looks

                                delegate: Rectangle {
                                    id: lookTile
                                    required property var modelData
                                    objectName: "lookTile_" + lookTile.modelData.id
                                    Layout.fillWidth: true
                                    implicitWidth: tileBody.implicitWidth
                                                   + 2 * root.m.shellsPanelPadding
                                    implicitHeight: tileBody.implicitHeight
                                                    + 2 * root.m.shellsPanelPadding
                                    radius: root.m.carouselFrameRadius
                                    color: root.colCard
                                    // The applied look is framed in the mark colour.
                                    border.color: lookTile.modelData.applied
                                                  ? root.pal.current_badge : root.pal.tools_border
                                    border.width: 1

                                    // The look description from the manifest goes into a tooltip: it does not
                                    // fit on the tile.
                                    HoverHandler {
                                        id: tileHover
                                    }
                                    ToolTip.visible: tileHover.hovered
                                                     && lookTile.modelData.description !== ""
                                    ToolTip.text: lookTile.modelData.description
                                    ToolTip.delay: 500

                                    ColumnLayout {
                                        id: tileBody
                                        anchors.left: parent.left
                                        anchors.right: parent.right
                                        anchors.top: parent.top
                                        anchors.margins: root.m.shellsPanelPadding
                                        spacing: root.m.statSpacing * 3

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: lookTile.modelData.name
                                            color: root.colText
                                            font.bold: true
                                            wrapMode: Text.WordWrap
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            visible: lookTile.modelData.badge !== ""
                                            text: lookTile.modelData.badge
                                            color: root.pal.current_badge
                                            font.weight: Font.DemiBold
                                        }

                                        Repeater {
                                            model: lookTile.modelData.buttons

                                            delegate: RexonButton {
                                                id: lookButton
                                                required property var modelData
                                                Layout.fillWidth: true
                                                implicitHeight: page.toolButtonMinHeight
                                                pal: root.pal
                                                tone: lookButton.modelData.style === "primary"
                                                      ? root.pal.primary_tone : root.pal.neutral_tone
                                                text: lookButton.modelData.text
                                                ToolTip.visible: lookButton.hovered
                                                                 && lookButton.modelData.tooltip !== ""
                                                ToolTip.text: lookButton.modelData.tooltip
                                                onClicked: root.runLook(lookButton.modelData.kind,
                                                                        lookTile.modelData.id)
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Border between the blocks — in the middle of the gap: vertical when the
                    // blocks are side by side, horizontal when "Tools" moved down.
                    Rectangle {
                        anchors.left: parent.left
                        anchors.leftMargin: middleBody.sideBySide
                           ? middleBody.blockWidth + Math.floor(middleBody.gap / 2) : 0
                        y: middleBody.sideBySide
                           ? 0 : looksBody.implicitHeight + Math.floor(middleBody.gap / 2)
                        width: middleBody.sideBySide ? 1 : middleBody.width
                        height: middleBody.sideBySide
                                ? Math.max(looksFlick.height, middleBody.implicitHeight) : 1
                        color: root.pal.tools_border
                    }

                    // ── "Tools": a column of buttons ──────────────────────
                    // The buttons and why some are disabled come from the core
                    // (`qt_desktops::tool_buttons`). The look is as before 25.10.
                    ColumnLayout {
                        id: toolsBody
                        objectName: "toolsBlock"
                        anchors.left: parent.left
                        anchors.leftMargin: middleBody.sideBySide
                                            ? middleBody.blockWidth + middleBody.gap : 0
                        y: middleBody.sideBySide ? 0 : looksBody.implicitHeight + middleBody.gap
                        width: middleBody.blockWidth
                        spacing: root.m.toolsSpacing

                        RexonText {
                            id: toolsHeaderText
                            Layout.fillWidth: true
                            text: page.toolsHeader
                            color: root.colText
                            font.pointSize: page.listHeaderPointSize
                            font.bold: true
                        }

                        // Button column as wide as the LONGEST label, with the core's cap, pressed
                        // to the left and not stretched over the whole block. Warning: and NEVER
                        // WIDER THAN THE BLOCK ITSELF: if the user drags the divider in, the
                        // buttons stay inside the panel.
                        ColumnLayout {
                            id: toolColumn
                            Layout.alignment: Qt.AlignLeft
                            Layout.fillWidth: false
                            Layout.maximumWidth: Math.min(root.m.toolButtonMaxWidth,
                                                          toolsBody.width)
                            spacing: root.m.toolsSpacing

                            Repeater {
                                model: page.toolButtons

                                delegate: RexonButton {
                                    id: toolButton
                                    required property var modelData
                                    Layout.fillWidth: true
                                    implicitHeight: page.toolButtonMinHeight
                                    pal: root.pal
                                    text: toolButton.modelData.text
                                    // SteamOS: the package button is visible but disabled — the reason is a
                                    // line below (`toolsNotice`).
                                    enabled: toolButton.modelData.enabled !== false
                                    ToolTip.visible: toolButton.hovered
                                    ToolTip.text: toolButton.modelData.tooltip
                                    onClicked: root.runTool(toolButton.modelData.kind)
                                }
                            }
                        }

                        // Why some buttons are disabled — one phrase across the block width (we do
                        // not put it into the button column: a long line would stretch the column
                        // to its own width).
                        RexonText {
                            Layout.fillWidth: true
                            Layout.preferredWidth: toolsBody.width
                            visible: page.toolsNotice !== ""
                            text: page.toolsNotice
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                        }
                    }
                }
            }
        }

        // ── Right panel: card of the selected desktop ─────────────────────
        Item {
            id: cardPanel
            SplitView.fillWidth: true
            // The width cap comes from the window height, as with the original's
            // splitter: beyond it the extra width goes to the middle panel ("Like in
            // the distribution" and "Tools"), not into empty space around the card.
            SplitView.maximumWidth: page.detailPanelMaxWidth(root.height)

            // An invitation to choose a desktop — when none is selected at all.
            RexonText {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.verticalCenter: cardFlick.verticalCenter
                visible: !root.card || root.card.deId === ""
                // The arrow points at the desktop list (`RexonArrows.keyHint`).
                text: root.arrows.keyHint(page.placeholderText)
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
                font.pointSize: root.m.placeholderPointSize
            }

            Flickable {
                id: cardFlick
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                // The card takes the full panel height (since 28.10.2026 there is no
                // "Tools" strip under it); the screenshot frame shrinks to make room for
                // the window footer (`carouselHeightFitted`).
                anchors.bottom: parent.bottom
                visible: root.card && root.card.deId !== ""
                contentWidth: width
                contentHeight: cardColumn.implicitHeight + root.m.cardMarginTop
                               + root.m.cardMarginBottom
                clip: true
                boundsBehavior: Flickable.StopAtBounds

                // The scroll bar is always visible: its appearance would change the
                // area's width, and the height of the screenshot frame (16:9) depends on
                // the width — that gave a jittering loop.
                ScrollBar.vertical: ScrollBar {
                    id: cardScroll
                    policy: ScrollBar.AlwaysOn
                }

                // Warning: a layout inside an ordinary Item must get its height by hand,
                // otherwise it collapses into a strip.
                ColumnLayout {
                    id: cardColumn

                    // In the original the scroll bar stands next to the card, not over it —
                    // its width is subtracted from the viewport.
                    readonly property int viewport: cardFlick.width - cardScroll.width
                    // The card is pressed to the RIGHT edge of the panel: in the original its
                    // wrapper divides the width between a spacer and the card 100:1, and a
                    // hundredth of the viewport remains on the left. The core computes it.
                    readonly property int gapLeft: page.cardLeftSpacer(viewport)
                                                   + page.cardGapLeft

                    // The margin from the start of the line is an anchor: for right to left
                    // the scroll bar moves to the left, and the card goes towards it.
                    anchors.left: parent.left
                    anchors.leftMargin: cardColumn.gapLeft
                    y: root.m.cardMarginTop
                    width: Math.min(cardColumn.viewport - cardColumn.gapLeft
                                    - root.m.cardMarginRight,
                                    page.cardMaxWidth)
                    height: implicitHeight
                    spacing: root.m.cardSpacing

                    // ── Screenshot carousel ───────────────────────────────
                    Item {
                        id: carousel
                        Layout.fillWidth: true
                        // The frame is always 16:9 and does not jump between slides: the card
                        // text is aligned to its left edge.
                        implicitHeight: frame.height + dots.implicitHeight
                                        + root.m.carouselDotsSpacing

                        readonly property var images: root.card && root.card.images
                                                      ? root.card.images : []
                        readonly property int count: carousel.images.length
                        /// "Back/forward" icons follow the pager's mirroring: for right to left
                        /// "forward" is to the left (`RexonArrows`).
                        readonly property RexonArrows arrows: RexonArrows {
                            mirrored: carousel.LayoutMirroring.enabled
                        }
                        readonly property string source: carousel.count > 0
                            ? carousel.images[Math.min(root.slide, carousel.count - 1)]
                            : ""

                        function step(direction) {
                            if (carousel.count === 0)
                                return;
                            root.slide = (root.slide + direction + carousel.count)
                                         % carousel.count;
                        }

                        Rectangle {
                            id: frame
                            anchors.right: parent.right
                            // A 16:9 frame from its own height plus the picture's side margins; the
                            // height comes from the width WITHOUT those margins, as in the original
                            // (`self.width() - 16`).
                            readonly property int sidePadding: 2 * root.m.carouselFramePaddingH
                            // Frame height in a window that has enough height. The width is computed
                            // FROM IT — and only from it; see the warning about the "ratchet" at
                            // `carousel_height_fitted` in the core.
                            readonly property int fullHeight:
                                page.carouselHeight(root.height,
                                                    parent.width - frame.sidePadding)
                            // How much height is left for the frame after everything else in the
                            // card: margins, the gap, the indicator dots and the text column.
                            // EVERYTHING except the frame itself is measured — otherwise the rule
                            // would be circular.
                            readonly property int room:
                                cardFlick.height - root.m.cardMarginTop
                                - root.m.cardMarginBottom - root.m.cardSpacing
                                - root.m.carouselDotsSpacing - dots.implicitHeight
                                - cardBody.implicitHeight
                            width: Math.min(parent.width,
                                            frame.fullHeight * 16 / 9 + frame.sidePadding)
                            // The frame gives height to the card when the window is short of it: in a
                            // window of the minimum size the status footer cut off the bottom row of
                            // buttons — now the screenshot becomes slightly smaller instead. How much
                            // to give and where the limit is — the core decides.
                            height: page.carouselHeightFitted(frame.fullHeight, frame.room)
                            color: root.pal.carousel_frame
                            radius: root.m.carouselFrameRadius
                            clip: true

                            // Margins around a screenshot whose sides are not 16:9 (Pantheon — almost
                            // 4:3): the same picture squeezed down to a handful of pixels and
                            // stretched back — that is the blur, without the effects module (it does
                            // not exist in Qt older than 6.5) — and muted with the frame colour so it
                            // does not compete with the screenshot.
                            Image {
                                id: shotBackdrop
                                anchors.fill: shot
                                visible: shot.visible && !shot.fits
                                         && shot.status === Image.Ready
                                source: carousel.source
                                sourceSize: Qt.size(32, 18)
                                fillMode: Image.PreserveAspectCrop
                                smooth: true
                                asynchronous: true
                                cache: true

                                Rectangle {
                                    anchors.fill: parent
                                    color: root.pal.carousel_frame
                                    opacity: 0.55
                                }
                            }

                            Image {
                                id: shot
                                anchors.fill: parent
                                // Margins only on the sides: the original's frame has none at the top and
                                // bottom.
                                anchors.leftMargin: root.m.carouselFramePaddingH
                                anchors.rightMargin: root.m.carouselFramePaddingH
                                visible: carousel.source !== ""
                                source: carousel.source
                                // Warning: the screenshot is shown WHOLE: cropping to the 16:9 frame cut
                                // both the top panel and the bottom dock off 4:3 screenshots — the card
                                // was left with "just a landscape" instead of a desktop (Pantheon, 08.10).
                                // The margins are covered by a blurred copy under the screenshot.
                                // A small source (an SVG placeholder) is NOT stretched — the original
                                // shows it as is, centred.
                                readonly property bool fits: shot.sourceSize.width > 0
                                    && shot.sourceSize.width <= shot.width
                                    && shot.sourceSize.height <= shot.height
                                fillMode: shot.fits ? Image.Pad : Image.PreserveAspectFit
                                horizontalAlignment: Image.AlignHCenter
                                verticalAlignment: Image.AlignVCenter
                                asynchronous: true
                                cache: true

                                MouseArea {
                                    anchors.fill: parent
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: zoom.open()
                                    ToolTip.visible: containsMouse
                                    ToolTip.text: root.carouselTexts.zoomTooltip
                                    hoverEnabled: true
                                }
                            }

                            RexonText {
                                anchors.centerIn: parent
                                visible: carousel.count === 0
                                text: root.carouselTexts.emptyText
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                font.pointSize: root.m.noScreenshotsPointSize
                            }

                            // The arrows are an overlay over the picture edges, not layout slots:
                            // gutters would push the screenshot away from the frame edge. Round and
                            // dark, as in the original: tone and side come from the core.
                            RexonButton {
                                id: prevBtn
                                visible: carousel.count > 1
                                anchors.left: parent.left
                                anchors.leftMargin: root.m.overlayMargin
                                anchors.verticalCenter: parent.verticalCenter
                                width: root.m.arrowSize
                                implicitHeight: root.m.arrowSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.arrow_tone
                                font.pointSize: root.m.arrowPointSize
                                text: carousel.arrows.previous
                                ToolTip.visible: prevBtn.hovered
                                ToolTip.text: root.carouselTexts.prevTooltip
                                onClicked: carousel.step(-1)
                            }

                            RexonButton {
                                id: nextBtn
                                visible: carousel.count > 1
                                anchors.right: parent.right
                                anchors.rightMargin: root.m.overlayMargin
                                anchors.verticalCenter: parent.verticalCenter
                                width: root.m.arrowSize
                                implicitHeight: root.m.arrowSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.arrow_tone
                                font.pointSize: root.m.arrowPointSize
                                text: carousel.arrows.next
                                ToolTip.visible: nextBtn.hovered
                                ToolTip.text: root.carouselTexts.nextTooltip
                                onClicked: carousel.step(1)
                            }

                            RexonButton {
                                id: zoomBtn
                                visible: carousel.count > 0
                                anchors.right: parent.right
                                anchors.top: parent.top
                                anchors.margins: root.m.overlayMargin
                                width: root.m.zoomButtonSize
                                implicitHeight: root.m.zoomButtonSize
                                padding: 0
                                pal: root.pal
                                tone: root.pal.zoom_button_tone
                                font.pointSize: root.m.zoomButtonPointSize
                                text: "🔍"
                                ToolTip.visible: zoomBtn.hovered
                                ToolTip.text: root.carouselTexts.zoomButtonTooltip
                                onClicked: zoom.open()
                            }
                        }

                        // Indicator dots centred under the screenshot column: the original's muted
                        // text — the same secondary colour and weight, without an opacity of our
                        // own.
                        RexonText {
                            id: dots
                            anchors.top: frame.bottom
                            anchors.topMargin: root.m.carouselDotsSpacing
                            anchors.horizontalCenter: frame.horizontalCenter
                            text: page.carouselDots(root.slide, carousel.count)
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }

                    // ── Under the screenshot: desktop text on the left, "Shells" on the right ─
                    // The "Shells" block exists only for Hyprland, niri and sway (the owner's
                    // decision 01.10.2026); the other desktops have none, and the text column
                    // takes the full width, as before. The block stands on the right when the
                    // card is wide enough; in a narrow window it moves under the desktop text
                    // (from what width — the core decides).
                    GridLayout {
                        id: cardBody
                        readonly property bool sideBySide:
                            cardBody.width >= root.m.shellsSideBySideMinWidth
                        Layout.alignment: Qt.AlignRight
                        Layout.preferredWidth: frame.width
                        Layout.maximumWidth: frame.width
                        columns: cardBody.sideBySide ? 2 : 1
                        columnSpacing: root.m.cardSpacing
                        rowSpacing: root.m.cardSpacing

                    // ── Text column ───────────────────────────────────────
                    ColumnLayout {
                        id: textColumn
                        // The row gives out the width: everything left over from the "Shells"
                        // block. Without a zero preferred width the column would ask for the width
                        // of its longest line, and the block on the right would slide past the
                        // card edge.
                        Layout.fillWidth: true
                        Layout.preferredWidth: 0
                        Layout.minimumWidth: 0
                        Layout.alignment: Qt.AlignTop
                        spacing: root.m.cardSpacing

                        // Title and status
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: root.m.headerRowSpacing

                            RexonText {
                                Layout.fillWidth: true
                                text: root.card.title
                                color: root.colText
                                font.pointSize: page.cardTitlePointSize
                                font.bold: true
                                elide: Text.ElideRight
                            }

                            RexonText {
                                text: root.statusOverride !== "" ? root.statusOverride : root.card.statusText
                                color: root.statusOverride !== "" ? root.card.statusPartialColor
                                       : root.card.statusColor === ""
                                       ? root.colTextSecondary : root.card.statusColor
                                // An unpainted status is secondary text, and the original sets that in
                                // weight 600.
                                font.weight: root.card.statusBold
                                             ? Font.Bold : root.pal.text_secondary_weight
                                font.pointSize: page.cardStatusPointSize
                            }
                        }

                        // Description
                        RexonText {
                            Layout.fillWidth: true
                            text: root.card.description
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.descPointSize
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: root.pal.separator
                        }

                        // Specs strip: memory, load, customization, games
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 0

                            Repeater {
                                model: root.card.stats

                                delegate: ColumnLayout {
                                    id: statCell
                                    required property var modelData
                                    Layout.fillWidth: true
                                    // Columns of EQUAL width, like the 1:1:1:1 stretch in the original:
                                    // without it the width was divided by label length, and "RAM" moved 16
                                    // pixels to the left.
                                    Layout.preferredWidth: 1
                                    spacing: root.m.statSpacing

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: statCell.modelData.icon
                                        color: root.colText
                                        horizontalAlignment: Text.AlignHCenter
                                    }

                                    RexonText {
                                        Layout.fillWidth: true
                                        // An empty value holds its place but is invisible: otherwise the columns
                                        // come out at different heights.
                                        text: statCell.modelData.value === ""
                                              ? "·" : statCell.modelData.value
                                        opacity: statCell.modelData.value === "" ? 0 : 1
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        horizontalAlignment: Text.AlignHCenter
                                    }

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: statCell.modelData.label
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        font.pointSize: root.m.statLabelPointSize
                                        horizontalAlignment: Text.AlignHCenter
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: 1
                            color: root.pal.separator
                        }

                        // How many packages will be installed and from where; for an installed
                        // desktop — whether everything is in place.
                        RexonText {
                            Layout.fillWidth: true
                            text: root.packagesLine !== "" ? root.packagesLine
                                                           : root.card.packagesText
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.packagesPointSize
                        }

                        // What from the set does not install right now (waiting for a rebuild in
                        // the repository) — before pressing "Install", not after the password in
                        // the pacman log.
                        RexonText {
                            Layout.fillWidth: true
                            visible: root.packagesNotice !== ""
                            text: root.packagesNotice
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                            font.pointSize: root.m.packagesPointSize
                        }

                        // Such a package can be built from the official Arch recipe right here — a
                        // button with an explanation under the warning.
                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.rebuildButton !== ""
                            spacing: root.m.buttonRowSpacing

                            RexonButton {
                                id: rebuildButton
                                pal: root.pal
                                tone: root.pal.neutral_tone
                                topPadding: root.m.cardButtonPaddingV
                                bottomPadding: root.m.cardButtonPaddingV
                                leftPadding: root.m.cardButtonPaddingH
                                rightPadding: root.m.cardButtonPaddingH
                                text: root.rebuildButton
                                onClicked: if (root.actions)
                                    root.actions.rebuildFromSources(root.card.deId)
                                onActiveFocusChanged: if (activeFocus)
                                    root.ensureVisible(rebuildButton)
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: root.rebuildHint
                                wrapMode: Text.WordWrap
                                color: root.colTextSecondary
                                font.weight: root.pal.text_secondary_weight
                                font.pointSize: root.m.packagesPointSize
                            }
                        }

                        // ── Action buttons ───────────────────────────────
                        RowLayout {
                            Layout.fillWidth: true
                            Layout.topMargin: root.m.buttonRowTopMargin
                            spacing: root.m.buttonRowSpacing

                            // The desktop is not available on this distribution — an explanation
                            // instead of buttons.
                            RexonText {
                                visible: !root.card.available
                                text: root.card.notice
                                color: root.pal.status_unavailable
                            }

                            // This is the current desktop: neither "Switch" (we are already in it) nor
                            // "Delete" (we would be left without graphics).
                            RexonText {
                                id: currentBadge
                                visible: root.card.isCurrent && root.card.available
                                text: root.card.currentBadgeText
                                color: root.pal.current_badge
                                font.weight: Font.DemiBold
                                ToolTip.visible: badgeHover.hovered
                                ToolTip.text: root.card.currentBadgeTooltip

                                HoverHandler {
                                    id: badgeHover
                                }
                            }

                            // Install and switch are blue, check is neutral with a border, remove is
                            // red. All three tones and the paddings come from the core; there is one
                            // button (the shared `RexonButton`), only the tone changes.
                            Repeater {
                                model: root.card.buttons

                                delegate: RexonButton {
                                    id: actionButton
                                    required property var modelData
                                    pal: root.pal
                                    tone: actionButton.modelData.style === "primary"
                                          ? root.pal.primary_tone
                                          : (actionButton.modelData.style === "destructive"
                                             ? root.pal.destructive_tone
                                             : root.pal.neutral_tone)
                                    topPadding: root.m.cardButtonPaddingV
                                    bottomPadding: root.m.cardButtonPaddingV
                                    leftPadding: root.m.cardButtonPaddingH
                                    rightPadding: root.m.cardButtonPaddingH
                                    text: actionButton.modelData.text
                                    // SteamOS: the button is visible but disabled; the reason is a line under
                                    // the buttons.
                                    // A conflict of the set with something installed: the same for the
                                    // buttons that install the set, the reason is a ⛔ line above the buttons.
                                    enabled: actionButton.modelData.enabled !== false
                                             && !(root.packagesBlocked
                                                  && actionButton.modelData.installsSet === true)
                                    ToolTip.visible: actionButton.hovered
                                                     && actionButton.modelData.tooltip !== ""
                                    ToolTip.text: actionButton.modelData.tooltip
                                    onClicked: root.runAction(actionButton.modelData.kind,
                                                              root.card.deId)
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(actionButton)
                                }
                            }

                            Item {
                                Layout.fillWidth: true
                            }
                        }
                        // The version that will ACTUALLY be installed from this distribution's
                        // repositories. While the query is running — a soft placeholder; without a
                        // package the line quietly disappears.
                        // A separate line under the buttons: next to the three buttons it had room
                        // for a word and a half ("Installed vers…", acceptance 10.10.2026).
                        RexonText {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            elide: Text.ElideRight
                            visible: root.versionPending || root.versionText !== ""
                            text: root.versionPending
                                  ? root.card.versionChecking : root.versionText
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            font.pointSize: root.m.versionPointSize
                        }

                        // SteamOS (Steam Deck and Steam Machine): why the buttons above are
                        // disabled — one phrase from the core.
                        RexonText {
                            Layout.fillWidth: true
                            visible: (root.card.blockedNotice || "") !== ""
                            text: root.card.blockedNotice || ""
                            wrapMode: Text.WordWrap
                            color: root.pal.status_unavailable
                        }
                    }

                    // ── "Shells" block ────────────────────────────────────
                    // An add-on over the desktop: each shell has its own "Install", and after
                    // installation "Switch" (log into this same desktop with the shell) and
                    // "Remove" (the desktop stays). Labels, the set of buttons and why a button
                    // is disabled — the core (`qt_desktops::shell_rows`).
                    Rectangle {
                        id: shellsPanel
                        readonly property var rows: root.card && root.card.shells
                                                    ? root.card.shells : []
                        visible: shellsPanel.rows.length > 0
                        Layout.alignment: Qt.AlignTop
                        Layout.fillWidth: !cardBody.sideBySide
                        Layout.preferredWidth: cardBody.sideBySide
                                               ? root.m.shellsPanelWidth : -1
                        Layout.maximumWidth: cardBody.sideBySide
                                             ? root.m.shellsPanelWidth : Number.POSITIVE_INFINITY
                        implicitHeight: shellsColumn.implicitHeight
                                        + 2 * root.m.shellsPanelPadding
                        color: root.pal.tools_bg
                        border.color: root.pal.tools_border
                        radius: root.m.carouselFrameRadius

                        ColumnLayout {
                            id: shellsColumn
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: root.m.shellsPanelPadding
                            spacing: root.m.shellsRowSpacing

                            RexonText {
                                id: shellsHeaderText
                                Layout.fillWidth: true
                                text: root.card.shellsHeader || ""
                                color: root.colText
                                font.bold: true
                                font.pointSize: root.m.shellsHeaderPointSize
                                ToolTip.visible: shellsHeaderHover.hovered
                                ToolTip.text: root.card.shellsTooltip || ""

                                HoverHandler {
                                    id: shellsHeaderHover
                                }
                            }

                            Repeater {
                                model: shellsPanel.rows

                                delegate: ColumnLayout {
                                    id: shellRow
                                    required property var modelData
                                    required property int index
                                    Layout.fillWidth: true
                                    spacing: root.m.statSpacing * 2

                                    Rectangle {
                                        Layout.fillWidth: true
                                        implicitHeight: 1
                                        color: root.pal.separator
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: shellRow.modelData.name
                                            color: root.colText
                                            font.bold: true
                                            elide: Text.ElideRight
                                        }

                                        RexonText {
                                            visible: shellRow.modelData.statusText !== ""
                                            text: shellRow.modelData.statusText
                                            color: shellRow.modelData.running
                                                   ? root.pal.current_badge
                                                   : root.pal.status_installed
                                            font.weight: Font.DemiBold
                                        }
                                    }

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: shellRow.modelData.description
                                        wrapMode: Text.WordWrap
                                        color: root.colTextSecondary
                                        font.weight: root.pal.text_secondary_weight
                                        font.pointSize: root.m.packagesPointSize
                                    }

                                    Flow {
                                        Layout.fillWidth: true
                                        spacing: root.m.buttonRowSpacing

                                        Repeater {
                                            model: shellRow.modelData.buttons

                                            delegate: RexonButton {
                                                id: shellButton
                                                required property var modelData
                                                pal: root.pal
                                                tone: shellButton.modelData.style === "primary"
                                                      ? root.pal.primary_tone
                                                      : (shellButton.modelData.style === "destructive"
                                                         ? root.pal.destructive_tone
                                                         : root.pal.neutral_tone)
                                                topPadding: root.m.cardButtonPaddingV
                                                bottomPadding: root.m.cardButtonPaddingV
                                                leftPadding: root.m.cardButtonPaddingH
                                                rightPadding: root.m.cardButtonPaddingH
                                                text: shellButton.modelData.text
                                                enabled: shellButton.modelData.enabled !== false
                                                ToolTip.visible: shellButton.hovered
                                                                 && shellButton.modelData.tooltip !== ""
                                                ToolTip.text: shellButton.modelData.tooltip
                                                onClicked: root.runShellAction(
                                                               shellButton.modelData.kind,
                                                               root.card.deId,
                                                               shellRow.modelData.shell)
                                                onActiveFocusChanged: if (activeFocus)
                                                    root.ensureVisible(shellButton)
                                            }
                                        }
                                    }

                                    // Why the button is disabled — in words, not only as a tooltip under the
                                    // cursor (gamepad).
                                    RexonText {
                                        Layout.fillWidth: true
                                        visible: shellRow.modelData.hint !== ""
                                        text: shellRow.modelData.hint
                                        wrapMode: Text.WordWrap
                                        color: root.pal.status_unavailable
                                        font.pointSize: root.m.packagesPointSize
                                    }
                                }
                            }
                        }
                    }
                    }
                }
            }
        }
    }

    // ── Screenshot zoom ───────────────────────────────────────────────────

    // A SEPARATE FULL-SCREEN WINDOW, not a pop-up inside the tab.
    //
    // There used to be a tab-sized `Popup` here, and "zoom to full screen"
    // turned out to be untrue: the section strip at the top and the status
    // footer at the bottom remained over the screenshot, and the margins
    // around it were the page margins. The excuse "the scene lives inside a
    // QQuickWidget" is irrelevant: a QML `Window` is an INDEPENDENT top-level
    // window, not bounded by the host scene, and it opens the same way from
    // the Rust window and from a tab inside a QQuickWidget. In the widget
    // original this is a frameless modal `QDialog` with `showFullScreen()` in
    // `showEvent` (`rexon/qt_desktops/window.py`) — here exactly the same
    // three properties.
    //
    // Exit leaves the window as it was: the viewer did not change the main
    // window's size (it did not live in it) and did not touch the tab layout —
    // closing simply hides the window and returns control to whoever opened
    // it.
    //
    // `transientParent: null` is deliberate. By default a nested `Window`
    // becomes transient for the scene's window, and a QQuickWidget host has no
    // real window at all: tying it to that would make the viewer unopenable
    // from one of the two entry points.
    Window {
        id: zoom

        // A separate window does not inherit the main window's root mirroring — it
        // turns on its own (the same rule as in MainWindow.qml).
        LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
        LayoutMirroring.childrenInherit: true

        transientParent: null
        flags: Qt.Window | Qt.FramelessWindowHint
        modality: Qt.ApplicationModal
        // The title comes from the core, as in the original (`setWindowTitle`).
        // The window has no frame, but the window switcher and the task bar see
        // the title.
        title: root.carouselTexts.zoomTitle
        // Viewer background — from the core (`rgba(0,0,0,0.95)` in the original).
        color: root.pal.zoom_bg

        /// Open the viewer in full screen. The name `open()` is kept as before: it
        /// is called both by a click on the screenshot and by the magnifier button
        /// under it.
        /// Focus inside the window is NOT taken here: it is taken by the show
        /// handler at the bottom, one for all the ways the window appears.
        ///
        /// Warning: the screen size is set BY HAND, before the full-screen request,
        /// and this is not a precaution for the sake of looks. `showFullScreen()`
        /// is exactly a request: the window manager fulfils it, and where there is
        /// none or where it did not understand it, the window would stay the size
        /// of its content. Measurement on a test bench without a window manager:
        /// the viewer came out as a 160×150 window in the top left corner — so
        /// "full screen" would again have been untrue, just for a different
        /// reason. With these two lines the window takes the screen in any case,
        /// and full-screen mode on top of that also removes the compositor panels.
        function open() {
            zoom.x = 0;
            zoom.y = 0;
            zoom.width = Screen.width;
            zoom.height = Screen.height;
            zoom.showFullScreen();
            zoom.raise();
            zoom.requestActivate();
        }

        /// Close the viewer. There is ONE exit point, and all four paths call it:
        /// a click on the screenshot, the cross, Esc and Q.
        function hideZoom() {
            zoom.close();
        }

        Item {
            id: zoomBody

            anchors.fill: parent

            // Keys are listened to by the window BODY ITSELF: `Keys` is an item
            // property, and a window is not an item. And on the first frame focus is
            // taken by the body, not by the "×" button: while focus was on the button,
            // Space PRESSED it (the viewer closed) instead of paging, although in the
            // widget viewer Space paged forward.
            focus: true

            /// Icons and the ← → keys (and their A/D pairs) follow the viewer window's
            /// mirroring: for right to left "forward" is to the left. Space is always
            /// forward: it is about order, not direction.
            readonly property RexonArrows arrows: RexonArrows {
                mirrored: zoomBody.LayoutMirroring.enabled
            }

            // The set is as in the widget viewer (`keyPressEvent`): ←/A back, →/D/Space
            // forward, Esc/Q close (for right to left ←/A and →/D swap roles —
            // `zoomBody.arrows`).
            //
            // Warning: Esc is named here EXPLICITLY. In a `Popup` it was closed by
            // `closePolicy`; a window has no such property at all: while the line
            // below was missing, the full-screen viewer did not close with the escape
            // key — and the user has exactly two other ways out of a frameless window,
            // both of which must be found with the mouse.
            Keys.onPressed: (event) => {
                switch (event.key) {
                case Qt.Key_Left:
                case Qt.Key_A:
                    carousel.step(zoomBody.arrows.leftStep);
                    event.accepted = true;
                    break;
                case Qt.Key_Right:
                case Qt.Key_D:
                    carousel.step(zoomBody.arrows.rightStep);
                    event.accepted = true;
                    break;
                case Qt.Key_Space:
                    carousel.step(1);
                    event.accepted = true;
                    break;
                case Qt.Key_Escape:
                case Qt.Key_Q:
                    zoom.hideZoom();
                    event.accepted = true;
                    break;
                }
            }

            Image {
                anchors.fill: parent
                anchors.margins: root.m.zoomMargin
                anchors.bottomMargin: root.m.zoomArrowSize
                source: carousel.source
                fillMode: Image.PreserveAspectFit
                asynchronous: true

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: zoom.hideZoom()
                }
            }

            RexonButton {
                id: zoomPrev
                visible: carousel.count > 1
                anchors.left: parent.left
                anchors.leftMargin: root.m.zoomMargin
                anchors.verticalCenter: parent.verticalCenter
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomArrowPointSize
                text: zoomBody.arrows.previous
                ToolTip.visible: zoomPrev.hovered
                ToolTip.text: zoomBody.arrows.keyHint(root.carouselTexts.zoomPrevTooltip)
                onClicked: carousel.step(-1)
            }

            RexonButton {
                id: zoomNext
                visible: carousel.count > 1
                anchors.right: parent.right
                anchors.rightMargin: root.m.zoomMargin
                anchors.verticalCenter: parent.verticalCenter
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomArrowPointSize
                text: zoomBody.arrows.next
                ToolTip.visible: zoomNext.hovered
                ToolTip.text: zoomBody.arrows.keyHint(root.carouselTexts.zoomNextTooltip)
                onClicked: carousel.step(1)
            }

            RexonButton {
                id: zoomClose
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: root.m.zoomMargin
                width: root.m.zoomArrowSize
                implicitHeight: root.m.zoomArrowSize
                padding: 0
                pal: root.pal
                tone: root.pal.zoom_arrow_tone
                font.pointSize: root.m.zoomClosePointSize
                text: "×"
                ToolTip.visible: zoomClose.hovered
                ToolTip.text: root.carouselTexts.zoomCloseTooltip
                onClicked: zoom.hideZoom()
            }

            RexonText {
                anchors.bottom: parent.bottom
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottomMargin: root.m.zoomHintPadding
                text: zoomBody.arrows.keyHint(page.zoomHint(root.slide, carousel.count))
                color: root.pal.zoom_hint
            }
        }

        // The viewer must be keyboard-operable from the first frame, and the page
        // from the first frame AFTER exit.
        //
        // Focus inside the viewer is taken by the window BODY (`zoomBody`): it
        // listens to the paging keys, and Tab from it leads to "❮ / ❯ / ×" if the
        // user wants the buttons. The window is not always shown through
        // `open()` — Qt itself shows it too when restoring the window — so focus is
        // taken on the show EVENT, not only in the function.
        //
        // Warning: returning focus to the page is not a trifle. Before it existed,
        // after closing the viewer the keyboard went nowhere: ←/→ did not page the
        // desktop list, Tab did not move between buttons, and the tab looked
        // frozen, although it worked with the mouse.
        //
        // It is the FOCUS that is returned, not the selection:
        // `focusFirstControl()` must not be called here — it moves the selection
        // to the first desktop in the list, and leaving the viewer would change
        // the shown card. The selected row stays the same as before zooming.
        onVisibleChanged: {
            if (zoom.visible)
                zoomBody.forceActiveFocus();
            else
                deList.forceActiveFocus();
        }
    }

    // ── Desktop operation window ──────────────────────────────────────────
    //
    // Shared by "Desktops" and "DE comparison": question or refusal →
    // progress → result (`DeOperationDialog.qml`).

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
        onDesktopsChanged: page.refreshInstalled()
    }

    // ── Focus from the first frame ────────────────────────────────────────

    // Warning: focus goes to the FIRST OPERABLE ELEMENT, not to the page root:
    // `forceActiveFocus()` on the root makes `activeFocusItem` a nameless Item,
    // and the page is dead both from the keyboard and from a gamepad.
    property int focusAttempts: 0

    function focusFirstControl() {
        // The first row is a section header: it is visible but not selectable,
        // and focus goes to the first SELECTABLE item.
        // The row of the desktop already selected by the model: on opening this is
        // the current desktop, after returning to the tab — the one the user
        // stopped at. Previously focus always went to the first row, and the tab
        // opened on Cinnamon (acceptance 10.10.2026).
        const wanted = page.selectedDe;
        if (wanted !== "") {
            for (let i = 0; i < deList.count; i++) {
                if (page.roleValue(i, "selectable") === true
                        && page.roleValue(i, "deId") === wanted) {
                    root.focusAttempts = 0;
                    deList.currentIndex = i;
                    deList.forceActiveFocus();
                    return;
                }
            }
        }
        for (let i = 0; i < deList.count; i++) {
            // A section header is visible but not selectable — focus goes to the
            // first SELECTABLE item. We ask the model, not the markup: the set of
            // sections is decided by the core, and the markup must not know about it.
            if (page.roleValue(i, "selectable") === true) {
                root.focusAttempts = 0;
                deList.currentIndex = i;
                deList.forceActiveFocus();
                return;
            }
        }
        // The list does not fill instantly; we retry on the next event loop pass,
        // but not forever.
        if (root.focusAttempts < 20) {
            root.focusAttempts += 1;
            Qt.callLater(root.focusFirstControl);
        } else {
            console.warn("вкладка рабочих столов: список пуст, фокус взять не с чего");
        }
    }

    Component.onCompleted: {
        root.focusFirstControl();
        root.requestVersion();
    }

    // In the embedded tab the page is created in advance and comes alive when
    // switched to — focus must be taken there too.
    onVisibleChanged: if (root.visible) root.focusFirstControl()
}
