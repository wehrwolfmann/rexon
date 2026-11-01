pragma ComponentBehavior: Bound
import QtQuick
import Qcm.Material as MD

// Compact pill-shaped label chip. Defaults to the primary container color;
// callers override bgColor/fgColor (e.g. a per-vendor MdColorMgr scheme).
Rectangle {
    id: root

    property alias text: tagText.text
    property alias textItem: tagText
    property color bgColor: MD.Token.color.primary_container
    property color fgColor: MD.Token.color.on_primary_container

    implicitWidth: tagText.implicitWidth + 16
    implicitHeight: tagText.implicitHeight + 6
    radius: height / 2
    color: root.bgColor

    MD.Text {
        id: tagText
        anchors.centerIn: parent
        typescale: MD.Token.typescale.label_small
        color: root.fgColor
    }
}
