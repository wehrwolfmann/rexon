// Menu of the Rexon main window: the "☰" button and the items under it.
//
// In the product this is a `QMenuBar` with one submenu (`window.py`,
// `_build_menubar`). Here it is the same thing with the stock `MenuBar`
// from QtQuick Controls: there are no widgets in this process at all, and
// `menuBar:` of `ApplicationWindow` is exactly where the menu belongs (not
// `header:`, which holds the section strip).
//
// THERE ARE NO DECISIONS OF OUR OWN HERE. Not a single label, order,
// separator or shortcut is typed in this file: everything arrives as the
// `items` list from `WindowMenuModel` (rust/rexon-gui/src/window_menu.rs),
// which asks the core (`rexon_core::window_menu`). The file only draws and
// reports the identifier of the pressed item upwards — by identifier, not
// by label: the label is translated, the decision is not.
//
// Warning: ITEMS ARE CREATED ONE BY ONE, NOT WITH A `Repeater`. The reason
// is not taste: `Menu` stacks its children in the order they arrive, and a
// `Repeater` creates its elements AFTER the static siblings — a separator
// written as a separate element would move to the top of the menu. Here
// the order is built exactly as the core said, including the position of
// the separator.
//
// Warning: KEYBOARD FOCUS. The menu must be navigable: `MenuBar` takes
// focus with Tab and opens with Space, `Menu` moves between items with the
// arrows and closes with Esc — all of this comes from Controls itself, but
// only if the items are real `MenuItem`s inside a real `Menu`. There is
// deliberately no fake menu of our own here: a fake would lose the focus
// chain, and the project rule is to keep it from the very start rather
// than finish it right before controller support.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

MenuBar {
    id: root

    /// The Rust menu object. Set by the window host — the same one that holds
    /// the palette: create the object here and the window run could not reach
    /// it.
    required property var menuModel

    /// Skin palette. The menu defines no colours of its own (rule #15).
    required property var pal

    /// Colours of the dangerous item ("Uninstall Rexon") — the tone of the
    /// danger button from the window palette (`destructive_tone`), the same as
    /// "Remove everything" in the uninstall window. Without a tone the
    /// dangerous item is highlighted like an ordinary one.
    readonly property var dangerTone: root.pal.destructive_tone !== undefined
                                      ? root.pal.destructive_tone : null

    /// The item with this identifier was pressed.
    signal itemActivated(string id)

    /// How many items were ACTUALLY built. A full list proves nothing yet: it
    /// may be complete while the delegates fail to build — and the menu turns
    /// out empty with a healthy model. The run measures by this number.
    property int builtItems: 0

    /// How many separators were built.
    property int builtSeparators: 0

    /// How many shortcuts were built. A shortcut lives as a separate object,
    /// and if it were not created, "Ctrl+Q" would vanish silently: the menu
    /// would look intact while the shortcut did not work at all.
    property int builtShortcuts: 0

    /// Open the menu for the run and close it again.
    ///
    /// The label width of a closed menu cannot be measured at all: items get
    /// it only when the menu is laid out, and before the first opening their
    /// width is zero. So the run OPENS the menu. Nobody sees anything: the run
    /// window lives without a screen.
    function openForCheck() { burger.open(); }
    function closeAfterCheck() { burger.close(); }

    /// Built items (without separators) — the run measures the width by them.
    /// A list of our own, not `burger.itemAt()`: the menu also contains a
    /// separator, which has no label at all, and a reference to the item here
    /// also keeps it alive.
    property var entryItems: []

    /// Reserve for the icon column and the label padding that the style would
    /// allot to the item itself if it had its own `contentItem`. Without the
    /// reserve the menu comes out exactly as wide as the letters — that is, on
    /// the verge of eliding.
    readonly property real labelReserve: 12

    /// How much room the widest item needs — the menu width is computed from
    /// it (see `implicitWidth` of `Menu` below). Recomputed when an item
    /// changes its request: the label is translated, the system font changes
    /// on the fly, and a width computed once at build time would be lying by
    /// evening.
    property real wantedWidth: 0

    function remeasure() {
        let w = 0;
        for (const it of root.entryItems)
            if (it)
                w = Math.max(w, it.wantedWidth);
        root.wantedWidth = w;
    }

    /// Whether the menu is laid out: every item has a width. Until then,
    /// measuring labels is pointless — "it fits" would come out for everyone,
    /// always.
    function laidOut() {
        if (root.entryItems.length === 0)
            return false;
        for (const it of root.entryItems)
            if (!it || it.width <= 0)
                return false;
        return true;
    }

    /// Labels of items that did NOT fit into their width and are shown elided.
    ///
    /// Warning: counting items does not catch this: there may be exactly as
    /// many as the core named, each in its place, and the user reads "🛟 Restore
    /// the…" without knowing what door is behind the item. So the WIDTH is
    /// measured, and Qt itself measures it (`Text.truncated`).
    function clippedLabels() {
        const out = [];
        for (const it of root.entryItems)
            if (it && it.labelClipped)
                out.push(it.text + " (" + Math.round(it.labelHas) + " из "
                         + Math.ceil(it.labelNeeds) + ")");
        return out;
    }

    /// Width of the menu and of the longest label — for the run log: these
    /// numbers show whether the menu is TIGHT even before a label gets elided.
    function widthReport() {
        let widest = 0;
        for (const it of root.entryItems)
            if (it)
                widest = Math.max(widest, it.labelNeeds);
        return "меню " + Math.round(burger.width)
             + " | пункту доступно " + Math.round(burger.availableWidth)
             + " | самой длинной подписи нужно " + Math.ceil(widest);
    }

    /// Menu parts painted NOT with the skin — for the window run.
    ///
    /// Warning: counting items and their width does not catch this: on
    /// 30.09.2026 the menu was complete, the labels whole, and the background
    /// white from the Basic style over the dark window. We measure the colour
    /// itself: the menu background and the label of every item.
    function offSkinParts() {
        const out = [];
        if (!Qt.colorEqual(menuSurface.color, root.pal.bg_card))
            out.push("подложка меню " + menuSurface.color
                     + " вместо " + root.pal.bg_card);
        for (const it of root.entryItems) {
            if (!it)
                continue;
            const want = it.enabled ? root.pal.text_primary : root.pal.text_disabled;
            if (!it.highlighted && !Qt.colorEqual(it.inkColor, want))
                out.push("подпись «" + it.text + "» " + it.inkColor
                         + " вместо " + want);
        }
        if (root.dangerTone === null
                && root.entryItems.some(it => it && it.destructive))
            out.push("у опасного пункта нет краски опасной кнопки "
                     + "(в палитре окна нет destructive_tone)");
        return out;
    }

    Menu {
        id: burger
        title: root.menuModel.buttonText

        // Warning: WE COMPUTE THE MENU WIDTH OURSELVES, AND HERE IS WHY.
        //
        // An item with ITS OWN `contentItem` (it is our own here — for the
        // "Ctrl+Q" on the right) loses what the style allots to the item by
        // itself: the icon column and the padding around the label. The menu then
        // comes out exactly as wide as the letters, without a single pixel of
        // reserve, and the label lives on the edge: any rounding of the pop-up
        // window width eats the last character, and instead of "🛟 Restore KDE
        // theme" the user reads "🛟 Restore the…" — without knowing what door is
        // behind the item (measurement on 18.09.2026: the label needs 164 pixels,
        // the menu gave 164).
        //
        // Warning: and this is NOT a quirk of one style. The `Menu` of Fusion and
        // Basic does not follow its items in width at all: the list inside does
        // not report its width upwards, the menu stays 200 pixels for any label,
        // and on a machine without the KDE style (GNOME, XFCE, bare Qt) everything
        // longer would be cut. So we compute not a "correction to the style" but
        // the width itself: by the widest item, as the product's `QMenu` does.
        // The lower bound from the style remains — if its background is
        // narrower, the menu should not shrink.
        implicitWidth: Math.max(burger.implicitBackgroundWidth
                                + burger.leftInset + burger.rightInset,
                                root.wantedWidth
                                + burger.leftPadding + burger.rightPadding)

        // Menu background — the skin card, like the windows of this same menu
        // (`MenuDialog` in WindowMenuDialogs.qml). The Basic style background is
        // white and does not follow the skin palette.
        background: Rectangle {
            id: menuSurface
            implicitWidth: 200
            implicitHeight: 40
            color: root.pal.bg_card
            border.color: root.pal.border
            border.width: 1
            radius: 4
        }

        // `Menu` does not put Components and non-Items among its items (they go
        // into `data`), so declaring them right here is safe.
        Component {
            id: itemComponent

            MenuItem {
                id: entry

                property string entryId: ""
                /// Behind the item is something irreversible (the core decides,
                /// `is_destructive`).
                property bool destructive: false
                /// What to paint a highlighted item with: a dangerous one with the danger
                /// button colour, the others with the skin's list row highlight.
                readonly property bool dangerLit: entry.destructive
                                                  && entry.highlighted
                                                  && root.dangerTone !== null
                /// Colour of the label and the shortcut. A disabled item is dimmed with
                /// the same colour as a disabled button (`text_disabled`).
                readonly property color inkColor: !entry.enabled
                                                  ? root.pal.text_disabled
                                                  : (entry.dangerLit
                                                     ? root.dangerTone.color
                                                     : root.pal.text_primary)
                /// The shortcut of THIS item in Qt notation — only to print it. The key is
                /// listened to by a separate `Shortcut` (see below): items of a closed
                /// menu do not process keys.
                property string keys: ""

                /// How much room the item needs for its label to read IN FULL: the whole
                /// label, the shortcut, the gap between them, the item padding from the
                /// style and the reserve for the icon column (`labelReserve` of `MenuBar`
                /// above). Measured from the texts themselves, not from
                /// `implicitContentWidth`: the layout reports its width on the next pass,
                /// and the menu would have opened with the old number.
                readonly property real wantedWidth:
                    entryLabel.implicitWidth
                    + (entry.keys !== ""
                       ? entryRow.spacing + entryKeys.implicitWidth : 0)
                    + entry.leftPadding + entry.rightPadding
                    + root.labelReserve
                onWantedWidthChanged: root.remeasure()

                /// Whether the label is shown elided ("🛟 Restore the…"). The answer comes
                /// from Qt itself, not from our width recalculation: `truncated` says
                /// exactly what the user sees on screen.
                readonly property bool labelClipped: entryLabel.truncated
                /// How much the label needs and how much it got — so that the run log
                /// says not just "elided" but by how much.
                readonly property real labelNeeds: entryLabel.implicitWidth
                readonly property real labelHas: entryLabel.width

                onTriggered: root.itemActivated(entry.entryId)

                // Warning: THE SHORTCUT IS PRINTED IN THE ITEM. The product's `QMenu`
                // appends "Ctrl+Q" on the right, muted, by itself, while a Controls
                // `MenuItem` prints nothing: the shortcut would work, but the user would
                // never learn about it (checked with a screenshot on 09.09.2026 — the
                // product shows "Ctrl+Q", the Rust window did not).
                //
                // Colours come from the skin palette, like the item background below: the
                // `palette` roles of the Basic style are light and do not follow the skin
                // (a white menu over the dark window, screenshot 30.09.2026).
                contentItem: RowLayout {
                    id: entryRow

                    spacing: 24

                    RexonText {
                        id: entryLabel

                        text: entry.text
                        font: entry.font
                        color: entry.inkColor
                        elide: Text.ElideRight
                        verticalAlignment: Text.AlignVCenter
                        Layout.fillWidth: true
                    }

                    RexonText {
                        id: entryKeys

                        visible: entry.keys !== ""
                        text: entry.keys
                        font: entry.font
                        color: entry.inkColor
                        opacity: 0.6
                        verticalAlignment: Text.AlignVCenter
                    }
                }

                // Row highlight — the same as for the items of the skin's combo boxes
                // (`bg_card_hover`); for the dangerous item — the danger button colour.
                // The frame is the keyboard and gamepad focus: the menu is navigated with
                // the arrows, and invisible focus is as good as none.
                background: Rectangle {
                    implicitWidth: 200
                    implicitHeight: 40
                    radius: 3
                    color: entry.dangerLit ? root.dangerTone.background
                           : (entry.highlighted ? root.pal.bg_card_hover
                                                : "transparent")
                    border.width: entry.visualFocus ? 2 : 0
                    border.color: root.pal.accent
                }
            }
        }

        Component {
            id: separatorComponent

            MenuSeparator {
                contentItem: Rectangle {
                    implicitWidth: 188
                    implicitHeight: 1
                    color: root.pal.border
                }
            }
        }

        Component {
            id: shortcutComponent

            Shortcut {
                property string entryId: ""
                onActivated: root.itemActivated(entryId)
            }
        }

        // Warning: THE PARENT OF A CREATED ITEM IS THE MENU'S `contentItem`, NOT
        // THE MENU ITSELF. `Menu` is not a scene item (`Popup` inherits QObject),
        // and an item created with such a parent is born OUTSIDE the graphics
        // scene: for every such call Qt writes "Created graphical object was not
        // placed in the graphics scene" to the log — one line per item and per
        // separator, five lines on every launch. There is no way and no reason to
        // silence the output: the menu has a real host item into which `addItem`
        // moves the item anyway — its `contentItem`. We create the item directly
        // in it, and there is no warning because there is no cause for one.
        //
        // The shortcut object (`Shortcut`) is not a scene item at all, and its
        // parent remains the menu itself: it needs an owner for its lifetime, not
        // a place on screen.
        Component.onCompleted: {
            const entries = root.menuModel.items;
            for (let i = 0; i < entries.length; ++i) {
                const e = entries[i];
                if (e.separatorBefore) {
                    burger.addItem(separatorComponent.createObject(
                                       burger.contentItem));
                    ++root.builtSeparators;
                }
                const item = itemComponent.createObject(burger.contentItem, {
                    entryId: e.id,
                    text: e.label,
                    keys: e.shortcut,
                    enabled: e.enabled,
                    destructive: e.destructive === true
                });
                if (item === null) {
                    // Staying silent is not allowed: a missing item is a missing door to
                    // uninstalling the program or to resetting a broken theme.
                    console.warn("пункт меню «" + e.id + "» не построился");
                    continue;
                }
                burger.addItem(item);
                root.entryItems.push(item);
                ++root.builtItems;

                // The shortcut lives as a separate `Shortcut`, not as an item property:
                // `MenuItem` has none at all, and Qt does not listen to an `Action` inside
                // a closed menu — so "Ctrl+Q" would work only while the menu is open,
                // which the product does not do.
                if (e.shortcut !== "") {
                    const key = shortcutComponent.createObject(burger, {
                        entryId: e.id,
                        sequence: e.shortcut
                    });
                    if (key === null) {
                        console.warn("горячая клавиша пункта «" + e.id
                                     + "» не построилась");
                    } else {
                        ++root.builtShortcuts;
                        // The item shows the shortcut label in the system language ("Strg+Q" in
                        // German), as `QMenu` prints it: `nativeText` translates key names with
                        // the qtbase catalogue. The core stores the Qt notation ("Ctrl+Q") — it is
                        // for `sequence`, not for the eyes.
                        if (key.nativeText !== "")
                            item.keys = key.nativeText;
                    }
                }
            }
            // The items are in place — now the menu width is computed from them too.
            // An item reports its width before it gets into the list, and without
            // this call the menu would stay as wide as its background.
            root.remeasure();
        }
    }

    delegate: MenuBarItem {
        id: barItem

        contentItem: RexonText {
            text: barItem.text
            font: barItem.font
            // Colours come from the window palette. In the product there is QSS in
            // this place, and not for looks: the desktop's window layer gives
            // `QMenuBar` ITS OWN palette, and on a light system theme the "☰" sign
            // came out dark on the dark skin background — that is, INVISIBLE, while
            // behind it live "Restore KDE theme", "About", "Uninstall Rexon" and
            // "Quit". In QML this problem does not exist at all: Controls never have
            // a platform palette, the colour is taken here and only here.
            color: root.pal.text_primary
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }

        background: Rectangle {
            implicitWidth: 34
            implicitHeight: 30
            radius: 4
            color: barItem.highlighted || barItem.activeFocus
                   ? root.pal.surface_card : "transparent"
            // Frame of the focused button: the menu is navigated from the keyboard,
            // and invisible focus is as good as none.
            border.width: barItem.activeFocus ? 2 : 0
            border.color: root.pal.accent
        }
    }

    // Warning: THE "☰" BUTTON IS ON THE RIGHT, NOT ON THE LEFT. In the product
    // this is done by `bar.setLayoutDirection(RightToLeft)`, and the button's
    // place is not a matter of taste: to its left, right next to it, the
    // section strip begins, and a button placed there reads as the first
    // section. Checked with a screenshot on 09.09.2026 — in the Rust window
    // "☰" ended up in the top left corner, in the product it is in the top
    // right.
    //
    // The yardstick is the product itself, so the line is right here:
    // `MenuBar` lays out its items with its `contentItem`, and an ordinary
    // `LayoutMirroring` on the button would not help — it would have to be
    // propagated to the children, and the submenu itself would be mirrored
    // along with them. The submenu stays left to right
    // (`menu.setLayoutDirection(LeftToRight)` in the product): it is a
    // separate window and does not inherit the bar's layout.
    //
    // Right-to-left language: the window is mirrored from the root
    // (MainWindow.qml), and this line flips together with the whole window —
    // "☰" moves to the left, i.e. to the end of the bar for such a reader, and
    // the submenu reads right to left, as it should. There is still no
    // `LayoutMirroring` of our own here: enabling it locally would flip the
    // submenu for left-to-right languages too.
    contentItem: Row {
        spacing: root.spacing
        layoutDirection: Qt.RightToLeft

        Repeater {
            model: root.contentModel
        }
    }

    background: Rectangle {
        color: root.pal.bg_primary
    }
}
