pragma ComponentBehavior: Bound
import QtCore
import QtQml
import QtQuick.Window

QtObject {
    id: root

    required property Window window
    property bool restored: false

    readonly property Settings settings: Settings {
        id: storedWindow
        category: "window"
        property int width: 948
        property int height: 632
        property bool maximized: false
    }

    function restore() {
        if (restored)
            return;
        window.width = storedWindow.width > 0 ? storedWindow.width : 948;
        window.height = storedWindow.height > 0 ? storedWindow.height : 632;
        if (storedWindow.maximized)
            window.showMaximized();
        else
            window.showNormal();
        restored = true;
    }

    function saveNormalSize() {
        if (!restored || window.visibility !== Window.Windowed)
            return;
        if (window.width > 0)
            storedWindow.width = window.width;
        if (window.height > 0)
            storedWindow.height = window.height;
    }

    readonly property Connections windowConnections: Connections {
        target: root.window

        function onWidthChanged() {
            // Wayland can deliver maximized geometry before the window state.
            Qt.callLater(root.saveNormalSize);
        }

        function onHeightChanged() {
            Qt.callLater(root.saveNormalSize);
        }

        function onClosing() {
            root.saveNormalSize();
        }

        function onVisibilityChanged() {
            if (!root.restored)
                return;
            if (root.window.visibility === Window.Maximized)
                storedWindow.maximized = true;
            else if (root.window.visibility === Window.Windowed)
                storedWindow.maximized = false;
        }
    }
}
