// Test subject of the idle screensaver overlay bench
// (`layer_shell_selftest::overlay`, `rexon-gui-selftest --overlay-selftest`).
//
// One window per screen on the OVERLAY layer with the exclusive keyboard —
// exactly what the screensaver will show (`LayerSpec::SCREENSAVER`, set by
// Rust through `applyScreensaverLayer`). The window reports what it got from
// the compositor and closes on the first key, click, wheel turn or a pointer
// move past the core's threshold (`idle_screensaver::WAKE_MOTION_PX`, passed
// in as `wakeMotionPx`).
//
// ⛔ This window covers EVERYTHING and takes the keyboard. It is meant for a
// nested compositor in an isolated environment (`--overlay-stand`), never for
// a live desktop; the parent half of the bench refuses to start it on the
// person's own session. Two limiters keep it from becoming an invisible wall
// even there: the "no input" timer below and the process watchdog in Rust.
//
// The report lines (REXON-OV-*) are parsed by the parent; the verdict itself
// comes from the Wayland protocol trace, not from these lines.
pragma ComponentBehavior: Bound

import QtQml
import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

QtObject {
    id: root

    // ── Set by Rust (setInitialProperties) ───────────────────────────
    /// A deliberate breakage for the bite run; empty — an honest run.
    ///   layer    — LayerTop instead of Overlay (panels would cover it);
    ///   keyboard — no keyboard (the waking key goes to the program below);
    ///   screens  — a window only on the first screen;
    ///   pin      — the surface is not aimed at its screen;
    ///   deaf     — input does not close the window.
    property string bite: ""
    /// Pointer travel that counts as "the user is back" (core constant).
    property real wakeMotionPx: 10
    /// How long to wait for input after READY before giving up.
    property int inputWaitMs: 8000

    // ── State ────────────────────────────────────────────────────────
    property var screenList: {
        var all = Qt.application.screens;
        var list = [];
        for (var i = 0; i < all.length; ++i)
            list.push(all[i]);
        return root.bite === "screens" && list.length > 1 ? [list[0]] : list;
    }
    property int reportedWindows: 0
    property bool focusSeen: false
    property bool ready: false
    property bool finished: false
    /// First pointer position seen after READY: motion is measured from it.
    property var pointerAnchor: null

    property WallpaperLayer layerShell: WallpaperLayer {
        scope: "rexon-overlay-selftest"
    }

    function wake(reason) {
        if (!root.ready || root.finished || root.bite === "deaf")
            return;
        root.finish(reason);
    }

    function pointerAt(win, x, y) {
        if (!root.ready || root.finished)
            return;
        // Global coordinates: the pointer may enter another screen's window. The
        // screen's corner, not the window's x/y: a layer surface's position is
        // not ours to know.
        var gx = win.modelData.virtualX + x;
        var gy = win.modelData.virtualY + y;
        if (root.pointerAnchor === null) {
            // The enter event right after showing is not the user coming back.
            root.pointerAnchor = { x: gx, y: gy };
            return;
        }
        var dx = gx - root.pointerAnchor.x;
        var dy = gy - root.pointerAnchor.y;
        if (Math.hypot(dx, dy) >= root.wakeMotionPx)
            root.wake("motion");
    }

    function finish(reason) {
        if (root.finished)
            return;
        root.finished = true;
        console.log("REXON-OV-EXIT reason=" + reason);
        // Off the screen BEFORE exiting: an exclusive-keyboard surface must not
        // linger for even a frame longer than needed.
        for (var i = 0; i < root.windows.count; ++i) {
            var w = root.windows.objectAt(i);
            if (w)
                w.visible = false;
        }
        Qt.exit(0);
    }

    function maybeReady() {
        if (root.ready || root.reportedWindows < root.windows.count || root.windows.count === 0)
            return;
        // The grace period counts from the moment every window is on screen, not
        // from start-up: a slow start must not eat it. A stand seat without any
        // keyboard never gives focus before the bench injects one — READY then
        // says focus=false, and the verdict looks for wl_keyboard.enter later.
        if (!root.focusSeen && !root.graceOver) {
            if (!root.focusGrace.running)
                root.focusGrace.start();
            return;
        }
        root.ready = true;
        console.log("REXON-OV-READY windows=" + root.windows.count
                    + " screens=" + Qt.application.screens.length
                    + " focus=" + root.focusSeen);
        root.inputWait.start();
    }

    /// Focus may come a little after the geometry; wait for it, but not forever.
    property Timer focusGrace: Timer {
        interval: 1500
        onTriggered: {
            root.graceOver = true;
            root.maybeReady();
        }
    }
    property bool graceOver: false

    property Timer inputWait: Timer {
        interval: root.inputWaitMs
        onTriggered: root.finish("timeout")
    }

    property Instantiator windows: Instantiator {
        model: root.screenList

        delegate: Window {
            id: win

            required property int index
            required property var modelData

            property bool layerApplied: false
            property bool pinned: false
            property bool reported: false

            visible: false
            color: "#0e141b"
            flags: Qt.FramelessWindowHint
            title: ""
            // Assigned BEFORE showing: LayerShellQt reads QWindow::screen().
            screen: win.modelData
            width: win.modelData.width
            height: win.modelData.height

            Component.onCompleted: {
                switch (root.bite) {
                case "layer":
                    root.layerShell.layer = 2;                  // LayerTop
                    root.layerShell.anchors = 15;
                    root.layerShell.exclusionZone = -1;
                    root.layerShell.keyboardInteractivity = 1;
                    win.layerApplied = root.layerShell.apply(win);
                    break;
                case "keyboard":
                    root.layerShell.layer = 3;                  // LayerOverlay
                    root.layerShell.anchors = 15;
                    root.layerShell.exclusionZone = -1;
                    root.layerShell.keyboardInteractivity = 0;  // None
                    win.layerApplied = root.layerShell.apply(win);
                    break;
                default:
                    win.layerApplied = root.layerShell.applyScreensaverLayer(win);
                    break;
                }
                if (win.layerApplied && root.bite !== "pin")
                    win.pinned = root.layerShell.pinScreen(win);
                win.visible = true;
                win.settle.start();
            }

            onActiveChanged: {
                if (win.active && !root.focusSeen) {
                    root.focusSeen = true;
                    console.log("REXON-OV-FOCUS screen=" + win.modelData.name);
                    root.maybeReady();
                }
            }

            // A protocol round trip before reading the geometry the compositor gave.
            property Timer settle: Timer {
                interval: 700
                onTriggered: {
                    if (win.reported)
                        return;
                    win.reported = true;
                    console.log("REXON-OV-WINDOW index=" + win.index
                                + " screen=" + win.modelData.name
                                + " applied=" + win.layerApplied
                                + " available=" + root.layerShell.available()
                                + " pinned=" + win.pinned
                                + " target=" + root.layerShell.screenName(win)
                                + " layer=" + root.layerShell.readLayer(win)
                                + " anchors=" + root.layerShell.readAnchors(win)
                                + " exclusionZone=" + root.layerShell.readExclusionZone(win)
                                + " keyboard=" + root.layerShell.readKeyboardInteractivity(win)
                                + " windowW=" + win.width + " windowH=" + win.height
                                + " screenW=" + win.modelData.width
                                + " screenH=" + win.modelData.height);
                    root.reportedWindows += 1;
                    root.maybeReady();
                }
            }

            Item {
                anchors.fill: parent
                focus: true
                Keys.onPressed: (event) => {
                    // The waking key is swallowed (`ShowPolicy::eat_wake_input`).
                    event.accepted = true;
                    root.wake("key");
                }

                MouseArea {
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.AllButtons
                    cursorShape: Qt.BlankCursor
                    onPositionChanged: (mouse) => root.pointerAt(win, mouse.x, mouse.y)
                    onPressed: root.wake("click")
                    onWheel: root.wake("wheel")
                }
            }

            // Bench marker, so a screenshot of the stand shows whose surface it is.
            Text {
                anchors.centerIn: parent
                text: "Rexon overlay probe · " + win.modelData.name
                color: "#66c0f4"
                font.pixelSize: 18
            }
        }
    }
}
