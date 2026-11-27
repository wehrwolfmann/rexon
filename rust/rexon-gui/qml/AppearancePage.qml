// The «Appearance» tab in QML. Data and decisions come from the Rust
// object AppearanceModel (rust/rexon-gui/src/appearance_page.rs), which
// decides nothing itself but asks the core (`rexon_core::appearance_page`).
// Only MARKUP lives here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from the default
// skin of the whole product (rule #17). The page defines no colours of its
// own — even the tile button colours arrive from the core
// (`page.buttonColors(state)`).
//
// ── Side actions ─────────────────────────────────────────────────────────
//
// Network and disk live outside: the page host puts into `root.actions` an
// object with the methods
//   environment()             -> JSON string: {qtStyles, widgetStyle,
//                                aurHelper, detectedDe, savedDe, seenDe}
//   loadCatalog(requestJson)  -> void (a long job, the answer arrives with
//                                the catalogReady signal; the `fresh` field
//                                means "bypass your disk scan cache")
//   loadPreview(job, itemId, url, low)
//                             -> void (one picture; the answer arrives with
//                                the previewDone/previewFailed signals with
//                                the same `job`. It must be written where
//                                the page looks for cache hits — the path is
//                                computed by the core,
//                                `appearance_page::preview_cache_path`)
//   cancelPreviews()          -> void (abort everything in flight for
//                                pictures)
//   loadPrefetch(requestJson) -> void (background cache warm-up: nothing to
//                                show, the answer arrives with prefetchReady)
//   remember(deId, seen)      -> bool (remember the desktop choice; an empty
//                                `seen` means "ask the machine yourself which
//                                desktop is running now")
//   loadDetail(itemId)        -> void (full item details, answer by signal)
//   installById(itemId, apply, fileName)-> void (installation, progress by
//                             signals; fileName is a file chosen by hand,
//                             empty means automatic)
//   installAur(pkgName)       -> void (building a package with the helper)
//   actOnStoreItem(itemId)    -> void (store item button: not installed —
//                                install, installed — apply)
//   applyLocalById(itemId)    -> void (apply a theme from disk; for an
//                                environment add-on — enable/disable, for
//                                Conky — start/stop)
//   globalApplyQuestion(itemId)-> JSON string of the "Apply "X"?" window of
//                                a Plasma global theme; empty means no
//                                window, the button does what it did before
//   applyGlobalById(itemId, appearance, layout, parts) — parts: mask of the
//                                    detailed view's parts, -1 — simple view
//                             -> void (apply a global theme with the window
//                                choice; result by the actionDone signal)
//   conkyAutostart(itemId)    -> bool (is Conky widget autostart enabled?)
//   setConkyAutostart(itemId, on) -> bool (toggle; result by actionDone)
//   cursorSize()              -> int (current cursor size, 0 — not set)
//   applyCursorSize(size)     -> void (cursor size; result by actionDone)
//   uninstall(itemJson)       -> void (a store item — by the journal; a theme
//                                from disk `local:…` — like removeLocalById)
//   removeLocalById(itemId)   -> void (mark a theme from disk for removal —
//                                the removalPending signal; refusal —
//                                actionDone)
//   commitRemovals()          -> void (remove the marked; result removalDone)
//   undoRemovals()            -> int  (clear the marks; how many came back)
//   toggleFavorite(itemId)    -> JSON string: the favourites after it
//   openUrl(url)              -> bool (item page in the browser)
//   openLinkedCard(requestJson)-> void (a link to a store item from
//                                third-party text: `{card, url}`; answer by
//                                the linkedCardReady signal)
//   purgeTranslations()       -> void (one-off cleanup of stale translations)
// and the signals
//   catalogReady(payload)     — answer to a catalogue request (JSON string)
//   detailReady(payload)      — full item details (JSON string)
//   descriptionReady(payload) — description translation arriving LATE for an
//                               already shown card (JSON string: `itemId`,
//                               `source` — the translation's source, and
//                               either `translated` or `pending`).
//                               `pending: true` lights the "Translating the
//                               description…" mark (the translation takes
//                               longer than the threshold), `false` clears
//                               it without a translation (we do not
//                               translate or there is no model — the
//                               original stays on the panel); `translated`
//                               puts in the translation and also clears the
//                               mark
//   previewDone(job, path)    — the request's picture is written to the cache
//   previewFailed(job, status)— the request failed: `status` is the server
//                               answer as a number in a string, empty (there
//                               was no answer) or "canceled" (we aborted it)
//   prefetchReady(payload)    — answer to the background warm-up (JSON
//                               string: the same request fields plus `items`)
//   installProgress(pct)      — installation progress, 0–100
//   installLog(text)          — the next line of the installation log
//   statusMessage(text)       — a message for the status line
//   actionDone(text, ok)      — result of installing/applying
//   removalPending(text)      — a theme from disk is marked for removal: the
//                               text of the "Restore" line
//   removalDone(text)         — the marked item is removed: result for the
//                               status line
//   galleryReady(index, path) — a carousel frame is ready
//   galleryFailed(index)      — a frame did not arrive
//   galleryFinished(count)    — the walk over links is finished
//   commentsReady(payload)    — a page of comments (JSON string)
//   commentTranslationsReady(payload)
//                             — comment translations arriving LATE for an
//                               already shown list (JSON string: `itemId`,
//                               `page` and `rows` with "original →
//                               translation" pairs). The answer contains only
//                               what was actually translated: Russian
//                               comments are not mentioned in it, and they
//                               will not get a "show original" link
//   suggestReady(payload)     — search suggestions (JSON string)
//   linkedCardReady(payload)  — the item behind a link: `item` (ThemeItem)
//                               or `aur` (package name); `failed` + `url`,
//                               `status` — opening failed
//
// Everything fast has a synchronous answer; only long jobs have signals
// (trap 18 of the porting scheme). For now these methods are in Python
// (rexon/appearance/ocs_client.py and rexon/appearance/installed_scan.py);
// when the live store executor arrives in Rust, it will take their place
// with the same names — the page will not change.
//
// Warning: the actions object arrives in TWO different ways, and the page
// must work with both: the tab host sets it as an initial property
// (`setInitialProperties`, as in "Steam"), while the test bench host sets
// it as a property AFTER `setSource` (as in "Settings", trap 20). So the
// start-up is gathered into one function `bootstrap()` with a guard, and
// both events call it — `Component.onCompleted` and `onActionsChanged`.
// Whichever comes first brings the page up; the second does nothing.

// Delegates refer to ids of outer elements (root, page). Without this
// pragma, QML lint rightly complains about unqualified access.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Window
import org.rexon.gui 1.0

Item {
    id: root

    // ── Bridge to the outside ─────────────────────────────────────────────

    // Side-actions object. OUR OWN Rust one by default; the page host may
    // substitute another (the Python tab does so with an initial property).
    // The set of methods and signals is described in the file header above.
    property var actions: ownActions

    AppearanceActions {
        id: ownActions
        // The name is needed by checks — the same approach as for the model below.
        objectName: "appearanceActions"
    }

    // "Listen" buttons of sound themes: finding the event file, the player and
    // downloading a Store item before installation belong to the core
    // (`rexon_core::sound_preview`); the object only passes the calls on.
    SoundPreviewActions {
        id: soundActions
        objectName: "soundPreviewActions"
    }

    readonly property string windowTitle: page.windowTitle

    /// Manual horizontal coordinates (the search drop-down, the button menu)
    /// follow the page's mirroring: for right to left they count from the right
    /// edge.
    readonly property RexonArrows arrows: RexonArrows {
        mirrored: root.LayoutMirroring.enabled
    }
    /// Substitutions into labels that the markup assembles itself (an isolate
    /// when writing right to left).
    RexonBidi { id: bidi }

    /// Labels of the "GNOME extensions" section (core:
    /// `gnome_ext_section::page_texts`).
    readonly property var gnomeTexts: {
        try {
            return root.actions && root.actions.gnomeExtTexts
                   ? JSON.parse(root.actions.gnomeExtTexts()) : ({});
        } catch (e) {
            return ({});
        }
    }
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    AppearanceModel {
        id: page
        // The name is needed by checks: the model is fetched with
        // `root.findChild(QObject, "appearanceModel")` and queried without a
        // single change in the page itself.
        objectName: "appearanceModel"

        // The core decided the catalogue contents are stale — go to the host.
        onCatalogWanted: root.askCatalog()

        // An opened item needs full details: the store list gives fewer
        // screenshots than the item has, and the description in it is truncated.
        onDetailWanted: function (itemId) {
            // AUR package: the RPC record and the pictures are a call of their own.
            if (page.detailSource === "aur") {
                if (root.actions && root.actions.loadAurDetail)
                    root.actions.loadAurDetail(itemId);
                return;
            }
            if (root.actions && root.actions.loadDetail)
                root.actions.loadDetail(itemId);
        }

        // A carousel frame is requested by the core — it knows which one is
        // needed.
        onGalleryWanted: function (index, url) {
            if (root.actions && root.actions.loadGalleryFrame)
                root.actions.loadGalleryFrame(index, url);
        }

        onCommentsWanted: function (itemId, commentsPage) {
            // For an AUR package the comments come from the package page on the site.
            if (page.detailSource === "aur") {
                if (root.actions && root.actions.loadAurComments)
                    root.actions.loadAurComments(itemId, commentsPage);
                return;
            }
            if (root.actions && root.actions.loadComments)
                root.actions.loadComments(itemId, commentsPage);
        }

        onSuggestWanted: function (request) {
            if (root.actions && root.actions.loadSuggestions)
                root.actions.loadSuggestions(request);
        }

        // ── Pictures ─────────────────────────────────────────────────────
        //
        // What to download, how many at once and in what order is decided by the
        // core queue. The markup only passes the request to the host and returns
        // the answer back under the same number.
        onPreviewWanted: function (job, itemId, url, low) {
            if (root.actions && root.actions.loadPreview)
                root.actions.loadPreview(job, itemId, url, low);
        }

        // The failure was temporary — the core named a pause before the retry.
        onPreviewRetryWanted: function (generation, job, delayMs) {
            root.schedulePreviewRetry(generation, job, delayMs);
        }

        // The section changed: the pictures in flight for the previous one are no
        // longer relevant, and the new one needs the channel.
        onPreviewsCanceled: {
            if (root.actions && root.actions.cancelPreviews)
                root.actions.cancelPreviews();
        }

        // Background cache warm-up: neighbouring sections and the next page.
        // There is nothing to show for it — it only fills the cache.
        onPrefetchWanted: function (request) {
            if (root.actions && root.actions.loadPrefetch)
                root.actions.loadPrefetch(request);
        }
    }

    // Warming up the other sections starts LATER than the first load: it must
    // not compete for the network with the section that is open.
    Timer {
        interval: page.prefetchStartMs
        repeat: false
        running: root.started
        onTriggered: page.startCategoryPrefetch()
    }

    // Pause between warm-ups — do not hammer the store. The same timer wakes
    // the queue after a catalogue answer: while the open section was loading,
    // the queue stood still.
    Timer {
        id: prefetchPause
        interval: page.prefetchPauseMs
        repeat: false
        onTriggered: page.pumpPrefetch()
    }

    // A one-off light cleanup of stale translations. The work is not urgent:
    // expired entries are not served on reading anyway, the cleanup only frees
    // room in the cache — which is why it runs after everything else.
    Timer {
        interval: page.purgeTranslationsMs
        repeat: false
        running: root.started
        onTriggered: {
            if (root.actions && root.actions.purgeTranslations)
                root.actions.purgeTranslations();
        }
    }

    // Applying a full theme triggers a Qt style change in OUR OWN process
    // (Plasma broadcasts it over D-Bus). The grid must not be rebuilt right at
    // that moment — we wait until the storm settles and only then re-read the
    // section, so the «✅ Active» badge moves to the applied theme.
    Timer {
        interval: page.styleStormMs
        repeat: false
        running: page.reloadPending
        onTriggered: page.reloadAfterStyleStorm()
    }

    // The tab is being closed: no new warm-ups are issued, pictures in flight
    // are aborted.
    Component.onDestruction: page.closePage()

    // Ready pictures are placed one per event loop pass: six decoded JPEGs in
    // a row give a noticeable stutter. The timer lives exactly as long as the
    // core queue says there is something to hand out.
    //
    // Warning: one millisecond, not zero: the original had a QTimer with zero,
    // but a REPEATING QtQuick Timer with a zero interval never fires — it is
    // built on a pause animation, and a zero-length pause slips by.
    // Measurement 29.08.2026 (1.5 s): `interval: 0` with `repeat: true` — zero
    // firings, with `repeat: false` — one. A repeat is needed here, so zero
    // has no place: the queue of ready pictures piled up, and the tiles stayed
    // empty forever — with a full picture cache on disk.
    Timer {
        interval: 1
        repeat: true
        running: page.previewReadyPending
        onTriggered: page.drainPreview()
    }

    // Retrying one request: a timer of its own per request, like the
    // original's single-shot timer. Retries are rare — creating them is
    // harmless.
    Component {
        id: previewRetryTimer
        Timer {
            repeat: false
            property string generation: ""
            property string job: ""
        }
    }

    function schedulePreviewRetry(generation, job, delayMs) {
        const t = previewRetryTimer.createObject(root, {
            "interval": parseInt(delayMs, 10) || 0,
            "generation": generation,
            "job": job
        });
        if (!t)
            return;
        t.triggered.connect(function () {
            page.previewRetry(t.generation, t.job);
            t.destroy();
        });
        t.start();
    }

    // The carousel of the shown tiles is warmed up AFTER them and with a
    // pause: people often run straight through the sections, and without the
    // pause every section that flashed by would manage to order a couple of
    // dozen full-size screenshots.
    property string galleryWarmGeneration: ""

    Timer {
        id: galleryWarm
        interval: page.galleryWarmDelayMs
        repeat: false
        onTriggered: page.warmGallery(root.galleryWarmGeneration)
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colSecondary: root.pal.bg_secondary
    readonly property color colCard: root.pal.bg_card
    readonly property color colCardHover: root.pal.bg_card_hover
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colStar: root.pal.rating_star

    // ── Controls overridden by the tab's style sheet ──────────────────────
    //
    // Warning: the shared palette surfaces (`surface_button`, `surface_field`)
    // say what Qt paints a control with ITSELF by role. That holds while the
    // page has no style sheet of its own. «Appearance» has one, and the role
    // takes no part in it at all: the combo box is painted with the
    // `bg_secondary` token, the page strip with `bg_card`, a secondary button
    // with `bg_card_hover`. Measurement 05.09: QML put the `Button` role
    // (42,64,85) there and differed from the original by 6 475 pixels on the
    // combo box, 2 339 on the refresh button and 2 121 on "Favourites". The
    // decision stays in the core (`appearance_page::control_colors`); the
    // markup only puts it in place.
    readonly property var toneCombo: page.controlColors("combo")
    readonly property var toneNav: page.controlColors("nav")
    readonly property var toneSecondary: page.controlColors("secondary")

    // ── Toggle chip ───────────────────────────────────────────────────────
    //
    // A separate kind of button, not the shared `RexonButton`: the original's
    // chips (`#SrcChip` in the style sheet) have their own drawing — a thin
    // border, a low backing, and the SELECTED one differs by its border and
    // label colour, not by fill. The shared button paints the selection in
    // translucent blue, and the «Store:», «View:» rows looked foreign compared
    // with the original. The shared type must not be touched — it is the same
    // in all tabs; here we define a local, file-scoped one.
    component RexonChip: Button {
        id: chip

        /// Skin palette — the same as for the whole page.
        property var pal: null

        readonly property color colChipBg: chip.pal ? chip.pal.bg_secondary
                                                    : "#16202d"
        readonly property color colChipHover: chip.pal ? chip.pal.bg_card : "#1e2d3d"
        readonly property color colChipOn: chip.pal ? chip.pal.bg_card_hover
                                                    : "#263d52"
        readonly property color colChipBorder: chip.pal ? chip.pal.border : "#2a4055"
        readonly property color colChipAccent: chip.pal ? chip.pal.accent : "#66c0f4"
        readonly property color colChipText: chip.pal ? chip.pal.text_primary
                                                      : "#ffffff"
        readonly property color colChipTextOff: chip.pal ? chip.pal.text_secondary
                                                         : "#c7d5e0"

        // Paddings from `#SrcChip { padding: 5px 10px }`.
        topPadding: 5
        bottomPadding: 5
        leftPadding: 10
        rightPadding: 10

        background: Rectangle {
            radius: 4
            color: chip.checked ? chip.colChipOn
                 : (chip.hovered || chip.down ? chip.colChipHover
                                              : chip.colChipBg)
            // The focus frame matters more than anything else: it is the gamepad
            // control chain, not a decoration.
            border.color: chip.activeFocus ? chip.colChipAccent
                        : (chip.checked ? chip.colChipAccent : chip.colChipBorder)
            border.width: chip.activeFocus ? 2 : 1
        }

        contentItem: RexonText {
            text: chip.text
            color: chip.checked ? chip.colChipAccent
                 : (chip.hovered ? chip.colChipText : chip.colChipTextOff)
            font.bold: chip.checked
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }

    // ── "Listen:" row of a sound theme ────────────────────────────────────
    //
    // Parity with the KDE "System Sounds" module: five icon buttons (bell,
    // warning, message, battery, device) and "▶" — all five in turn. Which
    // events stand behind the buttons and where their files are is decided by
    // the core (`sound_preview::PREVIEW_SLOTS`); here there is only the layout
    // and the queue. An installed theme is played by the system by event
    // (`theme`), a Store theme before installation — with files from the
    // unpacked archive (`sounds`).
    component SoundButtons: Row {
        id: soundRow

        // Warning: an inline component does not see the page's ids (it has its own
        // name scope) — the actions object, the model with labels and the palette
        // arrive as properties, as with `RexonChip`.
        property var player: null
        property var texts: null
        property var pal: null

        /// The installed theme — or empty.
        property string theme: ""
        /// Sounds of a Store theme: [{key, event, file}].
        property var storeSounds: []
        property int buttonSize: 34
        /// Playback error for the user (nothing to play with, no sound).
        signal failed(string text)

        readonly property var sounds: {
            if (soundRow.theme === "")
                return soundRow.storeSounds;
            if (!soundRow.player)
                return [];
            try {
                return JSON.parse(soundRow.player.themeSounds(soundRow.theme)).sounds || [];
            } catch (e) {
                return [];
            }
        }
        /// Index of the sound the "▶" queue is playing; -1 means the queue is
        /// stopped.
        property int queued: -1

        spacing: 4

        function glyph(key) {
            switch (key) {
            case "bell": return "🔔";
            case "warning": return "⚠️";
            case "message": return "💬";
            case "battery": return "🪫";
            case "device": return "🔌";
            }
            return "♪";
        }

        function play(i) {
            const one = soundRow.sounds[i];
            if (!one || one.file === "" || !soundRow.player)
                return;
            const err = soundRow.theme !== ""
                        ? soundRow.player.playThemeSound(soundRow.theme, one.event)
                        : soundRow.player.playFile(one.file);
            if (err !== "") {
                soundRow.queued = -1;
                soundRow.failed(err);
            }
        }

        function playNext(from) {
            for (let i = from; i < soundRow.sounds.length; ++i) {
                if (soundRow.sounds[i].file !== "") {
                    soundRow.queued = i;
                    soundRow.play(i);
                    if (soundRow.queued === i)
                        queueTimer.restart();
                    return;
                }
            }
            soundRow.queued = -1;
        }

        Timer {
            id: queueTimer
            interval: 1500
            repeat: false
            onTriggered: soundRow.playNext(soundRow.queued + 1)
        }

        Repeater {
            model: soundRow.sounds
            delegate: Item {
                id: soundCell
                required property var modelData
                required property int index
                width: soundRow.buttonSize
                height: soundRow.buttonSize

                RexonButton {
                    anchors.fill: parent
                    pal: soundRow.pal
                    leftPadding: 2
                    rightPadding: 2
                    padding: 2
                    enabled: soundCell.modelData.file !== ""
                    highlighted: soundRow.queued === soundCell.index
                    font.pointSize: 13
                    text: soundRow.glyph(soundCell.modelData.key)
                    onClicked: soundRow.play(soundCell.index)
                }

                // A tooltip even on a disabled button: "no sound" is an answer too.
                HoverHandler { id: soundHover }
                ToolTip.text: !soundRow.texts ? ""
                              : (soundCell.modelData.file !== ""
                                 ? soundRow.texts.soundTooltip(soundCell.modelData.event)
                                 : soundRow.texts.soundMissingTooltip)
                ToolTip.visible: soundHover.hovered
                ToolTip.delay: 500
            }
        }

        Item {
            width: soundRow.buttonSize
            height: soundRow.buttonSize

            RexonButton {
                anchors.fill: parent
                pal: soundRow.pal
                leftPadding: 2
                rightPadding: 2
                padding: 2
                enabled: soundRow.sounds.some(function (x) { return x.file !== ""; })
                font.pointSize: 13
                text: soundRow.queued >= 0 ? "■" : "▶"
                onClicked: {
                    if (soundRow.queued >= 0) {
                        queueTimer.stop();
                        soundRow.queued = -1;
                    } else {
                        soundRow.playNext(0);
                    }
                }
            }

            HoverHandler { id: playAllHover }
            ToolTip.text: soundRow.texts ? soundRow.texts.soundPlayAllTooltip : ""
            ToolTip.visible: playAllHover.hovered
            ToolTip.delay: 500
        }
    }

    // ── Start-up ──────────────────────────────────────────────────────────

    property bool started: false

    // ── "Install from file…" ─────────────────────────────────────────────
    //
    // A theme that is already on disk (downloaded by the user, including from
    // Windows): files or a folder through the standard picker window (on
    // Wayland — the desktop portal). What it is and where to install it is
    // decided by the core (`local_install`); here there is only the choice and
    // passing on the addresses.

    /// The section suggested for opening after installation (the theme turned
    /// out to be of another kind or is visible in "Downloaded"), and the button
    /// label.
    property string fileOpenKey: ""
    property string fileOpenText: ""

    // Moved to another section — the previous suggestion is no longer
    // relevant.
    readonly property string fileSection: page.installFileSection
    onFileSectionChanged: root.fileOpenKey = ""

    function installPicked(urls) {
        if (!root.actions || !root.actions.installFiles)
            return;
        const list = [];
        for (let i = 0; i < urls.length; ++i)
            list.push(urls[i].toString());
        if (list.length === 0)
            return;
        const json = JSON.stringify(list);
        root.fileOpenKey = "";
        page.installFileBegin(json);
        root.actions.installFiles(json, page.installFileSection);
    }

    FileDialog {
        id: installFileDialog
        title: page.installFileTitle
        nameFilters: page.installFileFilters
        fileMode: FileDialog.OpenFiles
        onAccepted: root.installPicked(selectedFiles)
    }

    FolderDialog {
        id: installFolderDialog
        title: page.installFileTitle
        onAccepted: root.installPicked([selectedFolder])
    }

    Menu {
        id: installFileMenu
        // Width follows the widest item: the `Menu` of the Basic and Fusion styles
        // does not follow the labels, and a long label in another language was
        // cut ("Go to file locat…"). The same calculation as in the window menu
        // (`WindowMenu.qml`).
        implicitWidth: {
            let widest = 0;
            for (let i = 0; i < installFileMenu.count; ++i) {
                const entry = installFileMenu.itemAt(i) as MenuItem;
                if (entry && entry.contentItem)
                    widest = Math.max(widest, entry.contentItem.implicitWidth
                                      + entry.leftPadding + entry.rightPadding + 16);
            }
            return Math.max(installFileMenu.implicitBackgroundWidth + installFileMenu.leftInset + installFileMenu.rightInset,
                            widest + installFileMenu.leftPadding + installFileMenu.rightPadding);
        }
        // The backing is the skin card, as in the window menu (`WindowMenu.qml`):
        // the Basic style backing is white and does not follow the palette.
        background: Rectangle {
            implicitWidth: 220
            color: root.colCard
            border.color: root.colBorder
            border.width: 1
            radius: 4
        }

        PickItem {
            text: page.installFileFilesText
            onTriggered: installFileDialog.open()
        }

        PickItem {
            text: page.installFileFolderText
            onTriggered: installFolderDialog.open()
        }
    }

    // A choice menu item: highlight and focus frame — as in the window menu.
    component PickItem: MenuItem {
        id: pickEntry
        contentItem: RexonText {
            text: pickEntry.text
            color: root.colText
            verticalAlignment: Text.AlignVCenter
            leftPadding: 8
        }
        background: Rectangle {
            implicitWidth: 220
            implicitHeight: 40
            radius: 3
            color: pickEntry.highlighted ? root.colCardHover : "transparent"
            border.width: pickEntry.visualFocus ? 2 : 0
            border.color: root.colAccent
        }
    }

    function bootstrap() {
        if (root.started || !root.actions)
            return;
        root.started = true;
        if (root.actions.environment)
            page.setEnvironment(root.actions.environment());
        // What draws the login screen and whether this is SteamOS — in the
        // background: the probe may call `systemctl`. The answer will arrive as
        // `loginScreenReady`.
        if (root.actions.probeLoginScreen)
            root.actions.probeLoginScreen();
        // Warning: the stars are part of the start-up too. Previously the window
        // host told the page about them (`_tell_favorites` in Python), and a page
        // with its own actions object would have nobody to tell it: the first show
        // would come with empty stars on themes the user had already marked.
        if (root.actions.favorites)
            page.setFavorites(root.actions.favorites());
        // The AUR installation queue lives longer than the page: a reopened page
        // picks up the running installation and the results of past ones.
        if (root.actions.aurJobs)
            page.setAurJobs(root.actions.aurJobs());
        root.focusFirstControl();
        root.takeSubpage();
    }

    onActionsChanged: root.bootstrap()
    Component.onCompleted: root.bootstrap()

    // Tile button. What exactly it does is decided by the tile kind: a store
    // item is installed and applied, a local theme is only applied, an AUR
    // package is built by the helper.
    // "🗑" on the tile of a theme from disk: what is allowed and how to remove
    // it is decided by the host (`removeLocalById`), the result arrives by the
    // `removalPending`/`actionDone` signals.
    /// The tile whose "🗑" was pressed last: through it `onRemovalDone` learns
    /// that the open card belongs to that very removed theme.
    property string removingId: ""

    function removeLocal(itemId) {
        root.removingId = itemId;
        if (root.actions && root.actions.removeLocalById)
            root.actions.removeLocalById(itemId);
    }

    /// A Plasma global theme: before applying — the "Apply "X"?" window
    /// (appearance / layout), as in Plasma. `true` means the window is open and
    /// will apply by itself.
    function askGlobal(itemId) {
        if (!root.actions || !root.actions.globalApplyQuestion)
            return false;
        const json = root.actions.globalApplyQuestion(itemId);
        if (!json)
            return false;
        globalApplyDialog.show(JSON.parse(json));
        return true;
    }

    function applyLocal(itemId) {
        if (root.askGlobal(itemId))
            return;
        page.actionBegin(itemId, "apply");
        if (root.actions && root.actions.applyLocalById)
            root.actions.applyLocalById(itemId);
    }

    /// An action of a GNOME extension that the shell knows: `toggle`, `prefs`,
    /// `folder`, `errorDetails` (removal goes through `removeLocal`: a question
    /// and "Restore", as for themes; core: `gnome_ext_section::run_action`). The
    /// result is `gnomeExtensionDone`; toggling and removal also give
    /// `actionDone`.
    function gnomeAction(itemId, action) {
        if (!root.actions || !root.actions.gnomeExtensionAction)
            return;
        if (action === "toggle")
            page.actionBegin(itemId, "apply");
        root.actions.gnomeExtensionAction(itemId, action);
    }

    function actOnCard(kind, itemId) {
        if (!root.actions)
            return;
        // A long action starts HERE, not on the host's first answer: otherwise
        // between the press and the first log line the page looks as if the
        // button did not work. The action kind decides whether it gets a progress
        // bar and what label it is called by.
        if (kind === "local") {
            root.applyLocal(itemId);
            return;
        }
        // An AUR package goes into the application's SHARED queue (one
        // installation at a time): the stage on the tile and the line at the top
        // are drawn from the queue snapshot (`aurJobsChanged`), not from the
        // page's general "busy" — while one package is building, the other tiles
        // can be pressed and go "Queued".
        if (kind === "aur") {
            if (root.actions.installAur)
                root.actions.installAur(itemId);
            return;
        }
        // As in KDE: «Install» only installs; for an installed theme the same
        // button is "Apply". What exactly to do is decided by the core.
        // An installed global theme — the choice window first.
        if (root.askGlobal(itemId))
            return;
        page.actionBegin(itemId, "install");
        if (root.actions.actOnStoreItem)
            root.actions.actOnStoreItem(itemId);
    }

    // The request is not sent instantly: clicking through sections in a row
    // must not send the store a request per click. The pauses are core numbers
    // (the first visit is shorter than the rest: an empty showcase when the
    // tab opens looks broken).
    Timer {
        id: catalogDebounce
        interval: root.firstLoadDone ? page.catalogDebounceMs : page.firstLoadMs
        repeat: false
        onTriggered: {
            root.firstLoadDone = true;
            root.sendCatalogRequest();
        }
    }

    property bool firstLoadDone: false

    // ── A link to a store item from third-party text ─────────────────────
    //
    // The card is assembled by the host (`openLinkedCard`, answer
    // `linkedCardReady`); a host without this method gets the same question as
    // for an external site.
    function openLinkedCard(card, url) {
        if (root.actions && root.actions.openLinkedCard) {
            root.actions.openLinkedCard(JSON.stringify({ card: card, url: url }));
            return;
        }
        linkAsk.ask(url, null);
    }

    RexonLinkDialog {
        id: linkAsk
        parent: root
        pal: root.pal
    }

    function askCatalog() {
        catalogDebounce.restart();
    }

    function sendCatalogRequest() {
        if (!root.actions || !root.actions.loadCatalog)
            return;
        const request = page.catalogRequest();
        if (request)
            root.actions.loadCatalog(request);
    }

    // Long work answers with signals — the network cannot have a synchronous
    // answer, the page would freeze on it together with the window.
    Connections {
        target: root.actions
        ignoreUnknownSignals: true

        function onCatalogReady(payload) {
            page.catalogAnswer(payload);
            root.galleryWarmGeneration = page.catalogGeneration();
            galleryWarm.restart();
            prefetchPause.restart();
        }

        function onPrefetchReady(payload) {
            page.prefetchAnswer(payload);
            prefetchPause.restart();
        }

        function onPreviewDone(job, path) {
            page.previewDone(job, path);
        }

        function onPreviewFailed(job, status) {
            page.previewFailed(job, status);
        }

        function onStatusMessage(text) {
            page.setStatus(text);
        }

        function onDetailReady(payload) {
            page.detailAnswer(payload);
        }

        // The item card from a link in third-party text is assembled — show it;
        // if that failed — the same question as for any external site.
        function onLinkedCardReady(payload) {
            const r = JSON.parse(payload);
            if (r.failed) {
                page.setStatus(r.status || "");
                linkAsk.ask(r.url || "", null);
                return;
            }
            page.openLinkedItem(payload);
        }

        function onAurDetailReady(payload) {
            page.aurDetailAnswer(payload);
        }

        function onAurTrustReady(payload) {
            page.aurTrustAnswer(payload);
        }

        function onAurPkgbuildReady(payload) {
            pkgbuildPopup.answer(payload);
        }

        function onDescriptionReady(payload) {
            page.descriptionAnswer(payload);
        }

        function onInstallProgress(pct) {
            page.setInstallPercent(pct);
        }

        function onInstallLog(text) {
            page.installLog(text);
        }

        function onAurJobsChanged(payload) {
            page.setAurJobs(payload);
        }

        // Result of a GNOME extension action. Toggling and removal also report
        // through `actionDone` (the line under the buttons) — here the grid is
        // re-read from the shell's fresh word; error details go to their own
        // window; a copied system path goes to the status line.
        function onGnomeExtensionDone(payload) {
            let r = {};
            try {
                r = JSON.parse(payload);
            } catch (e) {
                return;
            }
            if (r.action === "errorDetails") {
                if (r.ok)
                    gnomeErrorPopup.answer(r);
                else
                    page.setStatus(r.message || "");
                return;
            }
            const busy = r.action === "toggle" || r.action === "enable"
                         || r.action === "disable" || r.action === "uninstall";
            if (busy) {
                if (r.ok && r.action === "uninstall" && page.screen === "detail")
                    page.closeDetail();
                if (r.ok)
                    grid.refreshKeepingScroll();
                return;
            }
            if ((r.message || "") === "")
                return;
            if (page.screen === "detail")
                page.setDetailStatus(r.message, r.ok === true);
            else
                page.setStatus(r.message);
        }

        function onShellPaletteDone(text, ok) {
            shellPaletteBar.busy = false;
            shellPaletteBar.message = text;
            shellPaletteBar.failed = !ok;
            shellPaletteBar.refresh();
            // The shell banner button may have gone — the colours get their own.
            appliedRevertBar.rev++;
            // The scheme changed bypassing the tiles: the «✅ Active» badge in the
            // from-disk section moves only after re-reading (the host has already
            // dropped the scan cache).
            if (ok)
                page.reloadLocalFresh();
        }

        // "Install from file…": which section to suggest opening and whether the
        // open from-disk section needs re-reading — decided by the core
        // (`local_install`).
        function onLocalInstallDone(payload) {
            let r = {};
            try {
                r = JSON.parse(payload);
            } catch (e) {
                return;
            }
            root.fileOpenKey = r.openKey || "";
            root.fileOpenText = r.openText || "";
            // A section with installed items shows what was installed by itself:
            // re-read it.
            if (r.reload)
                page.showInstalledFresh();
        }

        // A theme from disk is marked for removal: the "Restore" line at the
        // bottom, the tile leaves the grid (the host does not return marked ones
        // in scan answers).
        function onRemovalPending(text) {
            removalBar.show(text);
            if (page.screen === "detail")
                page.closeDetail();
            grid.refreshKeepingScroll();
        }

        // The marked item is removed (or a system theme was removed by the
        // helper): the result goes to the status line, the section is re-read from
        // disk.
        function onRemovalDone(text) {
            if (text)
                page.setStatus(text);
            // The card of a removed theme would show something that is no longer on
            // disk, with working "Apply" and "Delete".
            if (page.screen === "detail" && page.detailItemId === root.removingId)
                page.closeDetail();
            root.removingId = "";
            grid.refreshKeepingScroll();
        }

        function onActionDone(text, ok) {
            page.setDetailStatus(text, ok);
            // The applied scheme may have gone straight to the shell — "Restore as it
            // was" must see that.
            shellPaletteBar.refresh();
            // A successful "Apply" left a snapshot — the "Restore as it was" button.
            appliedRevertBar.rev++;
            // The cursor size may have been changed both by the theme and by the chip
            // — we mark what actually came out.
            if (cursorSizeRow.visible && root.actions.cursorSize)
                cursorSizeRow.current = root.actions.cursorSize();
        }

        function onGalleryReady(index, path) {
            page.setGalleryImage(index, path);
        }

        function onGalleryFailed(index) {
            page.setGalleryFailed(index);
        }

        function onGalleryFinished(count) {
            page.setGalleryFinished(count);
        }

        function onCommentsReady(payload) {
            page.commentsAnswer(payload);
        }

        function onCommentTranslationsReady(payload) {
            page.commentTranslationsAnswer(payload);
        }

        function onSuggestReady(payload) {
            page.suggestAnswer(payload);
        }

        function onLoginScreenReady(payload) {
            page.setLoginScreen(payload);
        }

        // The page loads asynchronously: the probe answer may have arrived before
        // this `Connections` was connected. Once connected, we pick up the ready
        // one.
        Component.onCompleted: {
            if (root.actions && root.actions.loginScreen) {
                const last = root.actions.loginScreen();
                if (last !== "")
                    page.setLoginScreen(last);
            }
        }
    }


    // ── Listening to a Store item before installation ───────────────────
    //
    // The state lives on the page, not on the panel: the answer arrives as a
    // signal seconds later, and by then the panel may have been closed and
    // opened again. The answer for another item (a different one was opened
    // while the first was downloading) is not shown.
    property string storePreviewId: ""
    property bool storePreviewBusy: false
    property bool storePreviewFailed: false
    property string storePreviewStatus: ""
    property var storePreviewThemes: []
    property real storePreviewDone: 0
    property real storePreviewTotal: 0

    readonly property string openedItemId: page.detailItemId
    onOpenedItemIdChanged: {
        // A different item — the previous result does not apply to it.
        if (root.openedItemId !== root.storePreviewId) {
            root.storePreviewId = "";
            root.storePreviewBusy = false;
            root.storePreviewFailed = false;
            root.storePreviewStatus = "";
            root.storePreviewThemes = [];
        }
    }

    function startStorePreview() {
        if (root.storePreviewBusy)
            return;
        root.storePreviewId = page.detailItemId;
        root.storePreviewBusy = true;
        root.storePreviewFailed = false;
        root.storePreviewThemes = [];
        root.storePreviewDone = 0;
        root.storePreviewTotal = 0;
        root.storePreviewStatus = page.soundStoreProgress(0, 0);
        soundActions.previewStoreTheme(page.detailItemId, page.detailDownloadUrl());
    }

    Connections {
        target: soundActions

        function onStorePreviewProgress(contentId, done, total) {
            if (contentId !== root.storePreviewId || !root.storePreviewBusy)
                return;
            root.storePreviewDone = done;
            root.storePreviewTotal = total;
            root.storePreviewStatus = page.soundStoreProgress(done, total);
        }

        function onStorePreviewReady(payload) {
            let answer = null;
            try {
                answer = JSON.parse(payload);
            } catch (e) {
                return;
            }
            if (answer.contentId !== root.storePreviewId)
                return;
            root.storePreviewBusy = false;
            root.storePreviewFailed = !answer.ok;
            root.storePreviewStatus = answer.ok ? "" : answer.error;
            root.storePreviewThemes = answer.ok ? answer.themes : [];
        }
    }

    // Focus on the first frame goes to the FIRST OPERABLE ELEMENT, not to the
    // page root: `forceActiveFocus()` on the root makes activeFocusItem a
    // nameless Item, and the page is dead both from the keyboard and from a
    // gamepad.
    function focusFirstControl() {
        // Warning: the first focus goes to the SEARCH FIELD, as in the original:
        // its side panel is `NoFocus` and takes no part in the traversal at all,
        // and search comes first in order. Here focus used to go to the first row
        // of the panel — the live run on 01.09.2026 showed the difference at once:
        // the original highlights the search field with a frame, ours highlighted
        // some other place.
        if (searchBox.visible) {
            searchBox.forceActiveFocus();
            return;
        }
        // The section has no search — then the section panel: there must be no
        // empty focus, the chain is needed for gamepad control.
        if (sidebar.count > 0 && sidebar.itemAtIndex(0)) {
            sidebar.itemAtIndex(0).forceActiveFocus();
            return;
        }
        Qt.callLater(root.focusFirstControl);
    }

    onVisibleChanged: {
        if (root.visible) {
            root.focusFirstControl();
            root.takeSubpage();
        }
    }

    // ── Section opened from outside ───────────────────────────────────────
    //
    // An "Appearance → …" link of the "Settings" tab opens the tab through the
    // window door (`openSection("appearance", key)`), and the section key
    // travels as the window's sub-page (`startSubpage`). A foreign sub-page
    // ("compare" of "Desktops") is left alone: we take only the key of a
    // visible side panel section.
    function takeSubpage() {
        const w = root.Window.window;
        if (!w || w.startSubpage === undefined || w.startSubpage === "")
            return;
        const key = String(w.startSubpage);
        if (!(page.sidebarRows || []).some(r => r.key === key && r.available))
            return;
        w.startSubpage = "";
        page.selectCategory(key);
    }

    Connections {
        target: root.Window.window
        ignoreUnknownSignals: true
        function onStartSubpageChanged() { root.takeSubpage(); }
    }

    // ── Tour mode (`rexon-main-window --tour`) ────────────────────────────
    //
    // The tour executor (qml/MainWindow.qml) calls ONLY the `tour*` functions
    // below — it checks the name of each with the core
    // (`main_window_tour::call_allowed`) — and reads `tourState()`. These are
    // navigation, search and viewing: none of them installs, applies or
    // removes. The tour order and the analysis of what was seen are in the
    // core.

    function tourCategories() {
        return (page.sidebarRows || []).filter(r => r.available && r.key !== "")
                                       .map(r => String(r.key));
    }

    function tourSources() {
        return page.sourceBarVisible
               ? (page.sourceChips || []).map(c => String(c.key)) : [];
    }

    function tourSelectCategory(key) {
        if (!root.tourCategories().includes(key))
            return false;
        page.selectCategory(key);
        return true;
    }

    function tourSetSource(key) {
        if (!root.tourSources().includes(key))
            return false;
        page.setSource(key);
        return true;
    }

    // As a human does: a word in the field and Enter (the same path as
    // `onAccepted`).
    function tourTypeQuery(text) {
        if (!searchBox.visible)
            return false;
        searchBox.forceActiveFocus();
        searchBox.text = text;
        page.setQuery(text);
        page.submitQuery();
        page.updateSuggestions("");
        return true;
    }

    // Like the ✕ button.
    function tourClearQuery() {
        searchBox.text = "";
        page.setQuery("");
        page.updateSuggestions("");
        return true;
    }

    function tourState() {
        let tiles = page.tourTiles();
        // Axon has its own library and its own tiles (AxonLibraryView); the
        // catalogue model does not know them — otherwise the tour would see "0
        // tiles".
        if (page.screen === "axon" && axonLoader.item) {
            const items = (axonLoader.item.v.sections || []).reduce((a, s) => a.concat(s.items || []), []);
            tiles = { total: items.length, noPreview: items.filter(i => !i.cover).length, pending: 0 };
        }
        const shown = searchBox.visible && searchBox.width > 0;
        const p = shown ? searchBox.mapToItem(null, 0, 0) : { x: 0, y: 0 };
        const active = (page.sourceChips || []).find(c => c.checked);
        return {
            busy: page.tourBusy(),
            status: page.statusText,
            statusWarn: page.statusWarn,
            screen: page.screen,
            tiles: tiles.total,
            noPreview: tiles.noPreview,
            pending: tiles.pending,
            search: page.searchVisible,
            searchVisible: shown,
            searchX: Math.round(p.x),
            searchY: Math.round(p.y),
            searchText: searchBox.text,
            query: page.query,
            source: active ? String(active.key) : ""
        };
    }

    // ── Search suggestion drop-down ───────────────────────────────────────
    //
    // It lies on top of the page, not inside the header: the list must cover
    // the grid, not push it apart. Position and size are computed by the core
    // (`suggest_box`) — the markup has no margins of its own.
    Rectangle {
        id: suggestBox
        visible: page.suggestVisible
        z: 100
        color: root.colSecondary
        border.color: root.colBorder
        border.width: 1
        radius: 4

        // The core computes relative to the start of the line: for right to left
        // the edge of the search field and the drop-down itself are converted back
        // and forth by `placeX`, and the drop-down extends from the right edge of
        // the search field to the left.
        readonly property var box: page.suggestGeometry(
            String(Math.round(root.arrows.placeX(root.mapFromItem(searchBox, 0, 0).x,
                                                 searchBox.width, root.width))),
            String(Math.round(searchBox.width)),
            String(Math.round(root.width)))

        x: root.arrows.placeX(suggestBox.box.x, suggestBox.box.width, root.width)
        y: root.mapFromItem(searchBox, 0, searchBox.height).y + 2
        width: suggestBox.box.width
        height: suggestBox.box.height

        ListView {
            id: suggestList
            anchors.fill: parent
            anchors.margins: 2
            clip: true
            model: page.suggestRows

            delegate: ItemDelegate {
                id: suggestRow
                required property var modelData
                width: suggestList.width
                height: page.suggestRowHeight

                background: Rectangle {
                    color: suggestRow.modelData.current || suggestRow.hovered
                           ? root.colCardHover : "transparent"
                }

                contentItem: RowLayout {
                    spacing: 8

                    Rectangle {
                        Layout.preferredWidth: page.suggestThumbWidth
                        Layout.preferredHeight: page.suggestThumbHeight
                        color: root.colCard
                        radius: 2

                        Image {
                            anchors.fill: parent
                            source: suggestRow.modelData.thumb
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            visible: source !== ""
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0

                        RexonText {
                            Layout.fillWidth: true
                            text: suggestRow.modelData.name
                            color: root.colText
                            font.bold: true
                            elide: Text.ElideRight
                        }

                        RexonText {
                            Layout.fillWidth: true
                            text: suggestRow.modelData.author
                            color: root.colTextSecondary
                            font.pointSize: 9
                            elide: Text.ElideRight
                        }
                    }
                }

                onClicked: page.suggestPick(suggestRow.modelData.index)
            }
        }
    }

    // ── Large picture view ────────────────────────────────────────────────
    //
    // The layer is shared across the whole product (`RexonImageViewer.qml`):
    // the same as in the Steam, "Extensions" and "Settings" tabs — paging with
    // arrows, zoom, full screen, Esc. The page has no viewer of its own.

    /// Screenshots of the open theme card: paging over all its loaded frames,
    /// opening on the one shown.
    function openZoom() {
        const frames = page.zoomFrames();
        if (!frames || frames.length < 2)
            return;
        // The core puts the index of the frame to open on into the last slot.
        const at = parseInt(frames[frames.length - 1], 10);
        const items = [];
        for (let i = 0; i < frames.length - 1; i++) {
            const path = String(frames[i]);
            items.push({
                "key": String(i),
                "title": page.detailName,
                "thumb": path,
                "source": path,
                "animatedImage": page.isAnimatedPreview(path)
            });
        }
        zoomPopup.show(items, at);
    }

    /// Picture of a showcase tile — a single one, without paging: the theme's
    /// other screenshots live in its card. `big` is the theme's full-size
    /// picture on disk (empty — the tile picture is the largest there is):
    /// the tile picture shows at once, the large one replaces it when read.
    function openTileZoom(name, path, big) {
        if (!path)
            return;
        const full = big ? big : path;
        zoomPopup.show([{
            "key": path,
            "title": name,
            "thumb": path,
            "source": full,
            "animatedImage": page.isAnimatedPreview(full)
        }], 0);
    }

    // "Apply "X"?" of a Plasma global theme — the same window as in Plasma:
    // the simple view — «Appearance» and "Desktop and window layout" (check
    // boxes when the theme has both; a single part — as a line), the detailed
    // one ("Choose what to apply…") — a check box for every part of the
    // theme. Texts, visibility and default marks are decided by the core
    // (`themes::global_question_in`); outside Plasma the window is detailed
    // right away, and the parts that do not carry over here are disabled with
    // a reason. Focus on opening is on "Apply", as in Plasma; Escape cancels.
    Popup {
        id: globalApplyDialog
        objectName: "globalApplyDialog"
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min((parent ? parent.width : 600) - 40, 560)
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape

        property var q: ({})
        property bool appearance: false
        property bool layout: false
        /// The detailed view and the parts checked in it (a mask of Plasma flags).
        property bool detailed: false
        property int mask: 0
        readonly property var parts: q.parts || []
        readonly property var layoutParts: parts.filter(p => p.layout)
        readonly property var appearanceParts: parts.filter(p => !p.layout)

        function show(question) {
            q = question;
            detailed = question.detailed === true;
            reset();
            open();
        }

        /// Default marks — also on every view switch, as in Plasma
        /// (`resetCheckboxes`).
        function reset() {
            appearance = q.appearanceChecked === true;
            layout = q.layoutChecked === true;
            // A click on a check box breaks the `checked` binding, so the marks are
            // set again.
            globalApplyAppearanceBox.checked = appearance;
            globalApplyLayoutBox.checked = layout;
            let m = 0;
            for (const p of parts) {
                if (p.checked)
                    m |= p.mask;
            }
            mask = m;
        }

        function partOn(p) {
            return (mask & p.mask) !== 0;
        }

        function setPart(p, on) {
            mask = on ? (mask | p.mask) : (mask & ~p.mask);
        }

        /// Whether the desktop layout is checked (detailed view).
        readonly property bool desktopLayoutOn: detailed
            && layoutParts.some(p => p.id === "desktopLayout" && partOn(p))
        readonly property bool anyPicked: detailed
            ? parts.some(p => p.enabled && partOn(p))
            : (appearance || layout)

        Overlay.modal: Rectangle { color: root.pal.modal_dim }
        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        contentItem: ColumnLayout {
            spacing: 10

            RexonText {
                Layout.fillWidth: true
                text: globalApplyDialog.q.title || ""
                color: root.colText
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                visible: (globalApplyDialog.q.outsideNote || "") !== ""
                text: globalApplyDialog.q.outsideNote || ""
                color: root.colTextSecondary
                wrapMode: Text.WordWrap
            }

            // ── Simple view ──
            RexonText {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && (globalApplyDialog.q.emptyText || "") === ""
                text: globalApplyDialog.q.heading || ""
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RexonCheckBox {
                id: globalApplyAppearanceBox
                objectName: "globalApplyAppearance"
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.appearanceRow === "checkbox"
                pal: root.pal
                text: globalApplyDialog.q.appearanceText || ""
                onToggled: globalApplyDialog.appearance = checked
            }

            RexonText {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.appearanceRow === "bullet"
                text: "• " + (globalApplyDialog.q.appearanceText || "")
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RexonCheckBox {
                id: globalApplyLayoutBox
                objectName: "globalApplyLayout"
                // Window-wide: the label "(from Breeze — the theme has none of its own)"
                // is long and wraps rather than sticking out past the edge.
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.layoutRow === "checkbox"
                pal: root.pal
                text: globalApplyDialog.q.layoutText || ""
                onToggled: globalApplyDialog.layout = checked
            }

            RexonText {
                Layout.fillWidth: true
                visible: !globalApplyDialog.detailed && globalApplyDialog.q.layoutRow === "bullet"
                text: "• " + (globalApplyDialog.q.layoutText || "")
                color: root.colText
                wrapMode: Text.WordWrap
            }

            // ── Detailed view: "Layout:", then "Appearance:" ──
            // For a theme with all parts the list is long: on a low screen it scrolls,
            // while the window buttons stay in place.
            ScrollView {
                id: globalApplyPartsScroll
                Layout.fillWidth: true
                visible: globalApplyDialog.detailed
                Layout.preferredHeight: Math.min(globalApplyPartsCol.implicitHeight,
                    (globalApplyDialog.parent ? globalApplyDialog.parent.height : 600) - 260)
                clip: true
                contentWidth: availableWidth

                ColumnLayout {
                    id: globalApplyPartsCol
                    width: globalApplyPartsScroll.availableWidth
                    spacing: 10

                    RexonText {
                        Layout.fillWidth: true
                        visible: globalApplyDialog.detailed && globalApplyDialog.layoutParts.length > 0
                        text: globalApplyDialog.q.layoutHeading || ""
                        color: root.colText
                        font.bold: true
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: globalApplyDialog.detailed ? globalApplyDialog.layoutParts : []
                        delegate: globalApplyPartRow
                    }

                    RexonText {
                        Layout.fillWidth: true
                        visible: globalApplyDialog.detailed && globalApplyDialog.appearanceParts.length > 0
                        text: globalApplyDialog.q.appearanceHeading || ""
                        color: root.colText
                        font.bold: true
                        wrapMode: Text.WordWrap
                    }

                    Repeater {
                        model: globalApplyDialog.detailed ? globalApplyDialog.appearanceParts : []
                        delegate: globalApplyPartRow
                    }
                }
            }

            // A warning while the layout is checked: it wipes the current panels and
            // widgets.
            RexonText {
                Layout.fillWidth: true
                visible: globalApplyDialog.detailed
                         ? globalApplyDialog.desktopLayoutOn
                         : (globalApplyDialog.layout && globalApplyDialog.q.layoutRow === "checkbox")
                text: "⚠ " + (globalApplyDialog.q.layoutWarning || "")
                color: root.colStar
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                visible: (globalApplyDialog.q.emptyText || "") !== ""
                text: globalApplyDialog.q.emptyText || ""
                color: root.colText
                wrapMode: Text.WordWrap
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                // "Choose what to apply…" / "Fewer options…" — on the left, as in Plasma;
                // the marks are reset on switching.
                RexonButton {
                    objectName: "globalApplyMore"
                    pal: root.pal
                    text: globalApplyDialog.detailed
                          ? (globalApplyDialog.q.fewerText || "")
                          : (globalApplyDialog.q.moreText || "")
                    enabled: globalApplyDialog.q.moreEnabled === true
                    onClicked: {
                        globalApplyDialog.detailed = !globalApplyDialog.detailed;
                        globalApplyDialog.reset();
                    }
                }

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: globalApplyYes
                    objectName: "globalApplyYes"
                    pal: root.pal
                    highlighted: true
                    text: globalApplyDialog.q.applyText || ""
                    enabled: globalApplyDialog.anyPicked
                    onClicked: {
                        const id = globalApplyDialog.q.itemId || "";
                        const appearance = globalApplyDialog.appearance;
                        const layout = globalApplyDialog.layout;
                        const parts = globalApplyDialog.detailed ? globalApplyDialog.mask : -1;
                        globalApplyDialog.close();
                        page.actionBegin(id, "apply");
                        if (root.actions && root.actions.applyGlobalById)
                            root.actions.applyGlobalById(id, appearance, layout, parts);
                    }
                }

                RexonButton {
                    objectName: "globalApplyNo"
                    pal: root.pal
                    text: globalApplyDialog.q.cancelText || ""
                    onClicked: globalApplyDialog.close()
                }
            }
        }

        onOpened: globalApplyYes.forceActiveFocus()
    }

    // A part's check box in the detailed view of the "Apply "X"?" window; a
    // disabled one has its reason under it (outside Plasma).
    Component {
        id: globalApplyPartRow

        ColumnLayout {
            id: partRow
            required property var modelData
            Layout.fillWidth: true
            Layout.leftMargin: 8
            spacing: 2

            RexonCheckBox {
                objectName: "globalApplyPart_" + partRow.modelData.id
                Layout.fillWidth: true
                pal: root.pal
                text: partRow.modelData.text
                enabled: partRow.modelData.enabled
                // A part that does not carry over — muted, with the reason under it.
                opacity: enabled ? 1 : 0.55
                checked: globalApplyDialog.partOn(partRow.modelData)
                onToggled: globalApplyDialog.setPart(partRow.modelData, checked)
            }

            RexonText {
                Layout.fillWidth: true
                Layout.leftMargin: 28
                visible: partRow.modelData.reason !== ""
                text: partRow.modelData.reason
                color: root.colTextSecondary
                wrapMode: Text.WordWrap
            }
        }
    }

    // Our own colour scheme editor — for when the KDE editor
    // (kcolorschemeeditor) is not on the system.
    ColorSchemeEditor {
        id: schemeEditor
        objectName: "colorSchemeEditor"
        parent: root
        anchors.centerIn: parent
        pal: root.pal
        texts: JSON.parse(page.detailSchemeEdit || "{}")
        onSaveRequested: function(name, roles, base, overwrite) {
            const r = JSON.parse(page.saveSchemeCopy(name, roles, base, overwrite) || "{}");
            schemeEditor.done(r.ok === true, r.message || "");
            if (r.ok === true) {
                page.setStatus(r.message || "");
                // The new scheme is in the grid: go back there.
                page.closeDetail();
            }
        }

        function start(mode) {
            const r = JSON.parse(page.editDetailScheme(mode) || "{}");
            if (r.error) {
                page.setStatus(r.error);
                return;
            }
            if (r.mode === "own")
                schemeEditor.show(r);
        }
    }

    // The PKGBUILD of an AUR package — inside the program, not in the browser:
    // what will be downloaded and executed during the build.
    Popup {
        id: pkgbuildPopup
        objectName: "aurPkgbuildPopup"
        parent: root
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 960)
        height: root.height - 40
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        property string itemId: ""
        property string text: ""
        property string error: ""

        function show() {
            itemId = page.detailItemId;
            text = "";
            error = "";
            open();
            if (root.actions && root.actions.loadAurPkgbuild)
                root.actions.loadAurPkgbuild(page.detailItemId, page.aurPackageBase);
        }

        function answer(payload) {
            const v = JSON.parse(payload);
            if (v.itemId !== itemId)
                return;
            text = v.text || "";
            error = v.error || "";
        }

        Overlay.modal: Rectangle { color: root.pal.modal_dim }
        background: Rectangle {
            color: root.colBg
            border.color: root.colBorder
            radius: 4
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                RexonText {
                    Layout.fillWidth: true
                    text: page.aurPkgbuildTitle
                    color: root.colText
                    font.bold: true
                    font.pointSize: 13
                    elide: Text.ElideRight
                }

                RexonButton {
                    pal: root.pal
                    tone: root.toneSecondary
                    text: page.aurOpenText
                    onClicked: {
                        const url = page.detailSiteUrl();
                        if (url && root.actions && root.actions.openUrl)
                            root.actions.openUrl(url);
                    }
                }

                RexonButton {
                    id: pkgbuildClose
                    pal: root.pal
                    text: "❌"
                    onClicked: pkgbuildPopup.close()
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: pkgbuildPopup.text === "" || pkgbuildPopup.error !== ""
                text: pkgbuildPopup.error !== "" ? pkgbuildPopup.error : page.aurPkgbuildLoading
                color: pkgbuildPopup.error !== "" ? root.colStar : root.colTextSecondary
                wrapMode: Text.WordWrap
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                visible: pkgbuildPopup.text !== ""
                clip: true

                TextArea {
                    objectName: "aurPkgbuildText"
                    readOnly: true
                    selectByMouse: true
                    text: pkgbuildPopup.text
                    textFormat: TextEdit.PlainText
                    wrapMode: TextEdit.NoWrap
                    // Start of the line for right-to-left languages. Without explicit
                    // alignment the field takes it from the text itself, not from window
                    // mirroring. A PKGBUILD is shell code and reads left to right in any
                    // interface language, so the alignment is set explicitly and physically
                    // on the left: mirroring would flip an explicit `AlignLeft` to the right,
                    // while `AlignRight` under mirroring gives the left edge.
                    horizontalAlignment: LayoutMirroring.enabled ? TextEdit.AlignRight
                                                                 : TextEdit.AlignLeft
                    color: root.colText
                    font.family: "monospace"
                    font.pointSize: 10
                    background: Rectangle {
                        color: root.colSecondary
                        border.color: root.colBorder
                    }
                }
            }
        }

        onOpened: pkgbuildClose.forceActiveFocus()
    }

    // "Error details" of a GNOME extension: what the shell said when it
    // crashed (the last error and its log) — copy it all and take it to the
    // author on their page.
    Popup {
        id: gnomeErrorPopup
        objectName: "gnomeErrorPopup"
        parent: root
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 760)
        height: Math.min(root.height - 40, 520)
        modal: true
        focus: true
        padding: 16
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        property string itemId: ""
        property string name: ""
        property string url: ""
        property string text: ""

        function showFor(itemId, name) {
            gnomeErrorPopup.itemId = itemId;
            gnomeErrorPopup.name = name;
            gnomeErrorPopup.url = "";
            gnomeErrorPopup.text = "";
            open();
            root.gnomeAction(itemId, "errorDetails");
        }

        function answer(r) {
            if (!gnomeErrorPopup.opened)
                return;
            gnomeErrorPopup.text = r.details || "";
            gnomeErrorPopup.url = r.url || "";
            if ((r.name || "") !== "")
                gnomeErrorPopup.name = r.name;
        }

        Overlay.modal: Rectangle { color: root.pal.modal_dim }
        background: Rectangle {
            color: root.colBg
            border.color: root.colBorder
            radius: 4
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: 10

            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                RexonText {
                    Layout.fillWidth: true
                    text: bidi.arg(root.gnomeTexts.error_title || "%1", gnomeErrorPopup.name)
                    color: root.colText
                    font.bold: true
                    font.pointSize: 13
                    elide: Text.ElideRight
                }

                RexonButton {
                    id: gnomeErrorClose
                    pal: root.pal
                    text: "❌"
                    onClicked: gnomeErrorPopup.close()
                }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                TextArea {
                    id: gnomeErrorText
                    objectName: "gnomeErrorText"
                    readOnly: true
                    selectByMouse: true
                    text: gnomeErrorPopup.text
                    textFormat: TextEdit.PlainText
                    wrapMode: TextEdit.WrapAtWordBoundaryOrAnywhere
                    // A JavaScript error is code and reads left to right in any interface
                    // language: the alignment is explicit and physically on the left (like
                    // the PKGBUILD above).
                    horizontalAlignment: LayoutMirroring.enabled ? TextEdit.AlignRight
                                                                 : TextEdit.AlignLeft
                    color: root.colText
                    font.family: "monospace"
                    font.pointSize: 10
                    background: Rectangle {
                        color: root.colSecondary
                        border.color: root.colBorder
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "gnomeErrorCopy"
                    pal: root.pal
                    tone: root.toneSecondary
                    enabled: gnomeErrorPopup.text !== ""
                    text: root.gnomeTexts.copy || ""
                    onClicked: {
                        gnomeErrorText.selectAll();
                        gnomeErrorText.copy();
                        gnomeErrorText.deselect();
                    }
                }

                RexonButton {
                    objectName: "gnomeErrorWriteAuthor"
                    pal: root.pal
                    visible: gnomeErrorPopup.url !== ""
                    text: root.gnomeTexts.write_author || ""
                    ToolTip.text: root.gnomeTexts.write_author_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onClicked: {
                        if (root.actions && root.actions.openUrl)
                            root.actions.openUrl(gnomeErrorPopup.url);
                    }
                }
            }
        }

        onOpened: gnomeErrorClose.forceActiveFocus()
    }

    RexonImageViewer {
        id: zoomPopup
        objectName: "imageViewer"
        pal: root.pal
    }

    // ── Backing ───────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // ── Section side panel ────────────────────────────────────────────
        Rectangle {
            Layout.preferredWidth: page.sidebarWidth
            Layout.fillHeight: true
            color: root.colSecondary

            ListView {
                id: sidebar
                anchors.fill: parent
                clip: true
                // The rows arrive as a whole list, and choosing a section sends it again:
                // a direct `model: page.sidebarRows` binding recreated the list and threw
                // the scroll position back to the top — after choosing a section at the
                // bottom, the user lost sight of it (acceptance 10.10.2026). We swap the
                // list ourselves and restore the scroll position.
                property var rows: page.sidebarRows
                onRowsChanged: {
                    const y = sidebar.contentY;
                    sidebar.model = sidebar.rows;
                    Qt.callLater(function () {
                        const bottom = sidebar.contentHeight - sidebar.height;
                        sidebar.contentY = Math.max(0, Math.min(y, bottom));
                    });
                }
                // There are a couple of dozen rows: we keep them all created so the
                // content height is exact right after the list is swapped.
                cacheBuffer: 2000
                Component.onCompleted: sidebar.model = sidebar.rows
                // Sections that do not exist in the selected desktop are removed from the
                // panel: a look that does not work on your desktop is of no use to the
                // customer under any circumstances.
                //
                // The sections are gathered into groups ("Overview", "Desktop "MATE"",
                // "For all programs"…): the group title arrives in the group's first
                // visible row (`groupTitle`), the decision is the core's
                // (`categories::category_group`). A FocusScope wrapper hands focus to the
                // row itself: the gamepad and Tab move between sections, not between
                // titles.
                delegate: FocusScope {
                    id: sidebarBox
                    required property var modelData
                    required property int index
                    width: sidebar.width
                    height: sidebarRow.height + (sidebarHeader.visible ? sidebarHeader.height : 0)
                    visible: sidebarBox.modelData.available

                    Column {
                        id: sidebarHeader
                        visible: (sidebarBox.modelData.groupTitle || "") !== ""
                        width: parent.width
                        topPadding: sidebarBox.index > 0 ? 12 : 4
                        bottomPadding: 4
                        leftPadding: 10
                        rightPadding: 6
                        spacing: 1

                        RexonText {
                            width: parent.width - parent.leftPadding - parent.rightPadding
                            text: sidebarBox.modelData.groupTitle || ""
                            color: root.colTextSecondary
                            font.pointSize: 9
                            font.weight: Font.Bold
                            font.capitalization: Font.AllUppercase
                            wrapMode: Text.Wrap
                        }
                        RexonText {
                            width: parent.width - parent.leftPadding - parent.rightPadding
                            visible: text !== ""
                            text: sidebarBox.modelData.groupNote || ""
                            color: root.colTextSecondary
                            font.pointSize: 8
                            wrapMode: Text.Wrap
                        }
                    }

                ItemDelegate {
                    id: sidebarRow
                    readonly property var modelData: sidebarBox.modelData
                    focus: true
                    y: sidebarHeader.visible ? sidebarHeader.height : 0
                    width: sidebar.width
                    // A long label ("Window Decorations (Aurorae)", "Login screen (Plasma
                    // Login Manager)") wraps onto a second line, and the section row grows to
                    // fit it: cutting a name in the middle of a word would hide what section
                    // it is.
                    height: sidebarRow.modelData.available
                            ? Math.max(page.sidebarRowHeight,
                                       sidebarLabel.implicitHeight + 12)
                            : 0
                    visible: sidebarRow.modelData.available
                    enabled: sidebarRow.modelData.available
                    padding: 0

                    background: Rectangle {
                        color: sidebarRow.modelData.current ? root.colCardHover
                             : (sidebarRow.hovered ? root.colCard : "transparent")
                        // The left strip of the selected section — the same technique as in the
                        // widget tab's style sheet.
                        Rectangle {
                            width: 3
                            height: parent.height
                            color: sidebarRow.modelData.current ? root.colAccent
                                                                : "transparent"
                        }
                        // Warning: `visualFocus`, not `activeFocus`. The list gives focus to the
                        // first row right when shown, and with `activeFocus` a blue frame hung on
                        // «✅ Active» ALWAYS — even when another section was open (screenshots
                        // 01.09.2026). The original draws no such frame at all: its side panel is
                        // `NoFocus`. Removing focus entirely is not allowed — the chain is needed
                        // for gamepad control; `visualFocus` is set only when moving from the
                        // keyboard, not when focus is handed out at start-up.
                        border.color: sidebarRow.visualFocus ? root.colAccent
                                                             : "transparent"
                        border.width: 2
                    }

                    // Section icon and label separately. The wrapped second line of a long
                    // label ("Login screen (Plasma Login Manager)") must start under the first
                    // letter of the label, not under the icon at the panel edge (screenshots
                    // 01.10.2026: wrapping without an indent).
                    contentItem: Item {
                        id: sidebarContent

                        readonly property string full: sidebarRow.modelData.text.trim()
                        readonly property int gap: full.indexOf(" ")
                        // The icon is a short first word without case or digits (for core
                        // sections it is always an emoji); an ordinary word does not look like
                        // that.
                        readonly property string head: gap > 0 ? full.substring(0, gap) : ""
                        readonly property bool hasIcon: gap > 0 && gap <= 4
                            && head.toUpperCase() === head.toLowerCase()
                            && !/[0-9]/.test(head)
                        readonly property color tone:
                            sidebarRow.modelData.current ? root.colAccent
                            : (sidebarRow.hovered ? root.colText
                                                  : root.colTextSecondary)

                        RexonText {
                            id: sidebarIcon
                            visible: sidebarContent.hasIcon
                            // The core's indent (two spaces before the icon) becomes padding.
                            anchors.left: parent.left
                            anchors.leftMargin: 7
                            // The icon sits on the same font line as the FIRST line of the label, as
                            // it did while it was one text with it.
                            y: sidebarLabel.y + sidebarLabel.baselineOffset - baselineOffset
                            text: sidebarContent.head
                            color: sidebarContent.tone
                            font.pointSize: 11
                            font.weight: Font.DemiBold
                        }

                        RexonText {
                            id: sidebarLabel
                            anchors.left: sidebarContent.hasIcon ? sidebarIcon.right : parent.left
                            anchors.leftMargin: sidebarContent.hasIcon ? 5 : 7
                            anchors.right: parent.right
                            anchors.rightMargin: 6
                            anchors.verticalCenter: parent.verticalCenter
                            text: sidebarContent.hasIcon
                                  ? sidebarContent.full.substring(sidebarContent.gap + 1).trim()
                                  : sidebarContent.full
                            wrapMode: Text.Wrap
                            maximumLineCount: 2
                            // The original (`#Sidebar::item:hover`) brightens the label on hover too,
                            // not only the backing.
                            color: sidebarContent.tone
                            font.pointSize: 11
                            // `#Sidebar { font-weight: 600 }` — semi-bold.
                            font.weight: Font.DemiBold
                            elide: Text.ElideRight
                        }
                    }

                    onClicked: page.selectCategory(sidebarRow.modelData.key)
                }
                }
            }
        }

        // ── Right part ────────────────────────────────────────────────────
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            // ── Header ────────────────────────────────────────────────────
            //
            // Order and layout are as in the original: the section name on the left,
            // a spacer, then to the right the source chips, search, desktop, sorting
            // and "refresh".
            //
            // Warning: a `Flow` stood here — for line wrapping at the minimum window
            // width. It did wrap, but at the cost of the look: without a spacer all
            // the controls bunched up to the left of an empty field, and the header no
            // longer resembled the original. The tab's yardstick is parity with the
            // original, and that one is built on a `QHBoxLayout` with a stretch and
            // squeezes the search field the same way in a narrow window.
            //
            // Warning: narrow window (1100 pixels, screenshots 01.10.2026): one line
            // did not fit, its minimum width stretched the whole right part, and both
            // the header and the third column of tiles slid past the right edge. Now
            // the controls are split into two groups — "store + search" and "desktop
            // + sorting + refresh". When there is enough width, everything is on one
            // line, as in the original; when there is not, the second group wraps onto
            // its own line, pressed to the right. The search field shrinks when room
            // is short (the original does the same in a narrow window).
            GridLayout {
                id: headerRow

                /// Height of the header controls. Original: 34 pixels (measured from a
                /// screenshot); the top and bottom paddings come from the core.
                readonly property int rowHeight: 34
                /// Everything fits on one line. The yardstick is the groups' preferred
                /// widths; they do not depend on the number of columns, so there is no
                /// loop.
                readonly property bool oneRow: width >= headerTitle.implicitWidth
                                                        + searchGroup.implicitWidth
                                                        + toolsGroup.fullWidth
                                                        + 2 * columnSpacing

                Layout.fillWidth: true
                Layout.leftMargin: page.headerMarginH
                Layout.rightMargin: page.headerMarginH
                Layout.topMargin: page.headerMarginV
                Layout.bottomMargin: page.headerMarginV
                columns: oneRow ? 3 : 2
                columnSpacing: 8
                rowSpacing: 8

                RexonText {
                    id: headerTitle
                    // Warning: exactly `sectionTitle`, not `headerTitle`. The latter is the
                    // tab name and never changes; the original, however, shows the SELECTED
                    // section with its icon («✅ Active», "📥 Downloaded") from the very first
                    // moment, and the word «Appearance» is visible there for exactly zero
                    // seconds. We kept showing it all the time.
                    text: page.sectionTitle
                    color: root.colText
                    font.pointSize: page.headerTitlePointSize
                    font.bold: true
                    Layout.preferredHeight: headerRow.rowHeight
                    verticalAlignment: Text.AlignVCenter
                }

                RowLayout {
                    id: searchGroup
                    Layout.fillWidth: true
                    spacing: 8

                    // Spacer: in the original there is `addStretch(1)` after the title, and
                    // the whole rest of the row is pressed to the right edge.
                    Item { Layout.fillWidth: true }

                    // «Store:» chips.
                    Row {
                        spacing: 4
                        visible: page.sourceBarVisible
                        height: headerRow.rowHeight

                        RexonText {
                            text: page.sourceLabel
                            color: root.colTextSecondary
                            // Original: `#SrcLabel { font-weight: 600 }` — semi-bold, not bold. The
                            // difference is visible to the eye.
                            font.weight: Font.DemiBold
                            height: headerRow.rowHeight
                            verticalAlignment: Text.AlignVCenter
                        }

                        Repeater {
                            model: page.sourceChips
                            delegate: RexonChip {
                                required property var modelData
                                pal: root.pal
                                text: modelData.label
                                ToolTip.text: modelData.tooltip
                                ToolTip.visible: hovered && modelData.tooltip !== ""
                                ToolTip.delay: 600
                                checked: modelData.checked
                                onClicked: page.setSource(modelData.key)
                            }
                        }
                    }

                    RexonTextField {
                        id: searchBox
                        pal: root.pal
                        // The search field's border comes from the tab's own style sheet
                        // (`#SearchBox { border: 1px solid {border} }`), not from the Qt style:
                        // measurement 05.09 — (42,64,85) in the original versus (73,83,96) of the
                        // style outline.
                        borderTone: root.pal.border
                        visible: page.searchVisible
                        // Warning: exactly `Layout.*`, not `width`/`height`: the layout assigns
                        // the sizes of its direct children, and it silently overrides a bare
                        // `width` — the search field came out narrower than the 240 pixels the
                        // core ordered.
                        Layout.preferredWidth: page.searchWidth
                        // Does not grow wider than the core says; in a narrow window it shrinks
                        // rather than pushing the header past the edge.
                        Layout.fillWidth: true
                        Layout.maximumWidth: page.searchWidth
                        Layout.minimumWidth: 140
                        Layout.preferredHeight: headerRow.rowHeight
                        placeholderText: page.searchPlaceholder
                        text: page.query
                        // Search runs as you type: the disk overview filters immediately, the
                        // store after the core's pause (`searchDelayMs`), Enter immediately.
                        // Clearing the field resets the query IMMEDIATELY — otherwise the query
                        // stuck on the previous word while the field looked empty (a defect of
                        // the original, fixed in the core).
                        rightPadding: searchClear.visible ? searchClear.width + 6 : 10
                        onTextEdited: {
                            page.setQuery(text);
                            if (text.trim() !== "" && page.searchDelayMs > 0)
                                searchDebounce.restart();
                            else
                                searchDebounce.stop();
                            // The store is not bothered per character: below the length threshold the
                            // drop-down simply stays closed, beyond it there is the core's pause.
                            suggestDebounce.restart();
                        }
                        onAccepted: {
                            // A selected suggestion leads straight to the theme card; if none is
                            // selected — an ordinary search.
                            if (page.suggestActivate())
                                return;
                            searchDebounce.stop();
                            page.setQuery(text);
                            page.submitQuery();
                            // The search was sent — the drop-down has done its job: left open, it
                            // covered the header buttons.
                            suggestDebounce.stop();
                            page.updateSuggestions("");
                        }
                        onActiveFocusChanged: page.setSearchFocused(activeFocus)

                        // Warning: typing BREAKS the `text: page.query` binding — that is how the
                        // Qt text field works. So the field would not notice a section change by
                        // itself (the query is reset there) and would stay with a forgotten word
                        // over an unfiltered showcase. A property of our own does not lose the
                        // binding and brings the field back to what the core says.
                        // The comparison is by trimmed text: the core stores the word without
                        // spaces at the ends, and a typed space would otherwise be erased on the
                        // fly.
                        property string coreQuery: page.query
                        onCoreQueryChanged: {
                            if (searchBox.text.trim() !== searchBox.coreQuery) {
                                searchDebounce.stop();
                                searchBox.text = searchBox.coreQuery;
                            }
                        }

                        Keys.onDownPressed: page.suggestMove("down")
                        Keys.onUpPressed: page.suggestMove("up")
                        Keys.onEscapePressed: page.setSearchFocused(false)

                        // Typing pause before a store search. By this moment the word is already
                        // in the core (`setQuery`); all that remains is to apply it.
                        Timer {
                            id: searchDebounce
                            interval: Math.max(1, page.searchDelayMs)
                            repeat: false
                            onTriggered: {
                                if (searchBox.text.trim() === page.query)
                                    page.submitQuery();
                            }
                        }

                        // ✕ "Clear": noticeable while there is something in the field — a
                        // forgotten word in the field must not hide the section.
                        RexonButton {
                            id: searchClear
                            pal: root.pal
                            visible: searchBox.text !== ""
                            text: "✕"
                            width: height
                            height: searchBox.height - 8
                            anchors.right: parent.right
                            anchors.rightMargin: 4
                            anchors.verticalCenter: parent.verticalCenter
                            leftPadding: 0
                            rightPadding: 0
                            focusPolicy: Qt.NoFocus
                            ToolTip.text: page.searchClearTooltip
                            ToolTip.visible: hovered
                            ToolTip.delay: 600
                            onClicked: {
                                searchDebounce.stop();
                                suggestDebounce.stop();
                                searchBox.text = "";
                                page.setQuery("");
                                page.updateSuggestions("");
                            }
                        }

                        Timer {
                            id: suggestDebounce
                            interval: page.suggestDebounceMs
                            repeat: false
                            onTriggered: page.updateSuggestions(searchBox.text)
                        }
                    }
                }

                // The controls are a right-to-left flow: if they do not fit into the line,
                // they wrap rather than pushing the header and the tile grid past the
                // right edge (a 1100-pixel window: «Store:» and the third column slid past
                // the edge). The children's order is the reverse of the on-screen order —
                // that is how a right-to-left flow works.
                Flow {
                    id: toolsGroup
                    /// Width of all controls on one line — the yardstick for `oneRow`.
                    readonly property real fullWidth: {
                        let w = 0, n = 0;
                        for (let i = 0; i < children.length; ++i) {
                            const c = children[i];
                            if (!c.visible)
                                continue;
                            w += c.implicitWidth;
                            n += 1;
                        }
                        return w + Math.max(0, n - 1) * spacing;
                    }
                    // On its own line the group takes both columns.
                    Layout.columnSpan: headerRow.oneRow ? 1 : 2
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.preferredWidth: fullWidth
                    Layout.preferredHeight: implicitHeight
                    // The buttons are pressed to the end of the line. For right-to-left
                    // languages the root mirroring flips this direction too — the buttons go
                    // to the left edge, i.e. again at the end of the line for such a reader.
                    layoutDirection: Qt.RightToLeft
                    spacing: 8

                    RexonButton {
                        pal: root.pal
                        tone: root.toneNav
                        height: headerRow.rowHeight
                        text: page.refreshText
                        ToolTip.text: page.refreshTooltip
                        ToolTip.visible: hovered
                        ToolTip.delay: 600
                        // Axon has its own library: this same button refreshes it; a second "🔄"
                        // in the Axon row is not needed.
                        onClicked: {
                            if (page.screen === "axon" && axonLoader.item)
                                axonLoader.item.reload();
                            else
                                page.refresh();
                        }
                    }

                    RexonButton {
                        id: installFileButton
                        objectName: "installFileButton"
                        pal: root.pal
                        tone: root.toneNav
                        visible: page.installFileVisible
                        enabled: !page.actionBusy
                        height: headerRow.rowHeight
                        text: page.installFileText
                        ToolTip.text: page.installFileTooltip
                        // While the "Choose files / folder" menu is open there is no tooltip: it
                        // landed over the menu, and the button looked broken (acceptance
                        // 10.10.2026).
                        ToolTip.visible: hovered && !installFileMenu.opened
                        ToolTip.delay: 600
                        onClicked: {
                            installFileMenu.parent = installFileButton;
                            // The menu opens from the start of the button's line.
                            installFileMenu.x = root.arrows.placeX(
                                0, installFileMenu.width, installFileButton.width);
                            installFileMenu.y = installFileButton.height;
                            installFileMenu.open();
                        }
                    }

                    RexonComboBox {
                        id: sortCombo
                        pal: root.pal
                        tone: root.toneCombo
                        visible: page.sortVisible
                        height: headerRow.rowHeight
                        model: page.sorts
                        textRole: "label"
                        valueRole: "key"
                        selectedValue: page.sortKey
                        onActivated: page.setSort(currentValue)
                    }

                    Row {
                        spacing: 6
                        height: headerRow.rowHeight
                        visible: deCombo.count > 0

                        RexonText {
                            text: page.deLabel
                            color: root.colTextSecondary
                            font.weight: Font.DemiBold   // `#SrcLabel`, weight 600
                            height: headerRow.rowHeight
                            verticalAlignment: Text.AlignVCenter
                        }

                        RexonComboBox {
                            id: deCombo
                            pal: root.pal
                            tone: root.toneCombo
                            height: headerRow.rowHeight
                            model: page.deChoices
                            textRole: "name"
                            valueRole: "id"
                            ToolTip.text: page.deTooltip
                            ToolTip.visible: hintVisible
                            ToolTip.delay: 600
                            selectedValue: page.de
                            onActivated: {
                                // The value is taken BEFORE setDe: that one rebuilds the list (the
                                // "selected manually" label).
                                const picked = currentValue;
                                page.setDe(picked);
                                // A manual choice survives between launches: without this every visit
                                // showed the desktop detected from the session again. "Detect
                                // automatically" writes an empty choice — from then on the catalogue
                                // follows the session. The second argument is empty: which desktop is
                                // running NOW is known to the host, and it will write it too.
                                if (root.actions && root.actions.remember)
                                    root.actions.remember(picked === "auto" ? "" : picked, "");
                            }
                        }
                    }

                    // «⚙️ Settings» for "Fonts", "Icons" and "Cursors" (08.11.2026): settings
                    // of the same kind are a view inside the section, not a row of their own
                    // in the side panel. The last child of a right-to-left flow is first on
                    // screen, right after the search.
                    // In the settings view the same button leads back to the tiles: the exit
                    // is where the entrance is.
                    RexonButton {
                        id: sectionSettingsButton
                        objectName: "sectionSettingsButton"
                        pal: root.pal
                        tone: root.toneNav
                        visible: page.sectionSettingsVisible
                        height: headerRow.rowHeight
                        text: page.sectionSettingsText
                        ToolTip.text: page.sectionSettingsTooltip
                        ToolTip.visible: hovered && page.sectionSettingsTooltip !== ""
                        ToolTip.delay: 600
                        onClicked: page.toggleSectionSettings()
                    }
                }
            }

            // ── Sub-chips and the status line ─────────────────────────────
            Flow {
                Layout.fillWidth: true
                Layout.leftMargin: page.headerMarginH
                Layout.rightMargin: page.headerMarginH
                Layout.bottomMargin: 4
                spacing: 6

                Row {
                    id: submodeRow
                    spacing: 4
                    visible: page.styleBarVisible || page.lockBarVisible || page.toneBarVisible
                             || page.orientationBarVisible

                    RexonText {
                        text: page.orientationBarVisible ? page.orientationBarLabel : page.submodeLabel
                        color: root.colTextSecondary
                        font.weight: Font.DemiBold   // `#SrcLabel`, weight 600
                        height: headerRow.rowHeight
                        verticalAlignment: Text.AlignVCenter
                    }

                    Repeater {
                        model: page.styleBarVisible ? page.styleChips : []
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setStyleSub(modelData.key)
                        }
                    }

                    Repeater {
                        model: page.lockBarVisible ? page.lockChips : []
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setLockMode(modelData.key)
                        }
                    }

                    // "All / Light / Dark" for colour schemes and Plasma styles (styles also
                    // have "With colour scheme support"), as in KDE.
                    Repeater {
                        model: page.toneBarVisible ? page.toneChips : []
                        delegate: RexonChip {
                            required property var modelData
                            objectName: "toneChip_" + modelData.key
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setToneFilter(modelData.key)
                        }
                    }

                    // "All / Horizontal / Vertical" in a store that knows the
                    // picture size (Wallhaven); the server filters by it.
                    Repeater {
                        model: page.orientationBarVisible ? page.orientationChips : []
                        delegate: RexonChip {
                            required property var modelData
                            objectName: "orientationChip_" + (modelData.key || "all")
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered && modelData.tooltip !== ""
                            ToolTip.delay: 600
                            checked: modelData.checked
                            onClicked: page.setOrientationFilter(modelData.key)
                        }
                    }
                }

                // Cursor size — in the "Cursors" section. Applied over the current theme;
                // how far it reaches in each environment is decided by the core
                // (`themes::apply_cursor_plan_sized`); here there is only the choice.
                Row {
                    id: cursorSizeRow
                    spacing: 4
                    visible: page.cursorSizeBarVisible

                    // The checked size is the host's answer on entering the section.
                    property int current: 0
                    onVisibleChanged: {
                        if (visible && root.actions && root.actions.cursorSize)
                            current = root.actions.cursorSize();
                    }

                    RexonText {
                        text: page.cursorSizeLabel
                        color: root.colTextSecondary
                        font.weight: Font.DemiBold   // `#SrcLabel`, weight 600
                        height: headerRow.rowHeight
                        verticalAlignment: Text.AlignVCenter
                    }

                    Repeater {
                        model: page.cursorSizeChips
                        delegate: RexonChip {
                            required property var modelData
                            pal: root.pal
                            text: modelData.label
                            ToolTip.text: modelData.tooltip
                            ToolTip.visible: hovered
                            ToolTip.delay: 600
                            checked: parseInt(modelData.key, 10) === cursorSizeRow.current
                            onClicked: {
                                if (!root.actions || !root.actions.applyCursorSize)
                                    return;
                                cursorSizeRow.current = parseInt(modelData.key, 10);
                                root.actions.applyCursorSize(cursorSizeRow.current);
                            }
                        }
                    }
                }

                RexonText {
                    text: page.statusText
                    // A result with a caveat ("⚠️ Wallpaper applied: … . …") and an error are
                    // shown in the warning colour, not grey like an ordinary result.
                    color: page.statusWarn && root.pal.state_warn !== undefined
                           ? root.pal.state_warn : root.colTextSecondary
                    // Original: `#StatusBar { font-size: 10pt; font-weight: 600 }`.
                    font.pointSize: page.cardMetaPointSize
                    font.weight: Font.DemiBold
                    // Warning: the height follows the text while there are no sub-chips next
                    // to it. An unconditional 40 pixels used to stand here, and in sections
                    // WITHOUT chips (most of them) the status line stretched the strip to
                    // twice the original: the whole tile grid shifted down by 20 pixels. When
                    // there are chips, we stretch to their height — otherwise the label would
                    // stick to the top instead of the middle, as in the original.
                    height: Math.max(implicitHeight,
                                     submodeRow.visible ? submodeRow.height
                                   : cursorSizeRow.visible ? cursorSizeRow.height
                                                           : 0)
                    verticalAlignment: Text.AlignVCenter
                    // A long refusal reason ("X conflicts with Y…", the path to the log) wraps
                    // inside the strip rather than going past the right edge: the width is no
                    // more than the strip itself.
                    width: Math.min(implicitWidth, parent.width)
                    wrapMode: Text.WordWrap
                }

                // After "Install from file…": the theme turned out to be of another kind
                // ("these are cursors") or is visible in "Downloaded" — open that section
                // with one press.
                RexonButton {
                    objectName: "installFileOpenButton"
                    pal: root.pal
                    tone: root.toneNav
                    visible: root.fileOpenKey !== "" && root.fileOpenText !== ""
                    height: headerRow.rowHeight
                    text: root.fileOpenText
                    onClicked: {
                        const key = root.fileOpenKey;
                        root.fileOpenKey = "";
                        page.selectCategory(key);
                    }
                }
            }

            // ── Section caveat: "it works, but on a condition" ─────────────
            //
            // The section stays in place: hiding something that works instead of
            // naming the condition is not our way (rule #13).
            RexonText {
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 6
                text: page.caveatText
                visible: text !== ""
                color: root.colTextSecondary
                font.pointSize: 9
                wrapMode: Text.WordWrap
            }

            // ── Row of the "Conky widgets" section ────────────────────────
            //
            // The "Running" chip — only widgets with a process right now in the grid;
            // "Stop all" — all conky on the machine, other people's too, after a
            // question with the list (core: `conky_procs`). Labels come from the core.
            RowLayout {
                id: conkyToolsRow
                objectName: "conkyToolsRow"
                readonly property bool active: page.installFileSection === "conky"
                readonly property var texts: {
                    try {
                        return root.actions && root.actions.conkyTexts
                               ? JSON.parse(root.actions.conkyTexts()) : ({});
                    } catch (e) {
                        return ({});
                    }
                }
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: active && page.screen === "grid"
                spacing: 8

                onActiveChanged: {
                    runningChip.checked = false;
                    if (active && root.actions && root.actions.setConkyPreviewScreen)
                        root.actions.setConkyPreviewScreen(
                            Math.round(Screen.width * Screen.devicePixelRatio),
                            Math.round(Screen.height * Screen.devicePixelRatio),
                            Math.round(96 * Screen.devicePixelRatio));
                    if (active && root.actions && root.actions.watchConkyDisk)
                        root.actions.watchConkyDisk();
                }

                RexonChip {
                    id: runningChip
                    objectName: "conkyRunningChip"
                    pal: root.pal
                    checkable: true
                    text: conkyToolsRow.texts.running_chip || ""
                    ToolTip.text: conkyToolsRow.texts.running_chip_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onToggled: page.setRunningOnly(checked)
                }

                Item { Layout.fillWidth: true }

                RexonButton {
                    objectName: "conkyStopAllButton"
                    pal: root.pal
                    tone: root.toneSecondary
                    enabled: !page.actionBusy
                    text: conkyToolsRow.texts.stop_all || ""
                    ToolTip.text: conkyToolsRow.texts.stop_all_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onClicked: {
                        if (root.actions && root.actions.stopAllConky
                                && root.actions.stopAllConky())
                            grid.refreshKeepingScroll();
                    }
                }

                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    // A new widget was unpacked into ~/.conky (or removed) — the "Found on
                    // disk" tiles update by themselves.
                    function onConkyDiskChanged() {
                        if (conkyToolsRow.active && page.screen === "grid")
                            page.showInstalledFresh();
                    }
                    // New widget snapshots were taken for the tiles — re-read without
                    // disturbing the scroll position.
                    function onConkyPreviewsReady() {
                        if (conkyToolsRow.active && page.screen === "grid")
                            grid.refreshKeepingScroll();
                        // The open card (colours or settings changed) — show the fresh snapshot.
                        if (conkyToolsRow.active && page.screen === "detail")
                            page.refreshConkyDetailPicture();
                    }
                }
            }

            // ── Row of the "GNOME extensions" section ─────────────────────
            //
            // "Only for your GNOME version" (on by default; off — the showcase shows
            // everything, the incompatible ones are marked) and updates downloaded by
            // the shell: "Updates downloaded: N. They will apply after logging out" +
            // "Log out" (the confirmation is asked by GNOME itself). Labels and the
            // number come from the core (`gnome_ext_section`).
            RowLayout {
                id: gnomeToolsRow
                objectName: "gnomeToolsRow"
                readonly property bool active: page.installFileSection === "gnomeext"
                property var updates: ({})
                function reloadUpdates() {
                    try {
                        gnomeToolsRow.updates = root.actions && root.actions.gnomeUpdates
                                                ? JSON.parse(root.actions.gnomeUpdates()) : ({});
                    } catch (e) {
                        gnomeToolsRow.updates = ({});
                    }
                }
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: active && page.screen === "grid"
                spacing: 8

                onActiveChanged: {
                    if (active)
                        gnomeToolsRow.reloadUpdates();
                }

                RexonCheckBox {
                    id: gnomeCompatibleBox
                    objectName: "gnomeCompatibleOnly"
                    pal: root.pal
                    // The state belongs to the model: after the page restarts, the check box
                    // shows what the showcase was filtered by.
                    checked: page.egoCompatibleOnly
                    text: root.gnomeTexts.only_compatible || ""
                    ToolTip.text: root.gnomeTexts.only_compatible_tooltip || ""
                    ToolTip.visible: hovered
                    ToolTip.delay: 600
                    onToggled: {
                        page.setEgoCompatibleOnly(checked);
                        // A press breaks the binding — return it to the model.
                        checked = Qt.binding(function () { return page.egoCompatibleOnly; });
                    }
                }

                Item { Layout.fillWidth: true }

                RexonText {
                    objectName: "gnomeUpdatesStatus"
                    Layout.fillWidth: true
                    Layout.maximumWidth: implicitWidth
                    visible: text !== ""
                    text: gnomeToolsRow.updates.text || ""
                    color: root.colText
                    font.weight: Font.DemiBold
                    horizontalAlignment: Text.AlignLeft
                    elide: Text.ElideRight
                }

                RexonButton {
                    objectName: "gnomeLogOut"
                    pal: root.pal
                    visible: (gnomeToolsRow.updates.count || 0) > 0
                    text: gnomeToolsRow.updates.logoutLabel || ""
                    onClicked: {
                        if (root.actions && root.actions.logOutForUpdates)
                            root.actions.logOutForUpdates();
                    }
                }

                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    function onGnomeUpdatesChanged(payload) {
                        try {
                            gnomeToolsRow.updates = JSON.parse(payload);
                        } catch (e) {
                        }
                    }
                    // The section's catalogue answer recomputed the downloaded ones.
                    function onCatalogReady(payload) {
                        if (gnomeToolsRow.active)
                            gnomeToolsRow.reloadUpdates();
                    }
                }
            }

            // ── "On the desktop now" ribbon ───────────────────────────────
            //
            // Running Conky widgets as thumbnails, like gadgets on a Windows desktop:
            // a click opens the widget card (settings, parts, log), the cross removes
            // it from the desktop (the widget stays installed). The list and labels
            // come from the core (`conky_procs::ribbon`).
            ColumnLayout {
                id: conkyRibbon
                objectName: "conkyRibbon"
                property var items: []
                function reload() {
                    try {
                        conkyRibbon.items = root.actions && root.actions.conkyRibbon
                                            ? JSON.parse(root.actions.conkyRibbon()) : [];
                    } catch (e) {
                        conkyRibbon.items = [];
                    }
                }
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyToolsRow.visible && conkyRibbon.items.length > 0
                spacing: 4
                onVisibleChanged: if (conkyToolsRow.visible) conkyRibbon.reload()
                Component.onCompleted: conkyRibbon.reload()

                RexonText {
                    text: conkyToolsRow.texts.ribbon_title || ""
                    color: root.colText
                    font.bold: true
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: conkyRibbon.items
                        delegate: Rectangle {
                            id: gadget
                            required property var modelData
                            objectName: "conkyGadget"
                            width: 132
                            height: 104
                            radius: 4
                            color: gadgetHover.hovered ? root.colCardHover : root.colCard
                            HoverHandler { id: gadgetHover }
                            ToolTip.text: conkyToolsRow.texts.ribbon_open_tooltip || ""
                            ToolTip.visible: gadgetHover.hovered
                            ToolTip.delay: 600
                            TapHandler { onTapped: page.openDetail(gadget.modelData.item_id) }
                            Image {
                                id: gadgetPicture
                                anchors.fill: parent
                                anchors.margins: 6
                                anchors.bottomMargin: 24
                                fillMode: Image.PreserveAspectFit
                                asynchronous: true
                                sourceSize.width: 240
                                source: gadget.modelData.picture !== ""
                                        ? "file://" + encodeURI(gadget.modelData.picture) : ""
                            }
                            RexonText {
                                anchors.centerIn: gadgetPicture
                                visible: gadgetPicture.status !== Image.Ready
                                text: "📊"
                                font.pixelSize: 32
                            }
                            RexonText {
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: 6
                                text: gadget.modelData.title
                                color: root.colText
                                elide: Text.ElideRight
                                horizontalAlignment: Text.AlignHCenter
                            }
                            RexonButton {
                                objectName: "conkyGadgetStop"
                                anchors.top: parent.top
                                anchors.right: parent.right
                                anchors.margins: 2
                                implicitWidth: 26
                                implicitHeight: 26
                                pal: root.pal
                                enabled: !page.actionBusy
                                text: "✕"
                                ToolTip.text: conkyToolsRow.texts.ribbon_stop_tooltip || ""
                                ToolTip.visible: hovered
                                ToolTip.delay: 400
                                onClicked: {
                                    if (root.actions && root.actions.stopConkyRibbonItem
                                            && root.actions.stopConkyRibbonItem(gadget.modelData.item_id))
                                        grid.refreshKeepingScroll();
                                }
                            }
                        }
                    }
                }
                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    function onActionDone(text, ok) {
                        if (conkyToolsRow.visible)
                            conkyRibbon.reload();
                    }
                }
            }

            // ── Conky widget previews: no Xvfb ────────────────────────────
            //
            // One line per section, not per tile: previews are not drawn because there
            // is no invisible X server. The button installs the package through the
            // shared missing-programs path (the password window), ✕ means do not show
            // again. Decision, text and package come from the core
            // (`conky_preview::xvfb_hint`).
            Rectangle {
                id: conkyXvfbBar
                objectName: "conkyXvfbBar"

                property int rev: 0
                property bool busy: false
                readonly property var info: {
                    conkyToolsRow.active; conkyXvfbBar.rev;
                    try {
                        return conkyToolsRow.active && root.actions && root.actions.conkyXvfbHint
                               ? JSON.parse(root.actions.conkyXvfbHint()) : ({});
                    } catch (e) {
                        return ({});
                    }
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyToolsRow.visible && conkyXvfbBar.info.visible === true
                implicitHeight: conkyXvfbRow.implicitHeight + 16
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                RowLayout {
                    id: conkyXvfbRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: 8
                    spacing: 8

                    RexonText {
                        Layout.fillWidth: true
                        text: "ℹ️ " + (conkyXvfbBar.info.text || "")
                        color: root.colTextSecondary
                        font.pointSize: 9
                        wrapMode: Text.WordWrap
                    }

                    RexonButton {
                        objectName: "conkyXvfbInstallButton"
                        pal: root.pal
                        visible: (conkyXvfbBar.info.button || "") !== ""
                        enabled: !conkyXvfbBar.busy
                        text: conkyXvfbBar.info.button || ""
                        onClicked: {
                            conkyXvfbBar.busy = true;
                            root.actions.installConkyXvfb();
                        }
                    }

                    RexonButton {
                        objectName: "conkyXvfbCloseButton"
                        pal: root.pal
                        text: "✕"
                        focusPolicy: Qt.NoFocus
                        ToolTip.text: conkyXvfbBar.info.close_tooltip || ""
                        ToolTip.visible: hovered
                        ToolTip.delay: 600
                        onClicked: {
                            root.actions.closeConkyXvfbHint();
                            conkyXvfbBar.rev++;
                        }
                    }
                }

                Connections {
                    target: root.actions
                    ignoreUnknownSignals: true
                    function onActionDone(text, ok) {
                        if (conkyXvfbBar.busy) {
                            conkyXvfbBar.busy = false;
                            conkyXvfbBar.rev++;
                        }
                    }
                }
            }

            // ── Screen mini map ───────────────────────────────────────────
            //
            // Running Conky widgets on a diagram of the screen: drag to move them on
            // the desktop (`conky_layout`, written as the widget's overlay).
            ConkyScreenMap {
                id: conkyScreenMap
                objectName: "conkyScreenMap"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyRibbon.visible
                pal: root.pal
                actions: root.actions
                busy: page.actionBusy
            }

            // ── Conky widget sets ─────────────────────────────────────────
            //
            // Several widgets and a wallpaper with one button; "Save running ones as a
            // set". Sets from the .cmtheme files of theme packages are visible at
            // once. Decisions and labels come from the core (`conky_sets`).
            ConkySets {
                id: conkySets
                objectName: "conkySets"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: conkyToolsRow.visible
                pal: root.pal
                actions: root.actions
                busy: page.actionBusy
                onVisibleChanged: if (visible) conkySets.reload()
                onChanged: grid.refreshKeepingScroll()
            }

            // ── System sounds are off ─────────────────────────────────────
            //
            // In the "Sounds" section, with event sounds turned off any theme is
            // silent — we say so right away and offer to turn them on with one button.
            // Decision and labels come from the core (`ap::event_sounds_banner`).
            Rectangle {
                id: eventSoundsBar
                objectName: "eventSoundsBar"

                property int rev: 0
                property string message: ""
                readonly property var info: {
                    // Recompute on a section or desktop change.
                    page.sectionTitle; page.caveatText; eventSoundsBar.rev;
                    try {
                        return JSON.parse(page.eventSoundsBanner());
                    } catch (e) {
                        return ({});
                    }
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: eventSoundsBar.info.visible === true || eventSoundsBar.message !== ""
                implicitHeight: eventSoundsRow.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                RowLayout {
                    id: eventSoundsRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: 10
                    spacing: 10

                    RexonText {
                        Layout.fillWidth: true
                        text: eventSoundsBar.info.visible === true
                              ? "🔇 " + (eventSoundsBar.info.text || "")
                              : eventSoundsBar.message
                        color: root.colText
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    RexonButton {
                        objectName: "eventSoundsOnButton"
                        pal: root.pal
                        visible: eventSoundsBar.info.visible === true
                        text: eventSoundsBar.info.button || ""
                        onClicked: {
                            eventSoundsBar.message = page.enableEventSounds();
                            eventSoundsBar.rev++;
                        }
                    }
                }

                Connections {
                    target: page
                    function onSidebarChanged() { eventSoundsBar.message = ""; }
                    function onDeChanged() { eventSoundsBar.rev++; }
                }
            }

            // ── Shell palette ─────────────────────────────────────────────
            //
            // The strip is visible ONLY with a live foreign shell (Noctalia,
            // DankMaterialShell): it sets the dark/light mode for applications by
            // itself, and the conflict with it must be named, not hidden (rule #13).
            // Decision, text and labels come from the core
            // (`shell_palette::PaletteBanner` through `actions.shellPalette()`); here
            // there is only the layout and three buttons. The shell may be started and
            // stopped while the window is open — hence an infrequent timer.
            Rectangle {
                id: shellPaletteBar
                objectName: "shellPaletteBar"

                property var info: ({})
                property string message: ""
                property bool failed: false
                property bool busy: false

                function refresh() {
                    if (!root.actions || !root.actions.shellPalette)
                        return;
                    try {
                        shellPaletteBar.info = JSON.parse(root.actions.shellPalette());
                    } catch (e) {
                        shellPaletteBar.info = ({});
                    }
                }

                function act(name) {
                    if (!root.actions || !root.actions[name] || shellPaletteBar.busy)
                        return;
                    shellPaletteBar.busy = true;
                    shellPaletteBar.message = "";
                    root.actions[name]();
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: shellPaletteBar.info.visible === true
                implicitHeight: shellPaletteColumn.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                Timer {
                    interval: 15000
                    repeat: true
                    running: root.started && root.visible
                    triggeredOnStart: true
                    onTriggered: shellPaletteBar.refresh()
                }

                ColumnLayout {
                    id: shellPaletteColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 10
                    spacing: 8

                    RexonText {
                        Layout.fillWidth: true
                        text: "🎨 " + (shellPaletteBar.info.text || "")
                        color: root.colText
                        font.pointSize: 10
                        wrapMode: Text.WordWrap
                    }

                    Flow {
                        Layout.fillWidth: true
                        spacing: 6

                        RexonText {
                            text: shellPaletteBar.info.lead_caption || ""
                            color: root.colTextSecondary
                            font.weight: Font.DemiBold
                            height: 32
                            verticalAlignment: Text.AlignVCenter
                        }

                        Repeater {
                            model: [
                                { key: "rexon", label: shellPaletteBar.info.lead_rexon_label || "" },
                                { key: "shell", label: shellPaletteBar.info.lead_shell_label || "" }
                            ]
                            delegate: RexonChip {
                                required property var modelData
                                pal: root.pal
                                text: modelData.label
                                checked: shellPaletteBar.info.lead === modelData.key
                                onClicked: {
                                    if (root.actions && root.actions.setPaletteLead
                                            && root.actions.setPaletteLead(modelData.key))
                                        shellPaletteBar.refresh();
                                }
                            }
                        }

                        Item { width: 12; height: 1 }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.push_label || ""
                            enabled: shellPaletteBar.info.can_push === true && !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("pushShellPalette")
                        }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.pull_label || ""
                            enabled: !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("pullShellPalette")
                        }

                        RexonButton {
                            pal: root.pal
                            text: shellPaletteBar.info.revert_label || ""
                            visible: shellPaletteBar.info.can_revert === true
                            enabled: !shellPaletteBar.busy
                            onClicked: shellPaletteBar.act("revertShellPalette")
                        }
                    }

                    RexonText {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: shellPaletteBar.message
                        color: shellPaletteBar.failed
                               ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                    : root.colAccent)
                               : root.colTextSecondary
                        font.pointSize: 9
                        wrapMode: Text.WordWrap
                    }
                }
            }

            // ── Restore as it was ─────────────────────────────────────────
            //
            // A successful "Apply" left a snapshot of what was there before it — the
            // section has one button to restore that. For colours, while the "Shell
            // palette" strip button is visible, there is no separate one (that one
            // restores both the shell and the previous scheme). Decision and labels
            // come from the core (`ap::applied_revert_banner`, restoring via
            // `revert_applied_section`).
            Rectangle {
                id: appliedRevertBar
                objectName: "appliedRevertBar"

                property int rev: 0
                property string message: ""
                property bool failed: false
                // The section the result belongs to: after a successful restore the
                // section is re-read (the same signal as for a section change), and the
                // result must stay — it leaves only in another section.
                property string messageSection: ""
                readonly property bool showMessage: appliedRevertBar.message !== ""
                        && appliedRevertBar.info.section === appliedRevertBar.messageSection
                readonly property var info: {
                    // Recompute on a section change and after presses.
                    page.sectionTitle; page.caveatText; appliedRevertBar.rev;
                    try {
                        return JSON.parse(page.appliedRevertBanner());
                    } catch (e) {
                        return ({});
                    }
                }

                function revert() {
                    if (!root.actions || !root.actions.revertApplied)
                        return;
                    let out = ({});
                    try {
                        out = JSON.parse(root.actions.revertApplied(appliedRevertBar.info.section || ""));
                    } catch (e) {
                        out = ({});
                    }
                    appliedRevertBar.messageSection = appliedRevertBar.info.section || "";
                    appliedRevertBar.message = out.text || "";
                    appliedRevertBar.failed = out.ok !== true;
                    appliedRevertBar.rev++;
                    if (out.ok === true)
                        page.reloadLocalFresh();
                }

                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: appliedRevertBar.info.visible === true || appliedRevertBar.showMessage
                implicitHeight: appliedRevertColumn.implicitHeight + 20
                radius: 6
                color: root.colCard
                border.color: root.colBorder
                border.width: 1

                ColumnLayout {
                    id: appliedRevertColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: 10
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true
                        visible: appliedRevertBar.info.visible === true
                        spacing: 10

                        RexonText {
                            Layout.fillWidth: true
                            text: "↩️ " + (appliedRevertBar.info.text || "")
                            color: root.colText
                            font.pointSize: 10
                            wrapMode: Text.WordWrap
                        }

                        RexonButton {
                            objectName: "appliedRevertButton"
                            pal: root.pal
                            text: appliedRevertBar.info.button || ""
                            enabled: !page.actionBusy
                            onClicked: appliedRevertBar.revert()
                        }
                    }

                    RexonText {
                        Layout.fillWidth: true
                        visible: appliedRevertBar.showMessage
                        text: appliedRevertBar.message
                        color: appliedRevertBar.failed
                               ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                    : root.colAccent)
                               : root.colTextSecondary
                        font.pointSize: 9
                        wrapMode: Text.WordWrap
                    }
                }
            }

            // ── Login manager ─────────────────────────────────────────────
            //
            // In the login screen section (the SDDM theme shelf and the Plasma Login
            // Manager panel): installed login screens, the default one and "Make
            // default". Data come from the login screen probe, texts from the core.
            LoginManagerSection {
                id: loginManagerSection
                objectName: "loginManagerSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                // Here the block stands only above the explanation (`note`). Above the
                // theme grid it is the header of the grid itself (`grid.header`) and
                // scrolls with the tiles; with Plasma Login Manager it is in a shared flow
                // with the explanation (`plmView`).
                visible: page.loginSection && page.screen === "note"
                         && (loginManagerSection.managers.length > 0 || loginManagerSection.sessionOffer !== null)
                pal: root.pal
                sty: page.sectionStyles
                actions: root.actions
            }

            // ── Accent colour above the explanation ───────────────────────
            //
            // Where it stands and whether it is available here is decided by the core
            // (`accent`). Here the block stands only above the explanation
            // (`on_note`). Above the scheme grid it is the header of the grid itself
            // (`grid.header`) together with "Light and dark" and "Save current look…"
            // and scrolls with the tiles.
            AccentSection {
                id: accentSection
                objectName: "accentSection"
                Layout.fillWidth: true
                Layout.leftMargin: 16
                Layout.rightMargin: 16
                Layout.bottomMargin: 8
                visible: page.installFileSection === (accentSection.st.section || "-")
                         && accentSection.st.available === true
                         && page.screen === "note" && accentSection.st.on_note === true
                pal: root.pal
                sty: page.sectionStyles
            }

            // ── Tile grid or explanation ──────────────────────────────────
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                // The login screen is drawn by Plasma Login Manager: it has no themes, so
                // there is no shelf — an explanation and a way into its settings. All
                // labels come from the core (`plasma_login_panel`).
                //
                // The "Login manager" block and the explanation are one scrollable flow:
                // in a low window (1100×750) the explanation, centred in the remaining
                // height, climbed up over the list, and the page did not scroll. Now, when
                // height is short, the scroll area grows; when there is room, the
                // explanation is still centred.
                ScrollView {
                    id: plmView
                    objectName: "plasmaLoginView"
                    anchors.fill: parent
                    visible: page.screen === "plasmalogin"
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: plmView.availableWidth
                        height: Math.max(implicitHeight, plmView.availableHeight)
                        spacing: 0

                        LoginManagerSection {
                            id: plmLoginManagerSection
                            objectName: "plasmaLoginManagerSection"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.bottomMargin: 8
                            visible: page.loginSection && (plmLoginManagerSection.managers.length > 0
                                                           || plmLoginManagerSection.sessionOffer !== null
                                                           || plmLoginManagerSection.greeter !== null)
                            pal: root.pal
                            sty: page.sectionStyles
                            actions: root.actions
                        }

                        Item {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            Layout.minimumHeight: plmColumn.implicitHeight + 32
                            implicitHeight: plmColumn.implicitHeight + 32

                            ColumnLayout {
                                id: plmColumn
                                objectName: "plasmaLoginColumn"
                                anchors.centerIn: parent
                                width: Math.min(parent.width - 64, 720)
                                spacing: 14

                                RexonText {
                                    Layout.fillWidth: true
                                    text: page.plmTitle
                                    color: root.colText
                                    font.pointSize: 16
                                    font.bold: true
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }

                                RexonText {
                                    Layout.fillWidth: true
                                    text: page.plmBody
                                    color: root.colTextSecondary
                                    font.pointSize: 14
                                    lineHeight: 1.4
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }

                                RexonButton {
                                    id: plmOpenButton
                                    Layout.alignment: Qt.AlignHCenter
                                    Layout.topMargin: 6
                                    visible: page.plmOpenVisible
                                    pal: root.pal
                                    font.bold: true
                                    text: page.plmOpenLabel
                                    onClicked: {
                                        const ok = root.actions && root.actions.openLoginScreenSettings
                                                   ? root.actions.openLoginScreenSettings() : false;
                                        if (!ok)
                                            page.loginSettingsFailed();
                                    }
                                }

                                RexonText {
                                    Layout.fillWidth: true
                                    Layout.topMargin: 6
                                    text: page.plmStoreHint
                                    color: root.colTextSecondary
                                    font.pointSize: 11
                                    font.italic: true
                                    horizontalAlignment: Text.AlignHCenter
                                    wrapMode: Text.WordWrap
                                }
                            }
                        }
                    }
                }

                // "MATE panel": ready mate-panel layouts. The section is self-contained
                // (`MatePanelSection`, its own confirmation window) and fills the
                // screen: its buttons and the result of "Apply" stay pinned at the top,
                // only the list of layouts scrolls inside it.
                MatePanelSection {
                    anchors.fill: parent
                    anchors.margins: 16
                    shown: page.screen === "matepanel"
                    pal: root.pal
                    sty: page.sectionStyles
                }

                // Settings sections (animation, title bar buttons, the «⚙️ Settings» view
                // of fonts and icons…) — the same ones that were in the "Settings" tab, as
                // its page in single-section mode (`SettingsPage.panel`). Built on the
                // first visit and lives on afterwards; the section key is remembered while
                // the screen is not its own, so that the hidden page does not rebuild into
                // the full tab.
                Loader {
                    id: settingsPanelLoader
                    objectName: "settingsPanelLoader"
                    anchors.fill: parent
                    visible: page.screen === "settings"
                    // Built on the first visit and lives on afterwards. Warning: not by
                    // `onVisibleChanged`: a page opened by a link from "Settings" is built
                    // asynchronously already on this screen, and the loader does not see a
                    // visibility change — the section stayed empty.
                    active: page.screen === "settings" || settingsPanelLoader.built
                    asynchronous: true
                    property bool built: false
                    property string panel: ""
                    onLoaded: settingsPanelLoader.built = true

                    Binding {
                        target: settingsPanelLoader
                        property: "panel"
                        value: page.settingsPanel
                        when: page.screen === "settings"
                        restoreMode: Binding.RestoreNone
                    }

                    sourceComponent: Component {
                        SettingsPage {
                            panel: settingsPanelLoader.panel
                            lookDe: page.de
                        }
                    }
                }

                // The "Razer Axon" source of the wallpaper section — the Axon library on
                // this computer instead of the store grid (`SCREEN_AXON`). Built on the
                // first choice of the chip and lives on afterwards; on every visit it
                // re-reads the library by itself.
                Loader {
                    id: axonLoader
                    objectName: "axonLoader"
                    anchors.fill: parent
                    visible: page.screen === "axon"
                    active: page.screen === "axon" || axonLoader.built
                    asynchronous: true
                    property bool built: false
                    onLoaded: axonLoader.built = true

                    sourceComponent: Component {
                        AxonLibraryView {
                            pal: root.pal
                            layout: page
                            // The word comes from the shared search field of the header.
                            query: page.query
                            onMessage: text => page.setStatus(text)
                            onSearchCleared: page.setQuery("")
                        }
                    }
                }

                // An empty screen without an explanation is the worst thing that can be
                // shown to a person (rule #13). The text is assembled by the core from the
                // customization matrix.
                RexonText {
                    anchors.centerIn: parent
                    width: parent.width - 64
                    visible: page.screen === "note"
                    text: page.noteText
                    color: root.colTextSecondary
                    font.pointSize: 14
                    lineHeight: 1.5
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                }

                // ── Details panel ─────────────────────────────────────
                //
                // A separate screen, not a window: in the showcase an item opens in place,
                // and going back takes one button.
                ScrollView {
                    id: detailView
                    anchors.fill: parent
                    visible: page.screen === "detail"
                    clip: true
                    contentWidth: availableWidth

                    ColumnLayout {
                        width: detailView.availableWidth
                        // A layout inside an ORDINARY element gets its height from nobody and
                        // collapses into a strip — we set the height ourselves (trap 7 of the
                        // porting scheme).
                        height: implicitHeight
                        spacing: 10

                        RexonButton {
                            Layout.margins: 16
                            pal: root.pal
                            tone: root.toneNav
                            // The "←" arrow points to the start of the line: for right to left the
                            // button is on the right, and the arrow points right.
                            text: root.arrows.keyHint(page.detailBackText)
                            onClicked: page.closeDetail()
                        }

                        // Item picture. It opens the full-screen view — the gallery overlay
                        // controls arrive in the next slice.
                        Rectangle {
                            Layout.alignment: Qt.AlignHCenter
                            Layout.preferredWidth: page.detailPreviewWidth
                            Layout.preferredHeight: page.detailPreviewHeight
                            color: root.colSecondary
                            radius: 4

                            // GIF and animated WebP (the first picture of cursors is often like that)
                            // play; everything else is an ordinary picture. The core decides by the
                            // first bytes of the file: a cached name may have no extension.
                            readonly property bool previewAnimated:
                                page.isAnimatedPreview(page.detailPreview)

                            Image {
                                anchors.fill: parent
                                source: parent.previewAnimated ? "" : page.detailPreview
                                fillMode: Image.PreserveAspectFit
                                asynchronous: true
                                visible: source !== ""
                            }

                            AnimatedImage {
                                anchors.fill: parent
                                source: parent.previewAnimated ? page.detailPreview : ""
                                fillMode: Image.PreserveAspectFit
                                visible: source !== ""
                                // Frames are not cycled while the card is not visible or a full-screen
                                // view is open on top.
                                // Warning: do not read our own `visible` here: on page creation it gave a
                                // "playing" binding loop (log r1434). The picture is visible exactly when
                                // it has a source — that is what we ask.
                                playing: parent.previewAnimated && detailView.visible
                                    && !zoomPopup.opened
                            }

                            RexonText {
                                anchors.centerIn: parent
                                width: parent.width - 32
                                visible: page.detailPreview === ""
                                text: page.detailPreviewText
                                color: root.colTextSecondary
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.WordWrap
                            }

                            // The picture opens the large view: a magnifier under the cursor, a
                            // click, Enter — the shared entry point (`RexonZoomArea.qml`), as in the
                            // other tabs.
                            RexonZoomArea {
                                anchors.fill: parent
                                pal: root.pal
                                radius: parent.radius
                                tooltip: page.detailPreviewTooltip
                                enabled: page.detailPreview !== ""
                                visible: enabled
                                onActivated: root.openZoom()
                            }

                            // ── Carousel overlay controls ──────────────
                            // The place of each is computed by the core (overlay_*_pos): the markup
                            // has no margins of its own.
                            RexonButton {
                                pal: root.pal
                                visible: page.galleryNavVisible
                                width: page.overlayButtonSize
                                height: page.overlayButtonSize
                                anchors.left: parent.left
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.margins: page.overlayMargin
                                // The "◀️ ▶️" icons come from the core and do not flip by themselves,
                                // while mirroring swaps the buttons: for right to left "back" is on the
                                // right and points right.
                                text: root.arrows.mirrored ? page.overlayNextText
                                                           : page.overlayPrevText
                                ToolTip.text: page.overlayPrevTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.galleryPrev()
                            }

                            RexonButton {
                                pal: root.pal
                                visible: page.galleryNavVisible
                                width: page.overlayButtonSize
                                height: page.overlayButtonSize
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                anchors.margins: page.overlayMargin
                                text: root.arrows.mirrored ? page.overlayPrevText
                                                           : page.overlayNextText
                                ToolTip.text: page.overlayNextTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.galleryNext()
                            }

                            // The "2/5" counter: visible only when there is more than one frame (the
                            // core's decision).
                            Rectangle {
                                visible: page.galleryCounter !== ""
                                anchors.horizontalCenter: parent.horizontalCenter
                                anchors.bottom: parent.bottom
                                anchors.margins: page.overlayMargin
                                width: counterText.implicitWidth + 20
                                height: 24
                                radius: 12
                                color: root.colSecondary
                                opacity: 0.85

                                RexonText {
                                    id: counterText
                                    anchors.centerIn: parent
                                    text: page.galleryCounter
                                    color: root.colText
                                    font.bold: true
                                }
                            }
                        }

                        RexonText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            text: page.detailName
                            color: root.colText
                            font.pointSize: 15
                            font.bold: true
                            wrapMode: Text.WordWrap
                        }

                        RexonText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            text: page.detailMeta
                            color: root.colTextSecondary
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                        }

                        RowLayout {
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            spacing: 0
                            visible: page.detailStars !== ""

                            RexonText {
                                text: page.detailStars
                                color: root.colStar
                                font.bold: true
                                textFormat: Text.PlainText
                            }

                            RexonText {
                                text: page.detailDownloads
                                color: root.colText
                                font.bold: true
                                textFormat: Text.PlainText
                            }
                        }

                        // Actions. The set depends on where the item comes from: a local theme
                        // has no store at all.
                        Flow {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            spacing: 8

                            // A GNOME extension that the shell knows (core JSON, `detailGnome`):
                            // instead of «Install»/"Apply" — its buttons below.
                            QtObject {
                                id: detailGnome
                                readonly property var st: {
                                    try {
                                        return JSON.parse(page.detailGnome || "{}");
                                    } catch (e) {
                                        return ({});
                                    }
                                }
                                readonly property bool on: st.visible === true
                            }

                            Button {
                                id: installBtn
                                visible: !page.detailIsLocal && !detailGnome.on
                                readonly property bool isAur: page.detailSource === "aur"
                                // The AUR package button — exactly the one on its tile (the installation
                                // queue). The revision re-reads it on every queue snapshot.
                                property int aurRev: 0
                                readonly property var aurBtn: (aurRev, page.count,
                                                               page.detailItemId, isAur)
                                                              ? page.detailAurButton() : ({})
                                Connections {
                                    target: root.actions
                                    ignoreUnknownSignals: true
                                    function onAurJobsChanged() { installBtn.aurRev++; }
                                }
                                // While an action is running, starting is locked: a second press on top of
                                // the first speeds nothing up.
                                enabled: isAur ? aurBtn.enabled === true : !page.actionBusy
                                implicitHeight: 40
                                leftPadding: 14
                                rightPadding: 14
                                // A file other than the automatic one was chosen by hand: the button
                                // installs the chosen one, even for an installed item.
                                readonly property bool otherFile: !isAur && detailFiles.visible
                                                                  && detailFiles.differs
                                text: isAur ? (aurBtn.text || "")
                                            : (otherFile ? page.detailFileInstallText
                                                         : page.detailInstallText)
                                readonly property string tip: isAur ? (aurBtn.tooltip || "")
                                                                    : (otherFile ? ""
                                                                                 : page.detailInstallTooltip)
                                ToolTip.text: tip
                                ToolTip.visible: hovered && tip !== ""
                                ToolTip.delay: 600
                                onClicked: {
                                    // The same shared entry point as the tile button.
                                    if (isAur) {
                                        root.actOnCard("aur", page.detailItemId);
                                        installBtn.aurRev++;
                                        return;
                                    }
                                    if (otherFile) {
                                        page.actionBegin(page.detailItemId, "install");
                                        if (root.actions && root.actions.installById)
                                            root.actions.installById(page.detailItemId, false,
                                                                     detailFiles.selectedName);
                                        return;
                                    }
                                    root.actOnCard("theme", page.detailItemId);
                                }

                                readonly property var colors:
                                    page.detailButtonColors(isAur ? (aurBtn.state || "")
                                                                  : (otherFile ? "false"
                                                                               : page.detailInstallState))

                                background: Rectangle {
                                    radius: 3
                                    color: installBtn.colors.bg === ""
                                           ? "transparent" : installBtn.colors.bg
                                    border.color: installBtn.activeFocus
                                                  ? root.colAccent
                                                  : (installBtn.colors.border === ""
                                                     ? "transparent"
                                                     : installBtn.colors.border)
                                    border.width: installBtn.activeFocus ? 2 : 1
                                }

                                contentItem: RexonText {
                                    text: installBtn.text
                                    color: installBtn.colors.fg
                                    font.bold: true
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                }
                            }

                            // Risk signs of an AUR package — next to «Install», not instead of it:
                            // they do not forbid installation. The list is in the tooltip and in
                            // "Source check".
                            RexonText {
                                objectName: "aurTrustBadge"
                                visible: page.detailSource === "aur" && page.aurTrustBadge !== ""
                                height: 40
                                verticalAlignment: Text.AlignVCenter
                                leftPadding: 4
                                rightPadding: 4
                                text: page.aurTrustBadge
                                color: page.aurTrustLevel === "danger" ? page.aurTrustDangerColor
                                                                       : root.colStar
                                font.bold: true
                                ToolTip.text: page.aurTrustTooltip
                                ToolTip.visible: trustBadgeHover.hovered
                                ToolTip.delay: 300
                                HoverHandler { id: trustBadgeHover }
                            }

                            // For a theme from disk "Delete" may be unavailable (enabled, system) —
                            // the reason is in a tooltip; a disabled button does not catch hover, so
                            // the wrapper catches it.
                            Item {
                                visible: page.detailUninstallVisible && !detailGnome.on
                                implicitWidth: uninstallButton.implicitWidth
                                implicitHeight: uninstallButton.implicitHeight

                                RexonDangerButton {
                                    id: uninstallButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    enabled: page.detailUninstallEnabled
                                    text: page.detailUninstallText
                                    onClicked: {
                                        if (root.actions && root.actions.uninstall)
                                            root.actions.uninstall(page.detailItemJson());
                                    }
                                }

                                HoverHandler { id: uninstallHover }
                                ToolTip.text: page.detailUninstallTooltip
                                ToolTip.visible: uninstallHover.hovered
                                                 && page.detailUninstallTooltip !== ""
                                ToolTip.delay: 400
                            }

                            RexonButton {
                                visible: !page.detailIsLocal
                                pal: root.pal
                                // The site is secondary: for an AUR package the whole card is already
                                // here.
                                tone: page.detailSource === "aur" ? root.toneSecondary
                                                                  : null
                                text: page.detailSource === "aur" ? page.aurOpenText
                                                                  : page.detailSiteText
                                ToolTip.text: page.detailSource === "aur" ? page.aurOpenTooltip
                                                                          : page.detailSiteTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: {
                                    const url = page.detailSiteUrl();
                                    if (url && root.actions && root.actions.openUrl)
                                        root.actions.openUrl(url);
                                }
                            }

                            RexonButton {
                                visible: !page.detailIsLocal
                                pal: root.pal
                                text: page.detailCopyLinkText
                                ToolTip.text: page.detailCopyLinkTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: page.copyDetailLink()
                            }

                            RexonButton {
                                visible: page.detailIsLocal && !detailGnome.on
                                pal: root.pal
                                text: page.detailApplyLocalText
                                enabled: page.detailApplyLocalEnabled
                                         && !page.actionBusy
                                onClicked: root.applyLocal(page.detailItemId)
                            }

                            // ── GNOME extension buttons ──────────────────
                            // "Enable/Disable" — according to the shell's truth (one locked by the
                            // administrator is disabled, the reason in a tooltip caught by the
                            // wrapper), "Settings", "Open folder" for your own or "Copy path" for a
                            // system one, "Error details", "Delete" only for your own.
                            Item {
                                objectName: "detailGnomeToggle"
                                visible: detailGnome.on
                                implicitWidth: gnomeToggleBtn.implicitWidth
                                implicitHeight: gnomeToggleBtn.implicitHeight

                                Button {
                                    id: gnomeToggleBtn
                                    anchors.fill: parent
                                    readonly property var t: detailGnome.st.toggle || ({})
                                    enabled: t.enabled === true && !page.actionBusy
                                    implicitHeight: 40
                                    leftPadding: 14
                                    rightPadding: 14
                                    text: t.text || ""
                                    onClicked: root.gnomeAction(page.detailItemId, "toggle")

                                    readonly property var colors: page.detailButtonColors(t.state || "")

                                    background: Rectangle {
                                        radius: 3
                                        color: gnomeToggleBtn.colors.bg === ""
                                               ? "transparent" : gnomeToggleBtn.colors.bg
                                        border.color: gnomeToggleBtn.activeFocus
                                                      ? root.colAccent
                                                      : (gnomeToggleBtn.colors.border === ""
                                                         ? "transparent"
                                                         : gnomeToggleBtn.colors.border)
                                        border.width: gnomeToggleBtn.activeFocus ? 2 : 1
                                    }

                                    contentItem: RexonText {
                                        text: gnomeToggleBtn.text
                                        color: gnomeToggleBtn.colors.fg
                                        font.bold: true
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                    }
                                }

                                HoverHandler { id: gnomeToggleHover }
                                ToolTip.text: detailGnome.st.tooltip || ""
                                ToolTip.visible: gnomeToggleHover.hovered
                                                 && (detailGnome.st.tooltip || "") !== ""
                                ToolTip.delay: 400
                            }

                            RexonButton {
                                objectName: "detailGnomePrefs"
                                visible: detailGnome.on && detailGnome.st.prefs === true
                                pal: root.pal
                                tone: root.toneSecondary
                                text: root.gnomeTexts.prefs || ""
                                ToolTip.text: root.gnomeTexts.prefs_tooltip || ""
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: root.gnomeAction(page.detailItemId, "prefs")
                            }

                            RexonButton {
                                objectName: "detailGnomeFolder"
                                readonly property bool copy: detailGnome.st.folder === "copy"
                                visible: detailGnome.on && (detailGnome.st.folder || "") !== ""
                                pal: root.pal
                                tone: root.toneSecondary
                                text: copy ? (root.gnomeTexts.folder_copy || "")
                                           : (root.gnomeTexts.folder_open || "")
                                ToolTip.text: copy ? (root.gnomeTexts.folder_copy_tooltip || "")
                                                   : (root.gnomeTexts.folder_open_tooltip || "")
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: root.gnomeAction(page.detailItemId, "folder")
                            }

                            RexonButton {
                                objectName: "detailGnomeError"
                                visible: detailGnome.on && detailGnome.st.error === true
                                pal: root.pal
                                tone: root.toneSecondary
                                text: root.gnomeTexts.error_details || ""
                                ToolTip.text: root.gnomeTexts.error_details_tooltip || ""
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: gnomeErrorPopup.showFor(page.detailItemId,
                                                                   detailGnome.st.name || "")
                            }

                            RexonDangerButton {
                                objectName: "detailGnomeRemove"
                                visible: detailGnome.on && detailGnome.st.remove === true
                                pal: root.pal
                                enabled: !page.actionBusy
                                text: root.gnomeTexts.uninstall || ""
                                ToolTip.text: root.gnomeTexts.uninstall_tooltip || ""
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                // Question and "Restore" — as for the tile.
                                onClicked: root.removeLocal(page.detailItemId)
                            }

                            // "Configure…" of an application style (Breeze, Oxygen, Kvantum…) — as in
                            // the KDE "Application Style" module. If there is nothing to open it with,
                            // the button is disabled and the reason is in the tooltip (caught by the
                            // wrapper: a disabled button does not see hover).
                            Item {
                                id: detailStyleConfig
                                objectName: "detailStyleConfig"
                                readonly property var st: JSON.parse(page.detailStyleConfig || "{}")
                                visible: page.detailIsLocal && st.visible === true
                                implicitWidth: styleConfigButton.implicitWidth
                                implicitHeight: styleConfigButton.implicitHeight

                                RexonButton {
                                    id: styleConfigButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    enabled: detailStyleConfig.st.enabled === true
                                    text: detailStyleConfig.st.text || ""
                                    onClicked: {
                                        const err = page.configureDetailStyle();
                                        if (err !== "")
                                            page.setStatus(err);
                                    }
                                }

                                HoverHandler { id: styleConfigHover }
                                ToolTip.text: detailStyleConfig.st.tooltip || ""
                                ToolTip.visible: styleConfigHover.hovered
                                                 && (detailStyleConfig.st.tooltip || "") !== ""
                                ToolTip.delay: 400
                            }

                            // "Configure…" of a KDE window decoration (Breeze, Oxygen, Klassy, Aurorae
                            // themes) — like the edit button in the KDE "Window Decorations" window.
                            // No settings of its own or nothing to open it with — disabled, the reason
                            // in the tooltip. When the window is closed, the sample is re-captured.
                            Item {
                                id: detailDecoConfig
                                objectName: "detailDecoConfig"
                                readonly property var st: JSON.parse(page.detailDecoConfig || "{}")
                                visible: page.detailIsLocal && st.visible === true
                                implicitWidth: decoConfigButton.implicitWidth
                                implicitHeight: decoConfigButton.implicitHeight

                                RexonButton {
                                    id: decoConfigButton
                                    anchors.fill: parent
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    enabled: detailDecoConfig.st.enabled === true
                                    text: detailDecoConfig.st.text || ""
                                    onClicked: {
                                        const err = page.configureDetailDecoration();
                                        if (err !== "")
                                            page.setStatus(err);
                                    }
                                }

                                HoverHandler { id: decoConfigHover }
                                ToolTip.text: detailDecoConfig.st.tooltip || ""
                                ToolTip.visible: decoConfigHover.hovered
                                                 && (detailDecoConfig.st.tooltip || "") !== ""
                                ToolTip.delay: 400
                            }

                            // "Edit…" and "Create based on…" of a colour scheme — as in the KDE
                            // "Colours" module: the KDE editor, and without it — our own window.
                            Repeater {
                                id: detailSchemeEdit
                                objectName: "detailSchemeEdit"
                                readonly property var st: JSON.parse(page.detailSchemeEdit || "{}")
                                model: page.detailIsLocal && st.visible === true
                                       ? [["edit", st.edit, st.edit_tooltip],
                                          ["new", st.new_from, st.new_from_tooltip]]
                                       : []
                                delegate: RexonButton {
                                    required property var modelData
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    text: modelData[1] || ""
                                    ToolTip.text: modelData[2] || ""
                                    ToolTip.visible: hovered && (modelData[2] || "") !== ""
                                    ToolTip.delay: 600
                                    onClicked: schemeEditor.start(modelData[0])
                                }
                            }

                            // «Start at login» — only for Conky widgets: starting now and autostart
                            // are different things for them (the button next to it is "Start"/"Stop").
                            RexonCheckBox {
                                id: detailAutostart
                                objectName: "detailAutostart"
                                visible: page.detailConkyId !== ""
                                pal: root.pal
                                text: page.detailAutostartText
                                checked: visible && root.actions && root.actions.conkyAutostart
                                         ? root.actions.conkyAutostart(page.detailConkyId)
                                         : false
                                onToggled: {
                                    if (root.actions && root.actions.setConkyAutostart)
                                        detailAutostart.checked = root.actions.setConkyAutostart(
                                            page.detailConkyId, detailAutostart.checked);
                                }
                                // Enabling checks the widget's programs in the background
                                // (`conky_safety::live_prepare_start`): the result arrives later — re-read
                                // the state.
                                Connections {
                                    target: root.actions
                                    ignoreUnknownSignals: true
                                    function onActionDone(text, ok) {
                                        if (detailAutostart.visible && root.actions.conkyAutostart)
                                            detailAutostart.checked = root.actions.conkyAutostart(page.detailConkyId);
                                    }
                                }
                            }

                            RexonButton {
                                visible: page.detailIsLocal
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.detailFavoriteText
                                onClicked: {
                                    if (root.actions && root.actions.toggleFavorite)
                                        page.setFavorites(
                                            root.actions.toggleFavorite(page.detailItemId));
                                }
                            }
                        }

                        // Files of a store item when there are several: by default the one taken
                        // automatically is selected (as from the tile); if another is selected,
                        // the main button installs it.
                        StoreFileChoice {
                            id: detailFiles
                            objectName: "detailFiles"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            pal: root.pal
                            filesJson: page.detailSource === "store" ? page.detailFiles : "[]"
                            title: page.detailFilesTitle
                            hint: page.detailFilesHint
                            willInstallText: page.detailFileWillInstall
                            enabled: !page.actionBusy
                        }

                        // A GTK theme in GTK 4/libadwaita programs: links to its `gtk-4.0` in
                        // ~/.config/gtk-4.0. Nothing to carry over — the switch is disabled, and
                        // the reason is visible as a line.
                        ColumnLayout {
                            id: detailGtk4
                            objectName: "detailGtk4"
                            readonly property var st: JSON.parse(page.detailGtk4 || "{}")
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailIsLocal && st.visible === true
                            spacing: 2

                            RexonCheckBox {
                                id: detailGtk4Box
                                pal: root.pal
                                enabled: detailGtk4.st.enabled === true
                                checked: detailGtk4.st.checked === true
                                text: detailGtk4.st.text || ""
                                ToolTip.text: detailGtk4.st.note || ""
                                ToolTip.visible: hovered && detailGtk4.st.enabled === true
                                ToolTip.delay: 600
                                onToggled: {
                                    const err = page.setGtk4Apply(detailGtk4Box.checked);
                                    if (err !== "")
                                        page.setStatus(err);
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                visible: detailGtk4.st.enabled !== true
                                text: detailGtk4.st.note || ""
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }
                        }

                        // ── Conky widget parts and its log ───────────────
                        //
                        // A theme of several widgets (Seamod, variant sets) — each part has its
                        // own button. The log is what conky wrote on its last start from Rexon.
                        ColumnLayout {
                            id: detailConky
                            objectName: "detailConky"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailConkyId !== ""
                            spacing: 4

                            property int rev: 0
                            property var parts: []
                            property string log: ""
                            // A theme from Conky Studio, and it is on the machine.
                            property bool studio: false
                            readonly property var texts: conkyToolsRow.texts

                            function reload() {
                                if (!detailConky.visible || !root.actions || !root.actions.conkyParts) {
                                    detailConky.parts = [];
                                    detailConky.log = "";
                                    detailConky.studio = false;
                                    return;
                                }
                                detailConky.studio = root.actions.conkyStudioEditable
                                    ? root.actions.conkyStudioEditable(page.detailConkyId) : false;
                                try {
                                    detailConky.parts = JSON.parse(root.actions.conkyParts(page.detailConkyId));
                                } catch (e) {
                                    detailConky.parts = [];
                                }
                                detailConky.log = root.actions.conkyLog(page.detailConkyId);
                            }

                            onVisibleChanged: reload()
                            Connections {
                                target: page
                                function onDetailChanged() { detailConky.reload(); }
                            }
                            Connections {
                                target: root.actions
                                ignoreUnknownSignals: true
                                function onActionDone(text, ok) { detailConky.reload(); }
                            }

                            // The widget folder and its settings file — like "open folder" and manual
                            // editing in widget management programs. The file is that of the selected
                            // part.
                            RowLayout {
                                id: conkyOpenRow
                                objectName: "conkyOpenRow"
                                visible: detailConky.parts.length > 0
                                spacing: 8

                                RexonButton {
                                    objectName: "conkyOpenFolderButton"
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    text: detailConky.texts.open_folder || ""
                                    ToolTip.text: detailConky.texts.open_folder_tooltip || ""
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 600
                                    onClicked: {
                                        if (root.actions && root.actions.openConkyFolder)
                                            root.actions.openConkyFolder(page.detailConkyId);
                                    }
                                }

                                RexonButton {
                                    objectName: "conkyOpenFileButton"
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    enabled: detailConky.settingsConf !== ""
                                    text: detailConky.texts.open_file || ""
                                    ToolTip.text: detailConky.texts.open_file_tooltip || ""
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 600
                                    onClicked: {
                                        if (root.actions && root.actions.openConkyFile)
                                            root.actions.openConkyFile(page.detailConkyId, detailConky.settingsConf);
                                    }
                                }

                                // The editor the theme was made with; Rexon does not write an editor
                                // itself.
                                RexonButton {
                                    objectName: "conkyEditInStudioButton"
                                    visible: detailConky.studio
                                    pal: root.pal
                                    tone: root.toneSecondary
                                    text: detailConky.texts.edit_in_studio || ""
                                    ToolTip.text: detailConky.texts.edit_in_studio_tooltip || ""
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 600
                                    onClicked: {
                                        if (root.actions && root.actions.editConkyInStudio)
                                            root.actions.editConkyInStudio(page.detailConkyId);
                                    }
                                }
                            }

                            RexonText {
                                visible: detailConky.parts.length > 1
                                text: detailConky.texts.parts_title || ""
                                color: root.colTextSecondary
                                font.bold: true
                            }

                            Repeater {
                                model: detailConky.parts.length > 1 ? detailConky.parts : []
                                delegate: RowLayout {
                                    id: partRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 8

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: partRow.modelData.label
                                        color: root.colText
                                        elide: Text.ElideMiddle
                                        ToolTip.text: partRow.modelData.conf
                                        ToolTip.visible: partHover.hovered
                                        ToolTip.delay: 600
                                        HoverHandler { id: partHover }
                                    }

                                    RexonButton {
                                        objectName: "conkyPartButton"
                                        pal: root.pal
                                        enabled: !page.actionBusy
                                        text: partRow.modelData.running ? (detailConky.texts.part_stop || "")
                                                                        : (detailConky.texts.part_start || "")
                                        onClicked: {
                                            if (root.actions && root.actions.pressConkyPart)
                                                detailConky.parts = JSON.parse(root.actions.pressConkyPart(
                                                    page.detailConkyId, partRow.modelData.conf));
                                            grid.refreshKeepingScroll();
                                        }
                                    }
                                }
                            }

                            // Widget settings — for the selected part (for a single-file theme — for
                            // that file).
                            // The selected part is remembered by itself: the parts are re-read after
                            // every edit, and a list created anew would otherwise jump to the first.
                            property string chosenConf: ""
                            readonly property string settingsConf: {
                                const ps = detailConky.parts;
                                if (ps.length === 0)
                                    return "";
                                for (let i = 0; i < ps.length; i++)
                                    if (ps[i].conf === detailConky.chosenConf)
                                        return ps[i].conf;
                                return ps[0].conf;
                            }
                            Connections {
                                target: page
                                function onDetailChanged() { detailConky.chosenConf = ""; }
                            }

                            // «Start at login»: delay and "only in this environment" (core:
                            // conky_autostart).
                            RowLayout {
                                id: conkyAutostartRow
                                objectName: "conkyAutostartRow"
                                property var v: ({})
                                function reload() {
                                    if (!detailConky.visible || !root.actions || !root.actions.conkyAutostartOptions) {
                                        conkyAutostartRow.v = ({});
                                        return;
                                    }
                                    try {
                                        conkyAutostartRow.v = JSON.parse(root.actions.conkyAutostartOptions(page.detailConkyId));
                                    } catch (e) {
                                        conkyAutostartRow.v = ({});
                                    }
                                }
                                function save(delay, onlyHere) {
                                    if (root.actions && root.actions.setConkyAutostartOptions)
                                        conkyAutostartRow.v = JSON.parse(root.actions.setConkyAutostartOptions(
                                            page.detailConkyId, delay, onlyHere));
                                }
                                Connections {
                                    target: detailConky
                                    function onPartsChanged() { conkyAutostartRow.reload(); }
                                }
                                visible: conkyAutostartRow.v.on === true
                                spacing: 8
                                RexonText {
                                    text: (conkyAutostartRow.v.t || {}).delay || ""
                                    color: root.colText
                                    ToolTip.text: (conkyAutostartRow.v.t || {}).delay_tooltip || ""
                                    ToolTip.visible: delayHover.hovered
                                    ToolTip.delay: 600
                                    HoverHandler { id: delayHover }
                                }
                                RexonTextField {
                                    objectName: "conkyAutostartDelay"
                                    pal: root.pal
                                    Layout.preferredWidth: 60
                                    enabled: !page.actionBusy
                                    text: String(conkyAutostartRow.v.delay === undefined ? "" : conkyAutostartRow.v.delay)
                                    validator: IntValidator { bottom: 0; top: conkyAutostartRow.v.max_delay || 600 }
                                    onEditingFinished: {
                                        const n = parseInt(text);
                                        if (!isNaN(n) && n !== conkyAutostartRow.v.delay)
                                            conkyAutostartRow.save(n, conkyAutostartRow.v.only_here === true);
                                    }
                                }
                                RexonCheckBox {
                                    objectName: "conkyAutostartOnlyHere"
                                    visible: conkyAutostartRow.v.can_limit === true
                                    pal: root.pal
                                    enabled: !page.actionBusy
                                    text: (conkyAutostartRow.v.t || {}).only_here || ""
                                    checked: conkyAutostartRow.v.only_here === true
                                    ToolTip.text: (conkyAutostartRow.v.t || {}).only_here_tooltip || ""
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 600
                                    onToggled: conkyAutostartRow.save(conkyAutostartRow.v.delay || 0, checked)
                                }
                            }

                            RexonComboBox {
                                id: conkySettingsPart
                                objectName: "conkySettingsPart"
                                visible: detailConky.parts.length > 1
                                pal: root.pal
                                textRole: "label"
                                valueRole: "conf"
                                model: detailConky.parts
                                selectedValue: detailConky.settingsConf
                                onActivated: detailConky.chosenConf = currentValue
                            }

                            ConkySettings {
                                id: conkySettings
                                objectName: "conkySettings"
                                Layout.fillWidth: true
                                visible: detailConky.parts.length > 0
                                pal: root.pal
                                actions: root.actions
                                busy: page.actionBusy
                                itemId: page.detailConkyId
                                conf: detailConky.settingsConf
                            }

                            // The colour palette is for the whole widget (all its files), not for the
                            // selected part.
                            ConkyColours {
                                id: conkyColours
                                objectName: "conkyColours"
                                Layout.fillWidth: true
                                visible: detailConky.parts.length > 0
                                pal: root.pal
                                actions: root.actions
                                busy: page.actionBusy
                                itemId: page.detailConkyId
                                Connections {
                                    target: page
                                    function onDetailChanged() { conkyColours.reload(); }
                                }
                            }

                            RexonText {
                                Layout.topMargin: 4
                                visible: detailConky.log !== ""
                                text: detailConky.texts.log_title || ""
                                color: root.colTextSecondary
                                font.bold: true
                            }

                            RexonText {
                                objectName: "conkyLogText"
                                Layout.fillWidth: true
                                visible: detailConky.log !== ""
                                text: detailConky.log
                                color: root.colTextSecondary
                                font.family: "monospace"
                                font.pointSize: 9
                                textFormat: Text.PlainText
                                wrapMode: Text.WrapAnywhere
                            }
                        }

                        // ── Listen (sound theme) ───────────────────────
                        //
                        // An installed theme gets five buttons at once, as in KDE; a Store item
                        // first gets "Listen before installing", and after downloading the same
                        // buttons.
                        ColumnLayout {
                            id: detailSounds
                            objectName: "detailSounds"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.detailSoundTheme !== "" || page.detailSoundStore
                            spacing: 6

                            property string playError: ""

                            Row {
                                visible: page.detailSoundTheme !== ""
                                spacing: 8

                                RexonText {
                                    text: page.soundPreviewLabel
                                    color: root.colTextSecondary
                                    font.weight: Font.DemiBold
                                    height: 40
                                    verticalAlignment: Text.AlignVCenter
                                }

                                SoundButtons {
                                    player: soundActions
                                    texts: page
                                    pal: root.pal
                                    theme: page.detailSoundTheme
                                    buttonSize: 40
                                    onFailed: function (text) { detailSounds.playError = text; }
                                }
                            }

                            RexonButton {
                                objectName: "storePreviewButton"
                                visible: page.detailSoundStore
                                         && root.storePreviewThemes.length === 0
                                enabled: !root.storePreviewBusy
                                pal: root.pal
                                text: page.soundStorePreviewText
                                ToolTip.text: page.soundStorePreviewTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: {
                                    detailSounds.playError = "";
                                    root.startStorePreview();
                                }
                            }

                            ProgressBar {
                                Layout.preferredWidth: 320
                                visible: page.detailSoundStore && root.storePreviewBusy
                                indeterminate: root.storePreviewTotal <= 0
                                from: 0
                                to: Math.max(1, root.storePreviewTotal)
                                value: root.storePreviewDone
                            }

                            RexonText {
                                Layout.fillWidth: true
                                visible: page.detailSoundStore && text !== ""
                                text: root.storePreviewStatus
                                color: root.storePreviewFailed
                                       ? (root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                            : root.colAccent)
                                       : root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            // One archive may contain several themes — each gets its own row, with a
                            // name when there is more than one theme.
                            Repeater {
                                model: page.detailSoundStore ? root.storePreviewThemes : []
                                delegate: Column {
                                    id: storeTheme
                                    required property var modelData
                                    spacing: 4

                                    RexonText {
                                        visible: root.storePreviewThemes.length > 1
                                        text: storeTheme.modelData.theme
                                        color: root.colText
                                        font.bold: true
                                    }

                                    Row {
                                        spacing: 8

                                        RexonText {
                                            text: page.soundPreviewLabel
                                            color: root.colTextSecondary
                                            font.weight: Font.DemiBold
                                            height: 40
                                            verticalAlignment: Text.AlignVCenter
                                        }

                                        SoundButtons {
                                            player: soundActions
                                            texts: page
                                            pal: root.pal
                                            storeSounds: storeTheme.modelData.sounds
                                            buttonSize: 40
                                            onFailed: function (text) { detailSounds.playError = text; }
                                        }
                                    }
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                visible: text !== ""
                                text: detailSounds.playError
                                color: root.pal.state_warn !== undefined ? root.pal.state_warn
                                                                         : root.colAccent
                                wrapMode: Text.WordWrap
                            }
                        }

                        // Installation progress bar. In the original it is the PROGRESS that is
                        // visible, not a single result line: building with the AUR helper and
                        // large themes take tens of seconds, and without a bar the page looks
                        // frozen.
                        ProgressBar {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.progressVisible
                            from: 0
                            to: 100
                            value: page.installPercent
                        }

                        // Result of the last action — also the line-by-line work log: the
                        // installer writes each of its steps here. The "✅"/"❌" icon is set by the
                        // core — the markup has no branching of its own.
                        RexonText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: text !== ""
                            text: page.detailStatus
                            color: root.colText
                            wrapMode: Text.WordWrap
                        }

                        // Description with a "translation ⇄ original" switch. The switch is a link
                        // inside the text itself: the core builds it, here we only handle the
                        // press. Links in third-party text go through the shared path
                        // (`RexonLinkText`): an item of our own store opens as a card, everything
                        // else gets ↗ and a question before the browser.
                        RexonLinkText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            html: page.detailDescription
                            pal: root.pal
                            cardsSupported: true
                            color: root.colText
                            wrapMode: Text.WordWrap
                            onPageLink: page.toggleDescription()
                            onCardWanted: (card, url) => root.openLinkedCard(card, url)
                            onStatus: (text) => page.setStatus(text)
                        }

                        // ── AUR package details ────────────────────────
                        //
                        // The RPC record: maintainers, votes, dates, licence, dependencies,
                        // conflicts. Labels and the order of rows come from the core
                        // (`aur::info_facts`).
                        ColumnLayout {
                            objectName: "aurFacts"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 6
                            visible: page.detailSource === "aur"
                            spacing: 4

                            Repeater {
                                model: page.detailSource === "aur" ? page.aurFacts : []

                                delegate: RowLayout {
                                    id: factRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 14

                                    RexonText {
                                        Layout.preferredWidth: 190
                                        Layout.alignment: Qt.AlignTop
                                        visible: factRow.modelData.label !== ""
                                        text: factRow.modelData.label
                                        color: root.colTextSecondary
                                        font.bold: true
                                        textFormat: Text.PlainText
                                        wrapMode: Text.WordWrap
                                    }

                                    RexonLinkText {
                                        Layout.fillWidth: true
                                        html: factRow.modelData.html !== ""
                                              ? factRow.modelData.html
                                              : factRow.modelData.value
                                        plain: factRow.modelData.html === ""
                                        pal: root.pal
                                        cardsSupported: true
                                        onCardWanted: (card, url) => root.openLinkedCard(card, url)
                                        onStatus: (text) => page.setStatus(text)
                                        color: factRow.modelData.warn
                                               ? (root.pal.state_warn !== undefined
                                                  ? root.pal.state_warn : root.colStar)
                                               : root.colText
                                        linkColor: root.colAccent
                                        wrapMode: Text.WordWrap
                                    }
                                }
                            }
                        }

                        // ── AUR package source check ───────────────────
                        //
                        // Risk signs (`aur_trust`): what raised suspicion and why it matters.
                        // They do not forbid installation.
                        ColumnLayout {
                            objectName: "aurTrust"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            visible: page.detailSource === "aur"
                            spacing: 6

                            RexonText {
                                text: page.aurTrustTitle
                                color: root.colText
                                font.bold: true
                                font.pointSize: 12
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: page.aurTrustNote
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            Repeater {
                                model: page.detailSource === "aur" ? page.aurTrustRows : []

                                delegate: RowLayout {
                                    id: trustRow
                                    required property var modelData
                                    Layout.fillWidth: true
                                    spacing: 10

                                    readonly property color tone: trustRow.modelData.danger
                                                                  ? page.aurTrustDangerColor
                                                                  : root.colStar

                                    RexonText {
                                        Layout.alignment: Qt.AlignTop
                                        text: trustRow.modelData.danger ? "⛔" : "⚠️"
                                        color: trustRow.tone
                                        font.pointSize: 12
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 2

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: trustRow.modelData.text
                                            color: trustRow.tone
                                            font.bold: true
                                            textFormat: Text.PlainText
                                            wrapMode: Text.WordWrap
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: trustRow.modelData.why
                                            color: root.colTextSecondary
                                            textFormat: Text.PlainText
                                            wrapMode: Text.WordWrap
                                        }
                                    }
                                }
                            }

                            RexonText {
                                Layout.fillWidth: true
                                text: page.aurTrustStatus
                                color: root.colTextSecondary
                                textFormat: Text.PlainText
                                wrapMode: Text.WordWrap
                            }

                            RexonButton {
                                objectName: "aurPkgbuildButton"
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.aurPkgbuildBtn
                                ToolTip.text: page.aurPkgbuildBtnTooltip
                                ToolTip.visible: hovered
                                ToolTip.delay: 600
                                onClicked: pkgbuildPopup.show()
                            }
                        }

                        // ── Comments ───────────────────────────────────
                        //
                        // A local theme has none at all: the theme lies on disk and never has a
                        // store page. Cinnamon Spices has no comments for programs.
                        RexonText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            visible: !page.detailIsLocal && page.detailSource !== "spices"
                                     && page.detailSource !== "wallhaven"
                            text: page.commentsHeader
                            color: root.colText
                            font.bold: true
                            font.pointSize: 12
                        }

                        RexonText {
                            Layout.leftMargin: 16
                            visible: !page.detailIsLocal && page.detailSource !== "spices"
                                     && page.detailSource !== "wallhaven"
                                     && page.commentsEmpty
                                     && !page.commentsBlocked
                            text: page.commentsEmptyText
                            color: root.colTextSecondary
                        }

                        // The AUR package page was not provided (the site's bot protection, no
                        // network): comments are only on the site.
                        ColumnLayout {
                            objectName: "aurCommentsBlocked"
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            visible: page.commentsBlocked
                            spacing: 6

                            RexonText {
                                Layout.fillWidth: true
                                text: page.commentsBlockedText
                                color: root.colTextSecondary
                                wrapMode: Text.WordWrap
                            }

                            RexonButton {
                                pal: root.pal
                                tone: root.toneSecondary
                                text: page.aurOpenText
                                onClicked: {
                                    const url = page.detailSiteUrl();
                                    if (url && root.actions && root.actions.openUrl)
                                        root.actions.openUrl(url);
                                }
                            }
                        }

                        Repeater {
                            model: page.detailIsLocal ? [] : page.commentRows

                            delegate: ColumnLayout {
                                id: commentRow
                                required property var modelData
                                Layout.fillWidth: true
                                // Indent of a reply in a thread — a number from the core.
                                Layout.leftMargin: 16 + commentRow.modelData.indent
                                Layout.rightMargin: 16
                                Layout.topMargin: 6
                                spacing: 2

                                RexonText {
                                    Layout.fillWidth: true
                                    text: commentRow.modelData.replyPrefix
                                          + commentRow.modelData.author
                                          + "  ·  " + commentRow.modelData.date
                                    // The item's author is highlighted with the accent: their reply in a
                                    // thread weighs differently from someone else's.
                                    color: commentRow.modelData.isProductAuthor
                                           ? root.colAccent : root.colTextSecondary
                                    font.bold: true
                                    font.pointSize: 9
                                    elide: Text.ElideRight
                                }

                                RexonLinkText {
                                    Layout.fillWidth: true
                                    html: commentRow.modelData.body
                                    pal: root.pal
                                    cardsSupported: true
                                    color: root.colText
                                    wrapMode: Text.WordWrap
                                    onPageLink: page.toggleComment(commentRow.modelData.index)
                                    onCardWanted: (card, url) => root.openLinkedCard(card, url)
                                    onStatus: (text) => page.setStatus(text)
                                }
                            }
                        }

                        RexonButton {
                            Layout.leftMargin: 16
                            Layout.topMargin: 8
                            visible: page.commentsMoreVisible
                            pal: root.pal
                            tone: root.toneSecondary
                            text: page.commentsMoreText
                            onClicked: page.loadMoreComments()
                        }

                        RexonText {
                            Layout.fillWidth: true
                            Layout.leftMargin: 16
                            Layout.rightMargin: 16
                            Layout.topMargin: 10
                            Layout.bottomMargin: 16
                            text: page.detailPath
                            color: root.colTextSecondary
                            font.pointSize: 9
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                GridView {
                    id: grid
                    anchors.fill: parent
                    // Warning: paddings of the CONTENT, not of the area itself. The difference
                    // shows on the scroll bar: in the original the bar sits at the window
                    // edge, and the paddings belong to the grid inside it. When the paddings
                    // were on the area (`anchors.*Margin`), the bar moved 16 pixels inwards —
                    // measured on a live run 01.09.2026.
                    //
                    // The paddings differ by axis: on the sides exactly as much as the header
                    // and the status line have (16), otherwise the left edge of the tiles
                    // would not match the left edge of the label above them.
                    leftMargin: page.gridMarginH
                    // On the right the padding is narrower by the gap: the gap after the last
                    // column sits inside the cell, and the tile edge lands on the same 16
                    // pixels as on the left.
                    rightMargin: Math.max(0, page.gridMarginH - page.gridSpacing)
                    topMargin: page.gridMarginV
                    bottomMargin: page.gridMarginV
                    visible: page.screen === "grid"
                    clip: true
                    // Columns by width: as many columns as core tiles (`cardWidth` is the
                    // minimum width) fit, and the remaining width is shared among them.
                    // Previously the tile was constant, and in a 1100-pixel window an empty
                    // strip almost a tile wide gaped to the right of the two columns.
                    readonly property int usableWidth:
                        width - leftMargin - rightMargin
                    // The calculation is the core's (`ap::grid_columns`/`grid_cell_width`),
                    // one for all tile grids (and for the Razer Axon library).
                    readonly property int columns: page.gridColumns(usableWidth)
                    cellWidth: page.gridCellWidth(usableWidth)
                    // A catalogue page is whole rows: the core cuts it by these columns
                    // (`ap::grid_page_size`), otherwise with nine columns the last row of
                    // twenty came out with two tiles.
                    onColumnsChanged: page.setGridColumns(columns)
                    Component.onCompleted: page.setGridColumns(columns)
                    cellHeight: page.cardHeight + page.gridSpacing
                    model: page

                    ScrollBar.vertical: ScrollBar {}

                    // The blocks above the tiles are the header of the grid itself, not
                    // separate rows above it, and they scroll together with the tiles, as in
                    // KDE. Fixed in place, they ate the height: in a 1100×750 window the
                    // five-row "Login manager" left the tiles about 60 pixels, and above
                    // "Colour schemes" the accent together with "Light and dark" (all the
                    // more when expanded) covered the grid entirely, and the wheel had
                    // nothing to scroll (01.11.2026).
                    // The header sits in content coordinates at x = 0, i.e. already after the
                    // grid's left padding; its width reaches the right padding of the tiles
                    // (16, as for the page header).
                    header: Item {
                        width: grid.width - grid.leftMargin - page.gridMarginH
                        height: gridHeaderBlocks.implicitHeight

                        ColumnLayout {
                            id: gridHeaderBlocks
                            width: parent.width
                            spacing: 0

                            LoginManagerSection {
                                id: gridLoginManager
                                objectName: "gridLoginManagerSection"
                                Layout.fillWidth: true
                                Layout.bottomMargin: page.gridSpacing
                                visible: page.loginSection
                                         && (gridLoginManager.managers.length > 0 || gridLoginManager.sessionOffer !== null)
                                pal: root.pal
                                sty: page.sectionStyles
                                actions: root.actions
                            }

                            // Accent colour — above "Colour schemes", as in KDE "Colours". Where it
                            // stands and whether it is available — the core (`accent`).
                            AccentSection {
                                id: gridAccent
                                objectName: "accentSection"
                                Layout.fillWidth: true
                                Layout.bottomMargin: 8
                                visible: page.installFileSection === (gridAccent.st.section || "-")
                                         && gridAccent.st.available === true
                                pal: root.pal
                                sty: page.sectionStyles
                            }

                            // Light and dark — above "Global themes" (Plasma) and "Colour schemes":
                            // theme pairs and switching on a schedule. Where it stands and whether it
                            // is available — the core (`day_night`).
                            DayNightSection {
                                id: gridDayNight
                                objectName: "dayNightSection"
                                Layout.fillWidth: true
                                Layout.bottomMargin: 8
                                visible: (gridDayNight.st.sections || []).indexOf(page.installFileSection) >= 0
                                         && gridDayNight.st.available === true
                                pal: root.pal
                                sty: page.sectionStyles
                            }

                            // "Save current look…" — above "Global themes" in all environments (core
                            // `global_save`). Once saved, the grid re-reads the disk and the new
                            // theme's tile is in it at once.
                            GlobalSaveSection {
                                objectName: "globalSaveSection"
                                Layout.fillWidth: true
                                Layout.bottomMargin: 8
                                visible: page.installFileSection === "global"
                                pal: root.pal
                                sty: page.sectionStyles
                                onSaved: function(message) {
                                    page.setStatus(message);
                                    page.refresh();
                                }
                            }
                        }
                    }

                    // Re-read the section without losing the place: after marking for removal,
                    // "Restore" and removal, the grid is reset by the model and used to jump
                    // to the top, while in KDE the position stays. The place is remembered
                    // here and restored when the new contents arrive (`countChanged` is sent
                    // on every answer).
                    property real keptY: NaN

                    function refreshKeepingScroll() {
                        if (isNaN(grid.keptY))
                            grid.keptY = grid.contentY;
                        page.refresh();
                    }

                    function restoreKeptScroll() {
                        if (isNaN(grid.keptY))
                            return;
                        const y = grid.keptY;
                        grid.keptY = NaN;
                        grid.forceLayout();
                        const top = grid.originY - grid.topMargin;
                        const bottom = grid.originY + grid.contentHeight
                                       + grid.bottomMargin - grid.height;
                        grid.contentY = Math.max(top, Math.min(y, Math.max(top, bottom)));
                    }

                    Connections {
                        target: page
                        function onCountChanged() {
                            Qt.callLater(grid.restoreKeptScroll);
                        }
                    }

                    delegate: Item {
                        id: cell
                        required property string kind
                        required property string itemId
                        required property string name
                        required property string author
                        required property string stats
                        required property string statsTooltip
                        required property string starGlyph
                        required property string preview
                        required property string previewText
                        required property string previewFont
                        required property string previewSample
                        required property string previewNote
                        required property string previewBig
                        required property string mixBadge
                        required property string mixBadgeTooltip
                        required property string metaLine
                        required property string metaTooltip
                        required property bool metaWarn
                        required property string buttonText
                        required property string buttonState
                        required property bool buttonEnabled
                        required property string buttonTooltip
                        required property string favoriteText
                        required property string favoriteState
                        required property string soundTheme
                        required property string splashDemo
                        required property string aurNote
                        required property string aurNoteTooltip
                        required property string aurLog
                        required property string riskLevel
                        required property bool removeVisible
                        required property bool removeEnabled
                        required property string removeTooltip
                        required property bool current
                        required property string gnomeUuid
                        required property bool gnomePrefs
                        required property bool gnomeErrorInfo
                        // The section's current theme is highlighted, as in KDE: a frame in the
                        // highlight colour and a bold label (the other tiles of the section have
                        // an ordinary label).
                        readonly property bool marked: page.markCurrent && cell.current

                        width: grid.cellWidth
                        height: grid.cellHeight

                        Rectangle {
                            id: card
                            width: grid.cellWidth - page.gridSpacing
                            height: page.cardHeight
                            radius: 4
                            color: cardMouse.containsMouse ? root.colCardHover
                                                           : root.colCard
                            border.color: (cell.marked || cardMouse.containsMouse)
                                          ? root.colAccent : root.colBorder
                            border.width: 1

                            // The current theme's frame goes over the picture: the tile's own frame
                            // lies under the content, and a full-width picture covers it.
                            Rectangle {
                                objectName: "currentFrame"
                                anchors.fill: parent
                                z: 10
                                visible: cell.marked
                                color: "transparent"
                                radius: parent.radius
                                border.color: root.colAccent
                                border.width: 3
                            }

                            MouseArea {
                                id: cardMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                // An AUR package opens its own card inside the program, like a store item.
                                onClicked: page.openDetail(cell.itemId)
                            }

                            ColumnLayout {
                                anchors.fill: parent
                                spacing: 0

                                // Item picture.
                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: page.cardPreviewHeight
                                    id: previewBox
                                    color: root.colSecondary

                                    // A drawn picture for a theme without its own (the `local_placeholders`
                                    // catalogue): a sample from its content or a placeholder of its kind.
                                    // Cropping by height, appropriate for screenshots, would cut its edges: we
                                    // fit it whole, and the margins on the sides are covered by the same
                                    // colour gradient as on the frame itself.
                                    readonly property bool placeholder:
                                        cell.preview.indexOf("/local_placeholders/") >= 0
                                    // There is no picture at all (an AUR package without a GitHub link): the
                                    // same placeholder of its kind as for local themes — a sign on the same
                                    // backing.
                                    readonly property bool bare:
                                        cell.preview === "" && cell.previewText !== ""
                                        && fontSample.status !== FontLoader.Ready
                                    // A font package: the picture is the font file itself, which draws a text
                                    // sample.
                                    FontLoader {
                                        id: fontSample
                                        source: cell.previewFont
                                    }

                                    Rectangle {
                                        anchors.fill: parent
                                        visible: previewBox.placeholder || previewBox.bare
                                        gradient: Gradient {
                                            GradientStop { position: 0.0; color: "#263d52" }
                                            GradientStop { position: 1.0; color: "#16202d" }
                                        }
                                    }

                                    Image {
                                        anchors.fill: parent
                                        source: cell.preview
                                        fillMode: previewBox.placeholder
                                                  ? Image.PreserveAspectFit
                                                  : Image.PreserveAspectCrop
                                        asynchronous: true
                                        cache: true
                                        // A sound theme has the "Listen:" row in this place (below): a placeholder
                                        // picture with a bell would cover it.
                                        visible: source !== "" && cell.soundTheme === ""
                                    }

                                    // A click on the tile is taken by the theme card, so the large view of its
                                    // picture is opened with the magnifier under the cursor. A drawn picture
                                    // (a sample, a placeholder) has nothing larger to show — unless it was
                                    // drawn in full size too (a whole theme of the desktop).
                                    RexonZoomArea {
                                        anchors.fill: parent
                                        pal: root.pal
                                        badgeOnly: true
                                        enabled: cell.preview !== "" && cell.soundTheme === ""
                                                 && (!previewBox.placeholder || cell.previewBig !== "")
                                        visible: enabled
                                        onActivated: root.openTileZoom(cell.name, cell.preview, cell.previewBig)
                                    }

                                    // "Demonstrate" for a KDE splash screen, as in the KDE module: the splash
                                    // full screen for a few seconds (core — `splash_demo_argv`). Over the
                                    // magnifier.
                                    RexonButton {
                                        anchors.right: parent.right
                                        anchors.bottom: parent.bottom
                                        anchors.margins: 8
                                        visible: cell.splashDemo !== ""
                                        pal: root.pal
                                        // An SDDM theme has the same button — the login screen "Preview" (core —
                                        // `local_splash_demo`).
                                        text: "▶ " + (root.actions && root.actions.tileDemoLabel
                                                      ? root.actions.tileDemoLabel(cell.splashDemo)
                                                      : page.splashDemoLabel)
                                        onClicked: {
                                            if (!root.actions || !root.actions.demoSplash)
                                                return;
                                            const err = root.actions.demoSplash(cell.splashDemo);
                                            if (err !== "")
                                                page.setStatus(err);
                                        }
                                        HoverHandler { id: splashDemoHover }
                                        ToolTip.text: root.actions && root.actions.tileDemoTooltip
                                                      ? root.actions.tileDemoTooltip(cell.splashDemo)
                                                      : page.splashDemoTooltip
                                        ToolTip.visible: splashDemoHover.hovered
                                        ToolTip.delay: 500
                                    }

                                    // There is no picture at all — the item's sign is in its place. That is
                                    // what the original did: for an AUR package without a GitHub link there is
                                    // nowhere to get a picture from, and an empty rectangle read as "the tile
                                    // has not finished loading".
                                    Column {
                                        anchors.centerIn: parent
                                        width: parent.width - 24
                                        visible: previewBox.bare
                                        spacing: 6

                                        RexonText {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            text: cell.previewText
                                            color: "#ffffff"
                                            // Sign size — as on the drawn placeholder (48 at a frame height of 170),
                                            // smaller if there is a label under it.
                                            font.pixelSize: Math.round(previewBox.height
                                                                       * (cell.previewNote !== "" ? 0.28 : 0.38))
                                        }

                                        // An honest label: what this item is and why there is no screenshot (core
                                        // — `aur::no_preview_note`).
                                        RexonText {
                                            width: parent.width
                                            visible: cell.previewNote !== ""
                                            text: cell.previewNote
                                            color: "#c7d5e0"
                                            horizontalAlignment: Text.AlignHCenter
                                            wrapMode: Text.WordWrap
                                            maximumLineCount: 2
                                            elide: Text.ElideRight
                                        }
                                    }

                                    // Font sample — in this very font.
                                    RexonText {
                                        anchors.centerIn: parent
                                        width: parent.width - 24
                                        visible: fontSample.status === FontLoader.Ready
                                        text: cell.previewSample
                                        color: "#ffffff"
                                        font.family: fontSample.name
                                        font.pixelSize: Math.round(previewBox.height * 0.22)
                                        horizontalAlignment: Text.AlignHCenter
                                        elide: Text.ElideRight
                                    }

                                    // An AUR package among single items ("All"): a set, a system package, a
                                    // password.
                                    Rectangle {
                                        anchors.left: parent.left
                                        anchors.top: parent.top
                                        anchors.margins: 8
                                        visible: cell.mixBadge !== ""
                                        radius: 4
                                        color: "#cc16202d"
                                        border.color: root.pal.border || "#2a4055"
                                        width: Math.min(mixBadgeText.implicitWidth + 12, previewBox.width - 16)
                                        height: mixBadgeText.implicitHeight + 6

                                        RexonText {
                                            id: mixBadgeText
                                            anchors.centerIn: parent
                                            width: Math.min(implicitWidth, previewBox.width - 28)
                                            text: cell.mixBadge
                                            color: "#ffffff"
                                            font.pointSize: page.cardMetaPointSize
                                            elide: Text.ElideRight
                                        }

                                        HoverHandler { id: mixBadgeHover }
                                        ToolTip.text: cell.mixBadgeTooltip
                                        ToolTip.visible: mixBadgeHover.hovered && cell.mixBadgeTooltip !== ""
                                        ToolTip.delay: 500
                                    }

                                    // A sound theme has no picture: in its place there is the "Listen:" row,
                                    // as on the KDE "System Sounds" card.
                                    Column {
                                        anchors.centerIn: parent
                                        visible: cell.soundTheme !== ""
                                        spacing: 8

                                        RexonText {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            text: "🔔"
                                            font.pointSize: 24
                                        }

                                        RexonText {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            text: page.soundPreviewLabel
                                            color: root.colTextSecondary
                                            font.weight: Font.DemiBold
                                        }

                                        Loader {
                                            anchors.horizontalCenter: parent.horizontalCenter
                                            active: cell.soundTheme !== ""
                                            sourceComponent: SoundButtons {
                                                player: soundActions
                                                texts: page
                                                pal: root.pal
                                                theme: cell.soundTheme
                                                onFailed: function (text) { page.setStatus(text); }
                                            }
                                        }
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    // The tile body paddings in the original differ at the top and bottom (14
                                    // versus 10), and the row gap is twice as wide as it was here.
                                    Layout.leftMargin: page.cardBodyMargin
                                    Layout.rightMargin: page.cardBodyMargin
                                    Layout.topMargin: page.cardBodyTopMargin
                                    Layout.bottomMargin: page.cardBodyBottomMargin
                                    spacing: page.cardBodySpacing

                                    RexonText {
                                        Layout.fillWidth: true
                                        text: cell.name
                                        color: root.colText
                                        // In the original the theme name is LARGER than the tile's other lines
                                        // (11pt versus 10): it is how a theme is found in the grid. Here the size
                                        // was not set at all, and the name came out smaller than the author.
                                        font.pointSize: page.cardNamePointSize
                                        font.bold: !page.markCurrent || cell.marked
                                        elide: Text.ElideRight
                                    }

                                    // For an AUR package the author's place is taken by the source line ("🛠
                                    // AUR · version"), as in the original: an extra line did not fit into a
                                    // tile of constant height, and the «Install» button was cut at the bottom
                                    // (screenshots 01.10.2026).
                                    RexonText {
                                        Layout.fillWidth: true
                                        visible: cell.metaLine === ""
                                        text: cell.author
                                        color: root.colText
                                        font.pointSize: page.cardMetaPointSize
                                        // `#CardAuthor { font-weight: 600 }` — semi-bold, not bold; for the
                                        // current theme — bold (KDE).
                                        font.weight: cell.marked ? Font.Bold : Font.DemiBold
                                        elide: Text.ElideRight
                                    }

                                    // The source line — only for AUR packages: in "All" mode they are mixed
                                    // with store items, and the user must see where the package will come
                                    // from.
                                    RexonText {
                                        Layout.fillWidth: true
                                        visible: cell.metaLine !== ""
                                        text: cell.metaLine
                                        color: cell.riskLevel === "danger"
                                               ? page.aurTrustDangerColor
                                               : (cell.metaWarn ? root.colStar
                                                                : root.colTextSecondary)
                                        font.pointSize: page.cardMetaPointSize
                                        elide: Text.ElideRight
                                        ToolTip.text: cell.metaTooltip
                                        ToolTip.visible: metaHover.hovered
                                                         && cell.metaTooltip !== ""
                                        ToolTip.delay: 600
                                        HoverHandler { id: metaHover }
                                    }

                                    // An AUR installation refusal goes in place of the stats line (the tile
                                    // height is constant: an extra line would cut the button): the reason in
                                    // words and a "Log" link.
                                    RowLayout {
                                        Layout.fillWidth: true
                                        visible: cell.aurNote !== ""
                                        spacing: 6

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: cell.aurNote
                                            color: root.colStar
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                            elide: Text.ElideRight
                                            ToolTip.text: cell.aurNoteTooltip
                                            ToolTip.visible: noteHover.hovered
                                                             && cell.aurNoteTooltip !== ""
                                            ToolTip.delay: 300
                                            HoverHandler { id: noteHover }
                                        }

                                        RexonText {
                                            visible: cell.aurLog !== ""
                                            text: cell.aurLog
                                            color: root.colAccent
                                            font.pointSize: page.cardMetaPointSize
                                            font.underline: true
                                            MouseArea {
                                                anchors.fill: parent
                                                cursorShape: Qt.PointingHandCursor
                                                onClicked: {
                                                    if (root.actions && root.actions.openAurLog)
                                                        root.actions.openAurLog(cell.itemId);
                                                }
                                            }
                                        }
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true
                                        // An empty stats line (a theme from disk) is not shown at all — and the
                                        // star neither.
                                        visible: cell.aurNote === "" && cell.stats !== ""
                                        spacing: 0

                                        // The rating icon is the only colourful spot of the stats line; the colour
                                        // comes as a skin token.
                                        RexonText {
                                            text: cell.starGlyph
                                            color: root.colStar
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                        }

                                        RexonText {
                                            Layout.fillWidth: true
                                            text: cell.stats
                                            color: root.colText
                                            font.pointSize: page.cardMetaPointSize
                                            font.bold: true
                                            elide: Text.ElideRight
                                            ToolTip.text: cell.statsTooltip
                                            ToolTip.visible: statsHover.hovered
                                                             && cell.statsTooltip !== ""
                                            ToolTip.delay: 600
                                            HoverHandler { id: statsHover }
                                        }
                                    }

                                    // Tile button. The colours come from the core: the three states differ
                                    // not only by shade but also by silhouette (fill versus outline).
                                    RowLayout {
                                        id: cardButtons
                                        Layout.fillWidth: true
                                        Layout.topMargin: page.cardButtonTopMargin
                                        spacing: 6

                                        Button {
                                            id: cardButton
                                            Layout.fillWidth: true
                                            implicitHeight: page.cardButtonHeight
                                            enabled: cell.buttonEnabled
                                            // Warning: there must be no muting here. The «✅ Active» badge is
                                            // disabled, but in the original it is full-colour: the style sheet has no
                                            // `:disabled` rule of its own. Opacity mixed in the tile background and
                                            // changed the button colour.
                                            text: cell.buttonText
                                            // The exception is an action that will not work right now (an SDDM theme
                                            // with Plasma Login Manager): the button is muted, the reason is in the
                                            // tooltip.
                                            opacity: (cell.kind !== "aur"
                                                      && cell.buttonTooltip !== "") ? 0.45 : 1.0
                                            ToolTip.text: cell.buttonTooltip
                                            ToolTip.visible: hovered
                                                             && cell.buttonTooltip !== ""
                                            ToolTip.delay: 600

                                            // A GNOME extension that the shell knows is enabled and disabled by the
                                            // shell itself.
                                            onClicked: {
                                                if (cell.gnomeUuid !== "")
                                                    root.gnomeAction(cell.itemId, "toggle");
                                                else
                                                    root.actOnCard(cell.kind, cell.itemId);
                                            }
                                            // The button catches Space by itself; Enter is the same press (for a Conky
                                            // widget — start/stop).
                                            Keys.onReturnPressed: cardButton.clicked()
                                            Keys.onEnterPressed: cardButton.clicked()

                                            readonly property var colors:
                                                page.buttonColors(cell.buttonState)

                                            background: Rectangle {
                                                radius: 3
                                                color: cardButton.colors.bg === ""
                                                       ? "transparent"
                                                       : cardButton.colors.bg
                                                border.color: cardButton.activeFocus
                                                              ? root.colAccent
                                                              : (cardButton.colors.border === ""
                                                                 ? "transparent"
                                                                 : cardButton.colors.border)
                                                border.width: cardButton.activeFocus ? 2 : 1
                                            }

                                            contentItem: RexonText {
                                                text: cardButton.text
                                                color: cardButton.colors.fg
                                                font.bold: true
                                                horizontalAlignment: Text.AlignHCenter
                                                verticalAlignment: Text.AlignVCenter
                                                elide: Text.ElideRight
                                            }
                                        }

                                        // For an AUR package, instead of favourites — the package page on the
                                        // site, as in the original.
                                        RexonButton {
                                            Layout.fillWidth: true
                                            visible: cell.kind === "aur"
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            implicitHeight: page.cardButtonHeight
                                            text: page.aurDetailsText
                                            ToolTip.text: page.aurDetailsTooltip
                                            ToolTip.visible: hovered
                                            ToolTip.delay: 600
                                            // An AUR package has its own card inside the program; the site is reached
                                            // from it.
                                            onClicked: page.openDetail(cell.itemId)
                                        }

                                        // "★ Favourites" — next to the action, as in the original; an AUR package
                                        // never has favourites, and then the label is empty.
                                        RexonButton {
                                            Layout.fillWidth: true
                                            visible: cell.favoriteText !== ""
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            implicitHeight: page.cardButtonHeight
                                            text: cell.favoriteText
                                            highlighted: cell.favoriteState === "on"
                                            onClicked: {
                                                if (root.actions
                                                        && root.actions.toggleFavorite)
                                                    page.setFavorites(
                                                        root.actions.toggleFavorite(
                                                            cell.itemId));
                                            }
                                        }

                                        // "⚙️" and "⚠️" of a GNOME extension: its own settings and error details
                                        // (the folder is in the details).
                                        RexonButton {
                                            objectName: "gnomeTilePrefs"
                                            visible: cell.gnomePrefs
                                            Layout.preferredWidth: page.cardButtonHeight
                                            implicitHeight: page.cardButtonHeight
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            text: "⚙️"
                                            ToolTip.text: root.gnomeTexts.prefs_tooltip || ""
                                            ToolTip.visible: hovered
                                            ToolTip.delay: 400
                                            onClicked: root.gnomeAction(cell.itemId, "prefs")
                                        }

                                        RexonButton {
                                            objectName: "gnomeTileError"
                                            visible: cell.gnomeErrorInfo
                                            Layout.preferredWidth: page.cardButtonHeight
                                            implicitHeight: page.cardButtonHeight
                                            pal: root.pal
                                            tone: root.toneSecondary
                                            text: "⚠️"
                                            ToolTip.text: root.gnomeTexts.error_details_tooltip || ""
                                            ToolTip.visible: hovered
                                            ToolTip.delay: 400
                                            onClicked: gnomeErrorPopup.showFor(cell.itemId, cell.name)
                                        }

                                        // "🗑" — delete a theme from disk (the core decides, `theme_removal`). For
                                        // an enabled or system one the button is unavailable, the reason in a
                                        // tooltip; a disabled button does not catch hover, so the wrapper holds
                                        // the tooltip.
                                        Item {
                                            visible: cell.removeVisible
                                            Layout.preferredWidth: page.cardButtonHeight
                                            Layout.preferredHeight: page.cardButtonHeight

                                            RexonDangerButton {
                                                anchors.fill: parent
                                                pal: root.pal
                                                enabled: cell.removeEnabled
                                                text: "🗑️"
                                                // Your own GNOME extension goes the same way as a theme: a question with
                                                // its name and a "Restore" line (the shell removes it); a system one gets
                                                // no "🗑" at all.
                                                onClicked: root.removeLocal(cell.itemId)
                                            }

                                            HoverHandler { id: removeHover }
                                            ToolTip.text: cell.removeTooltip !== ""
                                                          ? cell.removeTooltip
                                                          : (cell.gnomeUuid !== ""
                                                             ? (root.gnomeTexts.uninstall_tooltip || "")
                                                             : page.detailUninstallText)
                                            ToolTip.visible: removeHover.hovered
                                            ToolTip.delay: 400
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Page strip ────────────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 10
                visible: page.paginationVisible && page.screen === "grid"
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    pal: root.pal
                    tone: root.toneNav
                    // The "← →" arrows in the labels follow mirroring, as for "Back to
                    // catalogue".
                    text: root.arrows.keyHint(page.pagePrevText)
                    enabled: page.prevEnabled
                    onClicked: page.prevPage()
                }

                RexonText {
                    text: page.pageLabel
                    color: root.colTextSecondary
                    font.bold: true
                    leftPadding: 12
                    rightPadding: 12
                    verticalAlignment: Text.AlignVCenter
                    Layout.alignment: Qt.AlignVCenter
                }

                RexonButton {
                    pal: root.pal
                    tone: root.toneNav
                    text: root.arrows.keyHint(page.pageNextText)
                    enabled: page.nextEnabled
                    onClicked: page.nextPage()
                }

                Item { Layout.fillWidth: true }
            }
        }
    }
    // ── "Restore" line of a deferred removal ─────────────────────────────
    //
    // As in KDE: a theme from disk is not removed at once but marked; the tile
    // goes away, and a line with "Restore" hangs at the bottom. The host
    // removes the theme when the line is closed (✕), expires or the page
    // closes. How long it hangs is a core number
    // (`theme_removal::UNDO_TIMEOUT_MS`).
    Rectangle {
        id: removalBar

        property string message: ""
        // How long is left until removal, ms. The countdown is visible (a bar and
        // the seconds on "Restore"), pauses while the cursor is over the line or
        // focus is on its buttons, and starts anew with every new mark.
        property real remainingMs: page.removalUndoMs
        readonly property int remainingSeconds: Math.max(0, Math.ceil(remainingMs / 1000))
        readonly property bool paused: barHover.hovered || undoButton.activeFocus
                                       || closeButton.activeFocus

        // The width does not jump along with the number of seconds: the widest
        // one shown is kept.
        property real widest: 0

        function show(text) {
            removalBar.message = text;
            removalBar.widest = 0;
            removalBar.remainingMs = page.removalUndoMs;
            removalBar.visible = true;
            removalTimer.restart();
        }

        // Close the line and remove the marked item.
        function finish() {
            if (!removalBar.visible)
                return;
            removalBar.visible = false;
            removalTimer.stop();
            if (root.actions && root.actions.commitRemovals)
                root.actions.commitRemovals();
        }

        function undo() {
            removalBar.visible = false;
            removalTimer.stop();
            if (root.actions && root.actions.undoRemovals)
                root.actions.undoRemovals();
            grid.refreshKeepingScroll();
        }

        objectName: "removalBar"
        visible: false
        z: 100
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 16
        width: Math.min(parent.width - 32, Math.max(removalBar.widest, barRow.implicitWidth) + 24)
        height: barRow.implicitHeight + 16
        radius: 4
        color: root.colCard
        border.color: root.colAccent
        border.width: 1

        HoverHandler {
            id: barHover
        }

        RowLayout {
            id: barRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: 12
            anchors.rightMargin: 12
            spacing: 10
            onImplicitWidthChanged: removalBar.widest = Math.max(removalBar.widest, implicitWidth)

            RexonText {
                objectName: "removalBarText"
                Layout.fillWidth: true
                // "Deleting "X". 8 seconds left — "Restore" will cancel the deletion." —
                // labels from the core (`theme_removal`).
                text: removalBar.message + " "
                      + page.removalCountdownText(removalBar.remainingSeconds)
                textFormat: Text.PlainText
                color: root.colText
                elide: Text.ElideRight
            }

            RexonButton {
                id: undoButton
                objectName: "removalUndoButton"
                pal: root.pal
                text: page.removalUndoText + " (" + removalBar.remainingSeconds + ")"
                onClicked: removalBar.undo()
            }

            RexonButton {
                id: closeButton
                pal: root.pal
                tone: root.toneSecondary
                text: "❌"
                onClicked: removalBar.finish()
            }
        }

        // Countdown bar: decreases to zero over the core's timeout.
        Rectangle {
            objectName: "removalCountdownBar"
            anchors.left: parent.left
            anchors.bottom: parent.bottom
            anchors.leftMargin: 1
            anchors.bottomMargin: 1
            height: 3
            radius: 1
            color: root.colAccent
            width: Math.max(0, (parent.width - 2) * removalBar.remainingMs
                                / Math.max(1, page.removalUndoMs))
        }

        Timer {
            id: removalTimer
            interval: 100
            repeat: true
            onTriggered: {
                if (removalBar.paused)
                    return;
                removalBar.remainingMs -= interval;
                if (removalBar.remainingMs <= 0)
                    removalBar.finish();
            }
        }

        Component.onDestruction: removalBar.finish()
    }
}
