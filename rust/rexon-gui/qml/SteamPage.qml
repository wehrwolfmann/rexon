// The "Steam" tab in QML. Data and decisions come from the Rust object
// SteamModel (rust/rexon-gui/src/steam_page.rs), which decides nothing
// itself but asks the core (`rexon_core::qt_steam`). Only MARKUP lives
// here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from the default
// skin of the whole product (rule #17). The page defines no colours of its
// own — the tab-specific colours (accent, "applied", "in favourites", the
// state chip, the card badge) also arrive from the core as ready strings.
//
// Side actions live outside: the page host puts into `root.actions` an
// object with the methods
//   scan()            -> {movies, current, favorites, enabled, rotate, sound}
//   pickFile()        -> string (picker window for your own video)
//   select(path)      -> bool   (save the choice to the splash config)
//   setFavorite(path) -> favourites list after toggling
//   setSound(on)      -> bool
//   setRotation(on)   -> bool
//   setAutostart(on)  -> string ("" — it worked, otherwise an error text)
//   play(path, sound) -> string ("" — it started, otherwise an error text)
// The page has two hosts, and they share ONE contract — that is why it did
// not change:
//   * the live product puts in the Python object `SteamActions`
//     (rexon/qt_steam/qml_window.py) on top of rexon/session_splash.py and
//     Steam token retrieval (archived, git tag python-final);
//   * the Rust window (`qml/MainWindow.qml`) puts in `SteamActions` from
//     rust/rexon-gui/src/steam_actions.rs — the same eight names, the same
//     answers, and the core inside: `session_splash` + `splash_exec` +
//     `steam_token`.
//
// The focus chain is built in from the start: all actions are on controls,
// the focused one shows a frame, and the page scrolls to it.
//
// ── Live clip preview in the large card ──────────────────────────────────
//
// The card shows not a frame but the clip itself — like an item's clip in
// a store. In the widget original (rexon/qt_steam/window.py) this was a
// QMediaPlayer + QVideoSink pair whose frames were taken by hand and drawn
// with a QLabel: there a live QVideoWidget defect had to be worked around
// (its own native window lay across the tab), and frames also had to be
// thinned out to 15 fps by hand, because every expansion of a frame into a
// QImage cost ~8 MB.
//
// Neither is needed here: `VideoOutput` is an ordinary scene graph node, it
// creates no window of its own and does not expand frames into RAM at all.
// So both workarounds are removed (rule #16), while the BEHAVIOUR is
// reproduced one to one:
//   * exactly the clip the card shows is playing (the hovered one, and
//     without hover the selected one);
//   * there is no sound at all: `audioOutput` is not set — the user did not
//     order a clip that blares when switching tabs;
//   * before the first frame and after stopping, the still frame is
//     visible: it lies UNDER the video and never goes away, so a black
//     rectangle has no chance to flash;
//   * we always stand still when nobody is watching — another tab, another
//     sub-tab, a minimized window, a full-screen show. This is not a
//     trifle, it is the battery.
//
// The decision "what is playing now" is entirely in Rust (`previewSource`);
// the markup tells it exactly one fact — whether the card is visible to
// the user.
//
// Warning: the player itself is brought up LAZILY, behind a `Loader`: the
// Qt multimedia layer costs ~117 MB, and someone who came to look at
// profile backgrounds should not pay for it. In the original this was done
// by `_sv_ensure_player`.

// Delegates refer to ids of outer elements (root, page). Without this
// pragma, QML lint rightly complains about unqualified access.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
// Top-level window (minimized / hidden) and the preview player.
import QtQuick.Window
import QtMultimedia
import org.rexon.gui 1.0

Item {
    id: root

    // ── Bridge to the outside ─────────────────────────────────────────────

    property var actions: null

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    // Exposed for the self-test: the host checks that the page knows the same
    // Steam root that the actions object finds. An empty path with a found
    // root means the profile backgrounds section will stay empty, and the
    // tab's look would not reveal it.
    readonly property string steamFoundPath: page.headerFoundPath

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    SteamModel {
        id: page
        // The name is needed by checks: the model is fetched with
        // `root.findChild(QObject, "steamModel")` and queried without a single
        // change in the page itself.
        objectName: "steamModel"

        // The message arrives as a property, while showing it is an ACTION: a
        // banner bound to a property cannot fade out by definition. That is
        // exactly why the message hung forever while `visible: statusText !== ""`
        // stood here.
        onStatusChanged: statusToast.show(page.statusText)
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    // Ready card colour from the core — the same overlay as in the style sheet
    // of the widget showcase (`qt_steam::card_colors`). See ComparePage.
    readonly property color colCard: root.pal.surface_card
    readonly property color colBorder: root.pal.border
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // Page margin. It used to be a number in `anchors.margins`; now the scroll
    // area also knows it and computes the content width and height from it —
    // two numbers in two places would silently drift apart.
    readonly property int pageMargin: 24

    // ── Tab styling that is not in the skin palette ───────────────────────
    //
    // Group section, showcase card, thesis inset, primary and green buttons,
    // toggle chip, thumbnail backings. Everything comes ready from the core —
    // the same set that paints the widget tab.
    readonly property var st: page.styles
    readonly property var toneChip: ({
        "background": "transparent",
        "border": root.st.chip.border,
        "hover": root.st.chip.hoverBackground,
        "radius": root.st.chip.radius
    })
    readonly property var toneChipOn: ({
        "background": root.st.chip.checkedBackground,
        "border": root.st.chip.checkedBorder,
        "hover": root.st.chip.checkedBackground,
        "radius": root.st.chip.radius
    })

    // ── Seam with background tasks ────────────────────────────────────────
    //
    // Tokio tasks put their results into an event queue; the page picks it up
    // with its own timer. This is the ONLY place where data crosses the thread
    // boundary — there are no signals from the background thread into QML at
    // all.
    Timer {
        id: pump
        interval: 120
        repeat: true
        running: true
        onTriggered: page.pump()
    }

    // ── Live preview: is the card visible to the user ─────────────────────
    //
    // Polling once per second — the same approach and the same period as in
    // the original. Qt has no "the surface was hidden" signal at all, and an
    // event filter on the main window would cost more: EVERY mouse movement
    // across the whole program goes through it.
    readonly property var hostWindow: root.Window.window
    /// The compositor shows our window (the answer of `QWindow::isExposed()`).
    property bool windowExposed: true
    /// The player has been needed at least once.
    ///
    /// Warning: the Qt multimedia layer (FFmpeg with codecs) costs ~117 MB and
    /// is brought up by a single created `MediaPlayer`. Someone who came to
    /// look at profile backgrounds should not pay for it — in the original the
    /// player was built lazily for the same reason (`_sv_ensure_player`). The
    /// latch never resets: the layer, once up, is already in memory, and
    /// recreating the player every time the cursor moves away is wasted work.
    property bool previewNeeded: false

    Binding {
        target: root
        property: "previewNeeded"
        value: true
        when: page.previewSource !== ""
        // Do NOT roll back: a latch is a latch.
        restoreMode: Binding.RestoreNone
    }

    Timer {
        id: previewWatch
        interval: 1000
        repeat: true
        // The page was hidden — there is nothing to poll: stopping the preview is
        // done by `onVisibleChanged`, and there is no reason to wake the CPU once
        // a second for a known negative answer (in the original the same timer
        // was stopped in `hideEvent`).
        running: root.visible
        onTriggered: root.syncPreview()
    }

    // Focus returning to the window is the only thing that arrives as a
    // signal: it also lifts the pause after a full-screen show.
    Connections {
        target: root.hostWindow
        function onActiveChanged() {
            if (root.hostWindow && root.hostWindow.active)
                page.resumePreview();
            root.syncPreview();
        }
    }

    function syncPreview() {
        root.windowExposed = root.hostWindow ? page.windowExposed(root.hostWindow) : true;
        page.setPreviewShown(root.previewAllowed());
        page.previewTick();
    }

    /// Whether anyone is looking at the large card right now.
    ///
    /// Warning: on Wayland a minimized window CANNOT be detected through
    /// `visibility`: the xdg-shell protocol does not tell the client about the
    /// "minimized" state, and after minimizing the window stays
    /// `Window.Windowed` under KWin. So an honest indicator from the core —
    /// `windowExposed` — is added to the visibility check. On X11 both work.
    function previewAllowed() {
        if (!root.visible || !heroBox.visible)
            return false;
        // The large view is open — the card under it is not visible, and there
        // must not be two copies of the same clip at once.
        if (viewer.opened)
            return false;
        const win = root.hostWindow;
        if (win && (!win.visible
                    || win.visibility === Window.Minimized
                    || win.visibility === Window.Hidden))
            return false;
        if (!root.windowExposed)
            return false;
        // The card was scrolled past the strip's edge — nobody can look at it.
        // The original could not do this at all and kept playing the clip of a
        // card scrolled past the top of the window (rule #16). The delay is up to
        // a second, exactly the same as for the other checks of the situation.
        if (splashScroll.height > 0) {
            const top = heroBox.mapToItem(splashScroll, 0, 0).y;
            if (top + heroBox.height <= 0 || top >= splashScroll.height)
                return false;
        }
        return true;
    }

    // Moved to another tab of the window — the player stops; came back — it
    // plays again and the page is keyboard-operable again.
    onVisibleChanged: {
        if (root.visible)
            root.focusFirstControl();
        root.syncPreview();
    }

    // The cursor lingered on a card — that one is shown large (like an item's
    // clip in a store). The delay is set by the core.
    Timer {
        id: hoverSettle
        interval: page.svHoverDelayMs
        repeat: false
        onTriggered: page.setHovered(root.pendingHover)
    }
    property string pendingHover: ""

    function hoverLater(path) {
        root.pendingHover = path;
        hoverSettle.restart();
    }

    // ── Large view ────────────────────────────────────────────────────────
    //
    // The layer is shared across the whole product (`RexonImageViewer.qml`);
    // the tab hands it the list and the card index. Paging goes over the WHOLE
    // filtered list, not over a single grid page.

    function openViewer(items, key) {
        if (!items || items.length === 0)
            return;
        let at = 0;
        for (let i = 0; i < items.length; i++) {
            if (items[i].key === key) {
                at = i;
                break;
            }
        }
        viewer.show(items, at);
    }

    function openBackgroundViewer(id) {
        root.openViewer(page.bgViewerItems(), id);
    }

    function openMovieViewer(path) {
        root.openViewer(page.svViewerItems(), path);
    }

    // ── Side actions ──────────────────────────────────────────────────────

    function reload() {
        if (!root.actions)
            return;
        const snapshot = root.actions.scan();
        if (!snapshot)
            return;
        page.setMovies(snapshot.movies, snapshot.current, snapshot.favorites);
        page.setSplashState(snapshot.enabled, snapshot.rotate, snapshot.sound);
    }

    function pickOwnFile() {
        if (!root.actions)
            return;
        const path = root.actions.pickFile();
        if (!path)
            return;
        root.actions.select(path);
        root.reload();
    }

    function toggleStar(path) {
        if (!root.actions || !path)
            return;
        page.setFavorites(root.actions.setFavorite(path));
    }

    function playFullscreen() {
        if (!root.actions)
            return;
        const path = page.shownPath();
        if (!path) {
            page.setStatus(page.svTestNoItemText);
            return;
        }
        const err = root.actions.play(path, page.soundOn);
        if (!err) {
            // The clip went full screen — the preview in the card freezes: there must
            // not be two copies of the same video at once. It is lifted by itself as
            // soon as the window gets focus again; a time margin is held by the core —
            // in case focus never returns.
            page.pausePreview();
        }
        // The result of a show used to vanish silently: success — invisible,
        // failure — too (rule #16). The original wrote it as a line under the
        // large card; here it goes to the shared status line.
        page.setStatus(err ? err : page.svTestPlayingText);
    }

    Component.onCompleted: {
        root.reload();
        // The page does not carry the owned backgrounds token: it is obtained by
        // the model's background task (going to the network is not a job for the
        // interface thread).
        page.start("");
        root.focusFirstControl();
        root.syncPreview();
        root.takeSubpage();
    }

    Component.onDestruction: page.shutdown()

    // ── Sub-tab opened from outside ───────────────────────────────────────
    //
    // `--section=steam/screensaver` (and the window door
    // `openSection("steam", key)`) bring the sub-tab key as the window's
    // sub-page (`startSubpage`). A foreign key is left alone — the section it
    // belongs to will take it.
    function takeSubpage() {
        const w = root.Window.window;
        if (!w || w.startSubpage === undefined || w.startSubpage === "")
            return;
        const key = String(w.startSubpage);
        if (!(page.segments || []).some(s => s.key === key))
            return;
        w.startSubpage = "";
        page.setSection(key);
    }

    Connections {
        target: root.Window.window
        ignoreUnknownSignals: true
        function onStartSubpageChanged() { root.takeSubpage(); }
    }

    // Focus on the first frame goes to the FIRST OPERABLE ELEMENT, not to the
    // page root: `forceActiveFocus()` on the root makes activeFocusItem a
    // nameless Item, and the page is dead both from the keyboard and from a
    // gamepad.
    function focusFirstControl() {
        if (segments.count > 0 && segments.itemAt(0)) {
            segments.itemAt(0).forceActiveFocus();
            return;
        }
        Qt.callLater(root.focusFirstControl);
    }

    // ── Markup ────────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── The whole page is in a scroll area ────────────────────────────────
    //
    // Warning: the original is built the same way: the whole page lies in a
    // `QScrollArea` (`rexon/qt_steam/window.py:797`), and both sections inside
    // are in their own (ibid., 842 and 850). The two inner ones were here, but
    // the outer one was not, and the page lost everything that did not fit
    // into the window:
    //
    //   * the long path to Steam in the header neither shrinks nor elides (in
    //     the original it is one `QLabel` with `<tt>`), so it made the page
    //     WIDER than the window — and the right edge of the section together
    //     with the "Own file…" button slid irretrievably past the edge;
    //   * at the main window's own minimum (1100×750) the page gets less
    //     height than its own minimum, and the bottom of the section together
    //     with the bottom margin was cut off.
    //
    // Now both are reachable by scrolling, and while everything fits no bars
    // are visible and the page looks exactly as before.
    //
    // Never: ONLY the page content scrolls. The status banner (`RexonToast`
    // below) stays outside: it is an overlay ON TOP of the page, and inside
    // the strip it would ride along with the content and slide past the edge.
    ScrollView {
        id: pageScroll
        // The name is for test bench runs: the strip is fetched with
        // `root.findChild(QObject, "pageScroll")` and scrolled in order to capture
        // what is beyond the window edge, as a human does by hand. Test benches
        // fetch the model itself the same way (`steamModel` above).
        objectName: "pageScroll"
        anchors.fill: parent
        clip: true
        // The width is set by the most incompressible thing on the page — the
        // header row with the path. The `implicitWidth` of the whole column cannot
        // be used: for the section's wrapping labels it equals the length of the
        // text ON ONE LINE, and the page would always be enormously wide.
        //
        // Warning: the measurement itself is a loop: `contentWidth` asks the
        // header and the segments for their width, and their width is assigned
        // through `pageColumn` by the same `contentWidth`. The values converge on
        // the first pass (`Math.max` only grows, the path does not depend on the
        // window width), but an ordinary binding cannot know that: while it is
        // being evaluated, reading `implicitWidth` triggers a layout recompute,
        // which reaches back for `contentWidth` — and QML rightly prints "Binding
        // loop detected", 32 lines on every entry into the tab.
        //
        // `delayed` is the standard answer to this: the value is computed once per
        // event loop turn, after the layout has settled, and no loop arises at
        // all. Verified on a separate test bench: 0 warnings versus 32, the content
        // width is the same both with a narrow window (the long path is likewise
        // reached by scrolling) and when stretching the window it follows it.
        // `delayed` must not be removed without removing the measurement itself —
        // the loop would come back.
        //
        // Warning: the lower bound is `availableWidth`, not `width`. The vertical
        // bar (always visible when the window is lower than the tab minimum) eats
        // into the area, and content as wide as the full `width` came out wider
        // than the visible part by the bar's thickness: a permanent, pointless
        // horizontal scroll bar hung at the bottom (screenshots 01.10.2026).
        Binding on contentWidth {
            delayed: true
            value: Math.max(pageScroll.availableWidth,
                            pageSegments.implicitWidth + root.pageMargin * 2)
        }
        // The page does not shrink below the tab's own minimum — as in the
        // original, where it is held by the content's `minimumSizeHint`.
        //
        // Warning: also delayed, and for the same reason, but the loop here is
        // someone else's: in the KDE style (`org/kde/desktop/ScrollView.qml:20`)
        // the area's paddings depend on whether the scroll bar is visible, and
        // whether it is visible is decided by comparing the content with the
        // available room, i.e. with the same paddings. Live (the KDE style, unlike
        // the test bench on Basic) this gave "Binding loop … implicitHeight" even
        // after the width was untangled.
        Binding on contentHeight {
            delayed: true
            value: Math.max(pageScroll.height, page.windowMinHeight)
        }

        ColumnLayout {
            id: pageColumn
            // The strip's content width is `contentWidth`: the width of `contentItem`,
            // to which the edges are attached, equals it.
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: root.pageMargin
            y: root.pageMargin
            height: pageScroll.contentHeight - root.pageMargin * 2
            spacing: 14

            // ── Header ────────────────────────────────────────────────────────
            RowLayout {
                id: pageHeader
                Layout.fillWidth: true
                spacing: 12

                RexonText {
                    text: page.headerTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                }
                Item { Layout.fillWidth: true }
                // "Found: " and the path are one label in two parts: in the original it is
                // one QLabel with `<tt>` around the path, so there must be neither bold
                // nor a gap here.
                //
                // Warning: unlike the original, the path IS SHORTENED: if it does not fit,
                // there is an ellipsis in the middle and the full path is in the tooltip.
                // An incompressible path made the page wider than the window (screenshots
                // of the live check r1434, 16.10.2026: with a long path "Retry" went past
                // the right edge, descriptions were cut, and a horizontal scroll bar hung
                // at the bottom). So the header no longer takes part in the page width
                // measurement (`contentWidth` above) either.
                RowLayout {
                    // No gap between "Found: " and the path — as in the original.
                    spacing: 0
                    // Shrinks to zero and grows no further than its own length: all the extra
                    // room is taken by the spacer on the left, and the path stays pressed to
                    // the right edge, as before.
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.maximumWidth: implicitWidth

                    RexonText {
                        text: page.headerFoundText
                        color: root.colTextSecondary
                    }
                    RexonText {
                        id: headerPath
                        objectName: "headerFoundPath"
                        visible: page.headerFoundPath !== ""
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.maximumWidth: implicitWidth
                        text: page.headerFoundPath
                        color: root.colTextSecondary
                        font.family: "monospace"
                        elide: Text.ElideMiddle

                        HoverHandler { id: headerPathHover }
                        ToolTip.text: page.headerFoundPath
                        ToolTip.visible: headerPath.truncated && headerPathHover.hovered
                        ToolTip.delay: 500
                    }
                }
            }

            // ── Segmented sub-tab switch ──────────────────────────────────────
            RowLayout {
                id: pageSegments
                Layout.fillWidth: true
                spacing: 4

                Repeater {
                    id: segments
                    model: page.segments
                    delegate: RexonButton {
                        required property var modelData
                        pal: root.pal
                        tone: root.toneChip
                        checkedTone: root.toneChipOn
                        text: modelData.label
                        checkable: true
                        checked: modelData.checked
                        onClicked: page.setSection(modelData.key)
                    }
                }
                Item { Layout.fillWidth: true }
            }

            // Switching hides the inactive section ENTIRELY — exactly the approach
            // that fixed a freeze of up to ~2 s on switching in the widget tab.
            StackLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                currentIndex: page.section === "bg" ? 1 : page.section === "showcase" ? 2
                              : page.section === "screensaver" ? 3 : 0

                // ══ "🎬 Video splash" section ═════════════════════════════════
                ScrollView {
                    id: splashScroll
                    clip: true
                    contentWidth: availableWidth

                    // Group section: the same bordered card as in the widget tab
                    // (steamSplashGroup). Colours and paddings come from the core.
                    Rectangle {
                        width: parent.width
                        // Never: the scroll area takes the content height from the
                        // `implicitHeight` of its only child, not from `height`. Without it
                        // `contentHeight` stayed -1 and the bar considered the content equal to
                        // the window: the grid went past the bottom edge without a scroll handle,
                        // and the wheel did not scroll (live r1439, 37 animated backgrounds,
                        // 16.10.2026).
                        implicitHeight: splashCol.implicitHeight + root.st.group.marginV * 2
                        // The group takes the whole strip even when there is less content: in the
                        // original it is the scroll area's widget with `setWidgetResizable(True)`
                        // and a stretch at the end.
                        height: Math.max(implicitHeight, splashScroll.availableHeight)
                        color: root.st.group.background
                        border.color: root.st.group.border
                        border.width: 1
                        radius: root.st.group.radius

                        ColumnLayout {
                            id: splashCol
                            anchors.fill: parent
                            anchors.leftMargin: root.st.group.marginH
                            anchors.rightMargin: root.st.group.marginH
                            anchors.topMargin: root.st.group.marginV
                            anchors.bottomMargin: root.st.group.marginV
                            spacing: root.st.group.spacing

                            RexonText {
                                Layout.fillWidth: true
                                text: page.splashGroupTitle
                                color: root.colText
                                font.pointSize: page.groupTitlePointSize
                                font.bold: true
                            }

                            // The main thesis — an accent inset.
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: pitch.implicitHeight
                                                + root.st.pitch.marginV * 2
                                color: root.st.pitch.background
                                radius: root.st.pitch.radius
                                border.width: 0
                                Rectangle {
                                    width: root.st.pitch.barWidth
                                    height: parent.height
                                    color: root.st.pitch.bar
                                }
                                RexonText {
                                    id: pitch
                                    anchors.fill: parent
                                    anchors.margins: root.st.pitch.marginV
                                    anchors.leftMargin: root.st.pitch.marginH
                                    anchors.rightMargin: root.st.pitch.marginH
                                    text: page.splashPitch
                                    color: root.colText
                                    wrapMode: Text.WordWrap
                                }
                            }

                            // ── Large card of the selected clip ───────────────────
                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: heroRow.implicitHeight + 28
                                color: root.st.card.background
                                radius: root.st.card.radius
                                border.color: heroHover.hovered ? root.st.card.hover
                                                                : root.st.card.border
                                border.width: 1
                                HoverHandler { id: heroHover }

                                RowLayout {
                                    id: heroRow
                                    anchors.fill: parent
                                    anchors.margins: 14
                                    spacing: 16

                                    Rectangle {
                                        id: heroBox
                                        Layout.alignment: Qt.AlignTop
                                        implicitWidth: page.svHeroWidth
                                        implicitHeight: page.svHeroHeight
                                        radius: root.st.thumb.heroRadius
                                        color: root.st.thumb.heroBackground
                                        clip: true

                                        // The still frame lies at the BOTTOM and is never removed. While there is
                                        // no first clip frame, it is visible; once frames flow, the video covers
                                        // it; when the player stops, it is visible again. Neither a switch nor a
                                        // "what is shown now" state is needed for this, and a black rectangle (the
                                        // file takes ~0.2 s to open) has no chance to flash.
                                        Image {
                                            anchors.fill: parent
                                            source: page.hero.poster
                                            fillMode: Image.PreserveAspectFit
                                            asynchronous: true
                                            visible: source !== ""
                                        }
                                        RexonText {
                                            anchors.centerIn: parent
                                            visible: page.hero.poster === ""
                                            text: page.svThumbPlaceholder
                                            font.pixelSize: 40
                                        }

                                        // The player appears only when it is first needed (see
                                        // `root.previewNeeded`).
                                        Loader {
                                            anchors.fill: parent
                                            active: root.previewNeeded
                                            sourceComponent: previewPlayer
                                        }

                                        Component {
                                            id: previewPlayer

                                            Item {
                                                VideoOutput {
                                                    id: previewOut
                                                    anchors.fill: parent
                                                    fillMode: VideoOutput.PreserveAspectFit
                                                    // The player stopped (stop, pause, end of file) — back to the still frame.
                                                    // A port of the original's `_sv_on_state`; also a safeguard against empty
                                                    // output while there is no first frame yet.
                                                    visible: preview.playbackState
                                                             === MediaPlayer.PlayingState
                                                }

                                                // Warning: there is NO audio output at all, and `audioOutput` is not set
                                                // on purpose: a `MediaPlayer` without an assigned output does not open the
                                                // device at all, while with an assigned and muted one the stream hangs in
                                                // the sound card for the whole session. The splash has sound only in the
                                                // full-screen show and at login itself.
                                                MediaPlayer {
                                                    id: preview
                                                    // What to play is decided by the core: hover, sub-tab, visibility, the
                                                    // full-screen show pause and broken clips are already taken into account —
                                                    // here it is just the source.
                                                    source: page.previewSource
                                                    videoOutput: previewOut
                                                    loops: MediaPlayer.Infinite

                                                    onSourceChanged: {
                                                        // There is nothing more to play — stop explicitly rather than wait for the
                                                        // player's mercy: stopping also clears the last frame, and the still one
                                                        // is visible again.
                                                        if (String(preview.source) === "")
                                                            preview.stop();
                                                    }
                                                    onMediaStatusChanged: {
                                                        if (preview.mediaStatus === MediaPlayer.LoadedMedia
                                                                || preview.mediaStatus === MediaPlayer.BufferedMedia)
                                                            preview.play();
                                                    }
                                                    onErrorOccurred: (err, msg) => {
                                                        // The clip cannot be read (a broken file, an unknown codec of your own
                                                        // video) — the card keeps the still frame, and the player is no longer
                                                        // bothered with this file: otherwise the once-a-second poll would open it
                                                        // again and again.
                                                        console.warn("REXON-STEAM превью не играет: "
                                                                     + err + " " + msg);
                                                        page.markPreviewBroken();
                                                    }
                                                }
                                            }
                                        }

                                        // A click on the card plays the same clip over the whole window (muted;
                                        // with sound and as at login — the button next to it).
                                        RexonZoomArea {
                                            anchors.fill: parent
                                            pal: root.pal
                                            radius: root.st.thumb.heroRadius
                                            enabled: page.hero.present
                                            visible: enabled
                                            onActivated: root.openMovieViewer(page.shownPath())
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 7

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: page.hero.title
                                            color: root.colText
                                            font.pointSize: 17
                                            font.bold: true
                                            wrapMode: Text.WordWrap
                                        }
                                        RexonText {
                                            Layout.fillWidth: true
                                            visible: page.hero.present
                                            text: page.hero.source
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        RexonText {
                                            Layout.fillWidth: true
                                            visible: page.hero.labels !== ""
                                            text: page.hero.labels
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        RexonText {
                                            Layout.fillWidth: true
                                            visible: page.hero.event !== ""
                                            text: page.hero.event
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                        Item { Layout.fillHeight: true }

                                        RowLayout {
                                            spacing: 10
                                            RexonButton {
                                                pal: root.pal
                                                tone: root.st.primaryButton
                                                text: page.svTestText
                                                ToolTip.text: page.svTestTooltip
                                                ToolTip.visible: hovered
                                                onClicked: root.playFullscreen()
                                            }
                                            RexonButton {
                                                pal: root.pal
                                                visible: page.hero.present
                                                text: page.hero.starText
                                                ToolTip.text: page.svStarTooltip
                                                ToolTip.visible: hovered
                                                onClicked: root.toggleStar(page.shownPath())
                                            }
                                            Item { Layout.fillWidth: true }
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            visible: page.hero.status !== ""
                                            text: page.hero.status
                                            color: root.colTextSecondary
                                            wrapMode: Text.WordWrap
                                        }
                                    }
                                }
                            }

                            // ── Final state chip ──────────────────────────────────
                            Rectangle {
                                implicitWidth: chipText.implicitWidth + 24
                                implicitHeight: chipText.implicitHeight + 10
                                radius: 12
                                color: page.stateChip.background
                                border.color: page.stateChip.border
                                border.width: 1
                                RexonText {
                                    id: chipText
                                    anchors.centerIn: parent
                                    text: page.stateChip.text
                                    color: page.stateChip.color
                                    font.bold: true
                                }
                            }

                            // ── Toggles ───────────────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 14

                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svAutostartText
                                    checked: page.autostartOn
                                    ToolTip.text: page.svAutostartTooltip
                                    ToolTip.visible: hovered
                                    onToggled: {
                                        // A refusal used to vanish silently: the check box bounced back on
                                        // reload(), and that was it. The text for the customer is composed by the
                                        // core.
                                        if (root.actions)
                                            page.autostartAnswered(checked, root.actions.setAutostart(checked));
                                        root.reload();
                                    }
                                }
                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svRotateText
                                    checked: page.rotateOn
                                    ToolTip.text: page.svRotateTooltip
                                    ToolTip.visible: hovered
                                    onToggled: {
                                        if (root.actions)
                                            root.actions.setRotation(checked);
                                        root.reload();
                                        // After reload(): the threshold is compared with what is on the showcase
                                        // now.
                                        page.rotationToggled(checked);
                                    }
                                }
                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.svSoundText
                                    checked: page.soundOn
                                    onToggled: {
                                        if (root.actions)
                                            root.actions.setSound(checked);
                                        root.reload();
                                    }
                                }
                                Item { Layout.fillWidth: true }
                            }

                            // ── Search and own file ───────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 10

                                RexonTextField {
                                    Layout.fillWidth: true
                                    pal: root.pal
                                    placeholderText: page.svSearchPlaceholder
                                    text: page.svSearchText
                                    onTextEdited: page.setSvSearch(text)
                                }
                                RexonButton {
                                    pal: root.pal
                                    text: page.svPickText
                                    ToolTip.text: page.svPickTooltip
                                    ToolTip.visible: hovered
                                    onClicked: root.pickOwnFile()
                                }
                            }

                            // ── "Where the splash comes from" filter chips ────────
                            // There are never empty buttons: the row is absent altogether when there
                            // is no choice (the core decides).
                            Flow {
                                Layout.fillWidth: true
                                spacing: 4
                                visible: page.svChips.length > 0

                                Repeater {
                                    model: page.svChips
                                    delegate: RexonButton {
                                        required property var modelData
                                        pal: root.pal
                                        tone: root.toneChip
                                        checkedTone: root.toneChipOn
                                        text: modelData.label
                                        checkable: true
                                        checked: modelData.checked
                                        onClicked: page.setSvGroup(modelData.key)
                                    }
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: page.svCountText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // ── Showcase card grid ────────────────────────────────
                            GridView {
                                id: showcase
                                Layout.fillWidth: true
                                Layout.preferredHeight: Math.max(cellHeight,
                                                                 cellHeight * Math.ceil(count / columns))
                                interactive: false
                                // Columns — as many cards as fit; the remaining width is shared among
                                // them, as in the "Appearance" grid: no empty strip on the right.
                                readonly property int columns: Math.max(1, Math.floor(
                                    width / (page.svCardWidth + root.st.card.gap)))
                                cellWidth: Math.max(page.svCardWidth + root.st.card.gap,
                                                    Math.floor(width / columns))
                                cellHeight: page.svThumbHeight + 78
                                model: page

                                delegate: Item {
                                    id: card
                                    required property string path
                                    required property string title
                                    required property string badge
                                    required property string badgeBackground
                                    required property string badgeColor
                                    required property string poster
                                    required property string starGlyph
                                    required property string starColor
                                    required property bool selected

                                    width: showcase.cellWidth
                                    height: showcase.cellHeight

                                    Rectangle {
                                        anchors.fill: parent
                                        anchors.margins: root.st.card.gap / 2
                                        radius: root.st.card.radius
                                        color: card.selected ? root.st.card.selectedBackground
                                                             : root.st.card.background
                                        border.width: 1
                                        border.color: card.selected
                                                      ? (hover.hovered ? root.st.card.selectedHover
                                                                       : root.st.card.selectedBorder)
                                                      : (hover.hovered ? root.st.card.hover
                                                                       : root.st.card.border)

                                        HoverHandler {
                                            id: hover
                                            onHoveredChanged: root.hoverLater(hover.hovered ? card.path : "")
                                        }
                                        TapHandler {
                                            onTapped: {
                                                if (root.actions)
                                                    root.actions.select(card.path);
                                                page.selectMovie(card.path);
                                            }
                                        }

                                        ColumnLayout {
                                            anchors.fill: parent
                                            anchors.margins: root.st.card.margin
                                            spacing: root.st.card.spacing

                                            Rectangle {
                                                Layout.alignment: Qt.AlignHCenter
                                                implicitWidth: page.svThumbWidth
                                                implicitHeight: page.svThumbHeight
                                                radius: root.st.thumb.radius
                                                color: root.st.thumb.svBackground
                                                // The clip frame is FITTED whole, not cropped: in the original the
                                                // thumbnail is read by `load_thumbnail` without `expand`, and a square
                                                // poster sits in the middle with margins on the sides.
                                                Image {
                                                    anchors.fill: parent
                                                    source: card.poster
                                                    fillMode: Image.PreserveAspectFit
                                                    asynchronous: true
                                                    visible: source !== ""
                                                }
                                                RexonText {
                                                    anchors.centerIn: parent
                                                    visible: card.poster === ""
                                                    text: page.svThumbPlaceholder
                                                    font.pixelSize: 22
                                                }
                                                // A click on the card is taken by choosing the clip, so the view is opened
                                                // with the magnifier.
                                                RexonZoomArea {
                                                    anchors.fill: parent
                                                    pal: root.pal
                                                    badgeOnly: true
                                                    radius: root.st.thumb.radius
                                                    onActivated: root.openMovieViewer(card.path)
                                                }
                                            }

                                            RowLayout {
                                                Layout.fillWidth: true
                                                spacing: 4
                                                RexonText {
                                                    // The elision width is set by the core (in the original — by the font
                                                    // metrics, not by the rest of the line): otherwise a long name would push
                                                    // the star apart.
                                                    Layout.fillWidth: true
                                                    Layout.maximumWidth: root.st.svTitleElideWidth
                                                    text: card.title
                                                    color: root.colText
                                                    elide: Text.ElideRight
                                                    ToolTip.text: card.title
                                                    ToolTip.visible: nameHover.hovered
                                                    HoverHandler { id: nameHover }
                                                }
                                                RexonText {
                                                    text: card.starGlyph
                                                    color: card.starColor
                                                    font.pixelSize: 15
                                                    ToolTip.text: page.svCardStarTooltip
                                                    ToolTip.visible: starHover.hovered
                                                    HoverHandler { id: starHover }
                                                    TapHandler {
                                                        onTapped: root.toggleStar(card.path)
                                                    }
                                                }
                                            }

                                            Rectangle {
                                                Layout.fillWidth: false
                                                visible: card.badge !== ""
                                                implicitWidth: Math.min(page.svThumbWidth,
                                                                        badgeText.implicitWidth + 14)
                                                implicitHeight: badgeText.implicitHeight + 2
                                                radius: 7
                                                color: card.badgeBackground
                                                RexonText {
                                                    id: badgeText
                                                    anchors.centerIn: parent
                                                    width: parent.width - 14
                                                    text: card.badge
                                                    color: card.badgeColor
                                                    font.pixelSize: 11
                                                    elide: Text.ElideRight
                                                    ToolTip.text: page.sourceTextFor(card.path)
                                                    ToolTip.visible: badgeHover.hovered
                                                    HoverHandler { id: badgeHover }
                                                }
                                            }
                                            Item { Layout.fillHeight: true }
                                        }
                                    }
                                }
                            }
                            Item { Layout.fillHeight: true }
                        }
                    }
                }

                // ══ "🖼 Steam backgrounds" section ════════════════════════════
                ScrollView {
                    id: bgScroll
                    // The name is for the window scenario (scrolling a long background grid).
                    objectName: "bgScroll"
                    clip: true
                    contentWidth: availableWidth

                    // Group section: the same bordered card as in the widget tab
                    // (steamBgGroup). Colours and paddings come from the core.
                    Rectangle {
                        width: parent.width
                        // Never: the scroll area takes the content height from the
                        // `implicitHeight` of its only child, not from `height`. Without it
                        // `contentHeight` stayed -1 and the bar considered the content equal to
                        // the window: the grid went past the bottom edge without a scroll handle,
                        // and the wheel did not scroll (live r1439, 37 animated backgrounds,
                        // 16.10.2026).
                        implicitHeight: bgCol.implicitHeight + root.st.group.marginV * 2
                        // The group takes the whole strip even when there is less content: in the
                        // original it is the scroll area's widget with `setWidgetResizable(True)`
                        // and a stretch at the end.
                        height: Math.max(implicitHeight, bgScroll.availableHeight)
                        color: root.st.group.background
                        border.color: root.st.group.border
                        border.width: 1
                        radius: root.st.group.radius

                        ColumnLayout {
                            id: bgCol
                            anchors.fill: parent
                            anchors.leftMargin: root.st.group.marginH
                            anchors.rightMargin: root.st.group.marginH
                            anchors.topMargin: root.st.group.marginV
                            anchors.bottomMargin: root.st.group.marginV
                            spacing: root.st.group.spacing

                            RexonText {
                                Layout.fillWidth: true
                                text: page.bgGroupTitle
                                color: root.colText
                                font.pointSize: page.groupTitlePointSize
                                font.bold: true
                            }
                            RexonText {
                                Layout.fillWidth: true
                                text: page.bgGroupDesc
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 12

                                RexonCheckBox {
                                    pal: root.pal
                                    text: page.bgAudioText
                                    checked: page.bgAudioOn
                                    onToggled: page.setBgAudio(checked)
                                }
                                // Background view: all / static / animated, with counters in the labels.
                                // The same chip buttons as in the splash filter.
                                Repeater {
                                    model: page.bgKindChips
                                    delegate: RexonButton {
                                        required property var modelData
                                        objectName: "bgKindChip_" + modelData.key
                                        pal: root.pal
                                        tone: root.toneChip
                                        checkedTone: root.toneChipOn
                                        text: modelData.label
                                        checkable: true
                                        checked: modelData.checked
                                        onClicked: page.setBgKind(modelData.key)
                                    }
                                }
                                RexonTextField {
                                    Layout.fillWidth: true
                                    pal: root.pal
                                    placeholderText: page.bgSearchPlaceholder
                                    text: page.bgSearchText
                                    onTextEdited: page.setBgSearch(text)
                                }
                            }

                            // Why there are no animated backgrounds and what to do: without this line
                            // "Animated (0)" stays silent, and the user does not know the issue is the
                            // Steam login.
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 12
                                visible: page.bgAnimatedNote !== ""

                                RexonText {
                                    objectName: "bgAnimatedNote"
                                    Layout.fillWidth: true
                                    text: page.bgAnimatedNote
                                    color: root.colText
                                    wrapMode: Text.WordWrap
                                }
                                RexonButton {
                                    objectName: "bgAnimatedRetry"
                                    pal: root.pal
                                    text: page.bgRetryText
                                    onClicked: page.reloadBackgrounds()
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: page.bgStatusText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // Cards of ONLY the current page (~25 out of hundreds) — the key technique
                            // against lag when opening the sub-tab.
                            //
                            // The columns are set by the core (`pager.cols`), and the page is cut by
                            // them too — rows are always complete. Previously a `Flow` laid out the
                            // cards by itself: in a 1920 window it put nine in a row, while the page
                            // was 8 × 5 = 40, and the last row came out with four. The remaining width
                            // is shared among the columns, as in the "Appearance" grid — no empty strip
                            // on the right.
                            Item {
                                id: bgGridBox
                                Layout.fillWidth: true
                                implicitHeight: bgGrid.implicitHeight
                                // The width of the grid itself, not of the window: it is also changed by
                                // the vertical scroll handle appearing, and the window sends no event
                                // about that.
                                onWidthChanged: page.setBgViewportWidth(String(Math.round(width)))
                                Component.onCompleted: page.setBgViewportWidth(String(Math.round(width)))

                                readonly property int cols: Math.max(1, page.pager.cols || 1)
                                readonly property int cardWidth: Math.max(page.bgCardWidth,
                                    Math.floor((width - (cols - 1) * root.st.card.gap) / cols))

                                Grid {
                                    id: bgGrid
                                    objectName: "bgGrid"
                                    width: parent.width
                                    columns: bgGridBox.cols
                                    columnSpacing: root.st.card.gap
                                    rowSpacing: root.st.card.gap

                                    Repeater {
                                        model: page.bgCards
                                        delegate: Rectangle {
                                            id: bgCard
                                            required property var modelData

                                            width: bgGridBox.cardWidth
                                            implicitHeight: bgCardCol.implicitHeight
                                                            + root.st.card.margin * 2
                                            radius: root.st.card.radius
                                            color: root.st.card.background
                                            border.width: 1
                                            border.color: bgHover.hovered ? root.st.card.hover
                                                                          : root.st.card.border
                                            HoverHandler { id: bgHover }

                                            // Warning: its own name, not shared with the SECTION's column: both were
                                            // called `bgCol`, and inside the tile delegate the section's name was
                                            // shadowed by its namesake. Any new reference to the section's column
                                            // from the tile would silently bind to the tile's own column.
                                            ColumnLayout {
                                                id: bgCardCol
                                                anchors.fill: parent
                                                anchors.margins: root.st.card.margin
                                                spacing: root.st.card.spacing

                                                Rectangle {
                                                    Layout.alignment: Qt.AlignHCenter
                                                    implicitWidth: page.bgThumbWidth
                                                    implicitHeight: page.bgThumbHeight
                                                    radius: root.st.thumb.radius
                                                    color: root.st.thumb.bgBackground
                                                    ToolTip.text: bgCard.modelData.thumbTooltip
                                                    ToolTip.visible: thumbHover.hovered
                                                                     && bgCard.modelData.thumbTooltip !== ""
                                                    HoverHandler { id: thumbHover }
                                                    Image {
                                                        anchors.fill: parent
                                                        source: bgCard.modelData.thumb
                                                        fillMode: Image.PreserveAspectCrop
                                                        asynchronous: true
                                                        visible: source !== ""
                                                    }
                                                    RexonText {
                                                        anchors.centerIn: parent
                                                        visible: bgCard.modelData.thumb === ""
                                                        text: bgCard.modelData.icon
                                                        font.pixelSize: 22
                                                    }
                                                    // Badge of an animated background: a static one has none at all.
                                                    Rectangle {
                                                        objectName: "bgAnimatedMark"
                                                        visible: bgCard.modelData.mark !== ""
                                                        anchors.left: parent.left
                                                        anchors.bottom: parent.bottom
                                                        anchors.margins: 4
                                                        width: bgMarkText.implicitWidth + 10
                                                        height: bgMarkText.implicitHeight + 4
                                                        radius: 4
                                                        color: "#cc000000"
                                                        RexonText {
                                                            id: bgMarkText
                                                            anchors.centerIn: parent
                                                            text: bgCard.modelData.mark
                                                            font.pixelSize: 15
                                                        }
                                                    }
                                                    RexonZoomArea {
                                                        anchors.fill: parent
                                                        pal: root.pal
                                                        radius: root.st.thumb.radius
                                                        // Only an animated background has its own tooltip — that one stays,
                                                        // without a second one on top.
                                                        tooltip: bgCard.modelData.thumbTooltip !== ""
                                                                 ? "" : qsTranslate("rexon", "Open large view")
                                                        onActivated: root.openBackgroundViewer(bgCard.modelData.id)
                                                    }
                                                }

                                                // Icon and name as two labels, not one: in the original ONLY the name is
                                                // elided (to the thumbnail width), while the icon stands before the
                                                // elision and is always visible.
                                                RowLayout {
                                                    Layout.fillWidth: true
                                                    spacing: 0
                                                    RexonText {
                                                        text: bgCard.modelData.icon + " "
                                                        color: root.colText
                                                    }
                                                    RexonText {
                                                        Layout.fillWidth: true
                                                        Layout.maximumWidth: page.bgThumbWidth
                                                        text: bgCard.modelData.name
                                                        color: root.colText
                                                        elide: Text.ElideRight
                                                        ToolTip.text: bgCard.modelData.name
                                                        ToolTip.visible: bgNameHover.hovered
                                                        HoverHandler { id: bgNameHover }
                                                    }
                                                }

                                                // An empty line is drawn anyway: otherwise the cards of a row end up with
                                                // different heights.
                                                RexonText {
                                                    Layout.fillWidth: true
                                                    Layout.maximumWidth: page.bgThumbWidth
                                                    text: bgCard.modelData.sub
                                                    color: root.colTextSecondary
                                                    elide: Text.ElideRight
                                                }

                                                RexonButton {
                                                    Layout.fillWidth: true
                                                    pal: root.pal
                                                    // Blue "Apply", green "✓ Applied": which background is set now is visible
                                                    // without reading the label.
                                                    tone: bgCard.modelData.applied
                                                          ? root.st.appliedButton
                                                          : root.st.primaryButton
                                                    text: bgCard.modelData.applied ? page.bgAppliedText
                                                                                   : page.bgApplyText
                                                    onClicked: page.applyBackground(bgCard.modelData.id)
                                                }
                                            }
                                        }
                                    }
                                }

                            }

                            // ── Pagination ────────────────────────────────────────
                            RowLayout {
                                Layout.fillWidth: true
                                visible: page.pager.visible
                                spacing: 10

                                Item { Layout.fillWidth: true }
                                // "‹ Prev" and "Next ›": the glyph is mirrored in Unicode, and Qt draws a
                                // right-to-left string with it flipped by itself — a translation into
                                // Arabic or Hebrew needs nothing. An untranslated (Latin) string would
                                // stay left to right, and the "‹" on a button moved to the right would
                                // point inwards; the RLM mark at the start makes it a right-to-left
                                // string — the glyph moves to the start of the line and points outwards.
                                // Swapping "‹" for "›" by hand is not allowed: in a translated string Qt
                                // would flip it a second time.
                                RexonButton {
                                    pal: root.pal
                                    text: (root.LayoutMirroring.enabled ? "\u200f" : "") + page.bgPrevText
                                    enabled: page.pager.prevEnabled
                                    onClicked: page.bgPrevPage()
                                }
                                RexonText {
                                    text: page.pager.text
                                    color: root.colText
                                    horizontalAlignment: Text.AlignHCenter
                                }
                                RexonButton {
                                    pal: root.pal
                                    text: (root.LayoutMirroring.enabled ? "\u200f" : "") + page.bgNextText
                                    enabled: page.pager.nextEnabled
                                    onClicked: page.bgNextPage()
                                }
                                Item { Layout.fillWidth: true }
                            }
                            Item { Layout.fillHeight: true }
                        }
                    }
                }

                // ══ "🖼 Profile showcases" section ════════════════════════════
                // Cutting your own picture for a profile showcase, and a preview. The
                // section is a separate component with its own actions object
                // (`SteamShowcaseSection.qml`); the tab gives it the palette, the styling
                // and the list of profile backgrounds.
                ScrollView {
                    id: showcaseScroll
                    clip: true
                    contentWidth: availableWidth

                    SteamShowcaseSection {
                        width: showcaseScroll.availableWidth
                        pal: root.pal
                        sty: root.st
                        warnColor: page.goldColor
                        backgrounds: page.bgChoices
                        onMessage: text => statusToast.show(text)
                        onZoomRequested: (items, index) => viewer.show(items, index)
                    }
                }

                // ══ "🌙 Screensaver" section ══════════════════════════════════
                // The Rexon screensaver for the Steam Big Picture idle screen — a separate
                // component with its own actions object (`SteamScreensaverSection.qml`).
                ScrollView {
                    id: screensaverScroll
                    clip: true
                    contentWidth: availableWidth

                    SteamScreensaverSection {
                        width: screensaverScroll.availableWidth
                        pal: root.pal
                        sty: root.st
                        warnColor: page.goldColor
                        onMessage: text => statusToast.show(text)
                    }
                }
            }
        }
    }

    // ── Large view of pictures and clips ──────────────────────────────────
    //
    // One layer for both grids of the tab. A full-size background file that is
    // not on disk yet is requested by the viewer through `wanted` and received
    // through `sources` — it is downloaded by the same loading layer as the
    // thumbnails.
    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
        sources: page.bgViewerSources
        onWanted: key => page.requestBgFull(key)
        onOpened: root.syncPreview()
        onClosed: root.syncPreview()
    }

    // ── Status line ───────────────────────────────────────────────────────
    //
    // An overlay, not a place in the layout: it reserves no height and does
    // not shift the showcase when there is no message.
    //
    // Warning: the banner is shared across the whole product (`RexonToast.qml`),
    // and the hide timer lives in it. Our own one stood here:
    // `visible: statusText !== ""` and not a single timer — the message hung
    // until the tab was closed (the live check on 18.09 caught it after 29
    // minutes). Our own numbers went away too (radius 6, paddings 28×16,
    // margin 18) along with our own centred placement: both come from the
    // core, and the banner moved to the bottom left — where the widget one
    // sits.
    RexonToast {
        id: statusToast
        skin: root.st.toast
        timeoutMs: page.toastTimeoutMs
        metrics: page
        availableWidth: root.width
        // The tab's messages carry background names and paths, not markup:
        // parsing them as markup would break an "&" in a title.
        textFormat: Text.PlainText

        // The initial message: there will be no signal about it — it has already
        // passed.
        Component.onCompleted: statusToast.show(page.statusText)
    }
}
