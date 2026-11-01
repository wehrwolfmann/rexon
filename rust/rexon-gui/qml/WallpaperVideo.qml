// Видео-обои Rexon: ОДИН проигрыватель на процесс, ПО ОКНУ НА КАЖДЫЙ ЭКРАН.
//
// Раньше их было три почти одинаковых файла (rexon/qml/VideoWallpaper.qml,
// VideoWallpaperX11.qml, VideoWallpaperOffscreen.qml), и различались они
// ровно тем, как окно попадает на задний план. Как только слой ставит Rust,
// а не присоединённое свойство QML, эта разница из разметки уходит целиком —
// остаётся один проигрыватель и три способа пристроить окно, которыми
// распоряжается хост (правило #12: общий механизм вместо копии на сценарий).
//
// ⚠️ Второй монитор (16.09.2026). До этого корнем файла было ОДНО окно
// размером `Screen.width × Screen.height`, то есть размером того экрана,
// на который его положил композитор: при двух мониторах обои получал ровно
// один, второй оставался с обоями рабочего стола. Теперь корень — не окно,
// а держатель окон: `Instantiator` заводит по окну на каждый подключённый
// экран и убирает окна ушедших мониторов. Появление и исчезновение монитора
// на ходу ловится сигналом `Qt.application.onScreensChanged`.
//
// Что при этом ОСТАЛОСЬ единственным на процесс (и обязано остаться):
//   * сам процесс-хост — по обоине, а не по монитору;
//   * надзиратель звука `WallpaperAudio` — один, у корня;
//   * звучащий проигрыватель — только на ПЕРВОМ экране
//     (`QGuiApplication::screens()` отдаёт основной первым), у остальных
//     `audioOutput` не назначен вовсе, и звукового потока у них нет.
//
// Сценарии (свойство `placement`, решает ядро `wallpaper::video_qml_file`):
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
// (QQmlApplicationEngine::setInitialProperties), имена и умолчания считает
// ядро (`rexon-core::wallpaper::video_context_properties`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import QtMultimedia
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Начальные свойства из хоста ───────────────────────────────────
    property url videoUrl
    property bool isWindowed: false
    property bool audioEnabled: true
    property real audioVolume: 0.5
    property bool loopVideo: true
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
    /// Громкость, назначенная ключом `--volume`, либо -1.
    ///
    /// Ручной прогон должен звучать ровно так, как попросили в командной
    /// строке, — поэтому заданный ключ отменяет слежение за настройкой.
    /// Из продукта этот ключ не приходит: там громкость берётся из
    /// настроек, чтобы ползунок работал живьём.
    property real volumeFixed: -1

    /// Жалоба «звук просили, дорожки нет» уже прозвучала. Одна на процесс:
    /// звучит только окно основного экрана, и файл у всех окон один.
    property bool noAudioWarned: false
    /// Наблюдение за звуком уже поднято (поднимается один раз на процесс).
    property bool audioStarted: false

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
    /// не назначается»: массив с `null` внутри модель делегатов понимает
    /// хуже, чем пустой список.
    readonly property var surfaceModel: root.perScreen ? root.screenList : ["single"]

    /// Окно основного экрана: звук, поток кадров и отчёт самопроверки —
    /// его. Основной экран в Qt идёт первым в `screens()`.
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

    // Настройщик слоя на Rust — ОДИН на все окна. Умолчания у него уже слой
    // обоев, поэтому забытая настройка даёт обои, а не окно поверх всего.
    property WallpaperLayer layerShell: WallpaperLayer {
        scope: "rexon-video-wallpaper"
    }

    // Остальные два способа пристроить окно (xprop на X11 и поток кадров в
    // общий файл) — тоже из Rust, тем же приёмом «объект настраивает
    // переданное окно».
    property WallpaperPlacement placer: WallpaperPlacement {}

    // Живое состояние звука: настройки, приглушение при полноэкранном окне
    // и при чужом звуке. Решение целиком считает Rust (см.
    // src/wallpaper_audio.rs и rexon-core::wallpaper_audio), здесь только
    // привязки — поэтому смена настройки доходит до проигрывателя без
    // перезапуска обоев. ⚠️ Объект ОДИН на процесс, а не на окно: иначе
    // каждый монитор завёл бы свой поток опроса оконного менеджера.
    property WallpaperAudio audio: WallpaperAudio {}

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
            /// Окно основного экрана — единственное, которое звучит.
            readonly property bool primary: surface.index === 0

            property bool layerApplied: false
            property bool x11Applied: false
            property bool mmapStarted: false
            property alias player: player

            // ⚠️ Порядок обязателен: окно создаётся НЕВИДИМЫМ, слой ставится
            // в Component.onCompleted, и только потом visible = true.
            // Показанное окно уже стало обычным xdg_toplevel и в слой не
            // переселится.
            visible: false
            color: "black"
            flags: root.isWindowed ? Qt.Window : Qt.FramelessWindowHint
            title: root.isWindowed ? qsTranslate("rexon", "Rexon — video wallpaper") : ""

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
                            console.warn("REXON-WPH-WARN обои не прицелились на экран "
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
                    console.warn("REXON-WPH-WARN окно X11 не подготовлено к роли рабочего стола");
                surface.visible = true;

                if (root.placement === "offscreen") {
                    // Окно на экран не выводится: кадры уходят расширению
                    // оболочки (GNOME/Cinnamon на Wayland — layer-shell там
                    // нет). Кадр берётся из приёмника `vout`, а не снимком
                    // окна: хост здесь на платформе offscreen, и VideoOutput
                    // там не рисует — снимок был бы чёрным.
                    surface.mmapStarted = root.placer.startMmapStream(
                        surface, vout, root.mmapPath, root.renderWidth,
                        root.renderHeight, root.mmapFps);
                    if (!surface.mmapStarted)
                        console.warn("REXON-WPH-ERROR поток кадров не запустился: "
                                     + root.mmapPath);
                } else if (root.placement === "x11" && !root.isWindowed) {
                    // После показа — просьбы EWMH корневому окну: так
                    // меняют состояние уже показанного окна.
                    x11Settle.start();
                }
            }

            // Пауза/продолжение по обстановке. Отдельной функцией, потому что
            // звать её надо и при смене решения, и когда носитель
            // перезагрузился.
            function syncPlayback() {
                if (root.audio.paused) {
                    if (player.playbackState === MediaPlayer.PlayingState)
                        player.pause();
                } else if (player.playbackState !== MediaPlayer.PlayingState
                           && (player.mediaStatus === MediaPlayer.LoadedMedia
                               || player.mediaStatus === MediaPlayer.BufferedMedia)) {
                    player.play();
                }
            }

            Connections {
                target: root.audio
                function onPausedChanged() { surface.syncPlayback(); }
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

            // ⚠️ Вывод звука ЕСТЬ только когда звук включён, и это не
            // придирка. В Qt6 у `MediaPlayer` без назначенного `audioOutput`
            // устройства нет вовсе, а с назначенным и заглушённым —
            // устройство открыто: поток висит в звуковой карте весь сеанс, в
            // системном микшере болтается ползунок «RexonVideoWallpaper»,
            // который ничего не играет, и приёмник из-за постоянного клиента
            // не уходит в спячку. «Звук выключен» должно означать «потока
            // нет», а не «поток на нуле», поэтому вывод отвязывается, а не
            // приглушается. По той же причине окна НЕ основных экранов не
            // получают вывод никогда: иначе звук обоев зазвучал бы столько
            // раз, сколько мониторов.
            AudioOutput {
                id: audioOut
                volume: root.volumeFixed >= 0 ? root.volumeFixed : root.audio.volume
            }

            MediaPlayer {
                id: player
                source: root.videoUrl
                videoOutput: vout
                // ⚠️ Ключ `--audio` из командной строки остаётся правом вето:
                // он приходит от того, кто запустил обои («эта обоина
                // немая»), а настройка — от человека. Ни одно из двух не
                // должно молча отменять другое, поэтому звук идёт, когда его
                // разрешили ОБА — и только на основном экране.
                audioOutput: (surface.primary && root.audioEnabled && root.audio.audioOn)
                             ? audioOut : null
                loops: root.loopVideo ? MediaPlayer.Infinite : 1
                // Воспроизведение стартует, когда носитель готов.
                onMediaStatusChanged: {
                    if (mediaStatus === MediaPlayer.LoadedMedia
                            || mediaStatus === MediaPlayer.BufferedMedia) {
                        // Через общую функцию, а не прямым play(): решение
                        // «играть или стоять» одно на все поводы, иначе
                        // перезагрузка носителя снимала бы паузу,
                        // поставленную из-за полноэкранного окна.
                        surface.syncPlayback();
                        // Звук просили, а дорожки в файле нет. Отдельная
                        // жалоба нужна потому, что САМ проигрыватель об этом
                        // не сообщает ничем: playbackState, mediaStatus,
                        // громкость и признак заглушения у немого файла ровно
                        // те же, что у файла со звуком, — «плеер доволен» и
                        // «звука не будет» с виду неотличимы.
                        // Один раз за процесс: готовность приходит дважды
                        // (LoadedMedia, затем BufferedMedia), а окон столько,
                        // сколько мониторов, — повторённая жалоба читалась бы
                        // как несколько разных бед.
                        if (surface.primary && root.audioEnabled && !hasAudio
                                && !root.noAudioWarned) {
                            root.noAudioWarned = true;
                            console.warn("REXON-WPH-NOAUDIO звук включён, но в файле"
                                         + " нет звуковой дорожки: " + root.videoUrl);
                        }
                    }
                }
                onErrorOccurred: (err, msg) => console.warn("REXON-WPH-ERROR " + err + " " + msg)

                // Пауза и возврат к игре — по решению наблюдателя. Держится
                // привязкой к состоянию, а не разовой командой: иначе после
                // любой перезагрузки носителя обои остались бы стоять.
                Component.onCompleted: surface.syncPlayback()
            }

            VideoOutput {
                id: vout
                anchors.fill: parent
                // Заполнить весь экран без чёрных полос — как обои Wallpaper Engine.
                fillMode: VideoOutput.PreserveAspectCrop
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
    /// окна обоев по нескольку раз подряд, то есть жёг бы процессор ровно
    /// тогда, когда человеку и так не до того (правило #13).
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
        console.log("REXON-WPH-SCREENS экранов=" + list.length
                    + " имена=" + root.screenNames());

        // Наблюдение за звуком поднимается один раз и только когда есть с
        // чем сравнивать размер чужого окна.
        if (!root.audioStarted) {
            var size = root.audioProbeSize();
            if (size[0] > 0 && size[1] > 0) {
                root.audioStarted = true;
                root.audio.start(size[0], size[1]);
            }
        }
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

    /// Сколько окон держат звуковой выход. Обязано быть 0 или 1: звучит
    /// только окно основного экрана, иначе обои зазвучали бы столько раз,
    /// сколько мониторов.
    function audioWindows() {
        var n = 0;
        for (var i = 0; i < root.surfaces.count; ++i) {
            var s = root.surfaces.objectAt(i);
            if (s && s.player && s.player.audioOutput)
                n += 1;
        }
        return n;
    }

    /// Размер экрана, с которым наблюдатель звука сравнивает чужие окна.
    ///
    /// ⚠️ Берётся САМЫЙ МАЛЕНЬКИЙ из экранов, и это осознанный выбор. Решение
    /// «окно во весь экран» ядро выносит сравнением `ширина ≥ экрана`
    /// (`wallpaper_audio::kwin_window_conditions`), а на каком именно
    /// мониторе стоит чужое окно, оконный менеджер не сообщает. По самому
    /// большому экрану кино, развёрнутое на маленьком мониторе, не считалось
    /// бы полноэкранным вовсе, и обои продолжали бы играть поверх него; по
    /// самому маленькому в худшем случае приглушимся раньше времени.
    /// Приглушиться зря — мелкая неприятность, играть поверх кино — та самая
    /// беда, ради которой наблюдатель и написан.
    function audioProbeSize() {
        if (root.placement === "offscreen")
            return [root.renderWidth, root.renderHeight];
        var w = -1;
        var h = -1;
        var all = Qt.application.screens;
        for (var i = 0; i < all.length; ++i) {
            if (w < 0 || all[i].width < w)
                w = all[i].width;
            if (h < 0 || all[i].height < h)
                h = all[i].height;
        }
        return [Math.max(w, 0), Math.max(h, 0)];
    }

    Component.onCompleted: {
        root.rebuildScreens();
        // Офскрин-режим экранов не знает, и ждать их появления ему незачем.
        if (!root.audioStarted) {
            var size = root.audioProbeSize();
            root.audioStarted = true;
            root.audio.start(size[0], size[1]);
        }
        if (root.selftestMs > 0)
            selftest.start();
    }

    // ── Самопроверка ──────────────────────────────────────────────────
    //
    // Отчёт печатается ОДНОЙ строкой и разбирается родительской половиной
    // стенда: «окно поднялось» и «видео пошло» — разные утверждения, и
    // молчание не должно выглядеть успехом.
    //
    // ⚠️ Про звук отчёт когда-то врал умолчанием. Он печатал громкость и
    // признак заглушения — то есть НАСТРОЙКУ вывода, — но не печатал, есть
    // ли в файле звуковая дорожка вообще. У немого файла строка выходила
    // ровно та же, что у файла со звуком («playback=1 status=5 hasVideo=true
    // volume=0.7 muted=false»), и самопроверка зеленела там, где до звуковой
    // карты не доходило ничего. Отсюда `hasAudio`, `audioTracks` и имя
    // устройства: проверяется то, что решает исход, а не то, что удобно
    // спросить. И приговор — код возврата, а не только строка: звук
    // просили, дорожки нет — это провал прогона, а не примечание к нему.
    //
    // ⚠️ `screens` и `windows` в отчёте — тоже не украшение: «обои
    // поднялись» и «обои легли на ВСЕ мониторы» — разные утверждения, и
    // первое годами выглядело как второе.
    property Timer selftest: Timer {
        id: selftest
        interval: root.selftestMs
        repeat: false
        onTriggered: {
            var top = root.primarySurface;
            if (!top) {
                console.log("REXON-WPH-REPORT placement=" + root.placement
                            + " screens=" + Qt.application.screens.length
                            + " windows=0 — обоям некуда лечь");
                Qt.exit(1);
                return;
            }
            var player = top.player;
            console.log("REXON-WPH-REPORT placement=" + root.placement
                        + " screens=" + Qt.application.screens.length
                        + " windows=" + root.surfaces.count
                        + " screenNames=" + root.screenNames()
                        + " windowScreens=" + root.surfaceScreens()
                        + " audioWindows=" + root.audioWindows()
                        + " layerScreen=" + root.layerShell.screenName(top)
                        + " layerApplied=" + root.layerApplied
                        + " x11Applied=" + root.x11Applied
                        + " mmapStarted=" + root.mmapStarted
                        + " mmapFrames=" + root.placer.mmapFrames(top)
                        + " mmapCenterRgb=" + root.placer.mmapCenterRgb(top)
                        + " windowW=" + top.width + " windowH=" + top.height
                        + " playback=" + player.playbackState
                        + " status=" + player.mediaStatus
                        + " hasVideo=" + player.hasVideo
                        + " position=" + player.position
                        + " audioEnabled=" + root.audioEnabled
                        + " audioOn=" + root.audio.audioOn
                        + " audioPaused=" + root.audio.paused
                        + " audioReason=" + root.audio.reason
                        + " hasAudio=" + player.hasAudio
                        + " audioTracks=" + player.audioTracks.length
                        + " audioDevice=" + (player.audioOutput
                                             ? player.audioOutput.device.description
                                             : "нет")
                        + " volume=" + (player.audioOutput ? player.audioOutput.volume : -1)
                        + " muted=" + (player.audioOutput ? player.audioOutput.muted : true));
            top.visible = false;
            // Код 4 — «звук просили, а играть нечем»: либо вывод не создан,
            // либо в файле нет звуковой дорожки.
            // ⚠️ Приговор — по НАЛИЧИЮ звуковой дорожки, а не по тому,
            // подключён ли сейчас вывод. Вывод отвязывает и обычная
            // работа: полноэкранное окно, чужой звук, снятая галочка. Это
            // решение, а не поломка, и валить прогон оно не должно —
            // иначе самопроверка снова начнёт мерить настройку.
            var zvukOk = !root.audioEnabled || player.hasAudio;
            Qt.exit(zvukOk ? 0 : 4);
        }
    }
}
