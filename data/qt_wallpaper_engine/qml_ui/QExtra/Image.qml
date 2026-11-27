// Перечисления `QExtra.Image` — замена C++-типа на QML.
//
// НАШ файл, не вендорный. С waywallen 0.6.0 вендорная разметка
// (`component/ThumbnailImage.qml`, страницы «Скрытые обои» и плейлисты)
// читает у QExtra не только `AnimatedImage`, но и перечисления базового
// типа `Image`: режим загрузки (`QE.Image.Automatic`/`Prefetch`/
// `Suspended`) и состояние (`QE.Image.Ready`/`Error`…). Без типа с этим
// именем выражения дают `undefined`, и карточка не понимает, готова ли
// картинка. Сам элемент здесь не рисует ничего: картинки показывает
// `AnimatedImage.qml` рядом. Значения совпадают по порядку с C++-типом
// (QExtra 3fcb45c, `image.hpp`) и со `status` штатного `Image` Qt.
import QtQuick

Item {
    enum LoadingMode {
        Automatic,
        Prefetch,
        Suspended
    }
    enum Status {
        Null,
        Ready,
        Loading,
        Error
    }
}
