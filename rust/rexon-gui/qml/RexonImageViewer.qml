// Large picture view — ONE for the whole product.
//
// Thumbnails exist in almost every tab (Steam backgrounds and screensavers,
// extensions, the wallpaper library, theme screenshots), but a picture
// could not be opened large everywhere, and where it could, each place did
// it its own way. This layer is shared: the page hands it the list of what
// is on the cards and the number of the card to open; it does everything
// else by itself.
//
// What it can do:
//   * the picture over the whole application window on a dimmed backdrop;
//   * "full screen" — with a button and the F key (the application window
//     goes full screen and returns to how it was on exit);
//   * closing — Esc, a button, a click outside the picture;
//   * paging through the list — on-screen arrows and the ← → keys (for
//     right to left "forward" is to the left, see `RexonArrows`);
//   * zoom — mouse wheel and the + − 0 keys, panning by dragging;
//     a double click toggles "fit / 1:1";
//   * caption: title, second line (game, source), a badge (for example,
//     "animated") and a counter "3 / 205";
//   * while the full-size picture loads, the thumbnail and a loading
//     indicator are visible; the window does not freeze meanwhile
//     (everything is read asynchronously);
//   * GIF/APNG/WebP play (`animatedImage`), a clip (`video`) loops muted
//     on top of the still frame.
//
// How to hook it up:
//
//     RexonImageViewer { id: viewer; pal: root.pal }
//     ...
//     RexonZoomArea { anchors.fill: parent; pal: root.pal
//                     onActivated: viewer.show(items, index) }
//
// A list item is a plain object:
//     { key, title, subtitle, badge, thumb, source, video,
//       animatedImage, pending }
// `thumb` is what is already on the card (shown immediately); `source` is
// the full-size picture; `video` is a clip. If the page does not have the
// full address yet, it sets `pending: true`, listens to `wanted(key)` and,
// when the file is ready, puts it into `sources`:
// { "<key>": { source, video, done } }.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Window
import QtMultimedia

Popup {
    id: viewer

    // ── Contract with the page ────────────────────────────────────────────

    /// Page skin palette (accent and text are taken from it).
    property var pal: null
    /// List of cards (see the file header).
    property var items: []
    /// Number of the card being shown.
    property int index: 0
    /// Late addresses: card key → { source, video, done }.
    property var sources: ({})
    /// Viewer backdrop. Darker than the common dimming of modal windows: the
    /// picture is viewed on near-black, not through the page.
    property color backdropColor: "#eb000000"

    /// Card `key` is shown (or its neighbour — it is preloaded): time for the
    /// page to obtain the full-size file if it does not have it yet.
    signal wanted(string key)

    readonly property int count: viewer.items ? viewer.items.length : 0

    /// "Back/forward" icons and the ← → keys follow the mirroring of the viewer
    /// itself: for right to left "forward" is to the left. The buttons swap
    /// anchors, and "⛶ ❌" move to the top left corner by the same row
    /// mirroring.
    readonly property RexonArrows arrows: RexonArrows {
        mirrored: viewer.mirrored
    }
    readonly property var current: viewer.count > 0
        ? viewer.items[Math.max(0, Math.min(viewer.index, viewer.count - 1))]
        : null
    readonly property string currentKey: viewer.current && viewer.current.key !== undefined
        ? String(viewer.current.key) : ""
    readonly property var late: viewer.currentKey !== "" && viewer.sources
                                && viewer.sources[viewer.currentKey]
        ? viewer.sources[viewer.currentKey] : null

    readonly property string thumbSource: viewer.current && viewer.current.thumb
        ? String(viewer.current.thumb) : ""
    readonly property string fullSource: viewer.late && viewer.late.source
        ? String(viewer.late.source)
        : (viewer.current && viewer.current.source ? String(viewer.current.source) : "")
    readonly property string videoSource: viewer.late && viewer.late.video
        ? String(viewer.late.video)
        : (viewer.current && viewer.current.video ? String(viewer.current.video) : "")
    readonly property bool animatedImage: viewer.current !== null
                                          && viewer.current.animatedImage === true

    /// The full-size picture is still on its way: the page promised it
    /// (`pending`) and has not reported (`done`), or the file itself is still
    /// being read.
    readonly property bool loading: viewer.opened && viewer.current !== null
        && ((viewer.current.pending === true && !(viewer.late && viewer.late.done === true))
            || (stage.fullItem !== null && stage.fullItem.status === Image.Loading)
            || (viewer.videoSource !== "" && !viewer.videoBroken && !stage.videoPlaying))
    /// The clip failed to open — the still frame remains.
    property bool videoBroken: false

    /// Open the viewer at card `at` of list `list`.
    function show(list, at) {
        viewer.items = list || [];
        if (viewer.count === 0)
            return;
        viewer.index = Math.max(0, Math.min(at | 0, viewer.count - 1));
        viewer.resetView();
        viewer.open();
        viewer.announce();
    }

    /// Paging wraps around.
    function step(delta) {
        if (viewer.count < 2)
            return;
        viewer.index = (viewer.index + delta + viewer.count) % viewer.count;
        viewer.resetView();
        viewer.announce();
    }

    /// Tell the page what is needed: the shown card and both neighbours.
    function announce() {
        const seen = {};
        for (const d of [0, 1, -1]) {
            if (viewer.count === 0)
                break;
            const it = viewer.items[(viewer.index + d + viewer.count) % viewer.count];
            if (!it || it.key === undefined)
                continue;
            const key = String(it.key);
            if (seen[key])
                continue;
            seen[key] = true;
            viewer.wanted(key);
        }
    }

    // ── Zoom and pan ──────────────────────────────────────────────────────

    /// Zoom relative to "fit": 1 means the whole picture fits in the window.
    property real zoom: 1.0
    property real panX: 0
    property real panY: 0
    readonly property real maxZoom: Math.max(8, 4 / Math.max(stage.fitScale, 0.01))
    readonly property bool zoomed: viewer.zoom > 1.001

    function resetView() {
        viewer.zoom = 1.0;
        viewer.panX = 0;
        viewer.panY = 0;
        viewer.videoBroken = false;
    }

    /// Do not let the picture drift past the edge: pan only by what did not fit.
    function clampPan() {
        const overX = Math.max(0, (stage.naturalWidth * stage.fitScale * viewer.zoom - stage.width) / 2);
        const overY = Math.max(0, (stage.naturalHeight * stage.fitScale * viewer.zoom - stage.height) / 2);
        viewer.panX = Math.max(-overX, Math.min(overX, viewer.panX));
        viewer.panY = Math.max(-overY, Math.min(overY, viewer.panY));
    }

    /// Set zoom `value` while keeping the same picture point under the cursor.
    function zoomTo(value, px, py) {
        const next = Math.max(1.0, Math.min(viewer.maxZoom, value));
        const ratio = next / viewer.zoom;
        const cx = px - stage.width / 2;
        const cy = py - stage.height / 2;
        viewer.panX = cx - (cx - viewer.panX) * ratio;
        viewer.panY = cy - (cy - viewer.panY) * ratio;
        viewer.zoom = next;
        viewer.clampPan();
    }

    /// Double click: "fit" ↔ "1:1" (one picture pixel to one screen pixel).
    function toggleActualSize(px, py) {
        if (viewer.zoomed) {
            viewer.zoom = 1.0;
            viewer.panX = 0;
            viewer.panY = 0;
            return;
        }
        viewer.zoomTo(1 / Math.max(stage.fitScale, 0.01), px, py);
    }

    // ── Full screen ───────────────────────────────────────────────────────
    //
    // The layer covers the whole application window, so "full screen" is the
    // full-screen mode of the window itself: the desktop panels go away, and
    // on exit the window returns to the state it was in (normal or maximized).
    readonly property var hostWindow: viewer.parent ? viewer.parent.Window.window : null
    readonly property bool fullScreen: viewer.hostWindow !== null
                                       && viewer.hostWindow.visibility === Window.FullScreen
    /// Window state before full-screen mode; -1 means we did not turn the mode on.
    property int savedVisibility: -1

    function toggleFullScreen() {
        const win = viewer.hostWindow;
        if (!win)
            return;
        if (viewer.fullScreen) {
            viewer.leaveFullScreen();
        } else {
            viewer.savedVisibility = win.visibility;
            win.showFullScreen();
        }
    }

    function leaveFullScreen() {
        const win = viewer.hostWindow;
        if (!win || !viewer.fullScreen)
            return;
        win.visibility = viewer.savedVisibility === Window.Maximized
            ? Window.Maximized : Window.Windowed;
        viewer.savedVisibility = -1;
    }

    // ── The layer itself ──────────────────────────────────────────────────

    parent: Overlay.overlay
    x: 0
    y: 0
    width: parent ? parent.width : 0
    height: parent ? parent.height : 0
    padding: 0
    margins: 0
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape
    enter: null
    exit: null

    // The layer dims by itself; a second dimming underneath would be redundant.
    Overlay.modal: Item {}
    background: Rectangle { color: viewer.backdropColor }

    /// The click that opened the viewer may be a double one. Its second half
    /// already arrives here and, landing outside the picture, would close the
    /// viewer at once.
    property double openedAt: 0

    onOpened: {
        viewer.openedAt = Date.now();
        stage.forceActiveFocus();
    }
    onClosed: {
        // We turned full-screen mode on, so we restore the window.
        if (viewer.savedVisibility >= 0)
            viewer.leaveFullScreen();
    }

    // Buttons over the picture: round, dark, with an accent border under the
    // cursor.
    readonly property color colAccent: viewer.pal && viewer.pal.accent !== undefined
                                       ? viewer.pal.accent : "#66c0f4"
    readonly property int chromeSize: 44
    readonly property int chromeMargin: 16
    readonly property var chromeTone: ({
        "background": "#99000000",
        "hover": "#d9000000",
        "border": "#40ffffff",
        "hoverBorder": viewer.colAccent,
        "color": "#ffffff",
        "radius": viewer.chromeSize / 2,
        "paddingH": 0,
        "paddingV": 0
    })

    contentItem: Item {
        id: stage
        objectName: "imageViewerStage"
        focus: true
        clip: true

        /// Full-size picture (ordinary or animated).
        readonly property var fullItem: viewer.fullSource === "" ? null
            : (viewer.animatedImage ? fullAnimated : fullStill)
        readonly property bool fullReady: stage.fullItem !== null
                                          && stage.fullItem.status === Image.Ready
        property bool videoPlaying: false

        // "As is" picture size: of the ready full-size one, otherwise of the
        // thumbnail, and while there is nothing — a nominal 16:9. Measured by an
        // ACTION (`measure`), not a binding: the pictures sit inside a canvas
        // whose size is computed from this measurement, and a binding would loop.
        property real naturalWidth: 1600
        property real naturalHeight: 900

        function measure() {
            const from = stage.fullReady ? stage.fullItem
                : (thumbImage.status === Image.Ready ? thumbImage : null);
            const w = from ? from.implicitWidth : 0;
            const h = from ? from.implicitHeight : 0;
            stage.naturalWidth = w > 0 && h > 0 ? w : 1600;
            stage.naturalHeight = w > 0 && h > 0 ? h : 900;
            viewer.clampPan();
        }
        readonly property real fitScale: stage.width > 0 && stage.height > 0
            ? Math.min(stage.width / Math.max(1, stage.naturalWidth),
                       stage.height / Math.max(1, stage.naturalHeight))
            : 1

        onWidthChanged: viewer.clampPan()
        onHeightChanged: viewer.clampPan()

        Keys.onPressed: (event) => {
            switch (event.key) {
            case Qt.Key_Left:
                viewer.step(viewer.arrows.leftStep);
                break;
            case Qt.Key_Right:
                viewer.step(viewer.arrows.rightStep);
                break;
            case Qt.Key_F:
            case Qt.Key_F11:
                viewer.toggleFullScreen();
                break;
            case Qt.Key_Plus:
            case Qt.Key_Equal:
                viewer.zoomTo(viewer.zoom * 1.25, stage.width / 2, stage.height / 2);
                break;
            case Qt.Key_Minus:
                viewer.zoomTo(viewer.zoom / 1.25, stage.width / 2, stage.height / 2);
                break;
            case Qt.Key_0:
                viewer.resetView();
                break;
            case Qt.Key_Escape:
                viewer.close();
                break;
            default:
                return;
            }
            event.accepted = true;
        }

        // Mouse over the whole layer: wheel zooms, dragging pans, a double click
        // on the picture toggles "fit / 1:1", a click outside exits.
        MouseArea {
            id: pointer
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton
            hoverEnabled: true
            cursorShape: viewer.zoomed
                         ? (pointer.pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor)
                         : Qt.ArrowCursor

            property real startX: 0
            property real startY: 0
            property real startPanX: 0
            property real startPanY: 0
            property bool dragged: false

            function onPicture(x, y) {
                return x >= canvas.x && x <= canvas.x + canvas.width
                    && y >= canvas.y && y <= canvas.y + canvas.height;
            }

            onPressed: (mouse) => {
                pointer.startX = mouse.x;
                pointer.startY = mouse.y;
                pointer.startPanX = viewer.panX;
                pointer.startPanY = viewer.panY;
                pointer.dragged = false;
            }
            onPositionChanged: (mouse) => {
                if (!pointer.pressed)
                    return;
                const dx = mouse.x - pointer.startX;
                const dy = mouse.y - pointer.startY;
                if (!pointer.dragged && Math.abs(dx) + Math.abs(dy) < 6)
                    return;
                pointer.dragged = true;
                if (!viewer.zoomed)
                    return;
                viewer.panX = pointer.startPanX + dx;
                viewer.panY = pointer.startPanY + dy;
                viewer.clampPan();
            }
            onClicked: (mouse) => {
                if (pointer.dragged || pointer.onPicture(mouse.x, mouse.y))
                    return;
                if (Date.now() - viewer.openedAt < 450)
                    return;
                viewer.close();
            }
            onDoubleClicked: (mouse) => {
                if (pointer.onPicture(mouse.x, mouse.y))
                    viewer.toggleActualSize(mouse.x, mouse.y);
            }
            onWheel: (wheel) => {
                const steps = wheel.angleDelta.y / 120;
                if (steps !== 0)
                    viewer.zoomTo(viewer.zoom * Math.pow(1.2, steps), wheel.x, wheel.y);
                wheel.accepted = true;
            }
        }

        Item {
            id: canvas
            objectName: "imageViewerCanvas"
            width: stage.naturalWidth * stage.fitScale * viewer.zoom
            height: stage.naturalHeight * stage.fitScale * viewer.zoom
            x: Math.round((stage.width - canvas.width) / 2 + viewer.panX)
            y: Math.round((stage.height - canvas.height) / 2 + viewer.panY)

            // The thumbnail is at the bottom and shown at once: while there is no
            // full-size picture it is visible, and an empty rectangle has no chance
            // to flash.
            Image {
                id: thumbImage
                anchors.fill: parent
                source: viewer.opened ? viewer.thumbSource : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                smooth: true
                visible: !stage.fullReady
                onStatusChanged: Qt.callLater(stage.measure)
            }

            Image {
                id: fullStill
                objectName: "imageViewerFull"
                anchors.fill: parent
                source: viewer.opened && !viewer.animatedImage ? viewer.fullSource : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                smooth: true
                mipmap: true
                visible: status === Image.Ready
                onStatusChanged: Qt.callLater(stage.measure)
            }

            AnimatedImage {
                id: fullAnimated
                anchors.fill: parent
                source: viewer.opened && viewer.animatedImage ? viewer.fullSource : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                smooth: true
                visible: status === Image.Ready
                playing: visible && viewer.opened
                onStatusChanged: Qt.callLater(stage.measure)
            }

            // The clip goes over the still frame. The player is created only when it
            // is needed: the Qt multimedia layer is heavy, and someone looking at
            // pictures should not pay for it.
            Loader {
                anchors.fill: parent
                active: viewer.opened && viewer.videoSource !== "" && !viewer.videoBroken
                sourceComponent: videoLayer
                onActiveChanged: {
                    if (!active)
                        stage.videoPlaying = false;
                }
            }

            Component {
                id: videoLayer

                Item {
                    VideoOutput {
                        id: videoOut
                        anchors.fill: parent
                        fillMode: VideoOutput.PreserveAspectFit
                        visible: stage.videoPlaying
                    }

                    // There is no audio output on purpose: the viewer is for looking at a
                    // picture, not for showing a wallpaper. A clip's sound plays only where
                    // it is applied.
                    MediaPlayer {
                        id: videoPlayer
                        source: viewer.videoSource
                        videoOutput: videoOut
                        loops: MediaPlayer.Infinite
                        onMediaStatusChanged: {
                            if (videoPlayer.mediaStatus === MediaPlayer.LoadedMedia
                                    || videoPlayer.mediaStatus === MediaPlayer.BufferedMedia)
                                videoPlayer.play();
                        }
                        onPlaybackStateChanged: stage.videoPlaying =
                            videoPlayer.playbackState === MediaPlayer.PlayingState
                        onErrorOccurred: (err, msg) => {
                            console.warn("REXON-VIEWER ролик не играет: " + err + " " + msg);
                            viewer.videoBroken = true;
                        }
                    }
                }
            }
        }

        // ── Buttons ───────────────────────────────────────────────────────

        RexonButton {
            id: prevButton
            objectName: "imageViewerPrev"
            visible: viewer.count > 1
            anchors.left: parent.left
            anchors.leftMargin: viewer.chromeMargin
            anchors.verticalCenter: parent.verticalCenter
            width: viewer.chromeSize
            implicitHeight: viewer.chromeSize
            pal: viewer.pal
            tone: viewer.chromeTone
            font.pointSize: 16
            text: viewer.arrows.previous
            ToolTip.visible: prevButton.hovered
            ToolTip.text: viewer.arrows.keyHint(qsTranslate("rexon", "Previous (←)"))
            onClicked: {
                viewer.step(-1);
                stage.forceActiveFocus();
            }
        }

        RexonButton {
            id: nextButton
            objectName: "imageViewerNext"
            visible: viewer.count > 1
            anchors.right: parent.right
            anchors.rightMargin: viewer.chromeMargin
            anchors.verticalCenter: parent.verticalCenter
            width: viewer.chromeSize
            implicitHeight: viewer.chromeSize
            pal: viewer.pal
            tone: viewer.chromeTone
            font.pointSize: 16
            text: viewer.arrows.next
            ToolTip.visible: nextButton.hovered
            ToolTip.text: viewer.arrows.keyHint(qsTranslate("rexon", "Next (→)"))
            onClicked: {
                viewer.step(1);
                stage.forceActiveFocus();
            }
        }

        Row {
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: viewer.chromeMargin
            spacing: 8

            RexonButton {
                id: fullScreenButton
                objectName: "imageViewerFullScreen"
                width: viewer.chromeSize
                implicitHeight: viewer.chromeSize
                pal: viewer.pal
                tone: viewer.chromeTone
                font.pointSize: 16
                text: "⛶"
                ToolTip.visible: fullScreenButton.hovered
                ToolTip.text: viewer.fullScreen
                              ? qsTranslate("rexon", "Exit full screen (F)")
                              : qsTranslate("rexon", "Full screen (F)")
                onClicked: {
                    viewer.toggleFullScreen();
                    stage.forceActiveFocus();
                }
            }

            RexonButton {
                id: closeButton
                objectName: "imageViewerClose"
                width: viewer.chromeSize
                implicitHeight: viewer.chromeSize
                pal: viewer.pal
                tone: viewer.chromeTone
                font.pointSize: 13
                text: "❌"
                ToolTip.visible: closeButton.hovered
                ToolTip.text: qsTranslate("rexon", "Close (Esc)")
                onClicked: viewer.close()
            }
        }

        // ── Caption ───────────────────────────────────────────────────────

        Rectangle {
            id: caption
            objectName: "imageViewerCaption"
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: viewer.chromeMargin
            width: Math.min(parent.width - 2 * viewer.chromeMargin,
                            captionColumn.implicitWidth + 32)
            height: captionColumn.implicitHeight + 16
            radius: 8
            color: "#b3000000"
            visible: viewer.current !== null

            Column {
                id: captionColumn
                anchors.centerIn: parent
                spacing: 2

                RexonText {
                    objectName: "imageViewerTitle"
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(implicitWidth, stage.width - 2 * viewer.chromeMargin - 32)
                    text: viewer.current && viewer.current.title ? viewer.current.title : ""
                    visible: text !== ""
                    color: "#ffffff"
                    font.bold: true
                    font.pointSize: 12
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                }
                RexonText {
                    objectName: "imageViewerSubtitle"
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(implicitWidth, stage.width - 2 * viewer.chromeMargin - 32)
                    // The badge (for example, "animated") comes before the second line, in
                    // the same caption.
                    text: {
                        const parts = [];
                        if (viewer.current && viewer.current.badge)
                            parts.push(viewer.current.badge);
                        if (viewer.current && viewer.current.subtitle)
                            parts.push(viewer.current.subtitle);
                        return parts.join("  ·  ");
                    }
                    visible: text !== ""
                    color: "#d9ffffff"
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: 8

                    RexonText {
                        objectName: "imageViewerCounter"
                        visible: viewer.count > 1
                        text: (viewer.index + 1) + " / " + viewer.count
                        color: "#99ffffff"
                    }
                    RexonText {
                        objectName: "imageViewerLoading"
                        visible: viewer.loading
                        text: qsTranslate("rexon", "⏳ Loading full size…")
                        color: "#99ffffff"
                    }
                    RexonText {
                        visible: viewer.zoomed
                        text: Math.round(stage.fitScale * viewer.zoom * 100) + "%"
                        color: "#99ffffff"
                    }
                }
            }
        }
    }
}
