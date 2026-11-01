// Клиент вывода демона waywallen на X11: ПО ОКНУ-РАБОЧЕМУ СТОЛУ НА ЭКРАН.
//
// Зачем (дефект 30.10.2026, MATE на X11): сцены Wallpaper Engine рисует
// демон waywallen, а на стол их выводит клиент вывода — на KDE это
// Plasma-плагин, на wlr-композиторах `waywallen-layer-shell`. На X11 вне KDE
// такого клиента у waywallen нет, дисплеев у демона ноль, и «Применить»
// оставалась серой. Здесь клиентом служит хост обоев Rexon: в каждом окне —
// элемент вывода waywallen (`Waywallen.DisplayEmbed`, библиотека
// waywallen-display из того же пакета `org.waywallen.kde`, что Rexon везёт
// для Plasma). Он сам подключается к сокету демона, регистрирует дисплей и
// показывает кадры; демон по нему решает, что и где рисовать.
//
// Окна кладутся на задний план тем же исполнителем X11, что у видео и веба
// (`WallpaperPlacement`, правило #12): тип «рабочий стол» и состояние — до
// показа, просьбы корневому окну — после; порядок относительно окна значков
// стола — после показа и сверкой раз в 2 с.
//
// Модуль вывода лежит в каталоге с другим именем, чем его URI, поэтому
// грузится через обёртку пакета (`PluginDisplay.qml`) по пути на диске, а
// не импортом: путь находит хост (`waywallen_kde_plugin::find_runtime_so`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Начальные свойства из хоста ───────────────────────────────────
    /// Обёртка элемента вывода (`…/Plugin/PluginDisplay.qml`) — адрес файла.
    property url displayItemUrl
    /// Сокет вывода демона (`--socket` от демона). Пусто — элемент находит
    /// его сам (свойство демона на шине, `$XDG_RUNTIME_DIR/waywallen`).
    property string socketPath: ""
    /// Отладочное окно 1280×720 вместо рабочих столов.
    property bool isWindowed: false

    /// Экраны, на которых лежат окна. Пересобирается целиком — см. разбор в
    /// WallpaperVideo.qml: обёртки `Qt.application.screens` живут по номеру.
    property var screenList: []

    readonly property var surfaceModel: root.isWindowed ? ["single"] : root.screenList

    property WallpaperPlacement placer: WallpaperPlacement {}

    function refreshScreens() {
        root.screenList = Array.from(Qt.application.screens);
    }

    property Connections screensWatch: Connections {
        target: Qt.application
        function onScreensChanged() { root.refreshScreens(); }
    }

    Component.onCompleted: root.refreshScreens()

    property Instantiator surfaces: Instantiator {
        model: root.surfaceModel

        delegate: Window {
            id: surface

            required property int index
            required property var modelData

            readonly property var info: root.isWindowed ? null : surface.modelData

            visible: false
            color: "black"
            flags: root.isWindowed ? Qt.Window : Qt.FramelessWindowHint
            // Отладочное окно — только для проверки разработчиком.
            title: root.isWindowed ? "Rexon" : ""
            screen: surface.info ? surface.info : null
            width: surface.info ? surface.info.width : 1280
            height: surface.info ? surface.info.height : 720

            /// Демон шлёт в этот дисплей кадры (`StreamState.Active` = 1).
            readonly property bool streaming: output.item ? output.item.streamState === 1 : false

            // ⚠️ Окно на столе — ТОЛЬКО пока идут кадры. Без потока (живые обои
            // остановлены, человек вернул обычную картинку, демон ещё
            // поднимается после входа) чёрное окно-рабочий стол закрыло бы
            // обои стола — правило #13. Но первый показ нужен всё равно:
            // элемент вывода проходит рукопожатие с демоном, когда у окна есть
            // графический контекст (форматы DMA-BUF он узнаёт у EGL окна).
            // Поэтому: показ при старте → рукопожатие → без кадров окно
            // прячется (с запасом на смену обоев, чтобы не мигать) → кадры
            // пошли — окно снова на столе.
            function showOnDesktop() {
                if (surface.visible)
                    return;
                // Свойства «рабочего стола» — ДО каждого показа: спрятанное
                // окно оконный менеджер выводит из управления и забывает его
                // состояние (EWMH).
                if (!root.isWindowed && !root.placer.prepareX11Desktop(surface))
                    console.warn("REXON-WWD-WARN окно X11 не подготовлено к роли рабочего стола");
                surface.visible = true;
                if (!root.isWindowed)
                    settle.start();
            }

            onStreamingChanged: {
                if (surface.streaming) {
                    hideIdle.stop();
                    surface.showOnDesktop();
                } else {
                    hideIdle.restart();
                }
            }

            Component.onCompleted: {
                if (surface.info) {
                    // Без явного угла все окна легли бы на основной монитор.
                    surface.x = surface.info.virtualX;
                    surface.y = surface.info.virtualY;
                }
                surface.showOnDesktop();
                hideIdle.restart();
            }

            Timer {
                id: settle
                interval: 150
                repeat: false
                onTriggered: root.placer.applyX11Desktop(surface)
            }

            // Порядок относительно окна значков стола (Caja, Nemo,
            // xfdesktop): окно значков пересоздаётся и встаёт сверху —
            // сверка ставит обои обратно, над непрозрачными значками и под
            // сквозными (`keepX11Order`, решение — ядро).
            Timer {
                interval: 2000
                repeat: true
                running: surface.visible && !root.isWindowed
                onTriggered: root.placer.keepX11Order(surface)
            }

            Timer {
                id: hideIdle
                interval: 3000
                repeat: false
                onTriggered: {
                    if (!surface.streaming && !root.isWindowed && surface.visible) {
                        surface.visible = false;
                        console.log("REXON-WWD-IDLE кадров нет — окно убрано со стола");
                    }
                }
            }

            Loader {
                id: output
                anchors.fill: parent
                source: root.displayItemUrl

                onLoaded: {
                    const d = output.item;
                    if (root.socketPath.length > 0)
                        d.socketPath = root.socketPath;
                    // Имя монитора — как его называет система: так дисплей
                    // узнаётся на странице «Мониторы» и в журнале демона.
                    d.displayName = surface.info
                        ? (surface.info.model && surface.info.model.trim().length > 0
                           ? surface.info.model.trim() : surface.info.name)
                        : "rexon-window";
                    d.autoReconnect = true;
                    d.displayWidth = Qt.binding(() => Math.round(surface.width * d.effectiveDevicePixelRatio));
                    d.displayHeight = Qt.binding(() => Math.round(surface.height * d.effectiveDevicePixelRatio));
                    console.log("REXON-WWD-START экран=" + d.displayName + " "
                                + surface.width + "x" + surface.height);
                }
                onStatusChanged: {
                    if (output.status === Loader.Error)
                        console.warn("REXON-WWD-ERROR элемент вывода waywallen не загрузился: "
                                     + root.displayItemUrl);
                }
            }

            Connections {
                target: output.item
                ignoreUnknownSignals: true
                function onDisplayIdChanged() {
                    console.log("REXON-WWD-DISPLAY экран=" + output.item.displayName
                                + " дисплей=" + output.item.displayId);
                }
            }
        }
    }
}
