// Snapshot of a KDE window decoration for a tile of the «Window Decorations» section.
//
// The decoration is drawn by the KDecoration module itself through the KWin QML module
// org.kde.kwin.private.kdecoration — the same one the Plasma «Window Decorations» window
// uses to draw its tiles (kwin/src/kcms/decoration/ui/Themes.qml):
// an inactive window behind, an active one in front shifted by the title bar height.
// It is started as a separate `qml` process (QT_QPA_PLATFORM=offscreen) so that
// the foreign module is not loaded into the Rexon process.
//
// Arguments after `--`: module, theme, window title, border size index
// (0 — None … 8 — Oversized), where to save the PNG.
// Exit: 0 — the snapshot was saved; 2 — the module or theme was not found; 3 — the snapshot
// was not saved.

import QtQuick
import QtQuick.Window
import org.kde.kwin.private.kdecoration as KDecoration

Window {
    id: root

    readonly property var args: Qt.application.arguments.slice(Qt.application.arguments.length - 5)
    readonly property int mainMargin: 8

    function place(item, marginTopLeft, marginBottomRight) {
        const shd = item.shadow;
        item.anchors.leftMargin = mainMargin + marginTopLeft - (shd ? shd.paddingLeft : 0);
        item.anchors.rightMargin = mainMargin + marginBottomRight - (shd ? shd.paddingRight : 0);
        item.anchors.topMargin = mainMargin + marginTopLeft - (shd ? shd.paddingTop : 0);
        item.anchors.bottomMargin = mainMargin + marginBottomRight - (shd ? shd.paddingBottom : 0);
    }

    width: 300
    height: 170
    visible: true
    color: "transparent"

    Item {
        id: frame

        anchors.fill: parent

        KDecoration.Bridge {
            id: bridgeItem

            plugin: root.args[0]
            theme: root.args[1]
        }
        KDecoration.Settings {
            id: settingsItem

            bridge: bridgeItem.bridge
            Component.onCompleted: settingsItem.borderSizesIndex = parseInt(root.args[3])
        }
        KDecoration.Decoration {
            id: inactivePreview

            bridge: bridgeItem.bridge
            settings: settingsItem
            anchors.fill: parent
            onShadowChanged: root.place(inactivePreview, 0, client.decoration.titleBar.height)
            Component.onCompleted: {
                client.active = false;
                client.caption = root.args[2];
                root.place(inactivePreview, 0, client.decoration.titleBar.height);
            }
        }
        KDecoration.Decoration {
            id: activePreview

            bridge: bridgeItem.bridge
            settings: settingsItem
            anchors.fill: parent
            onShadowChanged: root.place(activePreview, client.decoration.titleBar.height, 0)
            Component.onCompleted: {
                client.active = true;
                client.caption = root.args[2];
                root.place(activePreview, client.decoration.titleBar.height, 0);
            }
        }
    }

    // QML themes of the Aurorae engine finish drawing with a delay — the snapshot is taken after
    // half a second, as soon as the window frame is ready.
    Timer {
        interval: 500
        running: true
        onTriggered: {
            if (!bridgeItem.valid) {
                Qt.exit(2);
                return;
            }
            frame.grabToImage(function (result) {
                Qt.exit(result.saveToFile(root.args[4]) ? 0 : 3);
            });
        }
    }
}
