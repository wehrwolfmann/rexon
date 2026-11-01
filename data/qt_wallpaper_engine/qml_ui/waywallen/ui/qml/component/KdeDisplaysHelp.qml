pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W

ColumnLayout {
    id: root

    readonly property string githubUrl: "https://github.com/waywallen/waywallen-display"
    readonly property string kdeStoreUrl: "https://store.kde.org/p/2356221"

    // ПРАВКА REXON (починка дефекта, не доработка): Rexon сам ставит
    // плагин обоев Plasma и назначает его по «Применить», поэтому вместо
    // «поставьте расширение из GitHub/KDE Store и выберите его в настройках
    // стола» показываем, что делать на самом деле. На X11 вне KDE (MATE,
    // XFCE, Cinnamon X11) вывод поднимает хост Rexon — там подсказка
    // показывается при любом бэкенде демона, иначе под «Мониторы не
    // зарегистрированы» было пусто. Текст — ядро Rexon
    // (`W.App.noDisplaysHint`); пусто — вид апстрима. Правка живёт в
    // patches/0010-apply-without-displays-on-kde.patch.
    readonly property string rexonHint: W.App.noDisplaysHint

    spacing: 12
    visible: W.Notify.displayBackend.name === "kde-plasma" || root.rexonHint.length > 0

    MD.Text {
        Layout.fillWidth: true
        text: root.rexonHint.length > 0 ? root.rexonHint : qsTr("KDE Plasma needs the <b>waywallen-display</b> wallpaper extension to bridge wallpapers to the desktop. Install it from either source:")
        textFormat: Text.StyledText
        wrapMode: Text.WordWrap
        horizontalAlignment: Text.AlignHCenter
        typescale: MD.Token.typescale.body_medium
        color: MD.Token.color.on_surface
    }

    RowLayout {
        Layout.alignment: Qt.AlignHCenter
        spacing: 8
        visible: root.rexonHint.length === 0

        MD.Button {
            text: qsTr("GitHub")
            mdState.type: MD.Enum.BtFilledTonal
            onClicked: MD.Util.openUrlExternally(root.githubUrl)
            MD.ToolTip.visible: mdState.interactions.hovered
            MD.ToolTip.text: root.githubUrl
        }
        MD.Button {
            text: qsTr("KDE Store")
            mdState.type: MD.Enum.BtFilledTonal
            onClicked: MD.Util.openUrlExternally(root.kdeStoreUrl)
            MD.ToolTip.visible: mdState.interactions.hovered
            MD.ToolTip.text: root.kdeStoreUrl
        }
    }

    MD.Text {
        Layout.fillWidth: true
        visible: root.rexonHint.length === 0
        text: qsTr("Then right-click the desktop → <b>Configure Desktop and Wallpaper…</b> and pick the <b>Waywallen</b> wallpaper plugin.")
        textFormat: Text.StyledText
        wrapMode: Text.WordWrap
        horizontalAlignment: Text.AlignHCenter
        typescale: MD.Token.typescale.body_small
        color: MD.Token.color.on_surface_variant
    }
}
