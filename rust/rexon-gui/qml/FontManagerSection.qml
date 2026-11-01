// «Управление шрифтами» в разделе шрифтов вкладки «Настройки»: все
// установленные шрифты по семействам, группы «Все / Личные / Системные /
// Неклассифицированные» и свои группы (общие с KDE: создать, переименовать,
// удалить, добавить шрифт кнопкой или перетаскиванием, включить и выключить
// группу целиком, экспорт в ZIP), производитель, тип файла и письменность, образец каждого начертания с
// выбором кегля и своим текстом, таблица символов по блокам Юникода,
// поиск, установка шрифта из
// файла для себя или для всех пользователей, удаление своих и поставленных
// для всех (с паролем), включение и выключение без удаления, поиск
// дубликатов. Шрифты пакетов не удаляются.
//
// Только разметка: список, фильтры, символы, текст образца, установка,
// удаление, выключение, группы и дубликаты — `SettingsActions`
// (`fontLibrary`, `fontChars`, `setFontSampleText`, `installFontFile`,
// `removeFont`, `setFontEnabled`, `fontGroupEdit`, `setFontGroupEnabled`,
// `exportFontGroup`, `fontDuplicates` в
// rust/rexon-gui/src/settings_actions.rs), решения и подписи — ядро
// (`rexon_core::qt_settings`, раздел «Управление шрифтами»).

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: section

    property var actions: null
    property var pal: ({})
    property var sty: ({})

    /// Ответ действия — показать человеку.
    signal message(string text)
    /// Элемент получил фокус — странице прокрутить к нему.
    signal focused(Item item)

    property bool expanded: false
    property var lib: ({})
    property string query: ""
    property string origin: "all"
    /// Фильтры производителя, типа файла и письменности; пусто — любой.
    property string foundry: ""
    property string fileType: ""
    property string script: ""
    /// Для кого ставить шрифт (`lib.scopes`): «user» — для себя, «all» —
    /// для всех пользователей (с паролем).
    property string installScope: "user"
    property string selected: ""
    /// Кегль образца (пункты); пределы — из ядра (`lib.sample`).
    property int sampleSize: 16
    /// Показан список дубликатов вместо списка шрифтов.
    property bool showDuplicates: false
    property var dups: ({})
    /// Правая часть: «sample» — образцы начертаний, «chars» — таблица
    /// символов.
    property string view: "sample"
    /// Начертание, чьи символы показаны (номер в `current.styles`).
    property int charsStyle: 0
    /// Таблица символов (`fontChars`): блоки с кодами символов.
    property var chars: ({})
    property string charsBlock: ""
    property int pickedChar: -1
    /// Правка имени группы: «create» — новая, «rename» — выбранная; пусто —
    /// поле имени скрыто.
    property string naming: ""
    /// Семейство, которое сейчас перетаскивают на группу.
    property string dragFamily: ""
    /// Выбранная своя группа (фильтр `group:Имя`) или пусто.
    readonly property string customGroup: section.origin.startsWith("group:") ? section.origin.substring(6) : ""

    readonly property var texts: section.lib.texts || ({})
    /// Текст образцов: свой из настроек или по умолчанию (ядро).
    readonly property string sampleText: section.lib.sample ? section.lib.sample.text : ""
    readonly property color colText: section.pal.text_primary || "#ffffff"
    readonly property color colTextSecondary: section.pal.text_secondary || "#8f98a0"
    readonly property color colBorder: section.pal.border || "#3d4450"

    /// Семейства под фильтр и строку поиска — отбирает ядро.
    readonly property var shown: section.lib.families || []

    /// Выбранное семейство целиком (или null).
    readonly property var current: {
        const all = section.lib.families || [];
        for (let i = 0; i < all.length; ++i) {
            if (all[i].family === section.selected)
                return all[i];
        }
        return null;
    }

    function reload() {
        if (!section.actions)
            return;
        try {
            section.lib = JSON.parse(section.actions.fontLibrary(section.expanded, JSON.stringify({
                "origin": section.origin,
                "query": section.query,
                "foundry": section.foundry,
                "type": section.fileType,
                "script": section.script
            })));
        } catch (e) {
            section.lib = ({});
        }
        if (section.showDuplicates)
            section.reloadDuplicates();
    }

    /// Начертание, чьи символы показаны (или null).
    readonly property var charsFace: {
        const styles = section.current ? section.current.styles : [];
        if (styles.length === 0)
            return null;
        return styles[Math.min(section.charsStyle, styles.length - 1)];
    }

    /// Символы выбранного блока.
    readonly property var blockChars: {
        const blocks = section.chars.blocks || [];
        for (let i = 0; i < blocks.length; ++i) {
            if (blocks[i].id === section.charsBlock)
                return blocks[i].chars;
        }
        return blocks.length > 0 ? blocks[0].chars : [];
    }

    function reloadChars() {
        section.pickedChar = -1;
        if (!section.actions || section.view !== "chars" || section.charsFace === null) {
            section.chars = ({});
            return;
        }
        try {
            section.chars = JSON.parse(section.actions.fontChars(section.charsFace.file, section.charsFace.style));
        } catch (e) {
            section.chars = ({});
        }
        // Прежний блок, если он есть и у этого шрифта; иначе — первый.
        const blocks = section.chars.blocks || [];
        if (!blocks.some(b => b.id === section.charsBlock))
            section.charsBlock = blocks.length > 0 ? blocks[0].id : "";
    }

    onSelectedChanged: {
        section.charsStyle = 0;
        section.reloadChars();
    }
    onCharsStyleChanged: section.reloadChars()
    onViewChanged: section.reloadChars()

    function reloadDuplicates() {
        try {
            section.dups = JSON.parse(section.actions.fontDuplicates());
        } catch (e) {
            section.dups = ({});
        }
    }

    /// Правка своих групп (`fontGroupEdit`): ядро говорит, что выбрать
    /// после неё; удачное создание и переименование закрывают поле имени.
    function groupEdit(op, group, arg) {
        if (!section.actions)
            return;
        let r = ({});
        try {
            r = JSON.parse(section.actions.fontGroupEdit(op, group, arg));
        } catch (e) {
            r = ({});
        }
        if (r.origin) {
            section.origin = r.origin;
            if (op === "create" || op === "rename")
                section.naming = "";
        }
        if (r.text)
            section.message(r.text);
        section.reload();
    }

    function did(text) {
        if (text)
            section.message(text);
        section.reload();
    }

    // Установка и удаление «для всех» кончаются в фоне (окно пароля):
    // список перечитывается, когда пришёл итог.
    Connections {
        target: section.actions
        function onRefreshed() {
            if (section.expanded)
                section.reload();
        }
    }

    // Поиск спрашивает ядро не на каждую букву, а когда набор затих.
    Timer {
        id: searchDelay
        interval: 200
        onTriggered: section.reload()
    }

    spacing: section.sty.group ? section.sty.group.spacing : 8

    // До первого открытия читаются только подписи: список шрифтов (fc-list)
    // спрашивается, когда человек раскрыл раздел.
    Component.onCompleted: {
        section.reload();
        if (section.lib.sample)
            section.sampleSize = section.lib.sample.start;
    }

    RowLayout {
        Layout.fillWidth: true
        spacing: 8

        RexonButton {
            id: toggleButton
            pal: section.pal
            text: section.expanded ? (section.texts.close || "") : (section.texts.open || "")
            ToolTip.visible: hovered && !section.expanded
            ToolTip.text: section.texts.open_tip || ""
            onClicked: {
                section.expanded = !section.expanded;
                if (section.expanded) {
                    section.reload();
                    // Список раскрывается ниже кнопки и без этого оставался
                    // за краем окна: человек нажал и ничего не увидел.
                    Qt.callLater(function () { section.focused(panes); });
                }
            }
            onActiveFocusChanged: if (activeFocus) section.focused(toggleButton)
        }

        RexonButton {
            id: installButton
            visible: section.expanded
            pal: section.pal
            tone: section.sty.primary
            enabled: section.lib.available === true
            text: section.texts.install || ""
            ToolTip.visible: hovered
            ToolTip.text: section.installScope === "all" ? (section.texts.install_all_tip || "")
                                                         : (section.texts.install_tip || "")
            onClicked: section.did(section.actions.installFontFile(section.installScope))
            onActiveFocusChanged: if (activeFocus) section.focused(installButton)
        }

        RexonComboBox {
            id: scopeCombo
            // Один вариант (NixOS, SteamOS: только «для себя») — выбора нет.
            visible: section.expanded && (section.lib.scopes || []).length > 1
            enabled: section.lib.available === true
            pal: section.pal
            model: section.lib.scopes || []
            textRole: "label"
            valueRole: "id"
            selectedValue: section.installScope
            ToolTip.visible: hovered
            ToolTip.text: section.texts.scope_tip || ""
            onActivated: section.installScope = scopeCombo.currentValue
            onActiveFocusChanged: if (activeFocus) section.focused(scopeCombo)
        }

        RexonButton {
            id: duplicatesButton
            visible: section.expanded
            pal: section.pal
            enabled: section.lib.available === true
            text: section.showDuplicates ? (section.texts.duplicates_close || "")
                                         : (section.texts.duplicates || "")
            ToolTip.visible: hovered && !section.showDuplicates
            ToolTip.text: section.texts.duplicates_tip || ""
            onClicked: {
                section.showDuplicates = !section.showDuplicates;
                if (section.showDuplicates)
                    section.reloadDuplicates();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(duplicatesButton)
        }

        Item { Layout.fillWidth: true }
    }

    Text {
        Layout.fillWidth: true
        visible: section.expanded && section.lib.available !== true
        text: section.texts.no_fclist || ""
        wrapMode: Text.WordWrap
        color: section.colTextSecondary
    }

    // Группы — как список групп KDE: встроенные, затем свои. На свою
    // группу можно бросить семейство из списка ниже.
    Flow {
        id: groupChips
        Layout.fillWidth: true
        Layout.preferredWidth: 0
        visible: section.expanded && section.lib.available === true && !section.showDuplicates
        spacing: 6

        Repeater {
            model: section.lib.origins || []

            delegate: RexonButton {
                id: chip
                required property var modelData
                pal: section.pal
                checkable: true
                checked: section.origin === chip.modelData.id
                checkedTone: section.sty.primary
                text: chip.modelData.custom === true ? "🗂️ " + chip.modelData.label : chip.modelData.label
                ToolTip.visible: hovered
                ToolTip.text: section.texts.groups_tip || ""
                onClicked: {
                    chip.checked = Qt.binding(function () { return section.origin === chip.modelData.id; });
                    section.origin = chip.modelData.id;
                    section.naming = "";
                    section.reload();
                }
                onActiveFocusChanged: if (activeFocus) section.focused(chip)

                // Подсветка, пока над своей группой держат семейство.
                Rectangle {
                    anchors.fill: parent
                    visible: chipDrop.containsDrag
                    color: "transparent"
                    border.color: section.pal.accent || "#66c0f4"
                    border.width: 2
                    radius: 4
                }

                DropArea {
                    id: chipDrop
                    anchors.fill: parent
                    enabled: chip.modelData.custom === true
                    keys: ["rexon-font-family"]
                    onDropped: section.groupEdit("add", chip.modelData.label, section.dragFamily)
                }
            }
        }

        RexonButton {
            id: newGroup
            pal: section.pal
            text: section.texts.group_new || ""
            ToolTip.visible: hovered
            ToolTip.text: section.texts.group_new_tip || ""
            onClicked: {
                section.naming = "create";
                groupName.text = "";
                groupName.forceActiveFocus();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(newGroup)
        }
    }

    // Действия с выбранной своей группой.
    Flow {
        Layout.fillWidth: true
        Layout.preferredWidth: 0
        visible: section.expanded && section.lib.available === true && !section.showDuplicates
                 && section.customGroup !== "" && section.naming === ""
        spacing: 6

        RexonButton {
            id: renameGroup
            pal: section.pal
            text: section.texts.group_rename || ""
            onClicked: {
                section.naming = "rename";
                groupName.text = section.customGroup;
                groupName.forceActiveFocus();
                groupName.selectAll();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(renameGroup)
        }

        RexonButton {
            id: enableGroup
            pal: section.pal
            text: section.texts.group_enable || ""
            ToolTip.visible: hovered
            ToolTip.text: section.texts.group_toggle_tip || ""
            onClicked: section.did(section.actions.setFontGroupEnabled(section.customGroup, true))
            onActiveFocusChanged: if (activeFocus) section.focused(enableGroup)
        }

        RexonButton {
            id: disableGroup
            pal: section.pal
            text: section.texts.group_disable || ""
            ToolTip.visible: hovered
            ToolTip.text: section.texts.group_toggle_tip || ""
            onClicked: section.did(section.actions.setFontGroupEnabled(section.customGroup, false))
            onActiveFocusChanged: if (activeFocus) section.focused(disableGroup)
        }

        RexonButton {
            id: exportGroup
            pal: section.pal
            text: section.texts.group_export || ""
            ToolTip.visible: hovered
            ToolTip.text: section.texts.group_export_tip || ""
            onClicked: section.did(section.actions.exportFontGroup(section.customGroup))
            onActiveFocusChanged: if (activeFocus) section.focused(exportGroup)
        }

        RexonDangerButton {
            id: removeGroup
            pal: section.pal
            tone: section.sty.danger
            text: section.texts.group_remove || ""
            ToolTip.visible: hovered
            ToolTip.text: section.texts.group_remove_tip || ""
            onClicked: section.groupEdit("remove", section.customGroup, "")
            onActiveFocusChanged: if (activeFocus) section.focused(removeGroup)
        }
    }

    // Имя новой группы или новое имя выбранной — в строке, без окна.
    RowLayout {
        Layout.fillWidth: true
        visible: section.expanded && section.naming !== ""
        spacing: 8

        RexonTextField {
            id: groupName
            Layout.fillWidth: true
            pal: section.pal
            maximumLength: 100
            placeholderText: section.texts.group_name || ""
            onAccepted: groupSave.clicked()
            Keys.onEscapePressed: section.naming = ""
            onActiveFocusChanged: if (activeFocus) section.focused(groupName)
        }

        RexonButton {
            id: groupSave
            pal: section.pal
            tone: section.sty.primary
            text: section.naming === "rename" ? (section.texts.group_rename_save || "") : (section.texts.group_create || "")
            onClicked: section.groupEdit(section.naming, section.customGroup, groupName.text)
            onActiveFocusChanged: if (activeFocus) section.focused(groupSave)
        }

        RexonButton {
            id: groupCancel
            pal: section.pal
            text: section.texts.group_cancel || ""
            onClicked: section.naming = ""
            onActiveFocusChanged: if (activeFocus) section.focused(groupCancel)
        }
    }

    // Фильтры переносятся на следующую строку, а не распирают страницу
    // вширь: у узкого окна правая часть списка уходила за край.
    Flow {
        Layout.fillWidth: true
        // Своя ширина Flow — ряд целиком; без нуля она распирала бы
        // страницу так же, как прежняя строка.
        Layout.preferredWidth: 0
        visible: section.expanded && section.lib.available === true && !section.showDuplicates
        spacing: 8

        // Производитель, тип файла, письменность — как у фильтра KDE;
        // складываются с местом и поиском. Варианты — только то, что
        // установлено (ядро).
        RexonComboBox {
            id: foundryCombo
            visible: (section.lib.foundries || []).length > 1
            pal: section.pal
            model: section.lib.foundries || []
            textRole: "label"
            valueRole: "id"
            selectedValue: section.foundry
            ToolTip.visible: hovered
            ToolTip.text: section.texts.filter_foundry_tip || ""
            onActivated: {
                section.foundry = foundryCombo.currentValue;
                section.reload();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(foundryCombo)
        }

        RexonComboBox {
            id: typeCombo
            visible: (section.lib.fileTypes || []).length > 1
            pal: section.pal
            model: section.lib.fileTypes || []
            textRole: "label"
            valueRole: "id"
            selectedValue: section.fileType
            ToolTip.visible: hovered
            ToolTip.text: section.texts.filter_type_tip || ""
            onActivated: {
                section.fileType = typeCombo.currentValue;
                section.reload();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(typeCombo)
        }

        RexonComboBox {
            id: scriptCombo
            visible: (section.lib.scripts || []).length > 1
            pal: section.pal
            model: section.lib.scripts || []
            textRole: "label"
            valueRole: "id"
            selectedValue: section.script
            ToolTip.visible: hovered
            ToolTip.text: section.texts.filter_script_tip || ""
            onActivated: {
                section.script = scriptCombo.currentValue;
                section.reload();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(scriptCombo)
        }

        RexonTextField {
            id: searchField
            width: 220
            pal: section.pal
            placeholderText: section.texts.search || ""
            onTextChanged: {
                section.query = text;
                searchDelay.restart();
            }
            onActiveFocusChanged: if (activeFocus) section.focused(searchField)
        }
    }

    Text {
        Layout.fillWidth: true
        visible: section.expanded && section.lib.available === true && !section.showDuplicates
        text: section.lib.summary || ""
        elide: Text.ElideRight
        color: section.colTextSecondary
    }

    RowLayout {
        id: panes
        Layout.fillWidth: true
        // Таблице символов — выше: иначе в ней видно всего три ряда.
        Layout.preferredHeight: section.view === "chars" ? 480 : 360
        visible: section.expanded && section.lib.available === true && !section.showDuplicates
        spacing: 8

        // ── Семейства ────────────────────────────────────────────────────
        Rectangle {
            Layout.preferredWidth: Math.max(220, panes.width * 0.38)
            Layout.fillHeight: true
            color: "transparent"
            border.color: section.colBorder
            border.width: 1
            radius: 4

            // То, что видно под указателем, пока семейство несут на группу.
            Rectangle {
                id: dragGhost
                z: 10
                visible: section.dragFamily !== ""
                width: ghostText.implicitWidth + 16
                height: ghostText.implicitHeight + 10
                radius: 4
                color: section.pal.bg_card_hover || "#2a475e"
                border.color: section.pal.accent || "#66c0f4"
                Drag.active: section.dragFamily !== ""
                Drag.keys: ["rexon-font-family"]
                Drag.hotSpot.x: 8
                Drag.hotSpot.y: dragGhost.height / 2

                Text {
                    id: ghostText
                    anchors.centerIn: parent
                    text: section.dragFamily
                    color: section.colText
                }
            }

            ListView {
                id: familyList
                anchors.fill: parent
                anchors.margins: 1
                clip: true
                model: section.shown
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {}
                keyNavigationEnabled: true
                onActiveFocusChanged: if (activeFocus) section.focused(familyList)

                delegate: Rectangle {
                    id: famRow
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    height: famText.implicitHeight + 10
                    color: famRow.modelData.family === section.selected
                           ? (section.pal.bg_card_hover || "#2a475e")
                           : (famMouse.containsMouse ? (section.pal.surface_button || "#32363d") : "transparent")

                    Text {
                        id: famText
                        anchors.left: parent.left
                        anchors.right: famBadge.left
                        anchors.leftMargin: 8
                        anchors.rightMargin: 4
                        anchors.verticalCenter: parent.verticalCenter
                        text: famRow.modelData.family
                        elide: Text.ElideRight
                        // Выключенное семейство видно сразу — бледным.
                        color: famRow.modelData.disabled === true ? section.colTextSecondary
                                                                  : section.colText
                        font.italic: famRow.modelData.disabled === true
                    }

                    Text {
                        id: famBadge
                        anchors.right: parent.right
                        anchors.rightMargin: 8
                        anchors.verticalCenter: parent.verticalCenter
                        // Свои — отмечены: их одних можно удалить.
                        // Выключенные — подписью «выкл.».
                        text: (famRow.modelData.disabled === true || famRow.modelData.partly === true
                               ? (section.texts.off || "") + (famRow.modelData.user ? " " : "") : "")
                              + (famRow.modelData.user ? "⭐" : "")
                        color: section.pal.accent || "#66c0f4"
                    }

                    MouseArea {
                        id: famMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        // Семейство можно унести на свою группу; список
                        // при этом не прокручивается тасканием (колесо и
                        // полоса прокрутки остаются).
                        drag.target: (section.lib.groups || []).length > 0 ? dragGhost : null
                        drag.threshold: 12
                        // Без сглаживания то, что несут, стоит ровно под
                        // указателем: иначе оно отстаёт на порог и бросок
                        // мимо кнопки группы.
                        drag.smoothed: false
                        preventStealing: true
                        onPressed: mouse => {
                            const p = famRow.mapToItem(dragGhost.parent, mouse.x, mouse.y);
                            dragGhost.x = p.x - 8;
                            dragGhost.y = p.y - dragGhost.height / 2;
                        }
                        drag.onActiveChanged: {
                            if (famMouse.drag.active)
                                section.dragFamily = famRow.modelData.family;
                        }
                        onReleased: {
                            if (section.dragFamily !== "") {
                                dragGhost.Drag.drop();
                                section.dragFamily = "";
                            }
                        }
                        onCanceled: section.dragFamily = ""
                        onClicked: {
                            section.selected = famRow.modelData.family;
                            familyList.currentIndex = famRow.index;
                        }
                    }
                }

                onCurrentIndexChanged: {
                    if (currentIndex >= 0 && currentIndex < section.shown.length)
                        section.selected = section.shown[currentIndex].family;
                }

                Text {
                    anchors.centerIn: parent
                    width: familyList.width - 24
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    visible: familyList.count === 0
                    text: (section.customGroup !== "" ? section.texts.group_empty : section.texts.empty) || ""
                    color: section.colTextSecondary
                }
            }
        }

        // ── Начертания выбранного семейства ──────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            color: "transparent"
            border.color: section.colBorder
            border.width: 1
            radius: 4

            Text {
                anchors.centerIn: parent
                width: parent.width - 24
                horizontalAlignment: Text.AlignHCenter
                visible: section.current === null
                text: section.texts.pick || ""
                wrapMode: Text.WordWrap
                color: section.colTextSecondary
            }

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 8
                visible: section.current !== null
                spacing: 6

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        Layout.fillWidth: true
                        text: section.current ? section.current.family : ""
                        font.bold: true
                        elide: Text.ElideRight
                        color: section.colText
                    }

                    RexonButton {
                        id: toggleFamily
                        visible: section.current !== null
                        pal: section.pal
                        // Частично выключенное семейство включается целиком.
                        readonly property bool turnOn: section.current !== null
                                                       && (section.current.disabled === true
                                                           || section.current.partly === true)
                        text: toggleFamily.turnOn ? (section.texts.enable || "") : (section.texts.disable || "")
                        ToolTip.visible: hovered
                        ToolTip.text: toggleFamily.turnOn ? (section.texts.enable_tip || "")
                                                          : (section.texts.disable_tip || "")
                        onClicked: section.did(section.actions.setFontEnabled(section.current.family, "",
                                                                              toggleFamily.turnOn))
                        onActiveFocusChanged: if (activeFocus) section.focused(toggleFamily)
                    }

                    // Добавить в свою группу: первая строка — подпись.
                    RexonComboBox {
                        id: addToGroup
                        readonly property var choices: {
                            const out = [{ "id": "", "label": section.texts.group_add || "" }];
                            const own = section.current ? (section.current.groups || []) : [];
                            for (const name of (section.lib.groups || [])) {
                                if (own.indexOf(name) < 0)
                                    out.push({ "id": name, "label": "🗂️ " + name });
                            }
                            return out;
                        }
                        visible: section.current !== null && addToGroup.choices.length > 1
                        pal: section.pal
                        model: addToGroup.choices
                        textRole: "label"
                        valueRole: "id"
                        selectedValue: ""
                        ToolTip.visible: hovered
                        ToolTip.text: section.texts.group_add_tip || ""
                        onActivated: {
                            const name = addToGroup.currentValue;
                            addToGroup.currentIndex = 0;
                            if (name)
                                section.groupEdit("add", name, section.current.family);
                        }
                        onActiveFocusChanged: if (activeFocus) section.focused(addToGroup)
                    }

                    RexonButton {
                        id: takeFromGroup
                        visible: section.current !== null && section.customGroup !== ""
                        pal: section.pal
                        text: section.texts.group_take || ""
                        ToolTip.visible: hovered
                        ToolTip.text: section.texts.group_take_tip || ""
                        onClicked: section.groupEdit("take", section.customGroup, section.current.family)
                        onActiveFocusChanged: if (activeFocus) section.focused(takeFromGroup)
                    }

                    RexonDangerButton {
                        id: removeFamily
                        visible: section.current !== null
                                 && (section.current.user === true || section.current.shared === true)
                        pal: section.pal
                        tone: section.sty.danger
                        text: section.texts.remove || ""
                        ToolTip.visible: hovered
                        ToolTip.text: (section.current !== null && section.current.user !== true
                                       ? section.texts.shared : section.texts.remove_tip) || ""
                        onClicked: section.did(section.actions.removeFont(section.current.family, ""))
                        onActiveFocusChanged: if (activeFocus) section.focused(removeFamily)
                    }
                }

                Text {
                    Layout.fillWidth: true
                    // Честно: шрифт пакета из окна настроек не удаляется,
                    // поставленный для всех — удаляется с паролем.
                    text: section.current === null ? ""
                          : ((section.current.packaged === true ? section.texts.system
                             : section.current.shared === true ? section.texts.shared
                             : section.texts.user) || "")
                            + (section.current.disabled === true ? " · " + (section.texts.disabled_note || "") : "")
                    wrapMode: Text.WordWrap
                    color: section.colTextSecondary
                }

                // Образцы начертаний или таблица символов — как «Тип
                // предпросмотра» KDE.
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6

                    RexonButton {
                        id: viewSample
                        pal: section.pal
                        checkable: true
                        checked: section.view === "sample"
                        checkedTone: section.sty.primary
                        text: section.texts.view_sample || ""
                        onClicked: section.view = "sample"
                        onActiveFocusChanged: if (activeFocus) section.focused(viewSample)
                    }

                    RexonButton {
                        id: viewChars
                        pal: section.pal
                        checkable: true
                        checked: section.view === "chars"
                        checkedTone: section.sty.primary
                        text: section.texts.view_chars || ""
                        onClicked: section.view = "chars"
                        onActiveFocusChanged: if (activeFocus) section.focused(viewChars)
                    }

                    Item { Layout.fillWidth: true }
                }

                // Кегль образца: 12–48 пт, как у предпросмотра KDE.
                RowLayout {
                    Layout.fillWidth: true
                    visible: section.view === "sample"
                    spacing: 8

                    Text {
                        text: (section.texts.sample_size || "%1").replace("%1", section.sampleSize)
                        color: section.colTextSecondary
                    }

                    RexonSlider {
                        id: sizeSlider
                        Layout.fillWidth: true
                        pal: section.pal
                        from: section.lib.sample ? section.lib.sample.min : 12
                        to: section.lib.sample ? section.lib.sample.max : 48
                        stepSize: 1
                        snapMode: Slider.SnapAlways
                        value: section.sampleSize
                        onMoved: section.sampleSize = Math.round(sizeSlider.value)
                        onActiveFocusChanged: if (activeFocus) section.focused(sizeSlider)
                    }
                }

                // Свой текст образца: запоминается в настройках Rexon;
                // пустое поле — образец по умолчанию на языке интерфейса.
                RowLayout {
                    Layout.fillWidth: true
                    visible: section.view === "sample"
                    spacing: 8

                    RexonTextField {
                        id: sampleField
                        Layout.fillWidth: true
                        pal: section.pal
                        maximumLength: section.lib.sample ? section.lib.sample.maxLength : 300
                        placeholderText: section.lib.sample ? section.lib.sample.default : ""
                        // Поле показывает только свой текст: образец по
                        // умолчанию стоит подсказкой.
                        Component.onCompleted: sampleField.text = section.lib.sample
                                               && section.lib.sample.text !== section.lib.sample.default
                                               ? section.lib.sample.text : ""
                        ToolTip.visible: hovered
                        ToolTip.text: section.texts.sample_text_tip || ""
                        onTextEdited: sampleSave.restart()
                        onEditingFinished: if (sampleSave.running) { sampleSave.stop(); sampleSave.triggered(); }
                        onActiveFocusChanged: if (activeFocus) section.focused(sampleField)
                    }

                    RexonButton {
                        id: sampleReset
                        pal: section.pal
                        enabled: sampleField.text.length > 0
                        text: section.texts.sample_reset || ""
                        onClicked: {
                            sampleField.text = "";
                            sampleSave.stop();
                            sampleSave.triggered();
                        }
                        onActiveFocusChanged: if (activeFocus) section.focused(sampleReset)
                    }

                    // Запись — когда набор затих, а не на каждую букву.
                    Timer {
                        id: sampleSave
                        interval: 400
                        onTriggered: {
                            if (!section.actions || !section.lib.sample)
                                return;
                            const shown = section.actions.setFontSampleText(sampleField.text);
                            const sample = Object.assign({}, section.lib.sample);
                            sample.text = shown;
                            const lib = Object.assign({}, section.lib);
                            lib.sample = sample;
                            section.lib = lib;
                        }
                    }
                }

                ListView {
                    id: styleList
                    visible: section.view === "sample"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    spacing: 8
                    boundsBehavior: Flickable.StopAtBounds
                    ScrollBar.vertical: ScrollBar {}
                    model: section.current ? section.current.styles : []

                    delegate: ColumnLayout {
                        id: styleRow
                        required property var modelData
                        // Запас под полосу прокрутки: при 12 точках кнопки
                        // начертаний уходили под неё правым краем.
                        width: ListView.view.width - 22
                        spacing: 2

                        // Образец рисуется самим файлом шрифта: только что
                        // поставленный шрифт Qt ещё не знает по имени.
                        FontLoader {
                            id: loader
                            source: "file://" + encodeURI(styleRow.modelData.file).replace(/#/g, "%23")
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6

                            Text {
                                Layout.fillWidth: true
                                text: styleRow.modelData.style
                                      + (styleRow.modelData.disabled === true ? " · " + (section.texts.off || "") : "")
                                elide: Text.ElideRight
                                color: section.colTextSecondary
                                ToolTip.visible: styleHover.hovered
                                ToolTip.text: styleRow.modelData.file
                                HoverHandler { id: styleHover }
                            }

                            RexonButton {
                                id: toggleStyle
                                // Одно начертание переключается отдельно,
                                // только если у семейства их несколько.
                                visible: section.current !== null && section.current.styles.length > 1
                                pal: section.pal
                                readonly property bool turnOn: styleRow.modelData.disabled === true
                                text: toggleStyle.turnOn ? (section.texts.enable || "") : (section.texts.disable || "")
                                ToolTip.visible: hovered
                                ToolTip.text: toggleStyle.turnOn ? (section.texts.enable_tip || "")
                                                                 : (section.texts.disable_tip || "")
                                onClicked: section.did(section.actions.setFontEnabled(
                                                           section.current.family,
                                                           styleRow.modelData.file,
                                                           toggleStyle.turnOn))
                            }

                            RexonDangerButton {
                                id: removeStyle
                                // Одно начертание удаляется отдельно, только
                                // если у семейства их несколько.
                                visible: (styleRow.modelData.user === true || styleRow.modelData.shared === true)
                                         && section.current !== null
                                         && section.current.styles.length > 1
                                pal: section.pal
                                tone: section.sty.danger
                                text: "❌"
                                ToolTip.visible: hovered
                                ToolTip.text: section.texts.remove_tip || ""
                                onClicked: section.did(section.actions.removeFont(
                                                           section.current.family,
                                                           styleRow.modelData.file))
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            text: section.sampleText
                            wrapMode: Text.WordWrap
                            color: styleRow.modelData.disabled === true ? section.colTextSecondary
                                                                        : section.colText
                            font.pointSize: section.sampleSize
                            font.family: loader.status === FontLoader.Ready
                                         && loader.font.family === section.current.family
                                         ? loader.font.family : section.current.family
                            font.styleName: styleRow.modelData.style
                        }
                    }
                }

                // ── Таблица символов: блоки Юникода, что есть в файле ────
                ColumnLayout {
                    visible: section.view === "chars"
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 6

                    FontLoader {
                        id: charsLoader
                        source: section.charsFace === null ? ""
                                : "file://" + encodeURI(section.charsFace.file).replace(/#/g, "%23")
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        RexonComboBox {
                            id: charsStyleCombo
                            visible: section.current !== null && section.current.styles.length > 1
                            pal: section.pal
                            model: section.current ? section.current.styles.map(f => f.style) : []
                            currentIndex: section.charsStyle
                            ToolTip.visible: hovered
                            ToolTip.text: section.texts.chars_style_tip || ""
                            onActivated: index => section.charsStyle = index
                            onActiveFocusChanged: if (activeFocus) section.focused(charsStyleCombo)
                        }

                        RexonComboBox {
                            id: blockCombo
                            Layout.fillWidth: true
                            visible: (section.chars.blocks || []).length > 0
                            pal: section.pal
                            model: section.chars.blocks || []
                            textRole: "label"
                            valueRole: "id"
                            selectedValue: section.charsBlock
                            ToolTip.visible: hovered
                            ToolTip.text: section.texts.chars_block_tip || ""
                            onActivated: {
                                section.charsBlock = blockCombo.currentValue;
                                section.pickedChar = -1;
                            }
                            onActiveFocusChanged: if (activeFocus) section.focused(blockCombo)
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        visible: section.chars.blocks !== undefined && section.chars.blocks.length === 0
                        text: section.texts.chars_none || ""
                        wrapMode: Text.WordWrap
                        color: section.colTextSecondary
                    }

                    GridView {
                        id: charGrid
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true
                        // Клетка — не меньше 40 точек: так её удобно
                        // выбрать и пальцем, и геймпадом.
                        readonly property int cell: Math.max(40, Math.floor(charGrid.width / Math.max(1, Math.floor(charGrid.width / 44))))
                        cellWidth: charGrid.cell
                        cellHeight: charGrid.cell
                        boundsBehavior: Flickable.StopAtBounds
                        ScrollBar.vertical: ScrollBar {}
                        model: section.blockChars
                        keyNavigationEnabled: true
                        onCurrentIndexChanged: if (charGrid.activeFocus && currentIndex >= 0)
                                                   section.pickedChar = section.blockChars[currentIndex]
                        onActiveFocusChanged: if (activeFocus) section.focused(charGrid)

                        delegate: Rectangle {
                            id: charCell
                            required property var modelData
                            required property int index
                            width: charGrid.cellWidth - 2
                            height: charGrid.cellHeight - 2
                            radius: 3
                            color: charCell.modelData === section.pickedChar
                                   ? (section.pal.bg_card_hover || "#2a475e")
                                   : (charMouse.containsMouse ? (section.pal.surface_button || "#32363d") : "transparent")
                            border.color: section.colBorder
                            border.width: 1

                            Text {
                                anchors.centerIn: parent
                                text: String.fromCodePoint(charCell.modelData)
                                color: section.colText
                                font.pixelSize: Math.round(charCell.height * 0.55)
                                font.family: charsLoader.status === FontLoader.Ready ? charsLoader.font.family
                                             : (section.current ? section.current.family : "")
                                font.styleName: section.charsFace ? section.charsFace.style : ""
                            }

                            MouseArea {
                                id: charMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: {
                                    section.pickedChar = charCell.modelData;
                                    charGrid.currentIndex = charCell.index;
                                }
                            }
                        }
                    }

                    // Выбранный символ: код и сам символ — в поле, откуда
                    // их можно скопировать.
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8

                        Text {
                            Layout.fillWidth: section.pickedChar < 0
                            visible: section.pickedChar < 0
                            text: section.texts.char_pick || ""
                            elide: Text.ElideRight
                            color: section.colTextSecondary
                        }

                        Text {
                            visible: section.pickedChar >= 0
                            text: section.pickedChar >= 0 ? String.fromCodePoint(section.pickedChar) : ""
                            color: section.colText
                            font.pixelSize: 32
                            font.family: charsLoader.status === FontLoader.Ready ? charsLoader.font.family : ""
                            font.styleName: section.charsFace ? section.charsFace.style : ""
                        }

                        RexonTextField {
                            id: charCode
                            Layout.fillWidth: true
                            visible: section.pickedChar >= 0
                            pal: section.pal
                            readOnly: true
                            selectByMouse: true
                            text: section.pickedChar < 0 ? ""
                                  : "U+" + section.pickedChar.toString(16).toUpperCase().padStart(4, "0")
                                    + "  " + String.fromCodePoint(section.pickedChar)
                                    + "  &#" + section.pickedChar + ";"
                            onActiveFocusChanged: if (activeFocus) { charCode.selectAll(); section.focused(charCode); }
                        }
                    }
                }
            }
        }
    }

    // ── Дубликаты: одно начертание в нескольких файлах ───────────────────
    ColumnLayout {
        Layout.fillWidth: true
        visible: section.expanded && section.lib.available === true && section.showDuplicates
        spacing: 6

        Text {
            Layout.fillWidth: true
            text: section.dups.summary || ""
            wrapMode: Text.WordWrap
            font.bold: true
            color: section.colText
        }

        Text {
            Layout.fillWidth: true
            visible: (section.dups.groups || []).length > 0
            text: section.texts.duplicates_note || ""
            wrapMode: Text.WordWrap
            color: section.colTextSecondary
        }

        Rectangle {
            Layout.fillWidth: true
            // По содержимому, но не выше прежних 360: один найденный
            // дубликат не должен занимать пол-экрана пустой рамкой.
            Layout.preferredHeight: Math.min(360, dupList.contentHeight + 18)
            visible: (section.dups.groups || []).length > 0
            color: "transparent"
            border.color: section.colBorder
            border.width: 1
            radius: 4

            ListView {
                id: dupList
                anchors.fill: parent
                anchors.margins: 8
                clip: true
                spacing: 10
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar {}
                model: section.dups.groups || []

                delegate: ColumnLayout {
                    id: dupGroup
                    required property var modelData
                    width: ListView.view.width - 12
                    spacing: 4

                    Text {
                        Layout.fillWidth: true
                        text: dupGroup.modelData.family + " — " + dupGroup.modelData.style
                        font.bold: true
                        elide: Text.ElideRight
                        color: section.colText
                    }

                    Repeater {
                        model: dupGroup.modelData.copies

                        delegate: RowLayout {
                            id: copyRow
                            required property var modelData
                            Layout.fillWidth: true
                            spacing: 8

                            Text {
                                Layout.fillWidth: true
                                text: copyRow.modelData.file
                                elide: Text.ElideMiddle
                                color: section.colText
                                ToolTip.visible: copyHover.hovered
                                ToolTip.text: copyRow.modelData.file
                                HoverHandler { id: copyHover }
                            }

                            Text {
                                text: copyRow.modelData.user === true ? (section.texts.user || "")
                                      : copyRow.modelData.shared === true ? (section.texts.copy_shared || "")
                                      : (section.texts.copy_system || "")
                                color: section.colTextSecondary
                            }

                            // Удалить можно свою копию и поставленную для
                            // всех (с паролем); остальные принадлежат пакетам.
                            RexonDangerButton {
                                visible: copyRow.modelData.user === true || copyRow.modelData.shared === true
                                pal: section.pal
                                tone: section.sty.danger
                                text: section.texts.remove || ""
                                ToolTip.visible: hovered
                                ToolTip.text: section.texts.remove_tip || ""
                                onClicked: section.did(section.actions.removeFont(dupGroup.modelData.family,
                                                                                  copyRow.modelData.file))
                            }
                        }
                    }
                }
            }
        }
    }
}
