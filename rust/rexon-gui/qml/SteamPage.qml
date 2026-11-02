// Вкладка «Steam» на QML. Данные и решения — Rust-объект SteamModel
// (rust/rexon-gui/src/steam_page.rs), который сам ничего не решает, а
// спрашивает ядро (`rexon_core::qt_steam`). Здесь только РАЗМЕТКА.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из скина по умолчанию всего продукта
// (правило #17). Своих цветов страница не заводит — специфичные для вкладки
// краски (акцент, «применено», «в избранном», чип состояния, бейдж
// карточки) тоже приезжают из ядра готовыми строками.
//
// Побочные действия живут снаружи: хозяин страницы кладёт в `root.actions`
// объект с методами
//   scan()            -> {movies, current, favorites, enabled, rotate, sound}
//   pickFile()        -> string (окно выбора своего видео)
//   select(path)      -> bool   (сохранить выбор в конфиг заставки)
//   setFavorite(path) -> список избранного после переключения
//   setSound(on)      -> bool
//   setRotation(on)   -> bool
//   setAutostart(on)  -> string («» — получилось, иначе текст ошибки)
//   play(path, sound) -> string («» — пошло, иначе текст ошибки)
// Хозяев у страницы двое, и договор у них ОДИН — потому она и не менялась:
//   * живой продукт кладёт Python-объект `SteamActions`
//     (rexon/qt_steam/qml_window.py) поверх rexon/session_splash.py и
//     добычи токена Steam (архив, метка git python-final);
//   * окно на Rust (`qml/MainWindow.qml`) кладёт `SteamActions` из
//     rust/rexon-gui/src/steam_actions.rs — те же восемь имён, те же ответы,
//     а внутри ядро: `session_splash` + `splash_exec` + `steam_token`.
//
// Цепочка фокуса заложена сразу: все действия — на контролах, у
// сфокусированного видна рамка, страница подкручивается к нему.
//
// ── Живое превью ролика в крупной карточке ───────────────────────────────
//
// Карточка показывает не кадр, а сам ролик — как ролик товара в магазине.
// В виджетном оригинале (rexon/qt_steam/window.py) это связка QMediaPlayer +
// QVideoSink, у которой кадры забирались вручную и рисовались QLabel: там
// пришлось обойти живой дефект QVideoWidget (своё нативное окно ложилось
// поперёк вкладки) и вручную же прореживать кадры до 15 к/с, потому что
// каждый разворот кадра в QImage стоил ~8 МБ.
//
// Здесь ни того, ни другого не нужно: `VideoOutput` — обычный узел графа
// сцены, своего окна не заводит и кадр в оперативную память не разворачивает
// вовсе. Поэтому оба обхода сняты (правило #16), а ПОВЕДЕНИЕ повторено
// один в один:
//   * играет ровно тот ролик, что показывает карточка (наведённый, а без
//     наведения — выбранный);
//   * звука нет вовсе: `audioOutput` не задан — ролик, заоравший при
//     переключении вкладки, человек не заказывал;
//   * до первого кадра и после остановки видно статичный кадр: он лежит ПОД
//     видео и никуда не девается, поэтому чёрному прямоугольнику мелькнуть
//     негде;
//   * стоим всегда, когда смотреть некому — чужая вкладка, чужая подвкладка,
//     свёрнутое окно, полноэкранный показ. Это не мелочь, а батарея.
//
// Решение «что играет сейчас» целиком в Rust (`previewSource`); разметка
// сообщает ему ровно один факт — видно ли карточку человеку.
//
// ⚠️ Сам проигрыватель поднимается ЛЕНИВО, за `Loader`: мультимедийный слой
// Qt стоит ~117 МБ, и человек, зашедший посмотреть фоны профиля, платить за
// него не должен. В оригинале это делал `_sv_ensure_player`.

// Делегаты обращаются к id внешних элементов (root, page). Без этой прагмы
// проверка QML справедливо ругается на неквалифицированный доступ.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
// Окно верхнего уровня (свёрнуто / спрятано) и проигрыватель превью.
import QtQuick.Window
import QtMultimedia
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    property var actions: null

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    // Наружу — ради самопроверки: хозяин сверяет, что страница знает
    // тот же корень Steam, который находит объект действий. Пустой
    // путь при найденном корне значит, что секция фонов профиля
    // останется пустой, а по виду вкладки этого не заметить.
    readonly property string steamFoundPath: page.headerFoundPath

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    SteamModel {
        id: page
        // Имя нужно проверкам: модель достают
        // `root.findChild(QObject, "steamModel")` и спрашивают её без
        // единой правки в самой странице.
        objectName: "steamModel"

        // Сообщение приходит свойством, а показ — ДЕЙСТВИЕ: плашка, привязанная
        // к свойству, гаснуть не умеет по определению. Ровно этим и висело
        // сообщение вечно, пока здесь стояло `visible: statusText !== ""`.
        onStatusChanged: statusToast.show(page.statusText)
    }

    // ── Палитра стандартного скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    // Готовая краска карточки от ядра — та же накладка, что в таблице
    // стилей виджетной витрины (`qt_steam::card_colors`). См. ComparePage.
    readonly property color colCard: root.pal.surface_card
    readonly property color colBorder: root.pal.border
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // Поле страницы. Прежде стояло числом в `anchors.margins`; теперь его
    // знает и область прокрутки, которая считает по нему ширину и высоту
    // содержимого, — два числа в двух местах разъехались бы молча.
    readonly property int pageMargin: 24

    // ── Оформление вкладки, которого нет в палитре скина ──────────────────
    //
    // Группа-секция, карточка витрины, врезка-тезис, главная и зелёная
    // кнопки, чип-переключатель, подложки миниатюр. Всё приходит готовым от
    // ядра — тем же набором, которым красится виджетная вкладка.
    readonly property var st: page.styles
    readonly property var toneChip: ({
        "background": "transparent",
        "border": root.st.chip.border,
        "hover": root.st.chip.hoverBackground,
        "radius": root.st.chip.radius
    })
    readonly property var toneChipOn: ({
        "background": root.st.chip.checkedBackground,
        "border": root.st.chip.checkedBorder,
        "hover": root.st.chip.checkedBackground,
        "radius": root.st.chip.radius
    })

    // ── Шов с фоновыми задачами ───────────────────────────────────────────
    //
    // Задачи tokio складывают готовое в очередь событий; страница забирает
    // её своим таймером. Это ЕДИНСТВЕННОЕ место, где данные пересекают
    // границу потоков, — сигналов из фонового потока в QML нет вовсе.
    Timer {
        id: pump
        interval: 120
        repeat: true
        running: true
        onTriggered: page.pump()
    }

    // ── Живое превью: видно ли карточку человеку ──────────────────────────
    //
    // Опрос раз в секунду — тот же приём и тот же период, что в оригинале.
    // Сигнала «поверхность спрятали» в Qt нет вовсе, а фильтр событий на
    // главном окне обошёлся бы дороже: через него идёт КАЖДОЕ движение мыши
    // по всей программе.
    readonly property var hostWindow: root.Window.window
    /// Композитор показывает наше окно (ответ `QWindow::isExposed()`).
    property bool windowExposed: true
    /// Проигрыватель хоть раз понадобился.
    ///
    /// ⚠️ Мультимедийный слой Qt (FFmpeg с кодеками) стоит ~117 МБ и
    /// поднимается уже от одного созданного `MediaPlayer`. Платить за него
    /// тот, кто зашёл посмотреть фоны профиля, не должен — в оригинале
    /// плеер по той же причине строился лениво (`_sv_ensure_player`).
    /// Обратно латч не гаснет: поднятый слой уже в памяти, а пересоздавать
    /// плеер на каждый увод курсора — работа впустую.
    property bool previewNeeded: false

    Binding {
        target: root
        property: "previewNeeded"
        value: true
        when: page.previewSource !== ""
        // Обратно НЕ откатывать: латч на то и латч.
        restoreMode: Binding.RestoreNone
    }

    Timer {
        id: previewWatch
        interval: 1000
        repeat: true
        // Страницу спрятали — опрашивать нечего: остановку превью делает
        // `onVisibleChanged`, а будить процессор раз в секунду ради заведомо
        // отрицательного ответа незачем (в оригинале тот же таймер
        // останавливался в `hideEvent`).
        running: root.visible
        onTriggered: root.syncPreview()
    }

    // Возврат фокуса в окно — единственное, что приходит сигналом: им же
    // снимается пауза после полноэкранного показа.
    Connections {
        target: root.hostWindow
        function onActiveChanged() {
            if (root.hostWindow && root.hostWindow.active)
                page.resumePreview();
            root.syncPreview();
        }
    }

    function syncPreview() {
        root.windowExposed = root.hostWindow ? page.windowExposed(root.hostWindow) : true;
        page.setPreviewShown(root.previewAllowed());
        page.previewTick();
    }

    /// Смотрит ли кто-нибудь на крупную карточку прямо сейчас.
    ///
    /// ⚠️ Свёрнутость окна на Wayland по `visibility` НЕ определяется:
    /// протокол xdg-shell состояния «свёрнуто» клиенту не сообщает, и после
    /// сворачивания окно у KWin остаётся `Window.Windowed`. Поэтому к
    /// проверке видимости добавлен честный признак от ядра —
    /// `windowExposed`. На X11 работает и то, и другое.
    function previewAllowed() {
        if (!root.visible || !heroBox.visible)
            return false;
        // Открыт крупный просмотр — карточку под ним не видно, а двух копий
        // одного ролика разом быть не должно.
        if (viewer.opened)
            return false;
        const win = root.hostWindow;
        if (win && (!win.visible
                    || win.visibility === Window.Minimized
                    || win.visibility === Window.Hidden))
            return false;
        if (!root.windowExposed)
            return false;
        // Карточку прокрутили за край полосы — смотреть на неё некому.
        // Оригинал этого не умел вовсе и крутил ролик прокрученной за верх
        // окна карточке (правило #16). Задержка — до секунды, ровно та же,
        // что у остальных проверок обстановки.
        if (splashScroll.height > 0) {
            const top = heroBox.mapToItem(splashScroll, 0, 0).y;
            if (top + heroBox.height <= 0 || top >= splashScroll.height)
                return false;
        }
        return true;
    }

    // Ушли на другую вкладку окна — плеер стоп; вернулись — играет снова и
    // страница снова управляема с клавиатуры.
    onVisibleChanged: {
        if (root.visible)
            root.focusFirstControl();
        root.syncPreview();
    }

    // Курсор задержался на карточке — её и показываем крупно (как ролик
    // товара в магазине). Задержку задаёт ядро.
    Timer {
        id: hoverSettle
        interval: page.svHoverDelayMs
        repeat: false
        onTriggered: page.setHovered(root.pendingHover)
    }
    property string pendingHover: ""

    function hoverLater(path) {
        root.pendingHover = path;
        hoverSettle.restart();
    }

    // ── Крупный просмотр ──────────────────────────────────────────────────
    //
    // Слой общий на весь продукт (`RexonImageViewer.qml`); вкладка отдаёт
    // ему список и номер карточки. Листание идёт по ВСЕМУ отобранному
    // списку, а не по одной странице сетки.

    function openViewer(items, key) {
        if (!items || items.length === 0)
            return;
        let at = 0;
        for (let i = 0; i < items.length; i++) {
            if (items[i].key === key) {
                at = i;
                break;
            }
        }
        viewer.show(items, at);
    }

    function openBackgroundViewer(id) {
        root.openViewer(page.bgViewerItems(), id);
    }

    function openMovieViewer(path) {
        root.openViewer(page.svViewerItems(), path);
    }

    // ── Побочные действия ─────────────────────────────────────────────────

    function reload() {
        if (!root.actions)
            return;
        const snapshot = root.actions.scan();
        if (!snapshot)
            return;
        page.setMovies(snapshot.movies, snapshot.current, snapshot.favorites);
        page.setSplashState(snapshot.enabled, snapshot.rotate, snapshot.sound);
    }

    function pickOwnFile() {
        if (!root.actions)
            return;
        const path = root.actions.pickFile();
        if (!path)
            return;
        root.actions.select(path);
        root.reload();
    }

    function toggleStar(path) {
        if (!root.actions || !path)
            return;
        page.setFavorites(root.actions.setFavorite(path));
    }

    function playFullscreen() {
        if (!root.actions)
            return;
        const path = page.shownPath();
        if (!path) {
            page.setStatus(page.svTestNoItemText);
            return;
        }
        const err = root.actions.play(path, page.soundOn);
        if (!err) {
            // Ролик пошёл на весь экран — превью в карточке замирает: двух
            // копий одного видео разом быть не должно. Снимется само, как
            // только окно снова получит фокус; запас по времени держит ядро
            // — на случай, если фокус не вернётся вовсе.
            page.pausePreview();
        }
        // Итог показа раньше пропадал молча: получилось — не видно, не
        // получилось — тоже (правило #16). Оригинал писал его строкой под
        // крупной карточкой, здесь он идёт в общую строку состояния.
        page.setStatus(err ? err : page.svTestPlayingText);
    }

    Component.onCompleted: {
        root.reload();
        // Токен владеемых фонов страница не носит: его добывает фоновая
        // задача модели (поход в сеть — не дело потока интерфейса).
        page.start("");
        root.focusFirstControl();
        root.syncPreview();
    }

    Component.onDestruction: page.shutdown()

    // Фокус на первом кадре берёт ПЕРВЫЙ УПРАВЛЯЕМЫЙ ЭЛЕМЕНТ, а не корень
    // страницы: `forceActiveFocus()` на корне делает activeFocusItem
    // безымянным Item, и страница мертва и с клавиатуры, и с геймпада.
    function focusFirstControl() {
        if (segments.count > 0 && segments.itemAt(0)) {
            segments.itemAt(0).forceActiveFocus();
            return;
        }
        Qt.callLater(root.focusFirstControl);
    }

    // ── Разметка ──────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Страница целиком — в области прокрутки ────────────────────────────
    //
    // ⚠️ Так же устроен оригинал: вся страница лежит в `QScrollArea`
    // (`rexon/qt_steam/window.py:797`), и обе секции внутри — в своих
    // (там же, 842 и 850). Внутренние две здесь были, а внешней не было, и
    // страница теряла всё, что не влезло в окно:
    //
    //   * длинный путь к Steam в шапке не сжимается и не обрезается (у
    //     оригинала это один `QLabel` с `<tt>`), поэтому он делает страницу
    //     ШИРЕ окна — и правый край секции вместе с кнопкой «Свой файл…»
    //     уезжал за край безвозвратно;
    //   * на собственном минимуме главного окна (1100×750) под страницу
    //     остаётся меньше высоты, чем её собственный минимум, и низ секции
    //     вместе с нижним полем срезало.
    //
    // Теперь и то и другое достаётся прокруткой, а пока всё помещается —
    // полос не видно и страница выглядит ровно как прежде.
    //
    // ⛔ Прокрутке подлежит ТОЛЬКО содержимое страницы. Плашка состояния
    // (`RexonToast` ниже) остаётся снаружи: она накладка ПОВЕРХ страницы, и
    // внутри полосы ездила бы вместе с содержимым и уезжала за край.
    ScrollView {
        id: pageScroll
        // Имя — для стендовых прогонов: полосу достают
        // `root.findChild(QObject, "pageScroll")` и прокручивают, чтобы
        // снять то, что за краем окна, как это делает рукой человек. Тем же
        // приёмом стенды достают и саму модель (`steamModel` выше).
        objectName: "pageScroll"
        anchors.fill: parent
        clip: true
        // Ширину задаёт самое несжимаемое, что есть на странице, — строка
        // шапки с путём. Брать `implicitWidth` всей колонки нельзя: у
        // переносимых подписей секции он равен длине текста В ОДНУ СТРОКУ,
        // и страница оказалась бы широченной всегда.
        //
        // ⚠️ Мерка и есть круг: `contentWidth` спрашивает ширину у шапки и
        // сегментов, а ширину им через `pageColumn` назначает он же. Значения
        // сходятся с первого раза (`Math.max` только растёт, путь от ширины
        // окна не зависит), но обычная связка этого знать не может: пока она
        // считается, чтение `implicitWidth` дёргает пересчёт раскладки, тот
        // лезет за `contentWidth` обратно — и QML справедливо печатает
        // «Binding loop detected» по 32 строки на каждый вход на вкладку.
        //
        // `delayed` — штатный на это ответ: значение считается один раз за
        // оборот событий, уже после того как раскладка улеглась, и круга не
        // возникает вовсе. Проверено на отдельном стенде: 0 предупреждений
        // против 32, ширина содержимого та же и при узком окне (длинный путь
        // так же достаётся прокруткой), и при растяжении окна она следует за
        // ним. Снимать `delayed`, не сняв самой мерки, нельзя — вернётся.
        //
        // ⚠️ Нижняя граница — `availableWidth`, а не `width`. Вертикальная
        // полоса (она видна всегда, когда окно ниже минимума вкладки)
        // отъедает место у области, и содержимое шириной во всю `width`
        // оказывалось шире видимого на толщину полосы: внизу висела
        // постоянная горизонтальная прокрутка ни на что (снимки 01.10.2026).
        Binding on contentWidth {
            delayed: true
            value: Math.max(pageScroll.availableWidth,
                            pageSegments.implicitWidth + root.pageMargin * 2)
        }
        // Ниже собственного минимума вкладки страница не ужимается — как у
        // оригинала, где его держит `minimumSizeHint` содержимого.
        //
        // ⚠️ Тоже отложенно, и по той же причине, но круг здесь чужой: в стиле
        // KDE (`org/kde/desktop/ScrollView.qml:20`) поля области зависят от
        // того, видна ли полоса прокрутки, а видна она или нет — решается
        // сравнением содержимого с доступным местом, то есть с теми же полями.
        // Живьём (стиль KDE, в отличие от стенда на Basic) это давало
        // «Binding loop … implicitHeight» даже после развязки по ширине.
        Binding on contentHeight {
            delayed: true
            value: Math.max(pageScroll.height, page.windowMinHeight)
        }

        ColumnLayout {
            id: pageColumn
            x: root.pageMargin
            y: root.pageMargin
            width: pageScroll.contentWidth - root.pageMargin * 2
            height: pageScroll.contentHeight - root.pageMargin * 2
            spacing: 14

            // ── Шапка ─────────────────────────────────────────────────────────
            RowLayout {
                id: pageHeader
                Layout.fillWidth: true
                spacing: 12

                Text {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }
                Item { Layout.fillWidth: true }
                // «Найден: » и путь — одна подпись в две части: у оригинала это
                // один QLabel с `<tt>` вокруг пути, поэтому ни жирности, ни
                // зазора тут быть не должно.
                //
                // ⚠️ Путь, в отличие от оригинала, СОКРАЩАЕТСЯ: не влез —
                // многоточие в середине, полный путь во всплывающей подсказке.
                // Несжимаемый путь делал страницу шире окна (снимки живой
                // проверки r1434, 16.10.2026: при длинном пути «Повторить»
                // уходила за правый край, описания обрезались, внизу висела
                // горизонтальная прокрутка). Поэтому и в мерку ширины
                // страницы (`contentWidth` выше) шапка больше не входит.
                RowLayout {
                    // Без зазора между «Найден: » и путём — как у оригинала.
                    spacing: 0
                    // Сжимается до нуля, растёт не дальше своей длины: всё
                    // лишнее место забирает распорка слева, и путь остаётся
                    // прижатым к правому краю, как прежде.
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.maximumWidth: implicitWidth

                    Text {
                        text: page.headerFoundText
                        color: root.colTextSecondary
                    }
                    Text {
                        id: headerPath
                        objectName: "headerFoundPath"
                        visible: page.headerFoundPath !== ""
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.maximumWidth: implicitWidth
                        text: page.headerFoundPath
                        color: root.colTextSecondary
                        font.family: "monospace"
                        elide: Text.ElideMiddle

                        HoverHandler { id: headerPathHover }
                        ToolTip.text: page.headerFoundPath
                        ToolTip.visible: headerPath.truncated && headerPathHover.hovered
                        ToolTip.delay: 500
                    }
                }
            }

            // ── Сегментный переключатель подвкладок ───────────────────────────
            RowLayout {
                id: pageSegments
                Layout.fillWidth: true
                spacing: 4

                Repeater {
                    id: segments
                    model: page.segments
                    delegate: RexonButton {
                        required property var modelData
                        pal: root.pal
                        tone: root.toneChip
                        checkedTone: root.toneChipOn
                        text: modelData.label
                        checkable: true
                        checked: modelData.checked
                        onClicked: page.setSection(modelData.key)
                    }
                }
                Item { Layout.fillWidth: true }
            }

            // Переключение прячет неактивную секцию ЦЕЛИКОМ — ровно тот приём,
            // которым в виджетной вкладке чинился фриз до ~2 с на переключении.
            StackLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: page.section === "bg" ? 1 : page.section === "showcase" ? 2 : 0

                // ══ Секция «🎬 Видеозаставка» ═════════════════════════════════
                ScrollView {
                    id: splashScroll
                    clip: true
                    contentWidth: availableWidth

                    // Группа-секция: та же карточка с рамкой, что у виджетной
                    // вкладки (steamSplashGroup). Краски и поля — от ядра.
                    Rectangle {
                        width: parent.width
                        // ⛔ Высоту содержимого область прокрутки берёт из
                        // `implicitHeight` единственного ребёнка, а не из
                        // `height`. Без неё `contentHeight` оставался -1, полоса
                        // считала содержимое равным окну: сетка уходила за
                        // нижний край без ползунка, колесо не крутило
                        // (живьём r1439, 37 анимированных фонов, 16.10.2026).
                        implicitHeight: splashCol.implicitHeight + root.st.group.marginV * 2
                        // Группа занимает полосу целиком, даже когда содержимого
                        // меньше: у оригинала она — виджет области прокрутки с
                        // `setWidgetResizable(True)` и растяжкой в конце.
                        height: Math.max(implicitHeight, splashScroll.availableHeight)
                        color: root.st.group.background
                        border.color: root.st.group.border
                        border.width: 1
                        radius: root.st.group.radius

                        ColumnLayout {
                            id: splashCol
                            anchors.fill: parent
                            anchors.leftMargin: root.st.group.marginH
                            anchors.rightMargin: root.st.group.marginH
                            anchors.topMargin: root.st.group.marginV
                            anchors.bottomMargin: root.st.group.marginV
                            spacing: root.st.group.spacing

                            Text {
                                Layout.fillWidth: true
                                text: page.splashGroupTitle
                                color: root.colText
                                font.pointSize: page.groupTitlePointSize
                                font.bold: true
                            }

                            // Главный тезис — врезка акцентом.
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: pitch.implicitHeight
                                                + root.st.pitch.marginV * 2
                                color: root.st.pitch.background
                                radius: root.st.pitch.radius
                                border.width: 0
                                Rectangle {
                                    width: root.st.pitch.barWidth
                                    height: parent.height
                                    color: root.st.pitch.bar
                                }
                                Text {
                                    id: pitch
                                    anchors.fill: parent
                                    anchors.margins: root.st.pitch.marginV
                                    anchors.leftMargin: root.st.pitch.marginH
                                    anchors.rightMargin: root.st.pitch.marginH
                                    text: page.splashPitch
                                    color: root.colText
                                    wrapMode: Text.WordWrap
                                }
                            }

                            // ── Крупная карточка выбранного ролика ────────────────
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: heroRow.implicitHeight + 28
                                color: root.st.card.background
                                radius: root.st.card.radius
                                border.color: heroHover.hovered ? root.st.card.hover
                                                                : root.st.card.border
                                border.width: 1
                                HoverHandler { id: heroHover }

                                RowLayout {
                                    id: heroRow
                                    anchors.fill: parent
                                    anchors.margins: 14
                                    spacing: 16

                                    Rectangle {
                                        id: heroBox
                                        Layout.alignment: Qt.AlignTop
                                        implicitWidth: page.svHeroWidth
                                        implicitHeight: page.svHeroHeight
                                        radius: root.st.thumb.heroRadius
                                        color: root.st.thumb.heroBackground
                                        clip: true

                                        // Статичный кадр лежит СНИЗУ и не убирается
                                        // никогда. Пока первого кадра ролика нет —
                                        // видно его; пошли кадры — видео закрывает
                                        // его собой; плеер встал — оно снова на
                                        // виду. Ни переключателя, ни состояния «что
                                        // сейчас показано» для этого не нужно, а
                                        // чёрному прямоугольнику (файл открывается
                                        // ~0,2 с) мелькнуть негде.
                                        Image {
                                            anchors.fill: parent
                                            source: page.hero.poster
                                            fillMode: Image.PreserveAspectFit
                                            asynchronous: true
                                            visible: source !== ""
                                        }
                                        Text {
                                            anchors.centerIn: parent
                                            visible: page.hero.poster === ""
                                            text: page.svThumbPlaceholder
                                            font.pixelSize: 40
                                        }

                                        // Плеер появляется только когда впервые
                                        // понадобился (см. `root.previewNeeded`).
                                        Loader {
                                            anchors.fill: parent
                                            active: root.previewNeeded
                                            sourceComponent: previewPlayer
                                        }

                                        Component {
                                            id: previewPlayer

                                            Item {
                                                VideoOutput {
                                                    id: previewOut
                                                    anchors.fill: parent
                                                    fillMode: VideoOutput.PreserveAspectFit
                                                    // Плеер встал (стоп, пауза, конец
                                                    // файла) — обратно на статичный
                                                    // кадр. Порт `_sv_on_state`
                                                    // оригинала; заодно страховка от
                                                    // пустого вывода, пока первого
                                                    // кадра ещё нет.
                                                    visible: preview.playbackState
                                                             === MediaPlayer.PlayingState
                                                }

                                                // ⚠️ Вывода звука НЕТ вовсе, и
                                                // `audioOutput` не задан намеренно: у
                                                // `MediaPlayer` без назначенного
                                                // вывода устройство не открывается
                                                // совсем, а с назначенным и
                                                // заглушённым поток висит в звуковой
                                                // карте весь сеанс. Звук у заставки —
                                                // только на полноэкранном показе и при
                                                // самом входе.
                                                MediaPlayer {
                                                    id: preview
                                                    // Что играть, решает ядро:
                                                    // наведение, подвкладка, видимость,
                                                    // пауза полноэкранного показа и
                                                    // битые ролики уже учтены — здесь
                                                    // просто источник.
                                                    source: page.previewSource
                                                    videoOutput: previewOut
                                                    loops: MediaPlayer.Infinite

                                                    onSourceChanged: {
                                                        // Играть больше нечего —
                                                        // остановиться явно, а не ждать
                                                        // милости плеера: остановка
                                                        // заодно гасит последний кадр,
                                                        // и снова виден статичный.
                                                        if (String(preview.source) === "")
                                                            preview.stop();
                                                    }
                                                    onMediaStatusChanged: {
                                                        if (preview.mediaStatus === MediaPlayer.LoadedMedia
                                                                || preview.mediaStatus === MediaPlayer.BufferedMedia)
                                                            preview.play();
                                                    }
                                                    onErrorOccurred: (err, msg) => {
                                                        // Ролик не читается (битый
                                                        // файл, незнакомый кодек своего
                                                        // видео) — в карточке остаётся
                                                        // статичный кадр, и этим файлом
                                                        // плеер больше не дёргаем:
                                                        // иначе секундный опрос
                                                        // открывал бы его снова и
                                                        // снова.
                                                        console.warn("REXON-STEAM превью не играет: "
                                                                     + err + " " + msg);
                                                        page.markPreviewBroken();
                                                    }
                                                }
                                            }
                                        }

                                        // Щелчок по карточке — тот же ролик
                                        // во всё окно (без звука; со звуком
                                        // и как при входе — кнопка рядом).
                                        RexonZoomArea {
                                            anchors.fill: parent
                                            pal: root.pal
                                            radius: root.st.thumb.heroRadius
                                            enabled: page.hero.present
                                            visible: enabled
                                            onActivated: root.openMovieViewer(page.shownPath())
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 7

                                        Text {
                                            Layout.fillWidth: true
                                            text: page.hero.title
                                            color: root.colText
                                            font.pointSize: 17
                                            font.bold: true
                                            wrapMode: Text.WordWrap
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            visible: page.hero.present
                                            text: page.hero.source
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            visible: page.hero.labels !== ""
                                            text: page.hero.labels
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            visible: page.hero.event !== ""
                                            text: page.hero.event
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        Item { Layout.fillHeight: true }

                                        RowLayout {
                                            spacing: 10
                                            RexonButton {
                                                pal: root.pal
                                                tone: root.st.primaryButton
                                                text: page.svTestText
                                                ToolTip.text: page.svTestTooltip
                                                ToolTip.visible: hovered
                                                onClicked: root.playFullscreen()
                                            }
                                            RexonButton {
                                                pal: root.pal
                                                visible: page.hero.present
                                                text: page.hero.starText
                                                ToolTip.text: page.svStarTooltip
                                                ToolTip.visible: hovered
                                                onClicked: root.toggleStar(page.shownPath())
                                            }
                                            Item { Layout.fillWidth: true }
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            visible: page.hero.status !== ""
                                            text: page.hero.status
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                    }
                                }
                            }

                            // ── Чип итогового состояния ───────────────────────────
                            Rectangle {
                                implicitWidth: chipText.implicitWidth + 24
                                implicitHeight: chipText.implicitHeight + 10
                                radius: 12
                                color: page.stateChip.background
                                border.color: page.stateChip.border
                                border.width: 1
                                Text {
                                    id: chipText
                                    anchors.centerIn: parent
                                    text: page.stateChip.text
                                    color: page.stateChip.color
                                    font.bold: true
                                }
                            }

                            // ── Переключатели ─────────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 14

                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svAutostartText
                                    checked: page.autostartOn
                                    ToolTip.text: page.svAutostartTooltip
                                    ToolTip.visible: hovered
                                    onToggled: {
                                        // Отказ раньше пропадал молча: галочка
                                        // отскакивала на reload(), и всё. Текст
                                        // для покупателя собирает ядро.
                                        if (root.actions)
                                            page.autostartAnswered(checked, root.actions.setAutostart(checked));
                                        root.reload();
                                    }
                                }
                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svRotateText
                                    checked: page.rotateOn
                                    ToolTip.text: page.svRotateTooltip
                                    ToolTip.visible: hovered
                                    onToggled: {
                                        if (root.actions)
                                            root.actions.setRotation(checked);
                                        root.reload();
                                        // После reload(): порог сверяется с
                                        // тем, что на витрине сейчас.
                                        page.rotationToggled(checked);
                                    }
                                }
                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svSoundText
                                    checked: page.soundOn
                                    onToggled: {
                                        if (root.actions)
                                            root.actions.setSound(checked);
                                        root.reload();
                                    }
                                }
                                Item { Layout.fillWidth: true }
                            }

                            // ── Поиск и свой файл ─────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 10

                                RexonTextField {
                                    Layout.fillWidth: true
                                    pal: root.pal
                                    placeholderText: page.svSearchPlaceholder
                                    text: page.svSearchText
                                    onTextEdited: page.setSvSearch(text)
                                }
                                RexonButton {
                                    pal: root.pal
                                    text: page.svPickText
                                    ToolTip.text: page.svPickTooltip
                                    ToolTip.visible: hovered
                                    onClicked: root.pickOwnFile()
                                }
                            }

                            // ── Чипы фильтра «откуда заставка» ────────────────────
                            // Пустых кнопок не бывает: строки нет вовсе, когда
                            // выбора не существует (решает ядро).
                            Flow {
                                Layout.fillWidth: true
                                spacing: 4
                                visible: page.svChips.length > 0

                                Repeater {
                                    model: page.svChips
                                    delegate: RexonButton {
                                        required property var modelData
                                        pal: root.pal
                                        tone: root.toneChip
                                        checkedTone: root.toneChipOn
                                        text: modelData.label
                                        checkable: true
                                        checked: modelData.checked
                                        onClicked: page.setSvGroup(modelData.key)
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                text: page.svCountText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // ── Сетка карточек витрины ────────────────────────────
                            GridView {
                                id: showcase
                                Layout.fillWidth: true
                                Layout.preferredHeight: Math.max(cellHeight,
                                                                 cellHeight * Math.ceil(count / columns))
                                interactive: false
                                // Столбцы — сколько карточек влезает, остаток
                                // ширины делится между ними, как у сетки
                                // «Оформления»: без пустой полосы справа.
                                readonly property int columns: Math.max(1, Math.floor(
                                    width / (page.svCardWidth + root.st.card.gap)))
                                cellWidth: Math.max(page.svCardWidth + root.st.card.gap,
                                                    Math.floor(width / columns))
                                cellHeight: page.svThumbHeight + 78
                                model: page

                                delegate: Item {
                                    id: card
                                    required property string path
                                    required property string title
                                    required property string badge
                                    required property string badgeBackground
                                    required property string badgeColor
                                    required property string poster
                                    required property string starGlyph
                                    required property string starColor
                                    required property bool selected

                                    width: showcase.cellWidth
                                    height: showcase.cellHeight

                                    Rectangle {
                                        anchors.fill: parent
                                        anchors.margins: root.st.card.gap / 2
                                        radius: root.st.card.radius
                                        color: card.selected ? root.st.card.selectedBackground
                                                             : root.st.card.background
                                        border.width: 1
                                        border.color: card.selected
                                                      ? (hover.hovered ? root.st.card.selectedHover
                                                                       : root.st.card.selectedBorder)
                                                      : (hover.hovered ? root.st.card.hover
                                                                       : root.st.card.border)

                                        HoverHandler {
                                            id: hover
                                            onHoveredChanged: root.hoverLater(hover.hovered ? card.path : "")
                                        }
                                        TapHandler {
                                            onTapped: {
                                                if (root.actions)
                                                    root.actions.select(card.path);
                                                page.selectMovie(card.path);
                                            }
                                        }

                                        ColumnLayout {
                                            anchors.fill: parent
                                            anchors.margins: root.st.card.margin
                                            spacing: root.st.card.spacing

                                            Rectangle {
                                                Layout.alignment: Qt.AlignHCenter
                                                implicitWidth: page.svThumbWidth
                                                implicitHeight: page.svThumbHeight
                                                radius: root.st.thumb.radius
                                                color: root.st.thumb.svBackground
                                                // Кадр ролика ВПИСЫВАЕТСЯ целиком,
                                                // а не обрезается: у оригинала
                                                // миниатюра читается
                                                // `load_thumbnail` без `expand`,
                                                // и квадратный постер стоит в
                                                // середине с полями по бокам.
                                                Image {
                                                    anchors.fill: parent
                                                    source: card.poster
                                                    fillMode: Image.PreserveAspectFit
                                                    asynchronous: true
                                                    visible: source !== ""
                                                }
                                                Text {
                                                    anchors.centerIn: parent
                                                    visible: card.poster === ""
                                                    text: page.svThumbPlaceholder
                                                    font.pixelSize: 22
                                                }
                                                // Щелчок по карточке занят
                                                // выбором ролика, поэтому
                                                // просмотр открывает лупа.
                                                RexonZoomArea {
                                                    anchors.fill: parent
                                                    pal: root.pal
                                                    badgeOnly: true
                                                    radius: root.st.thumb.radius
                                                    onActivated: root.openMovieViewer(card.path)
                                                }
                                            }

                                            RowLayout {
                                                Layout.fillWidth: true
                                                spacing: 4
                                                Text {
                                                    // Ширину обрезки задаёт ядро
                                                    // (у оригинала — по метрике
                                                    // шрифта, а не по остатку
                                                    // строки): иначе длинное имя
                                                    // раздвигало бы звёздочку.
                                                    Layout.fillWidth: true
                                                    Layout.maximumWidth: root.st.svTitleElideWidth
                                                    text: card.title
                                                    color: root.colText
                                                    elide: Text.ElideRight
                                                    ToolTip.text: card.title
                                                    ToolTip.visible: nameHover.hovered
                                                    HoverHandler { id: nameHover }
                                                }
                                                Text {
                                                    text: card.starGlyph
                                                    color: card.starColor
                                                    font.pixelSize: 15
                                                    ToolTip.text: page.svCardStarTooltip
                                                    ToolTip.visible: starHover.hovered
                                                    HoverHandler { id: starHover }
                                                    TapHandler {
                                                        onTapped: root.toggleStar(card.path)
                                                    }
                                                }
                                            }

                                            Rectangle {
                                                Layout.fillWidth: false
                                                visible: card.badge !== ""
                                                implicitWidth: Math.min(page.svThumbWidth,
                                                                        badgeText.implicitWidth + 14)
                                                implicitHeight: badgeText.implicitHeight + 2
                                                radius: 7
                                                color: card.badgeBackground
                                                Text {
                                                    id: badgeText
                                                    anchors.centerIn: parent
                                                    width: parent.width - 14
                                                    text: card.badge
                                                    color: card.badgeColor
                                                    font.pixelSize: 11
                                                    elide: Text.ElideRight
                                                    ToolTip.text: page.sourceTextFor(card.path)
                                                    ToolTip.visible: badgeHover.hovered
                                                    HoverHandler { id: badgeHover }
                                                }
                                            }
                                            Item { Layout.fillHeight: true }
                                        }
                                    }
                                }
                            }
                            Item { Layout.fillHeight: true }
                        }
                    }
                }

                // ══ Секция «🖼 Steam-фоны» ════════════════════════════════════
                ScrollView {
                    id: bgScroll
                    // Имя — для сценария окна (прокрутка длинной сетки фонов).
                    objectName: "bgScroll"
                    clip: true
                    contentWidth: availableWidth

                    // Группа-секция: та же карточка с рамкой, что у виджетной
                    // вкладки (steamBgGroup). Краски и поля — от ядра.
                    Rectangle {
                        width: parent.width
                        // ⛔ Высоту содержимого область прокрутки берёт из
                        // `implicitHeight` единственного ребёнка, а не из
                        // `height`. Без неё `contentHeight` оставался -1, полоса
                        // считала содержимое равным окну: сетка уходила за
                        // нижний край без ползунка, колесо не крутило
                        // (живьём r1439, 37 анимированных фонов, 16.10.2026).
                        implicitHeight: bgCol.implicitHeight + root.st.group.marginV * 2
                        // Группа занимает полосу целиком, даже когда содержимого
                        // меньше: у оригинала она — виджет области прокрутки с
                        // `setWidgetResizable(True)` и растяжкой в конце.
                        height: Math.max(implicitHeight, bgScroll.availableHeight)
                        color: root.st.group.background
                        border.color: root.st.group.border
                        border.width: 1
                        radius: root.st.group.radius

                        ColumnLayout {
                            id: bgCol
                            anchors.fill: parent
                            anchors.leftMargin: root.st.group.marginH
                            anchors.rightMargin: root.st.group.marginH
                            anchors.topMargin: root.st.group.marginV
                            anchors.bottomMargin: root.st.group.marginV
                            spacing: root.st.group.spacing

                            Text {
                                Layout.fillWidth: true
                                text: page.bgGroupTitle
                                color: root.colText
                                font.pointSize: page.groupTitlePointSize
                                font.bold: true
                            }
                            Text {
                                Layout.fillWidth: true
                                text: page.bgGroupDesc
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 12

                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.bgAudioText
                                    checked: page.bgAudioOn
                                    onToggled: page.setBgAudio(checked)
                                }
                                // Вид фонов: все / статичные / анимированные,
                                // со счётчиками в подписях. Те же кнопки-чипы,
                                // что у фильтра заставок.
                                Repeater {
                                    model: page.bgKindChips
                                    delegate: RexonButton {
                                        required property var modelData
                                        objectName: "bgKindChip_" + modelData.key
                                        pal: root.pal
                                        tone: root.toneChip
                                        checkedTone: root.toneChipOn
                                        text: modelData.label
                                        checkable: true
                                        checked: modelData.checked
                                        onClicked: page.setBgKind(modelData.key)
                                    }
                                }
                                RexonTextField {
                                    Layout.fillWidth: true
                                    pal: root.pal
                                    placeholderText: page.bgSearchPlaceholder
                                    text: page.bgSearchText
                                    onTextEdited: page.setBgSearch(text)
                                }
                            }

                            // Почему анимированных фонов нет и что делать:
                            // без этой строки «Анимированные (0)» молчит, а
                            // человек не знает, что дело во входе в Steam.
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 12
                                visible: page.bgAnimatedNote !== ""

                                Text {
                                    objectName: "bgAnimatedNote"
                                    Layout.fillWidth: true
                                    text: page.bgAnimatedNote
                                    color: root.colText
                                    wrapMode: Text.WordWrap
                                }
                                RexonButton {
                                    objectName: "bgAnimatedRetry"
                                    pal: root.pal
                                    text: page.bgRetryText
                                    onClicked: page.reloadBackgrounds()
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                text: page.bgStatusText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // Карточки ТОЛЬКО текущей страницы (~25 из сотен) —
                            // ключевой приём против лага при открытии подвкладки.
                            //
                            // Столбцы задаёт ядро (`pager.cols`), по ним же
                            // нарезана страница — ряды всегда целые. Прежде
                            // `Flow` раскладывал карточки сам: на окне 1920
                            // клал девять в ряд, а страница была 8 × 5 = 40, и
                            // последний ряд выходил из четырёх. Остаток ширины
                            // делится между столбцами, как у сетки
                            // «Оформления», — без пустой полосы справа.
                            Item {
                                id: bgGridBox
                                Layout.fillWidth: true
                                implicitHeight: bgGrid.implicitHeight
                                // Ширина самой сетки, а не окна: её меняет и
                                // появление вертикального ползунка, а окно об
                                // этом события не шлёт.
                                onWidthChanged: page.setBgViewportWidth(String(Math.round(width)))
                                Component.onCompleted: page.setBgViewportWidth(String(Math.round(width)))

                                readonly property int cols: Math.max(1, page.pager.cols || 1)
                                readonly property int cardWidth: Math.max(page.bgCardWidth,
                                    Math.floor((width - (cols - 1) * root.st.card.gap) / cols))

                                Grid {
                                    id: bgGrid
                                    objectName: "bgGrid"
                                    width: parent.width
                                    columns: bgGridBox.cols
                                    columnSpacing: root.st.card.gap
                                    rowSpacing: root.st.card.gap

                                    Repeater {
                                        model: page.bgCards
                                        delegate: Rectangle {
                                            id: bgCard
                                            required property var modelData

                                            width: bgGridBox.cardWidth
                                            implicitHeight: bgCardCol.implicitHeight
                                                            + root.st.card.margin * 2
                                            radius: root.st.card.radius
                                            color: root.st.card.background
                                            border.width: 1
                                            border.color: bgHover.hovered ? root.st.card.hover
                                                                          : root.st.card.border
                                            HoverHandler { id: bgHover }

                                            // ⚠️ Имя своё, а не общее с колонкой
                                            // СЕКЦИИ: обе назывались `bgCol`, и внутри
                                            // делегата плитки имя секции было закрыто
                                            // тёзкой. Любая новая ссылка на колонку
                                            // секции из плитки молча привязалась бы к
                                            // колонке самой плитки.
                                            ColumnLayout {
                                                id: bgCardCol
                                                anchors.fill: parent
                                                anchors.margins: root.st.card.margin
                                                spacing: root.st.card.spacing

                                                Rectangle {
                                                    Layout.alignment: Qt.AlignHCenter
                                                    implicitWidth: page.bgThumbWidth
                                                    implicitHeight: page.bgThumbHeight
                                                    radius: root.st.thumb.radius
                                                    color: root.st.thumb.bgBackground
                                                    ToolTip.text: bgCard.modelData.thumbTooltip
                                                    ToolTip.visible: thumbHover.hovered
                                                                     && bgCard.modelData.thumbTooltip !== ""
                                                    HoverHandler { id: thumbHover }
                                                    Image {
                                                        anchors.fill: parent
                                                        source: bgCard.modelData.thumb
                                                        fillMode: Image.PreserveAspectCrop
                                                        asynchronous: true
                                                        visible: source !== ""
                                                    }
                                                    Text {
                                                        anchors.centerIn: parent
                                                        visible: bgCard.modelData.thumb === ""
                                                        text: bgCard.modelData.icon
                                                        font.pixelSize: 22
                                                    }
                                                    // Отметка анимированного фона:
                                                    // у статичного её нет вовсе.
                                                    Rectangle {
                                                        objectName: "bgAnimatedMark"
                                                        visible: bgCard.modelData.mark !== ""
                                                        anchors.left: parent.left
                                                        anchors.bottom: parent.bottom
                                                        anchors.margins: 4
                                                        width: bgMarkText.implicitWidth + 10
                                                        height: bgMarkText.implicitHeight + 4
                                                        radius: 4
                                                        color: "#cc000000"
                                                        Text {
                                                            id: bgMarkText
                                                            anchors.centerIn: parent
                                                            text: bgCard.modelData.mark
                                                            font.pixelSize: 15
                                                        }
                                                    }
                                                    RexonZoomArea {
                                                        anchors.fill: parent
                                                        pal: root.pal
                                                        radius: root.st.thumb.radius
                                                        // Своя подсказка есть только
                                                        // у анимированного фона — она
                                                        // и остаётся, без второй поверх.
                                                        tooltip: bgCard.modelData.thumbTooltip !== ""
                                                                 ? "" : qsTranslate("rexon", "Open large view")
                                                        onActivated: root.openBackgroundViewer(bgCard.modelData.id)
                                                    }
                                                }

                                                // Значок и имя — двумя подписями, а не
                                                // одной: у оригинала обрезается ТОЛЬКО
                                                // имя (по ширине миниатюры), а значок
                                                // стоит перед обрезкой и виден всегда.
                                                RowLayout {
                                                    Layout.fillWidth: true
                                                    spacing: 0
                                                    Text {
                                                        text: bgCard.modelData.icon + " "
                                                        color: root.colText
                                                    }
                                                    Text {
                                                        Layout.fillWidth: true
                                                        Layout.maximumWidth: page.bgThumbWidth
                                                        text: bgCard.modelData.name
                                                        color: root.colText
                                                        elide: Text.ElideRight
                                                        ToolTip.text: bgCard.modelData.name
                                                        ToolTip.visible: bgNameHover.hovered
                                                        HoverHandler { id: bgNameHover }
                                                    }
                                                }

                                                // Пустую строку всё равно рисуем: иначе
                                                // карточки ряда разъезжаются по высоте.
                                                Text {
                                                    Layout.fillWidth: true
                                                    Layout.maximumWidth: page.bgThumbWidth
                                                    text: bgCard.modelData.sub
                                                    color: root.colTextSecondary
                                                    elide: Text.ElideRight
                                                }

                                                RexonButton {
                                                    Layout.fillWidth: true
                                                    pal: root.pal
                                                    // Синяя «Применить», зелёная
                                                    // «✓ Применено»: какой фон стоит
                                                    // сейчас, видно, не читая надпись.
                                                    tone: bgCard.modelData.applied
                                                          ? root.st.appliedButton
                                                          : root.st.primaryButton
                                                    text: bgCard.modelData.applied ? page.bgAppliedText
                                                                                   : page.bgApplyText
                                                    onClicked: page.applyBackground(bgCard.modelData.id)
                                                }
                                            }
                                        }
                                    }
                                }

                            }

                            // ── Пагинация ─────────────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                visible: page.pager.visible
                                spacing: 10

                                Item { Layout.fillWidth: true }
                                RexonButton {
                                    pal: root.pal
                                    text: page.bgPrevText
                                    enabled: page.pager.prevEnabled
                                    onClicked: page.bgPrevPage()
                                }
                                Text {
                                    text: page.pager.text
                                    color: root.colText
                                    horizontalAlignment: Text.AlignHCenter
                                }
                                RexonButton {
                                    pal: root.pal
                                    text: page.bgNextText
                                    enabled: page.pager.nextEnabled
                                    onClicked: page.bgNextPage()
                                }
                                Item { Layout.fillWidth: true }
                            }
                            Item { Layout.fillHeight: true }
                        }
                    }
                }

                // ══ Секция «🖼 Витрины профиля» ═══════════════════════════════
                // Нарезка своей картинки под витрину профиля и предпросмотр.
                // Раздел — отдельный компонент со своим объектом действий
                // (`SteamShowcaseSection.qml`); вкладка даёт ему палитру,
                // оформление и список фонов профиля.
                ScrollView {
                    id: showcaseScroll
                    clip: true
                    contentWidth: availableWidth

                    SteamShowcaseSection {
                        width: showcaseScroll.availableWidth
                        pal: root.pal
                        sty: root.st
                        warnColor: page.goldColor
                        backgrounds: page.bgChoices
                        onMessage: text => statusToast.show(text)
                        onZoomRequested: (items, index) => viewer.show(items, index)
                    }
                }
            }
        }
    }

    // ── Крупный просмотр картинок и роликов ───────────────────────────────
    //
    // Один слой на обе сетки вкладки. Полноразмерный файл фона, которого
    // ещё нет на диске, просмотр просит через `wanted`, а получает через
    // `sources` — качает его тот же слой загрузки, что и миниатюры.
    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
        sources: page.bgViewerSources
        onWanted: key => page.requestBgFull(key)
        onOpened: root.syncPreview()
        onClosed: root.syncPreview()
    }

    // ── Строка состояния ──────────────────────────────────────────────────
    //
    // Накладка, а не место в раскладке: она не резервирует высоту и не
    // сдвигает витрину, когда сообщения нет.
    //
    // ⚠️ Плашка общая на весь продукт (`RexonToast.qml`), и таймер гашения
    // живёт в ней. Здесь стояла своя: `visible: statusText !== ""` и ни
    // одного таймера — сообщение висело, пока вкладку не закроют (живая
    // проверка 18.09 застала его через 29 минут). Заодно ушли свои числа
    // (радиус 6, поля 28×16, отступ 18) и своё место по центру: и то и
    // другое приходит из ядра, а плашка встала низом слева — там же, где
    // стоит виджетная.
    RexonToast {
        id: statusToast
        skin: root.st.toast
        timeoutMs: page.toastTimeoutMs
        metrics: page
        availableWidth: root.width
        // Сообщения вкладки несут имена фонов и пути, а не разметку:
        // разбирать их как разметку значило бы испортить «&» в названии.
        textFormat: Text.PlainText

        // Начальное сообщение: сигнала о нём не будет — он уже прошёл.
        Component.onCompleted: statusToast.show(page.statusText)
    }
}
