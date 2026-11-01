// Всплывающая плашка состояния — одна на все вкладки продукта.
//
// ⭐ Почему отдельный тип, а не плашка внутри каждой вкладки (правило #12).
// Один и тот же дефект — «сообщение не гаснет никогда» — заводился ТРИЖДЫ:
// в виджетных «Настройках» (28.06), в виджетном «Steam» (08.07) и в третий
// раз в QML-вкладке «Steam», где живая проверка 18.09 застала плашку висящей
// 29 минут подряд. Чинить его в четвёртый раз в четвёртом файле смысла нет:
// пока таймер, гашение и краски живут ЗДЕСЬ, новой копии дефекта завестись
// негде, а новая вкладка получает правильную плашку одной строкой разметки.
//
// Своих чисел и красок у плашки НЕТ вовсе:
//   * краски и поля приезжают картой `styles.toast` — из ядра
//     (`ui_base::toast_colors` и константы `TOAST_*` рядом с ней), теми же,
//     которыми красится виджетная плашка;
//   * ширина считается ядром (`ui_base::toast_width`) через метод модели
//     вкладки — плашка ложится ПОВЕРХ содержимого и обязана помещаться в
//     окно на любом размере;
//   * срок жизни — `ui_base::TOAST_TIMEOUT_MS`.
//
// Место — низ СЛЕВА с отступом ядра: там же, где стоит виджетная плашка.

pragma ComponentBehavior: Bound

import QtQuick

Rectangle {
    id: plate

    // ── Договор с вкладкой ────────────────────────────────────────────────

    /// Краски и метрики от ядра: карта `styles.toast` модели вкладки.
    required property var skin

    /// Через сколько плашка гаснет сама, мс (`ui_base::TOAST_TIMEOUT_MS`).
    /// Ноль и меньше — не гаснуть; так плашку можно оставить висеть
    /// осознанно, а не по забытому таймеру.
    required property int timeoutMs

    /// Модель вкладки — у неё спрашивается ширина плашки методом
    /// `toastWidth(ширинаСтраницы, ширинаТекста)`. Считает всё равно ядро.
    required property var metrics

    /// Ширина, в которую плашке нельзя не поместиться. По умолчанию —
    /// ширина родителя, то есть самой страницы.
    property int availableWidth: plate.parent ? plate.parent.width : 0

    /// Как разбирать текст. По умолчанию разметка — так плашку «Настроек»
    /// показывают с `<b>` и `<br>` из ядра. Вкладке, чьи сообщения несут
    /// имена файлов и пути, честнее поставить `Text.PlainText`.
    property int textFormat: Text.RichText

    /// Текст на плашке. Наружу — чтобы проверки читали его у самой плашки,
    /// а не угадывали по свойству страницы.
    property string message: ""

    /// Видна ли плашка человеку прямо сейчас. Отдельно от `visible`:
    /// в середине гашения `visible` ещё истинно, а сообщение уже уходит.
    readonly property bool shown: plate.opacity > 0

    // ── Показать и погасить ───────────────────────────────────────────────

    /// Показать сообщение и завести таймер гашения заново. Пустой текст —
    /// не «показать пустоту», а «сказать нечего»: плашка уходит.
    function show(text) {
        if (!text) {
            plate.dismiss();
            return;
        }
        plate.message = text;
        plate.opacity = 1;
        hideTimer.stop();
        if (plate.timeoutMs > 0)
            hideTimer.restart();
    }

    /// Погасить немедленно.
    // Имя не `hide`: у Item уже есть `visible`, и глагол-двойник в разметке
    // читался бы как его переключатель.
    function dismiss() {
        hideTimer.stop();
        plate.opacity = 0;
    }

    // ── Вид ───────────────────────────────────────────────────────────────

    /// Поле от края плашки до текста: внутреннее поле плюс рамка — ровно то,
    /// что у виджетной метки приходит таблицей стилей.
    readonly property int inset: plate.skin.paddingH + plate.skin.borderWidth

    anchors.left: parent ? parent.left : undefined
    anchors.bottom: parent ? parent.bottom : undefined
    anchors.leftMargin: plate.skin.margin
    anchors.bottomMargin: plate.skin.margin

    width: plate.metrics.toastWidth(plate.availableWidth, label.implicitWidth)
    height: label.implicitHeight + 2 * (plate.skin.paddingV + plate.skin.borderWidth)

    radius: plate.skin.radius
    color: plate.skin.background
    border.color: plate.skin.border
    border.width: plate.skin.borderWidth

    opacity: 0
    // Погасшая плашка не должна перехватывать ни щелчки, ни проверку вида.
    visible: opacity > 0

    Behavior on opacity {
        NumberAnimation { duration: 150 }
    }

    Text {
        id: label
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: plate.inset
        anchors.rightMargin: plate.inset
        text: plate.message
        textFormat: plate.textFormat
        wrapMode: Text.WordWrap
        color: plate.skin.textColor
    }

    Timer {
        id: hideTimer
        interval: plate.timeoutMs
        onTriggered: plate.opacity = 0
    }
}
