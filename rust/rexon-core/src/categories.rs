//! Appearance catalog categories — port of `rexon/appearance/categories.py`.
//!
//! cat_id → type → install path → apply command → requires_root. The data
//! (real KDE Store OCS categories + virtual/local ones) and the sidebar order
//! are carried over one to one; the correctness of the table transcription is
//! checked by a byte-for-byte comparison of an `asdict` dump with Python itself
//! (the golden test `categories_parity.rs`).
//!
//! In Python the `install_dir` paths are expanded by `os.path.expanduser` at
//! import time (from `$HOME`). Here the home is passed as a parameter — so the functions
//! stay pure and are tested with a fixed `$HOME` (just as `settings.rs`
//! takes the `command_exists` predicate as a parameter).

use std::path::Path;

use crate::i18n::{tr, trc};
use crate::models::{CatalogSource, ThemeCategory};

/// Like `os.path.expanduser`: replaces a leading `~` with the home directory.
/// A path without `~` (an absolute `/usr/share/...` or empty) is returned as is.
fn exp(home: &Path, p: &str) -> String {
    if p == "~" {
        return home.to_string_lossy().into_owned();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home.join(rest).to_string_lossy().into_owned();
    }
    p.to_string()
}

/// A helper constructing a real category with install_dir/apply_kind.
fn cat(id: i64, key: &str, name: String, icon: &str, install_dir: String, apply_kind: &str) -> ThemeCategory {
    ThemeCategory {
        id,
        key: key.to_string(),
        name,
        icon: icon.to_string(),
        install_dir,
        apply_kind: apply_kind.to_string(),
        requires_root: false,
        virtual_: false,
        store: CatalogSource::Ocs(vec![id]),
        default_sort: "new".to_string(),
    }
}

/// Real KDE Store categories in the declaration order of the Python dict
/// `CATEGORIES` (matters for the check — the key order is visible in the `asdict` dump).
///
/// `home` — the home directory for expanding `~` in `install_dir`.
///
/// EVERY real category has its own `apply_kind`, matching its key.
/// The shared `"none"` catch-all is gone: while wallpapers, fonts, sounds and widgets
/// shared one kind, the installed log could not tell a sound theme from
/// a wallpaper pack (both were written with the `wallpaper-pack` type), and the card could not
/// show "applied" for either. Wehrwolfmann 2026-08-22.
pub fn categories(home: &Path) -> Vec<ThemeCategory> {
    let e = |p: &str| exp(home, p);
    vec![
        cat(722, "global", trc("appearance section", "Global Themes"), "💎", e("~/.local/share/plasma/look-and-feel"), "global"),
        cat(104, "plasma", trc("appearance section", "Plasma Styles"), "🖥️", e("~/.local/share/plasma/desktoptheme"), "plasma"),
        cat(112, "colors", trc("appearance section", "Color Schemes"), "🌈", e("~/.local/share/color-schemes"), "colors"),
        cat(114, "aurorae", trc("appearance section", "Window Decorations (Aurorae)"), "🪟", e("~/.local/share/aurorae/themes"), "aurorae"),
        cat(132, "icons", trc("appearance section", "Icons"), "📁", e("~/.local/share/icons"), "icons"),
        cat(107, "cursor", trc("appearance section", "Cursors"), "🖱️", e("~/.local/share/icons"), "cursor"),
        cat(123, "kvantum", "Kvantum".to_string(), "🔲", e("~/.config/Kvantum"), "kvantum"),
        cat(135, "gtk", trc("appearance section", "GTK Themes"), "🎨", e("~/.local/share/themes"), "gtk"),
        cat(299, "wallpapers", trc("appearance section", "HD Wallpapers"), "🖼️", e("~/.local/share/wallpapers"), "wallpapers"),
        cat(715, "wallpaper_plugins", trc("appearance section", "Wallpaper Plugins"), "🎞️", e("~/.local/share/plasma/wallpapers"), "wallpaper_plugins"),
        cat(103, "fonts", trc("appearance section", "Fonts"), "🔤", e("~/.local/share/fonts"), "fonts"),
        cat(316, "sounds", trc("appearance section", "Sounds"), "🔔", e("~/.local/share/sounds"), "sounds"),
        // "Widgets" — the umbrella 705 "Plasma 6 Extensions", as in Discover
        // (`plasmoids.knsrc`): applets 706 and subcategories 707–712 (calendars,
        // clocks, menus, monitoring, multimedia, weather). Items arrive with
        // the subcategory typeid — [`STORE_TYPE_ALIASES`] leads them to the section.
        cat(705, "plasmoids", trc("appearance section", "Widgets"), "🧊", e("~/.local/share/plasma/plasmoids"), "plasmoids"),
        ThemeCategory {
            requires_root: true,
            ..cat(101, "sddm", trc("appearance section", "SDDM (Login Screen)"), "🔐", "/usr/share/sddm/themes".to_string(), "sddm")
        },
        ThemeCategory {
            requires_root: true,
            ..cat(108, "plymouth", "Plymouth".to_string(), "⚡", "/usr/share/plymouth/themes".to_string(), "plymouth")
        },
        // "Application style": Qt styles and Kvantum themes in one grid, the store —
        // Kvantum themes (category 123, installed by its own record above into
        // `~/.config/Kvantum`). Ready Qt styles (Darkly, Lightly, Klassy)
        // have no online catalog — they are in the AUR (`aur::category_queries`).
        ThemeCategory {
            store: CatalogSource::Ocs(vec![123]),
            ..cat(901, "widgetstyle", trc("appearance section", "Application Style"), "🎛️", String::new(), "widgetstyle")
        },
        ThemeCategory {
            store: CatalogSource::Ocs(vec![716, 488]),
            ..cat(902, "splash", trc("appearance section", "KDE Splash Screen"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash")
        },
        ThemeCategory {
            // The number by mode (full themes 722 / wallpapers 299) is chosen by
            // `CatalogView::query_category_id`.
            store: CatalogSource::Ocs(vec![722]),
            ..cat(903, "lockscreen", trc("appearance section", "Lock Screen"), "🔒", e("~/.local/share/plasma/look-and-feel"), "lockscreen")
        },
        cat(904, "lockwallpaper", trc("appearance section", "Lock Screen Wallpaper"), "🔒", e("~/.local/share/wallpapers"), "lockwallpaper"),
        cat(716, "splash", trc("appearance section", "Plasma 6 Splashscreens"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash"),
        cat(488, "splash", trc("appearance section", "Plasma Splashscreens"), "🌅", e("~/.local/share/plasma/look-and-feel"), "splash"),
    ]
    .into_iter()
    // ── Desktop sections (04.10.2026) ──────────────────────────────────
    // What the `.rxn` installation used to turn on by itself got its own section and
    // its own button: "Apply" for themes, "Enable"/"Disable" for extensions,
    // "Start"/"Stop" for Conky (`crate::de_apply`). The install directory
    // is taken from the matrix, and so is the desktop in which the section is visible
    // (`category_available`).
    .chain(DE_SECTIONS.iter().map(|(id, key)| de_section(home, *id, key)))
    .collect()
}

/// Desktop sections: number and key (also the apply kind). A number above
/// [`STORE_MAX_ID`] is synthetic (after the local 901–905), not above —
/// a real KDE store category. Where a section takes its catalog from is not decided
/// by the number: that is [`de_section_store`]. The Python reference did not know these sections.
/// The section type is [`category_kind`], the label is [`de_section_name`]; a new
/// desktop section is added here, there and as a matrix row, nowhere else
/// (the disk scan and the button will pick it up by themselves).
///
/// The window switcher, KWin scripts and effects are store categories
/// "Kwin Switching Layouts / Kwin Scripts / KWin Effects Plasma 6" (the same
/// as in `kwinswitcher.knsrc`, `kwinscripts.knsrc`, `kwineffect.knsrc`).
/// Until 04.10.2026 they were the local 911–913 — the old numbers are led by
/// [`STORE_TYPE_ALIASES`].
pub const DE_SECTIONS: &[(i64, &str)] = &[
    (906, "xfwm4"),
    (907, "marco"),
    (908, "gnomeshell"),
    (909, "cinnamontheme"),
    (910, "lxqttheme"),
    // Openbox and labwc frames — the window managers of LXQt (and of the labwc session).
    (OPENBOX_SECTION_ID, OPENBOX_KEY),
    (721, "tabbox"),
    (720, "kwinscripts"),
    (719, "kwineffects"),
    (914, "gnomeext"),
    (915, "cinnamonapplets"),
    (916, "cinnamondesklets"),
    (917, "cinnamonext"),
    (918, "nemoactions"),
    // Conky is a real KDE Store category ("Conky", 124, over a
    // thousand sets): the section shows its catalog, and a widget is installed
    // with a button, not with commands in a terminal (Wehrwolfmann's request of 10.10.2026).
    // Before that the section was the local 919 — the old number is led by
    // [`STORE_TYPE_ALIASES`].
    (CONKY_SECTION_ID, "conky"),
];

/// KDE "Appearance" sections scanned as desktop sections:
/// category number and key (also the apply kind and the active-theme map key,
/// `installed_scan::active_map_from_sources`).
///
/// Whether a section has an installed grid with a "Get new…" button
/// is no longer decided by this list: any section with a scan and a
/// store has it (`appearance_page::has_installed_grid_for`). The section scan is the same
/// as for [`DE_SECTIONS`] (`appearance_page::local_scan_plan` → `"section"`,
/// `installed_scan::LiveScan::section`), the directories come from the type table
/// `installed_scan::types_for`.
pub const KDE_INSTALLED_SECTIONS: &[(i64, &str)] = &[
    (722, "global"),
    (104, "plasma"),
    (112, "colors"),
    (114, "aurorae"),
    (132, "icons"),
    (107, "cursor"),
    (135, "gtk"),
    (316, "sounds"),
    // The login screen (SDDM) and the boot splash (Plymouth) — like kcm sddm and
    // kcm plymouth: a grid of installed themes with the current one highlighted, not
    // a store with installed items only in the "System" overview. The themes live in
    // system directories; "Apply" and "Remove" go through the helper
    // running as root (`ApplyAction::SystemTheme`, `theme_removal`). With Plasma Login
    // Manager there is no SDDM section in the panel ([`sidebar_for`]), on SteamOS both
    // sections explain the refusal ([`steamos_note`]).
    (101, "sddm"),
    (108, "plymouth"),
];

/// The highest number of a real KDE store category. Everything above is
/// synthetic section numbers: they never go to the store, the section's
/// catalog is set by its `store` field.
pub const STORE_MAX_ID: i64 = 900;

/// Store item number (its `typeid`) → the number of the section where it lives,
/// when they differ.
///
/// An umbrella section queries the store with the parent category, while items
/// arrive with their subcategory number: "Widgets" asks for 705, while
/// a clock widget arrives with 708. Without this table such an item found no
/// section — no install kind (plasmoid), no button, no label.
/// Here are also previous section numbers, so that log and favorites records
/// made before the number change do not lose their section.
pub const STORE_TYPE_ALIASES: &[(i64, i64)] = &[
    // "Plasma 6 Extensions" subcategories: applets, calendars, clocks, menus,
    // monitoring, multimedia, weather. 706 is also the previous section number.
    (706, 705),
    (707, 705),
    (708, 705),
    (709, 705),
    (710, 705),
    (711, 705),
    (712, 705),
    // Local KWin section numbers until 04.10.2026.
    (911, 721),
    (912, 720),
    (913, 719),
    // The local Conky section number until 10.10.2026.
    (919, 124),
    // The store's "Cinnamon Themes" (cinnamon-look): until 24.10.2026 the Cinnamon
    // themes section showed this catalog, now it shows Cinnamon Spices. The log
    // records of those installs stay in their section.
    (133, 909),
];

/// The section number for an item number: an umbrella subcategory → the umbrella, a previous
/// section number → the current one, the rest — as is.
pub fn section_id_for(type_id: i64) -> i64 {
    STORE_TYPE_ALIASES
        .iter()
        .find(|(from, _)| *from == type_id)
        .map(|(_, to)| *to)
        .unwrap_or(type_id)
}

/// Whether this is a desktop section (by number).
pub fn is_de_section(id: i64) -> bool {
    DE_SECTIONS.iter().any(|(i, _)| *i == id)
}

/// The desktop section label in the interface language.
fn de_section_name(key: &str) -> String {
    match key {
        "xfwm4" => trc("appearance section", "Window Borders (XFCE)"),
        "marco" => trc("appearance section", "Window Borders (MATE)"),
        "gnomeshell" => trc("appearance section", "GNOME Shell Theme"),
        "cinnamontheme" => trc("appearance section", "Cinnamon Theme"),
        "lxqttheme" => trc("appearance section", "LXQt Theme"),
        OPENBOX_KEY => trc("appearance section", "Window Borders (Openbox)"),
        "tabbox" => trc("appearance section", "Window Switcher (Alt+Tab)"),
        "kwinscripts" => trc("appearance section", "KWin Scripts"),
        "kwineffects" => trc("appearance section", "KWin Effects"),
        "gnomeext" => trc("appearance section", "GNOME Extensions"),
        "cinnamonapplets" => trc("appearance section", "Cinnamon Applets"),
        "cinnamondesklets" => trc("appearance section", "Cinnamon Desklets"),
        "cinnamonext" => trc("appearance section", "Cinnamon Extensions"),
        "nemoactions" => trc("appearance section", "Nemo Actions"),
        "conky" => trc("appearance section", "Conky Widgets"),
        other => other.to_string(),
    }
}

/// A local desktop section: section key = apply kind, the type — from
/// [`category_kind`], the directory and icon — from the matrix and the type table.
fn de_section(home: &Path, id: i64, key: &str) -> ThemeCategory {
    let name = de_section_name(key);
    let kind = category_kind(key).expect("раздел окружения связан с типом в category_kind");
    let dir = crate::de_customization::row(kind)
        .map(|r| r.dest_path(&home.to_string_lossy()))
        .unwrap_or_default();
    ThemeCategory {
        store: de_section_store(key),
        ..cat(id, key, name, crate::constants::type_icon(kind).unwrap_or("🧩"), dir, key)
    }
}

/// The desktop section's catalog — explicitly, by key, not by number: the section
/// number is held by the installed log and favorites and cannot change, while
/// a section's catalog may live in the store under another number. No record —
/// no catalog, the section shows only the disk.
pub fn de_section_store(key: &str) -> CatalogSource {
    // The five Cinnamon sections — the Cinnamon Spices store (the same as in the Cinnamon
    // settings window): the catalog kind comes from the spices type table.
    if let Some(t) = crate::cinnamon_spices::type_for_section(key) {
        return CatalogSource::Spices(t.json.to_string());
    }
    // "GNOME extensions" — the extensions.gnome.org site (`crate::gnome_ego`):
    // the shell itself installs an extension, the KDE Store does not carry them.
    if crate::gnome_ego::has_section(key) {
        return CatalogSource::GnomeExtensions;
    }
    match key {
        // The same categories as in Discover (`kwinswitcher.knsrc`,
        // `kwinscripts.knsrc`, `kwineffect.knsrc`).
        "tabbox" => CatalogSource::Ocs(vec![721]),
        "kwinscripts" => CatalogSource::Ocs(vec![720]),
        "kwineffects" => CatalogSource::Ocs(vec![719]),
        "conky" => CatalogSource::Ocs(vec![CONKY_SECTION_ID]),
        // Desktop sections under synthetic numbers: pling categories
        // (xfce-look, gnome-look, cinnamon-look; the same OCS API).
        // "XFCE/XFWM4 Themes".
        "xfwm4" => CatalogSource::Ocs(vec![138]),
        // "Metacity Themes": Marco reads the same `metacity-1` format.
        "marco" => CatalogSource::Ocs(vec![125]),
        // «Gnome Shell Themes».
        "gnomeshell" => CatalogSource::Ocs(vec![134]),
        // «LXQt Themes».
        "lxqttheme" => CatalogSource::Ocs(vec![446]),
        // "Openbox Themes" (box-look): labwc reads the same format too.
        OPENBOX_KEY => CatalogSource::Ocs(vec![140]),
        _ => CatalogSource::None,
    }
}

/// A virtual sidebar category (no cat_id, no network).
fn virtual_cat(key: &str, name: String, icon: &str, default_sort: &str) -> ThemeCategory {
    ThemeCategory {
        id: 0,
        key: key.to_string(),
        name,
        icon: icon.to_string(),
        install_dir: String::new(),
        apply_kind: String::new(),
        requires_root: false,
        virtual_: true,
        // Disk overviews do not know the network; "Popular" and "New" are a combined
        // storefront of the desktop's most common categories.
        store: if LOCAL_VIEW_KEYS.contains(&key) { CatalogSource::None } else { CatalogSource::OcsShowcase },
        default_sort: default_sort.to_string(),
    }
}

/// Virtual sidebar categories in the order of the Python constants.
/// popular/new aggregate real categories with different sorting;
/// active/downloaded/system/favorites — a local disk overview without network.
pub fn popular() -> ThemeCategory {
    virtual_cat("popular", trc("appearance section", "Popular"), "⭐", "high")
}
pub fn new_() -> ThemeCategory {
    virtual_cat("new", trc("appearance section", "New"), "🆕", "new")
}
pub fn active() -> ThemeCategory {
    virtual_cat("active", trc("appearance section", "Active"), "✅", "new")
}
pub fn downloaded() -> ThemeCategory {
    virtual_cat("downloaded", trc("appearance section", "Downloaded"), "📥", "new")
}
pub fn system() -> ThemeCategory {
    virtual_cat("system", trc("appearance section", "System"), "🖥️", "new")
}
pub fn favorites() -> ThemeCategory {
    virtual_cat("favorites", trc("appearance section", "Favorites"), "⭐", "new")
}

/// Sidebar order: local overviews → the store.
///
/// `home` expands `~` in real categories. Virtual categories are
/// listed explicitly (not from the network). Real ones are taken from `categories(home)` by
/// cat_id — like `CATEGORIES[…]` in Python.
pub fn sidebar(home: &Path) -> Vec<ThemeCategory> {
    let all = categories(home);
    let pick = |id: i64| -> ThemeCategory {
        all.iter()
            .find(|c| c.id == id)
            .cloned()
            .expect("cat_id есть в categories()")
    };
    vec![
        active(),
        downloaded(),
        system(),
        favorites(),
        popular(),
        new_(),
        pick(722),
        pick(104),
        pick(112),
        pick(114),
        // Window title buttons, system fonts, icons with toolbars
        // in programs and launch feedback have no row of their own
        // (08.11.2026): they are the "⚙️ Settings" view of the window frame,
        // "Fonts", "Icons" and "Cursors" sections ([`SECTION_SETTINGS`]), like the tabs
        // of KDE 6.8 modules.
        pick(132),
        pick(107),
        pick(135),
        pick(901),
        pick(299),
        pick(715),
        pick(103),
        pick(316),
        pick(705),
        // Conky widgets — right under "Widgets": they are desktop widgets too,
        // and at the other end of the list people would look for them in vain (10.10.2026).
        pick(CONKY_SECTION_ID),
        pick(101),
        pick(902),
        pick(903),
        pick(108),
    ]
    .into_iter()
    // Desktop sections — after the shared ones; on each desktop only its own
    // are visible (`category_available`).
    .chain(DE_SECTIONS.iter().filter(|(id, _)| *id != CONKY_SECTION_ID).map(|(id, _)| pick(*id)))
    // "MATE panel" is not a theme catalog but ready panel layouts; it sits
    // among the MATE desktop sections.
    .chain(std::iter::once(mate_panel_category()))
    // "Animation" — the last "Appearance" section, as in KDE 6.8
    // (`kcm_animations`, weight 40 in the `appearance` category). "Desktop
    // effects" have no row of their own (Wehrwolfmann's decision of
    // 08.11.2026): it is the "⚙️ Settings" view of the "KWin effects" section
    // ([`SECTION_SETTINGS`]) — the catalog and enabling effects in one place.
    // (The row used to stand here: in KDE they are in "Window Management", and
    // Rexon has no such tab. "Screen effects" (02.11.2026) sits between
    // them: also "how the whole desktop looks", and in KDE closest to them.
    .chain(["animations", SCREEN_EFFECTS_KEY].into_iter().map(settings_panel))
    .collect()
}

// ── Settings sections in "Appearance" (01.11.2026) ─────────────────────

/// Sections that until 01.11.2026 lived in the "Settings" tab, while in KDE 6.8
/// they are in "Appearance & Style" (a remark from the parity reviews of 30–31.10):
/// number (synthetic, they have no store) and key. The contents are the same
/// sections of the "Settings" tab (`SettingsPage.qml`, `panel` mode), here
/// it is only their place in the sidebar. Where each one stands — [`sidebar`],
/// in which group — [`category_group`], where it is visible — [`category_available`].
pub const SETTINGS_PANELS: &[(i64, &str)] = &[
    (925, "animations"),
    // 926 — "Desktop effects", since 08.11.2026 the "⚙️ Settings" view
    // of the "KWin effects" section ([`DESKTOP_EFFECTS_PANEL`]).
    (928, SCREEN_EFFECTS_KEY),
];

/// The "Desktop effects" section (`DesktopEffectsSection`): enabling and
/// configuring effects already in KWin — the "⚙️ Settings" view
/// of the "KWin effects" section (the KDE Store catalog); it has had no row of its own in the sidebar
/// since 08.11.2026 (Wehrwolfmann's decision).
pub const DESKTOP_EFFECTS_PANEL: &str = "desktopeffects";
/// The "System fonts" section of the "Settings" tab (`SettingsPage.qml`,
/// `panel` mode) — the "⚙️ Settings" view of the "Fonts" section.
pub const FONT_SETTINGS_PANEL: &str = "fontsettings";
/// The "Icons and toolbars in programs" section (`AppWidgetsSection`)
/// — the "⚙️ Settings" view of the "Icons" section.
pub const APP_WIDGETS_PANEL: &str = "appwidgets";
/// The "Launch feedback" section (`LaunchFeedbackSection`) — the
/// "⚙️ Settings" view of the "Cursors" section, like the tab of the KDE "Cursors" module.
pub const LAUNCH_FEEDBACK_PANEL: &str = "launchfeedback";
/// The "Wallpaper blur" frame of the "Settings" tab (`blurGroup`) — the
/// "⚙️ Settings" view of the "HD wallpapers" section (08.11.2026); it is no longer
/// in the "Settings" tab.
pub const WALLPAPER_BLUR_PANEL: &str = "wallpaperblur";
/// The "Razer Axon" block (`AxonSettingsSection`): our display settings for wallpapers
/// from the Razer Axon library — the "⚙️ Settings" view of the "HD wallpapers" section
/// (14.11.2026), next to the "Razer Axon" source. Present only if Axon is
/// found: selected by `appearance_page::CatalogView::section_settings_panels`.
pub const AXON_SETTINGS_PANEL: &str = "axonsettings";
/// The "Wallpaper playlist" block (`WallpaperPlaylistSection`): one playlist for
/// all of Rexon's own wallpapers (video, web, Flash, pictures, Razer Axon) —
/// the "⚙️ Settings" view of the "HD wallpapers" section (15.11.2026), first.
pub const WALLPAPER_PLAYLIST_PANEL: &str = "wallpaperplaylist";
/// The "Ambient sounds" (`ambientGroup`) and "Event sounds"
/// (`soundThemeGroup`) frames of the "Settings" tab — the "⚙️ Settings" view of the
/// "Sounds" section (08.11.2026); they are no longer in the "Settings" tab.
pub const AMBIENT_SOUNDS_PANEL: &str = "ambientsounds";
pub const EVENT_SOUNDS_PANEL: &str = "eventsounds";
/// The "Animation" section (`AnimationsSection`). Where the window manager has no
/// window animations (`animations::has_window_animations`), it has no row, and
/// the "No animations" checkbox for programs is the "⚙️ Settings" view of the
/// "GTK themes" section.
pub const ANIMATIONS_KEY: &str = "animations";

/// Catalog sections whose settings of the same kind open with the
/// "⚙️ Settings" button in the header rather than with their own sidebar row (Wehrwolfmann's
/// decision of 08.11.2026): `(catalog section, settings section)`. The catalog and
/// settings of one kind are in one place, like the tabs of KDE modules
/// ("Window Decorations → Titlebar Buttons", "Cursors → Launch Feedback").
/// One mechanism for all pairs (`appearance_page::CatalogView::section_settings_panels`),
/// a new pair is a row here. A section may have several rows: in
/// "GTK themes" of a desktop without its own window frames — both the title buttons and the animation
/// checkbox. Numbers 921–924 were freed together with the rows.
pub const SECTION_SETTINGS: &[(&str, &str)] = &[
    ("fonts", FONT_SETTINGS_PANEL),
    ("icons", APP_WIDGETS_PANEL),
    ("cursor", LAUNCH_FEEDBACK_PANEL),
    ("wallpapers", WALLPAPER_PLAYLIST_PANEL),
    ("wallpapers", WALLPAPER_BLUR_PANEL),
    ("wallpapers", AXON_SETTINGS_PANEL),
    ("sounds", EVENT_SOUNDS_PANEL),
    ("sounds", AMBIENT_SOUNDS_PANEL),
    ("kwineffects", DESKTOP_EFFECTS_PANEL),
    // Title buttons — with this desktop's window frames
    // ([`WINDOW_BORDER_SECTIONS`]); for a desktop without a frames section (GNOME,
    // Budgie, Pantheon, COSMIC, Deepin, tiling ones) — in "GTK themes": there
    // the titles are drawn by the GTK theme or by the GTK programs themselves.
    ("aurorae", WINDOW_BUTTONS_KEY),
    ("xfwm4", WINDOW_BUTTONS_KEY),
    ("marco", WINDOW_BUTTONS_KEY),
    ("cinnamontheme", WINDOW_BUTTONS_KEY),
    (OPENBOX_KEY, WINDOW_BUTTONS_KEY),
    ("gtk", WINDOW_BUTTONS_KEY),
    ("gtk", ANIMATIONS_KEY),
];

/// Settings sections with the catalog section `cat_key` that have something to configure on the
/// desktop `de_id`, in [`SECTION_SETTINGS`] order. A setting
/// the desktop does not have is not shown at all — neither as a row nor as a button
/// (Wehrwolfmann's decision of 08.11.2026: "MATE has no launch feedback setting…" above
/// an empty page is a dead end). "Icons and toolbars" — everywhere except
/// Deepin (`app_widgets::available`; an unknown desktop — visible), launch
/// feedback — KDE only (`launch_feedback::supported`), program animation
/// in "GTK themes" — only where there is no "Animation" row of its own
/// (a known desktop without window animations), title buttons in "GTK themes" —
/// only where there is no frames section, fonts — everywhere.
pub fn section_settings_panels(cat_key: &str, de_id: &str) -> Vec<&'static str> {
    let known = crate::de_customization::de_from_id(de_id).is_some();
    SECTION_SETTINGS
        .iter()
        .filter(|(k, _)| *k == cat_key)
        .map(|(_, panel)| *panel)
        .filter(|panel| match *panel {
            APP_WIDGETS_PANEL => !known || crate::app_widgets::available(de_id),
            LAUNCH_FEEDBACK_PANEL => crate::launch_feedback::supported(de_id),
            DESKTOP_EFFECTS_PANEL => !known || de_id == "kde",
            ANIMATIONS_KEY => known && !crate::animations::has_window_animations(de_id),
            WINDOW_BUTTONS_KEY if cat_key == "gtk" => {
                !WINDOW_BORDER_SECTIONS.iter().any(|b| category_available(b, de_id))
            }
            _ => true,
        })
        .collect()
}

/// The "Screen effects" section key (`screen_effects`). It was never in the
/// "Settings" tab.
pub const SCREEN_EFFECTS_KEY: &str = "screeneffects";

/// The "Window title buttons" section key (with frame size and frame checkboxes).
pub const WINDOW_BUTTONS_KEY: &str = "windowbuttons";

/// Window frame sections whose header has "⚙️ Settings" with [`WINDOW_BUTTONS_KEY`]: in KDE
/// title buttons are a tab of the "Window Decorations" module, and on every
/// desktop they come right after its frames section (Aurorae, XFWM4, Marco).
/// Cinnamon has no frames section of its own: since 5.4 the Muffin frames are drawn by the GTK theme
/// (`de_customization::UNAVAILABLE_REASONS`), and the buttons go after the Cinnamon
/// theme — the last section of its desktop.
pub const WINDOW_BORDER_SECTIONS: &[&str] = &["aurorae", "xfwm4", "marco", "cinnamontheme", OPENBOX_KEY];

/// Whether this is a settings section (by key).
pub fn is_settings_panel(key: &str) -> bool {
    SETTINGS_PANELS.iter().any(|(_, k)| *k == key)
}

/// The settings section label — the same title as the section itself has.
fn settings_panel_name(key: &str) -> String {
    match key {
        "animations" => tr("Animation"),
        SCREEN_EFFECTS_KEY => tr("Screen effects"),
        other => other.to_string(),
    }
}

/// A sidebar settings section: there is no store and no scan, it has its own screen
/// (`appearance_page::SCREEN_SETTINGS`). It is deliberately not in [`categories`]
/// — there is nothing to install into it.
fn settings_panel(key: &str) -> ThemeCategory {
    let (id, key) = *SETTINGS_PANELS
        .iter()
        .find(|(_, k)| *k == key)
        .expect("раздел настроек есть в SETTINGS_PANELS");
    let icon = match key {
        "animations" => "💫",
        SCREEN_EFFECTS_KEY => "📺",
        _ => "🪄",
    };
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(id, key, settings_panel_name(key), icon, String::new(), "")
    }
}

// There are no more "Appearance → …" links in place of the moved sections in the
// "Settings" tab (Wehrwolfmann's decision of 08.11.2026): the move is long done,
// the sections already stand in the "Appearance" sidebar.

/// Where a settings section is visible — by the same rules by which its section
/// hides itself: "Animation" — where the window
/// manager has window animations (`animations::has_window_animations`, otherwise
/// its checkbox is in "GTK themes → ⚙️ Settings"); "Screen effects" — where they
/// can be enabled in at least one session kind (`screen_effects::support`:
/// sway, river, labwc, niri, Budgie and Deepin cannot do it at all — a section with a single
/// "not possible" sentence is not shown, 08.11.2026). The other sections work on
/// every desktop (at least partly) and state their caveats themselves.
/// An unknown desktop — everything is visible. The session kind — [`Session`].
fn settings_panel_available(key: &str, de_id: &str, session: Session) -> bool {
    if crate::de_customization::de_from_id(de_id).is_none() {
        return true;
    }
    match key {
        ANIMATIONS_KEY => crate::animations::has_window_animations(de_id),
        SCREEN_EFFECTS_KEY => screen_effects_possible(de_id, session),
        _ => true,
    }
}

/// The session kind in which the section is viewed (08.11.2026): what is impossible in the session of
/// the chosen desktop ("X11 only", "Wayland only") is not
/// shown. `Any` — the session is unknown (the "Settings" tab, checks without
/// a window): visible if it works in at least one session kind. The "Appearance" tab
/// window takes the session kind of the chosen desktop (`LookCtx::for_chosen`:
/// the session's desktop — as is, one chosen by hand — its kind from
/// `screen_effects::KNOWN_DESKTOPS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Session {
    #[default]
    Any,
    X11,
    Wayland,
}

impl Session {
    pub fn from_wayland(wayland: bool) -> Self {
        if wayland { Session::Wayland } else { Session::X11 }
    }
    /// Session kinds to check: "any" means both.
    fn kinds(self) -> &'static [bool] {
        match self {
            Session::Any => &[true, false],
            Session::X11 => &[false],
            Session::Wayland => &[true],
        }
    }
}

/// Whether screen effects can be enabled on the desktop `de_id` in this session kind
/// (MATE, Xfce and LXQt — X11 only, via picom). A name that is not in
/// the effects desktop table (`screen_effects::KNOWN_DESKTOPS`) — visible:
/// "don't know" is no reason to hide.
fn screen_effects_possible(de_id: &str, session: Session) -> bool {
    use crate::screen_effects::{support, Coverage, KNOWN_DESKTOPS};
    if !KNOWN_DESKTOPS.iter().any(|(de, _)| *de == de_id) {
        return true;
    }
    session.kinds().iter().any(|wayland| support(de_id, *wayland).coverage != Coverage::None)
}

/// The Openbox and labwc frames section key.
pub const OPENBOX_KEY: &str = "openbox";
/// Its number (synthetic, after the settings sections 921–926; 928 —
/// "Screen effects", created later).
pub const OPENBOX_SECTION_ID: i64 = 927;

/// Where the Openbox frames section is visible: in LXQt (Openbox is its window manager on
/// X11, labwc — on Wayland) and in the labwc session. The matrix merges tiling ones into
/// one desktop, and none of sway, Hyprland or niri has frames.
/// An unknown desktop — visible, like everything else.
fn openbox_available(de_id: &str) -> bool {
    matches!(de_id, "lxqt" | "labwc") || crate::de_customization::de_from_id(de_id).is_none()
}

/// The "MATE panel" section key.
pub const MATE_PANEL_KEY: &str = "mate-panel";

/// The "MATE panel" section: ready mate-panel layouts with one button
/// (`mate_panel`). It has no themes — neither a store nor a disk scan, it has its own screen
/// (`appearance_page::SCREEN_MATE_PANEL`). It is deliberately not in [`categories`]:
/// there is nothing to install into it. The number is synthetic, nothing
/// depends on it.
pub fn mate_panel_category() -> ThemeCategory {
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(920, MATE_PANEL_KEY, trc("appearance section", "MATE Panel"), "🧰", String::new(), "")
    }
}

/// The "Conky widgets" section number ([`DE_SECTIONS`]): in the sidebar it stands
/// next to "Widgets", not at the tail with the desktop sections.
pub const CONKY_SECTION_ID: i64 = 124;

/// The "Login screen (Plasma Login Manager)" section key.
pub const PLASMA_LOGIN_KEY: &str = "plasmalogin";

/// The login screen section when Plasma Login Manager draws it.
///
/// It goes into the sidebar IN PLACE OF "SDDM (login screen)" (see
/// [`sidebar_for`]). It has no items: Plasma Login Manager has no themes, the look
/// is built into the binary itself, and there is no KDE store category for it.
/// So the section is local (no catalog, no network), and the number is synthetic,
/// above nine hundred, like the other sections outside the store: prewarming skips
/// such ones. It is deliberately NOT in [`categories`] — there is nothing to install into it, and
/// `category_by_kind`/`category_by_id` must not find it.
pub fn plasma_login_category() -> ThemeCategory {
    ThemeCategory {
        store: CatalogSource::None,
        ..cat(
            905,
            PLASMA_LOGIN_KEY,
            trc("appearance section", "Login Screen (Plasma Login Manager)"),
            "🔐",
            String::new(),
            "plasmalogin",
        )
    }
}

/// The sidebar taking into account what draws the login screen.
///
/// `plasma_login` — Plasma Login Manager is active
/// (`system_plan::is_plasma_login`). Then in place of the SDDM themes section stands
/// [`plasma_login_category`]: a shelf of SDDM themes there is a deception, none of them will reach
/// the login screen. Otherwise — exactly [`sidebar`].
pub fn sidebar_for(home: &Path, plasma_login: bool) -> Vec<ThemeCategory> {
    let mut out = sidebar(home);
    if plasma_login {
        for c in out.iter_mut() {
            if c.key == "sddm" {
                *c = plasma_login_category();
            }
        }
    }
    out
}

/// What is forbidden for this section on SteamOS (Steam Deck and Steam Machine).
///
/// Rexon does not change the login screen and the boot splash there at all
/// ([`crate::distro_compat::SteamOsBlock`]); the section stays in place and
/// IMMEDIATELY says why, rather than refusing after downloading a theme on the
/// "Install" button (rule #13). `None` — the section works on SteamOS.
pub fn steamos_block(cat_key: &str) -> Option<crate::distro_compat::SteamOsBlock> {
    use crate::distro_compat::SteamOsBlock;
    match cat_key {
        "sddm" | PLASMA_LOGIN_KEY => Some(SteamOsBlock::LoginScreen),
        "plymouth" => Some(SteamOsBlock::BootSplash),
        _ => None,
    }
}

/// An explanation instead of the shelf of a section closed on SteamOS. `None` — the section
/// is open (or this is not SteamOS).
pub fn steamos_note(home: &Path, cat_key: &str, steamos: bool) -> Option<String> {
    if !steamos {
        return None;
    }
    let why = steamos_block(cat_key)?.reason();
    Some(note_text(home, cat_key, &why))
}

/// Keys of local disk overview categories (no network, no "Store:" chips).
pub const LOCAL_VIEW_KEYS: [&str; 4] = ["active", "downloaded", "system", "favorites"];

/// For popular/new — which real cat_ids to request (the most common ones).
/// On a desktop only those that work there remain ([`aggregate_cats`]).
pub const AGGREGATE_CATS: [i64; 5] = [722, 104, 112, 132, 107];

/// Filling up the "Popular"/"New" storefronts where part of [`AGGREGATE_CATS`] does not
/// work (outside KDE global themes and Plasma styles are skipped): GTK themes, wallpapers
/// and Conky widgets — what is installed most often after icons and cursors.
pub const AGGREGATE_EXTRA_CATS: [i64; 3] = [135, 299, CONKY_SECTION_ID];

/// Store categories for "Popular" and "New" on this desktop.
///
/// Until 16.10.2026 the storefronts asked only for the first of [`AGGREGATE_CATS`] —
/// Plasma global themes, and in MATE both top storefronts consisted entirely of
/// things that do not apply there. Now the common categories that work
/// on the desktop are taken (`category_available`, the same matrix that hides
/// sections); if some dropped out, [`AGGREGATE_EXTRA_CATS`] fill up the set.
/// In KDE none drop out, and the set is exactly [`AGGREGATE_CATS`].
pub fn aggregate_cats(de_id: &str) -> Vec<i64> {
    let all = categories(Path::new(""));
    let works = |id: &i64| {
        all.iter()
            .find(|c| c.id == *id)
            .is_some_and(|c| category_available(&c.key, de_id))
    };
    let mut out: Vec<i64> = AGGREGATE_CATS.iter().copied().filter(works).collect();
    if out.len() < AGGREGATE_CATS.len() {
        out.extend(AGGREGATE_EXTRA_CATS.iter().copied().filter(works));
    }
    out
}

/// [`aggregate_cats`] as a string for the request: the store understands a comma-separated
/// list, like the splash umbrella "716,488".
pub fn aggregate_query(de_id: &str) -> String {
    aggregate_cats(de_id).iter().map(|c| c.to_string()).collect::<Vec<_>>().join(",")
}

/// Find a category by internal key — the first one in sidebar order
/// (as `get_category` iterates over `SIDEBAR`). Virtual ones included.
pub fn get_category(home: &Path, key: &str) -> Option<ThemeCategory> {
    sidebar(home)
        .into_iter()
        .find(|c| c.key == key)
        .or_else(|| (key == PLASMA_LOGIN_KEY).then(plasma_login_category))
}

/// Find a real category by OCS cat_id (like `CATEGORIES.get`).
///
/// A subcategory item number (or a previous section number) leads to its
/// section through [`STORE_TYPE_ALIASES`]: a clock widget (708) is the
/// "Widgets" section with everything that follows (install directory, kind, button).
///
/// An item from a desktop section's catalog arrives with the store category
/// number, not the section's: a frame from "XFCE/XFWM4 Themes" is 138, while the section
/// is 906. Such a number leads to the section whose catalog (`store`)
/// contains it — without a separate alias table for each section.
pub fn category_by_id(home: &Path, cat_id: i64) -> Option<ThemeCategory> {
    let all = categories(home);
    let pos = all
        .iter()
        .position(|c| c.id == cat_id)
        .or_else(|| all.iter().position(|c| c.id == section_id_for(cat_id)))
        .or_else(|| all.iter().position(|c| c.ocs_cats().contains(&cat_id)))?;
    all.into_iter().nth(pos)
}

/// Find a category by apply_kind — the first in the declaration order of `CATEGORIES`
/// (as `category_by_kind` iterates over `CATEGORIES.values()`). Robust to
/// OCS cat_id changes: install paths are tied to apply_kind.
pub fn category_by_kind(home: &Path, apply_kind: &str) -> Option<ThemeCategory> {
    categories(home).into_iter().find(|c| c.apply_kind == apply_kind)
}

// ============================================================================
// Link to the matrix: what to show on which desktop
// ============================================================================

/// The `.rxn` type a catalog category is packaged as.
///
/// This is the ONLY place where a sidebar section is linked to an appearance
/// type; after that the `de_customization` matrix decides everything — which sections
/// to show the buyer and why the others are hidden. There must be no type lists in
/// the interface code (rule #12).
///
/// `None` — the section is not about a specific type: overviews of what is already installed
/// (active, downloaded, system, favorites) and storefronts (popular,
/// new) are always shown.
pub fn category_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "global" => "theme-kde-global",
        "plasma" => "theme-kde-plasma",
        "colors" => "theme-kde-colors",
        "aurorae" => "theme-window-decoration",
        "icons" => "theme-icons",
        "cursor" => "theme-cursors",
        "gtk" => "theme-gtk",
        // "Application style" — choosing a Qt style and a Kvantum theme: natively
        // KDE and LXQt can do this.
        "widgetstyle" | "kvantum" => "theme-kvantum",
        "wallpapers" => "wallpaper-pack",
        "wallpaper_plugins" => "kde-wallpaper-plugin",
        "fonts" => "font-pack",
        "sounds" => "theme-sounds",
        "plasmoids" => "theme-plasmoid",
        "sddm" | PLASMA_LOGIN_KEY => "theme-login-screen",
        "splash" => "theme-kde-splash",
        "lockscreen" | "lockwallpaper" => "theme-kde-lockscreen",
        "plymouth" => "theme-plymouth",
        "xfwm4" => "theme-xfwm4",
        "marco" => "theme-marco",
        "gnomeshell" => "theme-gnome-shell",
        "cinnamontheme" => "theme-cinnamon",
        "lxqttheme" => "theme-lxqt",
        OPENBOX_KEY => "theme-openbox",
        "tabbox" => "kde-window-switcher",
        "kwinscripts" => "kde-kwin-script",
        "kwineffects" => "kde-kwin-effect",
        "gnomeext" => "gnome-shell-extension",
        "cinnamonapplets" => "cinnamon-applet",
        "cinnamondesklets" => "cinnamon-desklet",
        "cinnamonext" => "cinnamon-extension",
        "nemoactions" => "nemo-action",
        "conky" => "theme-conky",
        _ => return None,
    })
}

/// Whether to show a catalog section on this desktop.
///
/// An unknown desktop identifier — we show everything: "don't know where I am"
/// is no reason to hide half the program from the person. The session kind is not taken into account
/// (visible if it works in at least one) — for that there is [`category_available_in`].
pub fn category_available(cat_key: &str, de_id: &str) -> bool {
    category_available_in(cat_key, de_id, Session::Any)
}

/// [`category_available`] in the session kind `session`: one rule for all
/// sections that are possible only on X11 or only on Wayland.
pub fn category_available_in(cat_key: &str, de_id: &str, session: Session) -> bool {
    // Only MATE has mate-panel layouts; whether the panel itself is on this
    // machine is decided by the window (`mate_panel::is_available`).
    if cat_key == MATE_PANEL_KEY {
        return crate::de_customization::de_from_id(de_id) == Some(crate::de_customization::De::Mate);
    }
    if is_settings_panel(cat_key) {
        return settings_panel_available(cat_key, de_id, session);
    }
    if cat_key == OPENBOX_KEY {
        return openbox_available(de_id);
    }
    let Some(kind) = category_kind(cat_key) else {
        return true;
    };
    let Some(de) = crate::de_customization::de_from_id(de_id) else {
        return true;
    };
    // "Lock screen" holds two modes: full themes (the matrix type is
    // KDE only) and wallpapers, which have their own apply path. The section exists where
    // at least the wallpaper one works.
    if matches!(cat_key, "lockscreen" | "lockwallpaper") && LOCK_WALLPAPER_DES.contains(&de) {
        return true;
    }
    crate::de_customization::row(kind).is_none_or(|r| r.covers(de))
}

/// Where Rexon sets the lock screen wallpaper
/// (`appearance_installer::lock_wallpaper_plan`): KDE — `kscreenlockerrc`,
/// Deepin — the Control Center in a Treeland session
/// (`dde::control_center_wallpaper_argv`). A new desktop
/// with this mode goes here and into the plan, nowhere else.
pub const LOCK_WALLPAPER_DES: &[crate::de_customization::De] =
    &[crate::de_customization::De::Kde, crate::de_customization::De::Deepin];

/// Whether the desktop has the "Full themes" lock screen mode (the matrix type
/// `theme-kde-lockscreen`). An unknown desktop — it does: no reason to hide.
pub fn lock_themes_supported(de_id: &str) -> bool {
    match crate::de_customization::de_from_id(de_id) {
        None => true,
        Some(de) => crate::de_customization::row("theme-kde-lockscreen").is_none_or(|r| r.covers(de)),
    }
}

/// Under what condition the section works on this desktop — a caption under
/// the title (rule #13: name the condition in advance rather than leave the person
/// guessing why "nothing happened"). `None` — no conditions.
///
/// The pair of [`category_note`]: that one explains why the section is ABSENT, this one — what
/// is needed for the existing section to work.
///
/// The text is taken from the translation catalog in the interface language.
pub fn category_condition(cat_key: &str, de_id: &str) -> Option<String> {
    crate::de_customization::condition_note_id(category_kind(cat_key)?, de_id)
}

/// Why the section is absent on this desktop — text for the person (rule #13:
/// explain rather than show an empty screen). `None` — the section is available.
pub fn category_note(home: &Path, cat_key: &str, de_id: &str) -> Option<String> {
    if category_available(cat_key, de_id) {
        return None;
    }
    let kind = category_kind(cat_key)?;
    let de = crate::de_customization::de_from_id(de_id)?;
    // The desktop name — the same as in the "Desktops" tab.
    let here = crate::system::DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name)
        .unwrap_or("");
    let why = if cat_key == OPENBOX_KEY {
        let here = if here.is_empty() { de.display_label() } else { here.to_string() };
        crate::i18n::trf(
            "Openbox themes are for the Openbox and labwc window managers (as in LXQt). Your selected desktop is %1, \
             which draws window borders its own way, so we don't offer them.",
            &[&here],
        )
    } else {
        crate::de_customization::unavailable_note(kind, de, here)?
    };
    Some(note_text(home, cat_key, &why))
}

/// The explanation screen of a closed section: the section name as the title, the reason and
/// advice to pick another section. One for all reasons — both "not on this
/// desktop" and "not allowed on SteamOS".
fn note_text(home: &Path, cat_key: &str, why: &str) -> String {
    let name = get_category(home, cat_key).map(|c| c.name).unwrap_or_default();
    let head = if name.is_empty() {
        String::new()
    } else {
        // TRANSLATORS: a section name in quotation marks, as a heading.
        format!("{}\n", crate::i18n::trf("“%1”", &[&name]))
    };
    let tail = tr(
        "Pick another section on the left — everything your desktop can \
         actually use is still there.",
    );
    format!("{head}{why}\n{tail}")
}

/// Desktops for the "Current desktop" choice in the "Appearance" tab:
/// `(id, label)` in the order of [`crate::system::DE_META`].
///
/// Tiling ones are merged into ONE item. This is not an eyeballed simplification but what is already
/// recorded in the customization matrix: `de_from_id` returns one and the same
/// [`De::Wlroots`] for sway, Hyprland and
/// niri, so three rows in a row gave
/// the buyer a BIT-FOR-BIT identical catalog — and made them choose where there
/// is no choice (rule #13).
///
/// Under the shared item travels the REAL identifier: `current_de_id`,
/// if a tiling one is running now (or was chosen earlier), otherwise the first of the
/// "Tiling" section ([`crate::qt_desktops::TILING_IDS`], Hyprland). Not the first
/// matching one from [`crate::system::DE_META`]: there Wayfire comes before the tiling ones
/// (also wlroots, but not tiling), and the explanations would call the shared item
/// "Wayfire". It is the real one because outside the catalog there is a difference:
/// the explanation "why the section is absent" calls the desktop by name ("Sway", not
/// "Tiling"), and default application settings are written by each into its own
/// `<id>-mimeapps.list`. The merging lives ONLY in this list.
pub fn appearance_de_choices(current_de_id: &str) -> Vec<(String, String)> {
    use crate::de_customization::{de_from_id, De};

    let is_tiling = |id: &str| de_from_id(id) == Some(De::Wlroots);
    // What travels under the shared item: first what the person actually has.
    let in_meta = |id: &str| crate::system::DE_META.iter().any(|m| m.id == id);
    let tiling_id = if is_tiling(current_de_id) {
        current_de_id
    } else {
        crate::qt_desktops::TILING_IDS
            .iter()
            .copied()
            .find(|id| is_tiling(id) && in_meta(id))
            .or_else(|| crate::system::DE_META.iter().map(|m| m.id).find(|id| is_tiling(id)))
            .unwrap_or_default()
    };

    let mut out: Vec<(String, String)> = Vec::new();
    let mut tiling_added = false;
    for m in crate::system::DE_META {
        if is_tiling(m.id) {
            if tiling_added {
                continue;
            }
            tiling_added = true;
            out.push((tiling_id.to_string(), De::Wlroots.display_label()));
        } else {
            out.push((m.id.to_string(), m.display_name.to_string()));
        }
    }
    out
}

// ============================================================================
// Sidebar groups and section labels per desktop
// ============================================================================

/// A sidebar group. The order of variants is the order of groups on screen.
///
/// A flat list of two dozen sections did not say which of them change
/// the desktop itself, which change all programs, and which only Qt programs; in
/// MATE "Window frames (MATE)" and the Plasma login screen stood side by side. A group
/// answers the question "what will this affect" (Wehrwolfmann's request of
/// 16.10.2026: show only what works, with clear labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SidebarGroup {
    /// Active, downloaded, system, favorites, store storefronts.
    Overview,
    /// The native look of this desktop.
    Desktop,
    /// What all programs pick up: icons, cursors, GTK, fonts,
    /// sounds, wallpapers.
    AllApps,
    /// Only Qt and KDE programs (outside KDE and LXQt).
    QtApps,
    /// Widgets on the desktop: Plasma, Conky, Cinnamon desklets.
    Widgets,
    /// The login screen and the boot splash: visible before login, shared by all
    /// desktops of the machine.
    System,
}

impl SidebarGroup {
    /// A short key — for the sidebar model rows.
    pub fn key(self) -> &'static str {
        match self {
            SidebarGroup::Overview => "overview",
            SidebarGroup::Desktop => "desktop",
            SidebarGroup::AllApps => "all",
            SidebarGroup::QtApps => "qt",
            SidebarGroup::Widgets => "widgets",
            SidebarGroup::System => "system",
        }
    }
}

/// A desktop whose Qt programs are its own: there Qt colors and style
/// change the desktop itself, not "foreign" programs.
fn qt_native(de_id: &str) -> bool {
    use crate::de_customization::De;
    matches!(crate::de_customization::de_from_id(de_id), Some(De::Kde | De::Lxqt))
}

/// Which sidebar group the section is in on this desktop.
pub fn category_group(key: &str, de_id: &str) -> SidebarGroup {
    match key {
        "active" | "downloaded" | "system" | "favorites" | "popular" | "new" => SidebarGroup::Overview,
        "icons" | "cursor" | "gtk" | "fonts" | "sounds" | "wallpapers" => SidebarGroup::AllApps,
        "colors" | "widgetstyle" | "kvantum" => {
            if qt_native(de_id) {
                SidebarGroup::Desktop
            } else {
                SidebarGroup::QtApps
            }
        }
        "plasmoids" | "conky" | "cinnamondesklets" => SidebarGroup::Widgets,
        "sddm" | PLASMA_LOGIN_KEY | "plymouth" => SidebarGroup::System,
        _ => SidebarGroup::Desktop,
    }
}

/// The sidebar by groups: within a group — the previous order of
/// [`sidebar`], groups — in [`SidebarGroup`] order. In the widgets group
/// Conky comes last: the desktop's native widgets (Plasma, Cinnamon desklets) —
/// first, Conky — next to them as shared by all desktops (24.10.2026).
pub fn grouped_sidebar(sidebar: &[ThemeCategory], de_id: &str) -> Vec<ThemeCategory> {
    let mut out = sidebar.to_vec();
    out.sort_by_key(|c| (category_group(&c.key, de_id), c.key == "conky"));
    out
}

/// The desktop name for the group title: the same as in the
/// "Desktops" tab. Empty — the desktop is not recognized.
fn desktop_name(de_id: &str) -> String {
    crate::system::DE_META
        .iter()
        .find(|m| m.id == de_id)
        .map(|m| m.display_name.to_string())
        .or_else(|| crate::de_customization::de_from_id(de_id).map(|d| d.display_label()))
        .unwrap_or_default()
}

/// The group title in the interface language.
pub fn group_title(group: SidebarGroup, de_id: &str) -> String {
    match group {
        SidebarGroup::Overview => trc("appearance group", "Overview"),
        SidebarGroup::Desktop => {
            let name = desktop_name(de_id);
            if name.is_empty() {
                trc("appearance group", "Desktop")
            } else {
                // TRANSLATORS: sidebar group heading; %1 is the desktop (MATE, KDE Plasma…).
                crate::i18n::trf("Desktop “%1”", &[&name])
            }
        }
        SidebarGroup::AllApps => trc("appearance group", "For all apps"),
        SidebarGroup::QtApps => trc("appearance group", "Qt and KDE apps"),
        SidebarGroup::Widgets => trc("appearance group", "Desktop widgets"),
        SidebarGroup::System => trc("appearance group", "System: startup and login"),
    }
}

/// The explanation under the group title on this desktop. Empty — the title
/// is enough.
pub fn group_note(group: SidebarGroup, de_id: &str) -> String {
    match group {
        SidebarGroup::QtApps => tr("VLC, qBittorrent, Krita, Dolphin… The desktop itself doesn't change."),
        SidebarGroup::System => tr("Shared by all desktops on this computer."),
        SidebarGroup::Widgets => widgets_group_note(de_id),
        _ => String::new(),
    }
}

/// The explanation of the "Desktop widgets" group (Wehrwolfmann's decision of
/// 24.10.2026: widgets on the desktop — on every desktop). Almost no desktop
/// has widgets of its own except KDE and Cinnamon, and a person looking for "widgets
/// for the desktop" sees one word, "Conky" — the explanation says what it is,
/// without jargon. The Wayland caveat is gone from here (15.11.2026): on
/// desktops without layers (`conky_overlay::wayland_without_layers`: GNOME,
/// Cinnamon, Pantheon) Rexon runs conky through XWayland, and the widget
/// stays on the desktop as in an X11 session (`conky_overlay::via_xwayland`);
/// a rare session with no XWayland at all gets a caveat in the section title
/// (`de_apply::conky_section_note`), where the session is known. An unknown
/// desktop sees all widget sections at once — there is nothing to explain there.
fn widgets_group_note(de_id: &str) -> String {
    use crate::de_customization::De;
    let Some(de) = crate::de_customization::de_from_id(de_id) else {
        return String::new();
    };
    match de {
        De::Kde => tr("Conky widgets — clocks, weather and sensors — come from a separate program and also work on other desktops."),
        De::Cinnamon => tr(
            "Desklets are Cinnamon's own widgets. Conky widgets — clocks, weather and sensors — come from a \
             separate program and also work on other desktops.",
        ),
        _ => tr("Clocks, weather and system sensors right on the desktop. They're drawn by the Conky program, which works on any desktop."),
    }
}

/// The section label on this desktop.
///
/// The name in [`categories`] does not depend on the desktop (the log and
/// install messages are looked up by it), while in the sidebar and the section title it must
/// tell the truth about THIS desktop: outside Plasma "Color schemes" color
/// only Qt and KDE programs, and so does "Application style"; the "(MATE)" suffix on
/// window frames inside the "Desktop "MATE"" group is superfluous. An unknown
/// desktop (everything visible) — the old names: there both the XFCE frames and
/// the MATE frames stand side by side.
pub fn display_name(cat: &ThemeCategory, de_id: &str) -> String {
    let known = crate::de_customization::de_from_id(de_id).is_some();
    match cat.key.as_str() {
        "sddm" | PLASMA_LOGIN_KEY => trc("appearance section", "Login Screen"),
        "plymouth" => trc("appearance section", "Boot Splash (Plymouth)"),
        "colors" if known && !matches!(crate::de_customization::de_from_id(de_id), Some(crate::de_customization::De::Kde)) => {
            trc("appearance section", "Qt App Colors")
        }
        "widgetstyle" if known && !qt_native(de_id) => trc("appearance section", "Qt App Style (Kvantum)"),
        "xfwm4" | "marco" | OPENBOX_KEY if known => trc("appearance section", "Window Borders"),
        MATE_PANEL_KEY if known => trc("appearance section", "Panel Layout"),
        _ => cat.name.clone(),
    }
}

/// The human name of the login screen by the systemd unit
/// (`appearance_installer::active_login_manager`). Empty — an unknown one.
pub fn login_manager_label(unit: &str) -> &'static str {
    crate::system_plan::login_manager_spec(unit).map_or("", |s| s.label)
}

/// The explanation under the title of a system section (the login screen, the boot
/// splash): they are visible BEFORE a desktop is chosen and shared by all desktops,
/// and a person in MATE must understand why the Plasma login screen is here.
/// `login_unit` — the unit of the active login screen (empty — we don't know).
/// `None` — the section is not a system one.
pub fn system_section_note(key: &str, login_unit: &str) -> Option<String> {
    match key {
        "sddm" | PLASMA_LOGIN_KEY => {
            let shared = tr("Shared by all desktops.");
            let label = login_manager_label(login_unit);
            Some(if label.is_empty() {
                shared
            } else if key == "sddm" && login_unit.trim() != "sddm" {
                // A shelf of SDDM themes with another login screen: say in advance
                // that the themes take effect only if SDDM becomes it.
                format!(
                    "{shared} {}",
                    crate::i18n::trf(
                        "Right now it's shown by %1, and the themes here are for SDDM: \
                         they only take effect if SDDM becomes the login screen.",
                        &[&label],
                    )
                )
            } else {
                format!("{shared} {}", crate::i18n::trf("Right now it's shown by %1.", &[&label]))
            })
        }
        "plymouth" => Some(tr("Shown when the computer starts, before you log in.")),
        _ => None,
    }
}
