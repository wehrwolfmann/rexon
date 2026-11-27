// Size of the decoded image cache — a replacement for the `QExtra.ImageCache` singleton.
//
// OUR file, not vendored. Since v0.4.4 the vendor waywallen markup
// (`Global.qml`) sets the thumbnail cache size through the QExtra module — a C++
// library by the waywallen author that their application links in. Rexon
// does not have it, and without a module with this name `Global.qml` does not load, and
// the «Wallpaper Engine» tab stays empty entirely.
//
// How this differs from the original: QExtra has its own decoder with a frame cache
// limited by `capacityMiB`. Here pictures are decoded by Qt Quick itself, and
// there is no way to set a cap in mebibytes for it from QML. So the setting
// «Decoded image cache» acts as a switch: 0 — thumbnails
// are not cached (`cache: false` on `AnimatedImage` below), any other
// value — they are cached by Qt's rules.
pragma Singleton
import QtQml

QtObject {
    // As in the original: there is no negative size.
    property int capacityMiB: 64
    onCapacityMiBChanged: if (capacityMiB < 0)
        capacityMiB = 0

    // waywallen 0.6.0: the window's TEXTURE cache cap. Stock Qt Quick has
    // no such cap — the call is accepted and does nothing.
    function setTextureCapacityMiB(window, value) {
    }
}
