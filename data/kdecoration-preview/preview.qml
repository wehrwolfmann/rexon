// Снимок рамки окна KDE для плитки раздела «Оформление окон».
//
// Рамку рисует сам модуль KDecoration через QML-модуль KWin
// org.kde.kwin.private.kdecoration — тот же, которым окно «Оформление окон»
// Plasma рисует свои плитки (kwin/src/kcms/decoration/ui/Themes.qml):
// неактивное окно сзади, активное спереди со сдвигом на высоту заголовка.
// Запускается отдельным процессом `qml` (QT_QPA_PLATFORM=offscreen), чтобы
// чужой модуль не загружался в процесс Rexon.
//
// Аргументы после `--`: модуль, тема, заголовок окна, номер размера рамки
// (0 — None … 8 — Oversized), куда сохранить PNG.
// Выход: 0 — снимок сохранён; 2 — модуль или тема не нашлись; 3 — снимок не
// сохранился.

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

    // Темы QML движка Aurorae дорисовываются не сразу — снимок через
    // полсекунды, как только кадр окна готов.
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
