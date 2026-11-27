// Раздел «Эффекты экрана» вкладки «Оформление»: строки развёртки,
// кинескоп, видеокассета, киноплёнка… — одной настройкой во всех
// окружениях, где композитор даёт наложить шейдер.
//
// Только разметка: снимок и запись — `ScreenEffectsActions`
// (rust/rexon-gui/src/screen_effects_actions.rs → rexon_core::screen_effects).
// Что здесь можно, решает ядро (таблица `support`): нет применения — нет
// кнопки «Применить», а пояснение `note` честно говорит, почему.
//
// Плитки рисует тот же GLSL, что ляжет на экран: ядро порождает шейдер
// предпросмотра (`preview_glsl`, все пресеты в одном файле), build.rs
// собирает его в `.qsb`. Под шейдер подложен нарисованный здесь образец
// рабочего стола — своя картинка, без чужих ассетов.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    /// Палитра скина и раскладка вкладки — от хозяина страницы.
    property var pal: ({})
    property var sty: ({})

    /// Итог действия для всплывающей плашки хозяина.
    signal message(string text)

    property var st: ({})

    /// Выбранная плитка (ещё не применённая): `""` — «Выключено».
    property string selected: ""
    property real strength: 0.6
    property real speed: 1.0

    /// Часы предпросмотра, уже помноженные на скорость.
    property real clock: 0

    readonly property real labelWidth: Math.max(section.sty.widths.fieldLabel, 220)

    readonly property var selectedTile: (section.st.presets || []).find(p => p.id === section.selected) || null

    /// Плитки двигаются так же, как двигался бы экран: где хост не даёт
    /// времени (KWin, Wayfire), эффект и на плитке застывает.
    readonly property bool liveMotion: section.st.moving === true
                                       || section.st.coverage === "None"
                                       || section.st.coverage === "MonochromeOnly"

    /// Есть что применять: выбор или ползунки разошлись с тем, что включено.
    readonly property bool dirty: section.selected !== (section.st.preset || "")
                                  || (section.selected !== ""
                                      && (Math.abs(section.strength - (section.st.strength || 0)) > 0.001
                                          || Math.abs(section.speed - (section.st.speed || 0)) > 0.001))

    function reload() {
        section.st = JSON.parse(actions.state());
        section.selected = section.st.preset || "";
        section.strength = section.st.strength !== undefined ? section.st.strength : 0.6;
        section.speed = section.st.speed !== undefined ? section.st.speed : 1.0;
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    // ── Защита от чёрного экрана (правило #13) ────────────────────────────
    // Шейдер может не собраться на чужом драйвере, и тогда экран темнеет.
    // Поэтому включённый эффект живёт «на пробу»: как при смене разрешения,
    // без ответа за 15 с всё возвращается к прежнему (выключено или прежний
    // эффект). Выключение — безопасное действие, его не спрашиваем.
    readonly property int confirmSeconds: 15
    property bool confirming: false
    property int secondsLeft: 0
    property var previous: null

    function applySelected() {
        const before = { preset: section.st.preset || "", strength: section.st.strength, speed: section.st.speed };
        const text = actions.apply(section.selected, section.strength, section.speed);
        section.did(text);
        if (section.selected !== "" && (section.st.preset || "") === section.selected) {
            section.previous = before;
            section.secondsLeft = section.confirmSeconds;
            section.confirming = true;
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
        if (p)
            section.did(actions.apply(p.preset, p.strength, p.speed));
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

    // Окно закрыли или ушли из раздела, не ответив, — тоже откат.
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

    // ── Образец рабочего стола под плитками ───────────────────────────────
    // Рисуется один раз и отдаётся всем плиткам текстурой.
    Item {
        id: sample
        width: 320
        height: 200

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

        // Окно.
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

        // Панель с цветными значками.
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
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Screen effects")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Make the screen look like an old tube monitor, a worn video tape or a film print. Pick an effect and press “Apply”; “Off” returns the screen to normal.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // Что умеет это окружение — фраза ядра (таблица `support`).
        Text {
            objectName: "screenEffectsNote"
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.st.can_apply === true ? section.pal.text_secondary : section.pal.state_warn
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            visible: (section.st.needs || "") !== ""
            // TRANSLATORS: %1 is what must be installed, e.g. "picom instead of the built-in compositor".
            text: qsTranslate("rexon", "Needs: %1").arg(section.st.needs || "")
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
                        x: 6
                        y: 6
                        width: parent.width - 12
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
                            // «Выключено» — образец как есть (шейдер Qt по умолчанию).
                            fragmentShader: preview.plain ? "" : "shaders/screen-effects.frag.qsb"
                        }

                        RowLayout {
                            width: parent.width
                            spacing: 4

                            Text {
                                Layout.fillWidth: true
                                text: (tile.chosen ? "◉ " : "○ ") + tile.modelData.label
                                color: section.pal.text_primary
                                elide: Text.ElideRight
                            }

                            Text {
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

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Strength:")
                color: section.pal.text_primary
            }

            Text {
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

            Text {
                text: qsTranslate("rexon", "Strong")
                color: section.pal.text_secondary
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            // Скорость есть только у движущихся эффектов и только там, где
            // хост даёт им время.
            visible: section.st.moving === true

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Motion speed:")
                color: section.pal.text_primary
            }

            Text {
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

            Text {
                text: qsTranslate("rexon", "Fast")
                color: section.pal.text_secondary
            }
        }

        Text {
            Layout.fillWidth: true
            visible: section.st.moving === true && section.selectedTile !== null
                     && section.selectedTile.animated === true && section.speed > 0
            text: qsTranslate("rexon", "Moving effects keep the graphics card busy all the time. At “Still” the picture stays the same and costs almost nothing.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // Подтверждение «на пробу» — в самой странице, без всплывающего окна
        // (удобно и с геймпада).
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
                x: 10
                y: 8
                width: parent.width - 20
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    // TRANSLATORS: %1 is the number of seconds left.
                    text: qsTranslate("rexon", "Keep this effect? Otherwise the screen goes back as it was in %1 s.").arg(section.secondsLeft)
                    color: section.pal.text_primary
                    wrapMode: Text.WordWrap
                }

                RexonButton {
                    objectName: "screenEffectsKeep"
                    pal: section.pal
                    highlighted: true
                    text: qsTranslate("rexon", "Keep", "screen effect")
                    onClicked: section.keep()
                }

                RexonButton {
                    objectName: "screenEffectsRevert"
                    pal: section.pal
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
                enabled: section.dirty
                text: qsTranslate("rexon", "Apply")
                onClicked: section.applySelected()
            }

            // Снять наш блок: конфиг возвращается к прежним байтам.
            RexonButton {
                objectName: "screenEffectsRestore"
                pal: section.pal
                visible: (section.st.preset || "") !== ""
                text: qsTranslate("rexon", "Restore as it was")
                onClicked: section.did(actions.apply("", section.strength, section.speed))
            }

            Item { Layout.fillWidth: true }
        }
    }
}
