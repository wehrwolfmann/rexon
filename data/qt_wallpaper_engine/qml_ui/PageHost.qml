// Rexon's universal shell around ANY vendored waywallen page
// (OUR file, not upstream). It plays the role of their Window.qml: MD theming, toasts and
// the navigation rail on the left. The page is set by the pageUrl property, the rail items by
// the railItems property (they are named by the core, `we_window::build_rail`).
pragma ComponentBehavior: Bound
import QtQuick
import QtQml
import QtQuick.Layouts
import Qcm.Material as MD
import waywallen.ui as W
import QExtra as QE

Item {
    id: root

    // file:// URL of the vendored page (e.g. .../page/StatusPage.qml)
    property url pageUrl

    // ── Navigation rail (as in waywallen) ─────────────────────────────
    //
    // The rail is the same as in the vendor's `ui/qml/Window.qml`, and is built from ITS own
    // components (`MD.NavigationRail`, `MD.RailItem`,
    // `W.SidebarNowPlaying`): the collapse button, the logo with the name,
    // pages with Material icons, at the bottom the current wallpaper card and the items
    // «Plugins», «Settings», «About». The layout is repeated rather than taken
    // from the vendor file because there is no way to take it from there: in the vendor the rail
    // is a nameless `sourceComponent` inside `MD.ApplicationWindow`, i.e.
    // part of the TOP-LEVEL window. Creating that inside our scene means
    // opening a second window next to Rexon, and their `qmldir` has no
    // separate rail type. The vendor file must not be edited (it is checked by
    // `sync-waywallen-qml.py --check`).
    //
    // The set of items is named by the core (`railItems`: `id`, `label`, …), and
    // the layout "which is a page, which is a pop-up window" is kept by this file — the same
    // way `Window.qml` keeps it: «Plugins», «Settings» and
    // «About» in waywallen open as a window on top
    // (`component/PagePopup.qml`), not as a page.
    //
    // Ours here is a single item — «Dev» (the build page for the developer):
    // it comes from the core only in developer mode; in the collapsed rail it
    // sits at the bottom, next to «Settings», in the expanded one — as an icon in the header
    // (otherwise the rail does not fit vertically). Labels of vendor items are taken from the
    // VENDOR's translation catalog with its own context (`Window`), the label of ours — from the
    // core.
    property var railItems: []
    // Index of the selected item in `railItems` — kept by the owner (the core), and
    // the rail only asks to change it.
    property int currentIndex: 0
    signal pageRequested(int index)

    // Pages may be built (the daemon answered) and shown (it is not the
    // "connecting…" screen right now). The rail depends on neither: it is always visible.
    property bool pagesReady: true
    property bool pageVisible: true
    // How much space at the top is taken by the owner's bars ("whose background" and so on).
    property real pageTopInset: 0
    // Rail width — the owner places its layers to the right of it.
    readonly property real railWidth: m_rail_loader.active ? m_rail_loader.width : 0
    // Height of the bottom navigation bar — the owner places its layers above it.
    readonly property real barHeight: m_bar_loader.active ? m_bar_loader.height : 0

    // A narrow tab (Material size class "compact", narrower than 600): the rail is
    // replaced by the bottom navigation bar — as in the vendor's `Window.qml`.
    readonly property bool isCompact: MD.MProp.size.isCompact

    function _railIndex(id) {
        for (let i = 0; i < root.railItems.length; ++i) {
            if (root.railItems[i].id === id)
                return i;
        }
        return -1;
    }

    // Rail pages: order and icons — as in the vendor's `pageModel`.
    readonly property var _railPages: {
        const known = [
            ["wallpapers_qml", MD.Token.icon.wallpaper, qsTranslate("Window", "Wallpapers")],
            ["discover_qml", MD.Token.icon.explore, qsTranslate("Window", "Discover")],
            ["displays_qml", MD.Token.icon.monitor, qsTranslate("Window", "Displays")],
            ["status_qml", MD.Token.icon.monitor_heart, qsTranslate("Window", "Status")]
        ];
        const out = [];
        for (const [id, icon, name] of known) {
            const at = root._railIndex(id);
            if (at >= 0)
                out.push({
                    icon: icon,
                    name: name,
                    page: at
                });
        }
        return out;
    }

    readonly property int _devIndex: root._railIndex("dev")
    readonly property int _wallpapersIndex: root._railIndex("wallpapers_qml")

    // Which rail item to highlight; -1 — a page not from the list is open.
    readonly property int _railCurrent: {
        for (let i = 0; i < root._railPages.length; ++i) {
            if (root._railPages[i].page === root.currentIndex)
                return i;
        }
        return -1;
    }

    // Port of the vendor's `Window.showAbout`.
    property var _aboutPresentation: null

    function showAbout() {
        if (root._aboutPresentation?.active)
            return;
        root._aboutPresentation = root.presentPopup('waywallen.ui/PagePopup', {
            source: 'waywallen.ui/AboutPage'
        });
    }

    // Port of the vendor's `Window.openWallpaper`: the current wallpaper card opens
    // the wallpaper on the «Wallpapers» page. The page loads asynchronously, so
    // the request waits until it is ready.
    property string _pendingWallpaperId: ""

    function openWallpaper(wallpaperId) {
        root._pendingWallpaperId = String(wallpaperId || "");
        if (root._pendingWallpaperId.length === 0)
            return;
        if (root.currentIndex !== root._wallpapersIndex)
            root.pageRequested(root._wallpapersIndex);
        Qt.callLater(root._deliverPendingWallpaper);
    }

    function _deliverPendingWallpaper() {
        if (root.currentIndex !== root._wallpapersIndex || root._pendingWallpaperId.length === 0)
            return;
        const page = m_content.currentItem;
        if (!page || typeof page.openWallpaper !== "function")
            return;
        const wallpaperId = root._pendingWallpaperId;
        root._pendingWallpaperId = "";
        page.openWallpaper(wallpaperId);
    }

    // ── Header of the vendor page (MD.AppBar) ──────────────────────────
    //
    // ⚠️ Without this piece a vendor page that in waywallen lives in a
    // POP-UP window opens here WITHOUT a title and WITHOUT ANY action
    // buttons. `MD.Page` draws the header according to `MD.MProp.page.showHeader`
    // (`Qcm.Material/qml/control/Page.qml`, line 25) — i.e. according to a
    // property of the OWNER, not its own. Our page had no owner
    // at all, and it got the global `PageContext` with the default
    // value `false` (`src/control/page_context.cpp`, line 71).
    //
    // waywallen has two owners, and they decide differently: the rail page
    // container (`ui/qml/Window.qml`) sets `showHeader: false`, while
    // `component/PagePopup.qml` sets `showHeader: true`. Rail pages either
    // have no header or turn it on themselves (`SettingsPage.qml`), and
    // pop-up window pages do not rely on their own `showHeader`.
    //
    // Our rail holds both kinds, so the owner turns the header on, and
    // the knowledge "which page belongs where" comes from the core as one answer for both
    // versions of the tab (`we_window::RailItem::header`). Caught by a live
    // snapshot on 08.09.2026: «Plugins» drew the plugin list, but there was
    // neither a title nor the buttons «Inactive plugins», «Check for
    // updates» and «Install from .zip» — i.e. there was no way to install or update
    // a plugin, while the snapshot looked working.
    //
    // The page background is as in the page container in the vendor's `Window.qml`:
    // a rounded card, and on a narrow tab — no background and no rounding.
    // `leadingAction` is empty on purpose: in waywallen it is the «back» button
    // that closes the pop-up window, and in the rail there is nothing to close —
    // `MD.AppBar` hides it itself (`m_leading.visible: action`).
    property bool pageHeader: false

    MD.MProp.page: m_page_context

    MD.PageContext {
        id: m_page_context
        showHeader: root.pageHeader
        backgroundRadius: root.isCompact ? 0 : MD.Token.shape.corner.large
        showBackground: !root.isCompact
    }

    // The light/dark tone of the interface is set by the Rexon SKIN (host.py passes
    // the brightness of the window palette), NOT by the desktop's system theme. Material
    // v0.2.5 by default pulls the mode from the XDG portal (useSysColorSM=true):
    // when the portal/styleHints reports Light (a light or undetermined
    // desktop) while Rexon has the dark steam_dark skin selected — all text
    // (dialog titles, chip labels, sidebar labels, monitor
    // names, dialog buttons) is computed from the light scheme and becomes
    // dark gray/almost black on the dark skin background. We bind the Material mode
    // to the skin so that text contrast is right with any desktop
    // theme. Dark by default — the default skin is steam_dark.
    property bool skinDark: true

    // Whose colors to draw the interface with (decided by the core,
    // `we_window::vendor_theme`).
    //
    // By default — with the Rexon skin, like the other tabs: the plugin COMPUTES the Material
    // scheme from a seed, and the skin's accent is set as the seed
    // (palette type "content" — its primary container is exactly
    // that color), while the shell background and text are taken from the skin as is. Backgrounds,
    // surfaces and text of the scheme (cards, dialogs, fields) on top of it
    // are set from the skin colors one by one — `skinRoles` in
    // `MD.Token.color.overrides`. With QmlMaterial lacking this property the step
    // is silently skipped, and cards keep the scheme tones derived from the accent.
    //
    // `followVendorTheme` — the «Dark (Material)» skin: the shell touches neither the palette
    // type, nor the accent, nor the theme, and waywallen's own settings apply
    // («Theme», «Accent» in its «Settings» window) — as in its window.
    property bool followVendorTheme: false
    property color skinAccent: "#66c0f4"
    property color skinBackground: "#1b2838"
    property color skinText: "#ffffff"
    // Scheme roles in skin colors: a JSON object "role → #rrggbb" from the core.
    property string skinRoles: "{}"
    readonly property var _skinRolesMap: {
        try {
            return JSON.parse(root.skinRoles);
        } catch (e) {
            return {};
        }
    }
    // Whether MdColorMgr has the `overrides` property (QmlMaterial with it).
    readonly property bool _hasColorOverrides: "overrides" in MD.Token.color

    function _applyRoles(roles) {
        if (root._hasColorOverrides)
            MD.Token.color.overrides = roles;
    }

    MD.MProp.size.width: width
    MD.MProp.backgroundColor: root.followVendorTheme
        ? MD.MProp.color.surface_container : root.skinBackground
    MD.MProp.textColor: root.followVendorTheme
        ? MD.MProp.color.getOn(MD.MProp.backgroundColor) : root.skinText

    // Force the Material mode = the Rexon skin and silence the tracking of the
    // system portal. The XDG portal (org.freedesktop.appearance) may
    // send the color-scheme answer ASYNCHRONOUSLY, already AFTER the first frame —
    // if at that moment the mode detaches from the skin, on_surface/textColor
    // are recomputed from the light scheme and the text of dialogs/sheets/sidebar
    // becomes dark gray on a dark background. So the mode is reapplied not
    // once but also with a short timer, covering the race window with the portal.
    function _applyMode() {
        if (root.followVendorTheme)
            return;
        MD.Token.color.useSysAccentColor = false;
        MD.Token.color.useSysColorSM = false;
        MD.Token.color.paletteType = MD.Enum.PaletteContent;
        MD.Token.color.accentColor = root.skinAccent;
        MD.Token.color.mode = root.skinDark ? MD.Enum.Dark : MD.Enum.Light;
        root._applyRoles(root._skinRolesMap);
    }

    // waywallen's own scheme: what its `Global` sets from its
    // settings. While `Global` is not born yet (the daemon has not answered) — its
    // defaults: system theme and accent, neutral palette
    // (the plugin's default type). Once born, `Global` applies the
    // saved choice itself. If it is already alive (the skin was changed with pages
    // open) — its choice is applied again: otherwise the Rexon theme and
    // accent would remain until the next launch.
    function _applyVendorTheme() {
        root._applyRoles({});
        MD.Token.color.paletteType = MD.Enum.PaletteNeutral;
        MD.Token.color.useSysColorSM = true;
        MD.Token.color.useSysAccentColor = true;
        if (root._vendorGlobalAlive) {
            W.Global.setThemeMode(W.Global.themeMode);
            W.Global.setAccentMode(W.Global.accentMode);
        }
    }

    // Whether the pages are alive, and with them the `W.Global` singleton. `W.Global` itself is
    // not asked here: asking would create it, earlier than in the original.
    readonly property bool _vendorGlobalAlive: root.pagesReady && root._shownPageUrl.toString().length > 0

    onFollowVendorThemeChanged: {
        if (root.followVendorTheme)
            root._applyVendorTheme();
        else
            root._applyMode();
    }
    onSkinAccentChanged: root._applyMode()
    on_SkinRolesMapChanged: root._applyMode()

    // ── Vendor pop-up pages (presentPopup) ────────────────────
    //
    // ⚠️ Without this piece ALL pop-up pages of the tab are DEAD: the
    // playlist sheet and its editor, the wallpaper and Workshop filter dialogs,
    // tag selection, «Libraries», the Workshop wallpaper card, the plugins
    // page. Vendor pages call `Window.window.presentPopup(...)`
    // — a method of their `Window.qml`, whose role this file plays. The window
    // had NO such method of ours AT ALL, and every such click silently failed with
    // "TypeError: Property 'presentPopup' ... is not a function".
    // The defect lived since vendor 0.3.5 and was found by a live run of 0.3.7.
    //
    // Why the function is attached to the WINDOW rather than declared here: `Window.window`
    // inside a QQuickWidget is its offscreen window (QQuickWidgetOffscreenWindow),
    // a foreign C++ type, and there is nothing to replace it with. But the QObject wrapper in the QML engine
    // is extensible: a property added to it from JS lives together with the wrapper.
    // The wrapper is kept alive by `hostWindow` — without a strong reference the engine's garbage
    // collector would throw it away together with the added method.
    //
    // ⚠️ THE WINDOW DOES NOT APPEAR RIGHT AWAY, and asking for it ONCE is not enough.
    // There used to be an assignment in `Component.onCompleted` — and in the WIDGET
    // version of the tab it NEVER fired: the shell lives inside a
    // QQuickWidget, which first creates the root object (that is when
    // `onCompleted` arrives, `Window.window` is still empty) and only THEN puts it into
    // its offscreen window. The check `if (!w) return;` quietly returned, and the
    // window ended up with neither `presentPopup` nor `showChangelog` — i.e.
    // in the widget version ALL vendor pop-up pages were dead, not
    // just one button. Found by a live probe on 05.09 by clicking «Changelog»:
    // "TypeError: Property showChangelog of object
    // QQuickWidgetOffscreenWindow ... is not a function".
    // So the window is not asked for but TRACKED by a binding: the methods
    // are installed as soon as the window appears, and reinstalled if it
    // changes.
    readonly property var hostWindow: root.Window.window
    onHostWindowChanged: {
        root._installWindowApi();
        root._applyTextureCache();
    }

    // waywallen 0.6.0: the thumbnail TEXTURE cache cap is set on the window — that is what
    // the vendor's `Window.qml` does (on creation and when the setting changes).
    // Our window is the owner of the tab, so the same is done here. With the QML stand-in
    // for QExtra the call does nothing.
    function _applyTextureCache() {
        if (root.hostWindow)
            QE.ImageCache.setTextureCapacityMiB(root.hostWindow, W.Global.imageTextureCacheMiB);
    }
    Connections {
        target: W.Global
        function onImageTextureCacheMiBChanged() {
            root._applyTextureCache();
        }
    }

    readonly property alias popupPresenter: m_popup_presenter

    // One-to-one port of the vendor's `Window.presentPopup`: a failure to show goes out
    // as a toast instead of sinking.
    function presentPopup(source, properties) {
        const presentation = m_popup_presenter.present(source, properties || {});
        presentation.failed.connect(presentation, function (error) {
            W.Global.toastError(error);
        });
        if (presentation.status === MD.PopupPresentation.Error)
            W.Global.toastError(presentation.errorString);
        return presentation;
    }

    function _installWindowApi() {
        const w = root.hostWindow;
        if (!w)
            return;
        w.popupPresenter = m_popup_presenter;
        w.presentPopup = root.presentPopup;
        w.showChangelog = root.showChangelog;
    }

    // ── Engine changelog (showChangelog) ───────────────
    //
    // The second method of their window. The «Changelog» button of the
    // «About engine» page (`page/AboutPage.qml`) calls
    // `Window.window.showChangelog()`, and the click failed with "TypeError:
    // Property showChangelog ... is not a function" — EQUALLY in both
    // versions of the tab, i.e. this is not a porting divergence but an embedding
    // failure (section acceptance on 05.09).
    //
    // ⚠️ The class is the same as `presentPopup` above, so the list is
    // closed AS A WHOLE, not one by one (rule #12). Grepping
    // `Window.window` across the vendor tree gives exactly four
    // things pages ask of the owner window:
    //   * `presentPopup(source, properties)` — 15 calls, present;
    //   * `popupPresenter` — the same thing is passed to the filter dialogs
    //     as the `popupWindow` property (`DiscoverPage`, `WallpaperPage`),
    //     and inside they call the same `presentPopup` — present;
    //   * `showChangelog()` — installed here;
    //   * `height` (`Window.window?.height` in `WallpaperDetailPanel`) —
    //     a native window property, nothing to install.
    // There is not a single other access to the window in the vendor tree.
    //
    // The release list source is the same address as the vendor's:
    // `qrc:/waywallen/ui/assets/waywallen-ui.releases.xml`. Upstream it is
    // in the resources of its binary, here — in the rexon-gui resources
    // (`rust/rexon-gui/waywallen_assets.qrc`, the file is vendored, MIT, the same
    // tag — see `waywallen/ATTRIBUTION.md`). The address does not depend on
    // where on disk their markup lies.
    //
    // The window title is taken from the VENDOR's translation catalog with the same
    // context their `Window.qml` translates it with
    // (`qsTranslate("Window", …)`): our own `qsTr()` would get the context
    // "PageHost", which is not in their `.ts`, and the title would stay
    // in English.
    property var _changelogPresentation: null

    function showChangelog() {
        if (root._changelogPresentation?.active)
            return;
        root._changelogPresentation = root.presentPopup(m_changelog_dialog, {
            source: "qrc:/waywallen/ui/assets/waywallen-ui.releases.xml",
            title: qsTranslate("Window", "Changelog")
        });
    }

    Component {
        id: m_changelog_dialog

        MD.ChangelogDialog {}
    }

    // The same as their window's `Component.onDestruction`: an unclosed window
    // would outlive its shell when the tab changes.
    Component.onDestruction: {
        root._changelogPresentation?.cancel();
        root._aboutPresentation?.cancel();
    }

    MD.PopupPresenter {
        id: m_popup_presenter
        // In the vendor the owner is `win.contentItem`; here the window is foreign, so
        // the owner is the shell itself: it is stretched over the whole tab anyway.
        host: root
    }

    // ── Overlay window layer — only above the tab ────────────────────────
    //
    // Material puts windows on top, their dimming, menus and tooltips into ONE
    // layer for the whole window (`Qcm.Material.Overlay`: a child of the window's `contentItem`,
    // the size of the window). In waywallen the window is the whole interface, while here in the same
    // window Rexon's sections stand above the tab — and the dimming of «Plugins»,
    // «Settings» and «About» covered them too, and a window on top on a narrow
    // tab expanded over the whole Rexon window.
    //
    // So the layer is shrunk to the shell: that way everything living in it treats
    // the wallpaper tab as the "window" — the way the original's window covers only its own
    // content. The library resets the layer size itself on every window
    // resize, so it is not bound but refitted on
    // every change.
    //
    // The layer also clips its content to itself (`clip`). It is not about the picture:
    // under a modal window Material puts an invisible hover blocker into the layer,
    // and it answers "is the pointer inside me" for the whole window, not
    // for its own size — Rexon's sections above the tab stopped
    // highlighting under the mouse. A clipped layer does not hand out hover events
    // outside itself to its children, and the sections highlight as usual.
    //
    // Clicks outside the tab while a window on top is open are a separate half: they
    // are passed to the window by the owner (`WallpaperEnginePage.qml`).
    readonly property Item popupLayer: MD.Overlay.overlay

    function _fitPopupLayer() {
        const layer = root.popupLayer;
        if (!layer || !layer.parent || root.width <= 0 || root.height <= 0)
            return;
        if (!layer.clip)
            layer.clip = true;
        const at = root.mapToItem(layer.parent, 0, 0);
        if (layer.x !== at.x)
            layer.x = at.x;
        if (layer.y !== at.y)
            layer.y = at.y;
        if (layer.width !== root.width)
            layer.width = root.width;
        if (layer.height !== root.height)
            layer.height = root.height;
    }

    onPopupLayerChanged: root._fitPopupLayer()
    onWidthChanged: root._fitPopupLayer()
    onHeightChanged: root._fitPopupLayer()
    onVisibleChanged: root._fitPopupLayer()

    Connections {
        target: root.popupLayer
        function onWidthChanged() {
            root._fitPopupLayer();
        }
        function onHeightChanged() {
            root._fitPopupLayer();
        }
    }

    // The shell is also moved by its ancestors (the Rexon section bar changes height together with
    // the window width): the position in the window is rechecked on window resizes too.
    Connections {
        target: root.hostWindow
        ignoreUnknownSignals: true
        function onWidthChanged() {
            Qt.callLater(root._fitPopupLayer);
        }
        function onHeightChanged() {
            Qt.callLater(root._fitPopupLayer);
        }
    }

    Component.onCompleted: {
        if (root.followVendorTheme)
            root._applyVendorTheme();
        else
            root._applyMode();
        root._installWindowApi();
        root._switchPage();
        root._fitPopupLayer();
    }

    onSkinDarkChanged: root._applyMode()

    // ⚠️ The timer above is not enough. The vendor singleton `W.Global` on birth
    // sets the mode itself from its «Theme» setting (`setThemeMode` →
    // `useSysColorSM = true` for «System»), and it is born when a page
    // first asks for it — i.e. when the daemon has answered. Daemon 0.4.4
    // runs database migrations for a minute on first start, by that
    // time the timer has long gone quiet, and the whole tab stayed LIGHT on the dark
    // Rexon skin (bench snapshot 09.10.2026). So the mode returns to the
    // skin on every foreign switch, not only in the first two seconds.
    Connections {
        target: MD.Token.color
        function onUseSysColorSMChanged() {
            if (MD.Token.color.useSysColorSM)
                root._applyMode();
        }
        function onModeChanged() {
            if (MD.Token.color.mode !== (root.skinDark ? MD.Enum.Dark : MD.Enum.Light))
                root._applyMode();
        }
        // The same `Global` on birth also sets the accent from its
        // «Accent» setting (system or custom) — on the Rexon skin the accent stays
        // the skin accent.
        function onUseSysAccentColorChanged() {
            if (MD.Token.color.useSysAccentColor)
                root._applyMode();
        }
        function onAccentColorChanged() {
            if (!Qt.colorEqual(MD.Token.color.accentColor, root.skinAccent))
                root._applyMode();
        }
    }

    // Catch up with a late asynchronous portal answer (several ticks in a row).
    Timer {
        running: true
        repeat: true
        interval: 250
        triggeredOnStart: true
        property int _ticks: 0
        onTriggered: {
            root._applyMode();
            if (++_ticks >= 8)
                running = false;   // ~2 s is enough, after that the portal is silent
        }
    }

    Rectangle {
        anchors.fill: parent
        color: root.MD.MProp.backgroundColor
    }

    // ── Pages and their cache ─────────────────────────────────────────────
    //
    // Pages are held by `MD.PageContainer` — the same container as in the
    // vendor's `Window.qml`, and it is called the same way: `switchTo(url, {}, cache)`.
    // «Wallpapers» and «Discover» live in the container cache (the vendor's `pageCacheable`):
    // going to another page does not destroy them, and on return the
    // scroll position, the selected wallpaper, the typed search and the Workshop results are in place.
    // «Monitors» and «Status» are rebuilt on every visit — also as
    // in the vendor.
    //
    // Pages are loaded by the container pool (`MD.Pool`, QQmlIncubator) asynchronously:
    // synchronous loading would stop the main thread until the page
    // finished compiling.
    readonly property var _cacheablePages: ["wallpapers_qml", "discover_qml"]
    // URL of the page the container already shows (or is loading).
    property string _shownPageUrl: ""

    onPagesReadyChanged: Qt.callLater(root._switchPage)

    // Called deferred: the owner sends the page URL and the item index
    // in two bindings, and both must have arrived by the time of the call.
    function _switchPage() {
        const url = String(root.pageUrl);
        // Empty URL — an item without a vendor page is open («Dev»):
        // the previous page stays in the container, the owner just hides it.
        if (!root.pagesReady || url.length === 0 || url === root._shownPageUrl)
            return;
        const item = root.railItems[root.currentIndex];
        root._shownPageUrl = url;
        m_content.switchTo(url, {}, !!item && root._cacheablePages.indexOf(item.id) >= 0);
    }
    Item {
        id: m_page_area
        anchors.left: parent.left
        anchors.leftMargin: root.railWidth
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: m_bar_loader.top
        anchors.topMargin: root.pageTopInset
        visible: root.pageVisible
        clip: true

        MD.PageContainer {
            id: m_content
            anchors.fill: parent
            initialItem: Item {}

            onCurrentItemChanged: Qt.callLater(root._deliverPendingWallpaper)
            onPageLoadFailed: error => {
                // A failure must not lock the page: visiting the item
                // again must try once more.
                root._shownPageUrl = "";
                W.Global.toastError(error);
            }
        }
    }

    // ── The rail itself ─────────────────────────────────────────────────────
    //
    // The layout is the vendor's `Window.qml`, block "Navigation rail", line for
    // line; differences are marked with the word "REXON".
    Loader {
        id: m_rail_loader
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        active: root.railItems.length > 0 && !root.isCompact
        visible: active

        sourceComponent: MD.NavigationRail {
            id: m_rail
            model: root._railPages
            // REXON: in the vendor `win.currentPage`; here the selected item may also be
            // one not from the rail list («Dev»), so the highlight is driven by
            // the owner. Since QmlMaterial 0.5.0 the rail does not write `currentIndex` itself,
            // it only asks (`currentIndexChangeRequested`), and a plain
            // binding is enough.
            currentIndex: root._railCurrent

            autoExpand: W.Global.sidebarAutoExpand
            // The rail only re-syncs `expanded` on window-class
            // changes; apply a runtime toggle of the setting at once.
            onAutoExpandChanged: if (autoExpand)
                expanded = useEmbed

            onCurrentIndexChangeRequested: function (index) {
                root.pageRequested(root._railPages[index].page);
            }

            // Logo + a menu-toggle button (the rail's default header
            // is just the toggle; we add branding alongside it).
            header: Item {
                implicitWidth: MD.Util.lerp(m_rail.collapsedWidth, m_rail.expandedWidth, m_rail.expansionProgress)
                implicitHeight: m_logo.y + m_logo.height + 12

                MD.StandardIconButton {
                    id: m_menu_btn
                    x: MD.Util.lerp((m_rail.collapsedWidth - width) / 2, 32 - (width - 24) / 2, m_rail.expansionProgress)
                    y: 4
                    icon.name: m_rail.useLarge ? MD.Token.icon.menu_open : MD.Token.icon.menu
                    onClicked: m_rail.toggle()
                }

                // REXON: our only item — the build page for the
                // developer. In the expanded rail it sits here, as an icon
                // in the free corner of the header, not as an item at the bottom: with the extra
                // item the expanded rail did not fit at the minimum Rexon window height,
                // and «About» was cut off at the bottom. This way
                // the expanded rail matches the vendor's item for item. In the
                // collapsed rail there is no «About» and there is enough room — there
                // the item stays at the bottom (see the footer). The button is a toggle
                // (`IconToggleButton`, as everything the vendor highlights as
                // selected): `StandardIconButton` with QmlMaterial 0.5.0 has no
                // `checked` property.
                MD.IconToggleButton {
                    id: m_dev_btn
                    visible: root._devIndex >= 0 && opacity > 0
                    opacity: m_rail.expansionProgress
                    enabled: m_rail.expansionProgress === 1
                    x: m_rail.expandedWidth - width - (32 - (width - 24) / 2)
                    y: m_menu_btn.y
                    icon.name: MD.Token.icon.build
                    checked: root._devIndex >= 0 && root.currentIndex === root._devIndex
                    onCheckedChangeRequested: {
                        root.pageRequested(root._devIndex);
                        if (m_rail.useModal)
                            m_rail.close();
                    }

                    MD.ToolTip.visible: mdState.interactions.hovered
                    MD.ToolTip.delay: 300
                    MD.ToolTip.text: root._devIndex >= 0 ? root.railItems[root._devIndex].label : ""
                }

                Image {
                    id: m_logo
                    width: 32
                    height: 32
                    x: MD.Util.lerp((m_rail.collapsedWidth - width) / 2, 32, m_rail.expansionProgress)
                    y: m_menu_btn.y + m_menu_btn.height + 16
                    // The vendor's address; here the file is in the rexon-gui resources
                    // (`rust/rexon-gui/waywallen_assets.qrc`).
                    source: "qrc:/waywallen/ui/assets/waywallen-ui.svg"
                    fillMode: Image.PreserveAspectFit
                    sourceSize.width: 64
                    sourceSize.height: 64

                    // REXON: there is no "update available" badge on the logo
                    // on purpose. In the vendor it is lit by `W.UpdateChecker`
                    // (a new waywallen release on GitHub), and clicking leads to
                    // «About» to the «Version N available» button. Here the engine
                    // ships inside the Rexon package and is updated together with it:
                    // there is no release check (`W.UpdateChecker.supported` is
                    // false, and the vendor hides the buttons on «About» itself), and
                    // the badge would call for an update the package will not
                    // see. The release check for the developer lives on the
                    // «Dev» page and shows the result there as well.
                }

                MD.Label {
                    visible: opacity > 0
                    opacity: m_rail.expansionProgress
                    anchors.left: m_logo.right
                    anchors.leftMargin: 12
                    anchors.verticalCenter: m_logo.verticalCenter
                    text: "waywallen"
                    typescale: MD.Token.typescale.title_large
                }
            }

            footer: Item {
                implicitHeight: m_rail_footer.implicitHeight + m_about.height + m_rail_footer.spacing * m_rail.expansionProgress

                Column {
                    id: m_rail_footer
                    width: parent.width
                    spacing: 12 * (1 - m_rail.expansionProgress)

                    W.SidebarNowPlaying {
                        id: m_now_playing
                        width: parent.width
                        visible: W.App.presentationManager.count > 0
                        expanded: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        model: W.App.presentationManager.model
                        onOpenRequested: wallpaperId => root.openWallpaper(wallpaperId)

                        Binding {
                            target: m_rail
                            property: "drawerGestureEnabled"
                            value: !m_now_playing.pointerHovered
                        }
                    }

                    MD.RailItem {
                        width: parent.width
                        visible: root._railIndex("plugins_qml") >= 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: false
                        icon.name: MD.Token.icon.extension
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: qsTranslate("Window", "Plugins")
                        property var presentation: null
                        onSelectionRequested: {
                            if (presentation?.active)
                                return;
                            presentation = root.presentPopup('waywallen.ui/PagePopup', {
                                source: 'waywallen.ui/PluginManagePage'
                            });
                        }
                    }

                    MD.RailItem {
                        width: parent.width
                        visible: root._railIndex("settings_qml") >= 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: false
                        icon.name: MD.Token.icon.settings
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: qsTranslate("Window", "Settings")
                        property var presentation: null
                        onSelectionRequested: {
                            if (presentation?.active)
                                return;
                            presentation = root.presentPopup('waywallen.ui/PagePopup', {
                                source: 'waywallen.ui/SettingsPage'
                            });
                        }
                    }

                    // REXON: our «Dev» item in the collapsed rail. This is a
                    // page, not a window on top, so the item is highlighted
                    // while it is open. When the rail expands it fades out —
                    // mirroring the «About» item — and gives way to the icon
                    // in the header.
                    MD.RailItem {
                        width: parent.width
                        visible: root._devIndex >= 0 && opacity > 0
                        opacity: 1 - m_rail.expansionProgress
                        height: implicitHeight * (1 - m_rail.expansionProgress)
                        enabled: m_rail.expansionProgress === 0
                        expand: m_rail.useLarge
                        expansionProgress: m_rail.expansionProgress
                        selected: root._devIndex >= 0 && root.currentIndex === root._devIndex
                        icon.name: MD.Token.icon.build
                        iconStyle: MD.Enum.IconAndText
                        collapsedIconStyle: MD.Enum.IconOnly
                        text: root._devIndex >= 0 ? root.railItems[root._devIndex].label : ""
                        onSelectionRequested: root.pageRequested(root._devIndex)
                    }
                }

                MD.RailItem {
                    id: m_about
                    visible: opacity > 0 && root._railIndex("about_qml") >= 0
                    opacity: m_rail.expansionProgress
                    width: parent.width
                    y: m_rail_footer.height + m_rail_footer.spacing * m_rail.expansionProgress
                    expand: true
                    selected: false
                    icon.name: MD.Token.icon.info
                    text: qsTranslate("Window", "About")
                    height: implicitHeight * m_rail.expansionProgress
                    enabled: m_rail.expansionProgress === 1
                    onSelectionRequested: root.showAbout()
                }
            }
        }
    }

    // ── Bottom navigation bar (narrow tab) ───────────────────────
    //
    // The layout is the vendor's `Window.qml`, block "Bottom navigation bar". As
    // there, the bar holds only pages: «Plugins», «Settings» and «About»
    // disappear from navigation in the vendor on a narrow window.
    Loader {
        id: m_bar_loader
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        active: root.railItems.length > 0 && root.isCompact
        visible: active

        sourceComponent: MD.Pane {
            padding: 0
            backgroundColor: MD.MProp.color.surface_container
            elevation: MD.Token.elevation.level2

            contentItem: RowLayout {
                Repeater {
                    model: root._railPages

                    Item {
                        Layout.fillWidth: true
                        implicitHeight: 12 + children[0].implicitHeight + 16
                        required property var modelData
                        required property int index

                        MD.BarItem {
                            anchors.fill: parent
                            anchors.topMargin: 12
                            anchors.bottomMargin: 16
                            icon.name: parent.modelData.icon
                            text: parent.modelData.name
                            selected: root._railCurrent === parent.index
                            onSelectionRequested: root.pageRequested(parent.modelData.page)
                        }
                    }
                }
            }
        }
    }

    // ── Nested pages (pushItem / pop of the page context) ─────────
    //
    // The shell has no stack of its own — just as the rail page container in the
    // vendor's `Window.qml` has none. `pushItem` is called by three vendor pages
    // («Settings» → «Excluded windows», «Libraries» → «Add
    // library»), and all of them open as a window on top
    // (`component/PagePopup.qml`): it listens to the signals of its context itself
    // and keeps the stack — with a «back» button, a title and state restoration.
    // The shell's former "simple stack" was left from the times when «Settings»
    // was a rail page; after the move into a window on top not a single call
    // reached it.
    onPageUrlChanged: Qt.callLater(root._switchPage)

    // QR-code login — as in the vendor's `Window.qml`: the window lives in the shell and
    // opens itself on the daemon event `qrLoginProgress`. The event is sent by
    // a source plugin that has a login action (Steam login by
    // QR code in the Workshop plugin); without this window the click started the login,
    // but there was nothing to show the code with.
    W.QrLoginDialog {
        // REXON: in the vendor the login window gets its text and background colors by
        // inheritance from the main window (`Window.qml` sets them on itself). Our
        // shell is not a window, and its colors do not reach the overlay window layer: the caption
        // under the code came out black on dark. The same two values — explicitly.
        MD.MProp.textColor: root.MD.MProp.textColor
        MD.MProp.backgroundColor: root.MD.MProp.backgroundColor
    }

    // waywallen 0.6.1: sign-in through a browser window (a source plugin's
    // `browser_login` action, e.g. Pixiv). Like the QR dialog above it lives
    // in the shell, as in the vendor `Window.qml`, and opens itself on the
    // daemon's `browserLoginProgress` event; the browser window itself is the
    // daemon's helper. Same explicit colours, for the same reason.
    W.BrowserLoginDialog {
        MD.MProp.textColor: root.MD.MProp.textColor
        MD.MProp.backgroundColor: root.MD.MProp.backgroundColor
    }

    // ── Ctrl+P: pause or resume all wallpapers ────────────────
    //
    // Port of the shortcut from the vendor's `Window.qml`: a shared pause toggle and
    // the toast «Paused» / «Resumed».
    //
    // REXON: in the vendor the shortcut works in the whole application
    // (`Qt.ApplicationShortcut`) — there the application is the wallpaper. Here other
    // Rexon sections live in the same window, and pausing the wallpaper from «Appearance» or
    // «Settings» would be a surprise: the shortcut works only while the
    // wallpaper tab is on screen.
    W.GlobalPauseToggleQuery {
        id: m_pause_toggle
        onToggled: paused => W.Action.toast(paused ? qsTranslate("Window", "Paused") : qsTranslate("Window", "Resumed"))
    }

    Shortcut {
        sequence: "Ctrl+P"
        context: Qt.WindowShortcut
        enabled: root.visible && W.Notify.daemonPhase === W.Notify.DaemonPhase.Ready && !m_pause_toggle.querying
        onActivated: m_pause_toggle.reload()
    }

    // Toasts — as in the vendor's `Window.qml`: in the overlay window layer, with margins of
    // 24 on the left and right. The shell shrinks the layer to the tab (see above), so
    // the toast lands on the wallpaper tab, not on the Rexon sections above it.
    MD.SnakeView {
        id: m_snake
        parent: MD.Overlay.overlay
        anchors.fill: parent
        anchors.leftMargin: 24
        anchors.rightMargin: 24
    }

    // ⚠️ The signal is called `toast`, NOT `toasted`. The old name was left from the
    // Python bridge, which renamed the original's signal and thereby
    // diverged from the vendor's `Window.qml`. The bridge has long been in Rust and its signal
    // is as in the original — but the handler here stayed old, and not a single
    // toast of the tab was EVER shown: `Connections` to a nonexistent
    // signal silently does nothing.
    Connections {
        target: W.Action
        function onToast(text, duration, flags, action) {
            m_snake.show(text, duration, flags, action);
        }
    }

    // A transport error — with the same toast as the vendor's `Window.qml`:
    // otherwise a socket drop and a request failure never reach the person at all.
    //
    // A message rather than an error (for example, «live wallpaper did not start — the previous
    // wallpaper was restored») goes through its own signal and fades by itself: the timeout is computed by the core
    // (`ui_base::toast_timeout_for_text`). The vendor's error toast has no timeout —
    // such a message hung for over a minute on top of the panel buttons, including
    // the filter (VM, 30.10.2026).
    Connections {
        target: W.App
        function onErrorOccurred(error) {
            W.Global.toastError(error);
        }
        function onNoticeOccurred(text, durationMs) {
            W.Action.toast(text, durationMs, MD.Enum.TFCloseable, null);
        }
    }

    // Daemon events the vendor window reports with a toast from any
    // page (`Window.qml`, "Global daemon-event toasts"). Labels — from the
    // vendor's translation catalog, with its own context.
    Connections {
        target: W.Notify
        function onLibrariesAdded(paths) {
            const n = paths.length;
            W.Action.toast(qsTranslate("Window", "%n library(s) added", "", n));
        }
        function onDisplayConnectionFailed(clientName, clientProtocolVersion, errorCode, reason) {
            const who = clientName.length > 0 ? clientName : qsTranslate("Window", "Display client");
            W.Global.toastError(qsTranslate("Window", "%1 connection failed: %2").arg(who).arg(reason));
        }
        function onPluginRestartFailed(pluginId, error) {
            const who = pluginId.length > 0 ? pluginId : qsTranslate("Window", "Plugin");
            W.Action.toast(qsTranslate("Window", "%1 renderer restart failed: %2").arg(who).arg(error), 6000, 1, null);
        }
    }
}
