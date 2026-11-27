// Rexon video wallpapers: ONE player per process, ONE WINDOW PER SCREEN.
//
// There used to be three nearly identical files (rexon/qml/VideoWallpaper.qml,
// VideoWallpaperX11.qml, VideoWallpaperOffscreen.qml), and they differed
// exactly in how the window gets into the background. Once the layer is
// set by Rust rather than by an attached QML property, that difference
// leaves the markup entirely — what remains is one player and three ways
// to place the window, managed by the host (rule #12: a shared mechanism
// instead of a copy per scenario).
//
// Warning: second monitor (16.09.2026). Before this, the file root was ONE
// window of size `Screen.width × Screen.height`, i.e. the size of the
// screen the compositor put it on: with two monitors only one got the
// wallpaper, and the second kept the desktop's wallpaper. Now the root is
// not a window but a window holder: an `Instantiator` creates a window for
// every connected screen and removes the windows of monitors that went
// away. A monitor appearing and disappearing on the fly is caught with the
// `Qt.application.onScreensChanged` signal.
//
// What REMAINED single per process (and must remain so):
//   * the host process itself — one per wallpaper, not per monitor;
//   * the audio supervisor `WallpaperAudio` — one, at the root;
//   * the sounding player — only on the FIRST screen
//     (`QGuiApplication::screens()` returns the primary first); the others
//     have no `audioOutput` assigned at all, and no audio stream.
//
// Scenarios (the `placement` property, decided by the core
// `wallpaper::video_qml_file`):
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
// (QQmlApplicationEngine::setInitialProperties); names and defaults are
// computed by the core (`rexon-core::wallpaper::video_context_properties`).
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import QtMultimedia
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Initial properties from the host ──────────────────────────────
    property url videoUrl
    property bool isWindowed: false
    property bool audioEnabled: true
    property real audioVolume: 0.5
    property bool loopVideo: true
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
    /// Volume set with the `--volume` option, or -1.
    ///
    /// A manual run must sound exactly as requested on the command line — so
    /// a given option cancels following the setting. This option never comes
    /// from the product: there the volume is taken from the settings so the
    /// slider works live.
    property real volumeFixed: -1

    /// The "audio requested, no track" complaint has already been made. Once
    /// per process: only the primary screen's window plays sound, and all
    /// windows share one file.
    property bool noAudioWarned: false
    /// Audio watching has already been started (it is started once per
    /// process).
    property bool audioStarted: false

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
    /// no screen is assigned to it": the delegate model understands an array
    /// with `null` inside worse than an empty list.
    readonly property var surfaceModel: root.perScreen ? root.screenList : ["single"]

    /// The primary screen's window: sound, the frame stream and the self-test
    /// report belong to it. The primary screen comes first in Qt's `screens()`.
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

    // The Rust layer configurator — ONE for all windows. Its defaults are
    // already the wallpaper layer, so a forgotten setting gives a wallpaper,
    // not a window on top of everything.
    property WallpaperLayer layerShell: WallpaperLayer {
        scope: "rexon-video-wallpaper"
    }

    // The other two ways to place the window (xprop on X11 and the frame
    // stream into a shared file) also come from Rust, with the same "the
    // object configures the window passed to it" approach.
    property WallpaperPlacement placer: WallpaperPlacement {}

    // Heartbeat for the Rexon live wallpaper watchdog: request a frame from
    // every window and check in. If rendering hangs, this stream stops too,
    // the mark stops updating, and the watchdog brings the wallpaper back up
    // (after sleep — immediately). Decision and timeouts belong to the core
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

    // Live audio state: settings, muting with a full-screen window and with
    // foreign audio. Rust computes the whole decision (see
    // src/wallpaper_audio.rs and rexon-core::wallpaper_audio); here there are
    // only bindings — so a change of the setting reaches the player without
    // restarting the wallpaper. Warning: there is ONE object per process, not
    // per window: otherwise every monitor would start its own window manager
    // polling thread.
    property WallpaperAudio audio: WallpaperAudio {}

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
            /// The primary screen's window — the only one that plays sound.
            readonly property bool primary: surface.index === 0

            property bool layerApplied: false
            property bool x11Applied: false
            property bool mmapStarted: false
            property alias player: player

            // Warning: the order is mandatory: the window is created INVISIBLE, the
            // layer is set in Component.onCompleted, and only then visible = true. A
            // shown window has already become an ordinary xdg_toplevel and will not
            // move into the layer.
            visible: false
            color: "black"
            flags: root.isWindowed ? Qt.Window : Qt.FramelessWindowHint
            title: root.isWindowed ? qsTranslate("rexon", "Rexon — video wallpaper") : ""

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
                            console.warn("REXON-WPH-WARN обои не прицелились на экран "
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
                    console.warn("REXON-WPH-WARN окно X11 не подготовлено к роли рабочего стола");
                surface.visible = true;

                if (root.placement === "offscreen") {
                    // The window is not put on screen: frames go to the shell extension
                    // (GNOME/Cinnamon on Wayland — there is no layer-shell there). The frame
                    // is taken from the `vout` sink, not from a window snapshot: the host is
                    // on the offscreen platform here, and VideoOutput does not render there —
                    // a snapshot would be black.
                    surface.mmapStarted = root.placer.startMmapStream(
                        surface, vout, root.mmapPath, root.renderWidth,
                        root.renderHeight, root.mmapFps);
                    if (!surface.mmapStarted)
                        console.warn("REXON-WPH-ERROR поток кадров не запустился: "
                                     + root.mmapPath);
                } else if (root.placement === "x11" && !root.isWindowed) {
                    // After showing — EWMH requests to the root window: that is how the state
                    // of an already shown window is changed.
                    x11Settle.start();
                }
            }

            // Pause/resume depending on the situation. A separate function because it
            // has to be called both when the decision changes and when the media has
            // reloaded.
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

            // Warning: an audio output EXISTS only while sound is on, and this is not
            // nitpicking. In Qt6 a `MediaPlayer` without an assigned `audioOutput` has
            // no device at all, while with an assigned and muted one the device is
            // open: the stream hangs in the sound card for the whole session, the
            // system mixer shows a "RexonVideoWallpaper" slider that plays nothing,
            // and the receiver never goes to sleep because of a permanent client.
            // "Sound off" must mean "no stream", not "a stream at zero", so the
            // output is detached rather than muted. For the same reason windows of
            // NON-primary screens never get an output: otherwise the wallpaper sound
            // would play as many times as there are monitors.
            AudioOutput {
                id: audioOut
                volume: root.volumeFixed >= 0 ? root.volumeFixed : root.audio.volume
            }

            MediaPlayer {
                id: player
                source: root.videoUrl
                videoOutput: vout
                // Warning: the `--audio` command line option remains a veto: it comes from
                // whoever launched the wallpaper ("this wallpaper is silent"), while the
                // setting comes from the user. Neither of the two must silently cancel the
                // other, so sound plays when BOTH allow it — and only on the primary
                // screen.
                audioOutput: (surface.primary && root.audioEnabled && root.audio.audioOn)
                             ? audioOut : null
                loops: root.loopVideo ? MediaPlayer.Infinite : 1
                // Playback starts when the media is ready.
                onMediaStatusChanged: {
                    if (mediaStatus === MediaPlayer.LoadedMedia
                            || mediaStatus === MediaPlayer.BufferedMedia) {
                        // Through the shared function, not a direct play(): the decision "play or
                        // stand still" is one for all triggers, otherwise reloading the media
                        // would lift a pause set because of a full-screen window.
                        surface.syncPlayback();
                        // Audio was requested, but the file has no audio track. A separate
                        // complaint is needed because the player ITSELF reports this in no way:
                        // playbackState, mediaStatus, volume and the mute flag of a silent file
                        // are exactly the same as for a file with sound — "the player is happy"
                        // and "there will be no sound" look identical.
                        // Once per process: readiness arrives twice (LoadedMedia, then
                        // BufferedMedia), and there are as many windows as monitors — a repeated
                        // complaint would read as several different problems.
                        if (surface.primary && root.audioEnabled && !hasAudio
                                && !root.noAudioWarned) {
                            root.noAudioWarned = true;
                            console.warn("REXON-WPH-NOAUDIO звук включён, но в файле"
                                         + " нет звуковой дорожки: " + root.videoUrl);
                        }
                    }
                }
                onErrorOccurred: (err, msg) => console.warn("REXON-WPH-ERROR " + err + " " + msg)

                // Pause and resume follow the watcher's decision. Held by a binding to the
                // state, not by a one-off command: otherwise after any media reload the
                // wallpaper would remain stopped.
                Component.onCompleted: surface.syncPlayback()
            }

            VideoOutput {
                id: vout
                anchors.fill: parent
                // Fill the whole screen without black bars — like Wallpaper Engine
                // wallpapers.
                fillMode: VideoOutput.PreserveAspectCrop
            }

            // ── First frame of the clip on screen ─────────────────────
            // Wait measurement and, on Cinnamon with layer-shell, moving the
            // background service under us — on the event rather than after a delay:
            // until the first frame of the clip the window is black, and there is no
            // point showing it earlier. A clip frame has arrived in the sink — the
            // next displayed window frame already contains it. In offscreen mode the
            // window never goes on screen, so there is nothing to measure.
            property bool videoFrameSeen: false
            property bool firstFrameReported: false

            Connections {
                target: vout.videoSink
                enabled: root.placement !== "offscreen" && !surface.videoFrameSeen
                function onVideoFrameChanged() { surface.videoFrameSeen = true; }
            }

            Connections {
                target: surface
                enabled: surface.videoFrameSeen && !surface.firstFrameReported
                function onFrameSwapped() {
                    surface.firstFrameReported = true;
                    root.placer.firstFrameShown("video");
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
    /// in a row, i.e. it would burn the CPU exactly when the user has other
    /// things on their mind (rule #13).
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
        console.log("REXON-WPH-SCREENS экранов=" + list.length
                    + " имена=" + root.screenNames());

        // Audio watching is started once and only when there is something to
        // compare a foreign window's size against.
        if (!root.audioStarted) {
            var size = root.audioProbeSize();
            if (size[0] > 0 && size[1] > 0) {
                root.audioStarted = true;
                root.audio.start(size[0], size[1]);
            }
        }
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

    /// How many windows hold an audio output. Must be 0 or 1: only the primary
    /// screen's window plays sound, otherwise the wallpaper would sound as
    /// many times as there are monitors.
    function audioWindows() {
        var n = 0;
        for (var i = 0; i < root.surfaces.count; ++i) {
            var s = root.surfaces.objectAt(i);
            if (s && s.player && s.player.audioOutput)
                n += 1;
        }
        return n;
    }

    /// Screen size the audio watcher compares foreign windows against.
    ///
    /// Warning: the SMALLEST of the screens is taken, and this is a deliberate
    /// choice. The core decides "the window is full screen" by comparing
    /// `width ≥ screen` (`wallpaper_audio::kwin_window_conditions`), and the
    /// window manager does not report which monitor the foreign window is on.
    /// Using the largest screen, a film maximized on a small monitor would not
    /// count as full screen at all, and the wallpaper would keep playing over
    /// it; using the smallest, in the worst case we mute too early. Muting
    /// needlessly is a minor annoyance, playing over a film is exactly the
    /// disaster the watcher was written for.
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
    // test bench: "the window came up" and "the video started" are different
    // statements, and silence must not look like success.
    //
    // Warning: the report once lied about sound by omission. It printed the
    // volume and the mute flag — that is, the output SETTING — but not whether
    // the file has an audio track at all. For a silent file the line came out
    // exactly the same as for a file with sound ("playback=1 status=5
    // hasVideo=true volume=0.7 muted=false"), and the self-test went green
    // where nothing reached the sound card. Hence `hasAudio`, `audioTracks`
    // and the device name: what is checked is what decides the outcome, not
    // what is convenient to ask. And the verdict is an exit code, not just a
    // line: audio requested but no track is a failure of the run, not a
    // footnote to it.
    //
    // Warning: `screens` and `windows` in the report are not decoration
    // either: "the wallpaper came up" and "the wallpaper landed on ALL
    // monitors" are different statements, and for years the first looked like
    // the second.
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
            // Code 4 — "audio was requested, but there is nothing to play it with":
            // either no output was created or the file has no audio track.
            // Warning: the verdict is based on the PRESENCE of an audio track, not on
            // whether an output is attached right now. Normal operation detaches the
            // output too: a full-screen window, foreign audio, an unchecked box. That
            // is a decision, not a breakage, and it must not fail the run — otherwise
            // the self-test would start measuring the setting again.
            var zvukOk = !root.audioEnabled || player.hasAudio;
            Qt.exit(zvukOk ? 0 : 4);
        }
    }
}
