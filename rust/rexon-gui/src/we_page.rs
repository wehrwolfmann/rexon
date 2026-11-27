// WallpaperEngineModel — the shell of the "Wallpaper Engine" tab in Rust + cxx-qt + QML.
//
// Ported to QML following the common tab scheme. This is the FIRST step of
// porting the tab: the Navigation Rail and an empty page area. The content
// of the items is waywallen's vendored QML (MIT, (c) hypengw; attribution
// in NOTICE) — it comes in the next steps, so it is not here at all yet.
//
// Warning: how this tab differs from the six already ported. Their page is
// entirely OURS: markup in `qml/<Name>Page.qml`, data in the neighbouring
// bridge. The wallpaper tab's pages are vendored and have long been in QML
// — only the shell AROUND them remained in Python: the rail, the
// "connecting…"/"no daemon"/"modules not built" states and the hidden
// "Developer" page. It is the shell that is being ported, and there is a
// basis for porting it: all its decisions and labels already lie in
// `rexon_core::we_window` (ported in commit 8151d7f).
//
// What is here and what is not:
//   * there IS the QObject shell: the rail items as ready records, the
//     selected item, the "plugins built" and "developer mode" flags, the
//     skin palette and the subscription to its change;
//   * there is NOT a single line of product decisions — the rail
//     composition, the "item → vendored page" mapping, the button labels,
//     the parity divergence marker and the placeholder text are computed by
//     the core (`we_window::build_rail`, `rail_button_label`,
//     `TEXT_NO_QML_MODULES`).
//
// Warning: the type is called `WallpaperEngineModel`, not
// `WallpaperEnginePage`: the module's .qml files themselves become QML
// types by file name, and `WallpaperEnginePage` is taken by
// `qml/WallpaperEnginePage.qml`. A name clash gives "Ambiguous type
// detected" and a page that cannot find its own data source.

/// cxx-qt bridge: the shell of the "Wallpaper Engine" tab.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Rail sizes and labels. CONSTANT: they do not change during the
        // tab's lifetime, they are constants of the core.
        #[qproperty(i32, rail_width, cxx_name = "railWidth",
                    READ = rail_width, CONSTANT)]
        #[qproperty(QString, rail_logo, cxx_name = "railLogo",
                    READ = rail_logo, CONSTANT)]
        #[qproperty(i32, rail_button_min_height, cxx_name = "railButtonMinHeight",
                    READ = rail_button_min_height, CONSTANT)]
        // Shape and font size of an item button. In the widget version they are
        // set by the style sheet from the core (`we_window::rail_qss`), and the
        // SAME numbers are taken here: the rail must look the same in both
        // versions of the tab.
        #[qproperty(i32, rail_button_radius, cxx_name = "railButtonRadius",
                    READ = rail_button_radius, CONSTANT)]
        #[qproperty(i32, rail_button_font_size, cxx_name = "railButtonFontSize",
                    READ = rail_button_font_size, CONSTANT)]
        // Highlight of the SELECTED item — also as a ready `#AARRGGBB` colour from
        // the core, not a fraction over the accent: the fraction `0.25` is
        // truncated to 63/255 in widgets, while the QML scene computed by the
        // fraction itself, and the highlight came out one step lighter
        // (measurement 04.09).
        #[qproperty(QString, rail_selected_color,
                    cxx_name = "railSelectedColor",
                    READ = rail_selected_color, NOTIFY = palette_changed)]
        // Background of the rail strip — as a ready `#AARRGGBB` colour from the
        // core, not a fraction: widgets and the QML scene round the fraction
        // differently, and the strip came out one step lighter (measurement
        // 04.09).
        #[qproperty(QString, rail_background_color,
                    cxx_name = "railBackgroundColor",
                    READ = rail_background_color, NOTIFY = palette_changed)]
        // Font size and weight of the logo letter: the same numbers with which the
        // widget version's style sheet paints it (`logo_qss`).
        #[qproperty(i32, logo_font_size, cxx_name = "logoFontSize",
                    READ = logo_font_size, CONSTANT)]
        #[qproperty(i32, logo_font_weight, cxx_name = "logoFontWeight",
                    READ = logo_font_weight, CONSTANT)]
        // Numbers of the start screens ("connecting…", "daemon unavailable"):
        // font sizes, spacing and column margins. In the widget version they are
        // set by the same core constants through `wewin_constants`, here by the
        // same numbers.
        #[qproperty(i32, state_text_font_size, cxx_name = "stateTextFontSize",
                    READ = state_text_font_size, CONSTANT)]
        #[qproperty(i32, no_daemon_icon_font_size, cxx_name = "noDaemonIconFontSize",
                    READ = no_daemon_icon_font_size, CONSTANT)]
        #[qproperty(i32, state_column_spacing, cxx_name = "stateColumnSpacing",
                    READ = state_column_spacing, CONSTANT)]
        #[qproperty(i32, state_column_margin, cxx_name = "stateColumnMargin",
                    READ = state_column_margin, CONSTANT)]
        // Opacity of MUTED text (the original's `on_surface_variant`): the waiting
        // text in the widget version is the `Text` role with this fraction, not a
        // separate skin colour.
        #[qproperty(f64, variant_alpha, cxx_name = "variantAlpha",
                    READ = variant_alpha, CONSTANT)]
        // Whether the active skin is dark. The question is decided by the core by
        // the same criterion as in the whole product: the rail background is
        // computed from it, and "steam_dark in the name" is not an answer — the
        // customer chooses the skin, and "Light" must count as light.
        #[qproperty(bool, skin_dark, cxx_name = "skinDark",
                    READ = skin_dark, NOTIFY = palette_changed)]
        // What to paint the vendored interface with: the Rexon skin (by default)
        // or waywallen's own scheme (the "As in waywallen" skin). Decided by the
        // core (`we_window::vendor_theme`); the shell only puts the colours into
        // Material.
        #[qproperty(bool, vendor_theme_follow, cxx_name = "vendorThemeFollow",
                    READ = vendor_theme_follow, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_accent, cxx_name = "vendorAccent",
                    READ = vendor_accent, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_background, cxx_name = "vendorBackground",
                    READ = vendor_background, NOTIFY = palette_changed)]
        #[qproperty(QString, vendor_text, cxx_name = "vendorText",
                    READ = vendor_text, NOTIFY = palette_changed)]
        // Material scheme roles in skin colours, as a JSON object (`{}` for
        // waywallen's own scheme) — for `MD.Token.color.overrides`.
        #[qproperty(QString, vendor_roles, cxx_name = "vendorRoles",
                    READ = vendor_roles, NOTIFY = palette_changed)]
        // Placeholder text when the vendored interface's plugins are not built.
        #[qproperty(QString, no_qml_modules_text, cxx_name = "noQmlModulesText",
                    READ = no_qml_modules_text, CONSTANT)]

        // ── Start states of the tab: "connecting…", "no daemon", "pages". The
        // state numbers, labels, icon and reason text come from the core
        // (`we_window::STATE_*`, `TEXT_*`, `no_daemon_text`). There is not a single
        // label for the user of our own here: the product must speak with the
        // same words from the widget tab and from this one.
        #[qproperty(i32, state, READ = state, NOTIFY = state_changed)]
        #[qproperty(i32, state_connecting, cxx_name = "stateConnecting",
                    READ = state_connecting, CONSTANT)]
        #[qproperty(i32, state_no_daemon, cxx_name = "stateNoDaemon",
                    READ = state_no_daemon, CONSTANT)]
        #[qproperty(i32, state_pages, cxx_name = "statePages",
                    READ = state_pages, CONSTANT)]
        #[qproperty(QString, connecting_text, cxx_name = "connectingText",
                    READ = connecting_text, CONSTANT)]
        #[qproperty(QString, no_daemon_icon, cxx_name = "noDaemonIcon",
                    READ = no_daemon_icon, CONSTANT)]
        #[qproperty(QString, retry_daemon_label, cxx_name = "retryDaemonLabel",
                    READ = retry_daemon_label, CONSTANT)]
        // The reason lives together with the state, so they share a signal: a
        // separate signal for the text would mean two pushes for one transition.
        #[qproperty(QString, no_daemon_text, cxx_name = "noDaemonText",
                    READ = no_daemon_text, NOTIFY = state_changed)]
        // How long to wait for the `health` answer — the page host sets up the
        // watchdog.
        #[qproperty(i32, health_timeout_ms, cxx_name = "healthTimeoutMs",
                    READ = health_timeout_ms, CONSTANT)]
        // The overall engine wait timeout: the watchdog of the "Connecting…"
        // screen is set up by the markup itself, and the timeout comes from the
        // core (`we_window::CONNECT_TIMEOUT_MS`).
        #[qproperty(i32, connect_timeout_ms, cxx_name = "connectTimeoutMs",
                    READ = connect_timeout_ms, CONSTANT)]

        // ── The environment the tab opened in. Answered once when the object
        // is created: neither the plugins nor the dev mode marker appear or
        // disappear while Rexon is running.
        #[qproperty(bool, qml_available, cxx_name = "qmlAvailable",
                    READ = qml_available, CONSTANT)]
        #[qproperty(bool, dev_mode, cxx_name = "devMode",
                    READ = dev_mode, CONSTANT)]

        // ── The rail itself. The items change together with the parity
        // divergence marker: it lives right in the label of the "Dev." button.
        #[qproperty(QList_QVariant, rail_items, cxx_name = "railItems",
                    READ = rail_items, NOTIFY = rail_changed)]
        #[qproperty(i32, stub_index, cxx_name = "stubIndex",
                    READ = stub_index, NOTIFY = rail_changed)]
        #[qproperty(i32, dev_index, cxx_name = "devIndex",
                    READ = dev_index, NOTIFY = rail_changed)]
        #[qproperty(bool, parity_drift, cxx_name = "parityDrift",
                    READ = parity_drift, WRITE = set_parity_drift,
                    NOTIFY = rail_changed)]
        #[qproperty(i32, current_index, cxx_name = "currentIndex",
                    READ = current_index, WRITE = set_current_index,
                    NOTIFY = current_index_changed)]

        // Directory of waywallen's vendored interface (`qml_ui/` of the package).
        //
        // Warning: the value comes from the page host rather than being computed
        // here: the vendored QML lies INSIDE the `rexon/` package, and the address
        // of the loaded `.so` says nothing about the package layout — it is
        // delivered into the QML modules directory. The same argument as for
        // `we_env::pkg_dir`.
        #[qproperty(QString, vendor_qml_dir, cxx_name = "vendorQmlDir",
                    READ = vendor_qml_dir, WRITE = set_vendor_qml_dir,
                    NOTIFY = vendor_qml_dir_changed)]

        // Address of the shell around the vendored page (`PageHost.qml`) — OUR
        // file next to the vendored interface. Empty until the host names the
        // directory.
        #[qproperty(QUrl, page_host_url, cxx_name = "pageHostUrl",
                    READ = page_host_url, NOTIFY = vendor_qml_dir_changed)]

        // Palette of the active skin (rule #17). Not CONSTANT: the skin is changed
        // on the fly, and without a signal an open tab would stay in the previous
        // colours.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]

        // ── "Whose background": the strip above the page when a third-party shell
        // is alive (Noctalia, DankMaterialShell). The decision, text and labels
        // are the core's (`rexon_core::desktop_shell`); here there is only laying
        // them out into properties and two actions for the buttons.
        #[qproperty(bool, shell_banner_visible, cxx_name = "shellBannerVisible",
                    READ = shell_banner_visible, NOTIFY = shell_state_changed)]
        #[qproperty(QString, shell_banner_text, cxx_name = "shellBannerText",
                    READ = shell_banner_text, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_can_give, cxx_name = "shellCanGive",
                    READ = shell_can_give, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_can_return, cxx_name = "shellCanReturn",
                    READ = shell_can_return, NOTIFY = shell_state_changed)]
        #[qproperty(QString, shell_give_label, cxx_name = "shellGiveLabel",
                    READ = shell_give_label, CONSTANT)]
        #[qproperty(QString, shell_return_label, cxx_name = "shellReturnLabel",
                    READ = shell_return_label, CONSTANT)]
        // Outcome of the last press (success or refusal in the core's words) and
        // the refusal flag — so the markup paints it differently. Empty means
        // there were no presses.
        #[qproperty(QString, shell_action_message, cxx_name = "shellActionMessage",
                    READ = shell_action_message, NOTIFY = shell_state_changed)]
        #[qproperty(bool, shell_action_failed, cxx_name = "shellActionFailed",
                    READ = shell_action_failed, NOTIFY = shell_state_changed)]
        type WallpaperEngineModel = super::WallpaperEngineModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// The skin was changed on the fly — recompute the palette. The push comes
        /// from the shared registry (`crate::skin`), not from here.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut WallpaperEngineModel>);

        /// The composition or labels of the rail items changed.
        #[qsignal]
        #[cxx_name = "railChanged"]
        fn rail_changed(self: Pin<&mut WallpaperEngineModel>);

        #[qsignal]
        #[cxx_name = "currentIndexChanged"]
        fn current_index_changed(self: Pin<&mut WallpaperEngineModel>);

        #[qsignal]
        #[cxx_name = "vendorQmlDirChanged"]
        fn vendor_qml_dir_changed(self: Pin<&mut WallpaperEngineModel>);

        /// The tab switched to another screen: "connecting…", "no daemon" (together
        /// with the reason) or "pages".
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut WallpaperEngineModel>);

        /// "Whose background" was recomputed (or one of the two buttons was
        /// pressed).
        #[qsignal]
        #[cxx_name = "shellStateChanged"]
        fn shell_state_changed(self: Pin<&mut WallpaperEngineModel>);
    }

    // ── Property reading ──────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "railWidth"]
        fn rail_width(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railLogo"]
        fn rail_logo(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "railButtonMinHeight"]
        fn rail_button_min_height(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railButtonRadius"]
        fn rail_button_radius(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railButtonFontSize"]
        fn rail_button_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "railSelectedColor"]
        fn rail_selected_color(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "railBackgroundColor"]
        fn rail_background_color(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "logoFontSize"]
        fn logo_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "logoFontWeight"]
        fn logo_font_weight(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateTextFontSize"]
        fn state_text_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "noDaemonIconFontSize"]
        fn no_daemon_icon_font_size(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateColumnSpacing"]
        fn state_column_spacing(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateColumnMargin"]
        fn state_column_margin(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "variantAlpha"]
        fn variant_alpha(self: &WallpaperEngineModel) -> f64;
        #[cxx_name = "skinDark"]
        fn skin_dark(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "vendorThemeFollow"]
        fn vendor_theme_follow(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "vendorAccent"]
        fn vendor_accent(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorBackground"]
        fn vendor_background(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorText"]
        fn vendor_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "vendorRoles"]
        fn vendor_roles(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noQmlModulesText"]
        fn no_qml_modules_text(self: &WallpaperEngineModel) -> QString;
        fn state(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateConnecting"]
        fn state_connecting(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "stateNoDaemon"]
        fn state_no_daemon(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "statePages"]
        fn state_pages(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "connectingText"]
        fn connecting_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noDaemonIcon"]
        fn no_daemon_icon(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "retryDaemonLabel"]
        fn retry_daemon_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "noDaemonText"]
        fn no_daemon_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "healthTimeoutMs"]
        fn health_timeout_ms(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "connectTimeoutMs"]
        fn connect_timeout_ms(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "qmlAvailable"]
        fn qml_available(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "devMode"]
        fn dev_mode(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "railItems"]
        fn rail_items(self: &WallpaperEngineModel) -> QList_QVariant;
        #[cxx_name = "stubIndex"]
        fn stub_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "devIndex"]
        fn dev_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "parityDrift"]
        fn parity_drift(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "setParityDrift"]
        fn set_parity_drift(self: Pin<&mut WallpaperEngineModel>, value: bool);
        #[cxx_name = "currentIndex"]
        fn current_index(self: &WallpaperEngineModel) -> i32;
        #[cxx_name = "setCurrentIndex"]
        fn set_current_index(self: Pin<&mut WallpaperEngineModel>, value: i32);
        fn palette(self: &WallpaperEngineModel) -> QMap_QString_QVariant;
        #[cxx_name = "vendorQmlDir"]
        fn vendor_qml_dir(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "setVendorQmlDir"]
        fn set_vendor_qml_dir(self: Pin<&mut WallpaperEngineModel>, value: &QString);
        #[cxx_name = "pageHostUrl"]
        fn page_host_url(self: &WallpaperEngineModel) -> QUrl;
        #[cxx_name = "shellBannerVisible"]
        fn shell_banner_visible(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellBannerText"]
        fn shell_banner_text(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellCanGive"]
        fn shell_can_give(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellCanReturn"]
        fn shell_can_return(self: &WallpaperEngineModel) -> bool;
        #[cxx_name = "shellGiveLabel"]
        fn shell_give_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellReturnLabel"]
        fn shell_return_label(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellActionMessage"]
        fn shell_action_message(self: &WallpaperEngineModel) -> QString;
        #[cxx_name = "shellActionFailed"]
        fn shell_action_failed(self: &WallpaperEngineModel) -> bool;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// File name of the vendored page for item `idx`; empty means the item is
        /// not vendored (the "modules not built" placeholder or "Developer").
        #[qinvokable]
        #[cxx_name = "qmlForIndex"]
        fn qml_for_index(self: &WallpaperEngineModel, idx: i32) -> QString;

        /// Identifier of item `idx` (`wallpapers_qml`, `dev`, …); empty means there
        /// is no such item. Needed by checks: they drive the page by item names,
        /// not by numbers, which depend on dev mode.
        #[qinvokable]
        #[cxx_name = "railItemId"]
        fn rail_item_id(self: &WallpaperEngineModel, idx: i32) -> QString;

        /// Show the engine waiting screen.
        #[qinvokable]
        #[cxx_name = "showConnecting"]
        fn show_connecting(self: Pin<&mut WallpaperEngineModel>);

        /// Show the refusal screen with a reason.
        ///
        /// `reason` is the raw explanation from the daemon client, not a ready
        /// label: the core assembles the string for the user from it.
        #[qinvokable]
        #[cxx_name = "showNoDaemon"]
        fn show_no_daemon(self: Pin<&mut WallpaperEngineModel>, reason: &QString);

        /// The same, but the reason is single and known in advance: the daemon
        /// connected and is silent on the readiness check.
        #[qinvokable]
        #[cxx_name = "showHealthTimeout"]
        fn show_health_timeout(self: Pin<&mut WallpaperEngineModel>);

        /// The OVERALL engine wait timeout ([`ww::CONNECT_TIMEOUT_MS`]) expired:
        /// show the refusal with a retry button.
        ///
        /// Called by the markup's watchdog, because it waits precisely for the
        /// SCREEN: the paths that get stuck on it (the socket did not open, endless
        /// reconnect) have no watchdog of their own either in the transport or in
        /// the core.
        #[qinvokable]
        #[cxx_name = "showConnectTimeout"]
        fn show_connect_timeout(self: Pin<&mut WallpaperEngineModel>);

        /// Parse the `health` answer of the connected daemon and switch to the
        /// right screen. The verdict is given by the core
        /// (`we_window::health_verdict`).
        #[qinvokable]
        #[cxx_name = "applyHealth"]
        fn apply_health(self: Pin<&mut WallpaperEngineModel>, state: &QString);

        /// Address of the vendored page of item `idx` for the `Loader`. An empty
        /// address means there is nothing to load: the item is not vendored or the
        /// host has not named the interface directory yet, and the `Loader` stays
        /// empty.
        #[qinvokable]
        #[cxx_name = "vendorPageUrl"]
        fn vendor_page_url(self: &WallpaperEngineModel, idx: i32) -> QUrl;

        /// Whether the page of item `idx` needs the vendored HEADER (`MD.AppBar`
        /// with its title and action buttons). The answer is held by the core
        /// (`we_window::RailItem::header`) because it is the same for both versions
        /// of the tab.
        #[qinvokable]
        #[cxx_name = "vendorPageHeader"]
        fn vendor_page_header(self: &WallpaperEngineModel, idx: i32) -> bool;

        /// Recompute "whose background" from the live session (the markup calls it
        /// when the tab opens and on a timer: the shell may be started and stopped
        /// while the window is open).
        #[qinvokable]
        #[cxx_name = "refreshShellState"]
        fn refresh_shell_state(self: Pin<&mut WallpaperEngineModel>);

        /// "Give the wallpaper to Rexon": with the user's consent (the press is the
        /// consent) turn off the shell's wallpaper, remembering the previous value.
        #[qinvokable]
        #[cxx_name = "giveWallpaperToRexon"]
        fn give_wallpaper_to_rexon(self: Pin<&mut WallpaperEngineModel>);

        /// "Give back to the shell": restore what was there before Rexon.
        #[qinvokable]
        #[cxx_name = "returnWallpaperToShell"]
        fn return_wallpaper_to_shell(self: Pin<&mut WallpaperEngineModel>);
    }

    // The trait declaration must be IN THE BRIDGE, otherwise
    // `impl cxx_qt::Initialize` outside does not find the constructor: cxx-qt
    // emits it from this line.
    impl cxx_qt::Initialize for WallpaperEngineModel {}
    // The skin change subscription goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE.
    impl cxx_qt::Threading for WallpaperEngineModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QUrl, QVariant};

use rexon_core::desktop_shell as shell;
use rexon_core::we_window as ww;
use rexon_core::{ui_base, utils};

use crate::we_env;

// ============================================================================
// Pure layer: the tab environment (no Qt here)
// ============================================================================

/// Whether the hidden developer mode is on.
///
/// The branching is entirely in the core (`we_window::dev_mode`): here there
/// is only reading the environment and one file check on disk — the same
/// order as `is_dev_mode()` in `rexon/qt_wallpaper_engine/window.py`.
pub fn dev_mode_now() -> bool {
    let raw = std::env::var(ww::DEV_ENV_VAR).unwrap_or_default();
    let marker = utils::expand_user(
        ww::DEV_MARKER_PATH,
        &we_env::home(),
        we_env::xdg_config_home().as_deref(),
    );
    ww::dev_mode(&raw, std::path::Path::new(&marker).is_file())
}

/// Ready records of the rail items for QML.
///
/// A separate function so that the item composition is checked without Qt:
/// the button label is assembled by the core, and here there is only laying
/// it out by keys.
pub fn rail_entries(plan: &ww::RailPlan, drift: bool) -> Vec<RailEntry> {
    plan.items
        .iter()
        .enumerate()
        .map(|(idx, item)| RailEntry {
            id: item.id.clone(),
            icon: item.icon.clone(),
            label: item.label.clone(),
            qml: item.qml.clone(),
            // The parity divergence marker lives in the label and only on the
            // "Developer" item — as in the widget version. The item is recognized by
            // its number from the plan, not by the string `dev`: the item name is
            // decided by the core, and a second copy of it here would silently drift
            // apart.
            button_label: ww::rail_button_label(
                &item.icon,
                &item.label,
                drift && plan.dev_index == Some(idx),
            ),
        })
        .collect()
}

/// One rail item record in the form QML reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailEntry {
    /// Item identifier (`wallpapers_qml`, `dev`, …).
    pub id: String,
    /// Emoji icon.
    pub icon: String,
    /// Label under the icon.
    pub label: String,
    /// File name of the vendored page; empty for non-vendored items.
    pub qml: String,
    /// Ready button label (icon, line break, label, marker).
    pub button_label: String,
}

// ============================================================================
// Small conversions into Qt
// ============================================================================

/// Build a QVariantMap from pairs. QML reads such a map with a dot
/// (`item.label`).
fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Rust state of the tab shell.
pub struct WallpaperEngineModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the page is not subscribed yet.
    skin_watch: u64,
    /// The rail plan from the core: the items, the placeholder index, the dev
    /// page index.
    plan: ww::RailPlan,
    /// Whether the vendored interface's plugins are built.
    qml_available: bool,
    /// Whether the hidden developer mode is on.
    dev_mode: bool,
    /// Whether a parity divergence was found (the marker on the "Dev." button).
    parity_drift: bool,
    /// The selected rail item.
    current_index: i32,
    /// Directory of waywallen's vendored interface; named by the page host.
    vendor_qml_dir: String,
    /// The shown screen: "connecting…", "no daemon" or "pages".
    state: i32,
    /// Refusal reason for the "no daemon" screen; empty means there is no
    /// reason, and the core's short line without a colon is shown.
    no_daemon_reason: String,
    /// The "whose background" strip — as the core's ready decision.
    shell_banner: shell::ShellBanner,
    /// Outcome of the last press and the refusal flag.
    shell_action_message: String,
    shell_action_failed: bool,
}

/// The plan is built AT ONCE, not on first access: QML reads `railItems`
/// before any of our code, and the user would read an empty rail on the
/// first frame as a broken tab.
impl Default for WallpaperEngineModelRust {
    fn default() -> Self {
        let qml_available = we_env::qml_plugins_ready();
        let dev_mode = dev_mode_now();
        Self {
            skin_watch: 0,
            plan: ww::build_rail(qml_available, dev_mode),
            qml_available,
            dev_mode,
            parity_drift: false,
            current_index: 0,
            vendor_qml_dir: String::new(),
            // The tab opens on "connecting…" — like the widget one, whose state stack
            // starts with the same screen. The daemon has not been asked yet, and
            // drawing pages over a dead socket would mean showing the customer
            // emptiness without an explanation.
            state: ww::STATE_CONNECTING as i32,
            no_daemon_reason: String::new(),
            // The strip is computed on the first `refreshShellState` from the markup,
            // not here: collecting reads the session directory and `/proc`, and the
            // object constructor is not the place for that.
            shell_banner: shell::ShellBanner::default(),
            shell_action_message: String::new(),
            shell_action_failed: false,
        }
    }
}

/// Remove the skin change subscription: otherwise the registry would keep
/// the entry of a closed tab until the first broadcast.
impl Drop for WallpaperEngineModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl qobject::WallpaperEngineModel {
    // ── Rail sizes and labels ─────────────────────────────────────────────
    fn rail_width(&self) -> i32 {
        ww::RAIL_WIDTH
    }

    fn rail_logo(&self) -> QString {
        QString::from(ww::RAIL_LOGO)
    }

    fn rail_button_min_height(&self) -> i32 {
        ww::RAIL_BUTTON_MIN_HEIGHT
    }

    fn rail_button_radius(&self) -> i32 {
        ww::RAIL_BUTTON_RADIUS
    }

    fn rail_button_font_size(&self) -> i32 {
        ww::RAIL_BUTTON_FONT_SIZE
    }

    /// Background of the rail strip in the active skin's colour.
    fn rail_background_color(&self) -> QString {
        QString::from(ww::rail_background_argb_hex(self.skin_dark()).as_str())
    }

    fn logo_font_size(&self) -> i32 {
        ww::LOGO_FONT_SIZE
    }

    fn logo_font_weight(&self) -> i32 {
        ww::LOGO_FONT_WEIGHT
    }

    /// Highlight of the selected item in the active skin's colour.
    ///
    /// The colour is taken from the same palette that goes to QML (`accent`),
    /// and the opacity is applied by the core — as an eight-bit value, the way
    /// the widget version's style sheet reads it.
    fn rail_selected_color(&self) -> QString {
        let (r, g, b) = ui_base::active_skin_colors()
            .get("accent")
            .and_then(|c| ui_base::parse_hex(c))
            .unwrap_or((102, 192, 244));
        QString::from(ww::rail_selected_argb_hex(r, g, b).as_str())
    }

    fn state_text_font_size(&self) -> i32 {
        ww::STATE_TEXT_FONT_SIZE
    }

    fn no_daemon_icon_font_size(&self) -> i32 {
        ww::NO_DAEMON_ICON_FONT_SIZE
    }

    fn state_column_spacing(&self) -> i32 {
        ww::STATE_COLUMN_SPACING
    }

    fn state_column_margin(&self) -> i32 {
        ww::STATE_COLUMN_MARGIN
    }

    fn variant_alpha(&self) -> f64 {
        ww::VARIANT_ALPHA as f64
    }

    /// Whether the active skin is dark — by the same criterion as the
    /// neighbouring QML tabs (`ui_base::is_dark_rgb` by the background colour).
    fn skin_dark(&self) -> bool {
        ui_base::active_skin_colors()
            .get("bg_primary")
            .and_then(|c| ui_base::parse_hex(c))
            .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
            .unwrap_or(true)
    }

    /// Whether to paint the vendored interface with waywallen's own scheme.
    fn vendor_theme_follow(&self) -> bool {
        ww::active_vendor_theme().follow_vendor
    }

    /// Seed of the Material scheme — the active skin's accent.
    fn vendor_accent(&self) -> QString {
        QString::from(&ww::active_vendor_theme().accent)
    }

    /// Background of the vendored interface's shell — the skin's page
    /// background.
    fn vendor_background(&self) -> QString {
        QString::from(&ww::active_vendor_theme().background)
    }

    /// Main text of the shell — the skin's page text.
    fn vendor_text(&self) -> QString {
        QString::from(&ww::active_vendor_theme().text)
    }

    /// Material scheme roles in skin colours, as a JSON object.
    fn vendor_roles(&self) -> QString {
        QString::from(&ww::vendor_roles_json(&ww::active_vendor_theme()))
    }

    fn no_qml_modules_text(&self) -> QString {
        // There is a build, but for another Qt version — explain that rather than
        // sending the user to build what is already built (09.10.2026, "Settings"
        // crash after the Qt 6.11 → 6.12 update).
        if !self.qml_available {
            if let Some((built, running)) = we_env::qml_modules_wrong_qt() {
                return QString::from(&ww::text_qml_modules_wrong_qt(built, running));
            }
        }
        QString::from(&ww::text_no_qml_modules())
    }

    // ── Tab environment ───────────────────────────────────────────────────
    fn qml_available(&self) -> bool {
        self.rust().qml_available
    }

    fn dev_mode(&self) -> bool {
        self.rust().dev_mode
    }

    // ── Rail ──────────────────────────────────────────────────────────────
    fn rail_items(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for entry in rail_entries(&self.rust().plan, self.rust().parity_drift) {
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&entry.id)),
                ("icon", qstr(&entry.icon)),
                ("label", qstr(&entry.label)),
                ("qml", qstr(&entry.qml)),
                ("buttonLabel", qstr(&entry.button_label)),
            ])));
        }
        out
    }

    /// Index of the "modules not built" placeholder; -1 means there is no
    /// placeholder in the rail.
    fn stub_index(&self) -> i32 {
        idx_or_none(self.rust().plan.stub_index)
    }

    /// Index of the "Developer" page; -1 means dev mode is off.
    fn dev_index(&self) -> i32 {
        idx_or_none(self.rust().plan.dev_index)
    }

    fn parity_drift(&self) -> bool {
        self.rust().parity_drift
    }

    /// The parity divergence marker: changes the label of the "Dev." button.
    fn set_parity_drift(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().parity_drift == value {
            return;
        }
        self.as_mut().rust_mut().parity_drift = value;
        self.as_mut().rail_changed();
    }

    fn current_index(&self) -> i32 {
        self.rust().current_index
    }

    /// Select a rail item. A number outside the rail is ignored: the rail is
    /// short and its composition depends on dev mode, and a jump to a
    /// non-existent page would leave the tab empty.
    fn set_current_index(mut self: Pin<&mut Self>, value: i32) {
        let count = self.rust().plan.items.len() as i32;
        if value < 0 || value >= count || value == self.rust().current_index {
            return;
        }
        self.as_mut().rust_mut().current_index = value;
        self.as_mut().current_index_changed();
    }

    /// Palette of the ACTIVE skin (the saved `ui_skin`, falling back to the
    /// standard one by rule #17). The shell has no colours of its own.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // Background dimming under a modal window — one for the product, with the
        // same string as the neighbouring QML tabs.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    // ── API for QML ───────────────────────────────────────────────────────
    fn qml_for_index(&self, idx: i32) -> QString {
        if idx < 0 {
            return QString::default();
        }
        QString::from(self.rust().plan.qml_for_index(idx as usize).unwrap_or(""))
    }

    fn vendor_qml_dir(&self) -> QString {
        QString::from(&self.rust().vendor_qml_dir)
    }

    /// Tell the page where waywallen's vendored interface lies.
    fn set_vendor_qml_dir(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if self.rust().vendor_qml_dir == value {
            return;
        }
        self.as_mut().rust_mut().vendor_qml_dir = value;
        self.as_mut().vendor_qml_dir_changed();
    }

    /// Address of the shell around the vendored page (`PageHost.qml`).
    ///
    /// Warning: the vendored page is NOT loaded directly but inside this shell
    /// — the same way the Python host loaded it. It plays the role of their
    /// `Window.qml`: it keeps the Material mode by the Rexon skin, the size
    /// class (`MProp.size`) and the toasts. Without it the "Wallpapers" page
    /// opens empty: its layout asks for the size class, and without a Material
    /// host it is zero. Checked live on 30.08 — five pages out of six were
    /// drawn without the shell too, but "Wallpapers" was not.
    ///
    /// The file name is held by the core, the directory is named by the page
    /// host.
    fn page_host_url(&self) -> QUrl {
        let dir = &self.rust().vendor_qml_dir;
        if dir.is_empty() {
            return QUrl::default();
        }
        local_file_url(&rexon_core::qml_modules::qml_path(
            dir,
            rexon_core::qml_modules::PAGE_HOST_QML,
        ))
    }

    /// Address of the vendored page for the `Loader`.
    ///
    /// The path is assembled by the core (`qml_modules::vendor_page_path`) —
    /// with the same joining the Python host used. There is no second copy of
    /// the vendored directory layout here.
    fn vendor_page_url(&self, idx: i32) -> QUrl {
        let dir = &self.rust().vendor_qml_dir;
        if dir.is_empty() || idx < 0 {
            return QUrl::default();
        }
        // Warning: we ask for the VENDORED name, not just any: the "Dev." item does
        // have a QML file name, but it is OUR page from the `org.rexon.gui` module,
        // and it is not in the waywallen directory. With the general
        // `qml_for_index` the name of our page would go here, and the `Loader`
        // would get the address of a non-existent file.
        let Some(page) = self.rust().plan.vendor_qml_for_index(idx as usize) else {
            return QUrl::default();
        };
        let path = rexon_core::qml_modules::vendor_page_path(dir, page);
        if path.is_empty() {
            return QUrl::default();
        }
        local_file_url(&path)
    }

    /// Whether the page of item `idx` needs a header from the host.
    ///
    /// Warning: the property is set on the `PageHost.qml` shell, not on the page
    /// itself: `MD.Page` draws the header by `MD.MProp.page.showHeader` — by the
    /// HOST's context. A page that lives in a pop-up window in waywallen
    /// (`PluginManagePage.qml`) does not set its `showHeader` at all and in our
    /// rail opened without a title and without any action buttons.
    fn vendor_page_header(&self, idx: i32) -> bool {
        idx >= 0 && self.rust().plan.header_for_index(idx as usize)
    }

    fn rail_item_id(&self, idx: i32) -> QString {
        if idx < 0 {
            return QString::default();
        }
        let items = &self.rust().plan.items;
        QString::from(items.get(idx as usize).map(|i| i.id.as_str()).unwrap_or(""))
    }

    // ── Start states of the tab ───────────────────────────────────────────
    //
    // The screen numbers have no meaning of their own here: both the numbers
    // and the labels are held by the core, and the markup compares the `state`
    // property with named constants rather than with numbers.

    fn state(&self) -> i32 {
        self.rust().state
    }

    fn state_connecting(&self) -> i32 {
        ww::STATE_CONNECTING as i32
    }

    fn state_no_daemon(&self) -> i32 {
        ww::STATE_NO_DAEMON as i32
    }

    fn state_pages(&self) -> i32 {
        ww::STATE_PAGES as i32
    }

    fn connecting_text(&self) -> QString {
        QString::from(&ww::text_connecting())
    }

    fn no_daemon_icon(&self) -> QString {
        QString::from(ww::ICON_NO_DAEMON)
    }

    fn retry_daemon_label(&self) -> QString {
        QString::from(&ww::text_retry_daemon())
    }

    fn health_timeout_ms(&self) -> i32 {
        ww::HEALTH_TIMEOUT_MS
    }

    fn connect_timeout_ms(&self) -> i32 {
        ww::CONNECT_TIMEOUT_MS
    }

    /// Text of the refusal screen.
    ///
    /// Warning: the RAW reason is stored, while the label assembled by the core
    /// for the user is shown: the reason itself is machine-oriented (paths,
    /// `PATH`, the `health` request name) and belongs in the log, not on the
    /// screen (rule #13). An empty reason is a separate case: the screen is
    /// shown even before the engine's first answer.
    fn no_daemon_text(&self) -> QString {
        let reason = &self.rust().no_daemon_reason;
        if reason.is_empty() {
            return QString::from(&ww::text_no_daemon());
        }
        QString::from(&ww::no_daemon_text_in(reason, crate::we_env::rexon_packaged()))
    }

    /// Switch to a screen. One entry for all transitions: the signal is sent
    /// exactly once and only when something really changed.
    fn switch_state(mut self: Pin<&mut Self>, state: i32, reason: String) {
        if self.rust().state == state && self.rust().no_daemon_reason == reason {
            return;
        }
        self.as_mut().rust_mut().state = state;
        self.as_mut().rust_mut().no_daemon_reason = reason;
        self.as_mut().state_changed();
    }

    fn show_connecting(self: Pin<&mut Self>) {
        self.switch_state(ww::STATE_CONNECTING as i32, String::new());
    }

    fn show_no_daemon(self: Pin<&mut Self>, reason: &QString) {
        self.fail(reason.to_string());
    }

    fn show_health_timeout(self: Pin<&mut Self>) {
        self.fail(ww::TEXT_HEALTH_TIMEOUT.to_string());
    }

    fn show_connect_timeout(self: Pin<&mut Self>) {
        self.fail(ww::REASON_CONNECT_TIMEOUT.to_string());
    }

    /// One entry for all refusals: the reason goes to the log, the screen to
    /// the customer.
    ///
    /// It has to be logged here rather than in the transport: refusals the
    /// transport never saw also reach the screen (an expired wait, an unready
    /// `health` answer), and the log must know ALL of them — otherwise the
    /// technical detail would disappear from the screen without appearing in
    /// the log.
    fn fail(self: Pin<&mut Self>, reason: String) {
        if let Some(line) = ww::daemon_failure_log_line(&reason) {
            // The shared Rexon log (`rexon_core::applog`).
            rexon_core::log_warn!("wallpaper-tab", "{line}");
        }
        self.switch_state(ww::STATE_NO_DAEMON as i32, reason);
    }

    /// The `health` answer of the connected daemon.
    ///
    /// Warning: an open socket does NOT yet mean the engine works: it may have
    /// come up and crashed on its database migration. Without this check the
    /// customer would get pages that silently show nothing. The verdict on the
    /// answer is given by the core; an empty answer means an old daemon, and we
    /// trust it.
    fn apply_health(self: Pin<&mut Self>, state: &QString) {
        match ww::health_verdict(&state.to_string()) {
            ww::HealthVerdict::Ready => {
                self.switch_state(ww::STATE_PAGES as i32, String::new())
            }
            ww::HealthVerdict::NotReady(reason) => self.fail(reason),
        }
    }
}

/// Address of a local file (`QUrl::fromLocalFile`) — assembled by us from
/// the scheme and the path.
///
/// Warning: cxx-qt-lib 0.9 has no ready `from_local_file` at all, and
/// gluing the string "file://" + path is not allowed: the user's path may
/// be anything (the owner's is Cyrillic), and in a string address such
/// bytes would have to be encoded by hand. `set_path` puts the path as is,
/// and QUrl itself takes care of the encoding — exactly as in
/// `QUrl::fromLocalFile`.
pub(crate) fn local_file_url(path: &str) -> QUrl {
    let mut url = QUrl::default();
    url.set_scheme(&QString::from("file"));
    url.set_path(&QString::from(path));
    url
}

/// `Option<usize>` into a number for QML: "no such item" is -1, not 0.
fn idx_or_none(value: Option<usize>) -> i32 {
    value.map(|v| v as i32).unwrap_or(-1)
}

// ============================================================================
// State initialization
// ============================================================================

// ── "Whose background" ─────────────────────────────────────────────────────
impl qobject::WallpaperEngineModel {
    fn shell_banner_visible(&self) -> bool {
        self.rust().shell_banner.visible
    }

    fn shell_banner_text(&self) -> QString {
        QString::from(&self.rust().shell_banner.text)
    }

    fn shell_can_give(&self) -> bool {
        self.rust().shell_banner.can_give
    }

    fn shell_can_return(&self) -> bool {
        self.rust().shell_banner.can_return
    }

    fn shell_give_label(&self) -> QString {
        QString::from(&shell::give_label())
    }

    fn shell_return_label(&self) -> QString {
        QString::from(&shell::return_label())
    }

    fn shell_action_message(&self) -> QString {
        QString::from(&self.rust().shell_action_message)
    }

    fn shell_action_failed(&self) -> bool {
        self.rust().shell_action_failed
    }

    fn refresh_shell_state(mut self: Pin<&mut Self>) {
        let banner = shell::live_shell_wallpaper_state().banner;
        if banner != self.rust().shell_banner {
            self.as_mut().rust_mut().shell_banner = banner;
            self.as_mut().shell_state_changed();
        }
    }

    fn give_wallpaper_to_rexon(self: Pin<&mut Self>) {
        // Which shell — from the same answer by which the button was shown, not
        // from the markup: the button cannot be pressed without a live shell.
        let target = shell_from_id(&self.rust().shell_banner.shell);
        let result = match target {
            Some(s) => shell::hand_over(s, &shell::ShellHome::live()),
            None => return,
        };
        self.finish_shell_action(result);
    }

    fn return_wallpaper_to_shell(self: Pin<&mut Self>) {
        let target = shell_from_id(&self.rust().shell_banner.shell);
        let result = match target {
            Some(s) => shell::give_back(s, &shell::ShellHome::live()),
            None => return,
        };
        self.finish_shell_action(result);
    }

    fn finish_shell_action(mut self: Pin<&mut Self>, result: Result<String, String>) {
        let (message, failed) = match result {
            Ok(m) => (m, false),
            Err(m) => (m, true),
        };
        {
            let mut r = self.as_mut().rust_mut();
            r.shell_action_message = message;
            r.shell_action_failed = failed;
            r.shell_banner = shell::live_shell_wallpaper_state().banner;
        }
        self.as_mut().shell_state_changed();
    }
}

/// Shell by the strip identifier (`noctalia` | `dms`).
fn shell_from_id(id: &str) -> Option<rexon_core::system::DesktopShell> {
    rexon_core::system::DesktopShell::all().iter().copied().find(|s| s.as_str() == id)
}

impl cxx_qt::Initialize for qobject::WallpaperEngineModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Skin change subscription: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the same state.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::WallpaperEngineModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full rail: seven vendored items, no parity marker.
    #[test]
    fn rail_entries_full() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let plan = ww::build_rail(true, false);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries.len(), 7);
        assert_eq!(entries[0].id, "wallpapers_qml");
        assert_eq!(entries[0].qml, "WallpaperPage.qml");
        assert_eq!(entries[0].button_label, "🖼️\nWallpapers");
    }

    /// Plugins not built: there is one item, it is also the placeholder, and it
    /// has no vendored page.
    #[test]
    fn rail_entries_without_plugins() {
        let plan = ww::build_rail(false, false);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "no_qml");
        assert!(entries[0].qml.is_empty());
        assert_eq!(plan.stub_index, Some(0));
    }

    /// The parity divergence marker hangs ONLY on the "Developer" item.
    #[test]
    fn drift_mark_only_on_dev_item() {
        let plan = ww::build_rail(true, true);
        let entries = rail_entries(&plan, true);
        assert_eq!(entries.len(), 8);
        assert_eq!(plan.dev_index, Some(7));
        assert_eq!(entries[7].id, "dev");
        // "Developer" has its OWN page: there is a file name, but no vendored
        // one. Otherwise the shell would look for it in the waywallen directory.
        assert_eq!(entries[7].qml, ww::DEV_PAGE_QML);
        assert_eq!(plan.vendor_qml_for_index(7), None);
        assert!(entries[7].button_label.ends_with(ww::RAIL_DRIFT_MARK));
        for entry in &entries[..7] {
            assert!(!entry.button_label.ends_with(ww::RAIL_DRIFT_MARK));
        }
    }

    /// Without a divergence there is no marker on "Developer" either.
    #[test]
    fn no_drift_no_mark() {
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let plan = ww::build_rail(true, true);
        let entries = rail_entries(&plan, false);
        assert_eq!(entries[7].button_label, "🛠️\nDev");
    }

    /// The waiting watchdog must be present in the markup and be bound to the
    /// SCREEN.
    ///
    /// It is not there for looks: if this `Timer` disappears, the tab will hang
    /// on "Connecting…" forever again, and only the customer will find out,
    /// because neither the build nor `cargo test` executes the markup. Binding
    /// to the state (rather than to the attempt) is the second half of the
    /// contract: only it allows retrying any number of times and stops the
    /// countdown on a successful connection.
    #[test]
    fn page_guards_the_waiting_screen() {
        const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");
        for part in [
            "interval: page.connectTimeoutMs",
            "running: page.state === page.stateConnecting",
            "onTriggered: page.showConnectTimeout()",
        ] {
            assert!(
                PAGE_QML.contains(part),
                "у страницы «Wallpaper Engine» пропал сторож ожидания \
                 («{part}»): экран «Подключение…» снова станет бесконечным"
            );
        }
    }

    /// The "whose background" strip is in the markup, its buttons call the
    /// model's actions, and the recomputation runs both on opening and on a
    /// timer — otherwise a shell started after the window opened would go
    /// unnoticed.
    #[test]
    fn page_shows_whose_background_it_is() {
        const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");
        for part in [
            "objectName: \"shellBanner\"",
            "visible: page.shellBannerVisible",
            "onClicked: page.giveWallpaperToRexon()",
            "onClicked: page.returnWallpaperToShell()",
            "visible: page.shellCanGive",
            "visible: page.shellCanReturn",
            "triggeredOnStart: true",
            "onTriggered: page.refreshShellState()",
            // The Plasma wallpaper plugin banner — on its own action object.
            "objectName: \"kdePluginBanner\"",
            "visible: ownActions.kdePluginNotice !== \"\"",
            "onClicked: ownActions.installKdePlugin()",
        ] {
            assert!(PAGE_QML.contains(part), "у страницы пропало «{part}»");
        }
    }

    /// "No such item" goes to QML as -1: zero there would mean the first rail
    /// item, that is, an existing page.
    #[test]
    fn missing_index_is_minus_one() {
        assert_eq!(idx_or_none(None), -1);
        assert_eq!(idx_or_none(Some(0)), 0);
        assert_eq!(idx_or_none(Some(6)), 6);
    }
}
