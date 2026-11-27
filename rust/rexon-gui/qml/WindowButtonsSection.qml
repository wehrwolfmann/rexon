// The "Title bar buttons" section of the "Appearance" tab: the button side
// (right, as in Windows, or left, as in macOS), a drag-and-drop layout as
// in the "Titlebar Buttons" window of the KDE module (a title bar mock-up
// with a left and a right group + a shelf with all the KDE module's
// buttons; what the environment lacks is unavailable on the shelf with a
// reason), the KDE title bar check boxes and the border size — in all
// environments.
//
// With the mouse, buttons are dragged between the sides of the title bar
// and the shelf; with the keyboard and a gamepad — select a button
// (Enter/click) and press "Move left", "Move right", "Put on shelf" or
// "Add left/right"; for a selected title bar button the arrows and Delete
// do the same. The order and the native window manager strings come from
// the core (`window_buttons::move_button`, `nudge`).
//
// Markup only: snapshot and writing — `WindowButtonsActions`
// (rust/rexon-gui/src/window_buttons_actions.rs → rexon_core::window_buttons).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section
    RexonBidi { id: bidi }

    property var pal: ({})
    property var sty: ({})

    signal message(string text)

    property var st: ({})

    /// Width of the labels on the left: one per section, so the lists line up.
    readonly property real labelWidth: Math.max(section.sty.widths.fieldLabel, 300)

    function reload() {
        section.st = JSON.parse(actions.state());
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    // ── Drag-and-drop editor ────────────────────────────────────────────
    /// Selected button: side (`left`/`right`/`shelf`), position, id.
    property string selSide: ""
    property int selIndex: -1
    property string selId: ""

    /// What is being dragged right now (id, from where, position); empty means
    /// nothing.
    property string dragId: ""
    property string dragFrom: ""
    property int dragIndex: -1
    readonly property bool draggingTitle: section.dragFrom === "left" || section.dragFrom === "right"

    function shelfInfo(id) {
        const shelf = section.st.shelf || [];
        for (let i = 0; i < shelf.length; i++)
            if (shelf[i].id === id)
                return shelf[i];
        return { id: id, label: id, glyph: "", available: true, placed: false, reason: "" };
    }

    function select(side, index, id) {
        section.selSide = side;
        section.selIndex = index;
        section.selId = id;
    }

    function clearSelection() {
        section.select("", -1, "");
    }

    /// Move by mouse: the core answers with a message; the selection is cleared.
    function move(id, from, fromIndex, to, toIndex) {
        section.clearSelection();
        section.did(actions.moveButton(id, from, fromIndex, to, toIndex));
    }

    /// Step by button/key: the selection follows the button.
    function nudge(dir) {
        if (section.selSide !== "left" && section.selSide !== "right")
            return;
        const res = JSON.parse(actions.nudgeButton(section.selSide, section.selIndex, dir));
        const id = section.selId;
        section.did(res.message);
        if (res.at)
            section.select(res.at[0], res.at[1], id);
        else
            section.clearSelection();
    }

    function addSelected(side) {
        if (section.selSide !== "shelf")
            return;
        const id = section.selId;
        section.did(actions.addButton(id, side));
        const list = section.st[side] || [];
        // A new button is selected in its place (the gap is the one nearest to
        // the title).
        const at = id === "spacer" ? (side === "left" ? list.lastIndexOf(id) : list.indexOf(id))
                                   : list.indexOf(id);
        if (at >= 0)
            section.select(side, at, id);
    }

    /// Where in the side a button dropped at point `x` (in `item` coordinates)
    /// will go: as many buttons of the side as have their centres to the left
    /// of the point — not counting the button being moved (the position for
    /// the core is "already without it").
    function dropIndex(repeater, side, item, x) {
        let at = 0;
        for (let i = 0; i < repeater.count; i++) {
            if (section.dragFrom === side && i === section.dragIndex)
                continue;
            const slot = repeater.itemAt(i);
            if (!slot)
                continue;
            const c = slot.mapToItem(item, slot.width / 2, 0).x;
            if (c < x)
                at++;
        }
        return at;
    }

    /// Title bar sides with check boxes: both for environments with a side
    /// setting, one for COSMIC (only the right one, and without a side label).
    readonly property var sides: section.st.has_side === true
        ? [{ id: "left", label: qsTranslate("rexon", "Left side:") },
           { id: "right", label: qsTranslate("rexon", "Right side:") }]
        : [{ id: "right", label: qsTranslate("rexon", "Buttons:") }]

    // Drag layer: a "ghost" of the button follows the pointer over the mock-up
    // and the shelf (the buttons themselves stay in place in their rows).
    Item {
        id: dragLayer
        anchors.fill: parent
        z: 10

        Rectangle {
            id: ghost
            property real startX: 0
            property real startY: 0
            readonly property var info: section.shelfInfo(section.dragId)
            visible: section.dragId !== ""
            width: section.dragId === "spacer" ? 24 : 40
            height: 40
            radius: 6
            color: section.pal.bg_card
            border.color: section.pal.accent
            border.width: 2
            Drag.active: section.dragId !== ""
            Drag.keys: [section.dragFrom === "shelf" ? "shelf" : "title"]
            Drag.hotSpot.x: width / 2
            Drag.hotSpot.y: height / 2

            RexonText {
                anchors.centerIn: parent
                text: ghost.info.glyph
                color: section.pal.text_primary
                font.pointSize: 15
            }
        }
    }

    function beginDrag(item, id, from, index) {
        const p = item.mapToItem(dragLayer, 0, 0);
        ghost.startX = p.x;
        ghost.startY = p.y;
        ghost.x = p.x;
        ghost.y = p.y;
        section.clearSelection();
        section.dragFrom = from;
        section.dragIndex = index;
        section.dragId = id;
    }

    function dragTo(dx, dy) {
        ghost.x = ghost.startX + dx;
        ghost.y = ghost.startY + dy;
    }

    /// Released: drop it where the "ghost" is (the move happens afterwards, in
    /// `Qt.callLater`, so the rows are not recreated in the middle of the
    /// handler).
    function endDrag() {
        if (ghost.Drag.target)
            ghost.Drag.drop();
        else
            ghost.Drag.cancel();
        section.dragId = "";
        section.dragFrom = "";
        section.dragIndex = -1;
    }

    /// Side buttons for a mock-up row: id and side.
    function slots(side) {
        return (section.st[side] || []).map(id => ({ id: id, side: side }));
    }

    /// A button on the shelf: icon and label, as in the KDE module's grid.
    Component {
        id: shelfCell
        Item {
            id: cell
            required property var modelData
            required property int index
            readonly property bool selected: section.selSide === "shelf" && section.selIndex === cell.index
            objectName: "shelfButton_" + cell.modelData.id
            width: 120
            height: 40 + 4 + cellLabel.implicitHeight
            activeFocusOnTab: true

            Rectangle {
                id: cellTile
                anchors.horizontalCenter: parent.horizontalCenter
                width: cell.modelData.id === "spacer" ? 24 : 40
                height: 40
                radius: 6
                opacity: cell.modelData.available === true ? 1 : 0.35
                color: cell.modelData.id === "spacer" ? "transparent" : section.pal.bg_card
                border.color: cell.selected || cell.activeFocus ? section.pal.accent : section.pal.border
                border.width: cell.selected || cell.activeFocus ? 2 : 1

                RexonText {
                    anchors.centerIn: parent
                    text: cell.modelData.glyph
                    color: section.pal.text_primary
                    font.pointSize: 15
                }

                // Already in the title bar: dragging it there moves it.
                Rectangle {
                    visible: cell.modelData.placed === true
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: -4
                    width: 14
                    height: 14
                    radius: 7
                    color: section.pal.accent
                }
            }

            RexonText {
                id: cellLabel
                anchors.top: cellTile.bottom
                anchors.topMargin: 4
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                maximumLineCount: 3
                elide: Text.ElideRight
                text: cell.modelData.label
                color: cell.modelData.available === true ? section.pal.text_primary : section.pal.text_disabled
            }

            function pick() {
                cell.forceActiveFocus();
                section.select("shelf", cell.index, cell.modelData.id);
            }

            Keys.onReturnPressed: cell.pick()
            Keys.onSpacePressed: cell.pick()

            TapHandler {
                onTapped: cell.pick()
            }
            DragHandler {
                id: cellDrag
                target: null
                enabled: cell.modelData.available === true
                grabPermissions: PointerHandler.CanTakeOverFromAnything
                onActiveChanged: cellDrag.active ? section.beginDrag(cellTile, cell.modelData.id, "shelf", cell.index)
                                                 : section.endDrag()
                onActiveTranslationChanged: section.dragTo(cellDrag.activeTranslation.x, cellDrag.activeTranslation.y)
            }
        }
    }

    /// Keyboard focus follows the selected button (rows are recreated after
    /// every step).
    function refocus() {
        Qt.callLater(() => {
            const rep = section.selSide === "left" ? leftRep : (section.selSide === "right" ? rightRep : null);
            const item = rep ? rep.itemAt(section.selIndex) : null;
            if (item)
                item.forceActiveFocus();
        });
    }

    WindowButtonsActions {
        id: actions
        objectName: "windowButtonsActions"
    }

    Component.onCompleted: section.reload()

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        height: implicitHeight
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Window title bar buttons")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.has_side === true
            spacing: 8

            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Buttons position:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: sideCombo
                objectName: "windowButtonsSide"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: [
                    { id: "right", label: qsTranslate("rexon", "On the right (like Windows)") },
                    { id: "left", label: qsTranslate("rexon", "On the left (like macOS)") }
                ]
                selectedValue: section.st.side || "right"
                onActivated: section.did(actions.setLayout(sideCombo.currentValue,
                                                           (section.st.selected || []).join(",")))
            }
        }

        // ── Title bar mock-up: left group, title, right group ───────────
        Rectangle {
            id: titleBar
            objectName: "windowButtonsTitleBar"
            Layout.fillWidth: true
            visible: section.st.drag === true
            implicitHeight: titleRow.implicitHeight + 24
            radius: 8
            color: section.pal.bg_secondary
            border.color: titleDrop.containsDrag ? section.pal.accent : section.pal.border
            border.width: titleDrop.containsDrag ? 2 : 1

            // Physical zone: this is the button layout in the system's own window
            // title bars, not a line of our interface. "Right, as in Windows" means
            // the right edge of the window in any language, so for right-to-left
            // languages the rows here are NOT mirrored: the left group is on the
            // left, and the ← key and "Move left" move a button towards the left
            // edge. The "Window title" label is centred and needs no side.
            LayoutMirroring.enabled: false
            LayoutMirroring.childrenInherit: true

            /// A button in the title bar mock-up.
            Component {
                id: titleSlot
                Item {
                    id: slot
                    required property var modelData
                    required property int index
                    readonly property string side: slot.modelData.side
                    readonly property string bid: slot.modelData.id
                    readonly property var info: section.shelfInfo(slot.bid)
                    readonly property bool selected: section.selSide === slot.side && section.selIndex === slot.index
                    objectName: "titleButton_" + slot.side + "_" + slot.index
                    width: slot.bid === "spacer" ? 24 : 40
                    height: 40
                    activeFocusOnTab: true
                    opacity: section.dragFrom === slot.side && section.dragIndex === slot.index ? 0.25 : 1

                    Rectangle {
                        anchors.fill: parent
                        radius: 6
                        color: slot.bid === "spacer" ? "transparent" : section.pal.bg_card
                        border.color: slot.selected || slot.activeFocus ? section.pal.accent : section.pal.border
                        border.width: slot.selected || slot.activeFocus ? 2 : 1

                        Text {
                            anchors.centerIn: parent
                            text: slot.info.glyph
                            color: section.pal.text_primary
                            font.pointSize: 15
                        }
                    }

                    function pick() {
                        slot.forceActiveFocus();
                        section.select(slot.side, slot.index, slot.bid);
                    }

                    Keys.onReturnPressed: slot.pick()
                    Keys.onSpacePressed: slot.pick()
                    Keys.onLeftPressed: { slot.pick(); section.nudge("left"); section.refocus(); }
                    Keys.onRightPressed: { slot.pick(); section.nudge("right"); section.refocus(); }
                    Keys.onDeletePressed: { slot.pick(); section.nudge("shelf"); }

                    TapHandler {
                        onTapped: slot.pick()
                    }
                    DragHandler {
                        id: slotDrag
                        target: null
                        grabPermissions: PointerHandler.CanTakeOverFromAnything
                        onActiveChanged: slotDrag.active ? section.beginDrag(slot, slot.bid, slot.side, slot.index) : section.endDrag()
                        onActiveTranslationChanged: section.dragTo(slotDrag.activeTranslation.x, slotDrag.activeTranslation.y)
                    }
                }
            }

            RowLayout {
                id: titleRow
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.margins: 12
                spacing: 12

                Row {
                    id: leftRow
                    spacing: 4
                    Repeater {
                        id: leftRep
                        model: section.slots("left")
                        delegate: titleSlot
                    }
                }

                Text {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    elide: Text.ElideRight
                    text: qsTranslate("rexon", "Title bar")
                    color: section.pal.text_primary
                    font.bold: true
                }

                Row {
                    id: rightRow
                    spacing: 4
                    Repeater {
                        id: rightRep
                        model: section.slots("right")
                        delegate: titleSlot
                    }
                }
            }

            // A dropped button goes into the group whose edge it is closer to (as in
            // the KDE module), into its place among that group's buttons.
            DropArea {
                id: titleDrop
                anchors.fill: parent
                keys: ["shelf", "title"]
                onDropped: drop => {
                    const leftEdge = leftRow.mapToItem(titleDrop, leftRow.width, 0).x;
                    const rightEdge = rightRow.mapToItem(titleDrop, 0, 0).x;
                    const side = Math.abs(drop.x - leftEdge) <= Math.abs(drop.x - rightEdge) ? "left" : "right";
                    const at = section.dropIndex(side === "left" ? leftRep : rightRep, side, titleDrop, drop.x);
                    drop.accept();
                    Qt.callLater(section.move, section.dragId, section.dragFrom, section.dragIndex, side, at);
                }
            }
        }

        // ── Shelf: all buttons of the KDE module ───────────────────────
        Rectangle {
            id: shelfBox
            objectName: "windowButtonsShelf"
            Layout.fillWidth: true
            visible: section.st.drag === true
            implicitHeight: shelfFlow.implicitHeight + 24
            radius: 8
            color: "transparent"
            border.color: shelfDrop.containsDrag ? section.pal.accent : section.pal.border
            border.width: shelfDrop.containsDrag ? 2 : 1

            Flow {
                id: shelfFlow
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.margins: 12
                y: 12
                spacing: 8
                opacity: section.draggingTitle ? 0.15 : 1

                Repeater {
                    id: shelfRep
                    model: section.st.shelf || []
                    delegate: shelfCell
                }
            }

            RexonText {
                anchors.centerIn: parent
                width: parent.width - 24
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                visible: section.draggingTitle
                text: qsTranslate("rexon", "Drop button here to remove it")
                color: section.pal.text_primary
                font.bold: true
                font.pointSize: 13
            }

            DropArea {
                id: shelfDrop
                anchors.fill: parent
                keys: ["title"]
                onDropped: drop => {
                    drop.accept();
                    Qt.callLater(section.move, section.dragId, section.dragFrom, section.dragIndex, "shelf", 0);
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.st.drag === true
            opacity: section.dragId !== "" ? 0 : 1
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            text: qsTranslate("rexon", "Drag buttons between here and the title bar. Without a mouse: select a button and use the buttons below.")
            color: section.pal.text_secondary
        }

        // ── Without a mouse: the selected button and steps ───────────────
        Flow {
            id: selFlow
            Layout.fillWidth: true
            visible: section.st.drag === true && section.selId !== ""
            spacing: 8

            readonly property var info: section.shelfInfo(section.selId)
            readonly property bool inTitle: section.selSide === "left" || section.selSide === "right"

            RexonText {
                id: selText
                height: 40
                verticalAlignment: Text.AlignVCenter
                text: selFlow.info.label
                color: section.pal.text_primary
                font.bold: true
            }
            // The "Move left/Move right" and "Add left/right" pairs are about the
            // title bar sides (a physical zone, like the mock-up above): for
            // right-to-left languages the row of actions is mirrored, but the "Move
            // left" button inside a pair stays on the left — on the side it moves
            // towards.
            Row {
                id: moveRow
                visible: selFlow.inTitle
                spacing: selFlow.spacing
                LayoutMirroring.enabled: false
                LayoutMirroring.childrenInherit: true

                RexonButton {
                    objectName: "windowButtonsMoveLeft"
                    enabled: !(section.selSide === "left" && section.selIndex === 0)
                    pal: section.pal
                    text: qsTranslate("rexon", "Move left")
                    onClicked: section.nudge("left")
                }
                RexonButton {
                    objectName: "windowButtonsMoveRight"
                    enabled: !(section.selSide === "right" && section.selIndex === (section.st.right || []).length - 1)
                    pal: section.pal
                    text: qsTranslate("rexon", "Move right")
                    onClicked: section.nudge("right")
                }
            }
            RexonButton {
                objectName: "windowButtonsToShelf"
                visible: selFlow.inTitle
                pal: section.pal
                text: qsTranslate("rexon", "Remove from the title bar")
                onClicked: section.nudge("shelf")
            }
            Row {
                id: addRow
                visible: !selFlow.inTitle && selFlow.info.available === true
                spacing: selFlow.spacing
                LayoutMirroring.enabled: false
                LayoutMirroring.childrenInherit: true

                RexonButton {
                    objectName: "windowButtonsAddLeft"
                    pal: section.pal
                    text: qsTranslate("rexon", "Add on the left")
                    onClicked: section.addSelected("left")
                }
                RexonButton {
                    objectName: "windowButtonsAddRight"
                    pal: section.pal
                    text: qsTranslate("rexon", "Add on the right")
                    onClicked: section.addSelected("right")
                }
            }
            RexonText {
                height: 40
                verticalAlignment: Text.AlignVCenter
                visible: !selFlow.inTitle && selFlow.info.available !== true
                text: selFlow.info.reason || ""
                color: section.pal.text_secondary
            }
        }

        // Check boxes per side — where there is no drag and drop (COSMIC: only
        // "minimize" and "maximize" on/off). A button sits on one side: the check
        // box on the other side moves it there.
        Repeater {
            model: section.st.drag === true ? [] : section.sides

            delegate: RowLayout {
                id: sideRow
                required property var modelData
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.preferredWidth: section.labelWidth
                    Layout.alignment: Qt.AlignTop
                    wrapMode: Text.WordWrap
                    text: sideRow.modelData.label
                    color: section.pal.text_primary
                }

                Flow {
                    Layout.fillWidth: true
                    spacing: 12

                    Repeater {
                        model: section.st.buttons || []

                        delegate: RexonCheckBox {
                            id: btn
                            required property var modelData
                            objectName: "windowButton_" + sideRow.modelData.id + "_" + btn.modelData.id
                            pal: section.pal
                            text: btn.modelData.label
                            checked: (section.st[sideRow.modelData.id] || []).indexOf(btn.modelData.id) >= 0
                            onToggled: section.did(actions.toggleButton(sideRow.modelData.id,
                                                                        btn.modelData.id, btn.checked))
                        }
                    }
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.unavailable || "") !== ""
            text: section.st.unavailable || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.custom || "") !== ""
            text: bidi.arg(qsTranslate("rexon", "Your own layout is set now (%1); changing the buttons here replaces it."), section.st.custom || "")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // KWin title bar check boxes (KDE only): double click on the menu, button
        // tooltips, "hide from screenshots" always.
        Repeater {
            model: section.st.flags || []

            delegate: ColumnLayout {
                id: flagRow
                required property var modelData
                Layout.fillWidth: true
                spacing: 2

                RexonCheckBox {
                    objectName: "windowTitleFlag_" + flagRow.modelData.id
                    Layout.fillWidth: true
                    pal: section.pal
                    text: flagRow.modelData.label
                    checked: flagRow.modelData.checked === true
                    enabled: flagRow.modelData.enabled === true
                    onToggled: section.did(actions.setFlag(flagRow.modelData.id, checked))
                }

                RexonText {
                    Layout.fillWidth: true
                    visible: (flagRow.modelData.hint || "") !== ""
                    text: flagRow.modelData.hint || ""
                    color: section.pal.text_secondary
                    wrapMode: Text.WordWrap
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.flags_note || "") !== ""
            text: section.st.flags_note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.has_border === true
            spacing: 8

            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Window border size:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: borderCombo
                objectName: "windowBorderSize"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.border_options || []
                selectedValue: section.st.border || ""
                onActivated: section.did(actions.setBorder(borderCombo.currentValue))
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.border_note || "") !== ""
            text: section.st.border_note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonButton {
            objectName: "windowButtonsDefaults"
            pal: section.pal
            text: qsTranslate("rexon", "Defaults")
            onClicked: section.did(actions.defaults())
        }
    }
}
