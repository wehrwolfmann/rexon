// Раздел «Эффекты рабочего стола» вкладки «Оформление»: все эффекты KWin с
// включением, поиском, группами и «Настроить…» — как модуль KDE «Эффекты
// рабочего стола». Только в KDE (эффекты — у KWin): вне KDE раздел прячется.
//
// Только разметка: список, включение и открытие настроек — ядро
// (`AnimationsActions` → rexon_core::animations: `desktop_effects_state`,
// `set_desktop_effect`, `configure_effect`). Эффекты, которые уже есть в
// разделе «Анимация», ядро сюда не кладёт — так же делает и KDE.

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

    /// Строки после поиска (ядро даёт готовую строку `search`).
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
        x: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        width: parent.width - 2 * (section.sty.group.marginH + section.border.width)
        height: implicitHeight
        spacing: section.sty.group.spacing

        Text {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "Desktop effects")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
            wrapMode: Text.WordWrap
        }

        Text {
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

        Text {
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

                // Заголовок категории — над первым эффектом категории.
                Text {
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

                        Text {
                            Layout.fillWidth: true
                            text: item.modelData.name
                            color: section.pal.text_primary
                            wrapMode: Text.WordWrap
                        }

                        Text {
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
