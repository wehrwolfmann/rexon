// `QExtra.Image` enumerations — a QML replacement for the C++ type.
//
// OUR file, not vendored. Since waywallen 0.6.0 the vendor markup
// (`component/ThumbnailImage.qml`, the «Hidden wallpapers» pages and playlists)
// reads from QExtra not only `AnimatedImage` but also the enumerations of the base
// type `Image`: the loading mode (`QE.Image.Automatic`/`Prefetch`/
// `Suspended`) and the status (`QE.Image.Ready`/`Error`…). Without a type with this
// name the expressions yield `undefined`, and the card cannot tell whether the
// picture is ready. The element itself draws nothing here: pictures are shown by
// `AnimatedImage.qml` next to it. The values match the C++ type in order
// (QExtra 3fcb45c, `image.hpp`) and the `status` of the stock Qt `Image`.
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
