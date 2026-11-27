// The "Screensaver" sub-tab of the "Steam" tab: the screensavers of the Steam
// Big Picture idle screen (Steam Deck, Steam Machine, a TV, an ordinary
// computer) as cards with a preview — the same grid, large card and
// «Watch Full Screen» as in the «Video splash» sub-tab:
//   * the built-in screensavers of the installed Steam and the person's own
//     ones from `uioverrides` — Steam shows them itself; the card only shows
//     them and says where to choose them;
//   * the Rexon modes (a slide show of game art and screenshots, a bouncing
//     Rexon logo) — the selected one is what «Install Screensaver» puts into
//     Steam.
// Without Steam only the Rexon cards are there.
//
// Markup only. The catalog, options, limits, all phrases, the snapshots and
// the installation itself — the core `rexon_core::steam_screensaver`; the
// executor is `SteamScreensaverActions`
// (rust/rexon-gui/src/steam_screensaver_actions.rs).
//
// Never: Rexon does not touch the screensaver choice in Steam: it only reads
// it to mark the card «Selected in Steam»; the user picks it themselves in the
// Big Picture settings, and the hint at the bottom says where. Rexon ships no
// Steam logo: the built-in Steam pages are only read from the installed Steam.

pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

Rectangle {
    id: section
    RexonBidi { id: bidi }

    property var pal: ({})
    /// Tab styling (`SteamModel.styles`): group, inset, card, thumbnails, buttons.
    property var sty: ({})
    /// Error colour — ready from the tab core (`SteamModel.goldColor`).
    property color warnColor: section.pal.text_primary

    signal message(string text)
    /// Open the shared large view of the tab (`RexonImageViewer`).
    signal zoomRequested(var items, int index)

    // ── State ──────────────────────────────────────────────────────────

    /// Answer of `actions.choices()`: options, limits, hints, card sizes.
    property var ch: ({ modes: [], logos: [], intervalMin: 4, intervalMax: 60, howTo: "", restartHint: "",
                        thumbWidth: 176, thumbHeight: 110, cardWidth: 200, heroWidth: 400, heroHeight: 250,
                        titleElideWidth: 154, showText: "", logo: "" })
    /// Answer of `actions.status()`.
    property var st: ({ steamFound: false, installed: false, foreign: false, text: "", options: {}, updated: 0, roots: [] })
    /// Answer of `actions.catalog()`.
    property var cards: []
    /// Key of the selected card.
    property string selectedKey: ""
    /// Snapshots of the HTML screensavers: card key → picture.
    property var snapshots: ({})
    property string snapshotNote: ""
    /// Pictures of the Rexon slide show (with the settings below) and of the
    /// Steam game slideshow — for the thumbnails and the large view.
    property var pictures: []
    property var steamPictures: []

    property string mode: "slideshow"
    property string logo: "rexon"
    property bool gameArt: true
    property bool screenshots: true
    property bool clock: true
    property int interval: 9

    property bool busy: false
    property bool showBusy: false
    property string jobError: ""
    property bool scanBusy: false
    property string foundText: ""
    /// The first press of "Delete" only arms it: a second press within a few
    /// seconds deletes. There is no question window — it is awkward in Big
    /// Picture.
    property bool confirmRemove: false

    readonly property var selected: section.cards.find(c => c.key === section.selectedKey) || null
    readonly property bool selectedIsRexon: section.selected !== null && section.selected.origin === "rexon"

    function options() {
        return { mode: section.mode, logo: section.logo, gameArt: section.gameArt,
                 screenshots: section.screenshots, clock: section.clock, interval: section.interval };
    }

    function takeOptions(o) {
        if (!o)
            return;
        section.mode = o.mode || "slideshow";
        section.logo = o.logo || "rexon";
        section.gameArt = o.gameArt !== false;
        section.screenshots = o.screenshots !== false;
        section.clock = o.clock !== false;
        section.interval = o.interval || 9;
    }

    /// The settings differ from the installed screensaver — "Update" is needed.
    readonly property bool dirty: section.st.installed
                                  && JSON.stringify(section.options()) !== JSON.stringify(section.st.options)

    /// Picture of a card: a page snapshot, the first slide-show picture or none
    /// (the bouncing logo is drawn live).
    function thumbOf(c) {
        if (!c)
            return "";
        if (c.preview === "html")
            return section.snapshots[c.key] || c.thumb || "";
        if (c.preview === "slideshow") {
            const list = c.origin === "rexon" ? section.pictures : section.steamPictures;
            return list.length > 0 ? list[0].thumb : "";
        }
        return "";
    }

    function picturesOf(c) {
        if (!c || c.preview !== "slideshow")
            return [];
        return c.origin === "rexon" ? section.pictures : section.steamPictures;
    }

    function select(key) {
        section.selectedKey = key;
        if (key.startsWith("rexon:"))
            section.mode = key.slice("rexon:".length);
    }

    function zoom(c) {
        const items = section.picturesOf(c);
        if (items.length > 0) {
            section.zoomRequested(items, 0);
            return;
        }
        const pic = section.thumbOf(c);
        if (pic !== "")
            section.zoomRequested([{ key: c.key, title: c.title, thumb: pic, source: pic }], 0);
    }

    function reloadCatalog() {
        section.cards = JSON.parse(actions.catalog());
        if (!section.cards.some(c => c.key === section.selectedKey)) {
            const installed = section.cards.find(c => c.installed);
            section.select(installed ? installed.key : "rexon:" + section.mode);
        }
        actions.startSnapshots();
        poll.start();
    }

    function rescan() {
        actions.startScan(JSON.stringify(section.options()));
        section.scanBusy = true;
        poll.start();
    }

    function install() {
        const err = actions.startInstall(JSON.stringify(section.options()));
        if (err !== "") {
            section.jobError = err;
            return;
        }
        section.jobError = "";
        section.busy = true;
        poll.start();
    }

    function remove() {
        if (!section.confirmRemove) {
            section.confirmRemove = true;
            disarm.restart();
            return;
        }
        section.confirmRemove = false;
        const err = actions.startRemove();
        if (err !== "") {
            section.jobError = err;
            return;
        }
        section.jobError = "";
        section.busy = true;
        poll.start();
    }

    /// «Watch Full Screen». The Steam game slideshow is drawn by Steam itself —
    /// its pictures are shown in the large view instead.
    function showFullScreen() {
        const c = section.selected;
        if (!c)
            return;
        if (c.preview === "slideshow" && c.origin !== "rexon") {
            section.zoom(c);
            return;
        }
        const err = actions.startShow(c.key, JSON.stringify(section.options()));
        if (err !== "") {
            section.message(err);
            return;
        }
        if (c.origin === "rexon") {
            section.showBusy = true;
            poll.start();
        }
    }

    // What goes into the slide show depends on two check boxes — recompute.
    onGameArtChanged: rescanSoon.restart()
    onScreenshotsChanged: rescanSoon.restart()

    SteamScreensaverActions {
        id: actions
        objectName: "steamScreensaverActions"
    }

    Timer {
        id: rescanSoon
        interval: 300
        onTriggered: section.rescan()
    }

    Timer {
        id: disarm
        interval: 4000
        onTriggered: section.confirmRemove = false
    }

    Timer {
        id: poll
        interval: 150
        repeat: true
        onTriggered: {
            const p = JSON.parse(actions.poll());
            if (section.scanBusy && p.scan.done) {
                section.scanBusy = false;
                section.foundText = p.scan.text;
                section.pictures = p.scan.pictures || [];
                section.steamPictures = p.scan.steamPictures || [];
            }
            if (section.busy && p.job.done) {
                section.busy = false;
                section.st = p.job.status;
                if (p.job.ok)
                    section.message(p.job.message);
                else
                    section.jobError = p.job.message;
                // «Installed» / «Selected in Steam» marks follow the new state.
                section.cards = JSON.parse(actions.catalog());
            }
            if (section.showBusy && p.show.done) {
                section.showBusy = false;
                if (!p.show.ok)
                    section.message(p.show.message);
            }
            if (JSON.stringify(p.snapshots.shots) !== JSON.stringify(section.snapshots))
                section.snapshots = p.snapshots.shots;
            section.snapshotNote = p.snapshots.note;
            if (!section.scanBusy && !section.busy && !section.showBusy && !p.snapshots.busy)
                poll.stop();
        }
    }

    Component.onCompleted: {
        section.ch = JSON.parse(actions.choices());
        section.st = JSON.parse(actions.status());
        section.takeOptions(section.st.options);
        section.reloadCatalog();
        section.rescan();
    }

    // ── A live bouncing Rexon logo — the preview of the «Bouncing logo» mode ──
    // The same motion as on the page: straight lines, a bounce off every edge.
    // It stands still whenever it is not on screen (another sub-tab, a hidden
    // card).
    component BounceLogo: Item {
        id: bl
        property url source
        /// Seconds to cross the shorter side — as on the page.
        property real travelS: 10
        property real vx: 1
        property real vy: 1
        clip: true
        Image {
            id: logoImg
            source: bl.source
            width: Math.max(16, Math.round(Math.min(bl.width, bl.height) * 0.3))
            height: logoImg.width
            fillMode: Image.PreserveAspectFit
            smooth: true
            asynchronous: true
        }
        // The start point is set once, not bound: the motion below moves the logo
        // by hand, and a physical bounce has no reading direction to mirror.
        Component.onCompleted: {
            logoImg.x = (bl.width - logoImg.width) / 3;
            logoImg.y = (bl.height - logoImg.height) / 4;
        }
        FrameAnimation {
            running: bl.visible && bl.width > 0 && bl.height > 0
            onTriggered: {
                const speed = Math.min(bl.width, bl.height) / bl.travelS;
                const step = Math.min(frameTime, 0.1) * speed;
                let nx = logoImg.x + bl.vx * step;
                let ny = logoImg.y + bl.vy * step;
                const maxX = bl.width - logoImg.width;
                const maxY = bl.height - logoImg.height;
                if (nx <= 0 || nx >= maxX) {
                    bl.vx = -bl.vx;
                    nx = Math.max(0, Math.min(maxX, nx));
                }
                if (ny <= 0 || ny >= maxY) {
                    bl.vy = -bl.vy;
                    ny = Math.max(0, Math.min(maxY, ny));
                }
                logoImg.x = nx;
                logoImg.y = ny;
            }
        }
    }

    color: section.sty.group.background
    border.color: section.sty.group.border
    border.width: 1
    radius: section.sty.group.radius
    implicitHeight: body.implicitHeight + 2 * (section.sty.group.marginV + border.width)

    readonly property real labelWidth: 220
    readonly property var logoInfo: section.ch.logos.find(m => m.key === section.logo) || {}

    ColumnLayout {
        id: body
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: section.sty.group.marginH + section.border.width
        y: section.sty.group.marginV + section.border.width
        spacing: section.sty.group.spacing

        RexonText {
            Layout.fillWidth: true
            text: qsTranslate("rexon", "🌙 Screensaver")
            color: section.pal.text_primary
            font.bold: true
            font.pointSize: 13
        }
        RexonText {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: qsTranslate("rexon", "Steam shows a screensaver when you leave your Steam Deck, Steam Machine, TV or computer idle in Big Picture. Here are the screensavers of your Steam and the ones Rexon can make for it.")
        }

        // ── State ─────────────────────────────────────────────────────
        RexonText {
            objectName: "screensaverStatus"
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            color: section.pal.text_primary
            font.bold: true
            text: section.st.text
        }
        RexonText {
            Layout.fillWidth: true
            visible: section.st.installed && section.st.updated > 0
            color: section.pal.text_secondary
            text: bidi.arg(qsTranslate("rexon", "Updated: %1"), new Date(section.st.updated * 1000).toLocaleDateString(Qt.locale(), Locale.ShortFormat))
        }

        // ── Large card of the selected screensaver ────────────────────
        Rectangle {
            objectName: "screensaverHero"
            Layout.fillWidth: true
            visible: section.selected !== null
            implicitHeight: heroRow.implicitHeight + 28
            color: section.sty.card.background
            radius: section.sty.card.radius
            border.color: heroHover.hovered ? section.sty.card.hover : section.sty.card.border
            border.width: 1
            HoverHandler { id: heroHover }

            RowLayout {
                id: heroRow
                anchors.fill: parent
                anchors.margins: 14
                spacing: 16

                Rectangle {
                    Layout.alignment: Qt.AlignTop
                    implicitWidth: section.ch.heroWidth
                    implicitHeight: section.ch.heroHeight
                    radius: section.sty.thumb.heroRadius
                    color: section.sty.thumb.heroBackground
                    clip: true

                    Image {
                        anchors.fill: parent
                        source: section.thumbOf(section.selected)
                        fillMode: Image.PreserveAspectFit
                        asynchronous: true
                        visible: source !== ""
                    }
                    Loader {
                        anchors.fill: parent
                        active: section.selected !== null && section.selected.preview === "bounce"
                        sourceComponent: BounceLogo { source: section.ch.logo }
                    }
                    RexonText {
                        anchors.centerIn: parent
                        visible: section.selected !== null && section.selected.preview !== "bounce"
                                 && section.thumbOf(section.selected) === ""
                        text: "🌙"
                        font.pixelSize: 40
                    }
                    RexonZoomArea {
                        anchors.fill: parent
                        pal: section.pal
                        radius: section.sty.thumb.heroRadius
                        enabled: section.thumbOf(section.selected) !== ""
                        visible: enabled
                        onActivated: section.zoom(section.selected)
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 7

                    RexonText {
                        Layout.fillWidth: true
                        text: section.selected ? section.selected.title : ""
                        color: section.pal.text_primary
                        font.pointSize: 17
                        font.bold: true
                        wrapMode: Text.WordWrap
                    }
                    RexonText {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: section.selected ? section.selected.state : ""
                        color: section.pal.text_primary
                        wrapMode: Text.WordWrap
                    }
                    RexonText {
                        objectName: "screensaverHint"
                        Layout.fillWidth: true
                        text: section.selected ? section.selected.hint : ""
                        color: section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }
                    RexonText {
                        Layout.fillWidth: true
                        visible: text !== "" && section.selected !== null && section.selected.preview === "html"
                                 && section.thumbOf(section.selected) === ""
                        text: section.snapshotNote
                        color: section.pal.text_secondary
                        wrapMode: Text.WordWrap
                    }
                    Item { Layout.fillHeight: true }

                    RowLayout {
                        spacing: 10
                        RexonButton {
                            objectName: "screensaverShow"
                            pal: section.pal
                            enabled: !section.showBusy
                            text: section.ch.showText
                            onClicked: section.showFullScreen()
                        }
                        RexonButton {
                            objectName: "screensaverInstall"
                            visible: section.selectedIsRexon
                            pal: section.pal
                            tone: section.sty.primaryButton
                            enabled: section.st.steamFound && !section.busy
                            text: section.st.installed ? qsTranslate("rexon", "Update Screensaver")
                                                       : qsTranslate("rexon", "Install Screensaver")
                            ToolTip.visible: hovered && section.st.installed
                            ToolTip.text: qsTranslate("rexon", "Collects fresh artwork and screenshots and applies the settings above.")
                            onClicked: section.install()
                        }
                        RexonDangerButton {
                            objectName: "screensaverRemove"
                            visible: section.selectedIsRexon && section.st.installed
                            enabled: !section.busy
                            pal: section.pal
                            text: section.confirmRemove ? qsTranslate("rexon", "Click again to remove")
                                                        : qsTranslate("rexon", "Remove")
                            onClicked: section.remove()
                        }
                        Item { Layout.fillWidth: true }
                    }

                    RexonText {
                        Layout.fillWidth: true
                        wrapMode: Text.WordWrap
                        visible: text !== ""
                        color: section.jobError !== "" ? section.warnColor : section.pal.text_secondary
                        text: section.busy || section.showBusy ? qsTranslate("rexon", "Preparing the pictures…")
                              : section.jobError !== "" ? section.jobError
                              : section.dirty && section.selectedIsRexon
                                ? qsTranslate("rexon", "Press «Update Screensaver» to apply the changes.")
                              : ""
                    }
                }
            }
        }

        // ── Settings of the Rexon modes ───────────────────────────────
        // Slide show: what to build it from and how often to change the frame.
        ColumnLayout {
            Layout.fillWidth: true
            visible: section.selectedIsRexon && section.mode === "slideshow"
            enabled: !section.busy
            spacing: section.sty.group.spacing

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: section.labelWidth + 8
                spacing: 16
                RexonCheckBox {
                    objectName: "screensaverGameArt"
                    pal: section.pal
                    text: qsTranslate("rexon", "Game artwork")
                    checked: section.gameArt
                    onToggled: section.gameArt = checked
                }
                RexonCheckBox {
                    objectName: "screensaverShots"
                    pal: section.pal
                    text: qsTranslate("rexon", "My screenshots")
                    checked: section.screenshots
                    onToggled: section.screenshots = checked
                }
                Item { Layout.fillWidth: true }
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                RexonText {
                    Layout.preferredWidth: section.labelWidth
                    text: qsTranslate("rexon", "Next picture every:")
                    color: section.pal.text_primary
                }
                RexonSlider {
                    objectName: "screensaverInterval"
                    Layout.fillWidth: true
                    pal: section.pal
                    from: section.ch.intervalMin
                    to: section.ch.intervalMax
                    stepSize: 1
                    value: section.interval
                    onMoved: section.interval = Math.round(value)
                }
                RexonText {
                    Layout.preferredWidth: 60
                    text: bidi.arg(qsTranslate("rexon", "%1 s"), section.interval)
                    color: section.pal.text_secondary
                }
            }
            RexonText {
                Layout.fillWidth: true
                Layout.leftMargin: section.labelWidth + 8
                visible: text !== ""
                wrapMode: Text.WordWrap
                color: section.pal.text_secondary
                text: section.scanBusy ? qsTranslate("rexon", "Looking for pictures in Steam…") : section.foundText
            }
        }

        // Bouncing logo: whose.
        RowLayout {
            Layout.fillWidth: true
            visible: section.selectedIsRexon && section.mode === "bounce"
            enabled: !section.busy
            spacing: 8
            RexonText {
                Layout.preferredWidth: section.labelWidth
                text: qsTranslate("rexon", "Logo:")
                color: section.pal.text_primary
            }
            RexonComboBox {
                id: logoCombo
                objectName: "screensaverLogo"
                Layout.fillWidth: true
                pal: section.pal
                textRole: "label"
                valueRole: "key"
                model: section.ch.logos
                selectedValue: section.logo
                onActivated: section.logo = logoCombo.currentValue
            }
        }
        RexonText {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            visible: section.selectedIsRexon && section.mode === "bounce"
            wrapMode: Text.WordWrap
            color: section.pal.text_secondary
            text: section.logoInfo.hint || ""
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: section.labelWidth + 8
            visible: section.selectedIsRexon
            enabled: !section.busy
            RexonCheckBox {
                objectName: "screensaverClock"
                pal: section.pal
                text: qsTranslate("rexon", "Show the clock")
                checked: section.clock
                onToggled: section.clock = checked
            }
            Item { Layout.fillWidth: true }
        }

        // ── Card grid ─────────────────────────────────────────────────
        GridView {
            id: grid
            objectName: "screensaverGrid"
            Layout.fillWidth: true
            Layout.preferredHeight: Math.max(cellHeight, cellHeight * Math.ceil(count / columns))
            interactive: false
            // As many cards as fit; the remaining width is shared among them — the
            // same rule as the «Video splash» grid.
            readonly property int columns: Math.max(1, Math.floor(width / (section.ch.cardWidth + section.sty.card.gap)))
            cellWidth: Math.max(section.ch.cardWidth + section.sty.card.gap, Math.floor(width / columns))
            cellHeight: section.ch.thumbHeight + 96
            model: section.cards

            delegate: SteamMediaCard {
                id: ssCard
                required property var modelData

                width: grid.cellWidth
                height: grid.cellHeight
                pal: section.pal
                sty: section.sty
                thumbWidth: section.ch.thumbWidth
                thumbHeight: section.ch.thumbHeight
                titleElideWidth: section.ch.titleElideWidth
                title: ssCard.modelData.title
                note: ssCard.modelData.state
                poster: section.thumbOf(ssCard.modelData)
                placeholder: "🌙"
                live: ssCard.modelData.preview === "bounce"
                badge: ssCard.modelData.badge
                badgeBackground: ssCard.modelData.badgeBackground
                badgeColor: ssCard.modelData.badgeColor
                selected: ssCard.modelData.key === section.selectedKey

                onChosen: section.select(ssCard.modelData.key)
                onZoomed: section.zoom(ssCard.modelData)

                Loader {
                    anchors.fill: parent
                    active: ssCard.live
                    sourceComponent: BounceLogo { source: section.ch.logo }
                }
            }
        }

        // ── How to enable it in Steam ─────────────────────────────────
        Rectangle {
            Layout.fillWidth: true
            Layout.topMargin: 8
            implicitHeight: howCol.implicitHeight + section.sty.pitch.marginV * 2
            color: section.sty.pitch.background
            radius: section.sty.pitch.radius
            Rectangle {
                width: section.sty.pitch.barWidth
                height: parent.height
                color: section.sty.pitch.bar
            }
            ColumnLayout {
                id: howCol
                anchors.fill: parent
                anchors.margins: section.sty.pitch.marginV
                anchors.leftMargin: section.sty.pitch.marginH
                anchors.rightMargin: section.sty.pitch.marginH
                spacing: 4
                RexonText {
                    Layout.fillWidth: true
                    text: qsTranslate("rexon", "How to turn it on in Steam")
                    color: section.pal.text_primary
                    font.bold: true
                }
                RexonText {
                    objectName: "screensaverHowTo"
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: section.pal.text_primary
                    text: section.ch.howTo
                }
                RexonText {
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                    color: section.pal.text_secondary
                    text: section.ch.restartHint
                }
            }
        }
    }
}
