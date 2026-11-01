// Окно настроек рамки окна KDE — кнопка «Настроить…» раздела «Оформление
// окон».
//
// То же окно, что открывает кнопка правки на плитке окна «Оформление окон»
// Plasma: QML-модуль KWin org.kde.kwin.private.kdecoration сам собирает
// диалог с модулем настроек рамки (org.kde.kdecoration3.kcm/<модуль>) и
// передаёт ему тему — у тем Aurorae без неё настроек не показать. После «OK»
// или «Применить» он же просит KWin перечитать настройки.
// Запускается отдельным процессом `qml -a widget` на экране сеанса, чтобы
// чужой модуль не загружался в процесс Rexon; процесс завершается сам, когда
// диалог закрыт.
//
// Аргументы после `--`: модуль рамки, тема, модуль настроек, заголовок окна.
// Выход: 0 — диалог закрыт; 2 — модуль рамки не нашёлся.

import QtQuick
import QtQuick.Window
import org.kde.kwin.private.kdecoration as KDecoration

Window {
    id: root

    readonly property var args: Qt.application.arguments.slice(Qt.application.arguments.length - 4)

    // Своё окно не нужно — показывается только диалог модуля.
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
        // Рамка-образец нужна диалогу ради заголовка окна, как в KDE.
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
