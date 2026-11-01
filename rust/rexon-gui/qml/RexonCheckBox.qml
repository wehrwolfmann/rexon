// Галочка QML-вкладок Rexon в стандартном скине приложения.
//
// Та же причина, что у RexonButton: стиль QtQuick Controls по умолчанию
// (Basic) НЕ следует палитре приложения — его цвета прибиты внутри самого
// стиля, и галочка на тёмной странице осталась бы светлой. Менять стиль
// процессу целиком нельзя: в том же процессе живёт вкладка Wallpaper Engine
// с вендорным QML.
//
// Цвета приходят снаружи свойством `pal` — картой из
// `ui_base::skin_colors("steam_dark")`. Своих цветов контрол не заводит
// (правило #17: скин один на весь продукт).

import QtQuick
import QtQuick.Controls

CheckBox {
    id: control

    /// Палитра скина; без неё контрол встаёт на безопасные цвета steam_dark.
    property var pal: null

    readonly property color colBox: control.pal ? control.pal.border : "#2a4055"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colText: control.pal ? control.pal.text_primary : "#ffffff"

    // Минимум контрола из канона (Big Picture / Steam Deck): вкладки обязаны
    // быть управляемы не только мышью.
    implicitHeight: 40
    padding: 6

    indicator: Rectangle {
        implicitWidth: 20
        implicitHeight: 20
        x: control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: 3
        // Включённый флажок — сам знак «✅»: это уже зелёный квадрат с
        // галочкой, и своя заливка с рамкой под ним дали бы «квадрат в
        // квадрате». Поэтому у включённого рамка и заливка убраны, а знак
        // занимает всё место индикатора. Пустой флажок — прежняя рамка.
        color: "transparent"
        border.color: control.checked ? "transparent"
                      : (control.activeFocus || control.hovered
                         ? control.colAccent : control.colBox)
        border.width: control.activeFocus ? 2 : 1

        // Сама галочка: знак шрифта, а не картинка — иначе понадобился бы
        // внешний файл, а страница обязана оставаться самодостаточной.
        Text {
            anchors.centerIn: parent
            visible: control.checked
            text: "✅"
            // Цветной знак краски текста не слушается, поэтому недоступный
            // флажок гасится прозрачностью самого знака.
            opacity: control.enabled ? 1.0 : 0.4
            font.pixelSize: 21
        }

        // Рамка фокуса и наведения у включённого флажка — поверх знака:
        // под ним её не было бы видно.
        Rectangle {
            anchors.fill: parent
            visible: control.checked && (control.activeFocus || control.hovered)
            radius: parent.radius
            color: "transparent"
            border.color: control.colAccent
            border.width: control.activeFocus ? 2 : 1
        }
    }

    contentItem: Text {
        leftPadding: control.indicator.width + control.spacing
        text: control.text
        color: control.colText
        font: control.font
        textFormat: Text.RichText
        wrapMode: Text.WordWrap
        verticalAlignment: Text.AlignVCenter
    }
}
