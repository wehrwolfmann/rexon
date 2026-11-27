// The "Desktop effects" section of the "Appearance" tab: all KWin effects
// with toggling, search, groups and "Configure…" — like the KDE "Desktop
// Effects" module. Only in KDE (the effects belong to KWin): outside KDE
// the section is hidden.
//
// Markup only: the list, toggling and opening the settings — the core
// (`AnimationsActions` → rexon_core::animations: `desktop_effects_state`,
// `set_desktop_effect`, `configure_effect`). Effects that are already in
// the "Animation" section are not put here by the core — KDE does the same.

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

    /// Rows after search (the core provides a ready `search` string).
    readonly property var shown: {
        const q = search.text.trim().toLowerCase();
        return (section.st.effects || []).filter(e => q === "" || e.search.indexOf(q) >= 0);
    }

    function reload() {
        section.st = JSON.parse(actions.effectsState());
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    AnimationsActions {
        id: actions
        objectName: "desktopEffectsActions"
    }

    Component.onCompleted: section.reload()

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
            text: qsTranslate("rexon", "Desktop effects")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Effects of the KWin window manager. Window and desktop animations are in the Animation section.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        RexonTextField {
            id: search
            objectName: "desktopEffectsSearch"
            Layout.fillWidth: true
            pal: section.pal
            placeholderText: qsTranslate("rexon", "Search effects…")
        }

        RexonText {
            Layout.fillWidth: true
            visible: section.shown.length === 0
            text: qsTranslate("rexon", "No effects found.")
            color: section.pal.text_secondary
            wrapMode: Text.WordWrap
        }

        Repeater {
            model: section.shown

            delegate: ColumnLayout {
                id: item
                required property var modelData
                required property int index
                Layout.fillWidth: true
                spacing: 4

                // Category header — above the first effect of the category.
                RexonText {
                    Layout.fillWidth: true
                    Layout.topMargin: item.index === 0 ? 0 : 8
                    visible: item.index === 0
                             || section.shown[item.index - 1].category !== item.modelData.category
                    text: item.modelData.category
                    color: section.pal.text_primary
                    font.bold: true
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonCheckBox {
                        id: check
                        objectName: "desktopEffect_" + item.modelData.id
                        pal: section.pal
                        checked: item.modelData.enabled === true
                        onToggled: section.did(actions.setEffect(item.modelData.id, check.checked))
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        RexonText {
                            Layout.fillWidth: true
                            text: item.modelData.name
                            color: section.pal.text_primary
                            wrapMode: Text.WordWrap
                        }

                        RexonText {
                            Layout.fillWidth: true
                            visible: item.modelData.description !== ""
                            text: item.modelData.description
                            color: section.pal.text_secondary
                            wrapMode: Text.WordWrap
                        }
                    }

                    RexonButton {
                        visible: item.modelData.configurable === true
                        pal: section.pal
                        text: qsTranslate("rexon", "Configure…")
                        onClicked: section.did(actions.configure(item.modelData.id))
                    }
                }
            }
        }

        RexonButton {
            objectName: "desktopEffectsDefaults"
            pal: section.pal
            text: qsTranslate("rexon", "Defaults")
            onClicked: section.did(actions.effectsDefaults())
        }
    }
}
