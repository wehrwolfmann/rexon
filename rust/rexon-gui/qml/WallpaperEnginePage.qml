// The "Wallpaper Engine" tab in QML — the FIRST step of porting the shell.
//
// Data and decisions come from the Rust object WallpaperEngineModel
// (rust/rexon-gui/src/we_page.rs), which decides nothing itself but asks
// the core (`rexon_core::we_window`). Only MARKUP lives here.
//
// Warning: the content of the rail items is the vendored waywallen QML
// (MIT, (c) hypengw; attribution in NOTICE): it lives as a separate module
// `waywallen.ui`, requires built plugins plus a live daemon socket, and is
// loaded INSIDE the `PageHost.qml` shell, not directly here. So the page
// has three start states — "connecting…", "no daemon" with a retry button
// and "plugins not built": the customer must read in words what the tab
// is waiting for rather than stare at an empty rectangle (rule #13). What
// to show and with what words is decided by the core.
//
// Warning: there are no `Qcm.Material` and `waywallen.ui` imports in this
// file, and there must not be. The tab must open even where the plugins
// are NOT built — and a file with such an import does not compile at all
// on such a machine, so instead of the "plugins not built" screen the
// whole tab would silently fail. Everything that needs these modules (the
// Material mode following the skin, and toasts) lives in the shell, which
// is loaded as a separate file and only when the plugins are present.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from the product's
// active skin (rule #17). The page defines no colours of its own.
//
// The navigation rail is like waywallen's (Wehrwolfmann's decision,
// 10.10.2026): it is drawn by the `PageHost.qml` shell with Material
// components, because they cannot be imported here (see above). So the
// shell takes up the WHOLE tab, and this page's banners and state screens
// lie on top of it to the right of the rail (on a narrow tab — above the
// bottom navigation bar that replaces the rail). Of the former rail of
// our own only one item is kept — "Dev"; where the plugins are not built
// and there is no shell, it is shown as a separate button, otherwise the
// build page would be unreachable exactly when it is needed most.

// Delegates refer to ids of outer elements (root, page).
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts
import org.rexon.gui 1.0

Item {
    id: root

    // ── Bridge to the outside ─────────────────────────────────────────────

    // Side-actions object. Our own Rust one by default; the page host may
    // substitute another. It is needed for exactly one thing: bringing up the
    // wallpaper daemon on the "Retry start" button. The page itself does not
    // touch the system — what to show is computed by Rust.
    // Methods: retryDaemon(). Signals: retryStarted(); our own object also
    // has connecting(), daemonFailed(reason), healthReceived(state),
    // healthTimedOut() — daemon events, and the connectDaemon() method.
    property var actions: ownActions

    WeActions {
        id: ownActions
        // The name is needed by checks — the same approach as for the model below.
        objectName: "weActions"
    }

    // The "connecting…" screen after the button is pressed. It is set by the
    // model, not by the actions object: the latter has and must have no
    // pointer to it — the markup connects them.
    //
    // Warning: `ignoreUnknownSignals` is mandatory: the page host may
    // substitute its own actions object without this signal (the Python host
    // of the live tab does that — it switches the model itself), and such a
    // substitution must not drop a warning into the log on every frame.
    //
    // The other four signals are daemon events that the actions object relays
    // to the page (`we_actions.rs`): what they mean and which screen to answer
    // with is decided by the model, i.e. the core.
    Connections {
        target: root.actions
        ignoreUnknownSignals: true
        function onRetryStarted() { page.showConnecting(); }
        function onConnecting() { page.showConnecting(); }
        function onDaemonFailed(reason) { page.showNoDaemon(reason); }
        function onHealthReceived(state) { page.applyHealth(state); }
        function onHealthTimedOut() { page.showHealthTimeout(); }
    }

    // Wait guard: the "Connecting…" screen must END.
    //
    // Warning: without it the wait was endless, and the "Retry start" button
    // never appeared: the two paths to this screen have no timeout of their
    // own at all — a socket that does not open, and a dropped connection,
    // after which the transport reconnects every three seconds with no limit
    // on attempts. Both are analysed in the header of
    // `we_window::CONNECT_TIMEOUT_MS`.
    //
    // There are no decisions here: the timeout is computed by the core, the
    // refusal is composed by the model. The guard is tied to the SCREEN
    // itself, not to an attempt, so it covers any new path to waiting at once
    // (rule #12); a successful connection cancels it by changing the state,
    // and entering the screen again starts it anew — any number of times.
    //
    // Warning: while the engine is starting (`engineStarting`), the guard is
    // paused: the start has its own guard in the core (`we_client::StartWatch`)
    // — it waits for a live process longer than usual and ends with a refusal
    // by itself if the process died or hung. Without this, a long first
    // AppImage start would be cut off here after 25 s.
    Timer {
        interval: page.connectTimeoutMs
        repeat: false
        running: page.state === page.stateConnecting && !ownActions.engineStarting
        onTriggered: page.showConnectTimeout()
    }

    // Directory of the vendored waywallen interface (`qml_ui/` of the
    // package) — named by the page host as an initial property. Empty means
    // there will be no vendored pages and the `Loader` stays empty: that way
    // the page also opens in a process without the `rexon/` package at all.
    property string vendorQmlDir: ""

    // ── Tab core ──────────────────────────────────────────────────────────

    // The type name is WallpaperEngineModel, not WallpaperEnginePage: the
    // latter is taken by this very file (in a QML module every .qml is a type
    // named after the file).
    WallpaperEngineModel {
        id: page

        // The vendored page address is composed by the core, not the markup: here
        // we only pass on the directory the host knows.
        vendorQmlDir: root.vendorQmlDir
        // The name is needed by checks: from the page host the object is fetched
        // with `root.findChild(QObject, "wallpaperEngineModel")` and queried
        // without a single change in the page itself.
        objectName: "wallpaperEngineModel"

        // It has come to the pages — so it is time to build the vendored interface
        // shell. The flag is never cleared again: see `vendorHostWanted`.
        onStateChanged: {
            if (page.state === page.statePages)
                root.vendorHostWanted = true;
        }
    }

    // ── Palette of the active skin ────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    // Muted waiting text: the same text role with a share of opacity from the
    // core — that is how the widget version paints it (`muted_qss` over the
    // `Text` role). The original has no separate skin colour
    // (`text_secondary`) here: on the 03.09 screenshot the "connecting…" text
    // came out pure white in QML versus muted.
    readonly property color colTextMuted: Qt.rgba(root.colText.r, root.colText.g,
                                                  root.colText.b, page.variantAlpha)

    // ── Page of the selected item ─────────────────────────────────────────

    // A binding, not a one-off call: the address recomputes itself when the
    // item changes and when the host answers late.
    //
    // Warning: the directory is read EXPLICITLY, even though `vendorPageUrl`
    // asks for it internally anyway. Without this read the binding does not
    // know about it: QML collects dependencies from the PROPERTIES read, not
    // from what an invokable took from its own state. The tab's first frame on
    // 30.08 showed exactly this — "Wallpapers" opened with a placeholder
    // instead of the page: the address was computed once while the directory
    // was empty, and nothing could update it.
    readonly property url vendorPageUrl: page.vendorQmlDir !== ""
                                         ? page.vendorPageUrl(page.currentIndex)
                                         : ""

    // Whether there is anything to load. The indicator is an explicit
    // conversion to a string rather than comparing a `url` with an empty
    // string: they have different types, and such a comparison relies on a
    // conversion that QML lint rightly dislikes.
    readonly property bool hasVendorPage: String(root.vendorPageUrl) !== ""

    // Whether this page needs the VENDORED HEADER (`MD.AppBar` with its title
    // and action buttons). The answer is held by the core — it is one for both
    // versions of the tab; the question itself is discussed in the header of
    // `PageHost.qml`.
    readonly property bool vendorPageHeader: page.vendorPageHeader(page.currentIndex)

    // ── Hidden "Developer" page ───────────────────────────────────────────

    // Whether the "Dev" item is open now. We compare with the item index from
    // the core, not with the name `dev`: the item's name is its own business,
    // not the markup's.
    readonly property bool onDevPage: page.devIndex >= 0
                                      && page.currentIndex === page.devIndex

    // The page is built when it is reached and lives on (see `devHost`).
    property bool devHostWanted: false

    onOnDevPageChanged: {
        if (root.onDevPage)
            root.devHostWanted = true;
    }

    // ── Lazy shell of the vendored pages ──────────────────────────────────

    // The shell is built ONCE and only when it has come to the pages — as in
    // the widget version, where the shared QML host was created lazily on
    // their first show. Once built, it lives on: a dropped socket returns the
    // tab to "connecting…", and rebuilding the page on every drop would cost
    // the customer its state — the library scroll position, an open card, a
    // typed filter.
    property bool vendorHostWanted: false

    // Restarting the daemon. The actions object touches the system, not the
    // markup; the waiting screen is set by the core meanwhile — with the same
    // transition as on the tab's first start (see `Connections` above).
    function retryDaemon() {
        if (!root.actions)
            return;
        root.actions.retryDaemon();
    }

    // ── First frame ───────────────────────────────────────────────────────

    Component.onCompleted: {
        // We call the daemon ourselves — but only if the actions are OURS. A host
        // that substituted its own object drives the daemon itself, and a second
        // driver would open a second socket. Without this call, a tab in a window
        // without a host stayed on "connecting…" forever.
        if (root.actions === ownActions)
            ownActions.connectDaemon();
    }

    // How much room on the left the rail takes: its width is reported by the
    // shell (it changes when collapsing), and without the shell the room is
    // held by the fallback "Dev" button.
    readonly property real railInset: vendorHost.item
                                      ? vendorHost.item.railWidth
                                      : (devFallback.visible ? devFallback.width : 0)

    // ── Markup ────────────────────────────────────────────────────────────

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ══ Vendored interface shell: rail and page ═════════════════════════
    //
    // What is loaded is the SHELL around the page, not the page: it holds the
    // Material mode following the Rexon skin, the size class, toasts and the
    // navigation rail — that is, the role of their `Window.qml`. It is built
    // ONCE, as soon as it is known that the plugins are built: the rail is
    // visible both on the "connecting…" screen and on a daemon refusal — like
    // the waywallen window, which also opens with the rail without a daemon.
    // The pages themselves are built by the shell later (`pagesReady`).
    //
    // Warning: it is loaded ASYNCHRONOUSLY — like the pages in the original
    // waywallen (`MD.Pool`, QQmlIncubator in Asynchronous mode): a synchronous
    // `Loader` would stall the main thread until compilation finished.
    //
    // There is one engine for all pages: an item changes `pageUrl` rather
    // than creating a second `QQuickWidget`. Several engines with the
    // `waywallen.control` module deadlock `QQmlThread`.
    //
    // Warning: the "Settings" page does not rely on the shell alone: its
    // "Language" switch and the "Cache" section reach into the scene ENGINE
    // ITSELF (`retranslate()` and the disk cache of network images set up by
    // the Rust shim's factory).
    Loader {
        id: vendorHost

        // Warning: the vendored tab is LEFT TO RIGHT even for right-to-left
        // languages: the waywallen markup is not designed for mirroring (`x:`,
        // arrows, column order), and we do not put layout changes of our own into
        // the vendor patch series. Text on it is translated as usual; only the
        // layout around it (the rail and the Rexon sections) is mirrored. Full
        // mirroring of the tab goes through upstream waywallen; once it accepts
        // it, this stopgap is removed.
        // Warning: the stopgap covers only what lies INSIDE the host. Qt puts
        // pop-up windows (`Popup`, `Menu`, the `ComboBox` list) into the window
        // layer (`Overlay`), and they take mirroring from the window, not from
        // here (checked by running with `-reverse` on offscreen, 06.11.2026).
        LayoutMirroring.enabled: false
        LayoutMirroring.childrenInherit: true

        anchors.fill: parent
        asynchronous: true
        active: page.qmlAvailable
        source: page.pageHostUrl
        onLoaded: root.passOutsidePresses()
    }

    // Windows over the vendored interface ("Plugins", "Settings", "About",
    // page questions) cover only this tab, while the Rexon sections above it
    // stay clickable: the window layer over the shell shrinks itself to fit,
    // and presses outside the tab are passed to the window by the actions
    // object's filter. The filter is installed once the layer exists (the
    // shell's `popupLayer`): it must be queued after the layer's filter.
    Connections {
        target: vendorHost.item
        ignoreUnknownSignals: true
        function onPopupLayerChanged() {
            root.passOutsidePresses();
        }
    }

    function passOutsidePresses() {
        if (vendorHost.item && vendorHost.item.popupLayer)
            ownActions.passOutsidePresses(root);
    }

    // Which page the shell shows. By binding, not by assignment in
    // `onLoaded`: the shell is asynchronous, and the item may change before it
    // is built.
    Binding {
        target: vendorHost.item
        property: "pageUrl"
        value: root.vendorPageUrl
        when: vendorHost.status === Loader.Ready
    }

    // The vendored page header is turned on by the HOST, not by the page
    // itself: `MD.Page` draws it according to `MD.MProp.page.showHeader`.
    Binding {
        target: vendorHost.item
        property: "pageHeader"
        value: root.vendorPageHeader
        when: vendorHost.status === Loader.Ready
    }

    // The light or dark tone of the interface is set by the Rexon SKIN, not by
    // the desktop's system theme.
    Binding {
        target: vendorHost.item
        property: "skinDark"
        value: page.skinDark
        when: vendorHost.status === Loader.Ready
    }

    // Colours of the vendored interface: by default the Rexon skin (accent,
    // page background and text); with the "Like waywallen" skin, waywallen's
    // own scheme with its "Theme" and "Accent" settings. The core decides.
    Binding {
        target: vendorHost.item
        property: "followVendorTheme"
        value: page.vendorThemeFollow
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinAccent"
        value: page.vendorAccent
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinBackground"
        value: page.vendorBackground
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinText"
        value: page.vendorText
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "skinRoles"
        value: page.vendorRoles
        when: vendorHost.status === Loader.Ready
    }

    // The rail items and the selected one are named by the core; the shell
    // draws them.
    Binding {
        target: vendorHost.item
        property: "railItems"
        value: page.railItems
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "currentIndex"
        value: page.currentIndex
        when: vendorHost.status === Loader.Ready
    }

    Connections {
        target: vendorHost.item
        function onPageRequested(index) {
            page.currentIndex = index;
        }
    }

    // The pages are built no earlier than the daemon has answered that it is
    // healthy, and they live on after that (see `vendorHostWanted`); they are
    // visible only on the pages screen and only while a vendored item is open.
    Binding {
        target: vendorHost.item
        property: "pagesReady"
        value: root.vendorHostWanted
        when: vendorHost.status === Loader.Ready
    }

    Binding {
        target: vendorHost.item
        property: "pageVisible"
        value: page.state === page.statePages && root.hasVendorPage
        when: vendorHost.status === Loader.Ready
    }

    // This page's banners ("whose background", the Plasma plugin) sit above
    // the shell's page — it must give them room at the top.
    Binding {
        target: vendorHost.item
        property: "pageTopInset"
        value: stateArea.y
        when: vendorHost.status === Loader.Ready
    }

    // ══ Fallback "Dev" button ═══════════════════════════════════════════
    //
    // Only where there is no shell at all (the vendored interface plugins are
    // not built) and developer mode is on. Pressing opens the build page,
    // pressing again returns to the first item.
    RexonButton {
        id: devFallback

        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.margins: 8
        width: page.railWidth - 16
        visible: !page.qmlAvailable && page.devIndex >= 0
        pal: root.pal
        text: page.devIndex >= 0 ? page.railItems[page.devIndex].label : ""
        checkable: true
        checked: root.onDevPage
        onClicked: page.currentIndex = root.onDevPage ? 0 : page.devIndex
    }

    // ══ Right part: banners and the page area (over the shell) ══════════
    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: root.railInset
        // On a narrow tab the rail is replaced by the shell's bottom navigation bar.
        anchors.bottomMargin: vendorHost.item ? vendorHost.item.barHeight : 0
        spacing: 0

        // ── "Whose background" ────────────────────────────────────────────
        //
        // A foreign shell may live on top of the compositor (Noctalia,
        // DankMaterialShell) that draws the background ITSELF — with its own
        // surface over our wallpaper. Then a banner with a choice appears here, in
        // place: "Give the wallpaper to Rexon" (turn off the shell's wallpaper,
        // remembering the previous setting) or, if Rexon has already turned it
        // off, "Give it back to the shell". The decision, text and labels come
        // from the core (`desktop_shell::shell_banner`); the markup only shows
        // them. Without a live shell there is no banner at all.
        Rectangle {
            id: shellBanner
            objectName: "shellBanner"
            Layout.fillWidth: true
            Layout.margins: 12
            Layout.bottomMargin: 0
            visible: page.shellBannerVisible || page.shellActionMessage !== ""
            implicitHeight: shellBannerColumn.implicitHeight + 24
            radius: 6
            color: root.pal.bg_card
            border.width: 1
            border.color: root.pal.border

            ColumnLayout {
                id: shellBannerColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    visible: page.shellBannerVisible
                    text: page.shellBannerText
                    color: root.colText
                    wrapMode: Text.WordWrap
                }

                RowLayout {
                    visible: page.shellBannerVisible
                    spacing: 8

                    RexonButton {
                        objectName: "shellGiveButton"
                        pal: root.pal
                        visible: page.shellCanGive
                        text: page.shellGiveLabel
                        onClicked: page.giveWallpaperToRexon()
                    }

                    RexonButton {
                        objectName: "shellReturnButton"
                        pal: root.pal
                        visible: page.shellCanReturn
                        text: page.shellReturnLabel
                        onClicked: page.returnWallpaperToShell()
                    }
                }

                // Result of the press: success is muted, refusal is bold (the page has no
                // colours of its own, rule #17).
                RexonText {
                    objectName: "shellActionMessage"
                    Layout.fillWidth: true
                    visible: page.shellActionMessage !== ""
                    text: page.shellActionMessage
                    color: page.shellActionFailed ? root.colText : root.colTextMuted
                    font.bold: page.shellActionFailed
                    wrapMode: Text.WordWrap
                }
            }
        }

        // ── Plasma wallpaper plugin ───────────────────────────────────
        //
        // On KDE the waywallen wallpaper is shown by its Plasma plugin. Without the
        // plugin the background silently stays as it was; here this is said in
        // words and installing the plugin is offered. The text, the label and the
        // decision "can it be installed" come from the actions object
        // (`we_actions.rs`); the page only shows them. The page has no copy of
        // these properties of its own: they live on OUR OWN actions object, and a
        // host's substitute does not carry them.
        Rectangle {
            objectName: "kdePluginBanner"
            Layout.fillWidth: true
            Layout.margins: 12
            Layout.bottomMargin: 0
            visible: ownActions.kdePluginNotice !== ""
            implicitHeight: kdePluginColumn.implicitHeight + 24
            radius: 6
            color: root.pal.bg_card
            border.width: 1
            border.color: root.pal.border

            ColumnLayout {
                id: kdePluginColumn
                anchors.fill: parent
                anchors.margins: 12
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    text: ownActions.kdePluginNotice
                    color: root.colText
                    wrapMode: Text.WordWrap
                }

                RexonButton {
                    objectName: "kdePluginInstallButton"
                    pal: root.pal
                    visible: ownActions.kdePluginCanInstall
                    enabled: !ownActions.kdePluginInstalling
                    text: ownActions.kdePluginInstallLabel
                    onClicked: ownActions.installKdePlugin()
                }
            }
        }

        // A shell may be started and stopped while the window is open: "whose
        // background" is recomputed on opening and every few seconds while the tab
        // is visible. The check is cheap — the session directory and `/proc`,
        // without external processes.
        Timer {
            interval: 5000
            repeat: true
            triggeredOnStart: true
            running: root.visible
            onTriggered: page.refreshShellState()
        }

        // ══ Page area ═════════════════════════════════════════════════════
        //
        // The tab's three start states — "connecting…", "no daemon" and "pages" —
        // are the same as in the widget version and switch in the same order. The
        // rail is always visible meanwhile (the shell under this layer holds it):
        // it is part of the tab, not part of the state.
        Item {
            id: stateArea
            Layout.fillWidth: true
            Layout.fillHeight: true

            // ── Waiting for the engine ────────────────────────────────────
            RexonText {
                anchors.centerIn: parent
                width: parent.width - 2 * page.stateColumnMargin
                visible: page.state === page.stateConnecting && !root.onDevPage
                // A long start comes with an explanation that the first start can take a
                // while (the text comes from the core, `we_window::text_connecting_slow`).
                text: ownActions.engineStartNote !== ""
                      ? page.connectingText + "\n\n" + ownActions.engineStartNote
                      : page.connectingText
                color: root.colTextMuted
                font.pixelSize: page.stateTextFontSize
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
            }

            // ── Engine refusal and start retry ────────────────────────────
            // The column spacing and padding come from the core, the same as the
            // `QVBoxLayout` of the widget version. With our own spacing (12) the
            // screen drifted from the original: the icon 12 pixels higher, the text 3
            // (measurement 03.09).
            ColumnLayout {
                anchors.centerIn: parent
                width: parent.width - 2 * page.stateColumnMargin
                spacing: page.stateColumnSpacing
                visible: page.state === page.stateNoDaemon && !root.onDevPage

                RexonText {
                    Layout.alignment: Qt.AlignHCenter
                    text: page.noDaemonIcon
                    font.pixelSize: page.noDaemonIconFontSize
                }

                RexonText {
                    Layout.fillWidth: true
                    // The refusal title and the reason come as one line from the core: it
                    // holds the joining, not the markup.
                    text: page.noDaemonText
                    color: root.colText
                    font.pixelSize: page.stateTextFontSize
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                }

                RexonButton {
                    Layout.alignment: Qt.AlignHCenter
                    // The name is needed by checks: the button is fetched with
                    // `root.findChild(QObject, "retryDaemonButton")` and pressed with a REAL
                    // press, not by bypassing the markup.
                    objectName: "retryDaemonButton"
                    pal: root.pal
                    text: page.retryDaemonLabel
                    // The page always has its own actions object, but the host may null the
                    // property. Then there is nothing to bring the daemon up with, and the
                    // button must LOOK disabled rather than silently do nothing when pressed
                    // (rule #13).
                    enabled: root.actions !== null
                    onClicked: root.retryDaemon()
                }
            }

            // ── Pages ─────────────────────────────────────────────────────
            Item {
                anchors.fill: parent
                // The developer page does not depend on the daemon: on an engine refusal
                // it is needed most (it has the button for installing it).
                visible: page.state === page.statePages || root.onDevPage

                // The vendored interface plugins are not built: instead of the pages,
                // build instructions. The text comes from the core, the same as in the
                // widget version.
                // The ordinary text role, like the original's unstyled label, not the
                // skin's "secondary" colour.
                RexonText {
                    anchors.centerIn: parent
                    width: parent.width - 2 * page.stateColumnMargin
                    visible: !page.qmlAvailable && !root.onDevPage
                    text: page.noQmlModulesText
                    color: root.colText
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                }

                // The vendored page is drawn by the shell under this layer (`vendorHost`
                // above) — there is nothing for it here.
                //
                // Warning: the modal layer (`Overlay.overlay`) is created by the scene's
                // window, and there is one scene for the whole tab: a question of a
                // vendored page (`MD.Dialog` "Kill renderer?" on "Status") covers the
                // rail too, and there is no way to take the page out from under it. This
                // fixes a defect of the widget version rather than being a divergence.
                // Warning: a trap in accepting the "About engine" section: the "wrong
                // colour" of the "Changelog" button on the screenshot is the MOUSE HOVER
                // state (experiment 05.09); there is nothing to reconcile here.

                // The hidden "Developer" page is OURS, from this same QML module, not from
                // the waywallen directory. It is built LAZILY and exactly once: it has its
                // own background task runtime, and rebuilding it every time the item is
                // left would abort an engine installation in progress (~200 MiB) together
                // with its state.
                //
                // Warning: until 06.09.2026 there was a placeholder with the item name
                // here: the page was not ported at all when the tab moved to QML, and the
                // owner, opening "Dev", saw an empty space with the word `dev` — that is,
                // could not reach the button for installing a waywallen update.
                Loader {
                    id: devHost

                    anchors.fill: parent
                    asynchronous: true
                    active: root.devHostWanted
                    visible: root.onDevPage
                    sourceComponent: devPage
                }

                Component {
                    id: devPage

                    WeDeveloperPage {
                        // The parity divergence mark lives in the label of the rail button — the
                        // core sets it, and the page only reports the reconciliation verdict.
                        onParityDrift: (drift) => page.parityDrift = drift
                    }
                }
            }
        }
    }
}
