// Раздел «Анимация» вкладки «Оформление»: скорость, «без анимаций» и эффекты
// по событиям — как модуль «Анимация» KDE, но во всех окружениях.
//
// Только разметка: снимок и запись — `AnimationsActions`
// (rust/rexon-gui/src/animations_actions.rs → rexon_core::animations). Что
// показывать, решает ядро: нет ползунка скорости — его нет в снимке, нет
// эффектов окружения — нет строк, а пояснение `note` честно говорит, куда
// настройка не доходит.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    /// Палитра скина и раскладка вкладки — от хозяина страницы.
    property var pal: ({})
    property var sty: ({})

    /// Итог действия для всплывающей плашки хозяина.
    signal message(string text)

    property var st: ({})

    /// Ширина подписей слева: одна на раздел, чтобы списки стояли ровно.
    readonly property real labelWidth: Math.max(section.sty.widths.fieldLabel, 300)

    function reload() {
        section.st = JSON.parse(actions.state());
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    AnimationsActions {
        id: actions
        objectName: "animationsActions"
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
            text: qsTranslate("rexon", "Animation")
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

        RexonCheckBox {
            id: offBox
            objectName: "animationsOff"
            pal: section.pal
            text: qsTranslate("rexon", "No animations")
            checked: section.st.enabled === false
            onToggled: section.did(actions.setEnabled(!offBox.checked))
        }

        RowLayout {
            Layout.fillWidth: true
            visible: section.st.has_speed === true
            spacing: 8

            Text {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Animation speed:")
                color: section.pal.text_primary
            }

            Text {
                text: qsTranslate("rexon", "Slow")
                color: section.pal.text_secondary
            }

            RexonSlider {
                id: speed
                objectName: "animationsSpeed"
                Layout.fillWidth: true
                pal: section.pal
                enabled: section.st.enabled === true
                from: 0
                to: (section.st.speed_steps || [1]).length - 1
                stepSize: 1
                tickStep: 1
                snapMode: Slider.SnapAlways
                value: section.st.speed_index !== undefined ? section.st.speed_index : 3
                onMoved: section.did(actions.setSpeedIndex(Math.round(speed.value)))
            }

            Text {
                text: qsTranslate("rexon", "Fast")
                color: section.pal.text_secondary
            }
        }

        Text {
            Layout.fillWidth: true
            visible: (section.st.rows || []).length > 0
            text: qsTranslate("rexon", "Effects:")
            color: section.pal.text_primary
            font.bold: true
        }

        Repeater {
            model: section.st.rows || []

            delegate: RowLayout {
                id: row
                required property var modelData
                Layout.fillWidth: true
                spacing: 8
                enabled: section.st.enabled === true

                Text {
                    Layout.preferredWidth: section.labelWidth
                    text: row.modelData.label + ":"
                    color: section.pal.text_primary
                    wrapMode: Text.WordWrap
                }

                RexonComboBox {
                    id: combo
                    visible: row.modelData.kind === "choice"
                    Layout.fillWidth: true
                    pal: section.pal
                    model: row.modelData.options
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: row.modelData.value
                    onActivated: section.did(actions.setRow(row.modelData.id, combo.currentValue))
                }

                RexonCheckBox {
                    id: check
                    visible: row.modelData.kind === "toggle"
                    pal: section.pal
                    checked: row.modelData.value === "true"
                    onToggled: section.did(actions.setRow(row.modelData.id,
                                                          check.checked ? "true" : "false"))
                }

                Item {
                    visible: row.modelData.kind === "toggle"
                    Layout.fillWidth: true
                }

                // «Настроить…» — страница настроек выбранного эффекта KWin
                // (признак `configurable` даёт ядро; вне KDE его нет).
                RexonButton {
                    objectName: "animationsConfigure"
                    pal: section.pal
                    readonly property string effect: row.modelData.kind === "toggle"
                                                     ? row.modelData.id : row.modelData.value
                    visible: row.modelData.kind === "toggle"
                             ? row.modelData.configurable === true
                             : (row.modelData.options || []).some(o => o.id === row.modelData.value
                                                                       && o.configurable === true)
                    text: qsTranslate("rexon", "Configure…")
                    onClicked: section.did(actions.configure(effect))
                }
            }
        }

        RexonButton {
            objectName: "animationsDefaults"
            pal: section.pal
            text: qsTranslate("rexon", "Defaults")
            onClicked: section.did(actions.defaults())
        }
    }
}
