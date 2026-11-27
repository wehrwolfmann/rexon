// waywallen daemon output client on X11: ONE DESKTOP WINDOW PER SCREEN.
//
// Why (defect 30.10.2026, MATE on X11): Wallpaper Engine scenes are drawn
// by the waywallen daemon, and they are put on the desktop by an output
// client — on KDE that is the Plasma plugin, on wlr compositors
// `waywallen-layer-shell`. On X11 outside KDE waywallen has no such
// client, the daemon has zero displays, and "Apply" stayed grey. Here the
// Rexon wallpaper host acts as the client: each window holds a waywallen
// output element (`Waywallen.DisplayEmbed`, the waywallen-display library
// from the same `org.waywallen.kde` package that Rexon ships for Plasma).
// It connects to the daemon socket by itself, registers a display and
// shows frames; based on it the daemon decides what to draw and where.
// Since 25.11.2026 the element comes first from the `waywallen-display`
// distribution package (the `Waywallen.Display` module, no Plasma needed),
// through a wrapper the host writes; the plasmoid is the fallback.
//
// The windows are put into the background by the same X11 executor as for
// video and web (`WallpaperPlacement`, rule #12): the "desktop" type and
// state before showing, requests to the root window after; the order
// relative to the desktop icons window after showing and by a
// reconciliation every 2 s.
//
// The bundled output module lies in a directory whose name differs from its
// URI, so it is loaded through the package wrapper (`PluginDisplay.qml`) by a
// path on disk rather than by an import; the packaged module is imported by
// name from the host's own wrapper. The path is chosen by the host
// (`waywallen_kde_plugin::pick_display_item`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Initial properties from the host ──────────────────────────────
    /// Wrapper of the output element (`…/Plugin/PluginDisplay.qml`, or the host's
    /// wrapper of the packaged module) — the file address.
    property url displayItemUrl
    /// Output socket of the daemon (`--socket` from the daemon). Empty means
    /// the element finds it by itself (the daemon's bus property,
    /// `$XDG_RUNTIME_DIR/waywallen`).
    property string socketPath: ""
    /// A 1280×720 debug window instead of desktops.
    property bool isWindowed: false

    /// Screens the windows lie on. Rebuilt as a whole — see the analysis in
    /// WallpaperVideo.qml: the `Qt.application.screens` wrappers live by index.
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
            // Debug window — only for checking by a developer.
            title: root.isWindowed ? "Rexon" : ""
            screen: surface.info ? surface.info : null
            width: surface.info ? surface.info.width : 1280
            height: surface.info ? surface.info.height : 720

            /// The daemon sends frames to this display (`StreamState.Active` = 1).
            readonly property bool streaming: output.item ? output.item.streamState === 1 : false

            // Warning: the window is on the desktop ONLY while frames are flowing.
            // Without a stream (live wallpaper stopped, the user went back to an
            // ordinary picture, the daemon is still starting after login) a black
            // desktop window would cover the desktop's wallpaper — rule #13. But the
            // first show is needed anyway: the output element performs the handshake
            // with the daemon when the window has a graphics context (it learns the
            // DMA-BUF formats from the window's EGL). So: show at start → handshake →
            // without frames the window hides (with a margin for wallpaper changes,
            // so it does not flicker) → frames start flowing — the window is back on
            // the desktop.
            function showOnDesktop() {
                if (surface.visible)
                    return;
                // "Desktop" properties — BEFORE every show: the window manager unmanages a
                // hidden window and forgets its state (EWMH).
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
                    // Without an explicit corner all windows would land on the primary monitor.
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

            // Order relative to the desktop icons window (Caja, Nemo, xfdesktop): the
            // icons window gets recreated and goes on top — the reconciliation puts
            // the wallpaper back, above opaque icons and below see-through ones
            // (`keepX11Order`, the decision comes from the core).
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
                    // Monitor name as the system calls it: that way the display is recognized
                    // on the "Monitors" page and in the daemon log.
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
