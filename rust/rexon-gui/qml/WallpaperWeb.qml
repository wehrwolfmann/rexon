// Rexon web wallpapers: ONE WINDOW PER SCREEN, one process per wallpaper.
//
// Replacement for the removed `rexon/web_wallpaper.py` (791 lines, three
// nearly identical QML files: WebWallpaper.qml, WebWallpaperX11.qml,
// OffscreenWebWallpaper.qml). The difference between them was exactly in
// how the window gets into the background; once the layer is set by Rust
// rather than by an attached QML property, that difference leaves the
// markup entirely — just as already done for video (WallpaperVideo.qml).
// Rule #12: a shared mechanism instead of a copy per scenario.
//
// Warning: second monitor (16.09.2026). Before this, the file root was ONE
// window the size of the screen the compositor put it on: with two
// monitors only one got a wallpaper. Now the root is a window holder: an
// `Instantiator` creates a window for every connected screen and removes
// the windows of monitors that went away; a monitor appearing and
// disappearing on the fly is caught with the
// `Qt.application.onScreensChanged` signal. The arrangement is exactly the
// same as for video wallpapers — and it is deliberately one for both
// modes.
//
// Scenarios (the `placement` property, decided by the core
// `wallpaper::video_qml_file` — one decision for both hosts):
//   layer     — Wayland with wlr-layer-shell: the BACKGROUND layer is set
//               by Rust (WallpaperLayer from org.rexon.gui) BEFORE the
//               window is first shown, and the surface output is named
//               explicitly (`pinScreen`);
//   x11       — ordinary windows, one per screen, each placed in the corner
//               of its own screen; the host gives them
//               _NET_WM_WINDOW_TYPE_DESKTOP;
//   offscreen — GNOME/Cinnamon on Wayland: there is no layer-shell there,
//               the window's frames go to the shell extension through a
//               shared mmap file. There is ONE window and it knows nothing
//               about screens: the shell itself lays the frame out over
//               the monitors.
//
// Properties arrive from Rust as initial properties of the root
// (QQmlApplicationEngine::setInitialProperties); names, defaults and all
// injected JS are computed by the core (`rexon-core::wallpaper`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import QtWebEngine
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Initial properties from the host ──────────────────────────────
    /// What to show: a file:// to a local HTML file or an http(s) link.
    property url targetUrl
    property bool isWindowed: false
    /// Frame size for offscreen mode: there is no window on screen, so there is
    /// nowhere to take the screen size from.
    property int renderWidth: 1920
    property int renderHeight: 1080
    /// «layer» | «x11» | «offscreen».
    property string placement: "layer"
    /// Shared frame file for the shell extension (offscreen mode).
    property string mmapPath: ""
    /// Stream frame rate — set by the shell extension.
    property int mmapFps: 30
    /// Self-test run: the windows close by themselves after reporting.
    property int selftestMs: 0
    /// Stubs of the Wallpaper Engine global functions. Injected at the
    /// DocumentCreation point — BEFORE the page's first script.
    property string polyfillJs: ""
    /// Wallpaper properties from project.json + the first audio tick. The
    /// Deferred point — after the document has loaded, when the wallpaper has
    /// already set up its listener.
    property string afterLoadJs: ""

    // ── Screens ───────────────────────────────────────────────────────
    /// Screens the wallpaper currently lies on. Rebuilt as a whole rather than
    /// edited one by one: the `Qt.application.screens` wrappers live by INDEX
    /// and are reassigned to other monitors when the configuration changes —
    /// holding them one by one would mean holding a reference that silently
    /// started to mean something else.
    property var screenList: []

    /// Whether we place one window per screen. Not in offscreen mode or for the
    /// debug window: there is one window there and it belongs to no screen.
    readonly property bool perScreen: root.placement !== "offscreen" && !root.isWindowed

    /// Window model. A non-empty string instead of a screen means "one window,
    /// no screen is assigned to it".
    readonly property var surfaceModel: root.perScreen ? root.screenList : ["single"]

    /// The primary screen's window: the frame stream and the self-test report
    /// belong to it. The primary screen comes first in Qt's `screens()`.
    readonly property var primarySurface: root.surfaces.count > 0
                                          ? root.surfaces.objectAt(0)
                                          : null

    // Summary indicators for the self-test — from the primary screen's window.
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

    // The Rust layer configurator — the same object as for video wallpapers,
    // and ONE for all windows.
    property WallpaperLayer layerShell: WallpaperLayer {
        scope: "rexon-web-wallpaper"
    }

    // The other two ways to place the window (xprop on X11 and the frame
    // stream into a shared file) are also shared with the video mode.
    property WallpaperPlacement placer: WallpaperPlacement {}

    // Live audio and pause state — the same supervisor as for video and
    // Flash: the "Wallpapers" settings, windows on top, battery, a game, the
    // lock screen and a foreign session. Rust computes the whole decision
    // (src/wallpaper_audio.rs and rexon-core::wallpaper_audio); here we only
    // deliver it to the page.
    // Warning: there is ONE object per process, not per window: otherwise
    // every monitor would start its own polling thread.
    property WallpaperAudio audio: WallpaperAudio {}
    property bool audioStarted: false

    // Heartbeat for the Rexon live wallpaper watchdog — the same as for video
    // wallpapers: request a frame from every window and check in
    // (`rexon-core::renderer_watch`).
    property Timer heartbeat: Timer {
        interval: root.placer.heartbeatIntervalMs()
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            for (var i = 0; i < surfaces.count; ++i) {
                var w = surfaces.objectAt(i);
                if (w && w.visible)
                    w.update();
            }
            root.placer.heartbeat();
        }
    }

    // ── Wallpaper windows ─────────────────────────────────────────────
    property Instantiator surfaces: Instantiator {
        id: surfaces
        model: root.surfaceModel

        delegate: Window {
            id: surface

            required property int index
            required property var modelData

            /// This window's screen; null means the window belongs to no screen
            /// (offscreen mode, the debug window).
            readonly property var info: root.perScreen ? surface.modelData : null
            readonly property bool primary: surface.index === 0

            property bool layerApplied: false
            property bool x11Applied: false
            property bool mmapStarted: false
            /// Chromium already knows the viewport size — the page can be loaded.
            ///
            /// Warning: in frame stream mode the scene is drawn by QQuickRenderControl
            /// without a window, and the size does not reach Chromium at once.
            /// Measurement: a page that sets `canvas.width = innerWidth` once while
            /// parsing (most WebGL wallpapers do this) saw `innerWidth=0` — a 0×0
            /// canvas and a black frame, although a second later the view was
            /// 1920×1080. So `about:blank` comes first, and the real address only when
            /// the stub page itself sees a non-zero `innerWidth × innerHeight` (the
            /// workaround from QTBUG-132776: wait for the Chromium compositor, not just
            /// for loading).
            property bool viewportReady: root.placement !== "offscreen"
            /// Page load result: 0 means it has not arrived yet.
            property int loadStatus: 0
            property string loadError: ""
            property alias view: view

            /// Deliver the supervisor's decision (pause, volume) to the page. Muting is
            /// not done here but by the `audioMuted` property below: it also covers Web
            /// Audio, which the volume does not reach. While the `about:blank` stub is
            /// in place of the page there is nobody to call — the decision goes out
            /// when the page loads.
            function syncPage() {
                if (surface.viewportReady)
                    view.runJavaScript(root.audio.webStateScript());
            }

            Connections {
                target: root.audio
                function onPausedChanged() { surface.syncPage(); }
                function onVolumeChanged() { surface.syncPage(); }
            }

            // Warning: the order is mandatory: the window is created INVISIBLE, the
            // layer is set in Component.onCompleted, and only then visible = true. A
            // shown window has already become an ordinary xdg_toplevel and will not
            // move into the layer.
            visible: false
            color: "black"
            flags: root.isWindowed ? Qt.Window : Qt.FramelessWindowHint
            title: root.isWindowed ? qsTranslate("rexon", "Rexon — web wallpaper") : ""

            // Warning: the screen is assigned BEFORE showing: both the layer
            // (LayerShellQt reads QWindow::screen()) and X11 take it from there.
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
                        // Debug window: on top of the others, without anchors and without
                        // reserving space — otherwise it would not be visible behind the
                        // wallpaper.
                        root.layerShell.layer = 2;                  // LayerTop
                        root.layerShell.anchors = 0;                // AnchorNone
                        root.layerShell.exclusionZone = 0;
                        root.layerShell.keyboardInteractivity = 2;  // OnDemand
                        surface.layerApplied = root.layerShell.apply(surface);
                    } else {
                        surface.layerApplied =
                            root.layerShell.applyWallpaperLayer(surface);
                        // Warning: the surface output is named EXPLICITLY. Without this the
                        // compositor is free to put all wallpaper windows on one monitor, and the
                        // second would stay empty.
                        if (surface.layerApplied && !root.layerShell.pinScreen(surface))
                            console.warn("REXON-WEB-WARN обои не прицелились на экран "
                                         + (surface.info ? surface.info.name : "?"));
                    }
                } else if (root.placement === "x11" && surface.info) {
                    // On X11 the window has to be placed in the corner of its screen by hand:
                    // without this all wallpaper windows would land on the primary monitor.
                    surface.x = surface.info.virtualX;
                    surface.y = surface.info.virtualY;
                }
                // Warning: the "desktop" type and the state are set BEFORE showing: the
                // window manager reads the type when the window is shown, and the state of
                // a shown window is no longer changed through the property (EWMH). After
                // showing — requests to the root window (x11Settle).
                if (root.placement === "x11" && !root.isWindowed
                        && !root.placer.prepareX11Desktop(surface))
                    console.warn("REXON-WEB-WARN окно X11 не подготовлено к роли рабочего стола");
                // Warning: in frame stream mode the window is NEVER shown: the host lives
                // on the session's real platform (for the sake of the GPU), and a shown
                // window on Mutter would appear on top of the desktop. The scene is drawn
                // by QQuickRenderControl, to which the shim hands the content of this
                // window.
                if (root.placement !== "offscreen")
                    surface.visible = true;

                if (root.placement === "offscreen") {
                    // Frames go to the shell extension (GNOME/Cinnamon on Wayland — there is
                    // no layer-shell there). null instead of a VideoOutput — the frame is
                    // drawn by a windowless Qt Quick scene, on the GPU.
                    surface.mmapStarted = root.placer.startMmapStream(
                        surface, null, root.mmapPath, root.renderWidth,
                        root.renderHeight, root.mmapFps);
                    if (!surface.mmapStarted)
                        console.warn("REXON-WEB-ERROR поток кадров не запустился: "
                                     + root.mmapPath);
                } else if (root.placement === "x11" && !root.isWindowed) {
                    // After showing — EWMH requests to the root window: that is how the state
                    // of an already shown window is changed.
                    x11Settle.start();
                }
            }

            // First frame of the loaded page on screen: wait measurement and, on
            // Cinnamon with layer-shell, moving the background service under us — on
            // the event rather than after a delay (before loading the window is
            // black). Any load result counts: even with a failed page the window is
            // already on top.
            property bool firstFrameReported: false
            Connections {
                target: surface
                enabled: root.placement !== "offscreen" && !surface.firstFrameReported
                         && (surface.loadStatus === WebEngineView.LoadSucceededStatus
                             || surface.loadStatus === WebEngineView.LoadFailedStatus)
                function onFrameSwapped() {
                    surface.firstFrameReported = true;
                    root.placer.firstFrameShown("web");
                }
            }

            WebEngineView {
                id: view
                anchors.fill: parent
                url: surface.viewportReady ? root.targetUrl : "about:blank"
                // Black, not transparent: a wallpaper may have no background of its own,
                // and a transparent window on the layer would show what is underneath —
                // that is, the previous wallpaper. It matches the window's `color`, so no
                // seam is visible.
                backgroundColor: "black"
                // Only the primary screen's window plays sound — as with video: the
                // window on each monitor holds its own copy of the page, and without this
                // the wallpaper would sound as many times as there are monitors. Sound is
                // also muted by the supervisor's decision (the "Wallpaper sound" setting,
                // foreign audio, pause).
                audioMuted: !(surface.primary && root.audio.audioOn)

                // Warning: the WE stubs are installed as a script, not with a
                // runJavaScript call after loading, and this is not nitpicking. Workshop
                // wallpapers check
                // `typeof window.wallpaperRegisterAudioListener === 'function'`
                // SYNCHRONOUSLY while the HTML is parsed: by the time the page has loaded
                // and runJavaScript can be called, their script has already aborted. The
                // DocumentCreation point is the only one that is in time.
                //
                // The script type is passed as an object, not as a `WebEngineScript`
                // element: in Qt 6 `QWebEngineScript` is a value type, and in QML it is
                // declared with a lowercase letter (`webEngineScript`), so it cannot be
                // created as an element. The injection point numbers come from there too:
                // Deferred=0, DocumentReady=1, DocumentCreation=2; world 0 is the main
                // world, the same one the page itself lives in (in an isolated world
                // `window` would be a different one).
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
                    // The result of the `about:blank` stub is not the wallpaper's result:
                    // otherwise the self-test would take an empty page for a loaded one.
                    if (!surface.viewportReady)
                        return;
                    surface.loadStatus = info.status;
                    if (info.status === WebEngineView.LoadSucceededStatus)
                        surface.syncPage();
                    if (info.status === WebEngineView.LoadFailedStatus) {
                        surface.loadError = info.errorString;
                        // A silent black screen explains nothing to the user (rule #13): what
                        // exactly failed to load goes to the log.
                        console.warn("REXON-WEB-ERROR страница не загрузилась: "
                                     + info.url + " — " + info.errorString);
                    }
                }

                onJavaScriptConsoleMessage: (level, message, lineNumber, sourceID) => {
                    // This is the only way to see the wallpaper's own complaints: wallpapers
                    // have no developer window of their own, and without them "the wallpaper
                    // is black" is indistinguishable from "the wallpaper crashed on the
                    // second line of its script". Only the primary screen's window prints:
                    // there are as many windows as monitors, and the same complaint appearing
                    // several times in the log reads as several different problems.
                    if (surface.primary && level >= WebEngineView.WarningMessageLevel)
                        console.warn("REXON-WEB-JS " + sourceID + ":" + lineNumber
                                     + " " + message);
                }
            }

            // Esc / Q — exit. Useful in the debug window; on the wallpaper layer the
            // window has no keyboard at all, so here it has no effect.
            Shortcut {
                sequences: ["Escape", "Q"]
                onActivated: Qt.quit()
            }

            // The debug window is closed with the cross — and that must mean "quit".
            // The application survives closing its last window on purpose (a
            // disconnected monitor must not kill the wallpaper), so quitting is
            // stated explicitly here.
            onClosing: {
                if (root.isWindowed)
                    Qt.quit();
            }

            // Polling the stub: does Chromium know the viewport size.
            Timer {
                interval: 50
                running: !surface.viewportReady
                repeat: true
                onTriggered: view.runJavaScript("innerWidth * innerHeight", function (area) {
                    if (!surface.viewportReady && area > 0)
                        surface.viewportReady = true;
                })
            }

            // The size did not arrive from Chromium — load anyway: an empty wallpaper
            // is worse than one whose canvas may have taken the old size.
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

            // Order relative to the desktop icons window: the icons window gets
            // recreated (Caja restart, a change of its settings) and goes on top —
            // the reconciliation puts the wallpaper back (above opaque icons, below
            // see-through ones). If it already stands where it should, nothing is
            // done.
            Timer {
                interval: 2000
                repeat: true
                running: root.placement === "x11" && !root.isWindowed && surface.x11Applied
                onTriggered: root.placer.keepX11Order(surface)
            }
        }
    }

    // ── Monitor configuration change ──────────────────────────────────
    property Connections screenWatch: Connections {
        target: Qt.application
        function onScreensChanged() { settle.restart(); }
    }

    /// Pause before rebuilding the windows.
    ///
    /// Warning: not decoration: a monitor reconfiguration does NOT arrive as
    /// one event — the compositor turns an output off, changes the mode and
    /// turns it on again, and each step sends its own `screensChanged`. Without
    /// the delay the host would recreate all wallpaper windows several times
    /// in a row — and here each window costs a running Chromium, i.e. it would
    /// burn the CPU exactly when the user has other things on their mind
    /// (rule #13).
    property Timer settle: Timer {
        id: settle
        interval: 400
        repeat: false
        onTriggered: root.rebuildScreens()
    }

    /// Re-read the screens. As a whole, not one by one: see `screenList`.
    function rebuildScreens() {
        var all = Qt.application.screens;
        var list = [];
        for (var i = 0; i < all.length; ++i)
            list.push(all[i]);
        root.screenList = list;
        console.log("REXON-WEB-SCREENS экранов=" + list.length
                    + " имена=" + root.screenNames());

        // Watching is started once and only when there is something to compare a
        // foreign window's size against.
        if (!root.audioStarted) {
            var size = root.audioProbeSize();
            if (size[0] > 0 && size[1] > 0) {
                root.audioStarted = true;
                root.audio.start(size[0], size[1]);
            }
        }
    }

    /// Screen size the supervisor compares foreign windows against: the
    /// SMALLEST of the screens — the same rule for the same reason as for video
    /// (`WallpaperVideo.qml`, `audioProbeSize`): muting needlessly is a trifle,
    /// playing over a film is a disaster.
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

    /// Comma-separated screen names — for the log.
    function screenNames() {
        var names = [];
        for (var i = 0; i < root.screenList.length; ++i)
            names.push(root.screenList[i].name);
        return names.length > 0 ? names.join(",") : "нет";
    }

    /// Names of the screens the windows ACTUALLY landed on — one per window.
    ///
    /// Asked from the layer surface itself, not from the screen list: "there
    /// are two screens" and "the wallpaper landed on both" are different
    /// statements, and for years the first looked like the second.
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
        // Offscreen mode knows nothing about screens and has no reason to wait
        // for them to appear.
        if (!root.audioStarted) {
            var size = root.audioProbeSize();
            root.audioStarted = true;
            root.audio.start(size[0], size[1]);
        }
        if (root.selftestMs > 0)
            selftest.start();
    }

    // ── Self-test ─────────────────────────────────────────────────────
    //
    // The report is printed as ONE line and parsed by the parent half of the
    // test bench: "the window came up" and "the page loaded" are different
    // statements, and silence must not look like success.
    //
    // Warning: what is checked is what decides the outcome, not what is
    // convenient to ask: `loadStatus` differs between a page with a broken
    // link and a black but intact page, while `loadProgress` reaches 100 for
    // both. And separately — whether the WE stub took hold: without it a
    // Workshop wallpaper aborts at its very first check, and outwardly this
    // is the same black screen.
    //
    // Warning: `screens` and `windows` are not decoration either: "the
    // wallpaper came up" and "the wallpaper landed on ALL monitors" are
    // different statements, and for years the first looked like the second.
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
                                + " audioOn=" + root.audio.audioOn
                                + " audioPaused=" + root.audio.paused
                                + " audioReason=" + root.audio.reason
                                + " audioMuted=" + top.view.audioMuted
                                + " probe=" + probe);
                    top.visible = false;
                    // Code 5 — "the page did not load". The verdict is based on the load
                    // result, not on the percentage: for a broken link the percentage reaches
                    // a hundred too.
                    if (root.loadStatus !== WebEngineView.LoadSucceededStatus) {
                        Qt.exit(5);
                        return;
                    }
                    // Code 3 — "the frame stream did not start". Black frames used to count
                    // as success (WebGL wallpapers on offscreen), so a frame that is black over
                    // its whole area is also a failure: the extension would show the user a
                    // black screen.
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
