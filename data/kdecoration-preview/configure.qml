// KDE window decoration settings window — the «Configure…» button of the «Window
// Decorations» section.
//
// The same window that the edit button on a tile of the Plasma «Window Decorations»
// window opens: the KWin QML module org.kde.kwin.private.kdecoration builds the
// dialog with the decoration's settings module (org.kde.kdecoration3.kcm/<module>) itself and
// passes it the theme — Aurorae themes cannot show settings without it. After «OK»
// or «Apply» it also asks KWin to reread the settings.
// It is started as a separate `qml -a widget` process on the session screen so that
// the foreign module is not loaded into the Rexon process; the process exits by itself when
// the dialog is closed.
//
// Arguments after `--`: decoration module, theme, settings module, window title.
// Exit: 0 — the dialog was closed; 2 — the decoration module was not found.

import QtQuick
import QtQuick.Window
import org.kde.kwin.private.kdecoration as KDecoration

Window {
    id: root

    readonly property var args: Qt.application.arguments.slice(Qt.application.arguments.length - 4)

    // No window of our own is needed — only the module's dialog is shown.
    visible: false

    Item {
        id: anchor

        KDecoration.Bridge {
            id: bridgeItem

            plugin: root.args[0]
            theme: root.args[1]
            kcmoduleName: root.args[2]
        }
        KDecoration.Settings {
            id: settingsItem

            bridge: bridgeItem.bridge
        }
        // The dialog needs a sample decoration for the window title, as in KDE.
        KDecoration.Decoration {
            bridge: bridgeItem.bridge
            settings: settingsItem
            Component.onCompleted: client.caption = root.args[3]
        }
    }

    Component.onCompleted: {
        Qt.application.displayName = root.args[3];
        if (!bridgeItem.valid) {
            Qt.exit(2);
            return;
        }
        bridgeItem.bridge.configure(anchor);
    }
}
