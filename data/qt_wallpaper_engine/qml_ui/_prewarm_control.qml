// Warm-up of the Qt-Protobuf registration of the waywallen.control module on the MAIN
// thread. Without it the QQuickWidget hangs in QQmlThread on the first import of
// waywallen.control (deadlock in the global registration of protobuf types).
// Loaded once through QQmlApplicationEngine from host._prewarm_control().
import QtQuick
import waywallen.control as WC

QtObject {
    readonly property int _warm: WC.SortDirection.SORT_DIRECTION_ASC
}
