// Вход в крупный просмотр с миниатюры — ОДИН на весь продукт.
//
// Кладётся поверх миниатюры (`anchors.fill: parent`) и делает её понятной:
// под курсором в углу появляется лупа, курсор становится рукой, щелчок
// открывает крупный просмотр (`RexonImageViewer.qml`). С клавиатуры —
// Tab на миниатюру, затем Enter или пробел.
//
// `badgeOnly` — для карточек, где щелчок по картинке уже занят другим делом
// (выбор ролика, открытие описания): тогда просмотр открывает только лупа,
// а остальная карточка ведёт себя как прежде.
//
// Мышь слушает `MouseArea`, а не `TapHandler`: нажатие, принятое здесь, до
// обработчиков самой карточки не доходит, и одно нажатие не делает два дела.

import QtQuick
import QtQuick.Controls

Item {
    id: area

    /// Палитра скина страницы (рамка фокуса — её акцентом).
    property var pal: null
    /// Просмотр открывает только лупа, а не вся миниатюра.
    property bool badgeOnly: false
    /// Подсказка под курсором.
    property string tooltip: qsTranslate("rexon", "Open large view")
    /// Скругление рамки фокуса — как у миниатюры под ней.
    property real radius: 4

    /// Миниатюру просят показать крупно.
    signal activated()

    readonly property color colAccent: area.pal && area.pal.accent !== undefined
                                       ? area.pal.accent : "#66c0f4"
    readonly property bool hot: areaHover.hovered || area.activeFocus

    activeFocusOnTab: area.enabled && area.visible

    Keys.onReturnPressed: area.activated()
    Keys.onEnterPressed: area.activated()
    Keys.onSpacePressed: area.activated()

    HoverHandler { id: areaHover }

    MouseArea {
        id: wholeArea
        anchors.fill: parent
        enabled: !area.badgeOnly
        visible: enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: area.activated()
        ToolTip.text: area.tooltip
        ToolTip.visible: wholeArea.containsMouse && area.tooltip !== ""
        ToolTip.delay: 600
    }

    // Рамка фокуса: с клавиатуры видно, какая миниатюра откроется.
    Rectangle {
        anchors.fill: parent
        visible: area.activeFocus
        color: "transparent"
        radius: area.radius
        border.color: area.colAccent
        border.width: 2
    }

    Rectangle {
        id: badge
        objectName: "zoomBadge"
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 4
        width: 28
        height: 28
        radius: 14
        visible: area.hot
        color: badgeArea.containsMouse ? "#e6000000" : "#b3000000"
        border.width: 1
        border.color: badgeArea.containsMouse || area.activeFocus ? area.colAccent
                                                                  : "#40ffffff"

        Text {
            anchors.centerIn: parent
            text: "🔍"
            font.pixelSize: 14
        }

        MouseArea {
            id: badgeArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: area.activated()
            ToolTip.text: area.tooltip
            ToolTip.visible: badgeArea.containsMouse && area.tooltip !== ""
            ToolTip.delay: 600
        }
    }
}
