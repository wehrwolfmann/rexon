// The "Deepin windows" section of the "Settings" tab. Rexon supports
// Deepin only on Treeland, and there window corners, transparency and
// accent are set only by the Deepin Control Center — so the section says
// where to change them (in other Deepin sessions it says that Rexon
// supports Deepin only on Treeland). Shown only in a Deepin session.
//
// Markup only: snapshot — `DdeLookActions`
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
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        height: implicitHeight
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Deepin windows")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            objectName: "ddeLookNote"
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }
    }
}
