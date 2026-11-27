// Animated thumbnail — a replacement for `QExtra.AnimatedImage` built on stock
// Qt Quick elements.
//
// OUR file, not vendored. Since v0.4.4 the vendor's `ThumbnailImage.qml`
// shows previews through QExtra (a C++ library by the waywallen author; when its
// plugin is not built or not usable, the core takes this stand-in — see
// ImageCache.qml next to it). Here is the same set of properties that the markup
// sets on it and reads.
//
// HOW IT WORKS: two layers.
//   * `m_still` — a stock `Image`: it decodes in a Qt background thread and can
//     keep the previous frame while a new one is loading (`retainWhileLoading`).
//     It shows the picture and is responsible for `status`.
//   * `m_movie` — a stock `AnimatedImage`: it gets a source only when
//     `m_still` is already ready and has reported more than one frame. While the
//     animation runs, the still layer is hidden (otherwise its first frame would show
//     through the transparent parts of the animation).
//
// WHY NOT A SINGLE `AnimatedImage`. The stock `AnimatedImage` reads a local
// file right in the `source` assignment, on the GUI thread; its
// `asynchronous` does not affect that. Measured on 11.10.2026 (Qt 6.12, a
// 3000x3000 picture): the assignment takes 85–93 ms, and all that time the window stalls —
// not a single frame is drawn between the source change and the new frame. For
// `Image` the same assignment takes 0–1 ms. Still pictures (and most wallpaper
// previews are such) do not need the animation decoder at all.
//
// What is replicated:
//   * `maxSize` — the decode size cap. If `sourceSize` is set —
//     it is used, but not larger than the cap; if not set — the size of the element itself
//     (see `_wantedSize` below), also not larger than the cap. Qt itself keeps
//     the aspect ratio;
//   * `cache` together with the `ImageCache.capacityMiB` switch (0 — no cache);
//   * `retainWhileLoading` — the previous frame (and the previous animation) stays on
//     screen until the new source is decoded; without it the element goes blank for that
//     time, as in the original;
//   * `playing` survives a source change: the stock `AnimatedImage` turns it
//     off on a non-animated picture itself, so on every `Ready` the
//     flag is set again — as the native `ThumbnailImage` branch does.
//
// What is NOT replicated and why:
//   * `sharedPlayback` (a shared clock for identical animations) is accepted but
//     does nothing: the stock Qt decoder has no such mode. Visually
//     this only means that two identical animations may run out of sync;
//   * for an ANIMATED file the first animation frame is decoded on the GUI
//     thread (that is how the stock `AnimatedImage` works) — already after
//     the still layer has shown the picture;
//   * waywallen 0.6.0: `loadingMode` (`Image.qml` next to it) — `Suspended`
//     does not load at all, `Prefetch` loads as usual (the stock decoder
//     has no queueing); `preparationScope` and `frameDataRequired`
//     are accepted and do nothing.
import QtQuick

Item {
    id: root

    property url source
    // waywallen 0.6.1 (QExtra 1ce54ee): HTTP headers for the request, e.g. a
    // Referer some sources require for their previews. Accepted so the vendor
    // markup loads; the stock Image cannot send headers, so such previews
    // stay empty with this substitute and need the real QExtra plugin.
    property var sourceHeaders: ({})
    property size sourceSize: Qt.size(-1, -1)
    property size maxSize: Qt.size(-1, -1)
    property int fillMode: Image.Stretch
    property int horizontalAlignment: Image.AlignHCenter
    property int verticalAlignment: Image.AlignVCenter
    property bool cache: true
    property bool mipmap: false
    property bool mirror: false
    property bool playing: true
    property bool paused: false
    property real speed: 1
    property bool sharedPlayback: false
    property bool retainWhileLoading: false
    // waywallen 0.6.0 (QExtra 3fcb45c): loading mode and frame preparation.
    property int loadingMode: 0
    property Item preparationScope
    property bool frameDataRequired: true

    // There is a frame on screen — the picture is ready (or retained while loading).
    readonly property bool hasFrame: m_still.status === Image.Ready
        || (root.retainWhileLoading && m_still.status === Image.Loading && m_still.implicitWidth > 0)
    readonly property size intrinsicSize: Qt.size(m_still.implicitWidth, m_still.implicitHeight)

    readonly property alias status: m_still.status
    readonly property alias progress: m_still.progress
    readonly property alias paintedWidth: m_still.paintedWidth
    readonly property alias paintedHeight: m_still.paintedHeight
    readonly property alias frameCount: m_still.frameCount
    property alias currentFrame: m_movie.currentFrame

    implicitWidth: m_still.implicitWidth
    implicitHeight: m_still.implicitHeight

    function restart() {
        m_movie.currentFrame = 0;
        m_movie.playing = root.playing;
    }

    // Cap along one axis: 0 or less — "not set".
    function _cap(value, limit) {
        if (limit <= 0)
            return value;
        return value > 0 ? Math.min(value, limit) : limit;
    }

    // Decode size when the caller did not set `sourceSize` itself:
    // the size of the element itself (i.e. the card), not the `maxSize` cap.
    //
    // WHY. The stock Qt decoder keeps animation frames at the size at which
    // it decoded them, and the upstream cap of 512x512 is three times larger than needed for a
    // showcase card. Measured on 10.10.2026 on a live library (40 wallpapers,
    // tab open, grid loaded): 1471 MB with the cap alone versus
    // 513 MB with the card size. Upstream the same role is played by the cache of
    // decoded frames in the QExtra C++ library, which Rexon does not have.
    //
    // Until 10.10.2026 this was done by a patch to the vendor's `ThumbnailImage.qml`
    // (patches/0001). Here, in OUR module, it costs nothing on a
    // vendor update: the foreign file stays as the author wrote it.
    //
    // A 64-pixel step — so that smooth window resizing does not
    // make the decoder redo the work on every pixel.
    readonly property int _decodeStep: 64
    function _stepped(value) {
        return value > 0 ? Math.ceil(value / _decodeStep) * _decodeStep : 0;
    }
    readonly property size _wantedSize: (sourceSize.width > 0 || sourceSize.height > 0)
        ? sourceSize : Qt.size(_stepped(root.width), _stepped(root.height))

    readonly property size _decodeSize: Qt.size(_cap(_wantedSize.width, maxSize.width), _cap(_wantedSize.height, maxSize.height))

    // Source of the animation layer. Changes only here, by the state of the
    // still layer: ready and more than one frame — the animation of this
    // file; ready and one frame — no animation. While a new file is loading,
    // the previous animation stays with `retainWhileLoading` and is removed without
    // it. A load failure always removes it.
    property url _movieSource
    function _syncMovie() {
        if (m_still.status === Image.Ready)
            root._movieSource = m_still.frameCount > 1 ? m_still.source : "";
        else if (m_still.status !== Image.Loading || !root.retainWhileLoading)
            root._movieSource = "";
    }

    onPlayingChanged: m_movie.playing = root.playing

    Image {
        id: m_still
        anchors.fill: parent
        visible: m_movie.status !== AnimatedImage.Ready
        asynchronous: true
        // `Suspended` (2): the card is off screen — do not load at all.
        source: root.loadingMode === 2 ? "" : root.source
        sourceSize: root._decodeSize
        fillMode: root.fillMode
        horizontalAlignment: root.horizontalAlignment
        verticalAlignment: root.verticalAlignment
        cache: root.cache && ImageCache.capacityMiB > 0
        mipmap: root.mipmap
        mirror: root.mirror
        // A picture from the Qt cache can be ready without a `status` change (it was
        // ready and stayed so), so all three signals are listened to.
        onStatusChanged: root._syncMovie()
        onSourceChanged: root._syncMovie()
        onFrameCountChanged: root._syncMovie()
        // `Image` has this property since Qt 6.8. It is set from here rather than in the line
        // above: on an older Qt an unknown property would prevent the whole element
        // from loading, while this way there is simply no retention there.
        Component.onCompleted: if ("retainWhileLoading" in m_still)
            m_still.retainWhileLoading = Qt.binding(() => root.retainWhileLoading)
    }

    AnimatedImage {
        id: m_movie
        anchors.fill: parent
        source: root._movieSource
        sourceSize: root._decodeSize
        fillMode: root.fillMode
        horizontalAlignment: root.horizontalAlignment
        verticalAlignment: root.verticalAlignment
        cache: m_still.cache
        mipmap: root.mipmap
        mirror: root.mirror
        paused: root.paused
        speed: root.speed
        playing: true
        onStatusChanged: if (status === AnimatedImage.Ready)
            playing = root.playing
    }
}
