// The "Settings" tab in QML. Data and decisions come from the Rust object
// SettingsModel (rust/rexon-gui/src/settings_page.rs), which decides
// nothing itself but asks the core (`rexon_core::qt_settings`). Only
// MARKUP lives here.
//
// The styling is taken ready-made: the palette arrives through the
// `palette` property of the same object and originates from
// `ui_base::skin_colors("steam_dark")` — the default skin of the whole
// product (rule #17). The page defines no colours of its own.
//
// Side actions live outside: the page host puts into `root.actions` an
// object that can query the system (`facts`, `state`) and change it. Each
// of its methods returns a READY string for the pop-up message — composed
// by the same core through `rexon_py`, not in its own words. Long jobs
// (applying a wallpaper, installing yt-dlp, blurring, importing a theme,
// checking completeness) run in the background and deliver their answer
// through the `status`/`refreshed` signals: without them the button would
// stay silent until the job was done.
//
// The set of sections, their order, titles and descriptions come from the
// core (`qt_settings::SECTIONS` + `section_visible`) — the page has no
// chain of "whether to show the section" conditions of its own.
//
// The focus chain is built in from the start: all actions are
// QtQuick.Controls controls, the focused one shows a frame, and the page
// scrolls to it.

// Delegates refer to ids of outer elements (root, page).
// (A "// qmllint …" line must not be written here: it is read as a
// directive.)
pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import org.rexon.gui 1.0

Item {
    id: root

    /// The ← → arrows in the page's labels follow its mirroring.
    readonly property RexonArrows arrows: RexonArrows {
        mirrored: root.LayoutMirroring.enabled
    }

    // ── Bridge to the outside ─────────────────────────────────────────────

    /// Side-actions object. Our own Rust one by default
    /// (`rust/rexon-gui/src/settings_actions.rs`); the page host may substitute
    /// another — that is how the live Python product
    /// (`rexon/qt_settings/window.py`) keeps the tab until the port is
    /// finished.
    property var actions: ownActions

    /// A single section instead of the whole tab — for "Appearance"
    /// (01.11.2026): animation, fonts, title bar buttons and the rest that KDE
    /// 6.8 places under "Appearance & Style" are shown there, not here. The
    /// value is the key of a section in the "Appearance" side panel
    /// (`categories::SETTINGS_PANELS`) or comma-separated sections of the
    /// "⚙️ Settings" view of a catalogue section (`categories::SECTION_SETTINGS`:
    /// "GTK themes" can have two). Empty means the ordinary tab.
    property string panel: ""
    /// The environment selected in the "Appearance" tab (`panel` mode); empty
    /// means the one running now. Sections whose explanation depends on the
    /// desktop receive it.
    property string lookDe: ""

    /// Sections that moved: the content is the same as it was in the tab; they
    /// are shown by the `panel` mode.
    readonly property bool fullPage: root.panel === ""
    /// Sections of the `panel` mode as a list.
    readonly property var panels: root.panel === "" ? [] : root.panel.split(",")
    /// Whether section `key` is shown in `panel` mode.
    function showsPanel(key) {
        return root.panels.indexOf(key) >= 0;
    }
    /// Frames of the tab itself that moved to "Appearance" as the "⚙️ Settings"
    /// view of a catalogue section: `panel` mode key → frame (`page.sections`).
    /// They are absent from the tab, and in `panel` mode only they are present
    /// (fonts — "Fonts", wallpaper blur — "HD Wallpapers", background sounds
    /// and event sounds — "Sounds", 08.11.2026).
    readonly property var panelGroups: ({
        "fontsettings": "fontsGroup",
        "wallpaperblur": "blurGroup",
        "ambientsounds": "ambientGroup",
        "eventsounds": "soundThemeGroup"
    })
    /// Frame `id` moved to "Appearance".
    function movedGroup(id) {
        return Object.keys(root.panelGroups).some(k => root.panelGroups[k] === id);
    }
    /// Frame `id` is shown in this `panel` mode.
    function shownGroup(id) {
        return Object.keys(root.panelGroups).some(k => root.panelGroups[k] === id && root.showsPanel(k));
    }

    SettingsActions {
        id: ownActions
        // The name is needed by checks — the same approach as for the model below.
        objectName: "settingsActions"
    }

    /// Snapshot of the machine state (JSON from `actions.state()`), parsed.
    property var st: ({})

    /// The password has already been entered: Rexon may change the system until
    /// the window is closed (B13). Polled by a timer while the page is visible;
    /// an actions object without this method (the Python product) has no banner.
    property bool adminSession: false

    function reloadAdmin() {
        root.adminSession = !!(root.actions && root.actions.adminSession !== undefined
                               && root.actions.adminSession());
    }

    /// Label and explanation of the "Install Rexon into the system…" button
    /// (`{label, hint}` from the core). Empty means the page host substituted an
    /// actions object without the installation wizard (the Python product), and
    /// there will be no button.
    readonly property var installTexts: (root.actions
                                         && root.actions.installTexts !== undefined)
                                        ? root.actions.installTexts : ({})

    readonly property string windowTitle: page.windowTitle
    readonly property int windowMinWidth: page.windowMinWidth
    readonly property int windowMinHeight: page.windowMinHeight

    implicitWidth: page.windowMinWidth
    implicitHeight: page.windowMinHeight

    // ── Tab core ──────────────────────────────────────────────────────────

    SettingsModel {
        id: page
        // The name is needed by checks: from the page host the model is fetched
        // with `root.findChild(QObject, "settingsModel")`.
        objectName: "settingsModel"
    }

    // ── Standard skin palette ─────────────────────────────────────────────

    readonly property var pal: page.palette
    readonly property var txt: page.texts
    /// Layout and colours that are not in the palette: from the core, no
    /// numbers of our own.
    readonly property var sty: page.styles
    readonly property color colBg: root.pal.bg_primary
    readonly property color colBorder: root.pal.border
    readonly property color colAccent: root.pal.accent
    readonly property color colText: root.pal.text_primary
    readonly property color colTextSecondary: root.pal.text_secondary
    readonly property color colWarn: root.pal.state_warn
    readonly property color colBad: root.pal.state_bad

    // ── Seam with the side actions ────────────────────────────────────────

    /// Query the machine again and redraw everything that depends on it.
    function reload() {
        if (!root.actions)
            return;
        // The machine snapshot is needed only by the tab's own frames (and by the
        // frames that moved — fonts, blur); the other moved sections have their
        // own actions objects.
        if (!root.fullPage && !Object.keys(root.panelGroups).some(k => root.showsPanel(k)))
            return;
        root.st = JSON.parse(root.actions.state());
    }

    /// Run an action and show its answer. An empty answer means there is
    /// nothing to say (the action only saved a value).
    function say(message) {
        if (message)
            toast.show(message);
    }

    /// Re-read ONLY the list of playing background sounds.
    ///
    /// Separate from `reload` because it is called by a timer: the full state
    /// snapshot queries the system for fonts, sound themes and the list of
    /// installed wallpapers, and doing that every three seconds would keep the
    /// tab permanently busy for no reason.
    function reloadAmbient() {
        if (!root.actions)
            return;
        root.st = Object.assign({}, root.st,
                                { amb: JSON.parse(root.actions.ambientState()) });
    }

    /// The action changed something visible on the page: show the answer,
    /// re-read the state.
    function did(message) {
        root.say(message);
        root.reload();
    }

    Connections {
        target: root.actions

        // A long job has finished — its answer arrives here.
        function onStatus(message) {
            root.say(message);
        }

        // A long job changed the state of the machine.
        function onRefreshed() {
            root.reload();
        }
    }

    /// Scroll the page to the element that received focus: without this Tab
    /// takes focus past the bottom edge, and the tab looks frozen.
    function ensureVisible(item) {
        if (!item || !flick.contentItem)
            return;
        // The window became active again (the screenshot dialog from Print was
        // closed, the user returned from another program) — Qt gives focus back
        // to the same element. This is not a Tab move: the page must not scroll
        // to it, otherwise a tab scrolled down jumps to the top.
        if (item.focusReason === Qt.ActiveWindowFocusReason)
            return;
        const pos = item.mapToItem(flick.contentItem, 0, 0);
        const top = pos.y;
        const bottom = pos.y + item.height;
        const maxY = Math.max(0, flick.contentHeight - flick.height);
        if (top < flick.contentY)
            flick.contentY = Math.max(0, top - 12);
        else if (bottom > flick.contentY + flick.height)
            flick.contentY = Math.min(maxY, bottom - flick.height + 12);
    }

    Rectangle {
        anchors.fill: parent
        color: root.colBg
    }

    // ── Page ──────────────────────────────────────────────────────────────

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: pageColumn.implicitHeight + 2 * root.sty.page.margin
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {}

        // Warning: a layout inside an ORDINARY Item must get its height by hand:
        // by itself it stays at zero, and the page collapses into a strip.
        ColumnLayout {
            id: pageColumn
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.margins: root.sty.page.margin
            y: root.sty.page.margin
            height: implicitHeight
            spacing: root.sty.page.spacing

            RexonText {
                visible: root.fullPage
                text: page.pageTitle
                color: root.colText
                font.pointSize: page.pageTitlePointSize
                font.bold: true
            }

            // ── Warning banner: there is no polkit on the system ───────────
            // Without it EVERYTHING that needs administrator rights silently fails.
            // The cause is single and shared — we mention it once, at the top.
            Rectangle {
                Layout.fillWidth: true
                visible: (root.st.polkit || "") !== ""
                color: root.sty.banner.background
                border.color: root.sty.banner.border
                border.width: 1
                radius: root.sty.banner.radius
                implicitHeight: polkitText.implicitHeight
                                + 2 * root.sty.banner.padding

                RexonText {
                    id: polkitText
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: root.sty.banner.padding
                    y: root.sty.banner.padding
                    text: root.st.polkit || ""
                    wrapMode: Text.WordWrap
                    textFormat: Text.RichText
                    color: root.colText
                }
            }

            // ── "Rexon may change the system" banner (B13) ─────────────────
            // Visible while the password has been entered and the right is alive; the
            // button revokes it without closing the window. The same label and the
            // same action as in the tray icon menu.
            Rectangle {
                id: adminBanner
                objectName: "adminBanner"
                Layout.fillWidth: true
                visible: root.adminSession && root.fullPage
                color: root.sty.banner.background
                border.color: root.sty.banner.border
                border.width: 1
                radius: root.sty.banner.radius
                implicitHeight: adminRow.implicitHeight + 2 * root.sty.banner.padding

                RowLayout {
                    id: adminRow
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.margins: root.sty.banner.padding
                    y: root.sty.banner.padding
                    spacing: root.sty.banner.padding

                    RexonText {
                        Layout.fillWidth: true
                        text: "🔓 " + root.txt["admin.info"]
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }

                    RexonButton {
                        id: adminRevoke
                        objectName: "adminRevoke"
                        pal: root.pal
                        text: root.txt["admin.revoke"]
                        onClicked: {
                            root.say(root.actions.revokeAdmin());
                            root.reloadAdmin();
                        }
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(adminRevoke)
                    }
                }
            }

            // ── Sections ──────────────────────────────────────────────────
            Repeater {
                id: sectionRepeater
                // Fonts, wallpaper blur and sounds moved to "Appearance" (the "⚙️
                // Settings" view of the "Fonts", "HD Wallpapers" and "Sounds" sections):
                // their frames are absent from the tab, and in `panel` mode only they are
                // present (`panelGroups`).
                model: (page.sections || []).filter(
                           s => root.fullPage ? !root.movedGroup(s.id) : root.shownGroup(s.id))

                delegate: Rectangle {
                    id: sectionFrame
                    required property var modelData

                    Layout.fillWidth: true
                    color: root.sty.group.background
                    border.color: root.sty.group.border
                    border.width: 1
                    radius: root.sty.group.radius
                    // Section padding is counted FROM THE BORDER, not from the edge: in the
                    // original the border is drawn by a style sheet, and Qt places the layout
                    // margins (16 and 14) inside it. While the border thickness was not taken
                    // into account here, the content of every section came out one pixel
                    // further left and wider than the original (x 40 versus 41, width 1320
                    // versus 1318), and the section itself two pixels lower (measurement
                    // 05.09).
                    implicitHeight: sectionColumn.implicitHeight
                                    + 2 * (root.sty.group.marginV + border.width)

                    ColumnLayout {
                        id: sectionColumn
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: root.sty.group.marginH + sectionFrame.border.width
                        y: root.sty.group.marginV + sectionFrame.border.width
                        height: implicitHeight
                        spacing: root.sty.group.spacing

                        RexonText {
                            Layout.fillWidth: true
                            text: sectionFrame.modelData.title
                            color: root.colText
                            font.pointSize: page.sectionTitlePointSize
                            font.bold: true
                            wrapMode: Text.WordWrap
                        }

                        RexonText {
                            Layout.fillWidth: true
                            visible: sectionFrame.modelData.description !== ""
                            text: sectionFrame.modelData.description
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                            wrapMode: Text.WordWrap
                        }

                        Loader {
                            id: sectionBody
                            Layout.fillWidth: true
                            Layout.preferredHeight: item ? item.implicitHeight : 0
                            sourceComponent: root.sectionComponent(
                                                 sectionFrame.modelData.id)
                        }
                    }
                }
            }

            // ── Sections shown by "Appearance" (`panel` mode) ──
            // The same components that stood here before the move; they are built
            // only in their own section, so the tab does not query the machine for
            // nothing.
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("animations")
                visible: active
                sourceComponent: Component {
                    AnimationsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // "Screen effects" — only in "Appearance" (the tab never had them).
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("screeneffects")
                visible: active
                sourceComponent: Component {
                    ScreenEffectsSection {
                        pal: root.pal
                        sty: root.sty
                        de: root.lookDe
                        // Alone on the screen, the section fills it: "Apply" stays
                        // pinned at the top, the tiles scroll inside (as "MATE panel").
                        fillHeight: root.panel === "screeneffects"
                                    ? Math.max(360, flick.height - 2 * root.sty.page.margin)
                                    : 0
                        onMessage: text => root.say(text)
                    }
                }
            }

            // All KWin effects — only in KDE (the section hides itself elsewhere).
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("desktopeffects")
                visible: active
                sourceComponent: Component {
                    DesktopEffectsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("windowbuttons")
                visible: active
                sourceComponent: Component {
                    WindowButtonsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Launch feedback (as in the KDE "Cursors" module; outside KDE — an honest
            // label explaining why there is no setting).
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("launchfeedback")
                visible: active
                sourceComponent: Component {
                    LaunchFeedbackSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // The Rexon wallpaper playlist — first in the "⚙️ Settings" of "HD
            // Wallpapers".
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("wallpaperplaylist")
                visible: active
                sourceComponent: Component {
                    WallpaperPlaylistSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // The "Razer Axon" block in the "⚙️ Settings" of "HD Wallpapers" — only if
            // Axon is found (the section key is set by "Appearance").
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("axonsettings")
                visible: active
                sourceComponent: Component {
                    AxonSettingsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Icons in menus and on buttons, toolbar labels.
            Loader {
                Layout.fillWidth: true
                Layout.preferredHeight: item ? item.implicitHeight : 0
                active: root.showsPanel("appwidgets")
                visible: active
                sourceComponent: Component {
                    AppWidgetsSection {
                        pal: root.pal
                        sty: root.sty
                        onMessage: text => root.say(text)
                    }
                }
            }

            // Only in a Deepin session (the section hides itself elsewhere).
            DdeLookSection {
                id: ddeLook
                Layout.fillWidth: true
                visible: root.fullPage && ddeLook.st.available === true
                pal: root.pal
                sty: root.sty
            }
        }
    }

    /// Section content by its name from the core.
    ///
    /// An unknown name is not "an empty frame in silence": the core knows the
    /// section but the page does not, and that must be said out loud (the
    /// widget tab does the same).
    function sectionComponent(id) {
        switch (id) {
        case "langGroup": return cLanguage;
        case "skinGroup": return cSkin;
        case "obGroup": return cOnboarding;
        case "trayGroup": return cTray;
        case "linksGroup": return cLinks;
        case "fmGroup": return cFileManager;
        case "quickFix": return cQuickFix;
        case "completeness": return cCompleteness;
        case "wpGroup": return cWallpaper;
        case "libraryGroup": return cLibrary;
        case "importGroup": return cImport;
        case "blurGroup": return cBlur;
        case "ambientGroup": return cAmbient;
        case "soundThemeGroup": return cSound;
        case "fontsGroup": return cFonts;
        case "protGroup": return cProtection;
        case "panelGroup": return cPanel;
        case "aeroGroup": return cAero;
        }
        console.warn("вкладка «Настройки» не умеет строить раздел «" + id
                     + "»: ядро его знает, страница — нет");
        return null;
    }

    // ══ 🌐 Interface language ════════════════════════════════════════════
    //
    // The list comes from the core (`qt_settings::language_choices`:
    // po/LINGUAS and the catalogues found, labels in their own language). The
    // language changes after a restart: the snapshot says whether one is
    // needed (`st.lang.restart`), and then an explanation and a "Restart
    // Rexon" button appear under the list. The Python product's actions object
    // has no `setLanguage` — there the list only shows the choice.

    Component {
        id: cLanguage

        ColumnLayout {
            id: langBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var lang: root.st.lang || ({})
            readonly property bool canChange: root.actions
                                              && root.actions.setLanguage !== undefined

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["lang.row"]
                    textFormat: Text.RichText
                    color: root.colText
                }

                RexonComboBox {
                    id: langCombo
                    objectName: "languageCombo"
                    Layout.fillWidth: true
                    enabled: langBody.canChange
                    pal: root.pal
                    model: page.languages
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: langBody.lang.choice || "system"
                    onActivated: root.did(root.actions.setLanguage(langCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(langCombo)
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: langBody.lang.restart === true
                text: root.txt["lang.restart_hint"]
                wrapMode: Text.WordWrap
                color: root.colWarn
            }

            RexonButton {
                id: langRestart
                objectName: "languageRestart"
                visible: langBody.lang.restart === true && langBody.canChange
                pal: root.pal
                highlighted: true
                text: root.txt["lang.restart"]
                onClicked: root.say(root.actions.restartRexon())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(langRestart)
            }
        }
    }

    // ══ 🎨 Interface skin ═════════════════════════════════════════════════

    Component {
        id: cSkin

        ColumnLayout {
            id: skinBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["skin.row"]
                    textFormat: Text.RichText
                    color: root.colText
                }

                RexonComboBox {
                    id: skinCombo
                    Layout.fillWidth: true
                    pal: root.pal
                    model: page.skins
                    textRole: "label"
                    valueRole: "id"
                    // The value is set BEFORE subscribing to changes: the initial setting must
                    // not look like the user's choice (the skin was already applied when the
                    // application started).
                    selectedValue: root.st.skin || ""
                    onActivated: root.did(root.actions.setSkin(skinCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(skinCombo)
                }
            }

            RexonText {
                Layout.fillWidth: true
                text: page.skinDescription(root.st.skin || "")
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["skin.hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🎓 First-run tutorial ═════════════════════════════════════════════

    Component {
        id: cOnboarding

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: obCheck
                pal: root.pal
                text: root.txt["ob.check"]
                checked: root.st.onboarding === true
                onToggled: root.did(root.actions.setOnboarding(obCheck.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obCheck)
            }

            RexonButton {
                id: obReplay
                // A strip across the full card width, as in the widget tab: a button sized
                // to its content got lost under the check box and read as a minor
                // footnote, while it is the only way to bring the tutorial back.
                Layout.fillWidth: true
                pal: root.pal
                text: root.txt["ob.replay"]
                onClicked: root.did(root.actions.replayOnboarding())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obReplay)
            }

            // "Install Rexon into the system…" — Wehrwolfmann's decision 15.09.2026:
            // the button sits next to "Take the tutorial again". Whether to show it is
            // decided by the core (`installOffered`): for an installed Rexon — a
            // package, Steam, an installation for everyone or for yourself — there is
            // nothing to press it for, and there is no button at all. The label and
            // the explanation come from there too; there are no texts of our own here.
            RexonButton {
                id: obInstall
                Layout.fillWidth: true
                visible: root.actions && root.actions.installOffered === true
                pal: root.pal
                text: root.installTexts.label !== undefined
                      ? root.installTexts.label : ""
                onClicked: root.did(root.actions.installRexon())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(obInstall)
            }

            RexonText {
                Layout.fillWidth: true
                visible: obInstall.visible
                text: root.installTexts.hint !== undefined
                      ? root.installTexts.hint : ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🔗 Links to sites ══════════════════════════════════════════════════
    //
    // Sites for which the user chose "Don't ask again" in the question window
    // before an external link (`RexonLinkDialog`). The list and the reset come
    // from the core (`link_policy`) through the `LinkGuard` singleton.

    Component {
        id: cLinks

        ColumnLayout {
            id: linksBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            property string summary: LinkGuard.trustedSummary()

            RexonText {
                objectName: "trustedSites"
                Layout.fillWidth: true
                text: linksBody.summary
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonButton {
                id: linksReset
                objectName: "trustedSitesReset"
                pal: root.pal
                text: root.txt["links.reset"]
                onClicked: {
                    root.say(LinkGuard.forgetAll());
                    linksBody.summary = LinkGuard.trustedSummary();
                }
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(linksReset)
            }

            // The list may have grown in another tab while this one was hidden.
            onVisibleChanged: if (visible) linksBody.summary = LinkGuard.trustedSummary()
        }
    }

    // ══ 🔔 System tray icon ═══════════════════════════════════════════════

    Component {
        id: cTray

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: trayAutostart
                pal: root.pal
                text: root.txt["tray.autostart"]
                checked: root.st.tray !== undefined
                         && root.st.tray.autostart === true
                onToggled: root.did(root.actions.setAutostart(
                                        trayAutostart.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(trayAutostart)
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["tray.autostart_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // The session may have no tray at all — then there is no icon either, and
            // the window's close button quits Rexon as before. This must not be kept
            // quiet: the setting above will do nothing in such a session.
            RexonText {
                Layout.fillWidth: true
                visible: root.st.tray !== undefined
                         && root.st.tray.available === false
                text: root.txt["tray.missing"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // Fetching missing icons and cursor for the theme: while Rexon is running,
            // it watches the environment theme (`rexon_core::theme_watch`).
            RexonCheckBox {
                id: trayThemeDeps
                pal: root.pal
                text: root.txt["tray.theme_deps"]
                checked: root.st.tray === undefined
                         || root.st.tray.themeDepsAuto !== false
                onToggled: root.did(root.actions.setThemeDepsAuto(
                                        trayThemeDeps.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(trayThemeDeps)
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["tray.theme_deps_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 📁 File manager ═══════════════════════════════════════════════════

    Component {
        id: cFileManager

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            Repeater {
                model: page.fmChoices

                delegate: Rectangle {
                    id: fmRow
                    required property var modelData

                    readonly property bool isAuto: fmRow.modelData.id === "auto"
                    readonly property var badge: page.fmBadge(
                        (root.st.fm && root.st.fm.badges
                         && root.st.fm.badges[fmRow.modelData.id]) === true)

                    Layout.fillWidth: true
                    radius: root.sty.fmRow.radius
                    // Highlight of the focused row — the same "focus strip" as for the radio
                    // buttons of the widget tab; the colour comes from the core.
                    color: fmButton.activeFocus ? root.sty.fmRow.focus
                                                : "transparent"
                    implicitHeight: fmRowLayout.implicitHeight
                                    + 2 * root.sty.fmRow.marginV

                    RowLayout {
                        id: fmRowLayout
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.margins: root.sty.fmRow.marginH
                        y: root.sty.fmRow.marginV
                        height: implicitHeight
                        spacing: root.sty.fmRow.spacing

                        RadioButton {
                            id: fmButton
                            Layout.fillWidth: true
                            implicitHeight: 36
                            text: fmRow.modelData.label
                            checked: (root.st.fm ? root.st.fm.current : "")
                                     === fmRow.modelData.id
                            onClicked: root.did(
                                root.actions.setFileManager(fmRow.modelData.id))
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(fmRow)

                            indicator: Rectangle {
                                implicitWidth: 18
                                implicitHeight: 18
                                // The circle goes at the start of the line: on the right for right-to-left
                                // languages (as in `RexonCheckBox`).
                                x: fmButton.mirrored
                                   ? fmButton.width - width - fmButton.rightPadding
                                   : fmButton.leftPadding
                                y: fmButton.topPadding
                                   + (fmButton.availableHeight - height) / 2
                                radius: 9
                                color: "transparent"
                                border.color: fmButton.activeFocus
                                              || fmButton.hovered
                                              ? root.colAccent : root.colBorder
                                border.width: fmButton.activeFocus ? 2 : 1

                                Rectangle {
                                    anchors.centerIn: parent
                                    visible: fmButton.checked
                                    width: 9
                                    height: 9
                                    radius: 5
                                    color: root.colAccent
                                }
                            }

                            contentItem: RexonText {
                                leftPadding: fmButton.mirrored ? 0
                                             : fmButton.indicator.width + fmButton.spacing
                                rightPadding: fmButton.mirrored
                                              ? fmButton.indicator.width + fmButton.spacing : 0
                                text: fmButton.text
                                color: root.colText
                                verticalAlignment: Text.AlignVCenter
                            }
                        }

                        RexonText {
                            visible: !fmRow.isAuto
                            text: fmRow.badge.text
                            color: fmRow.badge.color
                        }

                        RexonText {
                            visible: fmRow.isAuto
                            // The arrow points at the choice on the left (`RexonArrows.keyHint`).
                            text: root.arrows.keyHint(root.txt["fm.auto_hint"])
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }
                }
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["fm.wm_note"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: fmApply
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["fm.apply"]
                    onClicked: root.did(root.actions.applyFileManager())
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(fmApply)
                }
            }

            RexonText {
                Layout.fillWidth: true
                text: page.t1("fm.current",
                              (root.st.fm && root.st.fm.currentName)
                              || root.txt["fm.unknown"])
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🔧 Quick application recovery ═════════════════════════════════════

    Component {
        id: cQuickFix

        ColumnLayout {
            id: qfBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var qf: root.st.quickfix || ({})

            // Warning: the environment name here comes from the snapshot (`qf.de`),
            // NOT from `page.deDisplay`. Section titles name the session ("KDE"),
            // while this section restores the applications of a particular desktop
            // and calls it by its full name from the desktop database ("KDE Plasma")
            // — just like the widget tab.

            // The environment is unknown to the core — there is nothing to restore,
            // and this is said plainly rather than with an empty button.
            RexonText {
                Layout.fillWidth: true
                visible: qfBody.qf.known !== true
                text: page.t1("quickfix.unknown_de", qfBody.qf.de || "")
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true
                visible: qfBody.qf.known === true
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    text: page.t1("quickfix.current", qfBody.qf.de || "")
                    textFormat: Text.RichText
                    wrapMode: Text.WordWrap
                    color: root.colText
                }

                RexonButton {
                    id: quickFixButton
                    pal: root.pal
                    tone: root.sty.primary
                    text: page.t1("quickfix.button", qfBody.qf.de || "")
                    onClicked: root.did(root.actions.restoreNative())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(quickFixButton)
                }
            }
        }
    }

    // ══ 🧩 Desktop completeness ═══════════════════════════════════════════

    Component {
        id: cCompleteness

        RowLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonText {
                Layout.fillWidth: true
                text: (root.st.compl && root.st.compl.text)
                      || root.txt["compl.idle"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonButton {
                id: complButton
                pal: root.pal
                tone: root.sty.primary
                enabled: !(root.st.compl && root.st.compl.busy)
                text: root.txt["compl.button"]
                onClicked: root.did(root.actions.checkCompleteness())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(complButton)
            }
        }
    }

    // ══ 🎬 Wallpapers ═════════════════════════════════════════════════════

    Component {
        id: cWallpaper

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            // The wallpaper engine did not come up — there is no section as such,
            // only an explanation why.
            RexonText {
                Layout.fillWidth: true
                visible: !(root.st.wp && root.st.wp.engine === true)
                text: root.txt["wp.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                id: wpBody
                Layout.fillWidth: true
                visible: (root.st.wp && root.st.wp.engine) === true
                spacing: root.sty.group.spacing

                readonly property var wp: root.st.wp || ({})
                readonly property var audio: wpBody.wp.audio || ({})

                /// Write all wallpaper sound and pause check boxes at once. The host
                /// watches the settings file by itself — wallpapers are not restarted.
                function saveAudio() {
                    root.did(root.actions.setWallpaperAudio(
                                 wpAudio.checked, Math.round(wpVolume.value),
                                 wpMuteOther.checked, wpHideCovered.checked,
                                 wpPauseBattery.checked, wpPauseGame.checked));
                }

                RexonText {
                    Layout.fillWidth: true
                    text: wpBody.wp.status || ""
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true

                    // The red button of an irreversible action is a shared type
                    // (`RexonDangerButton`), not a tinted ordinary one: the "dangerous action"
                    // colour is one for the whole product and arrives from the core with the
                    // palette (rule #12).
                    RexonDangerButton {
                        id: wpStop
                        pal: root.pal
                        tone: root.sty.danger
                        enabled: wpBody.wp.running === true
                        text: root.txt["wp.stop"]
                        onClicked: root.did(root.actions.stopWallpaper())
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(wpStop)
                    }

                    Item { Layout.fillWidth: true }
                }

                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["wp.from_file"]
                    color: root.colText
                }

                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["wp.file_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                // ── Wallpaper sound ───────────────────────────────────────
                // All four controls write to the same settings keys that the wallpaper
                // host watches: an edit is applied ON THE FLY, there is no need to
                // restart the wallpaper.
                RexonCheckBox {
                    id: wpAudio
                    pal: root.pal
                    text: root.txt["wp.video_audio"]
                    checked: wpBody.audio.enabled === true
                    onToggled: wpBody.saveAudio()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(wpAudio)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.volumeLabel
                        text: page.volumeLabel(wpVolume.value)
                        color: root.colText
                    }

                    RexonSlider {
                        id: wpVolume
                        Layout.maximumWidth: root.sty.widths.volumeSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: 0
                        to: 100
                        stepSize: 1
                        value: wpBody.audio.volume !== undefined
                               ? wpBody.audio.volume : 0
                        onMoved: wpBody.saveAudio()
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpVolume)
                    }

                    Item { Layout.fillWidth: true }
                }

                RexonCheckBox {
                    id: wpMuteOther
                    pal: root.pal
                    text: root.txt["wp.audio_mute_other"]
                    checked: wpBody.audio.muteOther === true
                    onToggled: wpBody.saveAudio()
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpMuteOther)
                }

                RexonCheckBox {
                    id: wpHideCovered
                    pal: root.pal
                    text: root.txt["wp.audio_mute_covered"]
                    checked: wpBody.audio.hideCovered === true
                    onToggled: wpBody.saveAudio()
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpHideCovered)
                }

                // Battery and game are the same conditions of the shared pause model as
                // "when they are not visible": they apply to video, web and Flash.
                RexonCheckBox {
                    id: wpPauseBattery
                    pal: root.pal
                    text: root.txt["wp.pause_battery"]
                    checked: wpBody.audio.pauseBattery === true
                    onToggled: wpBody.saveAudio()
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpPauseBattery)
                }

                RexonCheckBox {
                    id: wpPauseGame
                    pal: root.pal
                    text: root.txt["wp.pause_game"]
                    checked: wpBody.audio.pauseGame === true
                    onToggled: wpBody.saveAudio()
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(wpPauseGame)
                }

                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["wp.audio_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: wpPickImage
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["wp.pick_image"]
                        onClicked: root.did(root.actions.pickWallpaper("static"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpPickImage)
                    }

                    RexonButton {
                        id: wpPickVideo
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["wp.pick_video"]
                        onClicked: root.did(root.actions.pickWallpaper("video"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(wpPickVideo)
                    }

                    Item { Layout.fillWidth: true }
                }

                // ── YouTube / Vimeo ───────────────────────────────────────
                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["yt.header"]
                    color: root.colText
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.url
                        text: root.txt["yt.url_label"]
                        color: root.colText
                    }

                    RexonTextField {
                        id: ytUrl
                        Layout.fillWidth: true
                        pal: root.pal
                        placeholderText: root.txt["yt.url_hint"]
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytUrl)
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        text: root.txt["yt.quality_label"]
                        color: root.colText
                    }

                    RexonComboBox {
                        id: ytQuality
                        Layout.preferredWidth: 150
                        pal: root.pal
                        model: page.ytQualities
                        textRole: "label"
                        valueRole: "value"
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ytQuality)
                    }

                    RexonText {
                        text: root.txt["yt.mode_label"]
                        color: root.colText
                    }

                    RexonComboBox {
                        id: ytMode
                        Layout.preferredWidth: 150
                        pal: root.pal
                        model: page.ytModes
                        textRole: "label"
                        valueRole: "value"
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytMode)
                    }

                    Item { Layout.fillWidth: true }

                    RexonButton {
                        id: ytApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(wpBody.wp.busy === true)
                        text: wpBody.wp.busy === true ? root.txt["wp.working"]
                                                      : root.txt["yt.apply"]
                        onClicked: root.did(root.actions.applyYoutube(
                                                ytUrl.text, ytQuality.currentValue,
                                                ytMode.currentValue,
                                                ytAudio.checked))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytApply)
                    }
                }

                // A separate sound check box for YouTube is foolproofing: it does not
                // depend on the video wallpaper check box.
                RexonCheckBox {
                    id: ytAudio
                    pal: root.pal
                    text: root.txt["yt.audio"]
                    checked: wpBody.wp.ytAudio !== false
                    onToggled: root.did(root.actions.setYoutubeAudio(ytAudio.checked))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(ytAudio)
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        Layout.fillWidth: true
                        text: wpBody.wp.ytdlpLabel || ""
                        wrapMode: Text.WordWrap
                        color: root.colText
                    }

                    RexonButton {
                        id: ytdlpButton
                        pal: root.pal
                        tone: root.sty.primary
                        // While the installation is running, the button is disabled and says so:
                        // otherwise the user presses a second time and enters the password for a
                        // repeated installation.
                        enabled: !(wpBody.wp.ytdlpBusy === true)
                        text: wpBody.wp.ytdlpBusy === true
                              ? root.txt["wp.working"]
                              : (wpBody.wp.ytdlpButton || "")
                        onClicked: root.did(root.actions.ytdlp(
                                                wpBody.wp.ytdlpAction || ""))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ytdlpButton)
                    }
                }

                // ── Web wallpapers ────────────────────────────────────────
                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["web.header"]
                    color: root.colText
                }

                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["web.hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.url
                        text: root.txt["yt.url_label"]
                        color: root.colText
                    }

                    RexonTextField {
                        id: webUrl
                        Layout.fillWidth: true
                        pal: root.pal
                        placeholderText: root.txt["web.url_hint"]
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(webUrl)
                    }

                    RexonButton {
                        id: webApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(wpBody.wp.busy === true)
                        text: wpBody.wp.busy === true ? root.txt["wp.working"]
                                                      : root.txt["web.apply"]
                        onClicked: root.did(root.actions.applyWeb(webUrl.text))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(webApply)
                    }
                }
            }
        }
    }

    // ══ 📚 Wallpaper library ══════════════════════════════════════════════

    Component {
        id: cLibrary

        ColumnLayout {
            id: libBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var lib: root.st.lib || ({})
            readonly property var items: libBody.lib.items || []
            // Columns are computed by the core with the same grid engine as the widget
            // tabs — the page has no width steps of its own.
            readonly property int cols: Math.max(
                1, page.libraryColumns(libBody.width))
            readonly property int shown: Math.min(
                libBody.items.length, page.libraryPageSize(libBody.cols))

            RexonText {
                Layout.fillWidth: true
                visible: libBody.lib.engine !== true
                text: root.txt["lib.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonText {
                Layout.fillWidth: true
                visible: libBody.lib.engine === true
                         && libBody.items.length === 0
                // In the widgets the "empty" label sits with an 8 margin on all sides.
                Layout.topMargin: root.sty.lib.gridTopMargin
                padding: root.sty.lib.emptyPadding
                text: root.txt["lib.empty"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            GridLayout {
                Layout.fillWidth: true
                visible: libBody.items.length > 0
                Layout.topMargin: root.sty.lib.gridTopMargin
                columns: libBody.cols
                columnSpacing: root.sty.lib.gridSpacing
                rowSpacing: root.sty.lib.gridSpacing

                Repeater {
                    model: libBody.shown

                    delegate: Rectangle {
                        id: libCard
                        required property int index
                        readonly property var wp: libBody.items[libCard.index]

                        Layout.preferredWidth: page.libraryCardWidth
                        Layout.alignment: Qt.AlignTop | Qt.AlignLeft
                        implicitHeight: libCardColumn.implicitHeight
                        radius: root.sty.lib.cardRadius
                        color: "transparent"
                        border.width: 1
                        border.color: libHover.hovered ? root.sty.lib.cardHover
                                                       : root.sty.lib.cardBorder

                        HoverHandler { id: libHover }

                        ColumnLayout {
                            id: libCardColumn
                            width: parent.width
                            height: implicitHeight
                            spacing: 0

                            // Preview strip. Without a picture we draw the same placeholder as the
                            // widget tile.
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: page.libraryPreviewHeight
                                color: libPreview.status === Image.Ready
                                       ? "transparent"
                                       : root.sty.lib.placeholderBackground
                                radius: root.sty.lib.cardRadius

                                Image {
                                    id: libPreview
                                    anchors.fill: parent
                                    source: libCard.wp.preview || ""
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    cache: true
                                    clip: true
                                    visible: status === Image.Ready
                                }

                                RexonText {
                                    anchors.centerIn: parent
                                    visible: libPreview.status !== Image.Ready
                                    text: "🖼️"
                                    font.pointSize: root.sty.lib.placeholderPointSize
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RexonZoomArea {
                                    anchors.fill: parent
                                    pal: root.pal
                                    radius: root.sty.lib.cardRadius
                                    enabled: libPreview.status === Image.Ready
                                    visible: enabled
                                    onActivated: root.openLibraryViewer(libBody.items,
                                                                        libCard.index)
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                Layout.leftMargin: root.sty.lib.infoMarginH
                                Layout.rightMargin: root.sty.lib.infoMarginH
                                Layout.topMargin: root.sty.lib.infoMarginV
                                Layout.bottomMargin: root.sty.lib.infoMarginV
                                spacing: root.sty.lib.infoSpacing

                                RexonText {
                                    Layout.fillWidth: true
                                    text: libCard.wp.name || root.txt["lib.unnamed"]
                                    font.bold: true
                                    // The name is elided, the full one is in the tooltip: wrapping saves from
                                    // a long phrase, but not from one long word.
                                    elide: Text.ElideRight
                                    maximumLineCount: 2
                                    wrapMode: Text.WordWrap
                                    color: root.colText

                                    ToolTip.visible: nameHover.hovered
                                    ToolTip.text: libCard.wp.name
                                                  || root.txt["lib.unnamed"]
                                    HoverHandler { id: nameHover }
                                }

                                RexonText {
                                    Layout.fillWidth: true
                                    // The human-readable format label comes from the core (`formatLabel`);
                                    // the internal `format` is only for the button logic below.
                                    text: libCard.wp.formatLabel || libCard.wp.format || ""
                                    elide: Text.ElideRight
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: root.sty.lib.buttonSpacing

                                    RexonButton {
                                        id: libPreviewBtn
                                        Layout.preferredWidth: root.sty.widths.libPreviewButton
                                        leftPadding: 0
                                        visible: page.libraryShowsPreview(
                                                     libCard.wp.format || "")
                                        pal: root.pal
                                        text: root.txt["lib.preview"]
                                        ToolTip.visible: hovered
                                        ToolTip.text: root.txt["lib.preview_tip"]
                                        onClicked: root.say(
                                            root.actions.previewLibrary(
                                                libCard.wp.path || ""))
                                        onActiveFocusChanged: if (activeFocus)
                                            root.ensureVisible(libPreviewBtn)
                                    }

                                    RexonButton {
                                        id: libApply
                                        Layout.fillWidth: true
                                        pal: root.pal
                                        tone: root.sty.primary
                                        text: root.txt["lib.apply"]
                                        onClicked: root.did(
                                            root.actions.applyLibrary(
                                                libCard.wp.path || "",
                                                libCard.wp.format || ""))
                                        onActiveFocusChanged: if (activeFocus)
                                            root.ensureVisible(libApply)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ══ 🪟 Windows theme import ═══════════════════════════════════════════

    Component {
        id: cImport

        ColumnLayout {
            id: impBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var imp: root.st.imp || ({})

            RexonText {
                Layout.fillWidth: true
                text: root.txt["imp.formats"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RexonText {
                Layout.fillWidth: true
                visible: (impBody.imp.note || "") !== ""
                text: impBody.imp.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            RowLayout {
                Layout.fillWidth: true

                RexonButton {
                    id: impPick
                    pal: root.pal
                    tone: root.sty.primary
                    enabled: impBody.imp.available === true
                             && !(impBody.imp.busy === true)
                    text: impBody.imp.busy === true ? root.txt["imp.working"]
                                                    : root.txt["imp.pick"]
                    onClicked: root.did(root.actions.pickImport())
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(impPick)
                }

                Item { Layout.fillWidth: true }
            }

            RexonText {
                Layout.fillWidth: true
                visible: impBody.imp.available !== true
                text: root.txt["imp.unavailable_note"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }
        }
    }

    // ══ 🌫 Wallpaper blur ═════════════════════════════════════════════════

    Component {
        id: cBlur

        ColumnLayout {
            id: blurBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var blur: root.st.blur || ({})

            RexonText {
                Layout.fillWidth: true
                visible: blurBody.blur.engine !== true
                text: root.txt["wp.no_engine"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: blurBody.blur.engine === true
                spacing: root.sty.group.spacing

                RexonText {
                    Layout.fillWidth: true
                    visible: blurBody.blur.pillow !== true
                    text: root.txt["blur.no_pillow"]
                    wrapMode: Text.WordWrap
                    color: root.colWarn
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.blurLabel
                        text: root.txt["blur.strength"]
                        color: root.colText
                    }

                    RexonSlider {
                        id: blurSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: page.blurMin
                        to: page.blurMax
                        stepSize: page.blurStep
                        tickStep: page.blurStep
                        snapMode: Slider.SnapAlways
                        value: blurBody.blur.radius !== undefined
                               ? blurBody.blur.radius : page.blurDefault
                        onMoved: root.say(root.actions.setBlurRadius(Math.round(blurSlider.value)))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurSlider)
                    }

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.blurValue
                        text: page.t1("blur.px", String(Math.round(blurSlider.value)))
                        color: root.colText
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: blurApply
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: blurBody.blur.pillow === true
                                 && !(blurBody.blur.busy === true)
                        text: blurBody.blur.busy === true ? root.txt["wp.working"]
                                                          : root.txt["blur.apply"]
                        onClicked: root.did(root.actions.blur("blur"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurApply)
                    }

                    RexonButton {
                        id: blurRemove
                        pal: root.pal
                        tone: root.sty.primary
                        enabled: !(blurBody.blur.busy === true)
                        text: blurBody.blur.busy === true ? root.txt["wp.working"]
                                                          : root.txt["blur.remove"]
                        onClicked: root.did(root.actions.blur("unblur"))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(blurRemove)
                    }

                    Item { Layout.fillWidth: true }
                }
            }
        }
    }

    // ══ 🔊 Background sounds ══════════════════════════════════════════════

    Component {
        id: cAmbient

        ColumnLayout {
            id: ambBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var amb: root.st.amb || ({})
            readonly property var active: ambBody.amb.active || []

            RexonText {
                Layout.fillWidth: true
                visible: ambBody.amb.manager !== true
                text: root.txt["amb.no_manager"]
                wrapMode: Text.WordWrap
                color: root.colWarn
                font.bold: true
            }

            ColumnLayout {
                Layout.fillWidth: true
                visible: ambBody.amb.manager === true
                spacing: root.sty.group.spacing

                RexonText {
                    Layout.fillWidth: true
                    visible: ambBody.amb.mpv !== true
                    text: root.txt["amb.no_mpv"]
                    wrapMode: Text.WordWrap
                    color: root.colWarn
                    font.bold: true
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    RexonButton {
                        id: ambAdd
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["amb.add"]
                        ToolTip.visible: hovered
                        ToolTip.text: root.txt["amb.add_tip"]
                        onClicked: root.did(root.actions.addAmbient(Math.round(ambVolume.value)))
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(ambAdd)
                    }

                    RexonText {
                        text: root.txt["amb.volume"]
                        color: root.colText
                    }

                    RexonSlider {
                        id: ambVolume
                        Layout.maximumWidth: root.sty.widths.ambientSlider
                        Layout.fillWidth: true
                        pal: root.pal
                        from: 0
                        to: 100
                        stepSize: 1
                        value: page.ambientDefaultVolume
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ambVolume)
                    }

                    RexonText {
                        Layout.minimumWidth: root.sty.widths.ambientPercent
                        text: page.t1("amb.percent",
                                      String(Math.round(ambVolume.value)))
                        color: root.colText
                    }

                    Item { Layout.fillWidth: true }

                    RexonDangerButton {
                        id: ambStopAll
                        pal: root.pal
                        tone: root.sty.danger
                        text: root.txt["amb.stop_all"]
                        onClicked: root.did(root.actions.stopAllAmbient())
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(ambStopAll)
                    }
                }

                RexonText {
                    Layout.fillWidth: true
                    text: root.txt["amb.volume_hint"]
                    wrapMode: Text.WordWrap
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }

                // List of playing sounds: in the widgets this is a separate column with a
                // top margin of 4 and a row spacing of 4, not rows of the group.
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: root.sty.ambient.listTopMargin
                    spacing: root.sty.ambient.listSpacing

                    RexonText {
                        Layout.fillWidth: true
                        visible: ambBody.active.length === 0
                        text: root.txt["amb.empty"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }

                    Repeater {
                        model: ambBody.active

                        delegate: Rectangle {
                            id: ambRow
                            required property var modelData

                            Layout.fillWidth: true
                            radius: root.sty.ambient.rowRadius
                            color: root.sty.ambient.rowBackground
                            implicitHeight: ambRowLayout.implicitHeight
                                            + 2 * root.sty.ambient.rowMarginV

                            RowLayout {
                                id: ambRowLayout
                                anchors.left: parent.left
                                anchors.right: parent.right
                                anchors.margins: root.sty.ambient.rowMarginH
                                y: root.sty.ambient.rowMarginV
                                height: implicitHeight
                                spacing: root.sty.ambient.rowSpacing

                                RexonText {
                                    Layout.fillWidth: true
                                    text: ambRow.modelData.name || ""
                                    textFormat: Text.RichText
                                    elide: Text.ElideRight
                                    color: root.colText
                                }

                                RexonText {
                                    text: ambRow.modelData.volume || ""
                                    color: root.colTextSecondary
                                    font.weight: root.pal.text_secondary_weight
                                }

                                RexonButton {
                                    id: ambSetVol
                                    Layout.preferredWidth: root.sty.widths.ambientButton
                                    // A narrow icon button: the base button's side paddings do not fit into
                                    // its width.
                                    leftPadding: 0
                                    pal: root.pal
                                    text: root.txt["amb.setvol"]
                                    ToolTip.visible: hovered
                                    ToolTip.text: root.txt["amb.setvol_tip"]
                                    onClicked: root.did(root.actions.setAmbientVolume(
                                                            ambRow.modelData.id || "",
                                                            Math.round(ambVolume.value)))
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(ambSetVol)
                                }

                                RexonDangerButton {
                                    id: ambStop
                                    Layout.preferredWidth: root.sty.widths.ambientButton
                                    pal: root.pal
                                    tone: root.sty.ambientStop
                                    text: root.txt["amb.stop"]
                                    ToolTip.visible: hovered
                                    ToolTip.text: root.txt["amb.stop_tip"]
                                    onClicked: root.did(root.actions.stopAmbient(
                                                            ambRow.modelData.id || ""))
                                    onActiveFocusChanged: if (activeFocus)
                                        root.ensureVisible(ambStop)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ══ 🔔 Event sounds ═══════════════════════════════════════════════════

    Component {
        id: cSound

        ColumnLayout {
            id: sndBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var snd: root.st.snd || ({})
            readonly property var showcase: sndBody.snd.showcase || []

            // The theme that can REALLY be shown as selected: an empty string if the
            // system theme is not set at all or is named so that it is not among the
            // installed ones.
            readonly property string themeValue: {
                var themes = sndBody.snd.themes || [];
                var current = sndBody.snd.current || "";
                return themes.indexOf(current) >= 0 ? current : "";
            }

            // Rows of the theme list: label = theme name, value = the same. When there
            // is nothing to show, the first row is "— not set —" with an empty value.
            // Without it the list showed the FIRST installed theme, and the customer
            // read someone else's value as their own.
            readonly property var themeChoices: {
                var out = [];
                if (sndBody.themeValue === "") {
                    // There is a name but no theme — say exactly that: "not set" would be
                    // untrue here.
                    var named = sndBody.snd.current || "";
                    out.push({
                        "label": named !== ""
                                 ? (root.txt["snd.theme_missing"] || "").replace("%1", named)
                                 : root.txt["snd.theme_unset"],
                        "value": ""
                    });
                }
                var themes = sndBody.snd.themes || [];
                for (var i = 0; i < themes.length; ++i)
                    out.push({ "label": themes[i], "value": themes[i] });
                return out;
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.minimumWidth: root.sty.widths.fieldLabel
                    text: root.txt["snd.theme_label"]
                    color: root.colText
                }

                RexonComboBox {
                    id: sndTheme
                    // The name is needed by checks: the test bench fetches the list with
                    // `root.findChild(QObject, "soundThemeCombo")`.
                    objectName: "soundThemeCombo"
                    Layout.fillWidth: true
                    pal: root.pal
                    model: sndBody.themeChoices
                    textRole: "label"
                    valueRole: "value"
                    // The selection is set by VALUE, not by row number: the contract of the
                    // combo box itself (see the header of RexonComboBox.qml).
                    selectedValue: sndBody.themeValue
                    // An empty value is the "— not set —" row: it applies nothing, and the
                    // system theme stays as it is.
                    onActivated: if (sndTheme.currentValue)
                                     root.did(root.actions.setSoundTheme(
                                                  sndTheme.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(sndTheme)
                }
            }

            RexonCheckBox {
                id: sndEvents
                pal: root.pal
                text: root.txt["snd.events_check"]
                checked: sndBody.snd.events === true
                onToggled: root.did(root.actions.setEventSounds(sndEvents.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(sndEvents)
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["snd.test_label"]
                color: root.colText
            }

            RexonText {
                Layout.fillWidth: true
                visible: sndBody.showcase.length === 0
                text: root.txt["snd.no_events"]
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // The "listen" buttons wrap onto a second line instead of sliding past the
            // edge: RowLayout cannot wrap, and at the minimum window width the last
            // button would be cut.
            Flow {
                Layout.fillWidth: true
                spacing: root.sty.spacing.soundPreview

                Repeater {
                    model: sndBody.showcase

                    delegate: RexonButton {
                        id: sndButton
                        required property string modelData
                        readonly property var meta: page.eventMeta(sndButton.modelData)

                        pal: root.pal
                        text: sndButton.meta.emoji + " " + sndButton.meta.label
                        ToolTip.visible: hovered
                        ToolTip.text: page.t1("snd.event_tip", sndButton.modelData)
                        onClicked: root.say(root.actions.playSound(
                                                sndBody.snd.current || "",
                                                sndButton.modelData))
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(sndButton)
                    }
                }
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["snd.themes_hint"]
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }
        }
    }

    // ══ 🔤 System fonts ═══════════════════════════════════════════════════

    Component {
        id: cFonts

        ColumnLayout {
            id: fontBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var values: (root.st.font && root.st.font.values) || ({})
            readonly property var render: (root.st.font && root.st.font.render) || ({})
            readonly property var all: (root.st.font && root.st.font.all) || ({})
            readonly property var dpi: (root.st.font && root.st.font.dpi) || ({})
            // The "what to change" choice survives the snapshot refresh after an
            // action.
            property string allMode: "both"

            // Write the rendering as a whole: every change hands all values to the
            // core at once — that way no half of a setting remains in the file.
            function applyRender(aa, exclude, from, to, subpixel, hinting) {
                root.did(root.actions.setFontRendering(aa, exclude, from, to,
                                                       subpixel, hinting));
            }
            function applyCurrent() {
                applyRender(aaCheck.checked, aaExclude.checked,
                            parseInt(aaFrom.text) || fontBody.render.from,
                            parseInt(aaTo.text) || fontBody.render.to,
                            subpixelCombo.currentValue, hintingCombo.currentValue);
            }

            // Eight buttons in a row do not fit into a narrow window: they wrap.
            Flow {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                Repeater {
                    model: page.fontTargets

                    delegate: ColumnLayout {
                        id: fontCol
                        required property var modelData
                        // A column sized to its content, no wider than the button: the label under
                        // it wraps rather than stretching the column.
                        // With a margin: at exactly `implicitWidth` the button label lost its last
                        // letters ("System…", "Toolba…") — the emoji icon is wider than the layout
                        // thinks.
                        width: Math.ceil(fontButton.implicitWidth) + 18
                        spacing: root.sty.spacing.fontCol

                        RexonButton {
                            id: fontButton
                            Layout.fillWidth: true
                            pal: root.pal
                            tone: root.sty.primary
                            enabled: fontCol.modelData.available !== false
                            text: fontCol.modelData.label
                            ToolTip.visible: hovered
                            ToolTip.text: fontCol.modelData.tip
                            onClicked: root.did(root.actions.pickFont(
                                                    fontCol.modelData.id))
                            onActiveFocusChanged: if (activeFocus)
                                root.ensureVisible(fontButton)
                        }

                        RexonText {
                            // The column width is set by the button; the label under it wraps rather
                            // than stretching the column.
                            Layout.fillWidth: true
                            Layout.preferredWidth: 0
                            text: fontBody.values[fontCol.modelData.id]
                                  || root.txt["font.placeholder"]
                            wrapMode: Text.WordWrap
                            color: root.colTextSecondary
                            font.weight: root.pal.text_secondary_weight
                        }
                    }
                }
            }

            // LXQt desktop: restore the icon label font as it was before Rexon (the
            // button exists only if Rexon changed it).
            RexonButton {
                id: desktopRestoreButton
                readonly property var info: (root.st.font && root.st.font.desktop_restore) || ({})
                visible: desktopRestoreButton.info.visible === true
                pal: root.pal
                text: desktopRestoreButton.info.label || ""
                ToolTip.visible: hovered
                ToolTip.text: desktopRestoreButton.info.tip || ""
                onClicked: root.did(root.actions.restoreDesktopFont())
                onActiveFocusChanged: if (activeFocus)
                    root.ensureVisible(desktopRestoreButton)
            }

            RexonText {
                Layout.fillWidth: true
                text: page.t1("font.de_hint", page.deDisplay)
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // What "small", "toolbar" and "menu" affect where the environment has no
            // such fonts of its own.
            RexonText {
                Layout.fillWidth: true
                visible: text.length > 0
                text: (root.st.font && root.st.font.scope) || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Change all fonts ─────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonButton {
                    id: allFontsButton
                    pal: root.pal
                    tone: root.sty.primary
                    text: fontBody.all.label || ""
                    ToolTip.visible: hovered
                    ToolTip.text: fontBody.all.tip || ""
                    onClicked: root.did(root.actions.changeAllFonts(allModeCombo.currentValue))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(allFontsButton)
                }
                RexonText {
                    Layout.leftMargin: root.sty.group.spacing
                    text: root.txt["font.all_mode"]
                    color: root.colText
                }
                RexonComboBox {
                    id: allModeCombo
                    Layout.preferredWidth: 200
                    pal: root.pal
                    model: fontBody.all.modes || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.allMode
                    onActivated: fontBody.allMode = allModeCombo.currentValue
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(allModeCombo)
                }
                Item { Layout.fillWidth: true }
            }

            // ── Antialiasing, subpixels, hinting ─────────────────────────
            RexonText {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                text: root.txt["font.render_title"]
                textFormat: Text.StyledText
                wrapMode: Text.WordWrap
                color: root.colText
            }

            // Ready presets, as in MATE: antialiasing, subpixels and hinting with one
            // press. The selected one is the one the current rendering matches; a
            // custom setting highlights none.
            Flow {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonText {
                    height: presetRepeater.count > 0 ? implicitHeight : 0
                    text: root.txt["font.render_presets"]
                    color: root.colText
                }
                Repeater {
                    id: presetRepeater
                    model: fontBody.render.presets || []

                    delegate: RexonButton {
                        id: presetButton
                        required property var modelData
                        pal: root.pal
                        // Not `checkable`: a press must not break the binding; the highlight is
                        // set by the snapshot after writing.
                        checked: fontBody.render.preset === presetButton.modelData.id
                        checkedTone: root.sty.primary
                        text: presetButton.modelData.label
                        ToolTip.visible: hovered
                        ToolTip.text: presetButton.modelData.tip
                        onClicked: fontBody.applyRender(
                                       presetButton.modelData.antialias,
                                       aaExclude.checked,
                                       parseInt(aaFrom.text) || fontBody.render.from,
                                       parseInt(aaTo.text) || fontBody.render.to,
                                       presetButton.modelData.subpixel,
                                       presetButton.modelData.hinting)
                        onActiveFocusChanged: if (activeFocus) root.ensureVisible(presetButton)
                    }
                }
            }

            RexonCheckBox {
                id: aaCheck
                pal: root.pal
                text: root.txt["font.aa"]
                checked: fontBody.render.antialias === true
                onToggled: fontBody.applyCurrent()
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaCheck)
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonCheckBox {
                    id: aaExclude
                    pal: root.pal
                    enabled: aaCheck.checked
                    text: root.txt["font.aa_exclude"]
                    checked: fontBody.render.exclude === true
                    onToggled: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaExclude)
                }
                RexonTextField {
                    id: aaFrom
                    Layout.preferredWidth: 64
                    pal: root.pal
                    enabled: aaCheck.checked && aaExclude.checked
                    text: String(fontBody.render.from || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.render.min || 1
                        top: fontBody.render.max || 99
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.render.from)
                        fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaFrom)
                }
                RexonText {
                    text: root.txt["font.aa_to"]
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }
                RexonTextField {
                    id: aaTo
                    Layout.preferredWidth: 64
                    pal: root.pal
                    enabled: aaCheck.checked && aaExclude.checked
                    text: String(fontBody.render.to || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.render.min || 1
                        top: fontBody.render.max || 99
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.render.to)
                        fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(aaTo)
                }
                RexonText {
                    text: root.txt["font.pt"]
                    color: root.colTextSecondary
                    font.weight: root.pal.text_secondary_weight
                }
                Item { Layout.fillWidth: true }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: root.sty.spacing.fontRow

                RexonText {
                    text: root.txt["font.subpixel"]
                    color: root.colText
                }
                RexonComboBox {
                    id: subpixelCombo
                    Layout.preferredWidth: 200
                    pal: root.pal
                    enabled: aaCheck.checked
                    model: fontBody.render.subpixels || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.render.subpixel || ""
                    onActivated: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(subpixelCombo)
                }
                RexonText {
                    Layout.leftMargin: root.sty.group.spacing
                    text: root.txt["font.hinting"]
                    color: root.colText
                }
                RexonComboBox {
                    id: hintingCombo
                    Layout.preferredWidth: 160
                    pal: root.pal
                    model: fontBody.render.hintings || []
                    textRole: "label"
                    valueRole: "id"
                    selectedValue: fontBody.render.hinting || ""
                    onActivated: fontBody.applyCurrent()
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(hintingCombo)
                }
                Item { Layout.fillWidth: true }
            }

            RexonText {
                Layout.fillWidth: true
                text: root.txt["font.render_hint"]
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Forced font DPI ──────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                spacing: root.sty.spacing.fontRow

                RexonCheckBox {
                    id: dpiCheck
                    pal: root.pal
                    enabled: fontBody.dpi.available === true
                    text: root.txt["font.dpi"]
                    checked: fontBody.dpi.enabled === true
                    onToggled: root.did(root.actions.setFontDpi(
                                            dpiCheck.checked,
                                            parseInt(dpiField.text) || fontBody.dpi.value))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(dpiCheck)
                }
                RexonTextField {
                    id: dpiField
                    Layout.preferredWidth: 72
                    pal: root.pal
                    enabled: dpiCheck.enabled && dpiCheck.checked
                    text: String(fontBody.dpi.value || "")
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: fontBody.dpi.min || 48
                        top: fontBody.dpi.max || 480
                    }
                    onEditingFinished: if (parseInt(text) !== fontBody.dpi.value)
                        root.did(root.actions.setFontDpi(true, parseInt(text)))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(dpiField)
                }
                Item { Layout.fillWidth: true }
            }

            RexonText {
                Layout.fillWidth: true
                visible: text.length > 0
                text: fontBody.dpi.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            // ── Font management (its own component) ──────────────────────
            FontManagerSection {
                Layout.fillWidth: true
                Layout.topMargin: root.sty.group.spacing
                actions: root.actions
                pal: root.pal
                sty: root.sty
                onMessage: text => root.say(text)
                onFocused: item => root.ensureVisible(item)
            }
        }
    }

    // ══ 🔒 Default application protection ═════════════════════════════════

    Component {
        id: cProtection

        ColumnLayout {
            id: protBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var prot: root.st.prot || ({})

            RexonCheckBox {
                id: protMain
                pal: root.pal
                text: root.txt["prot.enable"]
                checked: protBody.prot.enabled === true
                onToggled: root.did(root.actions.setProtection("enabled",
                                                               protMain.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(protMain)
            }

            Repeater {
                model: page.protectionTypes

                delegate: RexonCheckBox {
                    id: protRow
                    required property var modelData
                    readonly property var meta: page.protectionRow(
                        protRow.modelData.id,
                        (protBody.prot.remembered
                         && protBody.prot.remembered[protRow.modelData.id]) || "")

                    Layout.fillWidth: true
                    pal: root.pal
                    text: protRow.meta.label
                    ToolTip.visible: hovered && protRow.meta.tip !== ""
                    ToolTip.text: protRow.meta.tip
                    checked: (protBody.prot.rules
                              && protBody.prot.rules[protRow.modelData.id]) === true
                    onToggled: root.did(root.actions.setProtection(
                                            protRow.modelData.id, protRow.checked))
                    onActiveFocusChanged: if (activeFocus) root.ensureVisible(protRow)
                }
            }

            // A theme change must not leave the terminal unreadable
            // (`terminal_guard`): its own switch, on by default.
            RexonCheckBox {
                id: protTermColors
                objectName: "terminalColorsGuard"
                Layout.fillWidth: true
                pal: root.pal
                text: root.txt["prot.terminal_colors"]
                ToolTip.visible: hovered
                ToolTip.text: root.txt["prot.terminal_colors_tip"]
                checked: protBody.prot.terminalColors !== false
                enabled: root.actions && root.actions.setTerminalColorsGuard !== undefined
                onToggled: root.did(root.actions.setTerminalColorsGuard(protTermColors.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(protTermColors)
            }

            // Without the xdg-terminal-exec utility the terminal choice is written,
            // but some programs will open their own — we say so plainly.
            RexonText {
                Layout.fillWidth: true
                visible: (protBody.prot.note || "") !== ""
                text: protBody.prot.note || ""
                wrapMode: Text.WordWrap
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
                ToolTip.visible: protNoteHover.hovered
                                 && (protBody.prot.noteTip || "") !== ""
                ToolTip.text: protBody.prot.noteTip || ""
                HoverHandler { id: protNoteHover }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.infoBox

                    RexonText {
                        Layout.fillWidth: true
                        text: root.txt["prot.snapshot_title"]
                        color: root.colText
                    }

                    RexonText {
                        Layout.fillWidth: true
                        text: root.txt["prot.snapshot_sub"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                    }
                }

                RexonButton {
                    id: protSnapshot
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["prot.snapshot"]
                    onClicked: root.did(root.actions.snapshot())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(protSnapshot)
                }
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                RexonText {
                    Layout.fillWidth: true
                    text: protBody.prot.status || ""
                    wrapMode: Text.WordWrap
                    color: root.colText
                }

                RexonButton {
                    id: protRestore
                    pal: root.pal
                    tone: root.sty.primary
                    enabled: protBody.prot.canRestore === true
                    text: root.txt["prot.restore"]
                    onClicked: root.did(root.actions.restoreDefaults())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(protRestore)
                }
            }
        }
    }

    // ══ 🛡 Panel protection ═════════════════════════════════════════════
    //
    // Every desktop Rexon knows (25.11.2026): the copies of the current
    // desktop on top, the copies of the other desktops in a folded block
    // below; a copy of another desktop can be restored from here too.

    Component {
        id: cPanel

        ColumnLayout {
            id: panelBody
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            readonly property var panel: root.st.panel || ({})
            readonly property var backups: panelBody.panel.backups || []
            readonly property var others: panelBody.panel.others || []
            property bool othersOpen: false

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.infoBox

                    RexonText {
                        Layout.fillWidth: true
                        text: root.txt["panel.backup_title"]
                        color: root.colText
                    }

                    RexonText {
                        Layout.fillWidth: true
                        text: panelBody.panel.supported === false
                              ? (panelBody.panel.unsupported || "")
                              : root.txt["panel.backup_sub"]
                        wrapMode: Text.WordWrap
                        color: root.colTextSecondary
                        font.weight: root.pal.text_secondary_weight
                        ToolTip.visible: panelSubHover.hovered && (panelBody.panel.tip || "") !== ""
                        ToolTip.text: panelBody.panel.tip || ""
                        HoverHandler { id: panelSubHover }
                    }
                }

                RexonButton {
                    id: panelBackup
                    visible: panelBody.panel.supported !== false
                    pal: root.pal
                    tone: root.sty.primary
                    text: root.txt["panel.backup"]
                    onClicked: root.did(root.actions.panelBackup())
                    onActiveFocusChanged: if (activeFocus)
                        root.ensureVisible(panelBackup)
                }
            }

            RexonText {
                Layout.fillWidth: true
                visible: panelBody.backups.length > 0
                text: panelBody.panel.header || ""
                color: root.colText
            }

            RexonText {
                Layout.fillWidth: true
                visible: panelBody.backups.length === 0 && panelBody.panel.supported !== false
                text: root.txt["panel.no_backups"]
                color: root.colTextSecondary
                font.weight: root.pal.text_secondary_weight
            }

            Repeater {
                model: panelBody.backups
                delegate: panelRowDelegate
            }

            // Copies of other desktops: folded until asked for.
            RexonButton {
                id: panelOthers
                visible: panelBody.others.length > 0
                pal: root.pal
                text: (panelBody.othersOpen ? "▾ " : root.arrows.collapsed + " ") + (panelBody.panel.othersHeader || "")
                onClicked: panelBody.othersOpen = !panelBody.othersOpen
                onActiveFocusChanged: if (activeFocus)
                    root.ensureVisible(panelOthers)
            }

            Repeater {
                model: panelBody.othersOpen ? panelBody.others : []

                delegate: ColumnLayout {
                    id: otherDesktop
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.panelRow

                    RexonText {
                        Layout.fillWidth: true
                        text: "<b>" + (otherDesktop.modelData.title || "") + "</b>"
                        color: root.colText
                    }

                    Repeater {
                        model: otherDesktop.modelData.backups || []
                        delegate: panelRowDelegate
                    }
                }
            }

            Component {
                id: panelRowDelegate

                RowLayout {
                    id: backupRow
                    required property var modelData
                    Layout.fillWidth: true
                    spacing: root.sty.spacing.panelRow

                    RexonText {
                        Layout.fillWidth: true
                        text: backupRow.modelData.label || ""
                        elide: Text.ElideRight
                        color: root.colText
                        ToolTip.visible: backupHover.hovered
                        ToolTip.text: backupRow.modelData.tip || ""
                        HoverHandler { id: backupHover }
                    }

                    RexonButton {
                        id: backupRestore
                        pal: root.pal
                        tone: root.sty.primary
                        text: root.txt["panel.restore"]
                        onClicked: root.askPanelRestore(backupRow.modelData.id || "")
                        onActiveFocusChanged: if (activeFocus)
                            root.ensureVisible(backupRestore)
                    }
                }
            }
        }
    }

    // ══ 🪟 Desktop look ═══════════════════════════════════════════════════

    Component {
        id: cAero

        ColumnLayout {
            width: parent ? parent.width : 0
            height: implicitHeight
            spacing: root.sty.group.spacing

            RexonCheckBox {
                id: aeroCheck
                Layout.fillWidth: true
                pal: root.pal
                text: root.txt["aero.check"]
                ToolTip.visible: hovered
                ToolTip.text: root.txt["aero.check_tip"]
                checked: (root.st.aero && root.st.aero.enabled) === true
                onToggled: root.did(root.actions.setAeroshell(aeroCheck.checked))
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aeroCheck)
            }

            RexonButton {
                id: aeroNow
                // The button stretches across the full width of the section: in the
                // original it is added to the group's vertical layout (`gl.addWidget`),
                // which stretches the widget horizontally. Without this QML drew it sized
                // to its content — 167 pixels instead of 1318 (measurement 05.09).
                Layout.fillWidth: true
                pal: root.pal
                tone: root.sty.primary
                text: root.txt["aero.check_now"]
                onClicked: root.did(root.actions.aeroshellCheckNow())
                onActiveFocusChanged: if (activeFocus) root.ensureVisible(aeroNow)
            }
        }
    }

    // ── Confirming the restore of a panel copy ────────────────────────────

    function askPanelRestore(id) {
        if (!root.actions || !id)
            return;
        const confirm = JSON.parse(root.actions.panelConfirm(id));
        confirmDialog.copyId = id;
        confirmDialog.heading = confirm.title;
        confirmDialog.question = confirm.question;
        confirmDialog.open();
    }

    Popup {
        id: confirmDialog

        property string copyId: ""
        property string heading: ""
        property string question: ""

        parent: root
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape
        anchors.centerIn: parent
        width: Math.min(root.width - 40, 520)
        padding: 16

        background: Rectangle {
            color: root.pal.bg_secondary
            border.color: root.colBorder
            border.width: 1
            radius: 8
        }

        // Dimming under a modal window is a product-wide decision
        // (`ui_base::MODAL_DIM_RGBA`, key `modal_dim`): the tutorial and "DE
        // comparison" dim the window with the same colour. The default style does
        // not dim the background, and the window was modal in fact but not in
        // looks.
        Overlay.modal: Rectangle {
            color: root.pal.modal_dim
        }

        contentItem: ColumnLayout {
            spacing: 10

            RexonText {
                Layout.fillWidth: true
                text: confirmDialog.heading
                color: root.colText
                font.bold: true
                font.pointSize: 14
                wrapMode: Text.WordWrap
            }

            RexonText {
                Layout.fillWidth: true
                text: confirmDialog.question
                textFormat: Text.RichText
                wrapMode: Text.WordWrap
                color: root.colText
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Item { Layout.fillWidth: true }

                RexonButton {
                    id: confirmYes
                    pal: root.pal
                    highlighted: true
                    text: root.txt["dialog.yes"]
                    onClicked: {
                        const id = confirmDialog.copyId;
                        confirmDialog.close();
                        root.did(root.actions.panelRestore(id));
                    }
                }

                RexonButton {
                    id: confirmNo
                    pal: root.pal
                    text: root.txt["dialog.no"]
                    onClicked: confirmDialog.close()
                }
            }
        }

        // Focus on opening is on the safe answer: an accidental Enter must not
        // restore a panel copy (foolproofing).
        onOpened: confirmNo.forceActiveFocus()
    }

    // ── Pop-up message ────────────────────────────────────────────────────
    //
    // The banner is shared across the whole product (`RexonToast.qml`): the
    // hide timer is there too — all the tab has to do is call `show`.

    // ── Large view of a wallpaper picture ─────────────────────────────────
    //
    // The layer is shared across the whole product (`RexonImageViewer.qml`).
    // Paging goes over the library wallpapers that have a picture.
    function openLibraryViewer(all, index) {
        const items = [];
        let at = 0;
        for (let i = 0; i < all.length; i++) {
            const wp = all[i];
            if (!wp.preview)
                continue;
            if (i === index)
                at = items.length;
            items.push({
                "key": wp.path || String(i),
                "title": wp.name || root.txt["lib.unnamed"],
                "subtitle": wp.formatLabel || "",
                "thumb": wp.preview,
                "source": wp.preview
            });
        }
        viewer.show(items, at);
    }

    RexonImageViewer {
        id: viewer
        objectName: "imageViewer"
        pal: root.pal
    }

    RexonToast {
        id: toast
        skin: root.sty.toast
        timeoutMs: page.toastTimeoutMs
        metrics: page
        availableWidth: root.width
    }

    // ── Re-reading the list of playing background sounds ──────────────────
    // The list changes without us too (a sound finished playing, it was
    // stopped from outside).

    Timer {
        interval: page.ambientRefreshMs
        // Background sounds live in "Appearance → Sounds → ⚙️ Settings"
        // (08.11.2026): polling happens while their frame is on screen. They play
        // without it too: the processes are held by the core executor for the
        // whole process (`wallpaper_exec::live_slot`), not by this page.
        running: root.visible && root.actions !== null && root.showsPanel("ambientsounds")
        repeat: true
        onTriggered: root.reloadAmbient()
    }

    // ── Polling the right to change the system (B13) ──────────────────────
    // The right expires without us (idle, screen lock, sleep), and appears
    // after a password in any tab — every two seconds, only while the page is
    // visible. The poll is cheap: a flag in the process memory.

    Timer {
        interval: 2000
        running: root.visible && root.actions !== null && root.fullPage
        repeat: true
        triggeredOnStart: true
        onTriggered: root.reloadAdmin()
    }

    // ── Window for installing what is missing ─────────────────────────────
    //
    // "Check and install missing": a question with the list of what is
    // missing → progress → result. The same window as in "Desktops"
    // (`DeOperationDialog.qml`) and the same core executor (`system_de_exec`).
    // The Python actions object has no `operation` property — then the window
    // is simply not shown.

    DeOperationDialog {
        anchors.fill: parent
        actions: root.actions
        pal: root.pal
    }

    // ── Focus from the first frame ────────────────────────────────────────

    // Warning: focus goes to the FIRST CONTROL, not to the page root:
    // `forceActiveFocus` on the root makes `activeFocusItem` a nameless Item,
    // and the page is dead both from the keyboard and from a gamepad.
    property int focusAttempts: 0

    function focusFirstControl() {
        // A section inside "Appearance" does not take focus for itself: it stays
        // in the side panel from which the user chose the section (gamepad and
        // Tab).
        if (!root.fullPage)
            return;
        if (sectionRepeater.count > 0) {
            const first = root.nextItemInFocusChain(true);
            if (first && first !== root) {
                root.focusAttempts = 0;
                focusRetry.stop();
                first.forceActiveFocus();
                return;
            }
        }
        // Sections do not fill instantly. We try again, but not forever: an empty
        // list of sections is a core breakage, and endless self-calls would only
        // hide it.
        if (root.focusAttempts < 40) {
            root.focusAttempts += 1;
            focusRetry.restart();
        } else {
            console.warn("вкладка «Настройки»: разделов нет, фокус взять не с чего");
        }
    }

    // Warning: a fix made during the port: the retry goes BY TIME, not through
    // `Qt.callLater`. `callLater` folds calls into the same event loop pass,
    // so all forty attempts burned out BEFORE the Repeater managed to build a
    // single section — and a page with sixteen sections honestly wrote "no
    // sections, nothing to take focus from" into the log and never took
    // focus. Under Python this went unnoticed: there the actions object was
    // substituted later, and the chain did not start at all.
    // 40 × 50 ms = 2 s for building — measured on this machine: 1 pass.
    Timer {
        id: focusRetry
        interval: 50
        onTriggered: root.focusFirstControl()
    }

    /// Query the machine and build the page from its answer.
    ///
    /// First the facts (the very SET of sections depends on them), then the
    /// state, then focus: before the facts there are no sections, and there is
    /// nothing to take focus from.
    function start() {
        if (!root.actions)
            return;
        page.configure(root.actions.facts());
        root.reload();
        root.focusFirstControl();
    }

    /// The actions object has appeared — only now can the machine be queried.
    ///
    /// Fires both for OUR OWN object (the page creates it itself) and for one
    /// substituted from outside: that is how the live Python product keeps the
    /// tab — it puts its own object into `actions` after the page is created.
    onActionsChanged: root.start()

    // In the embedded tab the page is created in advance and comes alive when
    // switched to: the state may have changed in other tabs.
    onVisibleChanged: if (root.visible) {
        root.reload();
        root.focusFirstControl();
    }

    // In "Appearance" one instance shows different sections: fonts need a
    // fresh snapshot when switched to.
    onPanelChanged: root.reload()
}
