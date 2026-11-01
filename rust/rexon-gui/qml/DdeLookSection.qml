// Раздел «Окна Deepin» вкладки «Настройки». Rexon поддерживает Deepin
// только на Treeland, а там углы, прозрачность и акцент окон задаёт только
// Центр управления Deepin, — поэтому раздел говорит, где их менять (в
// других сеансах Deepin — что Rexon поддерживает Deepin только на
// Treeland). Показывается только в сеансе Deepin.
//
// Только разметка: снимок — `DdeLookActions`
// (rust/rexon-gui/src/dde_look_actions.rs → rexon_core::dde).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    property var st: ({})

    DdeLookActions {
        id: actions
        objectName: "ddeLookActions"
    }

    Component.onCompleted: section.st = JSON.parse(actions.state())

    visible: section.st.available === true
    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: section.visible ? body.implicitHeight + 2 * (section.sty.group.marginV + border.width) : 0

    ColumnLayout {
        id: body
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Deepin windows")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            objectName: "ddeLookNote"
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }
    }
}
