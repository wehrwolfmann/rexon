// Вкладка «Wallpaper Engine» на QML — ПЕРВЫЙ шаг переноса шелла.
//
// Данные и решения — Rust-объект WallpaperEngineModel
// (rust/rexon-gui/src/we_page.rs), который сам ничего не решает, а
// спрашивает ядро (`rexon_core::we_window`). Здесь только РАЗМЕТКА.
//
// ⚠️ Содержимое пунктов рейла — вендорный QML waywallen (MIT, (c) hypengw;
// атрибуция в NOTICE): он живёт отдельным модулем `waywallen.ui`, требует
// собранных плагинов плюс живого сокета демона и грузится ВНУТРЬ шелла
// `PageHost.qml`, а не прямо сюда. Поэтому у страницы три стартовых
// состояния — «подключение…», «нет демона» с кнопкой повтора и «плагины не
// собраны»: покупатель обязан прочитать словами, чего вкладка ждёт, а не
// смотреть в пустой прямоугольник (правило #13). Что показать и какими
// словами, решает ядро.
//
// ⚠️ Импортов `Qcm.Material` и `waywallen.ui` в этом файле нет и быть не
// должно. Вкладка обязана открываться и там, где плагины НЕ собраны, — а
// файл с таким импортом в такой машине не компилируется вовсе, и вместо
// экрана «плагины не собраны» вкладка молча падала бы целиком. Всё, что
// требует этих модулей (режим Material по скину и тосты), живёт в шелле,
// который грузится отдельным файлом и только когда плагины есть.
//
// Оформление берётся из готового: палитра приходит свойством `palette` от
// того же объекта и родом из действующего скина продукта (правило #17).
// Своих цветов страница не заводит.
//
// Рейка навигации — как у waywallen (решение Wehrwolfmann 10.10.2026): её
// рисует шелл `PageHost.qml` компонентами Material, потому что здесь их
// импортировать нельзя (см. выше). Шелл поэтому занимает ВСЮ вкладку, а
// полосы и экраны состояний этой страницы лежат поверх него справа от
// рейки (на узкой вкладке — над нижней полосой навигации, которая сменяет
// рейку). Из прежней собственной рейки оставлен один пункт — «Разраб.»; там,
// где плагины не собраны и шелла нет, он показан отдельной кнопкой, иначе до
// страницы сборки было бы не добраться ровно тогда, когда она нужнее всего.

// Делегаты обращаются к id внешних элементов (root, page).
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Мост наружу ───────────────────────────────────────────────────────

    // Объект побочных действий. По умолчанию — свой, на Rust; хозяин
    // страницы может подставить другой. Нужен он ровно за одним: поднять
    // демона обоев по кнопке «Повторить запуск». Сама страница в систему не
    // ходит — что показать, считает Rust.
    // Методы: retryDaemon(). Сигналы: retryStarted(); у своего объекта ещё
    // connecting(), daemonFailed(reason), healthReceived(state),
    // healthTimedOut() — события демона, и метод connectDaemon().
    property var actions: ownActions

    WeActions {
        id: ownActions
        // Имя нужно проверкам — тем же приёмом, что у модели ниже.
        objectName: "weActions"
    }

    // Экран «подключение…» после нажатия кнопки. Ставит его модель, а не
    // объект действий: у того нет и не должно быть указателя на неё —
    // связывает их разметка.
    //
    // ⚠️ `ignoreUnknownSignals` обязателен: хозяин страницы вправе
    // подставить свой объект действий без этого сигнала (так делает
    // питоновский хозяин живой вкладки — он переводит модель сам), и такая
    // подстановка не должна ронять в журнал предупреждение на каждый кадр.
    //
    // Остальные четыре сигнала — события демона, которые объект действий
    // пересказывает странице (`we_actions.rs`): что они значат и каким
    // экраном отвечать, решает модель, то есть ядро.
    Connections {
        target: root.actions
        ignoreUnknownSignals: true
        function onRetryStarted() { page.showConnecting(); }
        function onConnecting() { page.showConnecting(); }
        function onDaemonFailed(reason) { page.showNoDaemon(reason); }
        function onHealthReceived(state) { page.applyHealth(state); }
        function onHealthTimedOut() { page.showHealthTimeout(); }
    }

    // Сторож ожидания: экран «Подключение…» обязан КОНЧАТЬСЯ.
    //
    // ⚠️ Без него ожидание было бесконечным, и кнопка «Повторить запуск» не
    // появлялась никогда: у двух путей к этому экрану своего срока нет
    // вовсе — сокет, который не открывается, и обрыв соединения, после
    // которого транспорт переподключается раз в три секунды без предела
    // попыток. Разбор обоих — в шапке `we_window::CONNECT_TIMEOUT_MS`.
    //
    // Решений здесь нет: срок считает ядро, отказ складывает модель. Сторож
    // привязан к самому ЭКРАНУ, а не к попытке, поэтому он покрывает любой
    // новый путь к ожиданию сразу (правило #12), успешное подключение
    // гасит его сменой состояния, а повторный заход на экран заводит
    // заново — сколько угодно раз.
    //
    // ⚠️ Пока идёт запуск движка (`engineStarting`), сторож стоит: у запуска
    // свой сторож в ядре (`we_client::StartWatch`) — он ждёт живой процесс
    // дольше обычного и сам кончается отказом, если процесс умер или завис.
    // Без этого долгий первый запуск AppImage обрывался бы здесь через 25 с.
    Timer {
        interval: page.connectTimeoutMs
        repeat: false
        running: page.state === page.stateConnecting && !ownActions.engineStarting
        onTriggered: page.showConnectTimeout()
    }

    // Каталог вендоренного интерфейса waywallen (`qml_ui/` пакета) — его
    // называет хозяин страницы начальным свойством. Пусто — вендорных
    // страниц не будет, `Loader` останется пустым: так страница открывается
    // и в процессе, где пакета `rexon/` нет вовсе.
    property string vendorQmlDir: ""

    // ── Ядро вкладки ──────────────────────────────────────────────────────

    // Имя типа — WallpaperEngineModel, а не WallpaperEnginePage: последнее
    // занято самим этим файлом (в QML-модуле каждый .qml — тип по имени).
    WallpaperEngineModel {
        id: page

        // Адрес вендорной страницы складывает ядро, а не разметка: здесь
        // только передача каталога, который знает хозяин.
        vendorQmlDir: root.vendorQmlDir
        // Имя нужно проверкам: из хозяина страницы объект достают
        // `root.findChild(QObject, "wallpaperEngineModel")` и спрашивают
        // без единой правки в самой странице.
        objectName: "wallpaperEngineModel"

        // Дошло до страниц — значит шелл вендорного интерфейса пора
        // строить. Обратно флаг не гасится: см. `vendorHostWanted`.
        onStateChanged: {
            if (page.state === page.statePages)
                root.vendorHostWanted = true;
        }
    }

    // ── Палитра действующего скина ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    // Приглушённый текст ожидания: та же роль текста с долей
    // прозрачности из ядра — так его красит виджетная версия
    // (`muted_qss` поверх роли `Text`). Отдельной краски скина
    // (`text_secondary`) у оригинала здесь нет: на снимке 03.09 текст
    // «подключение…» у QML выходил чисто белым против приглушённого.
    readonly property color colTextMuted: Qt.rgba(root.colText.r, root.colText.g,
                                                  root.colText.b, page.variantAlpha)

    // ── Страница выбранного пункта ────────────────────────────────────────

    // Привязка, а не разовый вызов: адрес пересчитывается сам при
    // переключении пункта и при позднем ответе хозяина.
    //
    // ⚠️ Каталог читается ЯВНО, хотя внутри `vendorPageUrl` он и так
    // спрашивается. Без этого чтения привязка о нём не знает: зависимости
    // QML собирает по прочитанным СВОЙСТВАМ, а не по тому, что инвокабл
    // взял из своего состояния. Первый кадр вкладки 30.08 показал ровно
    // это — «Обои» открылись заглушкой вместо страницы: адрес посчитался
    // один раз, пока каталог был пуст, и обновиться ему было нечем.
    readonly property url vendorPageUrl: page.vendorQmlDir !== ""
                                         ? page.vendorPageUrl(page.currentIndex)
                                         : ""

    // Есть ли что грузить. Признаком служит явное приведение к строке, а не
    // сравнение `url` с пустой строкой: у них разные типы, и такое сравнение
    // держится на приведении, которое проверка QML справедливо не любит.
    readonly property bool hasVendorPage: String(root.vendorPageUrl) !== ""

    // Нужна ли этой странице ВЕНДОРНАЯ ШАПКА (`MD.AppBar` с её заголовком и
    // кнопками действий). Ответ держит ядро — он один на обе версии
    // вкладки; сам вопрос разбирается в шапке `PageHost.qml`.
    readonly property bool vendorPageHeader: page.vendorPageHeader(page.currentIndex)

    // ── Скрытая страница «Разработчик» ────────────────────────────────────

    // Открыт ли сейчас пункт «Разраб.». Сравниваем с номером пункта из
    // ядра, а не с именем `dev`: имя пункта — его дело, а не разметки.
    readonly property bool onDevPage: page.devIndex >= 0
                                      && page.currentIndex === page.devIndex

    // Страница строится, когда до неё дошли, и живёт дальше (см. `devHost`).
    property bool devHostWanted: false

    onOnDevPageChanged: {
        if (root.onDevPage)
            root.devHostWanted = true;
    }

    // ── Ленивый шелл вендорных страниц ────────────────────────────────────

    // Шелл строится ОДИН раз и только когда дело дошло до страниц — как в
    // виджетной версии, где общий QML-хост создавался лениво на первом их
    // показе. Раз построенный, он живёт дальше: обрыв сокета возвращает
    // вкладку на «подключение…», и пересборка страницы на каждый обрыв
    // стоила бы покупателю её состояния — прокрутки библиотеки, открытой
    // карточки, набранного фильтра.
    property bool vendorHostWanted: false

    // Повторный запуск демона. В систему ходит объект действий, а не
    // разметка; экран ожидания при этом ставит ядро — тем же переходом,
    // что и при первом старте вкладки (см. `Connections` выше).
    function retryDaemon() {
        if (!root.actions)
            return;
        root.actions.retryDaemon();
    }

    // ── Первый кадр ───────────────────────────────────────────────────────

    Component.onCompleted: {
        // Демона зовём сами — но только если действия НАШИ. Хозяин, который
        // подставил свой объект, ведёт демона сам, и второй ведущий открыл бы
        // второй сокет. Без этого вызова вкладка в окне без хозяина
        // оставалась на «подключение…» навсегда.
        if (root.actions === ownActions)
            ownActions.connectDaemon();
    }

    // Сколько места слева занято рейкой: её ширину сообщает шелл (она
    // меняется при сворачивании), а без шелла место держит запасная кнопка
    // «Разраб.».
    readonly property real railInset: vendorHost.item
                                      ? vendorHost.item.railWidth
                                      : (devFallback.visible ? devFallback.width : 0)

    // ── Разметка ──────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ══ Шелл вендорного интерфейса: рейка и страница ═══════════════════
    //
    // Грузится ШЕЛЛ вокруг страницы, а не страница: он держит режим Material
    // по скину Rexon, размерный класс, тосты и рейку навигации — то есть
    // роль их `Window.qml`. Строится ОДИН раз, как только известно, что
    // плагины собраны: рейка видна и на экране «подключение…», и на отказе
    // демона — как окно waywallen, которое без демона тоже открывается с
    // рейкой. Сами страницы шелл строит позже (`pagesReady`).
    //
    // ⚠️ Грузится АСИНХРОННО — как страницы в оригинале waywallen
    // (`MD.Pool`, QQmlIncubator в режиме Asynchronous): синхронный `Loader`
    // останавливал бы главный поток до конца компиляции.
    //
    // Движок один на все страницы: пункт меняет `pageUrl`, а не создаёт
    // второй `QQuickWidget`. Несколько движков с модулем
    // `waywallen.control` дедлочат `QQmlThread`.
    //
    // ⚠️ Страница «Настройки» держится не только на шелле: её переключатель
    // «Язык» и раздел «Кэш» ходят в САМ ДВИЖОК сцены (`retranslate()` и
    // дисковый кэш сетевых картинок, поставленный фабрикой Rust-шима).
    Loader {
        id: vendorHost

        anchors.fill: parent
        asynchronous: true
        active: page.qmlAvailable
        source: page.pageHostUrl
        onLoaded: root.passOutsidePresses()
    }

    // Окна поверх вендорного интерфейса («Плагины», «Настройки», «О
    // программе», вопросы страниц) накрывают только эту вкладку, а разделы
    // Rexon над ней остаются нажимаемыми: слой окон поверх шелл ужимает до
    // себя сам, а нажатия вне вкладки пропускает окну фильтр объекта
    // действий. Фильтр ставится, когда слой уже заведён (`popupLayer`
    // шелла): он обязан стоять в очереди после фильтра слоя.
    Connections {
        target: vendorHost.item
        ignoreUnknownSignals: true
        function onPopupLayerChanged() {
            root.passOutsidePresses();
        }
    }

    function passOutsidePresses() {
        if (vendorHost.item && vendorHost.item.popupLayer)
            ownActions.passOutsidePresses(root);
    }

    // Какую страницу показывает шелл. Привязкой, а не присвоением в
    // `onLoaded`: шелл асинхронный, и пункт может смениться раньше, чем он
    // построится.
    Binding {
        target: vendorHost.item
        property: "pageUrl"
        value: root.vendorPageUrl
        when: vendorHost.status === Loader.Ready
    }

    // Шапку вендорной страницы включает ХОЗЯИН, а не сама страница:
    // `MD.Page` рисует её по `MD.MProp.page.showHeader`.
    Binding {
        target: vendorHost.item
        property: "pageHeader"
        value: root.vendorPageHeader
        when: vendorHost.status === Loader.Ready
    }

    // Светлый или тёмный тон интерфейса задаёт СКИН Rexon, а не системная
    // тема рабочего стола.
    Binding {
        target: vendorHost.item
        property: "skinDark"
        value: page.skinDark
        when: vendorHost.status === Loader.Ready
    }

    // Краски вендорного интерфейса: по умолчанию — скин Rexon (акцент,
    // фон и текст страницы), у скина «Как в waywallen» — схема самого
    // waywallen с его настройками «Тема» и «Акцент». Решает ядро.
    Binding {
        target: vendorHost.item
        property: "followVendorTheme"
        value: page.vendorThemeFollow
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinAccent"
        value: page.vendorAccent
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinBackground"
        value: page.vendorBackground
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinText"
        value: page.vendorText
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinRoles"
        value: page.vendorRoles
        when: vendorHost.status === Loader.Ready
    }

    // Пункты рейки и выбранный из них называет ядро; шелл их рисует.
    Binding {
        target: vendorHost.item
        property: "railItems"
        value: page.railItems
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "currentIndex"
        value: page.currentIndex
        when: vendorHost.status === Loader.Ready
    }

    Connections {
        target: vendorHost.item
        function onPageRequested(index) {
            page.currentIndex = index;
        }
    }

    // Страницы строятся не раньше, чем демон ответил, что здоров, и живут
    // после этого (см. `vendorHostWanted`); видны — только на экране страниц
    // и только пока открыт вендорный пункт.
    Binding {
        target: vendorHost.item
        property: "pagesReady"
        value: root.vendorHostWanted
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "pageVisible"
        value: page.state === page.statePages && root.hasVendorPage
        when: vendorHost.status === Loader.Ready
    }

    // Полосы этой страницы («чей фон», плагин Plasma) стоят над страницей
    // шелла — он обязан уступить им место сверху.
    Binding {
        target: vendorHost.item
        property: "pageTopInset"
        value: stateArea.y
        when: vendorHost.status === Loader.Ready
    }

    // ══ Запасная кнопка «Разраб.» ══════════════════════════════════════
    //
    // Только там, где шелла нет вовсе (плагины вендорного интерфейса не
    // собраны), а режим разработчика включён. Нажатие открывает страницу
    // сборки, повторное — возвращает к первому пункту.
    RexonButton {
        id: devFallback

        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.margins: 8
        width: page.railWidth - 16
        visible: !page.qmlAvailable && page.devIndex >= 0
        pal: root.pal
        text: page.devIndex >= 0 ? page.railItems[page.devIndex].label : ""
        checkable: true
        checked: root.onDevPage
        onClicked: page.currentIndex = root.onDevPage ? 0 : page.devIndex
    }

    // ══ Правая часть: полосы и место страницы (поверх шелла) ════════════
    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: root.railInset
        // На узкой вкладке рейку сменяет нижняя полоса навигации шелла.
        anchors.bottomMargin: vendorHost.item ? vendorHost.item.barHeight : 0
        spacing: 0

        // ── «Чей фон» ─────────────────────────────────────────────────────
        //
        // Поверх композитора может жить чужой шелл (Noctalia,
        // DankMaterialShell), который рисует фон САМ — своей поверхностью
        // поверх наших обоев. Тогда здесь, по месту, полоса с выбором:
        // «Отдать обои Rexon» (выключить обои шелла, запомнив прежнюю
        // настройку) либо, если Rexon их уже выключил, «Вернуть оболочке».
        // Решение, текст и подписи — ядро (`desktop_shell::shell_banner`);
        // разметка только показывает. Без живого шелла полосы нет вовсе.
        Rectangle {
            id: shellBanner
            objectName: "shellBanner"
            Layout.fillWidth: true
            Layout.margins: 12
            Layout.bottomMargin: 0
            visible: page.shellBannerVisible || page.shellActionMessage !== ""
            implicitHeight: shellBannerColumn.implicitHeight + 24
            radius: 6
            color: root.pal.bg_card
            border.width: 1
            border.color: root.pal.border

            ColumnLayout {
                id: shellBannerColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    visible: page.shellBannerVisible
                    text: page.shellBannerText
                    color: root.colText
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    visible: page.shellBannerVisible
                    spacing: 8

                    RexonButton {
                        objectName: "shellGiveButton"
                        pal: root.pal
                        visible: page.shellCanGive
                        text: page.shellGiveLabel
                        onClicked: page.giveWallpaperToRexon()
                    }

                    RexonButton {
                        objectName: "shellReturnButton"
                        pal: root.pal
                        visible: page.shellCanReturn
                        text: page.shellReturnLabel
                        onClicked: page.returnWallpaperToShell()
                    }
                }

                // Итог нажатия: успех — приглушённо, отказ — жирным (своих
                // красок у страницы нет, правило #17).
                Text {
                    objectName: "shellActionMessage"
                    Layout.fillWidth: true
                    visible: page.shellActionMessage !== ""
                    text: page.shellActionMessage
                    color: page.shellActionFailed ? root.colText : root.colTextMuted
                    font.bold: page.shellActionFailed
                    wrapMode: Text.WordWrap
                }
            }
        }

        // ── Плагин обоев Plasma ───────────────────────────────────────
        //
        // На KDE обои waywallen показывает его Plasma-плагин. Нет плагина
        // — фон молча остаётся прежним; здесь об этом говорится словами
        // и предлагается поставить плагин. Текст, подпись и решение
        // «можно ли ставить» — объект действий (`we_actions.rs`), страница
        // только показывает. Своей копии этих свойств у страницы нет:
        // они живут на СВОЁМ объекте действий, подставной хозяина их не
        // несёт.
        Rectangle {
            objectName: "kdePluginBanner"
            Layout.fillWidth: true
            Layout.margins: 12
            Layout.bottomMargin: 0
            visible: ownActions.kdePluginNotice !== ""
            implicitHeight: kdePluginColumn.implicitHeight + 24
            radius: 6
            color: root.pal.bg_card
            border.width: 1
            border.color: root.pal.border

            ColumnLayout {
                id: kdePluginColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: ownActions.kdePluginNotice
                    color: root.colText
                    wrapMode: Text.WordWrap
                }

                RexonButton {
                    objectName: "kdePluginInstallButton"
                    pal: root.pal
                    visible: ownActions.kdePluginCanInstall
                    enabled: !ownActions.kdePluginInstalling
                    text: ownActions.kdePluginInstallLabel
                    onClicked: ownActions.installKdePlugin()
                }
            }
        }

        // Шелл могут запустить и погасить, пока окно открыто: «чей фон»
        // пересчитывается при открытии и раз в несколько секунд, пока
        // вкладка видна. Сбор дешёвый — каталог сеанса и `/proc`, без
        // внешних процессов.
        Timer {
            interval: 5000
            repeat: true
            triggeredOnStart: true
            running: root.visible
            onTriggered: page.refreshShellState()
        }

        // ══ Место страницы ════════════════════════════════════════════════
        //
        // Три стартовых состояния вкладки — «подключение…», «нет демона» и
        // «страницы» — те же, что в виджетной версии, и переключаются тем же
        // порядком. Рейка при этом видна всегда (её держит шелл под этим
        // слоем): она часть вкладки, а не часть состояния.
        Item {
            id: stateArea
            Layout.fillWidth: true
            Layout.fillHeight: true

            // ── Ожидание движка ───────────────────────────────────────────
            Text {
                anchors.centerIn: parent
                width: parent.width - 2 * page.stateColumnMargin
                visible: page.state === page.stateConnecting && !root.onDevPage
                // Долгий запуск — с пояснением, что первый запуск бывает
                // долгим (текст — ядра, `we_window::text_connecting_slow`).
                text: ownActions.engineStartNote !== ""
                      ? page.connectingText + "\n\n" + ownActions.engineStartNote
                      : page.connectingText
                color: root.colTextMuted
                font.pixelSize: page.stateTextFontSize
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
            }

            // ── Отказ движка и повтор запуска ─────────────────────────────
            // Зазор и поля колонки — из ядра, те же, что у `QVBoxLayout`
            // виджетной версии. Со своим зазором (12) экран уезжал от
            // оригинала: значок выше на 12 точек, текст — на 3 (замер 03.09).
            ColumnLayout {
                anchors.centerIn: parent
                width: parent.width - 2 * page.stateColumnMargin
                spacing: page.stateColumnSpacing
                visible: page.state === page.stateNoDaemon && !root.onDevPage

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: page.noDaemonIcon
                    font.pixelSize: page.noDaemonIconFontSize
                }

                Text {
                    Layout.fillWidth: true
                    // Заголовок отказа и причина — одной строкой из ядра:
                    // склейку держит оно, а не разметка.
                    text: page.noDaemonText
                    color: root.colText
                    font.pixelSize: page.stateTextFontSize
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                }

                RexonButton {
                    Layout.alignment: Qt.AlignHCenter
                    // Имя нужно проверкам: кнопку достают
                    // `root.findChild(QObject, "retryDaemonButton")` и
                    // жмут НАСТОЯЩИМ нажатием, а не обходом разметки.
                    objectName: "retryDaemonButton"
                    pal: root.pal
                    text: page.retryDaemonLabel
                    // Свой объект действий у страницы есть всегда, но
                    // хозяин вправе занулить свойство. Тогда поднимать
                    // демона нечем, и кнопка обязана ВЫГЛЯДЕТЬ нерабочей, а
                    // не молча не срабатывать по нажатию (правило #13).
                    enabled: root.actions !== null
                    onClicked: root.retryDaemon()
                }
            }

            // ── Страницы ──────────────────────────────────────────────────
            Item {
                anchors.fill: parent
                // Страница разработчика от демона не зависит: на отказе
                // движка она нужнее всего (там кнопка его установки).
                visible: page.state === page.statePages || root.onDevPage

                // Плагины вендорного интерфейса не собраны: вместо страниц —
                // инструкция по сборке. Текст из ядра, тот же, что в
                // виджетной версии.
                // Обычная роль текста, как у метки оригинала без стиля,
                // а не «вторичная» краска скина.
                Text {
                    anchors.centerIn: parent
                    width: parent.width - 2 * page.stateColumnMargin
                    visible: !page.qmlAvailable && !root.onDevPage
                    text: page.noQmlModulesText
                    color: root.colText
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                }

                // Вендорную страницу рисует шелл под этим слоем
                // (`vendorHost` выше) — здесь для неё ничего нет.
                //
                // ⚠️ Модальный слой (`Overlay.overlay`) заводит окно сцены, а
                // сцена одна на всю вкладку: вопрос вендорной страницы
                // (`MD.Dialog` «Kill renderer?» на «Статусе») накрывает и
                // рейку, и увести страницу из-под него нечем. Это починка
                // дефекта виджетной версии, а не расхождение.
                //
                // ⚠️ Ловушка приёмки раздела «О движке»: «неверная краска»
                // кнопки «Журнал изменений» на снимке — состояние НАВЕДЕНИЯ
                // МЫШИ (опыт 05.09), сводить тут нечего.

                // Скрытая страница «Разработчик» — НАША, из этого же
                // QML-модуля, а не из каталога waywallen. Строится ЛЕНИВО и
                // ровно один раз: у неё свой рантайм фоновых задач, и
                // пересборка на каждый уход с пункта обрывала бы начатую
                // установку движка (~200 МиБ) вместе с её состоянием.
                //
                // ⚠️ До 06.09.2026 здесь стояла заглушка с именем пункта:
                // страницу при переезде вкладки на QML не портировали
                // вовсе, и владелец, открыв «Разраб.», видел пустое место
                // со словом `dev` — то есть не мог добраться до кнопки
                // установки обновления waywallen.
                Loader {
                    id: devHost

                    anchors.fill: parent
                    asynchronous: true
                    active: root.devHostWanted
                    visible: root.onDevPage
                    sourceComponent: devPage
                }

                Component {
                    id: devPage

                    WeDeveloperPage {
                        // Пометка расхождения паритета живёт в подписи
                        // кнопки рейла — её ставит ядро, а страница только
                        // сообщает вердикт сверки.
                        onParityDrift: (drift) => page.parityDrift = drift
                    }
                }
            }
        }
    }
}
