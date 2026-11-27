// The whole Rexon main window in QML: the host is Rust, there are no
// widgets at all.
//
// The difference from the product is in WHO owns whom. The live window is
// built by Python: a `QMainWindow` with a `QTabWidget`, and the QML pages
// lie INSIDE a `QQuickWidget`. Here it is the other way round: the window
// is an `ApplicationWindow`, the pages are nested in it directly, and the
// data comes from the Rust object `MainWindowModel`
// (rust/rexon-gui/src/main_window.rs), which decides nothing itself and
// asks the core (`rexon_core::qt_main`).
//
// Warning: this is the SKELETON of a separate developer entry point
// (`rust/target/release/rexon-main-window`), not a replacement for the
// product. The live Rexon still starts as before and does not see this
// file.
//
// The window defines no colours of its own: the palette arrives through
// the `palette` property of the same object and originates from
// `ui_base::active_qml_palette` — the default skin of the whole product
// (rule #17).
//
// Repeater delegates refer to ids of outer elements (root, shell, bar).
// Without this pragma, QML lint rightly complains about unqualified access,
// and Qt resolves such names only through the deprecated delegate context.
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.rexon.gui 1.0

ApplicationWindow {
    id: root

    // Right-to-left language (Arabic, Persian, Hebrew): the whole window
    // layout is mirrored from the root — anchors, Row/Grid/Layout, horizontal
    // lists and explicit text alignment (`RexonText`). The direction is set by
    // Qt from Qt's own translation (`QT_LAYOUT_DIRECTION` in qtbase_ar/he/fa);
    // checking without a translation means running with `-reverse`. Where the
    // layout is physical and must not be mirrored, a local
    // `LayoutMirroring.enabled: false` is set. The guard is
    // `rust/rexon-core/tests/rtl_parity.rs`.
    LayoutMirroring.enabled: Qt.application.layoutDirection === Qt.RightToLeft
    LayoutMirroring.childrenInherit: true
    /// The ← → arrows in the window's labels follow its mirroring.
    readonly property RexonArrows arrows: RexonArrows {
        mirrored: root.LayoutMirroring.enabled
    }

    // ── Window core ───────────────────────────────────────────────────────

    MainWindowModel {
        id: shell
        // The name is needed by checks: from outside the object is fetched with
        // `findChild(QObject, "mainWindowModel")` and queried without a single
        // change in this file.
        objectName: "mainWindowModel"
    }

    // Side-actions object of the "Desktops" tab. In the product its place is
    // taken by the Python class `DesktopsActions`; here it is a Rust object
    // with the same nine methods and the same `versionReady` signal. It
    // executes all nine by itself: installing, checking, switching and
    // removing a desktop — through the core executor
    // (`rexon_core::system_de_exec`), with one progress window inside
    // `DesktopsPage.qml` (the `operation` map, the `operationLog` and
    // `desktopsChanged` signals; the Python host has no such properties, and
    // the window is simply not shown there). Details are in the header of
    // src/desktops_actions.rs.
    DesktopsActions {
        id: desktopsActions
        objectName: "desktopsActions"
    }

    // Side-actions object of the "Steam" tab. In the product its place is
    // taken by the Python class `SteamActions` (`rexon/qt_steam/qml_window.py`);
    // here it is a Rust object with the same eight methods, and `SteamPage.qml`
    // does not change by a single line because of it.
    //
    // Warning: it is created HERE, not inside the page's `Component`, for the
    // same reason as its two neighbours: the `--selftest` run must be able to
    // reach it. Create it in the component, and there would be no way to check
    // that the tab's buttons answer to anybody.
    SteamActions {
        id: steamActions
        objectName: "steamActions"
    }

    // Window menu ("☰"). In the product it is built by
    // `RexonMainWindow._build_menubar` — four items with the KDE theme reset,
    // the about box, uninstalling Rexon and quitting behind them. The object
    // is created HERE, not inside the menu markup, for the same reason as its
    // three neighbours: the `--selftest` run must be able to reach it.
    WindowMenuModel {
        id: menuCore
        objectName: "windowMenuModel"
    }

    // ── Window ────────────────────────────────────────────────────────────

    title: shell.windowTitle
    width: shell.windowMinWidth
    height: shell.windowMinHeight
    minimumWidth: shell.windowMinWidth
    minimumHeight: shell.windowMinHeight
    visible: true
    color: root.colBg

    // Tooltips (`ToolTip.text` on the buttons of all tabs) are drawn by the
    // Basic style with the `toolTipBase`/`toolTipText` roles of the window
    // palette, and those are light there: a yellowish-white plate over the
    // dark window — the same trouble the "☰" menu had (screenshot
    // 30.09.2026). There are some fifty tooltips across six tabs, so the colour
    // is set once here, on the window, and the tabs inherit it.
    palette.toolTipBase: root.pal.bg_card
    palette.toolTipText: root.pal.text_primary

    readonly property var pal: shell.palette
    readonly property color colBg: root.pal.bg_primary
    readonly property color colCard: root.pal.surface_card
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary

    // A run without eyes: set as an initial property from Rust (`--selftest`).
    // See `runSelftest` at the bottom of the file.
    property bool selftest: false

    // Tour mode (`--tour`): the window is invisible and walks through the
    // sections by itself. See "Tour mode" at the bottom of the file.
    property bool tourMode: false

    // Section opened at startup (`--section=<id>`). Empty means the first one.
    // An unknown identifier neither stays silent nor crashes the window: it
    // stays on the first section, and the reason goes to the log.
    property string startSection: ""

    // Sub-page opened right at startup: `compare` — "DE comparison" inside
    // "Desktops" (`--page=compare` from the launcher). The section clears the
    // request once fulfilled: entering "Desktops" again must show the desktops
    // themselves, not the comparison again.
    property string startSubpage: ""

    // Directory of the vendored interface of the wallpaper tab (`qml_ui/` of
    // the `rexon/` package). Arrives as an initial property from
    // `bin/main_window.rs`.
    property string weVendorQmlDir: ""

    // The vendor's pop-up page API that the `PageHost.qml` shell installs on
    // the WINDOW (`_installWindowApi`): vendored pages call
    // `Window.window.presentPopup(...)` and `Window.window.showChangelog()`.
    // With the Python host the window is a foreign C++ object, and the JS adds
    // these properties to its wrapper by itself. Here the window is this root,
    // a type declared in QML, and a new property cannot be added to it
    // ("Cannot assign to non-existent property"): without the declaration all
    // pop-up pages of the tab are dead — filters, playlists, tags,
    // "Libraries".
    property var popupPresenter: null
    property var presentPopup: null
    property var showChangelog: null

    /// Open section `id` (and sub-page `subpage`, `compare` — "DE comparison").
    /// One door for the start section and for the request of a second launch
    /// (the activation listener calls it through the tray shim). false means
    /// there is no such section, and the window stays where it was.
    function openSection(id, subpage) {
        const sections = shell.sections;
        let found = -1;
        for (let i = 0; i < sections.length; ++i)
            if (sections[i].id === id)
                found = i;
        if (found < 0)
            return false;
        // The sub-page request is set BEFORE switching: a just-built section
        // reads it on creation, an already built one on the change signal (see
        // the "Desktops" section).
        root.startSubpage = subpage ? String(subpage) : "";
        bar.currentIndex = found;
        return true;
    }

    Component.onCompleted: {
        if (root.startSection !== ""
                && !root.openSection(root.startSection, root.startSubpage))
            console.warn("раздела «" + root.startSection + "» нет; "
                         + "открыт первый");
        if (root.tourMode) {
            // The tour shows neither the wizard, nor the tutorial, nor the look
            // reconciliation — for the same reason as the self-test.
            root.tourSend(null);
        } else if (root.selftest) {
            // The run walks through the sections in order and must start with the
            // first: if it started with the one named by `--section=`, the previous
            // sections would remain unchecked and the run green.
            bar.currentIndex = 0;
            selftestTimer.start();
        } else {
            // The self-test shows neither the installation wizard nor the tutorial:
            // the question would cover the window, and the answer would be written
            // into the user's settings.
            wizardTimer.start();
            // The section at startup goes to the action journal (changing the index
            // from zero to zero gives no signal).
            if (bar.currentIndex >= 0 && bar.currentIndex < shell.sections.length)
                ActionJournal.section(shell.sections[bar.currentIndex].id);
            // And the look reconciliation too: its question would cover the window
            // in the same way.
            aeroshellTimer.start();
        }
    }

    // ── Window menu ───────────────────────────────────────────────────────
    //
    // Exactly `menuBar:`, not `header:`: the header holds the section strip,
    // and the menu must live above it — where the product shows it.

    menuBar: WindowMenu {
        id: menuBarItem
        objectName: "windowMenu"
        menuModel: menuCore
        pal: root.pal
        onItemActivated: (id) => menuDialogs.activate(id)
    }

    WindowMenuDialogs {
        id: menuDialogs
        objectName: "windowMenuDialogs"
        menuModel: menuCore
        pal: root.pal
        onQuitRequested: Qt.quit()
    }

    // ── First-run tutorial ────────────────────────────────────────────────
    //
    // The scenario, texts and progress belong to the core; the progress lies
    // in the shared settings file, so someone who took the tutorial in the
    // Python window will not see it here.

    OnboardingModel {
        id: onboardingCore
        objectName: "onboardingModel"
    }

    OnboardingOverlay {
        id: onboarding
        objectName: "onboardingOverlay"
        core: onboardingCore
        locate: (kind, tabId) => root.onboardingTarget(kind, tabId)
    }

    /// The tutorial waits until the window is shown (a start minimized to the
    /// tray).
    property bool onboardingPending: false

    // As in the Python window: AFTER the first tab. A question over an empty
    // window looks as if the program had not loaded.
    //
    // Warning: this timer is started not by showing the window but by the END
    // OF THE INSTALLATION WIZARD (`firstRun.onFinished`). Previously the
    // tutorial started 700 ms after the window was shown, and on the first
    // launch of a non-installed Rexon two questions would stack up: the
    // tutorial card over the installation window. The wizard itself answers
    // `finished()` immediately when there is nothing to offer — then the
    // tutorial starts exactly as before.
    Timer {
        id: onboardingTimer
        interval: 700
        onTriggered: root.maybeStartOnboarding()
    }

    // ── First-run wizard ──────────────────────────────────────────────────
    //
    // The decision "whether to offer installation" and all labels belong to
    // the core; the overlay only asks the bridge
    // (`FirstRunWizardOverlay.begin()`). The tutorial waits for its end, and
    // the look reconciliation for the end of both.

    FirstRunWizardOverlay {
        id: firstRun
        objectName: "firstRunWizardOverlay"
        anchors.fill: parent
        pal: root.pal
        onFinished: root.maybeStartOnboarding()
    }

    /// The wizard waits until the window is shown (a start minimized to the
    /// tray).
    property bool wizardPending: false

    // The same delay as for the tutorial, and for the same reason: a question
    // over an empty window looks as if the program had not loaded.
    Timer {
        id: wizardTimer
        interval: 700
        onTriggered: root.maybeStartWizard()
    }

    function maybeStartWizard() {
        if (!root.visible) {
            root.wizardPending = true;
            return;
        }
        root.wizardPending = false;
        firstRun.begin();
    }

    function maybeStartOnboarding() {
        if (!root.visible) {
            root.onboardingPending = true;
            return;
        }
        root.onboardingPending = false;
        const decision = onboardingCore.start();
        // To the log: it shows why the tutorial appeared or not.
        console.info("обучение при запуске: " + decision.kind
                     + (decision.step ? " " + decision.step : ""));
        onboarding.begin(decision);
    }

    onVisibleChanged: {
        if (root.visible && root.wizardPending)
            wizardTimer.restart();
        if (root.visible && root.onboardingPending)
            onboardingTimer.restart();
        if (root.visible && root.aeroshellPending)
            aeroshellTimer.restart();
    }

    // ── The AeroShell look has fallen behind the system ───────────────────
    //
    // As in the Python window (`rexon/qt_main/window.py`): the reconciliation
    // comes AFTER the tutorial — two windows at once on the first launch is
    // too much. Here there is only the reconciliation and the question; the
    // rebuild happens exclusively by pressing "Restore" in the window. The
    // actions object and the window are the same as for the "Check now" button
    // in "Settings" (`settings_actions.rs`, core executor `system_de_exec`).

    SettingsActions {
        id: aeroshellActions
        objectName: "aeroshellActions"
    }

    DeOperationDialog {
        anchors.fill: parent
        actions: aeroshellActions
        pal: root.pal
    }

    /// The reconciliation waits until the window is shown (a start minimized
    /// to the tray).
    property bool aeroshellPending: false

    Timer {
        id: aeroshellTimer
        interval: 1500
        onTriggered: root.maybeCheckAeroshell()
    }

    function maybeCheckAeroshell() {
        if (!root.visible) {
            root.aeroshellPending = true;
            return;
        }
        // The installation wizard or the tutorial is still on screen — wait for
        // them to finish: a question over someone else's window would read as its
        // continuation.
        if (firstRun.opened || onboarding.opened) {
            aeroshellTimer.restart();
            return;
        }
        root.aeroshellPending = false;
        aeroshellActions.aeroshellStartupCheck();
    }

    /// Rectangle of the tutorial target in window coordinates, or null. "tab"
    /// is the tab header, "tab_content" its content (the window switches to the
    /// tab, as in the Python window).
    function onboardingTarget(kind, tabId) {
        const sections = shell.sections;
        let index = -1;
        for (let i = 0; i < sections.length; ++i)
            if (sections[i].id === tabId)
                index = i;
        if (index < 0)
            return null;
        let item = null;
        if (kind === "tab") {
            item = bar.itemAt(index);
        } else if (kind === "tab_content") {
            if (bar.currentIndex !== index)
                bar.currentIndex = index;
            item = root.sectionHolder(index);
        }
        if (!item || !item.visible || item.width <= 0 || item.height <= 0)
            return null;
        const p = item.mapToItem(null, 0, 0);
        return { x: p.x, y: p.y, width: item.width, height: item.height };
    }

    // ── Section strip ─────────────────────────────────────────────────────

    header: TabBar {
        id: bar

        // Section change: (1) into the action journal — the watchdog and the
        // button lines name the section the user was in; (2) the footer summary
        // ("… — Done ✅") belongs to the section where it was received — what to
        // clear is decided by the core (`system_de_exec::footer_hold`).
        onCurrentIndexChanged: {
            const sections = shell.sections;
            if (bar.currentIndex >= 0 && bar.currentIndex < sections.length)
                ActionJournal.section(sections[bar.currentIndex].id);
            desktopsActions.sectionChanged();
        }

        background: Rectangle {
            color: root.colCard
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: root.colBorder
            }
        }

        Repeater {
            model: shell.sections

            TabButton {
                id: tabButton
                required property int index
                required property var modelData

                text: tabButton.modelData.label
                // Height ≥ 40 px is a Big Picture requirement (architecture rule #16),
                // not a number of taste.
                implicitHeight: 44
                font.pointSize: 11
                // The width follows OUR label, not the platform style. In a KDE session
                // the controls take the org.kde.desktop style: it measures a tab with its
                // own font (10 pt versus our 11) and also with its own selection frame,
                // and the selected "Wallpaper Engine" was cut to "Wallpaper Engin" (test
                // bench 08.11.2026). The padding leaves room for the chip's margins on
                // both sides.
                leftPadding: shell.tabStyle.marginH + 10
                rightPadding: shell.tabStyle.marginH + 10
                implicitWidth: Math.ceil(tabButton.contentItem.implicitWidth)
                               + tabButton.leftPadding + tabButton.rightPadding

                contentItem: RexonText {
                    text: tabButton.text
                    font: tabButton.font
                    color: bar.currentIndex === tabButton.index
                           ? root.colAccent : root.colText
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                }

                // A "chip" plate on EVERY tab, as in the widget window.
                //
                // Warning: an inactive tab is NOT transparent. It was transparent until
                // 18.09.2026, and the section strip came out as one solid block: the
                // neighbours had neither a fill nor a gap, and it was impossible to see
                // where one tab ended and the next began (Wehrwolfmann's remark). Colours
                // and margins come from the core (`qt_main::TAB_*` through
                // `shell.tabStyle`), shared with the widget window; the markup defines no
                // numbers of its own.
                background: Rectangle {
                    color: "transparent"

                    Rectangle {
                        id: tabChip
                        anchors.fill: parent
                        anchors.leftMargin: shell.tabStyle.marginH
                        anchors.rightMargin: shell.tabStyle.marginH
                        anchors.topMargin: shell.tabStyle.marginV
                        anchors.bottomMargin: shell.tabStyle.marginV
                        radius: shell.tabStyle.radius
                        color: bar.currentIndex === tabButton.index
                               ? root.colBg
                               : (tabButton.hovered ? shell.tabStyle.hoverFill
                                                    : shell.tabStyle.idleFill)
                        // Frame of the focused button: the focus chain is built in from the very
                        // start, not "when we get to the controller".
                        border.width: tabButton.activeFocus ? 2 : 0
                        border.color: root.colAccent

                        Rectangle {
                            anchors.bottom: parent.bottom
                            anchors.left: parent.left
                            anchors.right: parent.right
                            height: 2
                            color: bar.currentIndex === tabButton.index
                                   ? root.colAccent : "transparent"
                        }
                    }
                }
            }
        }
    }

    // ── Page map: identifier → markup ─────────────────────────────────────
    //
    // There used to be a nail here: `Loader.sourceComponent` looked straight
    // at `comparePage`, and `wake()` compared the identifier with the literal
    // "compare" — exactly one section out of six could open, the other five
    // showed an explanation instead of a page.
    //
    // The map is the only place where the window knows which markup stands
    // behind a page identifier. WHICH section gets it is not decided by the
    // window at all: that is the `SECTION_PAGES` table in
    // src/main_window.rs.
    //
    // ALL pages that already exist in QML are listed, including one the
    // section table does not pick ("compare": the comparison is not a section
    // but the second page of "Desktops", and the section holder below puts it
    // there). The map is a catalogue of what is available, not a second copy
    // of the table. The reverse divergence — the table named a page the map
    // does not know — is caught by the `every_page_is_in_the_qml_map` guard.
    readonly property var pageComponents: ({
        "compare": comparePage,
        "desktops": desktopsPage,
        "appearance": appearancePage,
        "extensions": extensionsPage,
        "wallpaper_engine": wallpaperEnginePage,
        "steam": steamPage,
        "settings": settingsPage
    })

    // ── Section contents ──────────────────────────────────────────────────

    StackLayout {
        id: stack
        anchors.fill: parent
        currentIndex: bar.currentIndex

        Repeater {
            model: shell.sections

            Item {
                id: section
                required property int index
                required property var modelData

                /// This section's markup from the window map, or null if it has no page.
                /// This is where all the section's knowledge of what it shows ends.
                readonly property Component pageComponent:
                    root.pageComponents[section.modelData.page] ?? null

                /// The section's built page, or null — checks fetch it through this field
                /// without groping through the tree.
                readonly property Item page: pageLoader.item as Item

                // A page that is named but unknown to the map must not silently turn into
                // "section not ported yet": the text would lie about the cause, and the
                // cause is a divergence between the table and the map.
                Component.onCompleted: {
                    if (section.modelData.page !== ""
                            && section.pageComponent === null)
                        console.warn("страница «" + section.modelData.page
                                     + "» раздела «" + section.modelData.id
                                     + "» не значится в карте pageComponents");
                }

                Loader {
                    id: pageLoader
                    anchors.fill: parent
                    // Pages load asynchronously and lazily — as in the product: synchronous
                    // compilation of all six sections would land entirely on the window
                    // start.
                    asynchronous: true
                    active: false
                    visible: section.pageComponent !== null
                    sourceComponent: section.pageComponent

                    // A page is built when its section is opened for the FIRST time and stays
                    // built afterwards: the customer should not pay with compilation for every
                    // return to the tab.
                    function wake() {
                        if (section.pageComponent !== null
                                && bar.currentIndex === section.index)
                            pageLoader.active = true;
                    }

                    Component.onCompleted: pageLoader.wake()

                    Connections {
                        target: bar
                        function onCurrentIndexChanged() { pageLoader.wake(); }
                    }
                }

                // A section that has no Rust host yet. An empty page reads as a breakage
                // (rule #13), so the explanation comes from the core rather than being
                // drawn as emptiness.
                RexonText {
                    anchors.centerIn: parent
                    width: Math.min(parent.width - 80, 560)
                    visible: section.pageComponent === null
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    font.pointSize: 14
                    color: root.colTextSecondary
                    text: shell.notPortedNotice(section.modelData.id)
                }
            }
        }
    }

    // The pages are THE SAME ones the live Rexon opens, and none of them knows
    // who its host is. A `Component` builds nothing by itself until asked, so
    // the seven declarations cost the window start exactly zero: only the page
    // whose section was opened is built.
    //
    // Every page that needs a side-actions object now has one, and it reaches
    // the page in two different ways — this is not carelessness but a
    // difference in the page's own contract:
    //   * "DE comparison", "Desktops" and "Steam" expect it as the `actions`
    //     property, and the host provides it (the objects are declared above);
    //   * "Extensions" and "Wallpaper Engine" create it FOR THEMSELVES
    //     (`ExtensionsActions { }`, `WeActions { }` right in the .qml), so
    //     nothing is said about them here.
    // The pages still tolerate an empty `actions`: each checks it for null
    // itself, and none crashes because of it.
    Component {
        id: comparePage

        ComparePage {
            // The same object as for "Desktops": installing and switching a desktop go
            // through one executor and one progress window.
            actions: desktopsActions
        }
    }

    // The "Desktops" section is TWO pages, exactly as in the product: the
    // desktop showcase and the embedded "DE comparison" (it stopped being a
    // separate tab on 08.07.2026). The widget tab held them in a
    // `QStackedWidget`; here `StackLayout` does the same.
    //
    // The comparison cannot be inserted INSIDE the desktops scene: it is a
    // separate page with its own model. So the host switches the pages, and
    // the tab itself only asks for it — with the `compareRequested` signal of
    // the actions object. The back button label comes from the core
    // (`compareBackText`); there is no second copy of the text here.
    Component {
        id: desktopsPage

        Item {
            id: desktopsHolder

            // The section title is queried by the `--selftest` run, and the desktops
            // page ITSELF must provide it: put your own string here, and the check
            // "window host → page → model" would go green without checking anything.
            readonly property string windowTitle: desktopsView.windowTitle

            /// Which of the section's two pages is shown: 0 — desktops, 1 — comparison.
            /// The run uses this field to check that the request to show "DE
            /// comparison" leads somewhere.
            readonly property int subPage: desktopsStack.currentIndex

            /// Which desktop is selected and in which session: the run requires the
            /// tab to open on the current desktop.
            readonly property string selectedDe: desktopsView.selectedDe
            readonly property string currentDe: desktopsView.currentDe

            StackLayout {
                id: desktopsStack
                anchors.fill: parent
                currentIndex: 0

                DesktopsPage {
                    id: desktopsView
                    actions: desktopsActions
                }

                ColumnLayout {
                    spacing: 0

                    RexonButton {
                        Layout.leftMargin: 12
                        Layout.topMargin: 10
                        Layout.bottomMargin: 4
                        pal: root.pal
                        // The "back" arrow points to the start of the line (`RexonArrows.keyHint`).
                        text: root.arrows.keyHint(desktopsView.compareBackText)
                        onClicked: desktopsStack.currentIndex = 0
                    }

                    // The comparison is taken from the same `pageComponents` map and built
                    // only when requested: until the user presses "Compare", the section
                    // should not pay for compiling the second page.
                    Loader {
                        id: compareLoader
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        asynchronous: true
                        active: false
                        sourceComponent: root.pageComponents["compare"]
                    }
                }
            }

            function showCompare() {
                compareLoader.active = true;
                desktopsStack.currentIndex = 1;
            }

            Connections {
                target: desktopsActions

                function onCompareRequested() { desktopsHolder.showCompare(); }
            }

            // `--page=compare` (at startup or from a second launch): the same door as
            // the "Compare" button. The request is cleared once fulfilled.
            function takeSubpage() {
                if (root.startSubpage === "compare") {
                    root.startSubpage = "";
                    desktopsHolder.showCompare();
                }
            }

            Component.onCompleted: desktopsHolder.takeSubpage()

            Connections {
                target: root

                function onStartSubpageChanged() { desktopsHolder.takeSubpage(); }
            }
        }
    }

    Component {
        id: appearancePage

        AppearancePage {}
    }

    Component {
        id: extensionsPage

        ExtensionsPage {}
    }

    Component {
        id: wallpaperEnginePage

        // The directory of the vendored waywallen interface is named by the
        // process host (`we_env::vendor_qml_dir`, initial property
        // `weVendorQmlDir`). Empty means there is no `rexon/` package nearby, and
        // the page honestly opens without vendored pages, as it is written.
        WallpaperEnginePage {
            vendorQmlDir: root.weVendorQmlDir
        }
    }

    Component {
        id: steamPage

        SteamPage {
            actions: steamActions
        }
    }

    Component {
        id: settingsPage

        SettingsPage {}
    }

    // ── Footer: what a side action answered ───────────────────────────────

    // The answer of the LAST press, not of one chosen object: there are now
    // two side-actions objects, and "show only the first one" would mean that
    // half of the buttons answer into the void — that is, look broken
    // (rule #13).
    property string footerStatus: ""

    Connections {
        target: desktopsActions
        function onStatusChanged() {
            root.footerStatus = desktopsActions.status;
            // The line's lifetime comes from the core according to its kind: a
            // summary goes away by itself, an error and a wait for an answer are not
            // cleared by a timer.
            const ms = desktopsActions.statusTimeoutMs();
            footerTimer.stop();
            if (ms > 0) {
                footerTimer.interval = ms;
                footerTimer.start();
            }
        }
    }

    Timer {
        id: footerTimer
        objectName: "footerTimer"
        repeat: false
        onTriggered: desktopsActions.expireStatus()
    }

    footer: Rectangle {
        implicitHeight: root.footerStatus === "" ? 0 : 34
        visible: root.footerStatus !== ""
        color: root.colCard

        Rectangle {
            anchors.top: parent.top
            width: parent.width
            height: 1
            color: root.colBorder
        }

        RexonText {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            color: root.colTextSecondary
            font.pointSize: 10
            text: root.footerStatus
        }

        // A click on the footer removes a summary or an error (not a wait for an
        // answer: that is cleared by the question window).
        MouseArea {
            anchors.fill: parent
            onClicked: desktopsActions.dismissStatus()
        }
    }

    // ── Tour mode (`--tour`) ──────────────────────────────────────────────
    //
    // An executor, not a decider: what to do next, when the screen is ready
    // and what looks suspicious in what was seen is decided by the core
    // (`main_window_tour`), which `shell.tourNext` asks. Here there is only
    // the execution of the command and the measurement.
    //
    // Never: the tour installs, applies and removes nothing. It touches a page
    // through ONE door — `tourInvoke`, which calls a method only if the core
    // has allowed it (`shell.tourCallAllowed`). The page does not hand any
    // buttons to the tour at all.

    QtObject {
        id: tour
        /// Command in progress (parsed core JSON).
        property var cmd: null
        /// Execution step: page | settle | load | preview | grab.
        property string phase: ""
        property double phaseAt: 0
        /// Answer accumulated for the core.
        property var result: ({})
        property int lastNoPreview: -1
        property double stableSince: 0
    }

    Timer {
        id: tourTimer
        interval: 200
        repeat: true
        onTriggered: root.tourTick()
    }

    /// Hand the core the answer to the previous command and take on the next.
    function tourSend(report) {
        tourTimer.stop();
        const cmd = JSON.parse(shell.tourNext(report === null ? "" : JSON.stringify(report)));
        tour.cmd = cmd;
        tour.result = {};
        if (cmd.cmd === "finish") {
            Qt.exit(cmd.exit_code);
            return;
        }
        root.tourStart(cmd);
    }

    /// The tour's only door to a page: a method is called only if the core has
    /// allowed it.
    function tourInvoke(cmd, arg) {
        if (!shell.tourCallAllowed(cmd.method)) {
            tour.result.error = "вызов «" + cmd.method + "» запрещён ядром";
            return false;
        }
        const target = cmd.method === "openSection" ? root : root.tourPage();
        if (!target || typeof target[cmd.method] !== "function") {
            tour.result.error = "у страницы нет «" + cmd.method + "»";
            return false;
        }
        return target[cmd.method](arg) !== false;
    }

    /// Built page of the open section, or null.
    function tourPage() {
        const holder = root.sectionHolder(bar.currentIndex);
        return holder ? holder.page : null;
    }

    function tourPhase(name) {
        tour.phase = name;
        tour.phaseAt = Date.now();
        tourTimer.start();
    }

    function tourStart(cmd) {
        switch (cmd.cmd) {
        case "openSection":
            if (!root.tourInvoke(cmd, cmd.section)) {
                root.tourSend({ built: false, error: tour.result.error || "раздела нет в окне" });
                return;
            }
            root.tourPhase("page");
            return;
        case "selectCategory":
            tour.result.ok = root.tourInvoke(cmd, cmd.key);
            root.tourPhase("settle");
            return;
        case "setSource":
            tour.result.ok = root.tourInvoke(cmd, cmd.key);
            root.tourPhase("settle");
            return;
        case "typeQuery":
            tour.result.ok = root.tourInvoke(cmd, cmd.text);
            root.tourPhase("settle");
            return;
        case "clearQuery":
            tour.result.ok = root.tourInvoke(cmd, undefined);
            root.tourPhase("settle");
            return;
        case "capture":
            tour.lastNoPreview = -1;
            root.tourPhase("load");
            return;
        default:
            root.tourSend({ error: "незнакомая команда «" + cmd.cmd + "»" });
        }
    }

    function tourTick() {
        const cmd = tour.cmd;
        const elapsed = Date.now() - tour.phaseAt;
        const page = root.tourPage();
        const st = page && typeof page.tourState === "function" ? page.tourState() : null;
        if (tour.phase === "page") {
            const appearance = !!page && typeof page.tourCategories === "function";
            const cats = appearance ? page.tourCategories() : [];
            const ready = !!page && (!appearance || cats.length > 0);
            if (!ready && elapsed < cmd.timeout_ms)
                return;
            root.tourSend({ ok: ready, built: !!page,
                            kind: appearance ? "appearance" : "", categories: cats });
        } else if (tour.phase === "settle") {
            // The section header (chips, search field) is updated by a model signal,
            // not by the same call: give it a frame or two.
            if (elapsed < 300)
                return;
            const r = tour.result;
            if (cmd.cmd === "selectCategory" && page) {
                r.sources = page.tourSources();
                r.search = st ? st.search : false;
            }
            if (st)
                r.source = st.source;
            root.tourSend(r);
        } else if (tour.phase === "load") {
            const verdict = shell.tourWaitVerdict(JSON.stringify({
                phase: "load", elapsed_ms: elapsed, timeout_ms: cmd.load_timeout_ms,
                busy: st ? st.busy : false, status: st ? st.status : "" }));
            if (verdict === "wait")
                return;
            tour.result.load_ms = elapsed;
            tour.result.load_timed_out = verdict === "timeout";
            if (cmd.grid && st) {
                tour.lastNoPreview = st.noPreview;
                tour.stableSince = Date.now();
                root.tourPhase("preview");
            } else {
                root.tourGrab();
            }
        } else if (tour.phase === "preview") {
            if (st.noPreview !== tour.lastNoPreview) {
                tour.lastNoPreview = st.noPreview;
                tour.stableSince = Date.now();
            }
            const verdict = shell.tourWaitVerdict(JSON.stringify({
                phase: "preview", elapsed_ms: elapsed, timeout_ms: cmd.preview_timeout_ms,
                no_preview: st.noPreview, previews_pending: st.pending,
                stable_ms: Date.now() - tour.stableSince, settle_ms: cmd.settle_ms }));
            if (verdict === "wait")
                return;
            tour.result.preview_timed_out = verdict === "timeout";
            root.tourGrab();
        }
    }

    /// Capture the whole window (with the section strip and menu) into the
    /// command's file.
    function tourGrab() {
        tourTimer.stop();
        tour.phase = "grab";
        const page = root.tourPage();
        const st = page && typeof page.tourState === "function" ? page.tourState() : null;
        const r = tour.result;
        r.ok = true;
        if (st) {
            r.screen = st.screen;
            r.tiles = st.tiles;
            r.no_preview = st.noPreview;
            r.search_visible = st.searchVisible;
            r.search_x = st.searchX;
            r.search_y = st.searchY;
            r.search_text = st.searchText;
            r.model_query = st.query;
            r.status = st.status;
            r.status_warn = st.statusWarn;
            r.source = st.source;
        }
        // The whole window (`QQuickWindow::grabWindow`): `grabToImage` cannot do
        // the window root — the root element does not belong to the QML engine.
        r.saved = shell.tourGrab(tour.cmd.path);
        root.tourSend(r);
    }

    // ── Self-test ─────────────────────────────────────────────────────────

    // A test bench that is always green checks nothing: every check is named
    // individually, and a failure is printed as a separate
    // REXON-MAINWINDOW-FAIL line and sets the exit code to one.
    //
    // The run walks through ALL sections in turn, not only the open one. While
    // there was a single page, "the current section was built" and "the window
    // works" meant the same thing; with six pages such a check would skip
    // five — and a divergence between the map and the table would stay green.
    //
    // The current section's page loads ASYNCHRONOUSLY, and in the very first
    // frame it is not there yet. Checking at once would measure not the page's
    // readiness but the scheduler's speed: the first run turned red exactly
    // like that ("the current section's page is not built"), although the
    // window was fine. So every section has its own wait loop.

    /// Accumulated tour failures; `runSelftest` gives the verdict on them.
    property var selftestFailures: []

    Timer {
        id: selftestTimer
        interval: 100
        repeat: true
        property int ticks: 0
        onTriggered: {
            const holder = root.sectionHolder(bar.currentIndex);
            // A section without a page has nothing to wait for — it is ready at once.
            const waiting = holder !== null && holder.pageComponent !== null
                            && !holder.page;
            // 40 attempts of 100 ms = 4 s per section, six sections — 24 s. The number
            // is chosen to fit the binary's watchdog (30 s): outlast the watchdog, and
            // the failure would become a nameless "the window did not report" instead
            // of "the page of such-and-such section was not built". The tour measured
            // on this machine takes 1.06 s for all six, a twentyfold margin. Not
            // waiting long enough is already a failure, not "let's wait some more":
            // the page must build.
            if (waiting && selftestTimer.ticks++ < 40)
                return;
            selftestTimer.stop();
            selftestTimer.ticks = 0;
            root.checkSection();
        }
    }

    /// Section holder by its index.
    ///
    /// By search, not `stack.children[i]`: the children of a StackLayout also
    /// include the Repeater itself, and a child's index need not match the
    /// section's index. While there was one section the difference was
    /// invisible — with six it would become a silent check of the wrong page.
    function sectionHolder(index) {
        for (let i = 0; i < stack.children.length; ++i)
            if (stack.children[i].index === index)
                return stack.children[i];
        return null;
    }

    /// Verdict on ONE section; then the run moves on to the next.
    function checkSection() {
        const sections = shell.sections;
        const index = bar.currentIndex;
        const id = sections[index].id;
        const holder = root.sectionHolder(index);
        const page = holder ? holder.page : null;
        const wanted = holder !== null && holder.pageComponent !== null;

        if (holder === null)
            root.selftestFailures.push("раздел «" + id + "» не построен вовсе");
        else if (sections[index].page !== "" && !wanted)
            // The section table named a page that the window map does not know.
            // Without this check the run would stay green: the section shows the
            // explanation "not ported yet", which looks like a regular placeholder —
            // so the divergence would hide behind a healthy look.
            root.selftestFailures.push("страницы «" + sections[index].page
                                       + "» раздела «" + id
                                       + "» нет в карте pageComponents");
        else if (wanted && !page)
            root.selftestFailures.push("страница раздела «" + id
                                       + "» не построена");

        // The title from the Rust model is an end-to-end check "window host →
        // page → model". Not all pages provide one: the Wallpaper Engine page has
        // no title of its own at all, and requiring one would invent a failure out
        // of nothing.
        const hasTitle = page !== null && page.windowTitle !== undefined;
        console.log("REXON-MAINWINDOW-PAGE " + id
                    + " | " + (page ? "построена" : (wanted ? "НЕТ" : "заглушка"))
                    + " | " + (hasTitle ? page.windowTitle : "—"));
        if (hasTitle && page.windowTitle === "")
            root.selftestFailures.push("страница раздела «" + id
                                       + "» не отдала заголовок от Rust-модели");

        if (index + 1 < sections.length) {
            bar.currentIndex = index + 1;
            selftestTimer.start();
        } else {
            // The sections are done — now the window menu. It is OPENED: while the
            // menu is closed its items have no width, and a label that did not fit is
            // indistinguishable from one that did.
            menuBarItem.openForCheck();
            menuLayoutTimer.start();
        }
    }

    // The menu is not laid out in the same frame in which it opened: the items
    // get their width from the next layout pass. Measuring at once would
    // measure zeros and call it "fits".
    Timer {
        id: menuLayoutTimer
        interval: 50
        repeat: true
        property int ticks: 0
        onTriggered: {
            // 40 attempts of 50 ms = 2 s. If it is not laid out within that time, it
            // is already a failure, and it will be named as such in `runSelftest`.
            if (!menuBarItem.laidOut() && menuLayoutTimer.ticks++ < 40)
                return;
            menuLayoutTimer.stop();
            menuLayoutTimer.ticks = 0;
            root.runSelftest();
        }
    }

    function runSelftest() {
        let failures = root.selftestFailures;
        const sections = shell.sections;
        console.log("REXON-MAINWINDOW-TITLE " + shell.windowTitle);
        console.log("REXON-MAINWINDOW-SECTIONS " + sections.length);
        let withPage = 0;
        for (let i = 0; i < sections.length; ++i) {
            console.log("REXON-MAINWINDOW-SECTION " + sections[i].id
                        + " | " + sections[i].label
                        + " | " + sections[i].page);
            if (sections[i].page !== "")
                ++withPage;
        }
        console.log("REXON-MAINWINDOW-PAGES " + withPage + "/" + sections.length);
        if (sections.length === 0)
            failures.push("состав разделов пуст");
        if (shell.windowTitle === "")
            failures.push("заголовок окна пуст");
        // This is what the run was set up for: EVERY section must have its own
        // page. As long as even one opens with a placeholder, the Rust window
        // cannot replace the Python window.
        if (withPage !== sections.length)
            failures.push("разделов без своей страницы: "
                          + (sections.length - withPage));

        // A side action must ANSWER, not stay silent. The actions object of
        // "Desktops" is also that of "DE comparison". The install button starts
        // nothing by itself: it opens a question window (or a distribution
        // refusal) and writes into the footer that it is waiting for an answer.
        // An empty footer would mean the button stayed silent — the customer
        // would take it for a breakage. The run never answers "Yes" and closes the
        // opened window at once: there is never a password or a package manager
        // here.
        desktopsActions.install("kde");
        console.log("REXON-MAINWINDOW-DESKTOPS-STATUS " + desktopsActions.status);
        if (desktopsActions.status === "")
            failures.push("побочное действие «Рабочих столов» ничего не сказало");
        desktopsActions.dismissOperation();

        // The same question to the "Steam" actions object. We ask TWO things, and
        // both without leaving a single trace on the machine:
        //   * the showcase snapshot must be a map with all six keys the page
        //     reads; if it returned `undefined`, the showcase would stay empty and
        //     the toggles would fall to "off" by themselves;
        //   * showing WITHOUT a selected clip must answer in words. The page
        //     understands an empty answer as "it started" and writes "▶ Playing…",
        //     so a silent refusal would look like success.
        const snapshot = steamActions.scan();
        const missing = [];
        for (const key of ["movies", "current", "favorites",
                           "enabled", "rotate", "sound"])
            if (!snapshot || snapshot[key] === undefined)
                missing.push(key);
        console.log("REXON-MAINWINDOW-STEAM-SNAPSHOT "
                    + (snapshot ? Object.keys(snapshot).length : -1)
                    + " | пропущено: " + (missing.length ? missing.join(",") : "—"));
        if (missing.length)
            failures.push("снимок вкладки «Steam» без ключей: " + missing.join(","));

        // The third question is about the Steam root, and it is about a silent
        // loss.
        //
        // The page starts its background tasks by itself, in
        // `Component.onCompleted`. While the root came from outside, from the
        // Python host, under that host there was NOBODY to tell it — and the
        // "Steam profile backgrounds" section stayed empty forever, while the tab
        // looked working. Now the page finds the root itself, and the
        // reconciliation requires it to know exactly what the actions object
        // finds: if they differ, it has been left without a root again.
        const actionsRoot = steamActions.steamRoot();
        const pageHolder = root.sectionHolder(4);
        const pageRoot = pageHolder ? pageHolder.page : null;
        const knownRoot = pageRoot ? pageRoot.steamFoundPath : "";
        console.log("REXON-MAINWINDOW-STEAM-ROOT "
                    + (knownRoot === "" ? "—" : knownRoot)
                    + " | у объекта действий: "
                    + (actionsRoot === "" ? "—" : actionsRoot));
        if (knownRoot !== actionsRoot)
            failures.push("страница «Steam» не знает корня Steam: у неё «"
                          + knownRoot + "», а найден «" + actionsRoot + "»");

        const steamRefusal = steamActions.play("", false);
        console.log("REXON-MAINWINDOW-STEAM-PLAY " + steamRefusal);
        if (steamRefusal === "")
            failures.push("вкладка «Steam» промолчала об отказе показать заставку");

        // "DE comparison" is no longer a separate section but the second page of
        // "Desktops". If the request led nowhere, the comparison would simply
        // vanish from the window, and the tab's look would not reveal it.
        const desktopsHolder = root.sectionHolder(0);
        const desktopsRoot = desktopsHolder ? desktopsHolder.page : null;
        // The tab opens on the SESSION's desktop (acceptance 10.10.2026 and again
        // 20.10.2026: in MATE Cinnamon was selected — the first in the list). If
        // the session is not recognized, there is nothing to compare against.
        if (desktopsRoot) {
            console.log("REXON-MAINWINDOW-DESKTOPS-SELECTED " + desktopsRoot.selectedDe
                        + " | сеанс: " + (desktopsRoot.currentDe || "—"));
            if (desktopsRoot.currentDe !== ""
                    && desktopsRoot.selectedDe !== desktopsRoot.currentDe)
                failures.push("«Рабочие столы» открылись на «" + desktopsRoot.selectedDe
                              + "», а не на столе сеанса «" + desktopsRoot.currentDe + "»");
        }
        desktopsActions.openCompare();
        console.log("REXON-MAINWINDOW-DESKTOPS-SUBPAGE "
                    + (desktopsRoot ? desktopsRoot.subPage : -1));
        if (!desktopsRoot || desktopsRoot.subPage !== 1)
            failures.push("просьба показать «Сравнение DE» никуда не привела");

        // ── Window menu ──────────────────────────────────────────────
        //
        // A complete list of items proves NOTHING: the model may be complete while
        // the delegates fail to build — and the menu turns out empty with a
        // healthy core. So the yardstick here is twofold: what the core said and
        // how many items were ACTUALLY created in the `Menu`.
        const menuItems = menuCore.items;
        const wantShortcuts = menuItems.filter(e => e.shortcut !== "").length;
        console.log("REXON-MAINWINDOW-MENU " + menuItems.length
                    + " | построено: " + menuBarItem.builtItems
                    + " | разделителей: " + menuBarItem.builtSeparators
                    + " | клавиш: " + menuBarItem.builtShortcuts
                    + " из " + wantShortcuts
                    + " | кнопка: " + menuCore.buttonText);
        for (const e of menuItems)
            console.log("REXON-MAINWINDOW-MENU-ITEM " + e.id + " | " + e.label
                        + " | " + (e.shortcut === "" ? "—" : e.shortcut)
                        + " | " + (e.separatorBefore ? "черта выше" : "—")
                        + " | " + (e.enabled ? "доступен" : "НЕДОСТУПЕН"));
        if (menuItems.length === 0)
            failures.push("меню окна пусто");
        if (menuBarItem.builtItems !== menuItems.length)
            failures.push("пунктов меню построено " + menuBarItem.builtItems
                          + " из " + menuItems.length);
        if (menuBarItem.builtSeparators !== 1)
            failures.push("разделителей в меню построено "
                          + menuBarItem.builtSeparators + ", а нужен один");
        // The shortcut lives as an object separate from the item: if it were not
        // created, the menu would look intact while "Ctrl+Q" did not work at all.
        if (menuBarItem.builtShortcuts !== wantShortcuts)
            failures.push("горячих клавиш построено "
                          + menuBarItem.builtShortcuts + " из " + wantShortcuts);
        if (menuCore.buttonText === "")
            failures.push("кнопка меню без подписи — её не увидеть");

        // Warning: AN ITEM'S WIDTH IS A SEPARATE MEASURE, AND HERE IS WHY IT IS
        // HERE. The item count above is green even when all four are built but
        // read "🛟 Restore the…" instead of "🛟 Restore KDE theme". That is exactly
        // what slipped past the run on 18.09.2026: the items were in place, the
        // label was not. Now the menu is opened and the width is measured, and an
        // elided label fails the run.
        if (!menuBarItem.laidOut())
            failures.push("меню окна не разложилось — ширину подписей "
                          + "измерить нечем");
        console.log("REXON-MAINWINDOW-MENU-WIDTH " + menuBarItem.widthReport());
        const menuClipped = menuBarItem.clippedLabels();
        console.log("REXON-MAINWINDOW-MENU-CLIPPED "
                    + (menuClipped.length ? menuClipped.join(" | ") : "нет"));
        for (const c of menuClipped)
            failures.push("подпись пункта меню обрезана: " + c);
        // The menu must be in the window skin: a white Basic-style menu over the
        // dark window passed all the counts above (screenshot 30.09.2026).
        const menuOffSkin = menuBarItem.offSkinParts();
        console.log("REXON-MAINWINDOW-MENU-SKIN "
                    + (menuOffSkin.length ? menuOffSkin.join(" | ") : "в скине"));
        for (const c of menuOffSkin)
            failures.push("меню окна не в скине: " + c);
        // Next come the item windows, and an open menu would only get in their
        // way: the run checks the focus in the uninstall window by pressing keys.
        menuBarItem.closeAfterCheck();
        for (const e of menuItems) {
            if (e.label === "")
                failures.push("пункт меню «" + e.id + "» без подписи");
            if (!e.enabled)
                failures.push("пункт меню «" + e.id + "» недоступен");
        }

        // Dialog texts must belong to the core rather than be assembled by the
        // markup. The customer would take an empty text for a breakage (rule #13).
        const about = menuCore.aboutHtml();
        console.log("REXON-MAINWINDOW-MENU-ABOUT " + menuCore.aboutTitle()
                    + " | " + about.length + " знаков");
        if (menuCore.aboutTitle() === "" || about.length === 0)
            failures.push("справка «О программе» пуста");

        const recover = menuCore.recoverPrompt();
        console.log("REXON-MAINWINDOW-MENU-RECOVER " + recover.title
                    + " | тема: " + recover.target);
        if (!recover.title || !recover.text || !recover.target)
            failures.push("вопрос о сбросе темы KDE неполон");

        // The uninstall window is queried IN FULL but removes nothing: this is
        // reading the file system and querying the package manager. The removal
        // itself must not be tested here — it would wipe Rexon off the live
        // machine.
        const uninstall = menuCore.uninstallPrompt();
        const missingKeys = [];
        for (const key of ["title", "text", "informative", "files",
                           "keepLabel", "deleteLabel", "cancelLabel",
                           "defaultId"])
            if (uninstall[key] === undefined)
                missingKeys.push(key);
        console.log("REXON-MAINWINDOW-MENU-UNINSTALL "
                    + (uninstall.files ? uninstall.files.length : -1)
                    + " путей | по умолчанию: " + uninstall.defaultId
                    + " | пропущено: "
                    + (missingKeys.length ? missingKeys.join(",") : "—"));
        if (missingKeys.length)
            failures.push("окно удаления без ключей: " + missingKeys.join(","));
        if (uninstall.defaultId !== "cancel")
            failures.push("кнопка по умолчанию окна удаления не «Отмена»: "
                          + uninstall.defaultId);
        if (menuCore.busy())
            failures.push("удаление считает себя уже запущенным");

        // A menu item must lead SOMEWHERE, and this must be checked for EVERY one:
        // the model may be complete, the delegates built, and still the door
        // behind the item may fail to open — while the menu looks fine. "Quit" is
        // deliberately not touched here: it would close the window in the middle
        // of the run.
        //
        // Warning: the live system is not affected by this: the about box only
        // shows text, the theme reset question waits for "Yes", the uninstall
        // window for a button press. None of the three does anything by itself.
        for (const e of menuItems) {
            if (e.id === "quit")
                continue;
            menuDialogs.activate(e.id);
            console.log("REXON-MAINWINDOW-MENU-OPENED " + e.id + " → "
                        + (menuDialogs.openDialog === ""
                           ? "никуда" : menuDialogs.openDialog));
            if (menuDialogs.openDialog !== e.id)
                failures.push("пункт меню «" + e.id + "» никуда не привёл");
            // The uninstall window buttons are labelled when it opens. An empty label
            // here is a nameless button with the removal of the program from the
            // customer's machine behind it.
            if (e.id === "uninstall") {
                const labels = menuDialogs.uninstallButtonTexts();
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-BUTTONS "
                            + labels.join(" | "));
                for (const t of labels)
                    if (t === "")
                        failures.push("кнопка окна удаления без подписи");
                // The "keep" label names what will be kept; elided, it hides exactly that.
                const clipped = menuDialogs.uninstallClippedLabels();
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-CLIPPED "
                            + (clipped.length ? clipped.join(" | ") : "нет"));
                for (const c of clipped)
                    failures.push("подпись кнопки окна удаления обрезана: " + c);
                // Focus on "Cancel" is the same foolproofing as `setDefaultButton(Cancel)`
                // in the product. It cannot be checked by pressing: get it wrong, and the
                // program is removed for real.
                console.log("REXON-MAINWINDOW-MENU-UNINSTALL-FOCUS "
                            + (menuDialogs.uninstallDefaultHasFocus()
                               ? "на «Отмене»" : "НЕ на «Отмене»"));
                if (!menuDialogs.uninstallDefaultHasFocus())
                    failures.push("в открытом окне удаления фокус не на "
                                  + "«Отмене» — случайный Enter снёс бы Rexon");
            }
            menuDialogs.closeOpenDialog();
        }

        // ── Tutorial ─────────────────────────────────────────────────
        //
        // Read-only: the startup decision and the answers write the user's
        // settings, so they are not called here.
        const obTexts = onboardingCore.texts();
        const obFirst = onboardingCore.card("welcome");
        console.log("REXON-MAINWINDOW-ONBOARDING " + obTexts.askTitle
                    + " | " + (obFirst.title ?? "—"));
        if (!obTexts.askTitle || !obTexts.askYes || !obTexts.askNo)
            failures.push("вопрос обучения без текста");
        if (!obFirst.title || !obFirst.nextText)
            failures.push("первый шаг обучения без карточки");

        // ── Translation: QML and Rust speak the same language ──────
        //
        // Labels from Rust and labels in QML are translated by the same .mo
        // catalogue: QML asks it through the window's translator. The same string
        // must come out identical on both sides: if they differ, the QML
        // translator did not take hold, and the translated window would be left
        // with English QML labels.
        const qmlNext = qsTranslate("rexon", "Next");
        console.log("REXON-MAINWINDOW-L10N qml «" + qmlNext + "» | rust «"
                    + obFirst.nextText + "»");
        if (qmlNext !== obFirst.nextText)
            failures.push("QML и Rust перевели «Next» по-разному: «" + qmlNext
                          + "» против «" + obFirst.nextText + "»");

        for (const f of failures)
            console.log("REXON-MAINWINDOW-FAIL " + f);
        console.log("REXON-MAINWINDOW-DONE " + failures.length);
        Qt.exit(failures.length === 0 ? 0 : 1);
    }
}
