// Раздел «Отклик при запуске» вкладки «Оформление»: как курсор и панель
// задач показывают, что программа запускается, и когда анимация гаснет —
// как окно «Отклик при запуске» модуля «Курсоры» KDE. Вне KDE такой
// настройки нет — раздел говорит это прямо.
//
// Только разметка: снимок и запись — `LaunchFeedbackActions`
// (rust/rexon-gui/src/launch_feedback_actions.rs → rexon_core::launch_feedback).

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

    LaunchFeedbackActions {
        id: actions
        objectName: "launchFeedbackActions"
    }

    Component.onCompleted: section.reload()

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    ColumnLayout {
        id: body
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Launch feedback")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            text: section.st.supported === true
                  ? qsTranslate("rexon", "How the pointer and the task bar show that an application is starting.")
                  : (section.st.note || "")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.supported === true
            spacing: 8

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Pointer:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: cursorCombo
                objectName: "launchFeedbackCursor"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.cursor_options || []
                selectedValue: section.st.cursor || ""
                onActivated: section.did(actions.setCursor(cursorCombo.currentValue))
            }
        }

        RexonCheckBox {
            objectName: "launchFeedbackTaskbar"
            visible: section.st.supported === true
            pal: section.pal
            text: qsTranslate("rexon", "Animate the task bar button")
            checked: section.st.taskbar === true
            onToggled: section.did(actions.setTaskbar(checked))
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.supported === true
            spacing: 8

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Stop the animation after:")
                color: section.pal.text_primary
            }

            RexonComboBox {
                id: timeoutCombo
                objectName: "launchFeedbackTimeout"
                Layout.fillWidth: true
                enabled: section.st.timeout_enabled === true
                pal: section.pal
                textRole: "label"
                valueRole: "id"
                model: section.st.timeout_options || []
                selectedValue: section.st.timeout || ""
                onActivated: section.did(actions.setTimeout(parseInt(timeoutCombo.currentValue)))
            }
        }

        RexonButton {
            objectName: "launchFeedbackDefaults"
            visible: section.st.supported === true
            pal: section.pal
            text: qsTranslate("rexon", "Defaults")
            onClicked: section.did(actions.defaults())
        }
    }
}
