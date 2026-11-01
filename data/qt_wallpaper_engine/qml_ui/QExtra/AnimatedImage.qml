// Миниатюра с анимацией — замена `QExtra.AnimatedImage` на штатных
// элементах Qt Quick.
//
// НАШ файл, не вендорный. С v0.4.4 вендорный `ThumbnailImage.qml`
// показывает превью через QExtra (C++-библиотеку автора waywallen; когда её
// плагин не собран или не годится, ядро берёт эту подмену — см.
// ImageCache.qml рядом). Здесь — тот же набор свойств, который разметка
// ему задаёт и читает.
//
// КАК УСТРОЕНО: два слоя.
//   * `m_still` — штатный `Image`: декодирует в фоновом потоке Qt и умеет
//     держать прежний кадр, пока грузится новый (`retainWhileLoading`).
//     Он показывает картинку и отвечает за `status`.
//   * `m_movie` — штатный `AnimatedImage`: получает источник, только когда
//     `m_still` уже готов и доложил, что кадров больше одного. Пока идёт
//     анимация, неподвижный слой скрыт (иначе сквозь прозрачные места
//     анимации был бы виден её первый кадр).
//
// ПОЧЕМУ НЕ ОДИН `AnimatedImage`. Штатный `AnimatedImage` читает локальный
// файл прямо в присваивании `source`, в потоке интерфейса; его
// `asynchronous` на это не влияет. Замер 11.10.2026 (Qt 6.12, картинка
// 3000x3000): присваивание длится 85–93 мс, и всё это время окно стоит —
// между сменой источника и новым кадром не рисуется ни одного кадра. У
// `Image` то же присваивание занимает 0–1 мс. Неподвижным картинкам (а
// превью обоев в большинстве такие) декодер анимации не нужен вовсе.
//
// Что повторено:
//   * `maxSize` — потолок размера декодирования. Задан `sourceSize` —
//     берётся он, но не крупнее потолка; не задан — размер самого элемента
//     (см. `_wantedSize` ниже), тоже не крупнее потолка. Пропорции
//     сохраняет сам Qt;
//   * `cache` вместе с выключателем `ImageCache.capacityMiB` (0 — без кэша);
//   * `retainWhileLoading` — прежний кадр (и прежняя анимация) остаётся на
//     экране, пока новый источник не декодирован; без него элемент на это
//     время пустеет, как у оригинала;
//   * `playing` переживает смену источника: штатный `AnimatedImage` сам
//     гасит его на неанимированной картинке, поэтому на каждом `Ready`
//     признак ставится заново — как делает родная ветка `ThumbnailImage`.
//
// Что НЕ повторено и почему:
//   * `sharedPlayback` (общий такт у одинаковых анимаций) принимается, но
//     ничего не делает: у штатного декодера Qt такого режима нет. На вид
//     это значит только, что две одинаковые анимации могут идти вразнобой;
//   * у АНИМИРОВАННОГО файла первый кадр анимации декодируется в потоке
//     интерфейса (так устроен штатный `AnimatedImage`) — уже после того,
//     как неподвижный слой показал картинку.
import QtQuick

Item {
    id: root

    property url source
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

    // Потолок по одной оси: 0 и меньше — «не задано».
    function _cap(value, limit) {
        if (limit <= 0)
            return value;
        return value > 0 ? Math.min(value, limit) : limit;
    }

    // Размер декодирования, когда вызывающий не задал `sourceSize` сам:
    // размер самого элемента (то есть карточки), а не потолок `maxSize`.
    //
    // ЗАЧЕМ. Штатный декодер Qt держит кадры анимации в том размере, в каком
    // их декодировал, и потолок апстрима 512x512 для карточки витрины втрое
    // крупнее нужного. Замер 10.10.2026 на живой библиотеке (40 обоев,
    // вкладка открыта, сетка загружена): 1471 МБ с одним потолком против
    // 513 МБ с размером карточки. У апстрима ту же роль играет кэш
    // декодированных кадров в C++-библиотеке QExtra, которой в Rexon нет.
    //
    // До 10.10.2026 это делала правка вендорного `ThumbnailImage.qml`
    // (patches/0001). Здесь, в НАШЕМ модуле, она ничего не стоит при
    // обновлении вендора: чужой файл остаётся как у автора.
    //
    // Ступень 64 пикселя — чтобы плавное изменение размера окна не
    // заставляло декодер переделывать работу на каждый пиксель.
    readonly property int _decodeStep: 64
    function _stepped(value) {
        return value > 0 ? Math.ceil(value / _decodeStep) * _decodeStep : 0;
    }
    readonly property size _wantedSize: (sourceSize.width > 0 || sourceSize.height > 0)
        ? sourceSize : Qt.size(_stepped(root.width), _stepped(root.height))

    readonly property size _decodeSize: Qt.size(_cap(_wantedSize.width, maxSize.width), _cap(_wantedSize.height, maxSize.height))

    // Источник слоя анимации. Меняется только здесь, по состоянию
    // неподвижного слоя: готов и кадров больше одного — анимация этого
    // файла; готов и кадр один — анимации нет. Пока новый файл грузится,
    // прежняя анимация остаётся при `retainWhileLoading` и снимается без
    // него. Сбой загрузки снимает её всегда.
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
        source: root.source
        sourceSize: root._decodeSize
        fillMode: root.fillMode
        horizontalAlignment: root.horizontalAlignment
        verticalAlignment: root.verticalAlignment
        cache: root.cache && ImageCache.capacityMiB > 0
        mipmap: root.mipmap
        mirror: root.mirror
        // Картинка из кэша Qt бывает готова без смены `status` (была
        // готова — и осталась), поэтому слушаются все три признака.
        onStatusChanged: root._syncMovie()
        onSourceChanged: root._syncMovie()
        onFrameCountChanged: root._syncMovie()
        // Свойство есть у `Image` с Qt 6.8. Задано отсюда, а не строкой
        // выше: на более старом Qt неизвестное свойство не дало бы
        // загрузиться всему элементу, а так там просто нет удержания.
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
