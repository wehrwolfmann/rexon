// Conky widget settings in the card of the "Conky widgets" section:
// position and size, background, window, screen, update rate, clock and
// network.
//
// The author's file is not changed: every change writes a Rexon overlay
// (rexon_core::conky_overlay), and the running widget re-reads it by
// itself — the edit is visible at once, without "Apply". "As the author
// made it" deletes the overlay.
//
// Markup only: values, labels and writing — the page's actions object
// (`conkySettings`, `setConkySetting`, `resetConkySettings` in
// rust/rexon-gui/src/appearance_actions.rs).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: box

    property var pal: null
    property var actions: null
    property string itemId: ""
    /// The settings file (part of the widget) these settings belong to.
    property string conf: ""
    property bool busy: false

    property var view: ({})
    readonly property var t: box.view.t || ({})
    readonly property var cur: box.view.cur || ({})

    readonly property color colText: box.pal ? box.pal.text_primary : "#ffffff"
    readonly property color colHint: box.pal ? box.pal.text_secondary : "#8f98a0"

    spacing: 6

    function reload() {
        if (!box.actions || !box.actions.conkySettings || box.itemId === "" || box.conf === "") {
            box.view = ({});
            return;
        }
        try {
            box.view = JSON.parse(box.actions.conkySettings(box.itemId, box.conf));
        } catch (e) {
            box.view = ({});
        }
    }

    function set(key, value) {
        if (!box.actions || !box.actions.setConkySetting)
            return;
        try {
            box.view = JSON.parse(box.actions.setConkySetting(box.itemId, box.conf, key,
                                                              JSON.stringify(value)));
        } catch (e) {
            box.reload();
        }
    }

    function field(name) {
        return box.cur[name] || ({ value: 0, changed: false, locked: false });
    }

    onConfChanged: reload()
    onItemIdChanged: reload()
    Component.onCompleted: reload()

    // The opacity slider writes not on every step but after a pause.
    Timer {
        id: opacityTimer
        interval: 350
        onTriggered: box.set("bg_opacity", Math.round(opacitySlider.value))
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: 4
        RexonText {
            Layout.fillWidth: true
            text: box.t.title || ""
            color: box.colHint
            font.bold: true
        }
        RexonButton {
            objectName: "conkySettingsReset"
            pal: box.pal
            visible: box.view.available === true
            enabled: box.view.changed === true && !box.busy
            text: box.t.reset || ""
            ToolTip.text: box.t.reset_tooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onClicked: {
                if (box.actions && box.actions.resetConkySettings)
                    box.view = JSON.parse(box.actions.resetConkySettings(box.itemId, box.conf));
            }
        }
    }

    RexonText {
        Layout.fillWidth: true
        visible: box.view.available === false
        text: box.view.reason || ""
        color: box.colHint
        wrapMode: Text.WordWrap
    }

    ColumnLayout {
        Layout.fillWidth: true
        visible: box.view.available === true
        enabled: !box.busy
        spacing: 6

        // ── Position and size ─────────────────────────────────────────
        RexonText { text: box.t.position || ""; color: box.colText; font.bold: true }

        RowLayout {
            spacing: 16
            Grid {
                id: alignmentGrid
                objectName: "conkyAlignmentGrid"
                columns: 3
                spacing: 2
                // Physical zone: a 3×3 grid — the screen corners and edges where the
                // widget will be placed (the buttons have arrows ↖ ↑ ↗). The top left
                // corner of the screen is on the left in any language, so the grid is
                // not mirrored.
                LayoutMirroring.enabled: false
                LayoutMirroring.childrenInherit: true
                Repeater {
                    model: box.view.alignments || []
                    delegate: RexonButton {
                        required property var modelData
                        objectName: "conkyAlign_" + modelData.value
                        pal: box.pal
                        implicitWidth: 34
                        implicitHeight: 30
                        checkable: true
                        checked: box.field("alignment").value === modelData.value
                        enabled: !box.field("alignment").locked
                        text: modelData.arrow
                        ToolTip.text: modelData.label
                        ToolTip.visible: hovered
                        ToolTip.delay: 400
                        onClicked: box.set("alignment", modelData.value)
                    }
                }
            }

            GridLayout {
                columns: 2
                columnSpacing: 8
                rowSpacing: 4
                Repeater {
                    model: [
                        { key: "gap_x", label: box.t.gap_x || "" },
                        { key: "gap_y", label: box.t.gap_y || "" },
                        { key: "min_width", label: box.t.min_width || "" },
                        { key: "min_height", label: box.t.min_height || "" }
                    ]
                    delegate: RowLayout {
                        id: numRow
                        required property var modelData
                        spacing: 6
                        RexonText {
                            text: numRow.modelData.label
                            color: box.colText
                            Layout.preferredWidth: 150
                            elide: Text.ElideRight
                        }
                        RexonTextField {
                            objectName: "conkyField_" + numRow.modelData.key
                            pal: box.pal
                            Layout.preferredWidth: 70
                            enabled: !box.field(numRow.modelData.key).locked
                            text: String(box.field(numRow.modelData.key).value)
                            validator: IntValidator { bottom: -10000; top: 10000 }
                            ToolTip.text: box.field(numRow.modelData.key).locked ? (box.t.locked || "") : ""
                            ToolTip.visible: hovered && ToolTip.text !== ""
                            onEditingFinished: {
                                const v = parseInt(text);
                                if (!isNaN(v) && v !== box.field(numRow.modelData.key).value)
                                    box.set(numRow.modelData.key, v);
                            }
                        }
                    }
                }
            }
        }

        // Mini map of the screen: the same widget by dragging (conky_layout).
        ConkyScreenMap {
            objectName: "conkyCardScreenMap"
            Layout.fillWidth: true
            pal: box.pal
            actions: box.actions
            itemId: box.itemId
            conf: box.conf
            busy: box.busy
            onMoved: box.reload()
        }

        // ── Background ────────────────────────────────────────────────
        RexonText { text: box.t.background || ""; color: box.colText; font.bold: true; Layout.topMargin: 4 }

        RowLayout {
            spacing: 12
            RexonCheckBox {
                objectName: "conkyBgNone"
                pal: box.pal
                text: box.t.bg_none || ""
                enabled: !box.field("bg_none").locked
                checked: box.field("bg_none").value === true
                onToggled: box.set("bg_none", checked)
            }
            RexonText { text: box.t.bg_opacity || ""; color: box.colText }
            RexonSlider {
                id: opacitySlider
                objectName: "conkyBgOpacity"
                pal: box.pal
                Layout.preferredWidth: 160
                from: 0
                to: 100
                stepSize: 5
                enabled: box.field("bg_none").value !== true && !box.field("bg_opacity").locked
                value: box.field("bg_opacity").value
                onMoved: opacityTimer.restart()
            }
            RexonText { text: Math.round(opacitySlider.value) + " %"; color: box.colHint }
        }

        RowLayout {
            spacing: 8
            RexonText { text: box.t.bg_colour || ""; color: box.colText }
            Rectangle {
                Layout.preferredWidth: 22
                Layout.preferredHeight: 22
                radius: 3
                color: "#" + (box.field("bg_colour").value || "000000")
                border.color: box.colHint
            }
            RexonTextField {
                objectName: "conkyBgColour"
                pal: box.pal
                Layout.preferredWidth: 90
                enabled: box.field("bg_none").value !== true && !box.field("bg_colour").locked
                text: box.field("bg_colour").value || ""
                validator: RegularExpressionValidator { regularExpression: /#?[0-9a-fA-F]{6}/ }
                onEditingFinished: {
                    const v = text.replace("#", "").toUpperCase();
                    if (v.length === 6 && v !== box.field("bg_colour").value)
                        box.set("bg_colour", v);
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (box.view.compositor_note || "") !== ""
            text: box.view.compositor_note || ""
            color: box.colHint
            wrapMode: Text.WordWrap
        }

        // ── Window and screen ─────────────────────────────────────────
        RexonText { text: box.t.window || ""; color: box.colText; font.bold: true; Layout.topMargin: 4 }

        RexonCheckBox {
            objectName: "conkyWindowPreset"
            pal: box.pal
            text: box.t.window_preset || ""
            checked: box.cur.window_preset === true
            ToolTip.text: box.t.window_preset_tooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onToggled: box.set("window_preset", checked ? true : null)
        }

        RexonText {
            Layout.fillWidth: true
            visible: (box.view.preset_note || "") !== ""
            text: box.view.preset_note || ""
            color: box.colHint
            wrapMode: Text.WordWrap
        }

        RowLayout {
            visible: (box.view.monitors || []).length > 1
            spacing: 8
            RexonText { text: box.t.screen || ""; color: box.colText }
            RexonComboBox {
                objectName: "conkyScreen"
                pal: box.pal
                textRole: "label"
                valueRole: "value"
                model: box.view.screen_choices || []
                selectedValue: box.field("xinerama_head").value
                onActivated: box.set("xinerama_head", currentValue < 0 ? null : currentValue)
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (box.view.monitor_note || "") !== ""
            text: box.view.monitor_note || ""
            color: box.colHint
            wrapMode: Text.WordWrap
        }

        RowLayout {
            spacing: 8
            RexonText { text: box.t.update_interval || ""; color: box.colText }
            RexonTextField {
                objectName: "conkyUpdateInterval"
                pal: box.pal
                Layout.preferredWidth: 70
                enabled: !box.field("update_interval").locked
                text: String(box.field("update_interval").value)
                validator: DoubleValidator { bottom: 0.1; top: 3600; decimals: 1; notation: DoubleValidator.StandardNotation }
                onEditingFinished: {
                    const v = parseFloat(text.replace(",", "."));
                    if (!isNaN(v) && v > 0 && v !== box.field("update_interval").value)
                        box.set("update_interval", v);
                }
            }
        }

        // ── Clock and network (in the widget text) ────────────────────
        RexonCheckBox {
            objectName: "conkyClock24"
            visible: box.view.clock !== null && box.view.clock !== undefined
            pal: box.pal
            text: box.t.clock_24h || ""
            checked: box.view.clock_24h === true
            onToggled: box.set("clock_24h", checked)
        }

        Repeater {
            model: box.view.nets || []
            delegate: RowLayout {
                id: netRow
                required property var modelData
                spacing: 8
                RexonText { text: netRow.modelData.label; color: box.colText }
                RexonComboBox {
                    objectName: "conkyNet_" + netRow.modelData.from
                    pal: box.pal
                    textRole: "label"
                    valueRole: "value"
                    model: netRow.modelData.choices
                    selectedValue: netRow.modelData.to
                    onActivated: box.set("net:" + netRow.modelData.from, currentValue)
                }
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: (box.view.nets || []).length > 0
            text: box.t.net_note || ""
            color: box.colHint
            wrapMode: Text.WordWrap
            font.pointSize: 9
        }
    }
}
