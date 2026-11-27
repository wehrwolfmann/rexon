// Test subject window of the wallpaper layer test bench: the same window
// that the active Python path brings up (rexon/qml/WebWallpaper.qml), but
// the layer is set by Rust — through WallpaperLayer from org.rexon.gui,
// without Python in the process.
//
// The window is deliberately almost transparent: the test bench runs on
// the LIVE desktop, and it has no business covering Wehrwolfmann's
// wallpaper. Only a small label in the centre is visible, and only for a
// second and a half, in the BACKGROUND layer, i.e. below all windows. The
// test bench does not touch the user's wallpaper at all — it only shows
// its own window on top of it and closes it.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import org.rexon.gui 1.0

Window {
    id: root

    // Warning: order matters: the window is created INVISIBLE, the layer is
    // set in Component.onCompleted, and only then visible = true. The other
    // way round is impossible — a shown window has already become an ordinary
    // xdg_toplevel and will not move into the layer.
    visible: false
    color: "transparent"
    flags: Qt.FramelessWindowHint
    width: Screen.width
    height: Screen.height
    title: ""

    // The Rust layer configurator. Its defaults are already the wallpaper
    // layer; applyWallpaperLayer() sets the canonical set regardless of the
    // properties.
    WallpaperLayer {
        id: layerShell
        scope: "rexon-layer-shell-selftest"
    }

    property bool applied: false
    property bool reported: false

    // Warning: a bite test: a test bench that is always green proves nothing.
    // Here a KNOWN breakage of the layer is set up, and the parent half of the
    // test bench requires the run to go red. The value comes from Rust through
    // setInitialProperties (the REXON_LS_BITE variable); empty by default —
    // a normal run.
    //
    // The mutations are not made up: each one is a real way to break a
    // wallpaper.
    //   layer    — layer Top: the wallpaper would sit ON TOP of windows;
    //   anchors  — a single anchor: the wallpaper would not stretch over the
    //              screen;
    //   zone     — zone 0: panels would start pushing the wallpaper aside;
    //   keyboard — the wallpaper grabs the keyboard;
    //   size     — no anchors at all, a 640×360 window instead of the screen;
    //   late     — the layer is set AFTER the window is shown: it has already
    //              become an ordinary xdg_toplevel and will not move into the
    //              layer (the ordering trap).
    property string bite: ""

    Component.onCompleted: {
        if (root.bite === "size") {
            // A deliberately wrong size: without anchors the layer surface takes its
            // size from set_size, and the "full screen" check must fail.
            root.width = 640;
            root.height = 360;
        }
        if (root.bite === "late") {
            root.visible = true;
            root.applied = layerShell.applyWallpaperLayer(root);
        } else {
            root.applied = root.applyLayer();
            root.visible = true;
        }
        settle.interval = root.bite === "" ? 1500 : 800;
        settle.start();
    }

    // Sets the layer: the canonical set or one spoiled by the bite.
    function applyLayer() {
        switch (root.bite) {
        case "":
        case "late":
        case "size":
            break;
        case "layer":
            layerShell.layer = 2;            // LayerTop
            break;
        case "anchors":
            layerShell.anchors = 1;          // top only
            break;
        case "zone":
            layerShell.exclusionZone = 0;
            break;
        case "keyboard":
            layerShell.keyboardInteractivity = 1;  // Exclusive
            break;
        default:
            console.log("REXON-LS-BITE-UNKNOWN " + root.bite);
            break;
        }
        if (root.bite === "size")
            layerShell.anchors = 0;          // AnchorNone: the window decides the size
        // The canonical layer is set ONLY in a clean run: apply() takes the
        // object's properties, i.e. it would carry the bite to the compositor.
        return root.bite === ""
            ? layerShell.applyWallpaperLayer(root)
            : layerShell.apply(root);
    }

    // Pause for a protocol round trip: several frames pass between showing the
    // window and the compositor's reply (configure), and without it the
    // geometry would be read before KWin handed it out.
    Timer {
        id: settle
        // The value is set in Component.onCompleted: a bite run has no reason to
        // wait that long, and the window hangs on the LIVE desktop.
        interval: 1500
        repeat: false
        onTriggered: root.report()
    }

    function report() {
        if (root.reported)
            return;
        root.reported = true;

        console.log("REXON-LS-APPLY applied=" + root.applied
                    + " available=" + layerShell.available());
        // Reading back from LayerShellQt only proves that the setters were not
        // ignored. The real proof is the protocol; the parent process of the test
        // bench parses it.
        console.log("REXON-LS-READBACK layer=" + layerShell.readLayer(root)
                    + " anchors=" + layerShell.readAnchors(root)
                    + " exclusionZone=" + layerShell.readExclusionZone(root)
                    + " keyboard=" + layerShell.readKeyboardInteractivity(root));
        console.log("REXON-LS-GEOMETRY windowW=" + root.width
                    + " windowH=" + root.height
                    + " screenW=" + Screen.width
                    + " screenH=" + Screen.height);

        // The window is removed BEFORE exit, not "sometime when the process ends":
        // on a live desktop an extra frame of a foreign window is already a defect.
        root.visible = false;
        Qt.exit(0);
    }

    // The label makes the run visible to a human, not only in the log.
    Rectangle {
        anchors.centerIn: parent
        width: label.implicitWidth + 32
        height: label.implicitHeight + 20
        radius: 6
        // Base/Text from the steam_dark skin (rule #17).
        color: "#16202d"
        opacity: 0.85

        Text {
            id: label
            anchors.centerIn: parent
            text: "Rexon · проверка слоя обоев"
            color: "#66c0f4"
            font.pixelSize: 16
        }
    }
}
