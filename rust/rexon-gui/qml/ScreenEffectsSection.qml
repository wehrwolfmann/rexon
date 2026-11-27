// The "Screen effects" section of the "Appearance" tab: scan lines, CRT,
// video cassette, film… — one setting in all environments where the
// compositor allows overlaying a shader.
//
// Markup only: snapshot and writing — `ScreenEffectsActions`
// (rust/rexon-gui/src/screen_effects_actions.rs → rexon_core::screen_effects).
// What is possible here is decided by the core (the `support` table): no
// way to apply — no "Apply" button, and the `note` explanation honestly
// says why.
//
// The tiles are drawn by the same GLSL that will go onto the screen: the
// core generates a preview shader (`preview_glsl`, all presets in one
// file), and build.rs compiles it into a `.qsb`. Under the shader lies a
// desktop sample drawn here — our own picture, without foreign assets.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section
    RexonBidi { id: bidi }

    /// Skin palette and tab layout — from the page host.
    property var pal: ({})
    /// The environment selected in "Current environment" of the "Appearance"
    /// tab (empty means the one running now). The explanation, "Apply" and
    /// writing follow it: on 08.11.2026, with "Tiling and Wayfire" selected in
    /// a KDE session, the section talked about KWin.
    property string de: ""
    onDeChanged: section.reload()
    property var sty: ({})

    /// Action result for the host's pop-up banner.
    signal message(string text)

    property var st: ({})

    /// Selected tile (not yet applied): `""` means "Off".
    property string selected: ""
    property real strength: 0.6
    property real speed: 1.0

    /// Preview clock, already multiplied by the speed.
    property real clock: 0

    readonly property real labelWidth: Math.max(section.sty.widths.fieldLabel, 220)

    readonly property var selectedTile: (section.st.presets || []).find(p => p.id === section.selected) || null

    /// The tiles move just as the screen would: where the host provides no
    /// time (KWin, Wayfire), the effect freezes on the tile as well.
    readonly property bool liveMotion: section.st.moving === true
                                       || section.st.coverage === "None"
                                       || section.st.coverage === "MonochromeOnly"

    /// There is something to apply: the selection or the sliders differ from
    /// what is enabled.
    readonly property bool dirty: section.selected !== (section.st.preset || "")
                                  || (section.selected !== ""
                                      && (Math.abs(section.strength - (section.st.strength || 0)) > 0.001
                                          || Math.abs(section.speed - (section.st.speed || 0)) > 0.001))

    function take(json) {
        section.st = JSON.parse(json);
        section.selected = section.st.preset || "";
        section.strength = section.st.strength !== undefined ? section.st.strength : 0.6;
        section.speed = section.st.speed !== undefined ? section.st.speed : 1.0;
    }

    function reload() {
        section.take(actions.state(section.de));
    }

    /// Writing in progress (the `ScreenEffectsActions` worker thread): buttons
    /// are disabled. Writing is NOT synchronous — on 15.11.2026 the window
    /// froze on rapid presses.
    readonly property bool busy: actions.busy

    // ── Protection against a black screen (rule #13) ──────────────────────
    // A shader may fail to compile on someone else's driver, and then the
    // screen goes dark. So an enabled effect lives "on trial": as with a
    // resolution change, without an answer within 15 s everything returns to
    // the previous state (off or the previous effect). Turning off is a safe
    // action; we do not ask about it.
    readonly property int confirmSeconds: 15
    property bool confirming: false
    property int secondsLeft: 0
    property var previous: null

    /// What was there before the last "Apply" — for rolling back the trial.
    property var before: null

    function applySelected() {
        section.before = { preset: section.st.preset || "", strength: section.st.strength, speed: section.st.speed };
        section.asked = section.selected;
        actions.request("apply", section.de, section.selected, section.strength, section.speed);
    }

    /// The tile the last action came from. While it was running, the user may
    /// have chosen another one — the action's result does not override that
    /// choice.
    property string asked: ""

    function finished(tag, text, changed, shown, state) {
        if (text)
            section.message(text);
        const picked = section.selected !== section.asked ? section.selected : null;
        section.take(state);
        if (picked !== null)
            section.selected = picked;
        // The trial is only for our own shader (the core's `trial`; the stock
        // greyscale filter of KDE/Pantheon/COSMIC will not leave a black screen),
        // and only if the effect is actually enabled: on GNOME/Cinnamon this is
        // confirmed by the extension itself (`shown`).
        if (tag === "apply" && changed && shown && section.st.trial === true
                && (section.st.preset || "") !== "") {
            section.previous = section.before;
            section.secondsLeft = section.confirmSeconds;
            section.confirming = true;
        }
    }

    Connections {
        target: actions
        function onFinished(tag, text, changed, shown, state) {
            section.finished(tag, text, changed, shown, state);
        }
    }

    function keep() {
        section.confirming = false;
        section.previous = null;
    }

    function revert() {
        const p = section.previous;
        section.confirming = false;
        section.previous = null;
        if (p) {
            section.asked = p.preset;
            section.selected = p.preset;
            actions.request("revert", section.de, p.preset, p.strength, p.speed);
        }
    }

    Timer {
        interval: 1000
        repeat: true
        running: section.confirming
        onTriggered: {
            section.secondsLeft -= 1;
            if (section.secondsLeft <= 0)
                section.revert();
        }
    }

    // The window was closed or the section left without answering — also a
    // rollback.
    Component.onDestruction: if (section.confirming) section.revert()

    ScreenEffectsActions {
        id: actions
        objectName: "screenEffectsActions"
    }

    Component.onCompleted: section.reload()

    FrameAnimation {
        running: section.visible && section.liveMotion && section.speed > 0
        onTriggered: section.clock += frameTime * section.speed
    }

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    // ── Desktop sample under the tiles ────────────────────────────────────
    // Drawn once and handed to all tiles as a texture.
    Item {
        id: sample
        width: 320
        height: 200

        // Physical zone: this is a picture of the screen that the shader distorts,
        // not an interface line. The window, the sun and the hills are placed with
        // manual `x`, while mirroring would flip rows and anchors — for
        // right-to-left languages the result would be a mess (the clock on top of
        // the panel icons). The picture is the same in any language.
        LayoutMirroring.enabled: false
        LayoutMirroring.childrenInherit: true

        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.0; color: "#2b5d9c" }
                GradientStop { position: 0.55; color: "#e0905a" }
                GradientStop { position: 1.0; color: "#5a3a6e" }
            }
        }

        Rectangle {
            x: 230; y: 34; width: 46; height: 46; radius: 23
            color: "#ffd75e"
        }

        Rectangle {
            x: -40; y: 120; width: 260; height: 140; radius: 130
            color: "#3f7d4c"
        }

        Rectangle {
            x: 150; y: 135; width: 240; height: 130; radius: 120
            color: "#2f6a3e"
        }

        // Window.
        Rectangle {
            x: 28; y: 26; width: 170; height: 112
            color: "#f2f2f2"
            border.color: "#1b2838"

            Rectangle {
                width: parent.width; height: 18
                color: "#1b2838"

                Row {
                    anchors.right: parent.right
                    anchors.rightMargin: 5
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 4
                    Repeater {
                        model: ["#4cd964", "#ffcc00", "#ff3b30"]
                        delegate: Rectangle {
                            required property string modelData
                            width: 8; height: 8; radius: 4
                            color: modelData
                        }
                    }
                }
            }

            Column {
                x: 10; y: 26
                spacing: 6
                Repeater {
                    model: [120, 140, 96, 130, 70]
                    delegate: Rectangle {
                        required property int modelData
                        width: modelData; height: 6; radius: 2
                        color: "#5c6b7a"
                    }
                }
            }

            Rectangle {
                x: 108; y: 78; width: 52; height: 22; radius: 4
                color: "#2f80ed"
            }
        }

        // Panel with colourful icons.
        Rectangle {
            y: parent.height - 22
            width: parent.width; height: 22
            color: "#171a21"

            Row {
                x: 8
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                Repeater {
                    model: ["#e74c3c", "#2ecc71", "#3498db", "#f1c40f", "#9b59b6"]
                    delegate: Rectangle {
                        required property string modelData
                        width: 14; height: 14; radius: 3
                        color: modelData
                    }
                }
            }

            Text {
                anchors.right: parent.right
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                text: "12:00"
                color: "#ffffff"
                font.pixelSize: 11
            }
        }
    }

    ShaderEffectSource {
        id: sampleTexture
        sourceItem: sample
        hideSource: true
        width: 0
        height: 0
    }

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
            text: qsTranslate("rexon", "Screen effects")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Make the screen look like an old tube monitor, a worn video tape or a film print. Pick an effect and press “Apply”; “Off” returns the screen to normal.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // What this environment can do — a core phrase (the `support` table).
        RexonText {
            objectName: "screenEffectsNote"
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.st.can_apply === true ? section.pal.text_secondary : section.pal.state_warn
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: (section.st.needs || "") !== ""
            // TRANSLATORS: %1 is what must be installed, e.g. "picom instead of the built-in compositor".
            text: bidi.arg(qsTranslate("rexon", "Needs: %1"), section.st.needs || "")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Flow {
            Layout.fillWidth: true
            spacing: 10

            Repeater {
                model: [{ id: "", label: qsTranslate("rexon", "Off", "screen effect"), index: -1, animated: false }]
                       .concat(section.st.presets || [])

                delegate: Rectangle {
                    id: tile
                    required property var modelData
                    readonly property bool chosen: tile.modelData.id === section.selected
                    readonly property bool current: tile.modelData.id === (section.st.preset || "")

                    objectName: "screenEffectTile_" + (tile.modelData.id || "off")
                    width: 196
                    height: tileColumn.implicitHeight + 12
                    radius: 6
                    color: tileMouse.containsMouse ? section.pal.bg_card_hover : section.pal.bg_card
                    border.width: tile.chosen || tile.activeFocus ? 2 : 1
                    border.color: tile.chosen || tile.activeFocus ? section.pal.accent : section.pal.border
                    activeFocusOnTab: true

                    Keys.onReturnPressed: section.selected = tile.modelData.id
                    Keys.onSpacePressed: section.selected = tile.modelData.id

                    MouseArea {
                        id: tileMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            section.selected = tile.modelData.id;
                            tile.forceActiveFocus();
                        }
                    }

                    Column {
                        id: tileColumn
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: 6
                        y: 6
                        spacing: 4

                        ShaderEffect {
                            id: preview
                            width: parent.width
                            height: Math.round(width * sample.height / sample.width)
                            readonly property bool plain: tile.modelData.index < 0
                            property variant source: sampleTexture
                            property real rx_preset: Math.max(0, tile.modelData.index)
                            property real rx_s: section.strength
                            property real rx_window: section.st.per_window === true ? 1 : 0
                            property real rx_time: tile.modelData.animated === true ? section.clock : 0
                            property size rx_res: Qt.size(preview.width, preview.height)
                            // "Off" — the sample as is (the default Qt shader).
                            fragmentShader: preview.plain ? "" : "shaders/screen-effects.frag.qsb"
                        }

                        RowLayout {
                            width: parent.width
                            spacing: 4

                            RexonText {
                                Layout.fillWidth: true
                                text: (tile.chosen ? "◉ " : "○ ") + tile.modelData.label
                                color: section.pal.text_primary
                                elide: Text.ElideRight
                            }

                            RexonText {
                                visible: tile.current
                                text: qsTranslate("rexon", "(current)")
                                color: section.pal.accent
                            }
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            // The stock COSMIC filter has no strength — the slider would change nothing.
            visible: section.st.has_strength !== false

            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Strength:")
                color: section.pal.text_primary
            }

            RexonText {
                text: qsTranslate("rexon", "Subtle")
                color: section.pal.text_secondary
            }

            RexonSlider {
                id: strengthSlider
                objectName: "screenEffectsStrength"
                Layout.fillWidth: true
                pal: section.pal
                enabled: section.selected !== ""
                from: 0
                to: 1
                stepSize: 0.05
                value: section.strength
                onMoved: section.strength = strengthSlider.value
            }

            RexonText {
                text: qsTranslate("rexon", "Strong")
                color: section.pal.text_secondary
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            // Only moving effects have a speed, and only where the host gives them time.
            visible: section.st.moving === true

            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Motion speed:")
                color: section.pal.text_primary
            }

            RexonText {
                text: qsTranslate("rexon", "Still")
                color: section.pal.text_secondary
            }

            RexonSlider {
                id: speedSlider
                objectName: "screenEffectsSpeed"
                Layout.fillWidth: true
                pal: section.pal
                enabled: section.selectedTile !== null && section.selectedTile.animated === true
                from: 0
                to: section.st.max_speed || 3
                stepSize: 0.25
                value: section.speed
                onMoved: section.speed = speedSlider.value
            }

            RexonText {
                text: qsTranslate("rexon", "Fast")
                color: section.pal.text_secondary
            }
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.st.moving === true && section.selectedTile !== null
                     && section.selectedTile.animated === true && section.speed > 0
            text: qsTranslate("rexon", "Moving effects keep the graphics card busy all the time. At “Still” the picture stays the same and costs almost nothing.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // The "on trial" confirmation is in the page itself, without a pop-up
        // window (convenient with a gamepad too).
        Rectangle {
            objectName: "screenEffectsConfirm"
            Layout.fillWidth: true
            visible: section.confirming
            implicitHeight: confirmRow.implicitHeight + 16
            radius: 6
            color: section.pal.bg_card
            border.color: section.pal.state_warn
            border.width: 2

            RowLayout {
                id: confirmRow
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.margins: 10
                y: 8
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    // TRANSLATORS: %1 is the number of seconds left.
                    text: bidi.arg(qsTranslate("rexon", "Keep this effect? Otherwise the screen goes back as it was in %1 s."), section.secondsLeft)
                    color: section.pal.text_primary
                    wrapMode: Text.WordWrap
                }

                RexonButton {
                    objectName: "screenEffectsKeep"
                    pal: section.pal
                    enabled: !section.busy
                    highlighted: true
                    text: qsTranslate("rexon", "Keep", "screen effect")
                    onClicked: section.keep()
                }

                RexonButton {
                    objectName: "screenEffectsRevert"
                    pal: section.pal
                    enabled: !section.busy
                    text: qsTranslate("rexon", "Revert", "screen effect")
                    onClicked: section.revert()
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: section.st.can_apply === true && !section.confirming

            RexonButton {
                objectName: "screenEffectsApply"
                pal: section.pal
                highlighted: true
                enabled: section.dirty && !section.busy
                text: section.busy ? qsTranslate("rexon", "Applying…", "screen effect") : qsTranslate("rexon", "Apply")
                onClicked: section.applySelected()
            }

            // Remove our block: the config returns to its previous bytes.
            RexonButton {
                objectName: "screenEffectsRestore"
                pal: section.pal
                visible: (section.st.preset || "") !== ""
                enabled: !section.busy
                text: qsTranslate("rexon", "Restore as it was")
                // The environment goes as the second argument, right after the action
                // kind, as with "Apply": previously an empty string went in place of the
                // environment and the strength in place of the preset.
                onClicked: {
                    section.asked = "";
                    section.selected = "";
                    actions.request("restore", section.de, "", section.strength, section.speed);
                }
            }

            Item { Layout.fillWidth: true }
        }
    }
}
