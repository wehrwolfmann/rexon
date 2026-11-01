// Кнопка QML-вкладок Rexon в стандартном скине приложения.
//
// Зачем отдельный тип: стиль QtQuick Controls по умолчанию (Basic) НЕ
// следует палитре приложения — его цвета прибиты в самом стиле. Виджетная
// часть Rexon красится QPalette из `ui_base` и потому выглядит как продукт,
// а голая `Button` на той же странице осталась бы светлой на тёмном фоне.
// Менять стиль на Fusion процессу целиком нельзя: в том же процессе живёт
// вкладка Wallpaper Engine с вендорным QML, и смена стиля на ходу задела бы
// её вид.
//
// Цвета приходят снаружи, свойством `pal` — это карта из
// `ui_base::skin_colors("steam_dark")`, отданная Rust-объектом страницы.
// Своей палитры кнопка не заводит: скин один на весь продукт (правило #17).
//
// Высота 40 — минимум контрола из канона (Big Picture / Steam Deck): вкладки
// обязаны быть управляемы не только мышью.

import QtQuick
import QtQuick.Controls

Button {
    id: control

    /// Палитра скина; без неё кнопка встаёт на безопасные цвета steam_dark.
    property var pal: null

    /// Краски «особой» кнопки от ядра — главной («Применить», «Смотреть на
    /// весь экран»), зелёной («✓ Применено») либо чипа-переключателя.
    /// Карта с ключами `background`, `hover`, `pressed`, `disabled`,
    /// `disabledBorder`, `disabledColor`, `color`, `border`, `hoverBorder`,
    /// `radius`,
    /// `paddingH`, `paddingV`, `bold`; любого может не быть. Пусто — обычная
    /// кнопка скина. Выключенная кнопка с явными красками (`disabled`)
    /// красится ими, а не гасится прозрачностью: у виджетов «Настроек»
    /// выключенная синяя кнопка серая, а не бледно-синяя.
    ///
    /// ЗАЧЕМ. У виджетной части такие кнопки красит своя таблица стилей, и
    /// без этого свойства перенесённая вкладка теряла акцент: «Применить»
    /// выходила серой наравне с «Отмена», хотя в оригинале она синяя, а
    /// «✓ Применено» — зелёная. Своих красок кнопка по-прежнему не заводит:
    /// всё приходит готовым из ядра.
    property var tone: null

    /// Краски ВЫБРАННОГО состояния (для `checkable`-чипов): те же ключи.
    /// Пусто — выбор рисуется обычным `highlighted`.
    property var checkedTone: null

    /// Что красит кнопку прямо сейчас: выбранный чип важнее обычного тона.
    readonly property var activeTone: (control.checked && control.checkedTone)
                                      ? control.checkedTone : control.tone

    // Кнопку Qt красит ролью `Button` — краска приходит готовой из ядра
    // (`surface_button`). Токен `border` совпадал с ней только на
    // стандартном скине: у «Светлого» роль `Button` #e3e6e8, а токен
    // `border` #cdd3da, и кнопка выходила рамочного цвета.
    readonly property color colButton: control.pal ? control.pal.surface_button
                                                   : "#2a4055"
    readonly property color colButtonHover: control.pal ? control.pal.bg_card_hover
                                                        : "#263d52"
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    /// Обводка кнопки БЕЗ красок от ядра. Её рисует сам стиль Qt, а не роль
    /// палитры: замер 05.09 на живой Qt (скин steam_dark) дал у кнопки
    /// «Быстрых пар» «Сравнения» заливку (42,64,85) и рамку (85,102,119) —
    /// накладку белого 0.20 поверх заливки. Краска приходит готовой из ядра
    /// (`ui_base::CONTROL_OUTLINE_*`, поверхность `surface_button_outline`),
    /// как и краска рукоятки сплиттера «Рабочих столов».
    readonly property color colOutline: control.pal
                                        && control.pal.surface_button_outline !== undefined
                                        ? control.pal.surface_button_outline
                                        : "#556677"
    readonly property color colTextBtn: control.pal ? control.pal.text_primary : "#ffffff"
    /// Краска НЕАКТИВНОЙ надписи — из палитры ядра (группа `Disabled`
    /// палитры Qt). Своей доли кнопка не заводит.
    readonly property color colTextOff: control.pal ? control.pal.text_disabled
                                                    : "#8cffffff"

    // Надпись «особой» кнопки набрана плотнее: у виджетной части это
    // `font-weight: 600` в её таблице стилей.
    font.bold: control.activeTone !== null && control.activeTone.bold === true

    implicitHeight: 40
    // ⚠️ Выключенная кнопка гасится НАДПИСЬЮ, а не прозрачностью всей
    // кнопки. Прозрачность 0.45 стояла здесь, чтобы «не заводить своих
    // цветов», но она подмешивала фон страницы и МЕНЯЛА КРАСКУ ЗАЛИВКИ, а
    // виджетный оригинал заливку и рамку оставляет как есть и приглушает
    // одну подпись (`QPushButton:disabled { color: … }` у блока
    // «Установить» «Расширений», `#PageBtn:disabled` у «Оформления», и то
    // же самое даёт Qt группой `Disabled` палитры без всяких таблиц
    // стилей). Замер 05.09: 10 108 точек на «Расширениях». Краска
    // приглушённой надписи приходит из ядра (`text_disabled`).
    padding: (control.activeTone && control.activeTone.paddingV !== undefined)
             ? control.activeTone.paddingV : 10
    leftPadding: (control.activeTone && control.activeTone.paddingH !== undefined)
                 ? control.activeTone.paddingH : 14
    rightPadding: leftPadding

    background: Rectangle {
        radius: (control.activeTone && control.activeTone.radius !== undefined)
                ? control.activeTone.radius : 4
        // Порядок разбора: краски от ядра важнее всего, дальше — выделенная
        // кнопка (highlighted), дальше — обычная кнопка скина.
        color: control.activeTone
               ? (!control.enabled && control.activeTone.disabled !== undefined
                  ? control.activeTone.disabled
                  : (control.down && control.activeTone.pressed !== undefined
                     ? control.activeTone.pressed
                     : ((control.hovered || control.down)
                        && control.activeTone.hover !== undefined
                        ? control.activeTone.hover
                        : (control.activeTone.background !== undefined
                           ? control.activeTone.background
                           : "transparent"))))
               : (control.highlighted
                  ? Qt.rgba(control.colAccent.r, control.colAccent.g,
                            control.colAccent.b, control.down ? 0.55 : 0.35)
                  : (control.hovered || control.down ? control.colButtonHover
                                                     : control.colButton))
        // Рамка фокуса видна всегда, когда кнопка сфокусирована: это и есть
        // цепочка фокуса, а не украшение. Своя рамка чипа видна, только
        // когда фокус не здесь, — иначе её не отличить от рамки фокуса.
        // Под курсором у части кнопок оригинала меняется ОДНА рамка, а
        // заливка стоит на месте (`#RefreshBtn:hover`, `#DetailsBtn:hover`
        // в таблице стилей «Оформления»).
        border.color: control.activeFocus
                      ? control.colAccent
                      : (!control.enabled && control.activeTone
                         && control.activeTone.disabledBorder !== undefined
                         ? control.activeTone.disabledBorder
                         : ((control.hovered || control.down)
                            && control.activeTone
                            && control.activeTone.hoverBorder !== undefined
                            ? control.activeTone.hoverBorder
                            : ((control.activeTone
                                && control.activeTone.border !== undefined)
                               ? control.activeTone.border
                               : control.colOutline)))
        // Кнопка БЕЗ красок от ядра обведена всегда: так её рисует стиль Qt
        // у оригинала. Кнопке с красками рамку даёт (или не даёт) ядро —
        // там таблица стилей вкладки перекрывает стиль целиком.
        border.width: control.activeFocus
                      ? 2
                      : (!control.activeTone
                         || control.activeTone.border !== undefined ? 1 : 0)
    }

    // ⚠️ Надпись стоит на ОБЩЕЙ базовой линии, посчитанной по шрифту кнопки,
    // а не по высоте строки. Значок в начале подписи («🗑», «🔄») рисуется
    // запасным шрифтом, и у каждого такого шрифта свой подъём: строка
    // «🗑 Удалить» выходила выше соседних «🔄 Переключиться» и «🔧 Проверить»
    // на 3 точки (снимок владельца 08.10). Теперь базовая линия у всех кнопок
    // ряда одна, какой бы значок ни стоял впереди.
    //
    // ⚠️ Подпись-значок (один символ: «🔍», «❮», «×») НЕ сокращается
    // многоточием: на круглой кнопке поверх картинки места меньше ширины
    // значка, и Qt вместо лупы рисовал пустоту — тёмный круг без значка в
    // углу карусели. Такой значок рисуется по центру целиком.
    contentItem: Item {
        implicitWidth: label.implicitWidth
        implicitHeight: labelMetrics.height

        FontMetrics {
            id: labelMetrics
            font: control.font
        }

        Text {
            id: label
            width: parent.width
            y: Math.round((parent.height - labelMetrics.height) / 2
                          + labelMetrics.ascent - label.baselineOffset)
            text: control.text
            color: !control.enabled
                   ? ((control.activeTone
                       && control.activeTone.disabledColor !== undefined)
                      ? control.activeTone.disabledColor : control.colTextOff)
                   : ((control.activeTone && control.activeTone.color !== undefined)
                      ? control.activeTone.color : control.colTextBtn)
            font: control.font
            horizontalAlignment: Text.AlignHCenter
            elide: control.text.length <= 3 && control.text.indexOf(" ") < 0
                   ? Text.ElideNone : Text.ElideRight
        }
    }
}
