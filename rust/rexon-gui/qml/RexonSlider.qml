// Ползунок QML-вкладок Rexon в стандартном скине приложения.
// Причина отдельного типа — та же, что у RexonButton: стиль Basic не следует
// палитре приложения.

// Делегат делений обращается к id самого ползунка; без прагмы проверка QML
// справедливо ругается на неквалифицированный доступ.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

Slider {
    id: control

    /// Палитра скина.
    property var pal: null

    /// Шаг делений под дорожкой (в единицах значения); 0 — делений нет.
    /// Пара к `QSlider.TicksBelow` виджетной версии: у ползунка силы
    /// размытия деления стоят на каждом шаге.
    property real tickStep: 0

    readonly property color colTrack: control.pal ? control.pal.track
                                                  : "rgba(127,127,127,0.18)"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colBorder: control.pal ? control.pal.border : "#2a4055"

    implicitHeight: 40

    background: Rectangle {
        x: control.leftPadding
        y: control.topPadding + control.availableHeight / 2 - height / 2
        implicitWidth: 160
        implicitHeight: 6
        width: control.availableWidth
        height: implicitHeight
        radius: 3
        color: control.colTrack

        // Заливка — от НАЧАЛА шкалы до ручки: слева направо от левого края,
        // при языке справа налево от правого. Ширина по `position`, а не по
        // `visualPosition`: в зеркальном ползунке второе равно 1 − position,
        // и от левого края залилась бы не та сторона.
        Rectangle {
            x: control.mirrored ? parent.width - width : 0
            width: control.position * parent.width
            height: parent.height
            radius: 3
            color: control.colAccent
        }
    }

    // Деления: короткие штрихи под дорожкой, по одному на шаг, цветом рамки.
    Repeater {
        model: control.tickStep > 0
               ? Math.floor((control.to - control.from) / control.tickStep) + 1
               : 0

        delegate: Rectangle {
            required property int index
            readonly property real ratio: control.tickStep * index
                                          / (control.to - control.from)
            // Место штриха на экране: зеркальный ползунок отсчитывает шкалу
            // от правого края, как и ручку (`visualPosition`).
            readonly property real visualRatio: control.mirrored ? 1 - ratio : ratio
            x: control.leftPadding + control.handle.width / 2
               + visualRatio * (control.availableWidth - control.handle.width) - width / 2
            y: control.topPadding + control.availableHeight / 2
               + control.background.height / 2 + 3
            width: 1
            height: 5
            color: control.colBorder
        }
    }

    handle: Rectangle {
        x: control.leftPadding + control.visualPosition
           * (control.availableWidth - width)
        y: control.topPadding + control.availableHeight / 2 - height / 2
        implicitWidth: 18
        implicitHeight: 18
        radius: 9
        color: control.pressed ? control.colAccent : control.colBorder
        border.color: control.activeFocus || control.hovered
                      ? control.colAccent : control.colBorder
        border.width: 2
    }
}
