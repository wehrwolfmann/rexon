// The "Razer Axon" source of the "Appearance → HD Wallpapers" section: the
// Razer Axon library on this computer — like the "Library" section of Axon
// itself: search by titles or tags, sorting, a filter panel with the
// number of active filters, the "Recently added" and "Can be synced"
// sections.
//
// There is NO search field here: the word is typed into the shared field
// in the page header, the same as for KDE Store and Wallhaven (`query`
// comes from the page) — a field of our own drifted to the lower left and
// lost the word when the store changed (live check 16.11.2026). Where to
// search — titles or tags — is set in the "Filter".
//
// Markup only: what is in the grid, in what order, and all labels — the
// core (`rexon_core::axon_window`) through `AxonActions`
// (rust/rexon-gui/src/axon_actions.rs). The library is read by itself on
// entering the source — there is no "Load" button.
//
// Tile: double click — details, right click (and the menu key) — "Apply"
// (to all screens), "Go to file location", "Details". Enter on a tile
// means "Apply".

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: view

    /// Page skin palette.
    property var pal: ({})

    /// The "Appearance" page model: tile sizes and the column calculation are
    /// shared with the store grid (`ap::grid_columns`/`grid_cell_width`).
    property var layout: null
    readonly property int gridSpacing: view.layout ? view.layout.gridSpacing : 14
    readonly property int gridMarginH: view.layout ? view.layout.gridMarginH : 16
    readonly property int previewHeight: view.layout ? view.layout.cardPreviewHeight : 170

    /// Word from the shared search field of the page header
    /// (`AppearanceModel.query`).
    property string query: ""

    /// Text for the page status line.
    signal message(string text)
    /// "Clear filter" also reset the search — clear the header field.
    signal searchCleared()

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
    /// Core answer to the request (`axon_window::view_json`).
    property var v: ({ screen: "", note: "", sections: [], filter: { text: "", count: 0, flags: [], kinds: [] },
                       query: { sort: "date", mode: "titles", text: "" } })
    /// Details of the opened wallpaper (`axon_window::details_json`).
    property var detail: ({})
    /// Wallpaper the menu is open for.
    property string menuId: ""

    function refresh() {
        view.v = JSON.parse(axon.view());
    }

    function reload() {
        axon.reload();
        // The word was typed in another store before coming here: apply it at once.
        searchDebounce.stop();
        axon.setSearch(view.query);
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
        // Covers finished downloading into the cache — show them in the grid and
        // in the details.
        onCoversReady: {
            view.refresh();
            if (detailPopup.opened && view.detail.id) {
                const raw = axon.details(view.detail.id);
                if (raw !== "")
                    view.detail = JSON.parse(raw);
            }
        }
    }

    // The source was opened — the library is read by itself.
    onVisibleChanged: if (view.visible) view.reload()
    Component.onCompleted: if (view.visible) view.reload()

    // Input with a 300 ms delay — as in Axon's search.
    Timer {
        id: searchDebounce
        interval: 300
        onTriggered: {
            axon.setSearch(view.query);
            view.refresh();
        }
    }
    onQueryChanged: searchDebounce.restart()

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

        // ── Sorting, filter ───────────────────────────────────────────
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            spacing: 8
            visible: view.v.screen !== "note"

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
                ToolTip.visible: hintVisible
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

        }

        // ── Grid ──────────────────────────────────────────────────────
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
                            // As in the store grid: as many columns as tiles of the minimum width
                            // fit, and the remaining width goes into the tiles. The cell width
                            // depends only on the row width, not on the tiles, so there is no
                            // recompute loop. The row does not need the gap after the last column,
                            // so it is added to the width.
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
                                    // While the tile menu is open (right click) there is no tooltip: the
                                    // cursor stays over the tile, and the tooltip would hang over the menu
                                    // items.
                                    ToolTip.visible: tileMouse.containsMouse
                                                     && !tileMenu.visible
                                    ToolTip.delay: 1200
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Empty, nothing found, no library ─────────────────────────
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
                    view.searchCleared();
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

    // ── Filter panel ─────────────────────────────────────────────────
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

            // "Search in: Wallpaper titles | Tags" — the mode of the shared search field.
            RexonText {
                text: view.t.searchIn || ""
                color: view.colTextSecondary
                font.bold: true
            }

            RowLayout {
                spacing: 6
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
            }

            Item { implicitHeight: 6 }

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

    // ── Tile menu ────────────────────────────────────────────────────
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
        // Width follows the widest item: the `Menu` of the Basic and Fusion styles
        // does not follow the labels, and a long label in another language was
        // cut ("Go to file locat…"). The same calculation as in the window menu
        // (`WindowMenu.qml`).
        implicitWidth: {
            let widest = 0;
            for (let i = 0; i < tileMenu.count; ++i) {
                const entry = tileMenu.itemAt(i) as MenuItem;
                if (entry && entry.contentItem)
                    widest = Math.max(widest, entry.contentItem.implicitWidth
                                      + entry.leftPadding + entry.rightPadding + 16);
            }
            return Math.max(tileMenu.implicitBackgroundWidth + tileMenu.leftInset + tileMenu.rightInset,
                            widest + tileMenu.leftPadding + tileMenu.rightPadding);
        }
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

        // Rexon wallpaper playlist (the block in the section's "⚙️ Settings").
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

    // ── Details ──────────────────────────────────────────────────────
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
