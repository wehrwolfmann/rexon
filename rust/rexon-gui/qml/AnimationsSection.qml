// The "Animation" section of the "Appearance" tab: speed, "no animations"
// and per-event effects — like the KDE "Animation" module, but in all
// environments.
//
// Markup only: snapshot and writing — `AnimationsActions`
// (rust/rexon-gui/src/animations_actions.rs → rexon_core::animations). What
// to show is decided by the core: no speed slider — it is not in the
// snapshot; no environment effects — no rows; and the `note` explanation
// honestly says where the setting does not reach.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    /// Skin palette and tab layout — from the page host.
    property var pal: ({})
    property var sty: ({})

    /// Action result for the host's pop-up banner.
    signal message(string text)

    property var st: ({})

    /// Width of the labels on the left: one per section, so the lists line up.
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
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        height: implicitHeight
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Animation")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
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

            RexonText {
                Layout.preferredWidth: section.labelWidth
                wrapMode: Text.WordWrap
                text: qsTranslate("rexon", "Animation speed:")
                color: section.pal.text_primary
            }

            RexonText {
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

            RexonText {
                text: qsTranslate("rexon", "Fast")
                color: section.pal.text_secondary
            }
        }

        RexonText {
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

                RexonText {
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

                // "Configure…" — the settings page of the selected KWin effect (the
                // `configurable` flag comes from the core; outside KDE it is absent).
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
