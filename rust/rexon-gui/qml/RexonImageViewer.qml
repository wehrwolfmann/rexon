// Крупный просмотр картинки — ОДИН на весь продукт.
//
// Миниатюры есть почти в каждой вкладке (фоны и заставки Steam, расширения,
// библиотека обоев, снимки тем), а открыть картинку крупно можно было не
// везде, и там, где можно, — каждый раз по-своему. Этот слой общий: страница
// отдаёт ему список того, что лежит на карточках, и номер карточки, с
// которой открыть; всё остальное он делает сам.
//
// Что умеет:
//   * картинка во всё окно приложения на затемнённой подложке;
//   * «на весь экран» — кнопкой и клавишей F (окно приложения уходит в
//     полноэкранный режим и возвращается, как было, при выходе);
//   * закрытие — Esc, кнопка, щелчок мимо картинки;
//   * листание по списку — стрелки на экране и клавиши ← →;
//   * масштаб — колесо мыши и клавиши + − 0, сдвиг — перетаскиванием;
//     двойной щелчок переключает «вписать / 1:1»;
//   * подпись: название, вторая строка (игра, источник), отметка
//     (например, «анимированный») и счётчик «3 / 205»;
//   * пока полноразмерная картинка грузится — видна миниатюра и признак
//     загрузки; окно при этом не замирает (всё читается асинхронно);
//   * GIF/APNG/WebP играют (`animatedImage`), ролик (`video`) идёт без звука
//     по кругу поверх статичного кадра.
//
// Как подключить:
//
//     RexonImageViewer { id: viewer; pal: root.pal }
//     ...
//     RexonZoomArea { anchors.fill: parent; pal: root.pal
//                     onActivated: viewer.show(items, index) }
//
// Элемент списка — простой объект:
//     { key, title, subtitle, badge, thumb, source, video,
//       animatedImage, pending }
// `thumb` — то, что уже на карточке (показывается сразу); `source` —
// полноразмерная картинка; `video` — ролик. Если полного адреса у страницы
// ещё нет, она ставит `pending: true`, слушает `wanted(key)` и, когда файл
// готов, кладёт его в `sources`:  { "<key>": { source, video, done } }.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Window
import QtMultimedia

Popup {
    id: viewer

    // ── Договор со страницей ──────────────────────────────────────────────

    /// Палитра скина страницы (акцент и текст берутся из неё).
    property var pal: null
    /// Список карточек (см. шапку файла).
    property var items: []
    /// Номер показанной карточки.
    property int index: 0
    /// Поздние адреса: ключ карточки → { source, video, done }.
    property var sources: ({})
    /// Подложка просмотра. Темнее общего затемнения модальных окон:
    /// картинку смотрят на почти чёрном, а не сквозь страницу.
    property color backdropColor: "#eb000000"

    /// Показана карточка `key` (или её сосед — он подгружается заранее):
    /// странице пора добыть полноразмерный файл, если его ещё нет.
    signal wanted(string key)

    readonly property int count: viewer.items ? viewer.items.length : 0
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

    /// Полноразмерное ещё в пути: страница обещала (`pending`) и не
    /// отчиталась (`done`), либо сам файл ещё читается.
    readonly property bool loading: viewer.opened && viewer.current !== null
        && ((viewer.current.pending === true && !(viewer.late && viewer.late.done === true))
            || (stage.fullItem !== null && stage.fullItem.status === Image.Loading)
            || (viewer.videoSource !== "" && !viewer.videoBroken && !stage.videoPlaying))
    /// Ролик не открылся — остаётся статичный кадр.
    property bool videoBroken: false

    /// Открыть просмотр на карточке `at` списка `list`.
    function show(list, at) {
        viewer.items = list || [];
        if (viewer.count === 0)
            return;
        viewer.index = Math.max(0, Math.min(at | 0, viewer.count - 1));
        viewer.resetView();
        viewer.open();
        viewer.announce();
    }

    /// Листание по кругу.
    function step(delta) {
        if (viewer.count < 2)
            return;
        viewer.index = (viewer.index + delta + viewer.count) % viewer.count;
        viewer.resetView();
        viewer.announce();
    }

    /// Сказать странице, что понадобилось: показанное и оба соседа.
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

    // ── Масштаб и сдвиг ───────────────────────────────────────────────────

    /// Масштаб относительно «вписать»: 1 — картинка целиком в окне.
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

    /// Не дать картинке уехать за край: сдвиг — только на то, что не влезло.
    function clampPan() {
        const overX = Math.max(0, (stage.naturalWidth * stage.fitScale * viewer.zoom - stage.width) / 2);
        const overY = Math.max(0, (stage.naturalHeight * stage.fitScale * viewer.zoom - stage.height) / 2);
        viewer.panX = Math.max(-overX, Math.min(overX, viewer.panX));
        viewer.panY = Math.max(-overY, Math.min(overY, viewer.panY));
    }

    /// Поставить масштаб `value`, удержав под курсором ту же точку картинки.
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

    /// Двойной щелчок: «вписать» ↔ «1:1» (точка картинки в точку экрана).
    function toggleActualSize(px, py) {
        if (viewer.zoomed) {
            viewer.zoom = 1.0;
            viewer.panX = 0;
            viewer.panY = 0;
            return;
        }
        viewer.zoomTo(1 / Math.max(stage.fitScale, 0.01), px, py);
    }

    // ── На весь экран ─────────────────────────────────────────────────────
    //
    // Слой занимает окно приложения целиком, поэтому «на весь экран» — это
    // полноэкранный режим самого окна: панели рабочего стола уходят, а при
    // выходе окно возвращается в тот вид, в каком было (обычное или
    // развёрнутое).
    readonly property var hostWindow: viewer.parent ? viewer.parent.Window.window : null
    readonly property bool fullScreen: viewer.hostWindow !== null
                                       && viewer.hostWindow.visibility === Window.FullScreen
    /// Вид окна до полноэкранного режима; -1 — режим включали не мы.
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

    // ── Сам слой ──────────────────────────────────────────────────────────

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

    // Затемняет сам слой; второе затемнение под ним было бы лишним.
    Overlay.modal: Item {}
    background: Rectangle { color: viewer.backdropColor }

    /// Щелчок, которым просмотр открыли, бывает двойным. Вторая его половина
    /// приходит уже сюда и, попав мимо картинки, закрыла бы просмотр сразу.
    property double openedAt: 0

    onOpened: {
        viewer.openedAt = Date.now();
        stage.forceActiveFocus();
    }
    onClosed: {
        // Включали полноэкранный режим мы — мы и возвращаем окно.
        if (viewer.savedVisibility >= 0)
            viewer.leaveFullScreen();
    }

    // Кнопки поверх картинки: круглые, тёмные, с рамкой акцента под курсором.
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

        /// Полноразмерная картинка (обычная либо анимированная).
        readonly property var fullItem: viewer.fullSource === "" ? null
            : (viewer.animatedImage ? fullAnimated : fullStill)
        readonly property bool fullReady: stage.fullItem !== null
                                          && stage.fullItem.status === Image.Ready
        property bool videoPlaying: false

        // Размер картинки «как есть»: у готовой полноразмерной, иначе у
        // миниатюры, а пока нет ничего — условные 16:9. Меряется ДЕЙСТВИЕМ
        // (`measure`), а не связкой: картинки лежат внутри холста, чей
        // размер от этой мерки и считается, и связка замыкалась бы в круг.
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
                viewer.step(-1);
                break;
            case Qt.Key_Right:
                viewer.step(1);
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

        // Мышь по всему слою: колесо — масштаб, перетаскивание — сдвиг,
        // двойной щелчок по картинке — «вписать / 1:1», щелчок мимо — выход.
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

            // Миниатюра — снизу и сразу: пока полноразмерной нет, видно её,
            // и пустому прямоугольнику мелькнуть негде.
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

            // Ролик — поверх статичного кадра. Проигрыватель поднимается
            // только когда он нужен: мультимедийный слой Qt тяжёлый, и
            // платить за него тот, кто смотрит картинки, не должен.
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

                    // Вывода звука нет намеренно: просмотр — это взгляд на
                    // картинку, а не показ обоев. Звук у ролика — только там,
                    // где его применяют.
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

        // ── Кнопки ────────────────────────────────────────────────────────

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
            text: "❮"
            ToolTip.visible: prevButton.hovered
            ToolTip.text: qsTranslate("rexon", "Previous (←)")
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
            text: "❯"
            ToolTip.visible: nextButton.hovered
            ToolTip.text: qsTranslate("rexon", "Next (→)")
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

        // ── Подпись ───────────────────────────────────────────────────────

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

                Text {
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
                Text {
                    objectName: "imageViewerSubtitle"
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(implicitWidth, stage.width - 2 * viewer.chromeMargin - 32)
                    // Отметка (например, «анимированный») — перед второй
                    // строкой, той же подписью.
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

                    Text {
                        objectName: "imageViewerCounter"
                        visible: viewer.count > 1
                        text: (viewer.index + 1) + " / " + viewer.count
                        color: "#99ffffff"
                    }
                    Text {
                        objectName: "imageViewerLoading"
                        visible: viewer.loading
                        text: qsTranslate("rexon", "⏳ Loading full size…")
                        color: "#99ffffff"
                    }
                    Text {
                        visible: viewer.zoomed
                        text: Math.round(stage.fitScale * viewer.zoom * 100) + "%"
                        color: "#99ffffff"
                    }
                }
            }
        }
    }
}
