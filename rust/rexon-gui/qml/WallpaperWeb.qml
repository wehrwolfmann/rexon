// Веб-обои Rexon: ПО ОКНУ НА КАЖДЫЙ ЭКРАН, один процесс на обоину.
//
// Замена удалённого `rexon/web_wallpaper.py` (791 строка, три почти
// одинаковых QML-файла: WebWallpaper.qml, WebWallpaperX11.qml,
// OffscreenWebWallpaper.qml). Разница между ними была ровно в том, как окно
// попадает на задний план; как только слой ставит Rust, а не присоединённое
// свойство QML, эта разница из разметки уходит целиком — так же, как уже
// сделано для видео (WallpaperVideo.qml). Правило #12: общий механизм
// вместо копии на сценарий.
//
// ⚠️ Второй монитор (16.09.2026). До этого корнем файла было ОДНО окно
// размером с тот экран, на который его положил композитор: при двух
// мониторах обои получал ровно один. Теперь корень — держатель окон:
// `Instantiator` заводит по окну на каждый подключённый экран и убирает
// окна ушедших мониторов; появление и исчезновение монитора на ходу ловится
// сигналом `Qt.application.onScreensChanged`. Устройство ровно то же, что у
// видео-обоев, — и оно намеренно одно на оба режима.
//
// Сценарии (свойство `placement`, решает ядро `wallpaper::video_qml_file` —
// решение одно на оба хоста):
//   layer     — Wayland с wlr-layer-shell: слой BACKGROUND ставит Rust
//               (WallpaperLayer из org.rexon.gui) ДО первого показа окна,
//               выход поверхности называется явно (`pinScreen`);
//   x11       — обычные окна по числу экранов, каждое встаёт в угол своего
//               экрана, хост вешает им _NET_WM_WINDOW_TYPE_DESKTOP;
//   offscreen — GNOME/Cinnamon на Wayland: layer-shell там нет, кадры окна
//               уходят расширению оболочки через общий mmap-файл. Окно ОДНО
//               и экранов не знает: раскладывает кадр по мониторам сама
//               оболочка.
//
// Свойства приходят из Rust начальными свойствами корня
// (QQmlApplicationEngine::setInitialProperties), имена, умолчания и весь
// подмешиваемый JS считает ядро (`rexon-core::wallpaper`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import QtWebEngine
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Начальные свойства из хоста ───────────────────────────────────
    /// Что показывать: file:// на локальный HTML либо http(s)-ссылка.
    property url targetUrl
    property bool isWindowed: false
    /// Размер кадра для офскрин-режима: окна на экране нет, и размер экрана
    /// там брать неоткуда.
    property int renderWidth: 1920
    property int renderHeight: 1080
    /// «layer» | «x11» | «offscreen».
    property string placement: "layer"
    /// Общий файл кадров для расширения оболочки (офскрин-режим).
    property string mmapPath: ""
    /// Частота кадров потока — её задаёт расширение оболочки.
    property int mmapFps: 30
    /// Прогон самопроверки: окна закрываются сами, отчитавшись.
    property int selftestMs: 0
    /// Заглушки глобальных функций Wallpaper Engine. Подмешиваются точкой
    /// DocumentCreation — ДО первого скрипта самой страницы.
    property string polyfillJs: ""
    /// Свойства обоев из project.json + первый тик звука. Точка Deferred —
    /// после загрузки документа, когда обоина уже завела свой слушатель.
    property string afterLoadJs: ""

    // ── Экраны ────────────────────────────────────────────────────────
    //
    /// Экраны, на которых сейчас лежат обои. Пересобирается целиком, а не
    /// правится по одному: обёртки `Qt.application.screens` живут по
    /// НОМЕРУ и при смене конфигурации переназначаются на другие мониторы
    /// — держать их по одной значило бы держать ссылку, которая молча
    /// начала означать другое.
    property var screenList: []

    /// Кладём ли по окну на экран. Офскрин-режим и отладочное окно — нет:
    /// там окно одно и экранам не принадлежит.
    readonly property bool perScreen: root.placement !== "offscreen" && !root.isWindowed

    /// Модель окон. Непустая строка вместо экрана — «одно окно, экран ему
    /// не назначается».
    readonly property var surfaceModel: root.perScreen ? root.screenList : ["single"]

    /// Окно основного экрана: поток кадров и отчёт самопроверки — его.
    /// Основной экран в Qt идёт первым в `screens()`.
    readonly property var primarySurface: root.surfaces.count > 0
                                          ? root.surfaces.objectAt(0)
                                          : null

    // Сводные признаки для самопроверки — с окна основного экрана.
    readonly property bool layerApplied: root.primarySurface
                                         ? root.primarySurface.layerApplied : false
    readonly property bool x11Applied: root.primarySurface
                                       ? root.primarySurface.x11Applied : false
    readonly property bool mmapStarted: root.primarySurface
                                        ? root.primarySurface.mmapStarted : false
    readonly property bool viewportReady: root.primarySurface
                                          ? root.primarySurface.viewportReady : false
    readonly property int loadStatus: root.primarySurface
                                      ? root.primarySurface.loadStatus : 0
    readonly property string loadError: root.primarySurface
                                        ? root.primarySurface.loadError : ""

    // Настройщик слоя на Rust — тот же объект, что у видео-обоев, и ОДИН на
    // все окна.
    property WallpaperLayer layerShell: WallpaperLayer {
        scope: "rexon-web-wallpaper"
    }

    // Остальные два способа пристроить окно (xprop на X11 и поток кадров в
    // общий файл) — тоже общие с видео-режимом.
    property WallpaperPlacement placer: WallpaperPlacement {}

    // ── Окна обоев ────────────────────────────────────────────────────
    property Instantiator surfaces: Instantiator {
        id: surfaces
        model: root.surfaceModel

        delegate: Window {
            id: surface

            required property int index
            required property var modelData

            /// Экран этого окна; null — окно экрану не принадлежит
            /// (офскрин-режим, отладочное окно).
            readonly property var info: root.perScreen ? surface.modelData : null
            readonly property bool primary: surface.index === 0

            property bool layerApplied: false
            property bool x11Applied: false
            property bool mmapStarted: false
            /// Chromium уже знает размер окна просмотра — можно грузить
            /// страницу.
            ///
            /// ⚠️ В режиме потока кадров сцену рисует QQuickRenderControl без
            /// окна, и размер доходит до Chromium не сразу. Замер: страница,
            /// которая один раз при разборе ставит холсту
            /// `canvas.width = innerWidth` (так делает большинство
            /// WebGL-обоев), видела `innerWidth=0` — холст 0×0, кадр чёрный,
            /// хотя через секунду вид был 1920×1080. Поэтому сначала
            /// `about:blank`, и настоящий адрес — когда сама
            /// страница-заглушка видит ненулевой `innerWidth × innerHeight`
            /// (обход из QTBUG-132776: дождаться компоновщика Chromium, а не
            /// только загрузки).
            property bool viewportReady: root.placement !== "offscreen"
            /// Итог загрузки страницы: 0 — ещё не пришёл.
            property int loadStatus: 0
            property string loadError: ""
            property alias view: view

            // ⚠️ Порядок обязателен: окно создаётся НЕВИДИМЫМ, слой ставится
            // в Component.onCompleted, и только потом visible = true.
            // Показанное окно уже стало обычным xdg_toplevel и в слой не
            // переселится.
            visible: false
            color: "black"
            flags: root.isWindowed ? Qt.Window : Qt.FramelessWindowHint
            title: root.isWindowed ? qsTranslate("rexon", "Rexon — web wallpaper") : ""

            // ⚠️ Экран назначается ДО показа: и слой (LayerShellQt читает
            // QWindow::screen()), и X11 берут его именно там.
            screen: surface.info ? surface.info : null

            width: surface.info
                   ? surface.info.width
                   : (root.placement === "offscreen"
                      ? root.renderWidth
                      : (root.isWindowed ? 1280 : 1920))
            height: surface.info
                    ? surface.info.height
                    : (root.placement === "offscreen"
                       ? root.renderHeight
                       : (root.isWindowed ? 720 : 1080))

            Component.onCompleted: {
                if (root.placement === "layer") {
                    if (root.isWindowed) {
                        // Отладочное окно: поверх остальных, без якорей и без
                        // резервирования места — иначе его не видно за обоями.
                        root.layerShell.layer = 2;                  // LayerTop
                        root.layerShell.anchors = 0;                // AnchorNone
                        root.layerShell.exclusionZone = 0;
                        root.layerShell.keyboardInteractivity = 2;  // OnDemand
                        surface.layerApplied = root.layerShell.apply(surface);
                    } else {
                        surface.layerApplied =
                            root.layerShell.applyWallpaperLayer(surface);
                        // ⚠️ Выход поверхности называется ЯВНО. Без этого
                        // композитор вправе положить все окна обоев на один
                        // монитор, и второй остался бы пустым.
                        if (surface.layerApplied && !root.layerShell.pinScreen(surface))
                            console.warn("REXON-WEB-WARN обои не прицелились на экран "
                                         + (surface.info ? surface.info.name : "?"));
                    }
                } else if (root.placement === "x11" && surface.info) {
                    // На X11 окно надо поставить в угол своего экрана руками:
                    // без этого все окна обоев легли бы на основной монитор.
                    surface.x = surface.info.virtualX;
                    surface.y = surface.info.virtualY;
                }
                // ⚠️ Тип «рабочий стол» и состояние — ДО показа: тип оконный
                // менеджер читает при показе, а состояние показанного окна
                // свойством уже не меняют (EWMH). После показа — просьбы
                // корневому окну (x11Settle).
                if (root.placement === "x11" && !root.isWindowed
                        && !root.placer.prepareX11Desktop(surface))
                    console.warn("REXON-WEB-WARN окно X11 не подготовлено к роли рабочего стола");
                // ⚠️ В режиме потока кадров окно НЕ показывается никогда: хост
                // живёт на настоящей платформе сеанса (ради видеокарты), и
                // показанное окно на Mutter вышло бы поверх рабочего стола.
                // Сцену рисует QQuickRenderControl, которому шим отдаёт
                // содержимое этого окна.
                if (root.placement !== "offscreen")
                    surface.visible = true;

                if (root.placement === "offscreen") {
                    // Кадры уходят расширению оболочки (GNOME/Cinnamon на
                    // Wayland — layer-shell там нет). null вместо VideoOutput
                    // — кадр рисует сцена Qt Quick без окна, на видеокарте.
                    surface.mmapStarted = root.placer.startMmapStream(
                        surface, null, root.mmapPath, root.renderWidth,
                        root.renderHeight, root.mmapFps);
                    if (!surface.mmapStarted)
                        console.warn("REXON-WEB-ERROR поток кадров не запустился: "
                                     + root.mmapPath);
                } else if (root.placement === "x11" && !root.isWindowed) {
                    // После показа — просьбы EWMH корневому окну: так
                    // меняют состояние уже показанного окна.
                    x11Settle.start();
                }
            }

            WebEngineView {
                id: view
                anchors.fill: parent
                url: surface.viewportReady ? root.targetUrl : "about:blank"
                // Чёрный, а не прозрачный: у обоины может не быть своего фона,
                // и прозрачное окно на слое показало бы то, что под ним, — то
                // есть прежние обои. Совпадает с `color` окна, поэтому шва не
                // видно.
                backgroundColor: "black"

                // ⚠️ Заглушки WE ставятся сценарием, а не вызовом
                // runJavaScript после загрузки, и это не придирка. Обои
                // Мастерской проверяют
                // `typeof window.wallpaperRegisterAudioListener === 'function'`
                // СИНХРОННО при разборе HTML: к моменту, когда страница
                // загрузилась и можно звать runJavaScript, их скрипт уже
                // оборвался. Точка DocumentCreation — единственная, которая
                // успевает.
                //
                // Тип сценария передаётся объектом, а не элементом
                // `WebEngineScript`: в Qt 6 `QWebEngineScript` — значимый тип,
                // и в QML он объявлен строчной буквой (`webEngineScript`),
                // элементом его не создать. Числа точек внедрения оттуда же:
                // Deferred=0, DocumentReady=1, DocumentCreation=2; мир 0 —
                // главный, тот же, в котором живёт сама страница (в
                // изолированном мире `window` был бы чужой).
                Component.onCompleted: {
                    var scripts = [];
                    if (root.polyfillJs !== "")
                        scripts.push({
                            name: "rexon-we-polyfill",
                            sourceCode: root.polyfillJs,
                            injectionPoint: 2,
                            worldId: 0,
                            runsOnSubFrames: true
                        });
                    if (root.afterLoadJs !== "")
                        scripts.push({
                            name: "rexon-we-properties",
                            sourceCode: root.afterLoadJs,
                            injectionPoint: 0,
                            worldId: 0,
                            runsOnSubFrames: false
                        });
                    if (scripts.length > 0)
                        view.userScripts.collection = scripts;
                }

                onLoadingChanged: (info) => {
                    // Итог заглушки `about:blank` — не итог обоев:
                    // самопроверка иначе приняла бы пустую страницу за
                    // загруженную.
                    if (!surface.viewportReady)
                        return;
                    surface.loadStatus = info.status;
                    if (info.status === WebEngineView.LoadFailedStatus) {
                        surface.loadError = info.errorString;
                        // Молчаливый чёрный экран человеку ничего не объясняет
                        // (правило #13): что именно не загрузилось — в журнал.
                        console.warn("REXON-WEB-ERROR страница не загрузилась: "
                                     + info.url + " — " + info.errorString);
                    }
                }

                onJavaScriptConsoleMessage: (level, message, lineNumber, sourceID) => {
                    // Жалобы самой обоины видны только так: своего окна
                    // разработчика у обоев нет, а без них «обои чёрные»
                    // неотличимо от «обои упали на второй строке своего
                    // скрипта». Печатает только окно основного экрана: окон
                    // столько, сколько мониторов, и одна и та же жалоба в
                    // журнале несколько раз читается как несколько разных бед.
                    if (surface.primary && level >= WebEngineView.WarningMessageLevel)
                        console.warn("REXON-WEB-JS " + sourceID + ":" + lineNumber
                                     + " " + message);
                }
            }

            // Esc / Q — выход. Полезно в отладочном окне; на слое обоев
            // клавиатуры у окна нет вовсе, так что здесь это ни на что не
            // влияет.
            Shortcut {
                sequences: ["Escape", "Q"]
                onActivated: Qt.quit()
            }

            // Отладочное окно закрывают крестиком — и это должно значить
            // «выйти». Приложение переживает закрытие последнего окна
            // намеренно (отключённый монитор не должен убивать обои), потому
            // выход здесь называется вслух.
            onClosing: {
                if (root.isWindowed)
                    Qt.quit();
            }

            // Опрос заглушки: знает ли Chromium размер окна просмотра.
            Timer {
                interval: 50
                running: !surface.viewportReady
                repeat: true
                onTriggered: view.runJavaScript("innerWidth * innerHeight", function (area) {
                    if (!surface.viewportReady && area > 0)
                        surface.viewportReady = true;
                })
            }

            // Размер от Chromium не пришёл — грузим всё равно: пустые обои
            // хуже, чем обои, у которых холст мог встать по старому размеру.
            Timer {
                interval: 3000
                running: !surface.viewportReady
                repeat: false
                onTriggered: {
                    console.warn("REXON-WEB-WARN размер окна просмотра от Chromium не пришёл за "
                                 + interval + " мс — страница грузится без него");
                    surface.viewportReady = true;
                }
            }

            Timer {
                id: x11Settle
                interval: 150
                repeat: false
                onTriggered: surface.x11Applied = root.placer.applyX11Desktop(surface)
            }

            // Порядок относительно окна значков стола: окно значков
            // пересоздаётся (перезапуск Caja, смена её настроек) и встаёт
            // сверху — сверка ставит обои обратно (над непрозрачными
            // значками, под сквозными). Стоит где надо — ничего не делает.
            Timer {
                interval: 2000
                repeat: true
                running: root.placement === "x11" && !root.isWindowed && surface.x11Applied
                onTriggered: root.placer.keepX11Order(surface)
            }
        }
    }

    // ── Смена конфигурации мониторов ──────────────────────────────────
    property Connections screenWatch: Connections {
        target: Qt.application
        function onScreensChanged() { settle.restart(); }
    }

    /// Пауза перед пересборкой окон.
    ///
    /// ⚠️ Не украшение: перенастройка мониторов приходит НЕ одним событием
    /// — композитор гасит выход, меняет режим и зажигает снова, и каждый шаг
    /// шлёт свой `screensChanged`. Без выдержки хост пересоздавал бы все
    /// окна обоев по нескольку раз подряд — а здесь каждое окно стоит
    /// поднятого Chromium, то есть жёг бы процессор ровно тогда, когда
    /// человеку и так не до того (правило #13).
    property Timer settle: Timer {
        id: settle
        interval: 400
        repeat: false
        onTriggered: root.rebuildScreens()
    }

    /// Перечитать экраны заново. Целиком, а не по одному: см. `screenList`.
    function rebuildScreens() {
        var all = Qt.application.screens;
        var list = [];
        for (var i = 0; i < all.length; ++i)
            list.push(all[i]);
        root.screenList = list;
        console.log("REXON-WEB-SCREENS экранов=" + list.length
                    + " имена=" + root.screenNames());
    }

    /// Имена экранов через запятую — для журнала.
    function screenNames() {
        var names = [];
        for (var i = 0; i < root.screenList.length; ++i)
            names.push(root.screenList[i].name);
        return names.length > 0 ? names.join(",") : "нет";
    }

    /// Имена экранов, на которые РЕАЛЬНО легли окна, — по одному на окно.
    ///
    /// Спрашивается у самой поверхности слоя, а не у списка экранов: «экранов
    /// два» и «обои легли на оба» — разные утверждения, и первое годами
    /// выглядело как второе.
    function surfaceScreens() {
        var out = [];
        for (var i = 0; i < root.surfaces.count; ++i) {
            var s = root.surfaces.objectAt(i);
            if (!s)
                continue;
            var named = root.placement === "layer" ? root.layerShell.screenName(s) : "";
            if (named === "")
                named = s.screen ? s.screen.name : "";
            out.push(named === "" ? "?" : named);
        }
        return out.length > 0 ? out.join(",") : "нет";
    }

    Component.onCompleted: {
        root.rebuildScreens();
        if (root.selftestMs > 0)
            selftest.start();
    }

    // ── Самопроверка ──────────────────────────────────────────────────
    //
    // Отчёт печатается ОДНОЙ строкой и разбирается родительской половиной
    // стенда: «окно поднялось» и «страница загрузилась» — разные
    // утверждения, и молчание не должно выглядеть успехом.
    //
    // ⚠️ Проверяется то, что решает исход, а не то, что удобно спросить:
    // `loadStatus` у страницы с битой ссылкой и у чёрной, но целой страницы
    // разный, а вот `loadProgress` у обеих доходит до 100. И отдельно —
    // прижилась ли заглушка WE: без неё обоина Мастерской обрывается на
    // первой же проверке, и внешне это тот же чёрный экран.
    //
    // ⚠️ `screens` и `windows` — тоже не украшение: «обои поднялись» и
    // «обои легли на ВСЕ мониторы» — разные утверждения, и первое годами
    // выглядело как второе.
    property Timer selftest: Timer {
        id: selftest
        interval: root.selftestMs
        repeat: false
        onTriggered: {
            var top = root.primarySurface;
            if (!top) {
                console.log("REXON-WEB-REPORT placement=" + root.placement
                            + " screens=" + Qt.application.screens.length
                            + " windows=0 — обоям некуда лечь");
                Qt.exit(1);
                return;
            }
            top.view.runJavaScript(
                "[window.__rexonWePolyfillInstalled === true,"
                + " window.__rexonInitialTickFired === true,"
                + " document.title,"
                + " (window._rexonAudioListeners || []).length].join('|')",
                function (probe) {
                    var frames = root.placer.mmapFrames(top);
                    var lit = root.placer.mmapNonBlackPermille(top);
                    console.log("REXON-WEB-REPORT placement=" + root.placement
                                + " screens=" + Qt.application.screens.length
                                + " windows=" + root.surfaces.count
                                + " screenNames=" + root.screenNames()
                                + " windowScreens=" + root.surfaceScreens()
                                + " layerScreen=" + root.layerShell.screenName(top)
                                + " layerApplied=" + root.layerApplied
                                + " x11Applied=" + root.x11Applied
                                + " mmapStarted=" + root.mmapStarted
                                + " mmapCapture=" + root.placer.mmapCapture(top)
                                + " mmapFrames=" + frames
                                + " mmapCenterRgb=" + root.placer.mmapCenterRgb(top)
                                + " mmapNonBlackPermille=" + lit
                                + " viewportReady=" + root.viewportReady
                                + " windowW=" + top.width
                                + " windowH=" + top.height
                                + " url=" + top.view.url
                                + " loadStatus=" + root.loadStatus
                                + " loadProgress=" + top.view.loadProgress
                                + " loadError=" + root.loadError
                                + " probe=" + probe);
                    top.visible = false;
                    // Код 5 — «страница не загрузилась». Приговор по итогу
                    // загрузки, а не по проценту: у битой ссылки процент
                    // тоже доходит до ста.
                    if (root.loadStatus !== WebEngineView.LoadSucceededStatus) {
                        Qt.exit(5);
                        return;
                    }
                    // Код 3 — «поток кадров не пошёл». Чёрные кадры прежде
                    // считались успехом (WebGL-обои на offscreen), поэтому
                    // кадр во всю площадь чёрный — тоже провал: расширение
                    // показало бы человеку чёрный экран.
                    if (root.placement === "offscreen" && (frames <= 0 || lit <= 0)) {
                        console.warn("REXON-WEB-ERROR кадры потока чёрные или их нет: кадров="
                                     + frames + " не-чёрных=" + lit + "‰");
                        Qt.exit(3);
                        return;
                    }
                    Qt.exit(0);
                });
        }
    }
}
