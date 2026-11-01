// Блок «Светлое и тёмное» над сетками «Глобальных тем» (Plasma) и
// «Цветовых схем»: «Светлое», «Тёмное» или «Автоматически» — по времени либо
// от заката до рассвета, — и что переключать (пары тем, обои, режим
// программ).
//
// Только разметка: снимок, подписи, проверка, запись и применение —
// `DayNightActions` (rust/rexon-gui/src/day_night_actions.rs →
// rexon_core::day_night).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section

    property var pal: ({})
    property var sty: ({})

    property var st: ({})
    // Рабочая копия настройки: правки копятся здесь до «Сохранить».
    property var cfg: ({})
    property string message: ""
    property bool failed: false
    // Раскрыт ли блок — из снимка ядра; щелчок по шапке меняет и запоминает.
    property bool expanded: false
    // Часть, для которой открыт выбор файла, и сторона (light/dark).
    property string pickPart: ""
    property string pickSide: ""

    function reload() {
        try {
            section.st = JSON.parse(actions.state());
        } catch (e) {
            section.st = ({});
        }
        const c = JSON.parse(JSON.stringify(section.st.config || {}));
        // Подсказанные пары (по именам) — в рабочую копию.
        for (const p of (section.st.parts || [])) {
            if (c[p.id] === undefined)
                c[p.id] = ({ on: false, light: "", dark: "" });
            if ((c[p.id].light || "") === "" && (c[p.id].dark || "") === "") {
                c[p.id].light = p.light;
                c[p.id].dark = p.dark;
            }
        }
        section.cfg = c;
        section.expanded = section.st.expanded === true;
    }

    function toggle() {
        section.expanded = !section.expanded;
        actions.setExpanded(section.expanded);
    }

    function edit(change) {
        const c = JSON.parse(JSON.stringify(section.cfg));
        change(c);
        section.cfg = c;
    }

    function save() {
        let r = ({});
        try {
            r = JSON.parse(actions.save(JSON.stringify(section.cfg)));
        } catch (e) {
            r = ({});
        }
        section.failed = r.ok !== true;
        section.message = r.message || "";
        if (r.ok === true)
            section.reload();
    }

    DayNightActions {
        id: actions
        objectName: "dayNightActions"
    }

    FileDialog {
        id: wallpaperDialog
        fileMode: FileDialog.OpenFile
        nameFilters: ["*.png *.jpg *.jpeg *.webp *.bmp *.svg"]
        onAccepted: {
            const url = selectedFile.toString();
            const path = decodeURIComponent(url.replace(/^file:\/\//, ""));
            if (path !== "")
                section.edit(c => { c[section.pickPart][section.pickSide] = path; });
        }
    }

    Component.onCompleted: section.reload()
    // Перечитывать — после обработчика, а не в нём: `visible` у раздела на
    // странице сам зависит от `st`, и новый `st` посреди смены `visible`
    // давал «Binding loop detected for property "visible"».
    onVisibleChanged: if (section.visible) Qt.callLater(section.reload)

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

        // Шапка: свёрнутым блок — одна строка с тем, что выбрано сейчас, и
        // стрелкой; щелчок (или Enter/пробел) раскрывает. Раскрыт ли —
        // помнит ядро (`settings.json`), по умолчанию свёрнут, чтобы сетка
        // тем не уезжала вниз.
        Item {
            id: header
            objectName: "dayNightHeader"
            Layout.fillWidth: true
            implicitHeight: headerRow.implicitHeight
            activeFocusOnTab: true
            Accessible.role: Accessible.Button
            Accessible.name: headerText.text

            Keys.onReturnPressed: section.toggle()
            Keys.onEnterPressed: section.toggle()
            Keys.onSpacePressed: section.toggle()

            RowLayout {
                id: headerRow
                width: parent.width
                spacing: 8

                Text {
                    text: section.expanded ? "▾" : "▸"
                    color: section.pal.text_primary
                    font.pointSize: 13
                }

                Text {
                    id: headerText
                    Layout.fillWidth: true
                    text: section.expanded ? (section.st.title || "") : (section.st.summary || section.st.title || "")
                    color: section.pal.text_primary
                    font.bold: true
                    font.pointSize: 13
                    font.underline: header.activeFocus
                    wrapMode: Text.WordWrap
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: section.toggle()
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: section.expanded
            spacing: section.sty.group.spacing


            // Светлое / Тёмное / Автоматически.
            Flow {
                Layout.fillWidth: true
                spacing: 8

                Repeater {
                    model: section.st.modes || []

                    delegate: RexonButton {
                        id: modeButton
                        required property var modelData
                        objectName: "dayNightMode_" + modeButton.modelData.id
                        pal: section.pal
                        checkable: true
                        checked: section.cfg.mode === modeButton.modelData.id
                        font.bold: checked
                        text: modeButton.modelData.label
                        onClicked: {
                            const id = modeButton.modelData.id;
                            section.edit(c => { c.mode = id; });
                            // «Светлое» и «Тёмное» — сразу; у «Автоматически»
                            // сначала расписание.
                            if (id !== "auto")
                                section.save();
                        }
                    }
                }
            }

            // ── Расписание ───────────────────────────────────────────────────
            ColumnLayout {
                Layout.fillWidth: true
                visible: section.cfg.mode === "auto"
                spacing: 8

                Flow {
                    Layout.fillWidth: true
                    spacing: 8

                    Repeater {
                        model: section.st.schedules || []

                        delegate: RexonButton {
                            id: scheduleButton
                            required property var modelData
                            objectName: "dayNightSchedule_" + scheduleButton.modelData.id
                            pal: section.pal
                            checkable: true
                            checked: section.cfg.schedule === scheduleButton.modelData.id
                            font.bold: checked
                            text: scheduleButton.modelData.label
                            onClicked: section.edit(c => { c.schedule = scheduleButton.modelData.id; })
                        }
                    }
                }

                RowLayout {
                    visible: section.cfg.schedule === "times"
                    spacing: 8

                    Text {
                        text: section.st.light_from_label || ""
                        color: section.pal.text_primary
                    }
                    RexonTextField {
                        id: lightAt
                        objectName: "dayNightLightAt"
                        pal: section.pal
                        Layout.preferredWidth: 80
                        text: section.cfg.light_at || ""
                        inputMask: "99:99"
                        onEditingFinished: section.edit(c => { c.light_at = lightAt.text; })
                    }
                    Text {
                        text: section.st.dark_from_label || ""
                        color: section.pal.text_primary
                    }
                    RexonTextField {
                        id: darkAt
                        objectName: "dayNightDarkAt"
                        pal: section.pal
                        Layout.preferredWidth: 80
                        text: section.cfg.dark_at || ""
                        inputMask: "99:99"
                        onEditingFinished: section.edit(c => { c.dark_at = darkAt.text; })
                    }
                }

                ColumnLayout {
                    visible: section.cfg.schedule === "sun"
                    spacing: 6

                    RexonCheckBox {
                        id: autoLocation
                        objectName: "dayNightAutoLocation"
                        pal: section.pal
                        text: section.st.auto_location_label || ""
                        checked: section.cfg.auto_location === true
                        onToggled: section.edit(c => { c.auto_location = autoLocation.checked; })
                    }

                    Text {
                        visible: section.cfg.auto_location === true && (section.st.place || "") !== ""
                        text: section.st.place || ""
                        color: section.pal.text_secondary
                    }

                    RowLayout {
                        visible: section.cfg.auto_location !== true
                        spacing: 8

                        Text {
                            text: section.st.lat_label || ""
                            color: section.pal.text_primary
                        }
                        RexonTextField {
                            id: latField
                            objectName: "dayNightLat"
                            pal: section.pal
                            Layout.preferredWidth: 90
                            text: section.cfg.lat === null || section.cfg.lat === undefined ? "" : String(section.cfg.lat)
                            onEditingFinished: section.edit(c => {
                                const v = parseFloat(latField.text.replace(",", "."));
                                c.lat = isNaN(v) ? null : v;
                            })
                        }
                        Text {
                            text: section.st.lon_label || ""
                            color: section.pal.text_primary
                        }
                        RexonTextField {
                            id: lonField
                            objectName: "dayNightLon"
                            pal: section.pal
                            Layout.preferredWidth: 90
                            text: section.cfg.lon === null || section.cfg.lon === undefined ? "" : String(section.cfg.lon)
                            onEditingFinished: section.edit(c => {
                                const v = parseFloat(lonField.text.replace(",", "."));
                                c.lon = isNaN(v) ? null : v;
                            })
                        }
                    }
                }
            }

            // ── Что переключать ──────────────────────────────────────────────
            Repeater {
                model: section.st.parts || []

                delegate: RowLayout {
                    id: partRow
                    required property var modelData
                    readonly property var pair: section.cfg[partRow.modelData.id] || ({})
                    Layout.fillWidth: true
                    spacing: 8

                    RexonCheckBox {
                        id: partOn
                        objectName: "dayNightPart_" + partRow.modelData.id
                        Layout.preferredWidth: 200
                        pal: section.pal
                        text: partRow.modelData.label
                        checked: partRow.pair.on === true
                        onToggled: section.edit(c => { c[partRow.modelData.id].on = partOn.checked; })
                    }

                    Repeater {
                        model: ["light", "dark"]

                        delegate: RowLayout {
                            id: side
                            required property string modelData
                            Layout.fillWidth: true
                            enabled: partRow.pair.on === true
                            spacing: 4

                            Text {
                                text: side.modelData === "light" ? (section.st.light_label || "") : (section.st.dark_label || "")
                                color: section.pal.text_secondary
                            }

                            RexonComboBox {
                                id: pick
                                objectName: "dayNight_" + partRow.modelData.id + "_" + side.modelData
                                visible: partRow.modelData.file !== true
                                Layout.fillWidth: true
                                pal: section.pal
                                textRole: "label"
                                valueRole: "id"
                                model: partRow.modelData.items || []
                                selectedValue: partRow.pair[side.modelData] || ""
                                onActivated: section.edit(c => { c[partRow.modelData.id][side.modelData] = pick.currentValue; })
                            }

                            RexonButton {
                                objectName: "dayNightPick_" + partRow.modelData.id + "_" + side.modelData
                                visible: partRow.modelData.file === true
                                Layout.fillWidth: true
                                pal: section.pal
                                text: {
                                    const path = partRow.pair[side.modelData] || "";
                                    return path !== "" ? path.substring(path.lastIndexOf("/") + 1) : (section.st.pick_label || "");
                                }
                                onClicked: {
                                    section.pickPart = partRow.modelData.id;
                                    section.pickSide = side.modelData;
                                    wallpaperDialog.open();
                                }
                            }
                        }
                    }
                }
            }

            RexonCheckBox {
                id: portalOn
                objectName: "dayNightPortal"
                pal: section.pal
                text: section.st.portal_label || ""
                checked: section.cfg.portal === true
                onToggled: section.edit(c => { c.portal = portalOn.checked; })
            }

            Text {
                Layout.fillWidth: true
                visible: section.cfg.mode === "auto" && (section.st.native_note || "") !== ""
                text: section.st.native_note || ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: (section.st.status || "") !== ""
                text: section.st.status || ""
                color: section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            Text {
                Layout.fillWidth: true
                visible: section.message !== ""
                text: section.message
                color: section.failed && section.pal.state_warn !== undefined ? section.pal.state_warn
                                                                               : section.pal.text_secondary
                wrapMode: Text.WordWrap
            }

            RexonButton {
                objectName: "dayNightSave"
                pal: section.pal
                text: section.st.save_label || ""
                onClicked: section.save()
            }
        }
    }
}
