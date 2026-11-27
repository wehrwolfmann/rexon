// Подстановки в переведённые строки QML — ОДНО место на весь наш QML.
//
// Ядро подставляет `%1` через `i18n::fill`, и при языке справа налево каждая
// подстановка уходит в изолят U+2068…U+2069 (FSI/PDI): имя файла, путь,
// число или английское название не перемешивают порядок слов арабской или
// еврейской фразы вокруг себя. Строки, которые собирает сам QML
// (`qsTranslate(…).arg(…)`), мимо ядра — у `String.arg` изолятов нет, поэтому
// они идут сюда:
//
//     RexonBidi { id: bidi }
//     text: bidi.arg(qsTranslate("rexon", "Saved to %1"), dir)
//
// Изолят ставится по направлению приложения (`Qt.application.layoutDirection`
// — то же, чем корень окна включает зеркалирование), а не по месту: даже в
// физической зоне строка арабская, и подстановке внутри неё нужна граница.
// Слева направо вывод побайтно прежний — ни одного невидимого знака.
//
// Сторож — `rust/rexon-core/tests/rtl_parity.rs`: `.arg(` в нашем QML пишется
// только в этом файле.

import QtQml

QtObject {
    id: bidi

    /// Интерфейс пишется справа налево.
    readonly property bool rtl: Qt.application.layoutDirection === Qt.RightToLeft

    /// Обернуть подстановку в изолят при письме справа налево.
    function isolate(value) {
        const s = String(value);
        return bidi.rtl && s !== "" ? "\u2068" + s + "\u2069" : s;
    }

    /// `template` с `%1`…`%9` на места `values` по номеру — как цепочка
    /// `.arg()` над переводом, но за один проход (подстановка, в которой
    /// самой есть «%2», не съест следующую) и с изолятами справа налево.
    /// Чужой номер и «%» без цифры остаются как есть.
    function arg(template) {
        const values = Array.prototype.slice.call(arguments, 1);
        return String(template).replace(/%([1-9])/g, (m, d) =>
            d - 1 < values.length ? bidi.isolate(values[d - 1]) : m);
    }
}
