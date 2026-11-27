// Mini map of the screen for Conky widgets: monitors to scale, and on them
// the windows of the running widgets (and of the widget of the open card)
// at their own size in their own place. Drag a rectangle with the mouse or
// move it with the arrow keys — Rexon picks the nearest screen corner or
// edge, recomputes the offsets and writes them into the widget's overlay,
// like the numbers in the card settings. The author's file is not changed;
// "Restore" puts the widgets moved here back in their previous places.
//
// Markup only: geometry, snapping and labels — the core
// (rexon_core::conky_layout), actions — `conkyScreenMap`,
// `moveConkyWidget`, `restoreConkyPosition` in
// rust/rexon-gui/src/appearance_actions.rs.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: map

    property var pal: null
    property var actions: null
    /// Tile and file of the open card; empty means only the running ones.
    property string itemId: ""
    property string conf: ""
    property bool busy: false

    /// The position changed (the card should re-read its numbers).
    signal moved()

    property var view: ({})
    readonly property var t: map.view.t || ({})
    readonly property var widgets: map.view.widgets || []
    readonly property var frame: map.view.bounds || ({ x: 0, y: 0, w: 1, h: 1 })
    /// What to put back on "Restore": file → {item_id, saved}.
    property var undo: ({})
    readonly property int undoCount: Object.keys(map.undo).length
    /// File to give focus back to after the map is re-read.
    property string keepFocus: ""

    readonly property color colText: map.pal ? map.pal.text_primary : "#ffffff"
    readonly property color colHint: map.pal ? map.pal.text_secondary : "#8f98a0"
    readonly property color colAccent: map.pal ? map.pal.accent : "#66c0f4"
    readonly property color colCard: map.pal ? map.pal.bg_card : "#1b2838"
    readonly property color colBorder: map.pal ? map.pal.border : "#3d4450"
    readonly property color colWarn: map.pal ? map.pal.state_warn : "#e0a040"

    spacing: 6

    function screensJson() {
        const out = [];
        const all = Qt.application.screens;
        for (let i = 0; i < all.length; ++i) {
            const s = all[i];
            out.push({ name: s.name, x: s.virtualX, y: s.virtualY, width: s.width, height: s.height });
        }
        return JSON.stringify(out);
    }

    function reload() {
        if (!map.actions || !map.actions.conkyScreenMap) {
            map.view = ({});
            return;
        }
        try {
            map.view = JSON.parse(map.actions.conkyScreenMap(map.itemId, map.conf, map.screensJson()));
        } catch (e) {
            map.view = ({});
        }
    }

    /// Window of widget `w` was dropped at `x, y` (screen pixels).
    function commit(w, x, y, snap) {
        if (!map.actions || !map.actions.moveConkyWidget || map.busy)
            return;
        if (map.undo[w.conf] === undefined) {
            const u = Object.assign({}, map.undo);
            u[w.conf] = { item_id: w.item_id, saved: w.saved };
            map.undo = u;
        }
        map.keepFocus = w.conf;
        map.actions.moveConkyWidget(w.item_id, w.conf, Math.round(x), Math.round(y),
                                    w.rect.w, w.rect.h, snap, map.screensJson());
        map.reload();
        map.moved();
    }

    function revert() {
        if (!map.actions || !map.actions.restoreConkyPosition)
            return;
        for (const c in map.undo)
            map.actions.restoreConkyPosition(map.undo[c].item_id, c, JSON.stringify(map.undo[c].saved));
        map.undo = ({});
        map.reload();
        map.moved();
    }

    onItemIdChanged: { map.undo = ({}); map.reload(); }
    onConfChanged: { map.undo = ({}); map.reload(); }
    onVisibleChanged: if (visible) map.reload()
    Component.onCompleted: map.reload()

    // A widget was started, stopped or had its settings changed — rebuild the map.
    Connections {
        target: map.actions
        ignoreUnknownSignals: true
        function onActionDone(text, ok) {
            if (map.visible)
                map.reload();
        }
    }

    RowLayout {
        Layout.fillWidth: true
        RexonText {
            Layout.fillWidth: true
            text: map.t.title || ""
            color: map.colText
            font.bold: true
        }
        RexonButton {
            objectName: "conkyMapRevert"
            pal: map.pal
            visible: map.view.available === true
            enabled: map.undoCount > 0 && !map.busy
            text: map.t.revert || ""
            ToolTip.text: map.t.revert_tooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onClicked: map.revert()
        }
    }

    RexonText {
        Layout.fillWidth: true
        visible: map.view.available === false && (map.view.reason || "") !== ""
        text: map.view.reason || ""
        color: map.colHint
        wrapMode: Text.WordWrap
    }

    RexonText {
        Layout.fillWidth: true
        visible: map.view.available === true && map.widgets.length === 0
        text: map.t.empty || ""
        color: map.colHint
        wrapMode: Text.WordWrap
    }

    Item {
        id: canvas
        objectName: "conkyMapCanvas"
        // Physical zone: this is a diagram of the real screen — the left monitor
        // is on the left in any language, the rectangles are placed with manual
        // `x`, and the ← → arrows move the widget towards the left and right edges
        // of the screen.
        LayoutMirroring.enabled: false
        LayoutMirroring.childrenInherit: true

        visible: map.view.available === true && map.widgets.length > 0
        Layout.fillWidth: true
        Layout.maximumWidth: 560
        Layout.preferredHeight: Math.min(320, Math.max(120, width * map.frame.h / Math.max(1, map.frame.w)))
        /// Map pixels per screen pixel.
        readonly property real k: Math.min(width / Math.max(1, map.frame.w), height / Math.max(1, map.frame.h))
        readonly property real offX: (width - map.frame.w * k) / 2
        readonly property real offY: (height - map.frame.h * k) / 2

        Repeater {
            model: map.view.areas || []
            delegate: Rectangle {
                id: area
                required property var modelData
                x: canvas.offX + (area.modelData.rect.x - map.frame.x) * canvas.k
                y: canvas.offY + (area.modelData.rect.y - map.frame.y) * canvas.k
                width: area.modelData.rect.w * canvas.k
                height: area.modelData.rect.h * canvas.k
                color: map.colCard
                border.color: map.colBorder
                border.width: 1
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 2
                    horizontalAlignment: Text.AlignHCenter
                    text: area.modelData.name
                    color: map.colHint
                    font.pointSize: 8
                }
            }
        }

        Repeater {
            model: map.widgets
            delegate: Rectangle {
                id: gadget
                required property var modelData
                objectName: "conkyMapWidget"
                /// Offset by hand or by arrows, in screen pixels, before writing.
                property real dx: 0
                property real dy: 0
                readonly property bool guess: gadget.modelData.size_from === "guess"

                x: canvas.offX + (gadget.modelData.rect.x + gadget.dx - map.frame.x) * canvas.k
                y: canvas.offY + (gadget.modelData.rect.y + gadget.dy - map.frame.y) * canvas.k
                width: Math.max(6, gadget.modelData.rect.w * canvas.k)
                height: Math.max(6, gadget.modelData.rect.h * canvas.k)
                radius: 2
                color: Qt.rgba(map.colAccent.r, map.colAccent.g, map.colAccent.b,
                               gadget.modelData.focus ? 0.55 : 0.32)
                border.width: gadget.activeFocus ? 3 : 1
                border.color: gadget.activeFocus ? map.colText
                              : (gadget.modelData.unknown ? map.colWarn : map.colAccent)
                opacity: gadget.modelData.running ? 1 : 0.7
                activeFocusOnTab: true
                enabled: !map.busy
                z: drag.active || gadget.activeFocus ? 2 : 1

                Accessible.role: Accessible.Button
                Accessible.name: gadget.modelData.tooltip

                ToolTip.text: gadget.modelData.tooltip
                ToolTip.visible: hover.hovered && !drag.active
                ToolTip.delay: 500

                Component.onCompleted: if (gadget.modelData.conf === map.keepFocus) gadget.forceActiveFocus()

                Text {
                    anchors.fill: parent
                    anchors.margins: 2
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    text: gadget.modelData.title + (gadget.guess ? " ~" : "")
                    color: map.colText
                    font.pointSize: 8
                    elide: Text.ElideRight
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                }

                HoverHandler { id: hover; cursorShape: Qt.OpenHandCursor }

                DragHandler {
                    id: drag
                    target: null
                    cursorShape: Qt.ClosedHandCursor
                    onTranslationChanged: {
                        if (active) {
                            gadget.dx = translation.x / canvas.k;
                            gadget.dy = translation.y / canvas.k;
                        }
                    }
                    onActiveChanged: {
                        if (active) {
                            gadget.forceActiveFocus();
                            return;
                        }
                        const x = gadget.modelData.rect.x + gadget.dx;
                        const y = gadget.modelData.rect.y + gadget.dy;
                        if (Math.abs(gadget.dx) < 1 && Math.abs(gadget.dy) < 1) {
                            gadget.dx = 0;
                            gadget.dy = 0;
                            return;
                        }
                        map.commit(gadget.modelData, x, y, true);
                    }
                }

                // Arrows accumulate the offset and write it after a pause: conky re-reads
                // the overlay once rather than on every key press.
                Timer {
                    id: keyCommit
                    interval: 600
                    onTriggered: map.commit(gadget.modelData, gadget.modelData.rect.x + gadget.dx,
                                            gadget.modelData.rect.y + gadget.dy, false)
                }

                Keys.onPressed: (event) => {
                    const step = (event.modifiers & Qt.ShiftModifier) ? 1 : 10;
                    let moved = true;
                    if (event.key === Qt.Key_Left)
                        gadget.dx -= step;
                    else if (event.key === Qt.Key_Right)
                        gadget.dx += step;
                    else if (event.key === Qt.Key_Up)
                        gadget.dy -= step;
                    else if (event.key === Qt.Key_Down)
                        gadget.dy += step;
                    else
                        moved = false;
                    if (moved) {
                        event.accepted = true;
                        keyCommit.restart();
                    }
                }
            }
        }
    }

    RexonText {
        Layout.fillWidth: true
        visible: canvas.visible
        text: map.t.hint || ""
        color: map.colHint
        wrapMode: Text.WordWrap
        font.pointSize: 9
    }

    RexonText {
        Layout.fillWidth: true
        visible: canvas.visible && map.widgets.some(w => w.size_from === "guess")
        text: map.t.guess || ""
        color: map.colHint
        wrapMode: Text.WordWrap
        font.pointSize: 9
    }

    RexonText {
        Layout.fillWidth: true
        visible: canvas.visible && map.widgets.some(w => w.unknown)
        text: map.t.unknown || ""
        color: map.colWarn
        wrapMode: Text.WordWrap
        font.pointSize: 9
    }

    RexonText {
        Layout.fillWidth: true
        visible: canvas.visible && (map.view.note || "") !== ""
        text: map.view.note || ""
        color: map.colHint
        wrapMode: Text.WordWrap
        font.pointSize: 9
    }
}
