// Прогрев регистрации Qt-Protobuf модуля waywallen.control на ГЛАВНОМ
// потоке. Без этого QQuickWidget зависает в QQmlThread при первом импорте
// waywallen.control (дедлок глобальной регистрации protobuf-типов).
// Грузится один раз через QQmlApplicationEngine из host._prewarm_control().
import QtQuick
import waywallen.control as WC

QtObject {
    readonly property int _warm: WC.SortDirection.SORT_DIRECTION_ASC
}
