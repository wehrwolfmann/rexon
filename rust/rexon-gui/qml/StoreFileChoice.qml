// Выбор файла у товара магазина, когда файлов несколько.
//
// Какой файл берётся автоматически, решает ядро (`ocs::file_choices`, то же
// правило, что у кнопки плитки); строки приходят готовыми JSON-массивом
// `[{name, meta, chosen}]` из `detailFiles`. Здесь только показ и выбор:
// по умолчанию выбран автоматический, щелчок по строке выбирает другой, и
// главная кнопка панели ставит выбранный (`differs` — выбран не
// автоматический). Файл один — блока нет.
//
// Отдельным компонентом — ради стенда `LiveModelsProbe.qml` (сценарий
// «storefiles»): панель деталей целиком без живого магазина не поднять.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

ColumnLayout {
    id: block

    /// Палитра скина; без неё — безопасные цвета steam_dark.
    property var pal: null
    /// Строки ядра (JSON-массив).
    property string filesJson: "[]"
    property string title: ""
    property string hint: ""
    property string willInstallText: ""

    readonly property var files: block.parse(block.filesJson)
    readonly property string autoName: block.autoOf(block.files)
    /// Имя выбранного файла — его страница отдаёт в `installById`.
    property string selectedName: ""
    readonly property bool differs: block.selectedName !== ""
                                    && block.selectedName !== block.autoName

    readonly property color colText: block.pal ? block.pal.text_primary : "#ffffff"
    readonly property color colTextSecondary: block.pal ? block.pal.text_secondary : "#8f98a0"
    readonly property color colAccent: block.pal ? block.pal.accent : "#66c0f4"
    readonly property color colBorder: block.pal ? block.pal.border : "#2a4055"
    readonly property color colHover: block.pal ? block.pal.bg_card_hover : "#2a3f5a"

    visible: block.files.length > 1
    spacing: 4

    function parse(json) {
        try {
            const rows = JSON.parse(json);
            return Array.isArray(rows) ? rows : [];
        } catch (e) {
            return [];
        }
    }

    function autoOf(rows) {
        for (let i = 0; i < rows.length; i++) {
            if (rows[i].chosen)
                return rows[i].name;
        }
        return "";
    }

    function select(name) {
        block.selectedName = name;
    }

    // Другой товар (другой список) — выбор снова автоматический. Список
    // считается прямо из строки: привязка `files` к этому моменту может
    // ещё не пересчитаться.
    onFilesJsonChanged: block.selectedName = block.autoOf(block.parse(block.filesJson))
    Component.onCompleted: block.selectedName = block.autoOf(block.parse(block.filesJson))

    Text {
        text: block.title
        color: block.colTextSecondary
        font.bold: true
    }

    Text {
        Layout.fillWidth: true
        text: block.hint
        color: block.colTextSecondary
        textFormat: Text.PlainText
        wrapMode: Text.WordWrap
    }

    Repeater {
        model: block.files

        delegate: Rectangle {
            id: row

            required property var modelData
            required property int index
            readonly property bool picked: row.modelData.name === block.selectedName

            objectName: "storeFile" + row.index
            Layout.fillWidth: true
            // Минимум контрола из канона (Big Picture / Steam Deck).
            implicitHeight: Math.max(40, rowLine.implicitHeight + 12)
            radius: 3
            color: rowMouse.containsMouse ? block.colHover : "transparent"
            border.color: row.activeFocus ? block.colAccent
                                          : (row.picked ? block.colAccent : block.colBorder)
            border.width: row.activeFocus ? 2 : 1
            activeFocusOnTab: true
            Keys.onSpacePressed: block.select(row.modelData.name)
            Keys.onReturnPressed: block.select(row.modelData.name)

            RowLayout {
                id: rowLine
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10
                spacing: 8

                Text {
                    text: row.picked ? "◉" : "○"
                    color: row.picked ? block.colAccent : block.colTextSecondary
                    font.pixelSize: 18
                }

                Text {
                    Layout.fillWidth: true
                    text: row.modelData.name
                    color: block.colText
                    textFormat: Text.PlainText
                    elide: Text.ElideMiddle
                }

                Text {
                    visible: row.modelData.meta !== ""
                    text: row.modelData.meta
                    color: block.colTextSecondary
                    textFormat: Text.PlainText
                }

                Text {
                    objectName: "storeFileMark"
                    visible: row.picked
                    text: block.willInstallText
                    color: block.colAccent
                    font.bold: true
                    textFormat: Text.PlainText
                }
            }

            MouseArea {
                id: rowMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: block.select(row.modelData.name)
            }
        }
    }
}
