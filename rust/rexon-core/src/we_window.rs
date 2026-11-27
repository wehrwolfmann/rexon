//! Scaffolding of the «Wallpaper Engine» tab — port of `rexon/qt_wallpaper_engine/window.py`.
//!
//! The tab content itself is waywallen's vendored QML (MIT, (c) hypengw,
//! attribution in NOTICE), and its data is already handled by [`crate::we_client`],
//! [`crate::we_query`] and neighbours. The window module remained the SCAFFOLDING around
//! the moved bridge: the Navigation Rail and its items, the states
//! «connecting…»/«no daemon»/«QML modules not built», the hidden
//! «Developer» page and three network workers (release check, AppImage
//! install, OWE plugin install).
//!
//! Everything that is DECIDED and NAMED in this scaffolding lives here: the rail
//! composition and the «rail item → stack page» mapping, parsing `REXON_DEV`, texts and
//! colours of the dev page statuses, step-by-step worker verdicts, the verdict on the
//! daemon's health response and the style sheets. The Qt side is left to build
//! widgets from a ready plan — exactly the same boundary as in
//! [`crate::qt_settings`] and [`crate::onboarding`].
//!
//! Network, file system and WebSocket do not come in here: paths and parsing already
//! exist in [`crate::engine_update`], the schema check — in [`crate::we_parity`].
//!
//! ⚠️ Fixes of the original (rule #16 — bugs are fixed during the port, not preserved),
//! each one is marked `Fix N:` in place:
//!   1. the AppImage install worker removed the downloaded ~200 MiB temporary file
//!      only on cancel and on failed validation. If the install ITSELF failed —
//!      the file stayed: `install_appimage` calls `os.remove(new_path)` only
//!      on the success branch, and the worker did not clean up after itself. On a machine with `/tmp`
//!      in RAM this is a RAM leak until reboot. Now the verdict
//!      of each step carries the `remove_tmp` flag, and an install failure sets it;
//!   2. the OWE plugin IPC install timeout (60 s) was armed by a one-shot
//!      timer without identifying the attempt. A second install started within
//!      a minute after the daemon answered the first one got the OLD timer: it
//!      deleted its temporary zip and reported a false «the daemon did not answer».
//!      Now an attempt has a number, and the timeout verdict checks it;
//!   3. the engine version label had two different names: before the first
//!      check «Engine version: —», after it — «waywallen version: …». One
//!      field, two labels; now there is one name.
//!
//! Parity test — `rust/rexon-core/tests/we_window_parity.rs`.

// ============================================================================
// Hidden developer mode
// ============================================================================

/// Environment variable that turns dev mode on/off for one run.
pub const DEV_ENV_VAR: &str = "REXON_DEV";

/// Marker file of the permanent dev mode (expanded by `expand_user`, which
/// already respects `XDG_CONFIG_HOME` — no second XDG branch is needed here).
pub const DEV_MARKER_PATH: &str = "~/.config/rexon/dev_mode";

/// What a `REXON_DEV` value parses into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevEnv {
    /// `1`/`true`/`yes` — turn on, do not ask the marker.
    ForceOn,
    /// `0`/`false`/`no` — turn off even with the marker («as the buyer will see it»).
    ForceOff,
    /// Empty or anything else — the marker file decides.
    Unset,
}

/// Parse `REXON_DEV` (case and surrounding spaces do not matter).
pub fn parse_dev_env(raw: &str) -> DevEnv {
    match raw.trim().to_lowercase().as_str() {
        "0" | "false" | "no" => DevEnv::ForceOff,
        "1" | "true" | "yes" => DevEnv::ForceOn,
        _ => DevEnv::Unset,
    }
}

/// Result of `is_dev_mode()`: env outranks the marker in both directions.
pub fn dev_mode(raw_env: &str, marker_exists: bool) -> bool {
    match parse_dev_env(raw_env) {
        DevEnv::ForceOff => false,
        DevEnv::ForceOn => true,
        DevEnv::Unset => marker_exists,
    }
}

use crate::i18n::n_;
use std::collections::BTreeMap;

// ============================================================================
// Navigation Rail and pages
// ============================================================================

/// Width of the collapsed Navigation Rail, px.
pub const RAIL_WIDTH: i32 = 96;

/// Logo letter above the rail items.
pub const RAIL_LOGO: &str = "W";

/// Minimum height of a rail item button, px.
pub const RAIL_BUTTON_MIN_HEIGHT: i32 = 56;

/// Mark on the «Dev» button when parity diverges.
pub const RAIL_DRIFT_MARK: &str = " ⚠️";

/// Corner radius of a rail item button, px — HALF of its height.
///
/// ⭐ There is no number of our own here anymore. waywallen has no radius
/// constant at all: its rail draws the highlight of the selected item as a
/// «pill» — `radius: height / 2` (`qml/navigation/RailItem.qml`,
/// lines 148 and 156 of the `Qcm.Material` module, the revision is pinned in
/// `rexon/qt_wallpaper_engine/qml_ui/waywallen/ui/QML_MATERIAL_PIN`).
/// The former 12 px were OURS and did not rest on anything at the vendor: they
/// came from the style sheet of our own widget replica. What is written here is
/// the same RULE, not a number fitted to it — if the item height changes,
/// the radius follows.
///
/// ⚠️ The constant stays named not for looks: the tab has two
/// versions, and the QML version draws the buttons itself, not via a Qt style sheet. While
/// the number lived as a literal inside `rail_qss`, QML had nowhere to ask for it
/// — and the QML rail came out with a foreign radius. Computing `height / 2` in
/// place is not possible either: a Qt style sheet cannot do arithmetic, it needs pixels.
pub const RAIL_BUTTON_RADIUS: i32 = RAIL_BUTTON_MIN_HEIGHT / 2;

/// Font size of the label on a rail item button, px.
///
/// ⭐ The number is the VENDOR's, not ours: waywallen's collapsed rail item is labelled
/// with the `MD.Token.typescale.label_medium` face (`RailItem.qml`, line
/// 111), and `label_medium` in `Qcm.Material` is exactly 12 px
/// (`include/qml_material/token/type_scale.hpp`, line 106; the same
/// pinned revision). It matched the font size of our widget replica —
/// so nothing to change, but the vendor counts as the source.
///
/// ⚠️ The constant cannot be removed for the same reason as the radius: both
/// versions of the tab draw the label themselves, and they have nowhere to ask
/// `Qcm.Material` for the font size. The `Qcm.Material` module is NOT imported in
/// `WallpaperEnginePage.qml` ON PURPOSE — otherwise the tab would not open where
/// the QML plugins are not built (the reasoning is in that file's header).
pub const RAIL_BUTTON_FONT_SIZE: i32 = 12;

/// One rail item and, for QML pages, its QML file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailItem {
    /// Item id (`wallpapers_qml`, `dev`, …).
    pub id: String,
    /// Emoji icon.
    pub icon: String,
    /// Label under the icon — already in the interface language (the constructors
    /// receive the English msgid and translate it themselves).
    pub label: String,
    /// Page QML file name; empty for non-QML items.
    pub qml: String,
    /// Is the page a VENDOR one (a file inside waywallen's `qml_ui/`)?
    ///
    /// ⚠️ The flag was not added for tidiness. While «has a QML name» meant
    /// «the page is a vendor one», our own QML page
    /// («Developer») had no way to get a file name: any non-empty
    /// name immediately went into building the vendor path
    /// (`qml_modules::vendor_page_path`) — and the page would have been looked up in a foreign
    /// directory. Now there are two questions: «does the item have QML» and «whose is it».
    pub vendor: bool,
    /// Whether to show the page's vendor HEADER (`MD.AppBar` with its title
    /// and action buttons).
    ///
    /// ⭐ The flag is not about looks but about whether the page is usable at all.
    /// `MD.Page` draws the header by `MD.MProp.page.showHeader` — a property of the
    /// page's OWNER, not of the page itself
    /// (`Qcm.Material/qml/control/Page.qml`, line 25). For `PageContext`
    /// it is `false` by default (`src/control/page_context.cpp`, line
    /// 71), and the global context a page without an owner gets does not
    /// change the default.
    ///
    /// waywallen has TWO owners, and they decide differently:
    ///   * the window with the rail (`ui/qml/Window.qml`, the `MD.PageContext` of the page
    ///     container) — `showHeader: false`;
    ///   * the page popup window (`component/PagePopup.qml`,
    ///     `MD.PageContext`) — `showHeader: true`.
    /// Rail pages either have no header at all or turn it on FOR THEMSELVES
    /// (`SettingsPage.qml` — `showHeader: true`, `StatusPage.qml` and
    /// `DisplaysPage.qml` — only in a narrow window). Popup window pages
    /// do not rely on their own `showHeader`: `PagePopup` sets it
    /// for them.
    ///
    /// ⚠️ Our rail holds both kinds, so the header decision is made by the
    /// owner — `qml_ui/PageHost.qml` — while the knowledge of which page is
    /// whose lives HERE, one for both versions of the tab. Without it a vendor
    /// page from a popup window opens in the rail WITHOUT a header, i.e.
    /// without its title and without ALL action buttons: for
    /// `PluginManagePage.qml` those are «Inactive plugins», «Check for
    /// updates» and «Install from .zip» — all plugin installing and updating
    /// at once. The plugin list is still drawn, and the screenshot
    /// looks working (caught by a live screenshot on 08.09.2026).
    ///
    /// The rule by which the flag is set is guarded by
    /// `tests/we_rail_vendor_parity.rs`: it reads the vendor page itself.
    pub header: bool,
}

impl RailItem {
    /// An item with a waywallen VENDOR page that draws its header
    /// itself or has none at all — as in waywallen's own window.
    fn vendor(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            id: id.to_string(),
            icon: icon.to_string(),
            label: crate::i18n::tr(label),
            qml: qml.to_string(),
            vendor: true,
            header: false,
        }
    }

    /// An item with a VENDOR page that lives in a popup window at waywallen
    /// and therefore expects the header FROM THE OWNER (see `header`).
    fn vendor_headed(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            header: true,
            ..Self::vendor(id, icon, label, qml)
        }
    }

    /// An item with OUR page (the `org.rexon.gui` QML module) or with no
    /// page at all — then the file name is empty.
    fn own(id: &str, icon: &str, label: &str, qml: &str) -> Self {
        Self {
            id: id.to_string(),
            icon: icon.to_string(),
            label: crate::i18n::tr(label),
            qml: qml.to_string(),
            vendor: false,
            header: false,
        }
    }
}

/// Specs of the tab's QML pages (migration switch 2026-07-12). All pages
/// are shown by ONE shared `SharedQmlHost`: several `QQmlEngine`s with the
/// `waywallen.control` module deadlock `QQmlThread`.
pub fn qml_page_specs() -> Vec<RailItem> {
    vec![
        RailItem {
            label: wallpapers_rail_label(),
            ..RailItem::vendor("wallpapers_qml", "🖼️", "Wallpapers", "WallpaperPage.qml")
        },
        RailItem::vendor("discover_qml", "🔍", n_("Discover"), "DiscoverPage.qml"),
        RailItem::vendor("displays_qml", "🖥️", n_("Displays"), "DisplaysPage.qml"),
        RailItem::vendor("status_qml", "📈", n_("Status"), "StatusPage.qml"),
        // ⚠️ «Plugins» is placed HERE, between «Status» and «Settings», not
        // by taste: at waywallen this is the bottom trio of the rail in the order «Plugins,
        // Settings, About» (`ui/qml/Window.qml`, rail footer).
        //
        // At the vendor all three are not rail pages but buttons that open a
        // popup window (`presentPopup('waywallen.ui/PagePopup', {source:
        // …})`). Two of the three have long lived as page items here: our
        // rail is a list of pages, and «Settings» and «About» were already
        // accepted in that form. The third simply got lost in the port, and
        // the buyer could not reach plugin management at all —
        // to install, update or delete a renderer.
        //
        // The page is the very same vendor one (`page/PluginManagePage.qml`,
        // `MD.Page`, like «Settings» and «About»), and everything it
        // rests on we already have: the six request types
        // (`PluginListQuery`, `PluginInstallQuery`, `PluginInspectQuery`,
        // `PluginDeleteQuery`, `PluginUpdateCheckQuery`,
        // `PluginUpdateInstallQuery`) are printed by the `rexon-gui` crate
        // (`src/plugin_queries.rs`), and its own popup window
        // «inactive plugins» calls `Window.window.presentPopup` — a method
        // our shell `qml_ui/PageHost.qml` puts on the window.
        //
        // ⚠️ The item is created as `vendor_headed`, not `vendor`: at waywallen this
        // page lives in a popup window, and its title with three
        // buttons («Inactive plugins», «Check for updates»,
        // «Install from .zip») lies in the HEADER the owner turns on.
        // Created as a regular vendor page, it opened WITHOUT a header: the plugin list
        // is visible, but there is nothing to install or update with (see `header`).
        RailItem::vendor_headed("plugins_qml", "🧩", n_("Plugins"), "PluginManagePage.qml"),
        RailItem::vendor("settings_qml", "⚙️", n_("Settings"), "SettingsPage.qml"),
        RailItem::vendor("about_qml", "ℹ️", n_("About"), "AboutPage.qml"),
    ]
}

/// Placeholder item when the QML plugins are not built.
pub fn no_qml_rail_item() -> RailItem {
    RailItem {
        label: wallpapers_rail_label(),
        ..RailItem::own("no_qml", "🖼️", "Wallpapers", "")
    }
}

/// Label of the «Wallpapers» item in the rail — with its own translation context.
///
/// The rail is 96 px wide, and the appearance type label «Wallpapers»
/// (`constants.rs`, wallpaper-pack) does not fit there: Qt squeezed the German
/// «Hintergrundbilder» down to 8 px (screenshot 01.10.2026). The context
/// separates the two strings so that the rail shows the short «Hintergründe».
fn wallpapers_rail_label() -> String {
    // TRANSLATORS: label of the wallpapers page in a narrow 96 px side rail — keep it short
    crate::i18n::trc("side rail", "Wallpapers")
}

/// File name of OUR «Developer» page in the `org.rexon.gui` QML module.
///
/// The item has no vendor page and cannot have one: we write the page, not
/// waywallen. So the name lives here, not in `qml_page_specs`.
pub const DEV_PAGE_QML: &str = "WeDeveloperPage.qml";

/// The hidden «Developer» item.
pub fn dev_rail_item() -> RailItem {
    // TRANSLATORS: short label of the hidden developer page in a 96 px rail
    RailItem::own("dev", "🛠️", n_("Dev"), DEV_PAGE_QML)
}

/// Plan for building the rail and the page stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RailPlan {
    /// Rail items in order; item index == page index in the stack.
    pub items: Vec<RailItem>,
    /// Index of the «QML modules not built» placeholder page, if there is one.
    pub stub_index: Option<usize>,
    /// Index of the dev page, if there is one.
    pub dev_index: Option<usize>,
}

impl RailPlan {
    /// QML file name for stack page `idx`; `None` — not a QML page.
    pub fn qml_for_index(&self, idx: usize) -> Option<&str> {
        self.items
            .get(idx)
            .filter(|it| !it.qml.is_empty())
            .map(|it| it.qml.as_str())
    }

    /// File name of the VENDOR page for `idx`; `None` — the page is either our
    /// own or does not exist at all.
    ///
    /// This is exactly what must be asked everywhere the name goes into building a path
    /// inside `qml_ui/`: our page does not lie there.
    pub fn vendor_qml_for_index(&self, idx: usize) -> Option<&str> {
        self.items
            .get(idx)
            .filter(|it| it.vendor && !it.qml.is_empty())
            .map(|it| it.qml.as_str())
    }

    /// Whether page `idx` needs the vendor HEADER from the owner (see
    /// `RailItem::header`). No such item — `false`: the header is
    /// off by default in `PageContext` itself too.
    pub fn header_for_index(&self, idx: usize) -> bool {
        self.items.get(idx).is_some_and(|it| it.header)
    }
}

/// Build the rail plan: `qml_available` — whether the QML plugins are built,
/// `dev_mode` — whether to show the hidden «Developer» page.
///
/// The rail item index and the page index in `QStackedWidget` match in
/// both cases — `_nav()` relies on that.
pub fn build_rail(qml_available: bool, dev_mode: bool) -> RailPlan {
    let mut items = if qml_available {
        qml_page_specs()
    } else {
        vec![no_qml_rail_item()]
    };
    let stub_index = if qml_available { None } else { Some(0) };
    let mut dev_index = None;
    if dev_mode {
        dev_index = Some(items.len());
        items.push(dev_rail_item());
    }
    RailPlan {
        items,
        stub_index,
        dev_index,
    }
}

/// Rail button label: icon and label separated by a newline, plus
/// the parity divergence mark for «Dev».
pub fn rail_button_label(icon: &str, label: &str, drift: bool) -> String {
    format!(
        "{icon}\n{label}{}",
        if drift { RAIL_DRIFT_MARK } else { "" }
    )
}

// ============================================================================
// Colours and style sheets
// ============================================================================

/// Opacity of muted text (the original's `on_surface_variant`).
pub const VARIANT_ALPHA: f32 = 0.6;

/// Opacity of the selected rail item highlight.
pub const RAIL_SELECTED_ALPHA: f32 = 0.25;

/// Opacity of the rail strip backdrop on a DARK skin.
///
/// ⚠️ The number moved out of the style sheet into a named constant for the same
/// reason as the button radius and font size: the QML version draws the rail strip
/// itself, and while the number lived as a literal inside `rail_qss`, it had its own
/// copy — `Qt.rgba(1, 1, 1, 0.03)` in the markup.
pub const RAIL_BG_ALPHA_DARK: f32 = 0.03;

/// Opacity of the rail strip backdrop on a LIGHT skin.
pub const RAIL_BG_ALPHA_LIGHT: f32 = 0.02;

/// Rail strip backdrop as a `#AARRGGBB` string — the QML version takes the colour
/// in this form.
///
/// ⚠️ Opacity is given as an EIGHT-BIT number, not a fraction, and this is not
/// nitpicking. The Qt style sheet parser truncates the fraction `0.03` to 7/255, while
/// the QML scene blender computes with the fraction itself (7.65/255) — on the very same
/// skin the strip came out one level different between versions: (33,46,61) for
/// widgets versus (34,47,62) for QML (measured 04.09.2026 on steam_dark,
/// background 27,40,56). An eight-bit alpha is the same on both sides, and the level
/// matches.
pub fn rail_background_argb_hex(dark: bool) -> String {
    let (alpha, rgb) = if dark {
        (RAIL_BG_ALPHA_DARK, 0x00ff_ffff_u32)
    } else {
        (RAIL_BG_ALPHA_LIGHT, 0x0000_0000_u32)
    };
    // Truncation, NOT rounding: exactly how the QSS parser reads the fraction.
    let a = (alpha * 255.0) as u8;
    format!("#{a:02x}{rgb:06x}")
}

/// Highlight of the SELECTED rail item as a `#AARRGGBB` string — the QML version
/// takes the paint in this form. `hl_*` is the colour of the skin's `Highlight` role.
///
/// ⚠️ The same trap as with the strip backdrop, and it is caught only by exact
/// comparison: the Qt style sheet parser truncates the fraction `0.25` to 63/255, while
/// the QML scene computed with the fraction itself (63.75/255) and rounded the premultiplied
/// paint up — the highlight came out one level different between versions:
/// (50,82,106) for widgets versus (51,82,107) for QML (measured 04.09.2026 on
/// steam_dark, rail strip 33,46,61, `Highlight` #66c0f4; divergence
/// on 4224 pixels of the selected item). A separate experiment with the same paints:
/// the fraction `0.25` in the scene gives (51,82,107), the eight-bit alpha `0x40` —
/// also (51,82,107), and `0x3f` — exactly (50,82,106) of the widget version.
pub fn rail_selected_argb_hex(hl_r: u8, hl_g: u8, hl_b: u8) -> String {
    // Truncation, NOT rounding: exactly how the QSS parser reads the fraction.
    let a = (RAIL_SELECTED_ALPHA * 255.0) as u8;
    format!("#{a:02x}{hl_r:02x}{hl_g:02x}{hl_b:02x}")
}

// ============================================================================
// Colours of the vendor interface: Rexon skin or waywallen's own scheme
// ============================================================================

/// What to paint waywallen's vendor interface inside the tab with.
///
/// Its elements are drawn by QmlMaterial, and its paints are not a table but a
/// Material scheme the plugin COMPUTES from one seed colour (accent),
/// darkness and palette type. So «as in the other tabs» is made up
/// of three parts: the seed is the Rexon skin accent, darkness is the darkness of
/// the skin, and the shell background and main text are exactly the skin's page paints (the
/// plugin takes them from the inherited `MProp.backgroundColor` and `MProp.textColor`).
/// On top of the computed scheme, backgrounds, surfaces and text are set with skin
/// paints one by one ([`VendorTheme::roles`]) — in QmlMaterial with the
/// `MdColorMgr::overrides` property. An older QmlMaterial cannot set roles one by one,
/// and cards, dialogs and fields keep the scheme tones derived from the accent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorTheme {
    /// Paint with waywallen's own scheme: its palette type, its «Theme» and
    /// «Accent» settings in the tab's «Settings» window. The other fields are then
    /// not needed by the shell (they only paint the page's start screens).
    pub follow_vendor: bool,
    /// Seed of the Material scheme — the skin accent, `#rrggbb`.
    pub accent: String,
    /// Background of the shell and the rail — the skin's page background (`surface_page`), `#rrggbb`.
    pub background: String,
    /// Main text over the background — the skin's `text_primary`, `#rrggbb`.
    pub text: String,
    /// Whether the skin is dark — selects the dark or light Material scheme.
    pub dark: bool,
    /// Material scheme roles set with skin paints one by one: QmlMaterial role
    /// name → `#rrggbb` (see [`VENDOR_SCHEME_ROLES`]). They go into
    /// `MD.Token.color.overrides`; with a QmlMaterial lacking this property the shell
    /// silently skips them, and only the seed applies. For waywallen's own scheme —
    /// empty.
    pub roles: BTreeMap<String, String>,
}

/// Which Material scheme roles are painted by the skin and with which of its paints.
///
/// The layout is as in waywallen itself: its window is `surface_container`, and
/// the settings and list cards are `surface_container` too, i.e. exactly the
/// window background; the work area under them (`surface`) is one tone deeper, and
/// tiles and chips (`surface_container_high`/`_highest`) are one tone lighter.
/// In the Rexon skin the window is the page background, so the tab's cards are painted with it
/// too, the work area with the field paint (`#16202d` for «Dark (Steam)»),
/// raised elements with the card and button of the neighbouring tabs. Text is
/// `text_primary`, secondary (at waywallen that is almost all labels) is
/// `text_secondary`.
pub const VENDOR_SCHEME_ROLES: &[(&str, &str)] = &[
    ("background", "surface_field"),
    ("surface", "surface_field"),
    ("surface_dim", "surface_field"),
    ("surface_container_lowest", "surface_field"),
    ("surface_container_low", "surface_page"),
    ("surface_container", "surface_page"),
    ("surface_container_high", "surface_card"),
    ("surface_container_highest", "surface_button"),
    ("surface_variant", "surface_button"),
    ("surface_bright", "surface_button"),
    ("on_background", "text_primary"),
    ("on_surface", "text_primary"),
    ("on_surface_variant", "text_secondary"),
    ("outline", "surface_field_outline"),
    ("outline_variant", "surface_button"),
];

/// Vendor interface colours for skin `skin_id`.
///
/// By default the tab follows the Rexon skin, like all other tabs.
/// waywallen's own scheme is returned by a SEPARATE skin
/// [`crate::ui_base::VENDOR_SCHEME_SKIN`] — then the shell touches neither the palette
/// type, nor the theme, nor the accent, and waywallen's own settings work.
/// Paints are taken from the same palette that goes to QML pages
/// ([`crate::ui_base::qml_palette`]): an unknown skin falls back to the
/// standard one right there.
pub fn vendor_theme(skin_id: &str) -> VendorTheme {
    use crate::ui_base;
    let skin = ui_base::skin(skin_id).id;
    let palette = ui_base::qml_palette(skin);
    let pick = |key: &str, fallback: &str| {
        palette
            .get(key)
            .filter(|v| ui_base::parse_hex(v).is_some())
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    };
    let background = pick("surface_page", "#1b2838");
    let dark = ui_base::parse_hex(&background)
        .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
        .unwrap_or(true);
    let follow_vendor = skin == ui_base::VENDOR_SCHEME_SKIN;
    let roles = if follow_vendor {
        BTreeMap::new()
    } else {
        VENDOR_SCHEME_ROLES
            .iter()
            .filter_map(|(role, key)| {
                let value = palette.get(*key)?;
                ui_base::parse_hex(value)?;
                Some(((*role).to_string(), value.clone()))
            })
            .collect()
    };
    VendorTheme {
        follow_vendor,
        accent: pick("accent", "#66c0f4"),
        background,
        text: pick("text_primary", if dark { "#ffffff" } else { "#000000" }),
        dark,
        roles,
    }
}

/// The scheme roles [`VendorTheme::roles`] as a JSON object — the form QML
/// takes them in (`JSON.parse` → `MD.Token.color.overrides`).
pub fn vendor_roles_json(theme: &VendorTheme) -> String {
    serde_json::to_string(&theme.roles).unwrap_or_else(|_| "{}".to_string())
}

/// The same for the ACTIVE skin (the saved `ui_skin`).
pub fn active_vendor_theme() -> VendorTheme {
    vendor_theme(crate::ui_base::active_skin_id())
}

/// Font size of the logo letter above the rail items, px.
///
/// The same reasoning as for the other rail numbers: the QML version draws the letter
/// itself, and it must not have its own copy of the font size.
pub const LOGO_FONT_SIZE: i32 = 22;

/// Weight of the logo letter (`font-weight`).
pub const LOGO_FONT_WEIGHT: i32 = 700;

/// `on_surface_variant` — colour of the `Text` role with 60% alpha.
pub fn variant_color(r: u8, g: u8, b: u8) -> String {
    format!("rgba({r},{g},{b},{VARIANT_ALPHA})")
}

/// Style of the logo letter above the rail (`hl` — `#rrggbb` of the `Highlight` role).
pub fn logo_qss(hl_hex: &str) -> String {
    format!(
        "font-size: {LOGO_FONT_SIZE}px; font-weight: {LOGO_FONT_WEIGHT}; \
color: {hl_hex};"
    )
}

/// Rail style sheet: strip background, button shape, selected highlight.
pub fn rail_qss(dark: bool, hl_r: u8, hl_g: u8, hl_b: u8) -> String {
    let rail_bg = if dark {
        format!("rgba(255,255,255,{RAIL_BG_ALPHA_DARK})")
    } else {
        format!("rgba(0,0,0,{RAIL_BG_ALPHA_LIGHT})")
    };
    let sel_bg = format!("rgba({hl_r},{hl_g},{hl_b},{RAIL_SELECTED_ALPHA})");
    format!(
        "QFrame#weRail {{ background: {rail_bg}; }}\
QFrame#weRail QToolButton {{ border-radius: {RAIL_BUTTON_RADIUS}px; \
font-size: {RAIL_BUTTON_FONT_SIZE}px; padding: 4px; }}\
QFrame#weRail QToolButton:checked {{ background: {sel_bg}; }}"
    )
}

/// Text with a ready style sheet — what is left for the widget to set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Styled {
    /// Label text.
    pub text: String,
    /// Label style sheet.
    pub qss: String,
}

impl Styled {
    fn new(text: impl Into<String>, qss: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            qss: qss.into(),
        }
    }
}

/// A status line before it turned into a style sheet: text,
/// paint and weight separately.
///
/// ⚠️ Introduced because the tab has TWO versions. The widget one needs a ready
/// style sheet ([`Styled`]), the QML scene needs the paint by itself: `color:`
/// of `Text` does not understand style sheets. The decision «what colour this line is»
/// must be ONE (rule #12), so `StatusLine` is primary, and
/// `Styled` is derived from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    /// Line text.
    pub text: String,
    /// Paint `#rrggbb`; empty — no paint, the usual colour.
    pub color: String,
    /// Whether the line is bold.
    pub bold: bool,
}

impl StatusLine {
    fn new(text: impl Into<String>, color: &str, bold: bool) -> Self {
        Self {
            text: text.into(),
            color: color.to_string(),
            bold,
        }
    }

    /// The same line with a ready style sheet — the view for the widget version.
    pub fn to_styled(&self) -> Styled {
        if self.color.is_empty() {
            return Styled::new(self.text.clone(), STATUS_NEUTRAL_QSS);
        }
        Styled::new(self.text.clone(), status_qss(&self.color, self.bold))
    }
}

/// Green «all good».
pub const COLOR_OK: &str = "#2e7d32";
/// Orange «update available / out of sync».
pub const COLOR_WARN: &str = "#e65100";
/// Red «error / divergence».
pub const COLOR_ERR: &str = "#c62828";

fn status_qss(color: &str, bold: bool) -> String {
    if bold {
        format!("font-size: 13px; color: {color}; font-weight: 600;")
    } else {
        format!("font-size: 13px; color: {color};")
    }
}

// ============================================================================
// Start states of the tab
// ============================================================================

/// Index of the «Connecting…» screen in the state stack.
pub const STATE_CONNECTING: usize = 0;
/// Index of the «Daemon unavailable» screen in the state stack.
pub const STATE_NO_DAEMON: usize = 1;
/// Index of the screen with the pages in the state stack.
pub const STATE_PAGES: usize = 2;

/// Delay of the deferred start (daemon detection, upstream auto-check), ms.
pub const STARTUP_DELAY_MS: i32 = 100;

/// How long to wait for the `health` response from a connected daemon, ms.
pub const HEALTH_TIMEOUT_MS: i32 = 5000;

/// How long to wait for the daemon's response to `plugin_install`, ms.
pub const OWE_IPC_TIMEOUT_MS: i32 = 60_000;

/// How long to wait for the network workers to finish on application exit, ms.
pub const WORKER_WAIT_MS: i32 = 60_000;

/// How long IN TOTAL the tab stays on the «Connecting…» screen, ms.
///
/// ⚠️ Without this limit the wait was ENDLESS, and that is not theory: two
/// paths to the waiting screen have no watchdog of their own at all.
///   * the engine is listed on the bus, but the socket does not open — `open()` starts
///     the connection and never returns, neither with success nor with failure;
///   * there was a connection and it dropped — the transport reconnects every
///     3 s WITHOUT a limit on attempts (`we_socket::RECONNECT_MS`), and while the engine
///     is dead, the buyer looks at «Connecting…» for as long as they are willing
///     to look. The «Retry Start» button NEVER appears meanwhile.
///
/// Where 25 seconds come from. A cold start measurement of the daemon on this machine
/// (isolated bench: own bus, own `XDG_RUNTIME_DIR`, own
/// data/config/cache; waited for exactly what the tab waits for — `WsPort`
/// on D-Bus): name on the bus — 0.07–0.19 s, working port — 3.2 / 3.4 / 3.2 s
/// on a warmed-up database and 3.2 s on the first run with migrations; under load
/// (the machine busy with a build) — 5.0 s. So a normal start fits
/// into 3–5 s, and 25 s is a fivefold margin over the worst measurement.
///
/// ⚠️ The upper bound matters more than the lower one here: the limit MUST be larger than the sum
/// of the inner watchdogs (12 s of bus polling [`crate::we_client::POLL_LIMIT`]
/// × [`crate::we_client::POLL_MS`] + [`HEALTH_TIMEOUT_MS`]), otherwise it
/// fires first and replaces the precise failure reason with a generic one. When changing one of
/// the three numbers, check against `we_window_parity.rs` — there it is written down as
/// a test, not as a wish.
///
/// ⚠️ Since 30.10.2026 the limit does NOT run while the engine start watchdog
/// ([`crate::we_client::StartWatch`]) is working: it waits for a live process longer than 12 s
/// (first AppImage run on a slow machine) and ends with a failure itself
/// by its own deadlines. The markup keeps this timer off while the actions
/// object says `engineStarting` — otherwise 25 s would replace the honest
/// «starting…» with a generic failure.
pub const CONNECT_TIMEOUT_MS: i32 = 25_000;

/// Text of the waiting screen.
///
/// «Daemon» is a system administrator's word, and it tells the buyer
/// nothing (rule #13). The name `waywallen` itself stays in Latin: it is the name
/// of a technology, not a translation.
pub fn text_connecting() -> String {
    crate::i18n::tr("Starting the waywallen wallpaper engine…")
}

/// Explanation under «Starting…» when the start takes longer than usual
/// (the quiet window of the start watchdog `we_client::START_QUIET_MS` is past), while the engine
/// process is alive and working. Without it a person would look at the same screen
/// for a minute and decide that everything hung (defect 30.10.2026, r1895).
pub fn text_connecting_slow() -> String {
    crate::i18n::tr(
        "The first start after installation can take a minute or two while the engine \
unpacks itself. Please wait.",
    )
}

/// Text of the failure screen when the reason is unknown.
pub fn text_no_daemon() -> String {
    crate::i18n::tr("The waywallen wallpaper engine did not start")
}

/// Icon of the «Daemon unavailable» screen.
pub const ICON_NO_DAEMON: &str = "🔌";

/// Label of the daemon restart button.
pub fn text_retry_daemon() -> String {
    crate::i18n::tr("Retry Start")
}

/// Font size of the start screen text («connecting…», «daemon unavailable»),
/// px.
///
/// ⚠️ The start screen numbers moved out of the widget tab's style sheets
/// into named constants for the same reason as the rail ones: the tab has
/// two versions, and the QML version draws these screens itself. While the numbers lived
/// as literals in `window.py`, the QML version kept its own copy — and the
/// «daemon unavailable» screen drifted from the original in spacing (measured 03.09).
pub const STATE_TEXT_FONT_SIZE: i32 = 15;

/// Font size of the emoji icon on the «Daemon unavailable» screen, px.
pub const NO_DAEMON_ICON_FONT_SIZE: i32 = 48;

/// Spacing between the icon, the text and the button of a start screen, px. In
/// the original this is the `QVBoxLayout` default under the product style (measured
/// 03.09 on a live object: 6); here it is written explicitly so that both versions
/// take one number rather than each its own default.
pub const STATE_COLUMN_SPACING: i32 = 6;

/// Margins of the start screen column, px. Same reasoning as for the spacing.
pub const STATE_COLUMN_MARGIN: i32 = 6;

/// Style of the icon on the «Daemon unavailable» screen — for the widget version.
pub fn no_daemon_icon_qss() -> String {
    format!("font-size: {NO_DAEMON_ICON_FONT_SIZE}px;")
}

/// Style of the text on the «Daemon unavailable» screen — for the widget version.
pub fn no_daemon_text_qss() -> String {
    format!("font-size: {STATE_TEXT_FONT_SIZE}px;")
}

/// Placeholder text when `Qcm.Material` is built for a different minor version
/// of Qt than the running one (`qml_modules::incompatible_qcm`). The tab does not load
/// such a build: Qt's private API is incompatible between releases, and
/// the pages crashed (see `qml_modules`, 09.10.2026).
pub fn text_qml_modules_wrong_qt(built: (u32, u32), running: (u32, u32)) -> String {
    crate::i18n::tr(
        "The Wallpaper Engine interface components were built for Qt {built}, \
but the system now runs Qt {running}. They are not loaded, so the tab does not crash.\n\n\
Update or reinstall Rexon so they are rebuilt for the new Qt version, then restart Rexon.",
    )
    .replace("{built}", &crate::i18n::isolate(&format!("{}.{}", built.0, built.1)))
    .replace("{running}", &crate::i18n::isolate(&format!("{}.{}", running.0, running.1)))
}

/// Placeholder text when the QML plugins are not built.
pub fn text_no_qml_modules() -> String {
    crate::i18n::tr("The interface QML modules are not built.\n\n\
Build them and restart Rexon:\n\
  bash tools/build_qml_material.sh\n\
  bash tools/build_qml_control.sh")
}

// ── Failure screen: words for the buyer, details — to the log ───────────────
//
// ⚠️ This was the case until 18.09.2026: the failure reason was shown to the buyer VERBATIM,
// as the daemon client composed it. A person who bought the program for wallpapers
// read across the whole screen «Expected a bundled AppImage (rexon/vendor/),
// ~/.local/share/waywallen/waywallen.AppImage, in PATH or ~/.local/bin/
// waywallen» — four paths, the word PATH and not a single hint at what they
// should actually do (rule #13). Now the raw string goes to the log, and the
// screen shows what the buyer can UNDERSTAND and DO.
//
// The foreign string has to be parsed because the reason has only one channel — it
// goes through the actions object's signal and the markup, where there are no types.
// So that parsing does not drift from the messages themselves, recognition goes by the
// ORIGINAL functions of [`crate::we_client`], not by copies of their text rewritten here,
// and every recognition is pinned by a test in `we_window_parity.rs`.

/// Why the tab shows the failure screen — in the buyer's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonFailure {
    /// The engine is not on disk at all.
    NotInstalled,
    /// The engine file exists, but starting it failed.
    LaunchFailed,
    /// Started it and did not get readiness in time.
    StartTimeout,
    /// Started, but stays silent on requests.
    NotResponding,
    /// Answered, but not ready to work (old version, foreign database).
    NotReady,
    /// The reason is not recognized — generic words, details in the log.
    Unknown,
}

/// Start of the reason composed by [`health_verdict`]: by it a failure is
/// recognized as «answered, but not ready». There is no copy of the text below —
/// the message is built from this very constant.
const REASON_NOT_READY_HEAD: &str = "движок ответил, но не готов";

/// Reason for the log when `health` did not answer at all.
///
/// ⚠️ This is a RAW reason (the log writes it and [`daemon_failure`] reads it), not
/// a human-facing label: the label is composed by [`no_daemon_text`].
pub const TEXT_HEALTH_TIMEOUT: &str = "движок подключился, но не отвечает на \
проверку готовности (health)";

/// Reason for the log when the overall wait limit expired
/// ([`CONNECT_TIMEOUT_MS`]).
pub const REASON_CONNECT_TIMEOUT: &str = "движок не вышел на связь за \
отведённое время: сокет не открылся либо соединение рвётся раз за разом";

/// Recognize the failure reason from the daemon client's raw string.
pub fn daemon_failure(reason: &str) -> DaemonFailure {
    let reason = reason.trim();
    if reason.is_empty() {
        return DaemonFailure::Unknown;
    }
    if reason == crate::we_client::daemon_not_found_message() {
        return DaemonFailure::NotInstalled;
    }
    if reason == crate::we_client::daemon_poll_timeout_message()
        || reason.starts_with(crate::we_client::START_TIMEOUT_HEAD)
    {
        return DaemonFailure::StartTimeout;
    }
    // The engine process died without coming online: for a person this is the same
    // «the engine does not start» as a failed launch of the file.
    if reason == crate::we_client::daemon_exited_message() {
        return DaemonFailure::LaunchFailed;
    }
    // The failed-launch message carries the binary path — compare the beginning
    // built by the same function, not one rewritten here by hand.
    let launch_head = crate::we_client::daemon_launch_failed_message("");
    let launch_head = launch_head.trim_end();
    if reason.starts_with(launch_head) {
        return DaemonFailure::LaunchFailed;
    }
    if reason == TEXT_HEALTH_TIMEOUT || reason == REASON_CONNECT_TIMEOUT {
        return DaemonFailure::NotResponding;
    }
    if reason.starts_with(REASON_NOT_READY_HEAD) {
        return DaemonFailure::NotReady;
    }
    DaemonFailure::Unknown
}

/// Failure screen text — what the buyer reads (Rexon not from a package).
///
/// The form for a Rexon copy installed not by a distribution package; the product
/// screen uses [`no_daemon_text_in`] with the real installation flag.
pub fn no_daemon_text(reason: &str) -> String {
    no_daemon_text_in(reason, false)
}

/// Failure screen text — what the buyer reads.
///
/// Each case answers their two questions: what happened and what to
/// do. There are no machine details here in any variant — they go
/// as the [`daemon_failure_log_line`] string to the Rexon log.
///
/// `packaged` — Rexon is installed by a distribution package, i.e. the wallpaper engine
/// came along with it — since 30.10.2026 as a separate dependency package
/// `waywallen` (`we_client::SYSTEM_DAEMON`). Only then is the advice
/// «reinstall the waywallen package» true: reinstalling brings back the engine file.
///
/// ⚠️ Until 28.09.2026 the «engine not installed» screen told EVERYONE «it is installed
/// together with Rexon — reinstall Rexon», although the package did not contain the engine
/// at all, and reinstalling changed nothing. And «not ready to work» claimed
/// that the engine was «outdated», although the screen does not know the reason (the `health` state).
/// Now every piece of advice is doable and does not promise what will not happen.
pub fn no_daemon_text_in(reason: &str, packaged: bool) -> String {
    use crate::i18n::tr;
    match (daemon_failure(reason), packaged) {
        (DaemonFailure::NotInstalled, true) => tr(
            "The waywallen wallpaper engine is missing.\n\nThe Rexon package \
installs it as a separate system package, waywallen, but its file is not in \
place. Reinstall the waywallen package, and wallpapers will work.",
        ),
        (DaemonFailure::NotInstalled, false) => tr(
            "The waywallen wallpaper engine is not installed.\n\nThis copy of \
Rexon was installed without the engine. Install Rexon from its package — the \
engine comes with it — and wallpapers will work.",
        ),
        (DaemonFailure::LaunchFailed, true) => tr(
            "The waywallen wallpaper engine does not start.\n\nPress “Retry \
Start”. If that does not help, reinstall the waywallen package: the engine \
file may be damaged.",
        ),
        (DaemonFailure::LaunchFailed, false) => tr(
            "The waywallen wallpaper engine does not start.\n\nPress “Retry \
Start”. If that does not help, restart the computer; the details are in the \
Rexon log.",
        ),
        (DaemonFailure::StartTimeout, _) => tr(
            "The waywallen wallpaper engine is taking too long to start.\n\n\
Press “Retry Start”. If that does not help, restart the computer.",
        ),
        (DaemonFailure::NotResponding, _) => tr(
            "The waywallen wallpaper engine started but does not respond.\n\n\
Press “Retry Start”. If that does not help, restart the computer.",
        ),
        (DaemonFailure::NotReady, _) => tr(
            "The waywallen wallpaper engine started but reports that it is not \
ready to work.\n\nPress “Retry Start”. If that does not help, restart the \
computer; the details are in the Rexon log.",
        ),
        (DaemonFailure::Unknown, _) => tr(
            "The waywallen wallpaper engine did not start.\n\nPress “Retry \
Start”. If that does not help, restart the computer; the details are in the \
Rexon log.",
        ),
    }
}

/// Log line with the RAW failure reason — the very machine internals that
/// used to be on the screen.
///
/// An empty reason gives no line: writing «failure without a reason» to the log
/// means cluttering it on every opening of the tab without an engine.
pub fn daemon_failure_log_line(reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return None;
    }
    Some(format!(
        "rexon: вкладка Wallpaper Engine, движок не поднялся — {reason}"
    ))
}

/// Verdict on the `health` response of a connected daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthVerdict {
    /// Show the pages (the daemon is healthy or old, without the `health` field).
    Ready,
    /// Show the failure screen with this RAW reason.
    NotReady(String),
}

/// Parsing the `health` response: an empty state is an old daemon, we trust it.
///
/// A WS connection by itself does not mean yet that the daemon works (it might have crashed
/// on a DB migration) — without this check the buyer gets a black screen instead of wallpapers.
pub fn health_verdict(state: &str) -> HealthVerdict {
    let state = state.trim().to_lowercase();
    if state.is_empty() || state == "healthy" {
        return HealthVerdict::Ready;
    }
    HealthVerdict::NotReady(format!("{REASON_NOT_READY_HEAD} (состояние: {state})"))
}

// ============================================================================
// The «Developer» page
// ============================================================================

/// Setting key «auto-INSTALL updates» (off by default).
pub const KEY_AUTO_DOWNLOAD: &str = "dev_engine_auto_download";
/// Setting key «auto-CHECK upstream on start» (on).
pub const KEY_AUTO_UPSTREAM: &str = "dev_auto_upstream_check";
/// Setting key «auto parity check on opening the page» (on).
pub const KEY_AUTO_PARITY: &str = "dev_auto_parity_check";

/// Default value for [`KEY_AUTO_DOWNLOAD`].
pub const DEFAULT_AUTO_DOWNLOAD: bool = false;
/// Default value for [`KEY_AUTO_UPSTREAM`].
pub const DEFAULT_AUTO_UPSTREAM: bool = true;
/// Default value for [`KEY_AUTO_PARITY`].
pub const DEFAULT_AUTO_PARITY: bool = true;

/// Dev page title.
pub fn dev_title() -> String {
    crate::i18n::tr("Developer")
}
/// Dev page title style.
pub const DEV_TITLE_QSS: &str = "font-size: 18px; font-weight: 600;";
/// Dev page subtitle.
pub fn dev_subtitle() -> String {
    crate::i18n::tr("Hidden page (REXON_DEV=1). Not available to buyers in the Steam build.")
}
/// Dev page section title style.
pub const DEV_SECTION_QSS: &str = "font-size: 15px; font-weight: 500;";
/// Style of the monospace panels (changelog, parity report).
pub const DEV_MONO_QSS: &str = "font-family: monospace; font-size: 12px;";

// ── Dev page numbers ────────────────────────────────────────────────────
//
// ⚠️ The same numbers that appear in the style sheets and layouts above, but
// by name. Same reasoning as for the rail and start screen numbers: the page
// is drawn by TWO versions of the tab, and the QML scene does not understand Qt style sheets —
// while a number lives only inside the string `"font-size: 18px; …"`, the QML
// markup has nowhere to ask for it, and it gets its own copy that silently
// drifts. The check «the style string is built from these numbers» is held by the test
// `rust/rexon-core/tests/we_window_parity.rs`.

/// Dev page title font size, px.
pub const DEV_TITLE_FONT_SIZE: i32 = 18;
/// Dev page title weight.
pub const DEV_TITLE_FONT_WEIGHT: i32 = 600;
/// Section title font size, px.
pub const DEV_SECTION_FONT_SIZE: i32 = 15;
/// Section title weight.
pub const DEV_SECTION_FONT_WEIGHT: i32 = 500;
/// Font size of the monospace panels (changelog, parity report), px.
pub const DEV_MONO_FONT_SIZE: i32 = 12;
/// Font size of muted labels (subtitle, operation progress line), px.
pub const DEV_MUTED_FONT_SIZE: i32 = 12;
/// Font size of the target status lines and the parity badge, px.
pub const DEV_STATUS_FONT_SIZE: i32 = 13;
/// Weight of the label above the changelog panel.
pub const DEV_CHANGELOG_LABEL_WEIGHT: i32 = 500;
/// Page margins, px (`root.setContentsMargins(12, 12, 12, 12)`).
pub const DEV_PAGE_MARGIN: i32 = 12;
/// Spacing between page sections, px.
pub const DEV_COLUMN_SPACING: i32 = 12;
/// Section panel margins, px (`sv.setContentsMargins(16, 16, 16, 16)`).
pub const DEV_PANE_PADDING: i32 = 16;
/// Spacing inside a section panel, px.
pub const DEV_PANE_SPACING: i32 = 8;
/// Spacing in a button row, px. In the widget version this is the
/// `QHBoxLayout` default under the product style — the same number as the spacing
/// of the start screens; here it is written explicitly so that both versions take
/// one number rather than each its own default.
pub const DEV_ROW_SPACING: i32 = 6;
/// Minimum height of the changelog panel, px.
pub const DEV_CHANGELOG_MIN_HEIGHT: i32 = 120;
/// Maximum height of the changelog panel, px.
pub const DEV_CHANGELOG_MAX_HEIGHT: i32 = 260;

/// Label of the confirm button in the page's questions.
///
/// In the widget version Qt sets it itself (`QMessageBox.StandardButton.Yes`);
/// the QML scene draws the question buttons itself, and it needs the label as a word.
pub fn dialog_yes() -> String {
    crate::i18n::tr("Yes")
}
/// Label of the decline button in the page's questions.
pub fn dialog_no() -> String {
    crate::i18n::tr("No")
}

/// Style of a muted label (subtitle, operation status line).
pub fn muted_qss(variant_color: &str, size_px: i32) -> String {
    format!("font-size: {size_px}px; color: {variant_color};")
}

/// Title of the upstream updates section.
pub fn section_upstream() -> String {
    crate::i18n::tr("Upstream Updates")
}
/// Title of the parity report section.
pub fn section_parity() -> String {
    crate::i18n::tr("UI Parity Report")
}

/// Label of the upstream auto-check checkbox.
pub fn cb_auto_upstream() -> String {
    crate::i18n::tr("Check for upstream updates on startup (network)")
}
/// Label of the auto-install checkbox.
pub fn cb_auto_download() -> String {
    crate::i18n::tr("Install waywallen and OWE plugin updates automatically (after validation, with .bak)")
}
/// Label of the parity auto-check checkbox.
pub fn cb_auto_parity() -> String {
    crate::i18n::tr("Check UI parity when the page opens")
}

/// Label of the «Check now» button.
pub fn btn_check() -> String {
    crate::i18n::tr("Check Now")
}
/// Label of the engine install button.
pub fn btn_update() -> String {
    crate::i18n::tr("Download and Install waywallen")
}
/// Label of the OWE plugin install button.
pub fn btn_owe() -> String {
    crate::i18n::tr("Download and Install OWE Plugin")
}
/// Label of the «mark as reviewed» button.
pub fn btn_seen() -> String {
    crate::i18n::tr("OWE: Mark as Reviewed")
}
/// Label of the changelog copy button.
pub fn btn_copy() -> String {
    crate::i18n::tr("Copy All")
}
/// Label of the parity check button.
pub fn btn_parity() -> String {
    crate::i18n::tr("Compare with Engine")
}
/// Label above the changelog panel.
pub fn label_changelog() -> String {
    crate::i18n::tr("What's changed:")
}
/// Placeholder of the empty changelog panel.
pub fn placeholder_changelog() -> String {
    crate::i18n::tr("The list of releases with release notes will appear after the check.")
}
/// Initial text of the parity report panel.
pub fn placeholder_parity() -> String {
    crate::i18n::tr("Press “Compare with Engine” or open the page.")
}

/// Targets for which the dev page shows a separate status line:
/// `(target key, displayed name)`.
pub const STATUS_TARGETS: [(&str, &str); 2] =
    [("waywallen", "waywallen"), ("owe", "open-wallpaper-engine")];

/// Engine version label.
///
/// ⚠️ Fix 3: before the first check the original wrote «Engine version: —», and
/// after it — «waywallen version: …». One field must not have two
/// names; here there is one name, and an empty version stays an honest dash.
pub fn version_label(version: &str) -> String {
    if version.trim().is_empty() {
        crate::i18n::tr("waywallen version: (unknown)")
    } else {
        crate::i18n::trf("waywallen version: %1", &[&version])
    }
}

/// The same label before the first check (the version has not been asked yet).
pub fn version_label_initial() -> String {
    crate::i18n::trf("waywallen version: %1", &[&"—"])
}

/// Status line style before the first check — no colour yet.
pub const STATUS_NEUTRAL_QSS: &str = "font-size: 13px;";

/// Initial target status line.
pub fn target_status_initial_parts(display_name: &str) -> StatusLine {
    StatusLine::new(crate::i18n::trf("%1: not checked yet", &[&display_name]), "", false)
}

/// The same with a ready style sheet — for the widget version.
pub fn target_status_initial(display_name: &str) -> Styled {
    target_status_initial_parts(display_name).to_styled()
}

/// Target status line by the check result.
///
/// `ok=false` — the target was not checked (network/rate limit), red without a mark.
/// Otherwise «⬆️» when an update is available (orange) or «✅» (green).
pub fn target_status_parts(
    ok: bool,
    title: &str,
    note: &str,
    update_available: bool,
) -> StatusLine {
    if !ok {
        return StatusLine::new(format!("{title}: {note}"), COLOR_ERR, false);
    }
    let mark = if update_available { "⬆️" } else { "✅" };
    let color = if update_available { COLOR_WARN } else { COLOR_OK };
    StatusLine::new(format!("{mark} {title}: {note}"), color, false)
}

/// The same with a ready style sheet — for the widget version.
pub fn target_status_line(ok: bool, title: &str, note: &str, update_available: bool) -> Styled {
    target_status_parts(ok, title, note, update_available).to_styled()
}

/// waywallen status line after a successful install.
pub fn waywallen_installed_parts() -> StatusLine {
    StatusLine::new(
        crate::i18n::tr("✅ waywallen (interface app): updated, daemon restart required"),
        COLOR_OK,
        false,
    )
}

/// The same with a ready style sheet — for the widget version.
pub fn waywallen_installed_line() -> Styled {
    waywallen_installed_parts().to_styled()
}

/// OWE status line after a successful plugin install.
pub fn owe_installed_parts(version: &str) -> StatusLine {
    let v = if version.trim().is_empty() { "?" } else { version };
    StatusLine::new(
        crate::i18n::trf("✅ open-wallpaper-engine (renderer engine): plugin %1 installed", &[&v]),
        COLOR_OK,
        false,
    )
}

/// The same with a ready style sheet — for the widget version.
pub fn owe_installed_line(version: &str) -> Styled {
    owe_installed_parts(version).to_styled()
}

/// OWE status line after «mark as reviewed».
pub fn owe_seen_parts(title: &str, version: &str) -> StatusLine {
    StatusLine::new(
        crate::i18n::trf("✅ %1: %2 marked as reviewed", &[&title, &version]),
        COLOR_OK,
        false,
    )
}

/// The same with a ready style sheet — for the widget version.
pub fn owe_seen_line(title: &str, version: &str) -> Styled {
    owe_seen_parts(title, version).to_styled()
}

/// Status «upstream check in progress».
pub fn text_check_running() -> String {
    crate::i18n::tr("Checking upstream…")
}
/// Status «check finished».
pub fn text_check_done() -> String {
    crate::i18n::tr("Check complete.")
}
/// Status «check failed» (network/rate limit — no crashes).
pub fn text_check_failed() -> String {
    crate::i18n::tr("Could not check for updates (network / GitHub rate limit).")
}
/// Status after copying the changelog.
pub fn text_changelog_copied() -> String {
    crate::i18n::tr("Changelog copied to the clipboard.")
}
/// Changelog panel when there are no changes.
pub fn text_changelog_empty() -> String {
    crate::i18n::tr("No changes relative to our version.")
}

/// What to show in the changelog panel: the assembled text or an honest «no changes».
pub fn changelog_panel_text(rendered: &str) -> String {
    if rendered.is_empty() {
        text_changelog_empty()
    } else {
        rendered.to_string()
    }
}

/// Confirmation of installing a new engine.
pub fn confirm_update_text(latest: &str, asset_url: &str) -> String {
    let v = if latest.trim().is_empty() { "?" } else { latest };
    crate::i18n::trf(
        "A new engine version is out (%1).\nDownload and install it?\n\nSource:\n%2",
        &[&v, &asset_url],
    )
}

/// Title of the engine install confirmation window.
pub fn confirm_update_title() -> String {
    crate::i18n::tr("Engine Update")
}

/// Confirmation of installing the OWE plugin.
pub fn confirm_owe_text(latest: &str, asset_url: &str) -> String {
    let v = if latest.trim().is_empty() { "?" } else { latest };
    crate::i18n::trf(
        "Install open-wallpaper-engine %1 as a waywallen plugin?\n\nSource:\n%2",
        &[&v, &asset_url],
    )
}

/// Title of the OWE plugin install confirmation window.
pub fn confirm_owe_title() -> String {
    crate::i18n::tr("OWE Plugin Installation")
}

/// Result line of the engine install.
pub fn install_done_text(ok: bool, message: &str) -> String {
    if ok {
        crate::i18n::trf("Installed: %1 Restart the daemon to apply.", &[&message])
    } else {
        crate::i18n::trf("Error: %1", &[&message])
    }
}

/// Result line of the OWE plugin install.
pub fn owe_done_text(ok: bool, message: &str) -> String {
    if ok {
        crate::i18n::trf("OWE plugin installed: %1", &[&message])
    } else {
        crate::i18n::trf("OWE plugin error: %1", &[&message])
    }
}

/// Refusal line of the waywallen ≥ 0.2.3 compatibility gate.
pub fn owe_gate_refused_text(why: &str) -> String {
    crate::i18n::trf("OWE plugin not installed: %1", &[&why])
}

/// Status «handed the zip to the daemon».
pub fn text_owe_ipc_running() -> String {
    crate::i18n::tr("OWE plugin: installing through the daemon…")
}

/// Message about a successful install through the daemon.
pub fn owe_ipc_success_text(plugin_id: &str, needs_restart: bool) -> String {
    // Two whole sentences, not «…through the daemon.» glued to a tail: the translator
    // has their own word order.
    if needs_restart {
        crate::i18n::trf(
            "plugin %1 installed through the daemon. Daemon restart required.",
            &[&plugin_id],
        )
    } else {
        crate::i18n::trf(
            "plugin %1 installed through the daemon. The daemon restarted the renderers (hot-install).",
            &[&plugin_id],
        )
    }
}

/// Daemon refusal message when it sent no text of its own.
pub fn text_owe_ipc_refused() -> String {
    crate::i18n::tr("the daemon rejected the plugin installation")
}

/// Message of an expired IPC install timeout.
pub fn text_owe_ipc_timeout() -> String {
    crate::i18n::tr("the daemon did not answer the plugin installation within 60 seconds — check its log and try again")
}

/// Whether the IPC install timeout really fired.
///
/// ⚠️ Fix 2: the original's one-shot timer did not know which attempt it
/// was guarding. The daemon's answer to the first install came quickly, while the timer lived
/// up to a minute more — and the next install started in that window got
/// someone else's alarm: it deleted its temporary zip and reported a false «the daemon
/// did not answer within 60 seconds». Now an attempt has a number, and the timeout stays silent
/// if it is not from the current one.
pub fn owe_ipc_timeout_fires(armed_attempt: u64, current_attempt: u64, waiting: bool) -> bool {
    waiting && armed_attempt == current_attempt
}

// ============================================================================
// Parity check
// ============================================================================

/// Panel text when the daemon is not connected.
pub fn text_parity_no_daemon() -> String {
    crate::i18n::tr("Daemon not connected.")
}

/// Panel text when the engine answered with an error without its own message.
pub fn text_parity_request_failed() -> String {
    crate::i18n::tr("Schema request to the engine failed")
}

/// Report header: the live daemon version versus the tag of our proto snapshot.
pub fn parity_report_header(daemon_version: &str, proto_tag: &str) -> String {
    let dv = if daemon_version.trim().is_empty() {
        crate::i18n::tr("(version unknown)")
    } else {
        daemon_version.to_string()
    };
    let tag = if proto_tag.trim().is_empty() {
        crate::i18n::tr("(tag not recorded)")
    } else {
        proto_tag.to_string()
    };
    format!(
        "{}\n",
        crate::i18n::trf("Daemon: %1 · Rexon proto snapshot: %2", &[&dv, &tag])
    )
}

/// Parity check badge.
///
/// `proto_match`: `Some(false)` — the daemon version diverged from our protocol
/// snapshot (then «✓» would be misleading and is not shown at all),
/// `None` — the daemon version could not be found out, `Some(true)` — they match.
/// The badge covers ONLY the renderer settings schema, not the whole UI parity.
/// `unchecked_count` — how many settings had nothing to be checked against (the engine has no
/// renderer of the needed type): «✓» stays, but with a caveat and the warning colour.
pub fn parity_badge_parts(
    proto_match: Option<bool>,
    daemon_version: &str,
    proto_tag: &str,
    has_drift: bool,
    issue_count: usize,
    warning_count: usize,
    unchecked_count: usize,
) -> StatusLine {
    if proto_match == Some(false) {
        let dv = if daemon_version.trim().is_empty() {
            "?"
        } else {
            daemon_version
        };
        let tag = if proto_tag.trim().is_empty() { "?" } else { proto_tag };
        return StatusLine::new(
            crate::i18n::trf(
                "⚠️ daemon %1 ≠ protocol snapshot %2 — proto-resync needed",
                &[&dv, &tag],
            ),
            COLOR_WARN,
            true,
        );
    }
    if !has_drift {
        let text = if proto_match == Some(true) {
            crate::i18n::tr("renderer settings schema")
        } else {
            crate::i18n::tr("renderer settings schema (could not compare the daemon version)")
        };
        // The icon is chosen by state: a coloured glyph does not obey the text colour, so
        // the «not checked» caveat is carried by the icon itself, not only by the orange text.
        if unchecked_count > 0 {
            let note = crate::i18n::trf("not checked: %1", &[&unchecked_count]);
            return StatusLine::new(format!("⚠️ {text} · {note}"), COLOR_WARN, false);
        }
        return StatusLine::new(
            format!("✅ {text}"),
            COLOR_OK,
            false,
        );
    }
    let text = if warning_count > 0 {
        crate::i18n::trf("⚠️ mismatches: %1 (%2 important)", &[&issue_count, &warning_count])
    } else {
        crate::i18n::trf("⚠️ mismatches: %1", &[&issue_count])
    };
    StatusLine::new(
        text,
        COLOR_ERR,
        true,
    )
}

/// The same badge with a ready style sheet — for the widget version.
pub fn parity_badge(
    proto_match: Option<bool>,
    daemon_version: &str,
    proto_tag: &str,
    has_drift: bool,
    issue_count: usize,
    warning_count: usize,
    unchecked_count: usize,
) -> Styled {
    parity_badge_parts(
        proto_match,
        daemon_version,
        proto_tag,
        has_drift,
        issue_count,
        warning_count,
        unchecked_count,
    )
    .to_styled()
}

/// Whether to show the mark on the «Dev» button: a schema divergence OR
/// a diverged protocol snapshot.
pub fn parity_drift_flag(has_drift: bool, proto_match: Option<bool>) -> bool {
    has_drift || proto_match == Some(false)
}

// ============================================================================
// Network workers: step-by-step verdicts
// ============================================================================

/// How a worker step ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerOutcome {
    /// Success or failure (goes into the `done` signal).
    pub ok: bool,
    /// Message for the status line.
    pub message: String,
    /// Whether to delete the downloaded temporary file (see fix 1).
    pub remove_tmp: bool,
    /// Whether to hand the temporary file path to the GUI thread (IPC plugin install).
    pub hand_over_tmp: bool,
}

impl WorkerOutcome {
    fn fail(message: impl Into<String>, remove_tmp: bool) -> Self {
        Self {
            ok: false,
            message: message.into(),
            remove_tmp,
            hand_over_tmp: false,
        }
    }
}

/// Download progress message: `prefix` is empty for the engine, [`prefix_owe`]
/// for the plugin. A zero `total` — the server did not tell the size, progress stays silent.
pub fn download_progress(prefix: &str, done_bytes: u64, total_bytes: u64) -> Option<String> {
    if total_bytes == 0 {
        return None;
    }
    Some(format!(
        "{prefix}{}",
        crate::i18n::trf(
            "Downloading… %1 / %2 MiB",
            &[&(done_bytes >> 20), &(total_bytes >> 20)]
        )
    ))
}

/// Message prefix of the engine worker (there is none).
pub const PREFIX_ENGINE: &str = "";
/// Message prefix of the OWE plugin worker.
pub fn prefix_owe() -> String {
    crate::i18n::tr("OWE plugin: ")
}

/// Worker start message («Downloading…» / «OWE plugin: downloading…»).
pub fn download_start_text(prefix: &str) -> String {
    if prefix.is_empty() {
        crate::i18n::tr("Downloading…")
    } else {
        format!("{prefix}{}", crate::i18n::tr("downloading…"))
    }
}

/// Message of the engine validation step.
pub fn text_validate_appimage() -> String {
    crate::i18n::tr("Checking the new AppImage…")
}
/// Message of the plugin zip validation step.
pub fn text_validate_plugin() -> String {
    crate::i18n::tr("OWE plugin: checking the archive…")
}
/// Message of the plugin file-system install step (daemon not running).
pub fn text_install_plugin_fs() -> String {
    crate::i18n::tr("OWE plugin: installing (daemon not running)…")
}

/// Cancel message on application exit.
pub fn text_cancelled_on_quit() -> String {
    crate::i18n::tr("cancelled (the application is closing)")
}

/// Verdict after the download step.
///
/// `tmp_ready` — the file is received; `interrupted` — asked to exit the application.
/// `reason` — the executor's exception text, if there was one: without it a person
/// saw «failed/cancelled» and could not tell a network drop from a core import
/// error (live case 09.09.2026: the `rexon_py` wheel was rebuilt
/// under a running process, a lazy import failed, and the reason was eaten by
/// `except Exception` — two failures in a row were left without a diagnosis).
/// `None` — go on.
pub fn after_download(
    prefix: &str,
    tmp_ready: bool,
    interrupted: bool,
    reason: Option<&str>,
) -> Option<WorkerOutcome> {
    if !tmp_ready {
        let head = if prefix.is_empty() {
            crate::i18n::tr("download failed")
        } else {
            crate::i18n::tr("OWE plugin download failed")
        };
        let what = match reason.map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => format!("{head}: {r}"),
            None => crate::i18n::trf("%1/cancelled", &[&head]),
        };
        // no file — nothing to delete
        return Some(WorkerOutcome::fail(what, false));
    }
    if interrupted {
        return Some(WorkerOutcome::fail(text_cancelled_on_quit(), true));
    }
    None
}

/// Verdict after the validation step. `None` — go on.
pub fn after_validate(is_plugin: bool, ok: bool, message: &str) -> Option<WorkerOutcome> {
    if ok {
        return None;
    }
    let text = if is_plugin {
        crate::i18n::trf("plugin zip is invalid: %1", &[&message])
    } else {
        crate::i18n::trf("AppImage is invalid: %1", &[&message])
    };
    Some(WorkerOutcome::fail(text, true))
}

/// Verdict after the AppImage install.
///
/// ⚠️ Fix 1: `remove_tmp` is set ALWAYS. The original removed the downloaded
/// temporary file only on cancel and on failed validation; if the install itself failed —
/// ~200 MiB stayed lying around, because `install_appimage`
/// calls `os.remove(new_path)` only on the success branch. On a machine where
/// `/tmp` is mounted in RAM, this is a RAM leak until reboot.
/// On the success branch the file is already gone, removing it again is harmless.
pub fn after_install_appimage(ok: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok,
        message: message.to_string(),
        remove_tmp: true,
        hand_over_tmp: false,
    }
}

/// Plugin worker verdict after validation: hand the zip to the GUI thread for an
/// IPC install or install it ourselves into the file system.
///
/// A QWebSocket must not be touched from a foreign thread, so the GUI thread does IPC;
/// the worker only delivers the path.
pub fn after_validate_plugin_route(fs_install: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok: true,
        message: message.to_string(),
        remove_tmp: false,
        hand_over_tmp: !fs_install,
    }
}

/// Verdict after the plugin file-system install: the temporary zip is always removed.
pub fn after_install_plugin_fs(ok: bool, message: &str) -> WorkerOutcome {
    WorkerOutcome {
        ok,
        message: message.to_string(),
        remove_tmp: true,
        hand_over_tmp: false,
    }
}

// ============================================================================
// GUI thread decisions
// ============================================================================

/// Whether to install an update automatically: there is something to install, the checkbox is on and
/// it has not been done in this session yet (the checkbox default is off).
pub fn should_auto_install(pending: bool, auto_enabled: bool, already_done: bool) -> bool {
    pending && auto_enabled && !already_done
}

/// OWE plugin install mode: `true` — unpacking into the file system
/// (daemon not connected), `false` — hot install through the daemon's IPC.
pub fn plugin_fs_install(daemon_open: bool) -> bool {
    !daemon_open
}
