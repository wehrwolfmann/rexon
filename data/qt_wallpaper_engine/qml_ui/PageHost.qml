// Универсальный шелл Rexon вокруг ЛЮБОЙ вендоренной страницы waywallen
// (НАШ файл, не upstream). Роль их Window.qml: MD-темизация, тосты и
// рейка навигации слева. Страница задаётся свойством pageUrl, пункты рейки —
// свойством railItems (их называет ядро, `we_window::build_rail`).
pragma ComponentBehavior: Bound
import QtQuick
import QtQml
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W

Item {
    id: root

    // file:// URL вендоренной страницы (напр. .../page/StatusPage.qml)
    property url pageUrl

    // ── Рейка навигации (как у waywallen) ─────────────────────────────
    //
    // Рейка — та же, что в `ui/qml/Window.qml` вендора, и собрана ЕГО же
    // компонентами (`MD.NavigationRail`, `MD.RailItem`,
    // `W.SidebarNowPlaying`): кнопка сворачивания, логотип с названием,
    // страницы со значками Material, внизу карточка текущих обоев и пункты
    // «Плагины», «Настройки», «О программе». Разметка повторена, а не взята
    // из вендорного файла, потому что взять её оттуда нечем: у вендора рейка
    // — безымянный `sourceComponent` внутри `MD.ApplicationWindow`, то есть
    // часть ОКНА верхнего уровня. Создать его внутри нашей сцены значит
    // открыть второе окно рядом с Rexon, а отдельного типа рейки в их
    // `qmldir` нет. Править вендорный файл нельзя (его сверяет
    // `sync-waywallen-qml.py --check`).
    //
    // Состав пунктов называет ядро (`railItems`: `id`, `label`, …), а
    // раскладку «что страница, что всплывающее окно» держит этот файл — так
    // же, как её держит `Window.qml`: «Плагины», «Настройки» и
    // «О программе» у waywallen открываются окном поверх
    // (`component/PagePopup.qml`), а не страницей.
    //
    // Наш здесь один пункт — «Разраб.» (страница сборки для разработчика):
    // он приходит от ядра только в режиме разработчика; в свёрнутой рейке
    // стоит внизу, рядом с «Настройками», в развёрнутой — значком в шапке
    // (иначе рейка не помещается по высоте). Подписи вендорных пунктов берутся из каталога
    // перевода ВЕНДОРА его же контекстом (`Window`), подпись нашего — из
    // ядра.
    property var railItems: []
    // Номер выбранного пункта в `railItems` — его держит хозяин (ядро), а
    // рейка только просит сменить.
    property int currentIndex: 0
    signal pageRequested(int index)

    // Страницы можно строить (демон ответил) и показывать (сейчас не экран
    // «подключение…»). Рейка от обоих не зависит: она видна всегда.
    property bool pagesReady: true
    property bool pageVisible: true
    // Сколько места сверху занято полосами хозяина («чей фон» и т.п.).
    property real pageTopInset: 0
    // Ширина рейки — хозяин кладёт по ней свои слои справа от неё.
    readonly property real railWidth: m_rail_loader.active ? m_rail_loader.width : 0
    // Высота нижней полосы навигации — хозяин кладёт свои слои над ней.
    readonly property real barHeight: m_bar_loader.active ? m_bar_loader.height : 0

    // Узкая вкладка (размерный класс Material «compact», уже 600): рейку
    // сменяет нижняя полоса навигации — как в `Window.qml` вендора.
    readonly property bool isCompact: MD.MProp.size.isCompact

    function _railIndex(id) {
        for (let i = 0; i < root.railItems.length; ++i) {
            if (root.railItems[i].id === id)
                return i;
        }
        return -1;
    }

    // Страницы рейки: порядок и значки — как в `pageModel` вендора.
    readonly property var _railPages: {
        const known = [
            ["wallpapers_qml", MD.Token.icon.wallpaper, qsTranslate("Window", "Wallpapers")],
            ["discover_qml", MD.Token.icon.explore, qsTranslate("Window", "Discover")],
            ["displays_qml", MD.Token.icon.monitor, qsTranslate("Window", "Displays")],
            ["status_qml", MD.Token.icon.monitor_heart, qsTranslate("Window", "Status")]
        ];
        const out = [];
        for (const [id, icon, name] of known) {
            const at = root._railIndex(id);
            if (at >= 0)
                out.push({
                    icon: icon,
                    name: name,
                    page: at
                });
        }
        return out;
    }

    readonly property int _devIndex: root._railIndex("dev")
    readonly property int _wallpapersIndex: root._railIndex("wallpapers_qml")

    // Какой пункт рейки подсветить; -1 — открыта страница не из списка.
    readonly property int _railCurrent: {
        for (let i = 0; i < root._railPages.length; ++i) {
            if (root._railPages[i].page === root.currentIndex)
                return i;
        }
        return -1;
    }

    // Порт `Window.showAbout` вендора.
    property var _aboutPresentation: null

    function showAbout() {
        if (root._aboutPresentation?.active)
            return;
        root._aboutPresentation = root.presentPopup('waywallen.ui/PagePopup', {
            source: 'waywallen.ui/AboutPage'
        });
    }

    // Порт `Window.openWallpaper` вендора: карточка текущих обоев открывает
    // обоину на странице «Обои». Страница грузится асинхронно, поэтому
    // просьба ждёт её готовности.
    property string _pendingWallpaperId: ""

    function openWallpaper(wallpaperId) {
        root._pendingWallpaperId = String(wallpaperId || "");
        if (root._pendingWallpaperId.length === 0)
            return;
        if (root.currentIndex !== root._wallpapersIndex)
            root.pageRequested(root._wallpapersIndex);
        Qt.callLater(root._deliverPendingWallpaper);
    }

    function _deliverPendingWallpaper() {
        if (root.currentIndex !== root._wallpapersIndex || root._pendingWallpaperId.length === 0)
            return;
        const page = m_content.currentItem;
        if (!page || typeof page.openWallpaper !== "function")
            return;
        const wallpaperId = root._pendingWallpaperId;
        root._pendingWallpaperId = "";
        page.openWallpaper(wallpaperId);
    }

    // ── Шапка вендорной страницы (MD.AppBar) ──────────────────────────
    //
    // ⚠️ Без этого куска вендорная страница, которая у waywallen живёт во
    // ВСПЛЫВАЮЩЕМ окне, открывается у нас БЕЗ заголовка и БЕЗ ВСЕХ кнопок
    // действий. Шапку `MD.Page` рисует по `MD.MProp.page.showHeader`
    // (`Qcm.Material/qml/control/Page.qml`, строка 25) — то есть по
    // свойству ХОЗЯИНА, а не своему. Хозяина у нашей страницы не было
    // вовсе, и ей доставался глобальный `PageContext` со значением по
    // умолчанию `false` (`src/control/page_context.cpp`, строка 71).
    //
    // У waywallen хозяев два, и решают они по-разному: контейнер страниц
    // рейла (`ui/qml/Window.qml`) ставит `showHeader: false`, а
    // `component/PagePopup.qml` — `showHeader: true`. Страницы рейла либо
    // шапки не имеют, либо включают её себе сами (`SettingsPage.qml`), а
    // страницы всплывающего окна на своё `showHeader` не полагаются.
    //
    // Наш рейл держит и те и другие, поэтому шапку включает хозяин, а
    // знание «какая страница чья» приходит из ядра одним ответом на обе
    // версии вкладки (`we_window::RailItem::header`). Поймано живым
    // снимком 08.09.2026: у «Плагинов» рисовался список плагинов, но не
    // было ни заголовка, ни кнопок «Неактивные плагины», «Проверить
    // обновления» и «Установить из .zip» — то есть поставить и обновить
    // плагин было нечем, а снимок при этом выглядел рабочим.
    //
    // Фон страницы — как у контейнера страниц в `Window.qml` вендора:
    // карточка со скруглением, а на узкой вкладке — без фона и скругления.
    // `leadingAction` пуст осознанно: у waywallen это кнопка «назад»,
    // закрывающая всплывающее окно, а в рейле закрывать нечего —
    // `MD.AppBar` прячет её сам (`m_leading.visible: action`).
    property bool pageHeader: false

    MD.MProp.page: m_page_context

    MD.PageContext {
        id: m_page_context
        showHeader: root.pageHeader
        backgroundRadius: root.isCompact ? 0 : MD.Token.shape.corner.large
        showBackground: !root.isCompact
    }

    // Светлый/тёмный тон интерфейса задаёт СКИН Rexon (host.py передаёт
    // яркость палитры окна), а НЕ системная тема рабочего стола. Material
    // v0.2.5 по умолчанию тянет режим из XDG-портала (useSysColorSM=true):
    // когда портал/styleHints рапортует Light (светлый или неопределённый
    // рабочий стол), а у Rexon выбран тёмный скин steam_dark — весь текст
    // (заголовки диалогов, метки чипов, метки боковой панели, имена
    // мониторов, кнопки диалогов) считается по светлой схеме и становится
    // тёмно-серым/почти чёрным на тёмном фоне скина. Привязываем режим
    // Material к скину, чтобы контраст текста был правильным при любой теме
    // рабочего стола. По умолчанию тёмный — дефолтный скин steam_dark.
    property bool skinDark: true

    // Чьими красками рисовать интерфейс (решает ядро,
    // `we_window::vendor_theme`).
    //
    // По умолчанию — скином Rexon, как остальные вкладки: схему Material
    // плагин СЧИТАЕТ из затравки, и затравкой ставится акцент скина
    // (тип палитры «по содержимому» — у него основной контейнер ровно
    // этого цвета), а фон и текст шелла берутся из скина как есть. Фоны,
    // поверхности и текст схемы (карточки, диалоги, поля) поверх неё
    // задаются красками скина поштучно — `skinRoles` в
    // `MD.Token.color.overrides`. У QmlMaterial без этого свойства шаг
    // молча пропускается, и карточки остаются тонами схемы от акцента.
    //
    // `followVendorTheme` — скин «Тёмный (Material)»: шелл не трогает ни тип
    // палитры, ни акцент, ни тему, и действуют собственные настройки
    // waywallen («Тема», «Акцент» в его окне «Настройки») — как в его окне.
    property bool followVendorTheme: false
    property color skinAccent: "#66c0f4"
    property color skinBackground: "#1b2838"
    property color skinText: "#ffffff"
    // Роли схемы красками скина: объект JSON «роль → #rrggbb» от ядра.
    property string skinRoles: "{}"
    readonly property var _skinRolesMap: {
        try {
            return JSON.parse(root.skinRoles);
        } catch (e) {
            return {};
        }
    }
    // Есть ли у MdColorMgr свойство `overrides` (QmlMaterial с ним).
    readonly property bool _hasColorOverrides: "overrides" in MD.Token.color

    function _applyRoles(roles) {
        if (root._hasColorOverrides)
            MD.Token.color.overrides = roles;
    }

    MD.MProp.size.width: width
    MD.MProp.backgroundColor: root.followVendorTheme
        ? MD.MProp.color.surface_container : root.skinBackground
    MD.MProp.textColor: root.followVendorTheme
        ? MD.MProp.color.getOn(MD.MProp.backgroundColor) : root.skinText

    // Принудительно держим режим Material = скин Rexon и глушим слежку за
    // системным порталом. XDG-портал (org.freedesktop.appearance) может
    // прислать ответ color-scheme АСИНХРОННО, уже ПОСЛЕ первого кадра —
    // если в этот момент режим отвяжется от скина, on_surface/textColor
    // пересчитаются по светлой схеме и текст диалогов/листов/боковой панели
    // становится тёмно-серым на тёмном фоне. Поэтому переприменяем режим не
    // один раз, а ещё и коротким таймером, перекрывая окно гонки с порталом.
    function _applyMode() {
        if (root.followVendorTheme)
            return;
        MD.Token.color.useSysAccentColor = false;
        MD.Token.color.useSysColorSM = false;
        MD.Token.color.paletteType = MD.Enum.PaletteContent;
        MD.Token.color.accentColor = root.skinAccent;
        MD.Token.color.mode = root.skinDark ? MD.Enum.Dark : MD.Enum.Light;
        root._applyRoles(root._skinRolesMap);
    }

    // Схема самого waywallen: то, что ставит его `Global` по своим
    // настройкам. Пока `Global` не родился (демон ещё не ответил) — его
    // значения по умолчанию: системные тема и акцент, нейтральная палитра
    // (тип по умолчанию у плагина). Родившись, `Global` сам применит
    // сохранённый выбор. Если он уже жив (скин сменили при открытых
    // страницах) — его выбор применяется заново: иначе осталась бы тема и
    // акцент Rexon до следующего запуска.
    function _applyVendorTheme() {
        root._applyRoles({});
        MD.Token.color.paletteType = MD.Enum.PaletteNeutral;
        MD.Token.color.useSysColorSM = true;
        MD.Token.color.useSysAccentColor = true;
        if (root._vendorGlobalAlive) {
            W.Global.setThemeMode(W.Global.themeMode);
            W.Global.setAccentMode(W.Global.accentMode);
        }
    }

    // Живы ли страницы, а с ними синглтон `W.Global`. Сам `W.Global` здесь
    // не спрашивается: спрос его и породил бы, раньше, чем у оригинала.
    readonly property bool _vendorGlobalAlive: root.pagesReady && root._shownPageUrl.toString().length > 0

    onFollowVendorThemeChanged: {
        if (root.followVendorTheme)
            root._applyVendorTheme();
        else
            root._applyMode();
    }
    onSkinAccentChanged: root._applyMode()
    on_SkinRolesMapChanged: root._applyMode()

    // ── Всплывающие страницы вендора (presentPopup) ────────────────────
    //
    // ⚠️ Без этого куска у вкладки МЁРТВЫ все всплывающие страницы: лист
    // плейлистов и его редактор, диалоги фильтров обоев и Мастерской,
    // выбор меток, «Библиотеки», карточка обоины из Мастерской, страница
    // плагинов. Вендорные страницы зовут `Window.window.presentPopup(...)`
    // — метод их `Window.qml`, роль которого играет этот файл. Нашего
    // метода у окна не было ВОВСЕ, и каждое такое нажатие молча падало в
    // «TypeError: Property 'presentPopup' ... is not a function».
    // Дефект жил с вендора 0.3.5 и найден живым прогоном 0.3.7.
    //
    // Почему функция вешается на ОКНО, а не объявлена здесь: `Window.window`
    // внутри QQuickWidget — это его закадровое окно (QQuickWidgetOffscreenWindow),
    // чужой C++-тип, и подменить его нечем. Зато обёртка QObject в движке QML
    // расширяема: свойство, дописанное в неё из JS, живёт вместе с обёрткой.
    // Обёртку держит живой `hostWindow` — без сильной ссылки сборщик мусора
    // движка выбросил бы её вместе с дописанным методом.
    //
    // ⚠️ ОКНО ПОЯВЛЯЕТСЯ НЕ СРАЗУ, и спросить его ОДИН раз недостаточно.
    // Здесь стояло присвоение на `Component.onCompleted` — и в ВИДЖЕТНОЙ
    // версии вкладки оно не срабатывало НИКОГДА: шелл живёт внутри
    // QQuickWidget, а тот сперва создаёт корневой объект (тогда и приходит
    // `onCompleted`, `Window.window` ещё пуст) и лишь ПОТОМ кладёт его в
    // своё закадровое окно. Проверка `if (!w) return;` тихо выходила, и у
    // окна не оказывалось ни `presentPopup`, ни `showChangelog` — то есть
    // у виджетной версии были мертвы ВСЕ всплывающие страницы вендора, а
    // не одна кнопка. Найдено живой пробой 05.09 нажатием «Журнала
    // изменений»: «TypeError: Property showChangelog of object
    // QQuickWidgetOffscreenWindow ... is not a function».
    // Поэтому окно не спрашивается, а ОТСЛЕЖИВАЕТСЯ привязкой: методы
    // ставятся, как только окно появилось, и переставляются, если оно
    // сменилось.
    readonly property var hostWindow: root.Window.window
    onHostWindowChanged: root._installWindowApi()

    readonly property alias popupPresenter: m_popup_presenter

    // Порт `Window.presentPopup` вендора один-в-один: отказ показа уходит
    // тостом, а не тонет.
    function presentPopup(source, properties) {
        const presentation = m_popup_presenter.present(source, properties || {});
        presentation.failed.connect(presentation, function (error) {
            W.Global.toastError(error);
        });
        if (presentation.status === MD.PopupPresentation.Error)
            W.Global.toastError(presentation.errorString);
        return presentation;
    }

    function _installWindowApi() {
        const w = root.hostWindow;
        if (!w)
            return;
        w.popupPresenter = m_popup_presenter;
        w.presentPopup = root.presentPopup;
        w.showChangelog = root.showChangelog;
    }

    // ── Журнал изменений движка (showChangelog) ───────────────
    //
    // Второй метод их окна. Кнопка «Журнал изменений» страницы
    // «О движке» (`page/AboutPage.qml`) зовёт
    // `Window.window.showChangelog()`, и нажатие падало в «TypeError:
    // Property showChangelog ... is not a function» — ОДИНАКОВО у обеих
    // версий вкладки, то есть это не расхождение переноса, а провал
    // встраивания (приёмка раздела 05.09).
    //
    // ⚠️ Класс тот же, что у `presentPopup` выше, поэтому список
    // закрыт ЦЕЛИКОМ, а не по одному (правило #12). Греп
    // `Window.window` по вендорному дереву даёт ровно четыре
    // вещи, которые страницы спрашивают у окна-хозяина:
    //   * `presentPopup(source, properties)` — 15 вызовов, есть;
    //   * `popupPresenter` — его же передают диалогам фильтров
    //     свойством `popupWindow` (`DiscoverPage`, `WallpaperPage`),
    //     и внутри они зовут тот же `presentPopup` — есть;
    //   * `showChangelog()` — ставится здесь;
    //   * `height` (`Window.window?.height` в `WallpaperDetailPanel`) —
    //     родное свойство окна, ставить нечего.
    // Больше в вендорном дереве обращений к окну нет ни одного.
    //
    // Источник списка выпусков — тот же адрес, что у вендора:
    // `qrc:/waywallen/ui/assets/waywallen-ui.releases.xml`. У апстрима он
    // в ресурсах его бинаря, у нас — в ресурсах rexon-gui
    // (`rust/rexon-gui/waywallen_assets.qrc`, файл вендорный, MIT, тот же
    // тег — см. `waywallen/ATTRIBUTION.md`). Адрес не зависит от того,
    // где на диске лежит их разметка.
    //
    // Подпись окна берём из каталога переводов ВЕНДОРА тем же
    // контекстом, каким её переводит их `Window.qml`
    // (`qsTranslate("Window", …)`): свой `qsTr()` получил бы контекст
    // «PageHost», которого в их `.ts` нет, и заголовок остался бы
    // английским.
    property var _changelogPresentation: null

    function showChangelog() {
        if (root._changelogPresentation?.active)
            return;
        root._changelogPresentation = root.presentPopup(m_changelog_dialog, {
            source: "qrc:/waywallen/ui/assets/waywallen-ui.releases.xml",
            title: qsTranslate("Window", "Changelog")
        });
    }

    Component {
        id: m_changelog_dialog

        MD.ChangelogDialog {}
    }

    // То же, что `Component.onDestruction` их окна: незакрытое окно
    // пережило бы свой шелл при смене вкладки.
    Component.onDestruction: {
        root._changelogPresentation?.cancel();
        root._aboutPresentation?.cancel();
    }

    MD.PopupPresenter {
        id: m_popup_presenter
        // У вендора хозяин — `win.contentItem`; здесь окно чужое, поэтому
        // хозяин — сам шелл: он и так растянут на всю вкладку.
        host: root
    }

    // ── Слой окон поверх — только над вкладкой ────────────────────────
    //
    // Окна поверх, их затемнение, меню и подсказки Material кладёт в ОДИН
    // слой на всё окно (`Qcm.Material.Overlay`: ребёнок `contentItem` окна,
    // размером с окно). У waywallen окно и есть весь интерфейс, а у нас в том
    // же окне над вкладкой стоят разделы Rexon — и затемнение «Плагинов»,
    // «Настроек» и «О программе» накрывало их тоже, а окно поверх на узкой
    // вкладке раскрывалось на всё окно Rexon.
    //
    // Поэтому слой ужимается до шелла: так всё, что в нём живёт, считает
    // «окном» вкладку обоев — как окно оригинала накрывает только своё
    // содержимое. Размер слою библиотека возвращает сама при каждой смене
    // размера окна, поэтому он не привязывается, а подгоняется заново по
    // каждому изменению.
    //
    // Слой ещё и обрезает содержимое по себе (`clip`). Дело не в картинке:
    // под модальным окном Material кладёт в слой невидимую преграду для
    // наведения, и «попал ли в неё указатель» она отвечает по всему окну, а
    // не по своему размеру — разделы Rexon над вкладкой переставали
    // подсвечиваться под мышью. Обрезанный слой события наведения вне себя
    // детям не раздаёт, и разделы подсвечиваются как обычно.
    //
    // Нажатия вне вкладки при открытом окне поверх — отдельная половина: их
    // пропускает окну хозяин (`WallpaperEnginePage.qml`).
    readonly property Item popupLayer: MD.Overlay.overlay

    function _fitPopupLayer() {
        const layer = root.popupLayer;
        if (!layer || !layer.parent || root.width <= 0 || root.height <= 0)
            return;
        if (!layer.clip)
            layer.clip = true;
        const at = root.mapToItem(layer.parent, 0, 0);
        if (layer.x !== at.x)
            layer.x = at.x;
        if (layer.y !== at.y)
            layer.y = at.y;
        if (layer.width !== root.width)
            layer.width = root.width;
        if (layer.height !== root.height)
            layer.height = root.height;
    }

    onPopupLayerChanged: root._fitPopupLayer()
    onWidthChanged: root._fitPopupLayer()
    onHeightChanged: root._fitPopupLayer()
    onVisibleChanged: root._fitPopupLayer()

    Connections {
        target: root.popupLayer
        function onWidthChanged() {
            root._fitPopupLayer();
        }
        function onHeightChanged() {
            root._fitPopupLayer();
        }
    }

    // Шелл сдвигают и предки (полоса разделов Rexon меняет высоту вместе с
    // шириной окна): положение в окне сверяется и по смене размеров окна.
    Connections {
        target: root.hostWindow
        ignoreUnknownSignals: true
        function onWidthChanged() {
            Qt.callLater(root._fitPopupLayer);
        }
        function onHeightChanged() {
            Qt.callLater(root._fitPopupLayer);
        }
    }

    Component.onCompleted: {
        if (root.followVendorTheme)
            root._applyVendorTheme();
        else
            root._applyMode();
        root._installWindowApi();
        root._switchPage();
        root._fitPopupLayer();
    }

    onSkinDarkChanged: root._applyMode()

    // ⚠️ Таймера выше мало. Синглтон вендора `W.Global` при рождении сам
    // ставит режим по своей настройке «Тема» (`setThemeMode` →
    // `useSysColorSM = true` для «Системной»), а рождается он, когда его
    // впервые спросит страница — то есть когда демон ответил. Демон 0.4.4
    // при первом запуске минуту прогоняет миграции базы, таймер к этому
    // времени давно молчит, и вся вкладка оставалась СВЕТЛОЙ на тёмном
    // скине Rexon (снимок стенда 09.10.2026). Поэтому режим возвращается к
    // скину на каждый чужой перевод, а не только в первые две секунды.
    Connections {
        target: MD.Token.color
        function onUseSysColorSMChanged() {
            if (MD.Token.color.useSysColorSM)
                root._applyMode();
        }
        function onModeChanged() {
            if (MD.Token.color.mode !== (root.skinDark ? MD.Enum.Dark : MD.Enum.Light))
                root._applyMode();
        }
        // Тот же `Global` при рождении ставит и акцент по своей настройке
        // «Акцент» (системный или свой) — на скине Rexon акцент остаётся
        // акцентом скина.
        function onUseSysAccentColorChanged() {
            if (MD.Token.color.useSysAccentColor)
                root._applyMode();
        }
        function onAccentColorChanged() {
            if (!Qt.colorEqual(MD.Token.color.accentColor, root.skinAccent))
                root._applyMode();
        }
    }

    // Догоняем поздний асинхронный ответ портала (несколько тактов подряд).
    Timer {
        running: true
        repeat: true
        interval: 250
        triggeredOnStart: true
        property int _ticks: 0
        onTriggered: {
            root._applyMode();
            if (++_ticks >= 8)
                running = false;   // ~2 c достаточно, дальше портал молчит
        }
    }

    Rectangle {
        anchors.fill: parent
        color: root.MD.MProp.backgroundColor
    }

    // ── Страницы и их кэш ─────────────────────────────────────────────
    //
    // Страницы держит `MD.PageContainer` — тот же контейнер, что в
    // `Window.qml` вендора, и зовётся он так же: `switchTo(адрес, {}, кэш)`.
    // «Обои» и «Обзор» живут в кэше контейнера (`pageCacheable` вендора):
    // уход на другую страницу их не разрушает, и по возвращении на месте
    // прокрутка, выбранная обоина, набранный поиск и выдача Мастерской.
    // «Мониторы» и «Состояние» строятся заново при каждом заходе — тоже как
    // у вендора.
    //
    // Грузит страницы пул контейнера (`MD.Pool`, QQmlIncubator) асинхронно:
    // синхронная загрузка останавливала бы главный поток до конца
    // компиляции страницы.
    readonly property var _cacheablePages: ["wallpapers_qml", "discover_qml"]
    // Адрес страницы, которую контейнер уже показывает (или грузит).
    property string _shownPageUrl: ""

    onPagesReadyChanged: Qt.callLater(root._switchPage)

    // Зовётся отложенно: адрес страницы и номер пункта хозяин присылает
    // двумя привязками, и к моменту вызова должны прийти оба.
    function _switchPage() {
        const url = String(root.pageUrl);
        // Пустой адрес — открыт пункт без вендорной страницы («Разраб.»):
        // прежняя страница остаётся в контейнере, хозяин её просто прячет.
        if (!root.pagesReady || url.length === 0 || url === root._shownPageUrl)
            return;
        const item = root.railItems[root.currentIndex];
        root._shownPageUrl = url;
        m_content.switchTo(url, {}, !!item && root._cacheablePages.indexOf(item.id) >= 0);
    }
    Item {
        id: m_page_area
        anchors.left: parent.left
        anchors.leftMargin: root.railWidth
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: m_bar_loader.top
        anchors.topMargin: root.pageTopInset
        visible: root.pageVisible
        clip: true

        MD.PageContainer {
            id: m_content
            anchors.fill: parent
            initialItem: Item {}

            onCurrentItemChanged: Qt.callLater(root._deliverPendingWallpaper)
            onPageLoadFailed: error => {
                // Отказ не должен запирать страницу: повторный заход на
                // пункт обязан попробовать снова.
                root._shownPageUrl = "";
                W.Global.toastError(error);
            }
        }
    }

    // ── Сама рейка ─────────────────────────────────────────────────────
    //
    // Разметка — `Window.qml` вендора, блок «Navigation rail», строка в
    // строку; отличия помечены словом «REXON».
    Loader {
        id: m_rail_loader
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        active: root.railItems.length > 0 && !root.isCompact
        visible: active

        sourceComponent: MD.NavigationRail {
            id: m_rail
            model: root._railPages
            // REXON: у вендора `win.currentPage`; у нас выбранным бывает и
            // пункт не из списка рейки («Разраб.»), поэтому подсветку ведёт
            // хозяин. С QmlMaterial 0.5.0 рейка сама `currentIndex` не пишет,
            // а только просит (`currentIndexChangeRequested`), и обычной
            // привязки хватает.
            currentIndex: root._railCurrent

            autoExpand: W.Global.sidebarAutoExpand
            // The rail only re-syncs `expanded` on window-class
            // changes; apply a runtime toggle of the setting at once.
            onAutoExpandChanged: if (autoExpand)
                expanded = useEmbed

            onCurrentIndexChangeRequested: function (index) {
                root.pageRequested(root._railPages[index].page);
            }

            // Logo + a menu-toggle button (the rail's default header
            // is just the toggle; we add branding alongside it).
            header: Item {
                implicitWidth: MD.Util.lerp(m_rail.collapsedWidth, m_rail.expandedWidth, m_rail.expansionProgress)
                implicitHeight: m_logo.y + m_logo.height + 12

                MD.StandardIconButton {
                    id: m_menu_btn
                    x: MD.Util.lerp((m_rail.collapsedWidth - width) / 2, 32 - (width - 24) / 2, m_rail.expansionProgress)
                    y: 4
                    icon.name: m_rail.useLarge ? MD.Token.icon.menu_open : MD.Token.icon.menu
                    onClicked: m_rail.toggle()
                }

                // REXON: единственный наш пункт — страница сборки для
                // разработчика. В развёрнутой рейке он стоит здесь, значком
                // в свободном углу шапки, а не пунктом внизу: с лишним
                // пунктом развёрнутая рейка при наименьшей высоте окна Rexon
                // не помещалась, и «О программе» обрезался снизу. Так
                // развёрнутая рейка совпадает с вендорной пункт в пункт. В
                // свёрнутой рейке «О программе» нет и места хватает — там
                // пункт остаётся внизу (см. подвал). Кнопка — переключатель
                // (`IconToggleButton`, как у вендора всё, что светится
                // выбранным): у `StandardIconButton` с QmlMaterial 0.5.0
                // свойства `checked` нет.
                MD.IconToggleButton {
                    id: m_dev_btn
                    visible: root._devIndex >= 0 && opacity > 0
                    opacity: m_rail.expansionProgress
                    enabled: m_rail.expansionProgress === 1
                    x: m_rail.expandedWidth - width - (32 - (width - 24) / 2)
                    y: m_menu_btn.y
                    icon.name: MD.Token.icon.build
                    checked: root._devIndex >= 0 && root.currentIndex === root._devIndex
                    onCheckedChangeRequested: {
                        root.pageRequested(root._devIndex);
                        if (m_rail.useModal)
                            m_rail.close();
                    }

                    MD.ToolTip.visible: mdState.interactions.hovered
                    MD.ToolTip.delay: 300
                    MD.ToolTip.text: root._devIndex >= 0 ? root.railItems[root._devIndex].label : ""
                }

                Image {
                    id: m_logo
                    width: 32
                    height: 32
                    x: MD.Util.lerp((m_rail.collapsedWidth - width) / 2, 32, m_rail.expansionProgress)
                    y: m_menu_btn.y + m_menu_btn.height + 16
                    // Адрес вендора; у нас файл в ресурсах rexon-gui
                    // (`rust/rexon-gui/waywallen_assets.qrc`).
                    source: "qrc:/waywallen/ui/assets/waywallen-ui.svg"
                    fillMode: Image.PreserveAspectFit
                    sourceSize.width: 64
                    sourceSize.height: 64

                    // REXON: значка «есть обновление» на логотипе нет
                    // намеренно. У вендора его зажигает `W.UpdateChecker`
                    // (новый выпуск waywallen на GitHub), а нажатие ведёт на
                    // «О программе» к кнопке «Доступна версия N». Движок у нас
                    // едет внутри пакета Rexon и обновляется вместе с ним:
                    // проверки выпусков нет (`W.UpdateChecker.supported` —
                    // ложь, и кнопки на «О программе» вендор прячет сам), и
                    // значок звал бы за обновлением, которого пакет не
                    // увидит. Проверка выпуска для разработчика живёт на
                    // странице «Разраб.» и показывает итог там же.
                }

                MD.Label {
                    visible: opacity > 0
                    opacity: m_rail.expansionProgress
                    anchors.left: m_logo.right
                    anchors.leftMargin: 12
                    anchors.verticalCenter: m_logo.verticalCenter
                    text: "waywallen"
                    typescale: MD.Token.typescale.title_large
                }
            }

            footer: Item {
                implicitHeight: m_rail_footer.implicitHeight + m_about.height + m_rail_footer.spacing * m_rail.expansionProgress

                Column {
                    id: m_rail_footer
                    width: parent.width
                    spacing: 12 * (1 - m_rail.expansionProgress)

                    W.SidebarNowPlaying {
                        id: m_now_playing
                        width: parent.width
                        visible: W.App.presentationManager.count > 0
                        expanded: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        model: W.App.presentationManager.model
                        onOpenRequested: wallpaperId => root.openWallpaper(wallpaperId)

                        Binding {
                            target: m_rail
                            property: "drawerGestureEnabled"
                            value: !m_now_playing.pointerHovered
                        }
                    }

                    MD.RailItem {
                        width: parent.width
                        visible: root._railIndex("plugins_qml") >= 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: false
                        icon.name: MD.Token.icon.extension
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: qsTranslate("Window", "Plugins")
                        property var presentation: null
                        onSelectionRequested: {
                            if (presentation?.active)
                                return;
                            presentation = root.presentPopup('waywallen.ui/PagePopup', {
                                source: 'waywallen.ui/PluginManagePage'
                            });
                        }
                    }

                    MD.RailItem {
                        width: parent.width
                        visible: root._railIndex("settings_qml") >= 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: false
                        icon.name: MD.Token.icon.settings
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: qsTranslate("Window", "Settings")
                        property var presentation: null
                        onSelectionRequested: {
                            if (presentation?.active)
                                return;
                            presentation = root.presentPopup('waywallen.ui/PagePopup', {
                                source: 'waywallen.ui/SettingsPage'
                            });
                        }
                    }

                    // REXON: наш пункт «Разраб.» в свёрнутой рейке. Это
                    // страница, а не окно поверх, поэтому пункт подсвечивается,
                    // пока она открыта. При развороте рейки он сходит на нет —
                    // зеркально пункту «О программе» — и уступает место значку
                    // в шапке.
                    MD.RailItem {
                        width: parent.width
                        visible: root._devIndex >= 0 && opacity > 0
                        opacity: 1 - m_rail.expansionProgress
                        height: implicitHeight * (1 - m_rail.expansionProgress)
                        enabled: m_rail.expansionProgress === 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: root._devIndex >= 0 && root.currentIndex === root._devIndex
                        icon.name: MD.Token.icon.build
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: root._devIndex >= 0 ? root.railItems[root._devIndex].label : ""
                        onSelectionRequested: root.pageRequested(root._devIndex)
                    }
                }

                MD.RailItem {
                    id: m_about
                    visible: opacity > 0 && root._railIndex("about_qml") >= 0
                    opacity: m_rail.expansionProgress
                    width: parent.width
                    y: m_rail_footer.height + m_rail_footer.spacing * m_rail.expansionProgress
                    expand: true
                    selected: false
                    icon.name: MD.Token.icon.info
                    text: qsTranslate("Window", "About")
                    height: implicitHeight * m_rail.expansionProgress
                    enabled: m_rail.expansionProgress === 1
                    onSelectionRequested: root.showAbout()
                }
            }
        }
    }

    // ── Нижняя полоса навигации (узкая вкладка) ───────────────────────
    //
    // Разметка — `Window.qml` вендора, блок «Bottom navigation bar». Как и
    // там, в полосе только страницы: «Плагины», «Настройки» и «О программе»
    // у вендора на узком окне из навигации пропадают.
    Loader {
        id: m_bar_loader
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        active: root.railItems.length > 0 && root.isCompact
        visible: active

        sourceComponent: MD.Pane {
            padding: 0
            backgroundColor: MD.MProp.color.surface_container
            elevation: MD.Token.elevation.level2

            contentItem: RowLayout {
                Repeater {
                    model: root._railPages

                    Item {
                        Layout.fillWidth: true
                        implicitHeight: 12 + children[0].implicitHeight + 16
                        required property var modelData
                        required property int index

                        MD.BarItem {
                            anchors.fill: parent
                            anchors.topMargin: 12
                            anchors.bottomMargin: 16
                            icon.name: parent.modelData.icon
                            text: parent.modelData.name
                            selected: root._railCurrent === parent.index
                            onSelectionRequested: root.pageRequested(parent.modelData.page)
                        }
                    }
                }
            }
        }
    }

    // ── Вложенные страницы (pushItem / pop контекста страницы) ─────────
    //
    // Своей стопки у шелла нет — как нет её у контейнера страниц рейки в
    // `Window.qml` вендора. `pushItem` зовут три вендорные страницы
    // («Настройки» → «Исключённые окна», «Библиотеки» → «Добавить
    // библиотеку»), и все они открываются окном поверх
    // (`component/PagePopup.qml`): оно само слушает сигналы своего контекста
    // и ведёт стопку — с кнопкой «назад», заголовком и возвратом состояния.
    // Прежняя «простая стопка» шелла осталась от времён, когда «Настройки»
    // были страницей рейки; с переездом в окно поверх до неё не доходил ни
    // один вызов.
    onPageUrlChanged: Qt.callLater(root._switchPage)

    // Вход по QR-коду — как в `Window.qml` вендора: окно живёт в шелле и
    // само открывается по событию демона `qrLoginProgress`. Событие шлёт
    // плагин-источник, у которого есть действие входа (вход в Steam по
    // QR-коду у плагина Мастерской); без этого окна нажатие запускало вход,
    // а показать код было нечем.
    W.QrLoginDialog {
        // REXON: у вендора цвет текста и фона окну входа достаются по
        // наследству от главного окна (`Window.qml` ставит их на себе). Наш
        // шелл — не окно, и до слоя окон поверх его цвета не доходят: подпись
        // под кодом выходила чёрной на тёмном. Те же два значения — явно.
        MD.MProp.textColor: root.MD.MProp.textColor
        MD.MProp.backgroundColor: root.MD.MProp.backgroundColor
    }

    // ── Ctrl+P: приостановить или возобновить все обои ────────────────
    //
    // Порт сочетания из `Window.qml` вендора: общий переключатель паузы и
    // тост «Приостановлено» / «Возобновлено».
    //
    // REXON: у вендора сочетание действует во всём приложении
    // (`Qt.ApplicationShortcut`) — там приложение и есть обои. У нас в том же
    // окне живут другие разделы Rexon, и пауза обоев из «Оформления» или
    // «Настроек» была бы неожиданностью: сочетание действует, только пока
    // вкладка обоев на экране.
    W.GlobalPauseToggleQuery {
        id: m_pause_toggle
        onToggled: paused => W.Action.toast(paused ? qsTranslate("Window", "Paused") : qsTranslate("Window", "Resumed"))
    }

    Shortcut {
        sequence: "Ctrl+P"
        context: Qt.WindowShortcut
        enabled: root.visible && W.Notify.daemonPhase === W.Notify.DaemonPhase.Ready && !m_pause_toggle.querying
        onActivated: m_pause_toggle.reload()
    }

    // Тосты — как в `Window.qml` вендора: в слое окон поверх, с полями по
    // 24 слева и справа. Слой шелл ужимает до вкладки (см. выше), поэтому
    // тост ложится на вкладку обоев, а не на разделы Rexon над ней.
    MD.SnakeView {
        id: m_snake
        parent: MD.Overlay.overlay
        anchors.fill: parent
        anchors.leftMargin: 24
        anchors.rightMargin: 24
    }

    // ⚠️ Сигнал зовётся `toast`, а НЕ `toasted`. Прежнее имя осталось от
    // питоновского моста, который переименовал сигнал оригинала и тем
    // разошёлся с вендорным `Window.qml`. Мост давно на Rust и сигнал у
    // него как в оригинале — а обработчик здесь остался старым, и ни один
    // тост вкладки не показывался НИ РАЗУ: `Connections` на несуществующий
    // сигнал молча ничего не делает.
    Connections {
        target: W.Action
        function onToast(text, duration, flags, action) {
            m_snake.show(text, duration, flags, action);
        }
    }

    // Ошибка транспорта — тем же тостом, что у вендорного `Window.qml`:
    // обрыв сокета и отказ запроса иначе не доходят до человека вовсе.
    //
    // Сообщение, а не ошибка (например, «живые обои не запустились — прежние
    // обои возвращены»), идёт своим сигналом и гаснет само: срок считает ядро
    // (`ui_base::toast_timeout_for_text`). Тост ошибки у вендора без срока —
    // такое сообщение висело больше минуты поверх кнопок панели, в том числе
    // фильтра (ВМ, 30.10.2026).
    Connections {
        target: W.App
        function onErrorOccurred(error) {
            W.Global.toastError(error);
        }
        function onNoticeOccurred(text, durationMs) {
            W.Action.toast(text, durationMs, MD.Enum.TFCloseable, null);
        }
    }

    // События демона, о которых окно вендора сообщает тостом с любой
    // страницы (`Window.qml`, «Global daemon-event toasts»). Подписи — из
    // каталога перевода вендора, его же контекстом.
    Connections {
        target: W.Notify
        function onLibrariesAdded(paths) {
            const n = paths.length;
            W.Action.toast(qsTranslate("Window", "%n library(s) added", "", n));
        }
        function onDisplayConnectionFailed(clientName, clientProtocolVersion, errorCode, reason) {
            const who = clientName.length > 0 ? clientName : qsTranslate("Window", "Display client");
            W.Global.toastError(qsTranslate("Window", "%1 connection failed: %2").arg(who).arg(reason));
        }
        function onPluginRestartFailed(pluginId, error) {
            const who = pluginId.length > 0 ? pluginId : qsTranslate("Window", "Plugin");
            W.Action.toast(qsTranslate("Window", "%1 renderer restart failed: %2").arg(who).arg(error), 6000, 1, null);
        }
    }
}
