// Раздел «Значки и панели инструментов» вкладки «Оформление»: значки в меню и
// на кнопках, подписи на кнопках панелей инструментов — во всех окружениях
// (как «Настроить значки и панели инструментов» стиля приложений KDE).
//
// Только разметка: снимок и запись — `AppWidgetsActions`
// (rust/rexon-gui/src/app_widgets_actions.rs → rexon_core::app_widgets).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    signal message(string text)

    property var st: ({})

    readonly property real labelWidth: Math.max(section.sty.widths.fieldLabel, 300)

    function reload() {
        section.st = JSON.parse(actions.state());
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    AppWidgetsActions {
        id: actions
        objectName: "appWidgetsActions"
    }

    Component.onCompleted: section.reload()

    visible: section.st.available === true
    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: visible ? body.implicitHeight + 2 * (section.sty.group.marginV + border.width) : 0

    ColumnLayout {
        id: body
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Icons and toolbars in applications")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            visible: (section.st.note || "") !== ""
            text: section.st.note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Flow {
            Layout.fillWidth: true
            spacing: 12

            RexonCheckBox {
                id: menuIcons
                objectName: "appWidgetsMenuIcons"
                pal: section.pal
                text: qsTranslate("rexon", "Show icons in menus")
                checked: section.st.menu_icons === true
                onToggled: section.did(actions.setMenuIcons(menuIcons.checked))
            }

            RexonCheckBox {
                id: buttonIcons
                objectName: "appWidgetsButtonIcons"
                pal: section.pal
                text: qsTranslate("rexon", "Show icons on buttons")
                checked: section.st.button_icons === true
                onToggled: section.did(actions.setButtonIcons(buttonIcons.checked))
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                // Главная панель — различие KDE; вне Plasma первый выбор
                // действует на все панели остальных программ.
                text: section.st.de === "kde"
                      ? qsTranslate("rexon", "Main toolbar labels:")
                      : qsTranslate("rexon", "Toolbar labels:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: toolbarCombo
                objectName: "appWidgetsToolbar"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.toolbar_options || []
                selectedValue: section.st.toolbar || ""
                onActivated: section.did(actions.setToolbar(toolbarCombo.currentValue))
            }
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.has_other_toolbars === true
            spacing: 8

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Secondary toolbar labels:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: otherCombo
                objectName: "appWidgetsOtherToolbars"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.toolbar_options || []
                selectedValue: section.st.other_toolbars || ""
                onActivated: section.did(actions.setOtherToolbars(otherCombo.currentValue))
            }
        }

        // Вне Plasma прочие панели различают только программы KDE.
        Text {
            Layout.fillWidth: true
            visible: section.st.has_other_toolbars === true && (section.st.other_toolbars_note || "") !== ""
            text: section.st.other_toolbars_note || ""
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonButton {
            objectName: "appWidgetsDefaults"
            pal: section.pal
            text: qsTranslate("rexon", "Defaults")
            onClicked: section.did(actions.defaults())
        }
    }
}
