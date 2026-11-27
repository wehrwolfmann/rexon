// Блок «Плейлист обоев» в «⚙️ Настройках» раздела «Обои HD»: один список
// для всех собственных обоев Rexon — видео, веб, Flash, картинки «Обоев HD»
// и обои Razer Axon вперемешку; порядок, интервал, смена при входе,
// «Следующие/Предыдущие», пауза. Обои Wallpaper Engine ведёт его вкладка.
//
// Только разметка: подписи, снимок и правки — ядро (`wallpaper_playlist`)
// через `WallpaperPlaylistActions` (rust/rexon-gui/src/wallpaper_playlist_actions.rs).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    signal message(string text)

    readonly property var t: JSON.parse(actions.texts())
    property var st: ({ enabled: false, items: [], orders: [], intervals: [], line: "" })
    property bool confirmClear: false

    function reload() {
        section.st = JSON.parse(actions.state());
    }

    function edit(op) {
        const answer = JSON.parse(actions.edit(JSON.stringify(op)));
        if (answer.message)
            section.message(answer.message);
        section.reload();
    }

    WallpaperPlaylistActions {
        id: actions
        objectName: "wallpaperPlaylistActions"
        onFinished: text => {
            if (text)
                section.message(text);
            section.reload();
        }
    }

    Component.onCompleted: section.reload()

    // Таймер идёт сам — строку «сейчас / следующие через» освежаем, пока
    // блок виден.
    Timer {
        interval: 5000
        repeat: true
        running: section.visible
        onTriggered: section.reload()
    }

    FileDialog {
        id: fileDialog
        title: section.t.addFilesTitle || ""
        fileMode: FileDialog.OpenFiles
        onAccepted: section.edit({ op: "addFiles", paths: selectedFiles.map(u => u.toString()) })
    }

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
            text: section.t.title || ""
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            text: section.t.description || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonCheckBox {
            id: enabledBox
            objectName: "playlistEnabled"
            pal: section.pal
            text: section.t.enabled || ""
            checked: section.st.enabled === true
            onToggled: {
                section.edit({ op: "enabled", on: enabledBox.checked });
                enabledBox.checked = Qt.binding(function () { return section.st.enabled === true; });
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            RexonText {
                text: section.t.order || ""
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: orderBox
                objectName: "playlistOrder"
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.orders || []
                selectedValue: section.st.order
                onActivated: section.edit({ op: "order", value: orderBox.currentValue })
            }

            RexonText {
                text: section.t.interval || ""
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: intervalBox
                objectName: "playlistInterval"
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.intervals || []
                selectedValue: section.st.interval
                onActivated: section.edit({ op: "interval", value: intervalBox.currentValue })
            }

            Item { Layout.fillWidth: true }
        }

        RexonCheckBox {
            id: loginBox
            objectName: "playlistOnLogin"
            pal: section.pal
            text: section.t.onLogin || ""
            checked: section.st.onLogin === true
            onToggled: {
                section.edit({ op: "onLogin", on: loginBox.checked });
                loginBox.checked = Qt.binding(function () { return section.st.onLogin === true; });
            }
        }

        RexonText {
            objectName: "playlistLine"
            Layout.fillWidth: true
            visible: text !== ""
            text: section.st.line || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.st.dayNightNote === true
            text: section.t.dayNightNote || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            visible: section.st.items.length > 0

            RexonButton {
                objectName: "playlistPrev"
                pal: section.pal
                text: section.t.prev || ""
                onClicked: actions.command("prev")
            }

            RexonButton {
                objectName: "playlistNext"
                pal: section.pal
                highlighted: true
                text: section.t.next || ""
                onClicked: actions.command("next")
            }

            RexonButton {
                objectName: "playlistPause"
                pal: section.pal
                visible: section.st.enabled === true
                text: section.st.paused ? (section.t.resume || "") : (section.t.pause || "")
                onClicked: actions.command("pause")
            }

            Item { Layout.fillWidth: true }
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.st.items.length === 0
            text: section.t.empty || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        // Список: название, вид, ▲ ▼ ✕.
        Repeater {
            model: section.st.items

            delegate: Rectangle {
                id: row
                required property var modelData
                required property int index
                Layout.fillWidth: true
                implicitHeight: rowBox.implicitHeight + 8
                radius: 4
                color: row.modelData.current ? section.pal.bg_card : "transparent"
                border.color: section.pal.border
                border.width: 1

                RowLayout {
                    id: rowBox
                    anchors.fill: parent
                    anchors.margins: 4
                    spacing: 6

                    RexonText {
                        Layout.fillWidth: true
                        text: (row.modelData.current ? "▶ " : "") + row.modelData.title
                        color: section.pal.text_primary
                        elide: Text.ElideMiddle
                    }

                    RexonText {
                        text: row.modelData.kind
                        color: section.pal.text_secondary
                    }

                    RexonButton {
                        pal: section.pal
                        text: "▲"
                        enabled: row.index > 0
                        ToolTip.text: section.t.up || ""
                        ToolTip.visible: hovered
                        onClicked: section.edit({ op: "move", key: row.modelData.key, delta: -1 })
                    }

                    RexonButton {
                        pal: section.pal
                        text: "▼"
                        enabled: row.index < section.st.items.length - 1
                        ToolTip.text: section.t.down || ""
                        ToolTip.visible: hovered
                        onClicked: section.edit({ op: "move", key: row.modelData.key, delta: 1 })
                    }

                    RexonButton {
                        pal: section.pal
                        text: "✕"
                        ToolTip.text: section.t.remove || ""
                        ToolTip.visible: hovered
                        onClicked: section.edit({ op: "remove", key: row.modelData.key })
                    }
                }
            }
        }

        Flow {
            Layout.fillWidth: true
            spacing: 8

            RexonButton {
                objectName: "playlistAddFiles"
                pal: section.pal
                text: section.t.addFiles || ""
                onClicked: fileDialog.open()
            }

            RexonButton {
                objectName: "playlistAddHd"
                pal: section.pal
                text: section.t.addHd || ""
                onClicked: section.edit({ op: "addHd" })
            }

            RexonButton {
                objectName: "playlistImportAxon"
                pal: section.pal
                visible: section.st.axonQueue === true
                text: section.t.importAxon || ""
                ToolTip.text: section.t.importAxonTooltip || ""
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: section.edit({ op: "importAxon" })
            }

            RexonDangerButton {
                objectName: "playlistClear"
                pal: section.pal
                visible: section.st.items.length > 0
                text: section.confirmClear ? (section.t.clearConfirm || "") : (section.t.clear || "")
                onClicked: {
                    if (section.confirmClear) {
                        section.confirmClear = false;
                        section.edit({ op: "clear" });
                    } else {
                        section.confirmClear = true;
                    }
                }
            }
        }
    }
}
