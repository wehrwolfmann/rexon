// Кнопка необратимого действия («🗑 Удалить») в скине приложения.
//
// Отдельный тип, а не `RexonButton` с подкрашенной подложкой: красная
// кнопка встречается на каждой плитке расширения, в строке списка и в
// подробном разборе, и третья копия одного и того же выражения цвета
// разъехалась бы молча (правило #12).
//
// Своего цвета кнопка НЕ заводит: и оттенок, и обе прозрачности приезжают
// из ядра палитрой (`qt_extensions::DANGER_COLOR` и `DANGER_FILL_*_ALPHA`),
// куда они переехали из QSS виджетной вкладки — там это было
// `rgba(220,80,80,0.2)` и `rgba(220,80,80,0.4)`.

import QtQuick
import QtQuick.Controls

Button {
    id: control

    /// Палитра скина; без неё кнопка встаёт на безопасные цвета steam_dark.
    property var pal: null

    /// Краски ЭТОЙ кнопки от ядра, когда они не совпадают с общими из
    /// палитры: карта с ключами `color` (оттенок), `fillAlpha`,
    /// `fillHoverAlpha`, `textColor`, `bold`, `disabledBackground`,
    /// `disabledColor`, `paddingH`, `paddingV`; любого может не быть.
    ///
    /// ЗАЧЕМ. У виджетных «Настроек» две разные красные кнопки: остановка
    /// обоев и звуков — с красной жирной подписью и серым выключенным
    /// состоянием, «⏹» в строке звука — оттенком кнопок удаления
    /// «Расширений» и своей заливкой. Обе приезжают из ядра; своих красок
    /// кнопка по-прежнему не заводит.
    property var tone: null

    readonly property bool hasDisabledTone: control.tone
                                            && control.tone.disabledBackground !== undefined

    readonly property color colDanger: control.tone && control.tone.color !== undefined
                                       ? control.tone.color
                                       : (control.pal ? control.pal.danger : "#dc5050")
    readonly property real fillAlpha: control.tone && control.tone.fillAlpha !== undefined
                                      ? control.tone.fillAlpha
                                      : (control.pal ? control.pal.danger_fill_alpha : 0.2)
    readonly property real fillHoverAlpha: control.tone
                                           && control.tone.fillHoverAlpha !== undefined
                                           ? control.tone.fillHoverAlpha
                                           : (control.pal ? control.pal.danger_fill_hover_alpha
                                                          : 0.4)
    readonly property color colAccent: control.pal ? control.pal.accent : "#66c0f4"
    readonly property color colTextBtn: control.pal ? control.pal.text_primary : "#ffffff"
    /// Краска НЕАКТИВНОЙ надписи — из палитры ядра, как у `RexonButton`.
    readonly property color colTextOff: control.pal ? control.pal.text_disabled
                                                    : "#8cffffff"

    // Высота 40 — минимум контрола из канона (Big Picture / Steam Deck).
    implicitHeight: 40
    // Выключенная кнопка гасится НАДПИСЬЮ, а не прозрачностью всей кнопки —
    // то же решение и по той же причине, что у `RexonButton`: прозрачность
    // меняла краску заливки, а оригинал заливку не трогает.
    padding: control.tone && control.tone.paddingV !== undefined
             ? control.tone.paddingV : 10
    leftPadding: control.tone && control.tone.paddingH !== undefined
                 ? control.tone.paddingH : 14
    rightPadding: leftPadding
    font.bold: control.tone !== null && control.tone.bold === true

    background: Rectangle {
        radius: 4
        color: !control.enabled && control.hasDisabledTone
               ? control.tone.disabledBackground
               : Qt.rgba(control.colDanger.r, control.colDanger.g, control.colDanger.b,
                         control.hovered || control.down ? control.fillHoverAlpha
                                                         : control.fillAlpha)
        // Рамка фокуса видна всегда, когда кнопка сфокусирована: это и есть
        // цепочка фокуса, а не украшение.
        border.color: control.activeFocus ? control.colAccent : "transparent"
        border.width: 2
    }

    // Надпись — на общей базовой линии по шрифту кнопки, значок из одного
    // символа не сокращается: то же правило и та же причина, что у
    // `RexonButton` (подписи «🗑 …» запасным шрифтом сидели выше соседних
    // кнопок ряда).
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
                   ? (control.tone && control.tone.disabledColor !== undefined
                      ? control.tone.disabledColor : control.colTextOff)
                   : (control.tone && control.tone.textColor !== undefined
                      ? control.tone.textColor : control.colTextBtn)
            font: control.font
            horizontalAlignment: Text.AlignHCenter
            elide: control.text.length <= 3 && control.text.indexOf(" ") < 0
                   ? Text.ElideNone : Text.ElideRight
        }
    }
}
