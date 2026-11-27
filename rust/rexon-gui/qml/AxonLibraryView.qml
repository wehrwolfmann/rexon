// Источник «Razer Axon» раздела «Оформление → Обои HD»: библиотека Razer
// Axon на этом компьютере — как раздел «Библиотека» самого Axon: поиск по
// названиям или тегам, сортировка, панель фильтров с числом включённых,
// разделы «Недавно добавленные» и «Можно синхронизировать».
//
// Только разметка: что в сетке, в каком порядке и все подписи — ядро
// (`rexon_core::axon_window`) через `AxonActions`
// (rust/rexon-gui/src/axon_actions.rs). Библиотека читается сама при
// заходе в источник — кнопки «Загрузить» нет.
//
// Плитка: двойной щелчок — подробности, правый щелчок (и клавиша меню) —
// «Применить» (на все экраны), «Перейти к расположению файла»,
// «Подробности». Enter на плитке — «Применить».

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: view

    /// Палитра скина страницы.
    property var pal: ({})

    /// Модель страницы «Оформления»: размеры плиток и расчёт столбцов —
    /// общие с сеткой магазина (`ap::grid_columns`/`grid_cell_width`).
    property var layout: null
    readonly property int gridSpacing: view.layout ? view.layout.gridSpacing : 14
    readonly property int gridMarginH: view.layout ? view.layout.gridMarginH : 16
    readonly property int previewHeight: view.layout ? view.layout.cardPreviewHeight : 170

    /// Строка для строки состояния страницы.
    signal message(string text)

    readonly property color colCard: view.pal.bg_card || "#1e2d3d"
    readonly property color colCardHover: view.pal.bg_card_hover || "#263d52"
    readonly property color colBorder: view.pal.border || "#2a4055"
    readonly property color colAccent: view.pal.accent || "#66c0f4"
    readonly property color colText: view.pal.text_primary || "#ffffff"
    readonly property color colTextSecondary: view.pal.text_secondary || "#c7d5e0"
    readonly property color colBg: view.pal.bg_primary || "#171d25"

    readonly property var t: JSON.parse(axon.texts())
    readonly property var playlistTexts: JSON.parse(playlist.texts())

    WallpaperPlaylistActions {
        id: playlist
        objectName: "axonPlaylistActions"
    }
    /// Ответ ядра по запросу (`axon_window::view_json`).
    property var v: ({ screen: "", note: "", sections: [], filter: { text: "", count: 0, flags: [], kinds: [] },
                       query: { sort: "date", mode: "titles", text: "" } })
    /// Подробности открытых обоев (`axon_window::details_json`).
    property var detail: ({})
    /// Обои, для которых открыто меню.
    property string menuId: ""

    function refresh() {
        view.v = JSON.parse(axon.view());
    }

    function reload() {
        axon.reload();
        view.refresh();
    }

    function apply(id) {
        view.message(axon.apply(id));
    }

    function showDetails(id) {
        const raw = axon.details(id);
        if (raw === "")
            return;
        view.detail = JSON.parse(raw);
        detailPopup.open();
    }

    function openMenu(id) {
        view.menuId = id;
        tileMenu.popup();
    }

    AxonActions {
        id: axon
        objectName: "axonActions"
        onFinished: message => view.message(message)
        // Обложки докачались в кэш — показать их в сетке и в подробностях.
        onCoversReady: {
            view.refresh();
            if (detailPopup.opened && view.detail.id) {
                const raw = axon.details(view.detail.id);
                if (raw !== "")
                    view.detail = JSON.parse(raw);
            }
        }
    }

    // Источник открыли — библиотека читается сама.
    onVisibleChanged: if (view.visible) view.reload()
    Component.onCompleted: if (view.visible) view.reload()

    // Ввод с задержкой 300 мс — как у поиска Axon.
    Timer {
        id: searchDebounce
        interval: 300
        onTriggered: {
            axon.setSearch(searchField.text);
            view.refresh();
        }
    }

    FolderDialog {
        id: folderDialog
        title: view.t.pickTitle || ""
        onAccepted: {
            const answer = JSON.parse(axon.addFolder(selectedFolder.toString()));
            view.message(answer.message);
            view.refresh();
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        // ── Поиск, сортировка, фильтр ─────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            spacing: 8
            visible: view.v.screen !== "note"

            RexonTextField {
                id: searchField
                objectName: "axonSearch"
                Layout.preferredWidth: 260
                Layout.minimumWidth: 140
                pal: view.pal
                borderTone: view.colBorder
                placeholderText: view.t.searchPlaceholder || ""
                onTextEdited: searchDebounce.restart()
                onAccepted: {
                    searchDebounce.stop();
                    axon.setSearch(searchField.text);
                    view.refresh();
                }
            }

            // «Названия обоев | Теги».
            Repeater {
                model: view.t.modes || []
                delegate: RexonButton {
                    required property var modelData
                    objectName: "axonMode_" + modelData.key
                    pal: view.pal
                    text: modelData.label
                    checkable: true
                    checked: view.v.query.mode === modelData.key
                    highlighted: checked
                    onClicked: {
                        axon.setSearchMode(modelData.key);
                        view.refresh();
                    }
                }
            }

            Item { Layout.fillWidth: true }

            RexonComboBox {
                id: sortBox
                objectName: "axonSort"
                pal: view.pal
                textRole: "label"
                valueRole: "key"
                model: view.t.sorts || []
                selectedValue: view.v.query.sort
                ToolTip.text: view.t.sortTooltip || ""
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onActivated: {
                    axon.setSort(sortBox.currentValue);
                    view.refresh();
                }
            }

            RexonButton {
                id: filterButton
                objectName: "axonFilter"
                pal: view.pal
                text: view.v.filter.text
                highlighted: view.v.filter.count > 0
                onClicked: filterPopup.opened ? filterPopup.close() : filterPopup.open()
            }

            RexonButton {
                objectName: "axonRefresh"
                pal: view.pal
                text: "🔄"
                onClicked: view.reload()
            }
        }

        // ── Сетка ─────────────────────────────────────────────────────
        ScrollView {
            id: scroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: view.v.screen === "grid"
            clip: true
            contentWidth: availableWidth

            ColumnLayout {
                width: scroll.availableWidth
                spacing: 12

                Repeater {
                    model: view.v.sections
                    delegate: ColumnLayout {
                        id: section
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.leftMargin: view.gridMarginH
                        Layout.rightMargin: view.gridMarginH
                        spacing: 8

                        RexonText {
                            Layout.fillWidth: true
                            text: section.modelData.title
                            color: view.colTextSecondary
                            font.bold: true
                        }

                        Flow {
                            id: tiles
                            Layout.fillWidth: true
                            spacing: view.gridSpacing
                            // Как у сетки магазина: столбцов столько, сколько
                            // влезает плиток наименьшей ширины, остаток ширины
                            // — в плитки. Ширина ячейки зависит только от
                            // ширины ряда, не от плиток, — цикла пересчёта нет.
                            // Зазор после последнего столбца ряду не нужен,
                            // поэтому он прибавлен к ширине.
                            readonly property int cellWidth: view.layout
                                ? view.layout.gridCellWidth(tiles.width + view.gridSpacing) : 314

                            Repeater {
                                model: section.modelData.items
                                delegate: Rectangle {
                                    id: tile
                                    required property var modelData
                                    objectName: "axonTile_" + tile.modelData.id
                                    width: tiles.cellWidth - view.gridSpacing
                                    height: view.previewHeight + 76
                                    radius: 6
                                    activeFocusOnTab: true
                                    color: tileMouse.containsMouse ? view.colCardHover : view.colCard
                                    border.width: tile.activeFocus ? 2 : 1
                                    border.color: (tile.activeFocus || tile.modelData.playing)
                                                  ? view.colAccent : view.colBorder
                                    opacity: tile.modelData.downloaded ? 1.0 : 0.6

                                    Keys.onReturnPressed: view.apply(tile.modelData.id)
                                    Keys.onEnterPressed: view.apply(tile.modelData.id)
                                    Keys.onMenuPressed: view.openMenu(tile.modelData.id)

                                    ColumnLayout {
                                        anchors.fill: parent
                                        anchors.margins: 6
                                        spacing: 4

                                        Rectangle {
                                            Layout.fillWidth: true
                                            Layout.preferredHeight: view.previewHeight
                                            radius: 4
                                            color: view.colBg
                                            clip: true

                                            Image {
                                                anchors.fill: parent
                                                visible: tile.modelData.cover !== ""
                                                source: tile.modelData.cover
                                                fillMode: Image.PreserveAspectCrop
                                                asynchronous: true
                                                sourceSize.width: 600
                                            }

                                            RexonText {
                                                anchors.centerIn: parent
                                                visible: tile.modelData.cover === ""
                                                text: "🖼️"
                                                font.pointSize: 28
                                            }
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: tile.modelData.title
                                            color: view.colText
                                            font.bold: true
                                            elide: Text.ElideRight
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: tile.modelData.line
                                            color: view.colTextSecondary
                                            elide: Text.ElideRight
                                        }
                                    }

                                    MouseArea {
                                        id: tileMouse
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                                        onClicked: mouse => {
                                            tile.forceActiveFocus();
                                            if (mouse.button === Qt.RightButton)
                                                view.openMenu(tile.modelData.id);
                                        }
                                        onDoubleClicked: mouse => {
                                            if (mouse.button === Qt.LeftButton)
                                                view.showDetails(tile.modelData.id);
                                        }
                                    }

                                    ToolTip.text: view.t.tileTooltip || ""
                                    ToolTip.visible: tileMouse.containsMouse
                                    ToolTip.delay: 1200
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Пусто, ничего не найдено, библиотеки нет ─────────────────
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: view.v.screen !== "grid" && view.v.screen !== ""
            spacing: 12

            Item { Layout.fillHeight: true }

            RexonText {
                objectName: "axonNote"
                Layout.fillWidth: true
                Layout.leftMargin: 32
                Layout.rightMargin: 32
                text: view.v.note
                color: view.colTextSecondary
                font.pointSize: 14
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
            }

            RexonButton {
                objectName: "axonClearFilter"
                Layout.alignment: Qt.AlignHCenter
                visible: view.v.screen === "notfound"
                pal: view.pal
                text: view.t.clearFilter || ""
                onClicked: {
                    axon.clearFilter();
                    searchField.text = "";
                    view.refresh();
                }
            }

            RexonButton {
                objectName: "axonPickFolder"
                Layout.alignment: Qt.AlignHCenter
                visible: view.v.screen === "note"
                pal: view.pal
                text: view.t.pick || ""
                ToolTip.text: view.t.pickTooltip || ""
                ToolTip.visible: hovered
                ToolTip.delay: 600
                onClicked: folderDialog.open()
            }

            Item { Layout.fillHeight: true }
        }
    }

    // ── Панель фильтров ──────────────────────────────────────────────
    Popup {
        id: filterPopup
        objectName: "axonFilterPopup"
        parent: filterButton
        y: filterButton.height + 4
        padding: 12
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent

        background: Rectangle {
            color: view.colCard
            border.color: view.colBorder
            border.width: 1
            radius: 4
        }

        ColumnLayout {
            spacing: 4

            Repeater {
                model: view.t.flags || []
                delegate: RexonCheckBox {
                    required property var modelData
                    objectName: "axonFlag_" + modelData.key
                    pal: view.pal
                    text: modelData.label
                    checked: view.v.filter.flags.indexOf(modelData.key) >= 0
                    onToggled: {
                        axon.setFilter(modelData.key, checked);
                        view.refresh();
                    }
                }
            }

            RexonText {
                Layout.topMargin: 6
                text: view.t.typeLabel || ""
                color: view.colTextSecondary
                font.bold: true
            }

            Repeater {
                model: view.t.kinds || []
                delegate: RexonCheckBox {
                    required property var modelData
                    objectName: "axonKind_" + modelData.key
                    pal: view.pal
                    text: modelData.label
                    checked: view.v.filter.kinds.indexOf(modelData.key) >= 0
                    onToggled: {
                        axon.setFilter(modelData.key, checked);
                        view.refresh();
                    }
                }
            }

            RexonButton {
                objectName: "axonFilterClear"
                Layout.topMargin: 6
                Layout.fillWidth: true
                pal: view.pal
                enabled: view.v.filter.count > 0
                text: view.t.clear || ""
                onClicked: {
                    axon.clearFilters();
                    view.refresh();
                }
            }
        }
    }

    // ── Меню плитки ──────────────────────────────────────────────────
    component AxonMenuItem: MenuItem {
        id: entry
        contentItem: RexonText {
            text: entry.text
            color: entry.enabled ? view.colText : view.colTextSecondary
            verticalAlignment: Text.AlignVCenter
            leftPadding: 8
        }
        background: Rectangle {
            implicitWidth: 240
            implicitHeight: 40
            radius: 3
            color: entry.highlighted ? view.colCardHover : "transparent"
            border.width: entry.visualFocus ? 2 : 0
            border.color: view.colAccent
        }
    }

    Menu {
        id: tileMenu
        objectName: "axonTileMenu"
        background: Rectangle {
            implicitWidth: 240
            color: view.colCard
            border.color: view.colBorder
            border.width: 1
            radius: 4
        }

        AxonMenuItem {
            objectName: "axonMenuApply"
            text: view.t.apply || ""
            ToolTip.text: view.t.applyTooltip || ""
            ToolTip.visible: hovered
            ToolTip.delay: 600
            onTriggered: view.apply(view.menuId)
        }

        // Плейлист обоев Rexon (блок в «⚙️ Настройках» раздела).
        AxonMenuItem {
            objectName: "axonMenuPlaylist"
            text: view.playlistTexts.addToPlaylist || ""
            onTriggered: {
                const answer = JSON.parse(playlist.edit(JSON.stringify({ op: "addAxon", id: view.menuId })));
                view.message(answer.message);
            }
        }

        AxonMenuItem {
            objectName: "axonMenuLocation"
            text: view.t.openLocation || ""
            onTriggered: axon.openLocation(view.menuId)
        }

        AxonMenuItem {
            objectName: "axonMenuDetails"
            text: view.t.details || ""
            onTriggered: view.showDetails(view.menuId)
        }
    }

    // ── Подробности ──────────────────────────────────────────────────
    Popup {
        id: detailPopup
        objectName: "axonDetails"
        parent: view
        modal: true
        focus: true
        anchors.centerIn: parent
        width: Math.min(view.width - 40, 720)
        height: Math.min(view.height - 40, detailBody.implicitHeight + 2 * padding)
        padding: 16
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        background: Rectangle {
            color: view.colBg
            border.color: view.colBorder
            border.width: 1
            radius: 8
        }

        onOpened: detailClose.forceActiveFocus()

        ScrollView {
            anchors.fill: parent
            clip: true
            contentWidth: availableWidth

            ColumnLayout {
                id: detailBody
                width: detailPopup.availableWidth
                spacing: 8

                Image {
                    Layout.fillWidth: true
                    Layout.preferredHeight: visible ? Math.round(width * 9 / 16) : 0
                    visible: (view.detail.cover || "") !== ""
                    source: view.detail.cover || ""
                    fillMode: Image.PreserveAspectFit
                    asynchronous: true
                }

                RexonText {
                    Layout.fillWidth: true
                    text: view.detail.title || ""
                    color: view.colText
                    font.bold: true
                    font.pointSize: 15
                    wrapMode: Text.WordWrap
                }

                RexonText {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: view.detail.author || ""
                    color: view.colTextSecondary
                    wrapMode: Text.WordWrap
                }

                RexonText {
                    Layout.fillWidth: true
                    text: view.detail.line || ""
                    color: view.colTextSecondary
                    wrapMode: Text.WordWrap
                }

                Repeater {
                    model: view.detail.rows || []
                    delegate: RowLayout {
                        id: detailRow
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 8

                        RexonText {
                            Layout.preferredWidth: 140
                            text: detailRow.modelData.label
                            color: view.colTextSecondary
                            font.bold: true
                        }

                        RexonText {
                            Layout.fillWidth: true
                            text: detailRow.modelData.value
                            color: view.colText
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                RexonText {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: view.detail.description || ""
                    color: view.colText
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        objectName: "axonDetailApply"
                        pal: view.pal
                        highlighted: true
                        enabled: view.detail.downloaded === true
                        text: view.t.apply || ""
                        onClicked: {
                            view.apply(view.detail.id);
                            detailPopup.close();
                        }
                    }

                    RexonButton {
                        objectName: "axonDetailLocation"
                        pal: view.pal
                        enabled: (view.detail.folder || "") !== ""
                        text: view.t.openLocation || ""
                        ToolTip.text: view.detail.folder || ""
                        ToolTip.visible: hovered && (view.detail.folder || "") !== ""
                        onClicked: axon.openLocation(view.detail.id)
                    }

                    Item { Layout.fillWidth: true }

                    RexonButton {
                        id: detailClose
                        pal: view.pal
                        text: view.t.close || ""
                        onClicked: detailPopup.close()
                    }
                }
            }
        }
    }
}
