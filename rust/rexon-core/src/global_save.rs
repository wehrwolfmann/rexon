//! "Save current appearance…" — a custom global theme from what is on the
//! desktop right now, like the button of the KDE "Global Theme" module.
//!
//! There is one package for all environments — a Plasma global theme package
//! (`~/.local/share/plasma/look-and-feel/<id>/`): `metadata.json`,
//! `contents/defaults` (nested groups `[kdeglobals][General]` and so on),
//! `contents/layouts/` (title bar buttons, panel layout), previews in
//! `contents/previews/`. It is read both by Plasma itself and by Rexon's "Apply
//! "X"?" dialog outside Plasma (`themes::lnf_contents_with`,
//! `outside_global_plan`) — so a theme saved in MATE is applied part by part in
//! XFCE or entirely in Plasma.
//!
//! In Plasma the parts are taken from its files (like Plasma's own snapshot)
//! and, on top of that, fonts (`[General] font…`, `[WM] activeFont`), border
//! size, title bar buttons, the splash screen and the scheme's colors file.
//! Outside Plasma — what Rexon can carry over: colors (the Qt layer scheme),
//! application style, icons, cursors, fonts and title bar buttons, each from
//! the native place of this desktop.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::animations::{LookCtx, LookIo};
use crate::i18n::{tr, trc, trf};
use crate::themes::LnfPart;

/// Part keys: "file/group/key" — that is also how the theme contents name them
/// (`themes::LnfContents::values`).
const COLORS: &str = "kdeglobals/General/ColorScheme";
const WIDGET_STYLE: &str = "kdeglobals/KDE/widgetStyle";
const ICONS: &str = "kdeglobals/Icons/Theme";
const CURSORS: &str = "kcminputrc/Mouse/cursorTheme";
const PLASMA_THEME: &str = "plasmarc/Theme/name";
const DECO_LIBRARY: &str = "kwinrc/org.kde.kdecoration2/library";
const DECO_THEME: &str = "kwinrc/org.kde.kdecoration2/theme";
const BORDER_SIZE: &str = "kwinrc/org.kde.kdecoration2/BorderSize";
const SWITCHER: &str = "kwinrc/WindowSwitcher/LayoutName";
const PLACEMENT: &str = "kwinrc/Windows/Placement";
const FONT_GENERAL: &str = "kdeglobals/General/font";
const FONT_FIXED: &str = "kdeglobals/General/fixed";
const FONT_TITLE: &str = "kdeglobals/WM/activeFont";
/// Fonts: interface, monospace, small, toolbar, menu — in `[General]`; window
/// title — in `[WM]` (that is where Plasma themes keep them).
const FONT_KEYS: [&str; 6] = [
    FONT_GENERAL,
    FONT_FIXED,
    "kdeglobals/General/smallestReadableFont",
    "kdeglobals/General/toolBarFont",
    "kdeglobals/General/menuFont",
    FONT_TITLE,
];

/// Name of the Plasma layout directory in the package.
pub const LAYOUT_SCRIPT: &str = "org.kde.plasma.desktop-layout.js";

/// Width of the small preview — as in Plasma's snapshot.
pub const PREVIEW_SMALL_WIDTH: u32 = 512;

/// What is set now — the values of the parts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// "file/group/key" → value (only non-empty ones).
    pub values: BTreeMap<String, String>,
    /// Title bar buttons as KWin letters: (left, right).
    pub buttons: Option<(String, String)>,
    /// `[Windows] BorderlessMaximizedWindows` KWin.
    pub borderless: Option<String>,
    /// Panel and widget layout — a Plasma script (`dumpCurrentLayoutJS`).
    pub layout_js: Option<String>,
    /// The `contents/splash` directory of the current Plasma splash screen's package.
    pub splash_dir: Option<String>,
    /// Name of the current splash screen (for the label).
    pub splash_name: Option<String>,
    /// The color scheme file (`*.colors`) — put into the package as the `colors` file.
    pub scheme_file: Option<String>,
    /// Text with colors for the preview tile (`[Colors:*]`).
    pub colors_text: Option<String>,
}

impl Snapshot {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    fn put(&mut self, key: &str, value: Option<String>) {
        if let Some(v) = value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            self.values.insert(key.to_string(), v);
        }
    }

    /// Whether a part has anything to save.
    pub fn has(&self, part: LnfPart) -> bool {
        match part {
            LnfPart::TitlebarLayout => self.buttons.is_some(),
            LnfPart::SplashScreen => self.splash_dir.is_some(),
            LnfPart::DesktopLayout => self.layout_js.is_some(),
            _ => part_keys(part).iter().any(|k| self.values.contains_key(*k)),
        }
    }
}

/// Keys a part puts into `contents/defaults`.
fn part_keys(part: LnfPart) -> &'static [&'static str] {
    match part {
        LnfPart::Colors => &[COLORS],
        LnfPart::WidgetStyle => &[WIDGET_STYLE],
        LnfPart::Icons => &[ICONS],
        LnfPart::Cursors => &[CURSORS],
        LnfPart::Fonts => &FONT_KEYS,
        LnfPart::PlasmaTheme => &[PLASMA_THEME],
        LnfPart::WindowDecoration => &[DECO_LIBRARY, DECO_THEME],
        LnfPart::BorderSize => &[BORDER_SIZE],
        LnfPart::WindowSwitcher => &[SWITCHER],
        LnfPart::DesktopLayout => &[PLACEMENT],
        _ => &[],
    }
}

/// Parts that can be saved on desktop `de`: in Plasma — everything the package
/// writes; outside Plasma — what Rexon carries over to any desktop.
pub fn parts_for(de: &str) -> &'static [LnfPart] {
    if de == "kde" {
        &[
            LnfPart::Colors,
            LnfPart::WidgetStyle,
            LnfPart::WindowDecoration,
            LnfPart::BorderSize,
            LnfPart::Icons,
            LnfPart::PlasmaTheme,
            LnfPart::Cursors,
            LnfPart::Fonts,
            LnfPart::WindowSwitcher,
            LnfPart::SplashScreen,
            LnfPart::TitlebarLayout,
            LnfPart::DesktopLayout,
        ]
    } else {
        &[
            LnfPart::Colors,
            LnfPart::WidgetStyle,
            LnfPart::Icons,
            LnfPart::Cursors,
            LnfPart::Fonts,
            LnfPart::TitlebarLayout,
        ]
    }
}

// ── Reading the current state ──────────────────────────────────────────────

/// Environment of the snapshot: the machine, the paths and the process variables.
pub struct SaveEnv<'a> {
    pub io: &'a dyn LookIo,
    pub ctx: LookCtx,
    pub env: &'a dyn Fn(&str) -> String,
}

impl SaveEnv<'_> {
    fn conf(&self, file: &str) -> String {
        self.io.read(&self.ctx.cfg(file)).unwrap_or_default()
    }

    fn kread(&self, file: &str, group: &str, key: &str) -> Option<String> {
        crate::shell_palette::ini_get(&self.conf(file), group, key).filter(|v| !v.trim().is_empty())
    }
}

/// Value of the "file/group/key" key from the KDE settings files.
fn kread_path(env: &SaveEnv, path: &str) -> Option<String> {
    let (file, rest) = path.split_once('/')?;
    let (group, key) = rest.rsplit_once('/')?;
    env.kread(file, group, key)
}

/// Snapshot of the desktop's current appearance from `env.ctx.de`.
pub fn snapshot(env: &SaveEnv) -> Snapshot {
    let mut s = if env.ctx.de == "kde" { snapshot_plasma(env) } else { snapshot_outside(env) };
    common_parts(env, &mut s);
    s
}

/// Plasma: its files; where a key is missing — what Plasma shows by default.
fn snapshot_plasma(env: &SaveEnv) -> Snapshot {
    let mut s = Snapshot::default();
    let or = |path: &str, default: &str| Some(kread_path(env, path).unwrap_or_else(|| default.to_string()));
    s.put(WIDGET_STYLE, or(WIDGET_STYLE, "Breeze"));
    // ⭐ Fix: by default Plasma 6 has "Breeze Light" (`BreezeLight`); it has no
    // `Breeze` scheme, and a theme with that name would not change the colors.
    s.put(COLORS, or(COLORS, "BreezeLight"));
    s.put(ICONS, or(ICONS, "breeze"));
    s.put(PLASMA_THEME, or(PLASMA_THEME, "default"));
    s.put(CURSORS, or(CURSORS, "breeze_cursors"));
    s.put(SWITCHER, or("kwinrc/TabBox/LayoutName", "org.kde.breeze.desktop"));
    s.put(DECO_LIBRARY, or(DECO_LIBRARY, "org.kde.breeze"));
    s.put(DECO_THEME, kread_path(env, DECO_THEME));
    s.put(BORDER_SIZE, kread_path(env, BORDER_SIZE));
    s.put(PLACEMENT, kread_path(env, PLACEMENT));
    for key in FONT_KEYS {
        s.put(key, kread_path(env, key));
    }
    s.borderless = env.kread("kwinrc", "Windows", "BorderlessMaximizedWindows");

    // Panel layout — Plasma's own script.
    let tool = ["qdbus6", "qdbus"].into_iter().find(|t| env.io.have(t));
    if let Some(tool) = tool {
        let argv: Vec<String> = [tool, "org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell.dumpCurrentLayoutJS"]
            .iter()
            .map(|a| a.to_string())
            .collect();
        let (ok, out) = env.io.run(&argv);
        if ok && !out.trim().is_empty() {
            s.layout_js = Some(out);
        }
    }

    // Splash screen: the files of the current splash go into the package (Plasma
    // shows a custom splash only from the theme package itself).
    let engine = env.kread("ksplashrc", "KSplash", "Engine");
    if engine.as_deref() != Some("none") {
        let theme = env.kread("ksplashrc", "KSplash", "Theme").unwrap_or_else(|| crate::themes::LNF_FALLBACK_PACKAGE.into());
        if let Some(dir) = crate::themes::lnf_package_dir(env.io, &env.ctx.home, &theme) {
            let splash = format!("{dir}/contents/splash");
            if env.io.list_dir(&splash).iter().any(|n| n == "Splash.qml") {
                s.splash_dir = Some(splash);
                s.splash_name = Some(theme);
            }
        }
    }
    s
}

/// Outside Plasma: each from the native place of this desktop, as Rexon sees it.
fn snapshot_outside(env: &SaveEnv) -> Snapshot {
    let mut s = Snapshot::default();
    let de = env.ctx.de.as_str();
    let deepin = de == "deepin";

    // Colors — the scheme of Rexon's Qt layer (`[General] ColorScheme` in
    // kdeglobals). In Deepin programs are drawn by Chameleon, there is no layer.
    if !deepin {
        s.put(COLORS, kread_path(env, COLORS));
    }

    // Application style — Qt's own order: the variable, our environment.d file,
    // kdeglobals, lxqt.conf, qt6ct/qt5ct.
    if !deepin {
        let style = Some((env.env)("QT_STYLE_OVERRIDE"))
            .filter(|v| !v.trim().is_empty())
            .or_else(|| env_file_value(&env.conf("environment.d/rexon-qt-style.conf"), "QT_STYLE_OVERRIDE"))
            .or_else(|| kread_path(env, WIDGET_STYLE))
            .or_else(|| env.kread("lxqt/lxqt.conf", "Qt", "style"))
            .or_else(|| env.kread("qt6ct/qt6ct.conf", "Appearance", "style"))
            .or_else(|| env.kread("qt5ct/qt5ct.conf", "Appearance", "style"));
        s.put(WIDGET_STYLE, style);
    }

    // Icons and cursors — the desktop's native storage, otherwise fallback places.
    let [_, icons, cursor] = crate::installed_scan::native_role_values(de, |cmd| {
        let (ok, out) = env.io.run(&cmd.argv);
        (ok && !out.trim().is_empty()).then_some(out)
    });
    s.put(ICONS, icons.or_else(|| kread_path(env, ICONS)));
    s.put(
        CURSORS,
        cursor
            .or_else(|| kread_path(env, CURSORS))
            .or_else(|| Some((env.env)("XCURSOR_THEME")).filter(|v| !v.trim().is_empty())),
    );

    // Fonts: interface, monospace, title — from the desktop's native keys; small,
    // toolbar and menu — from our block in gtk.css (outside KDE they have no native
    // key), if they are set.
    if crate::qt_settings::font_refusal_live(de).is_none() {
        let wm = crate::qt_settings::session_title_wm(
            de,
            &env.ctx.config_home,
            &(env.env)("XDG_CONFIG_DIRS"),
            env.ctx.wayland,
            &|p| env.io.read(p),
        );
        let kde_tool = ["kreadconfig6", "kreadconfig5"].into_iter().find(|t| env.io.have(t)).unwrap_or("");
        let targets = [
            ("system", FONT_GENERAL),
            ("mono", FONT_FIXED),
            ("small", FONT_KEYS[2]),
            ("toolbar", FONT_KEYS[3]),
            ("menu", FONT_KEYS[4]),
            ("title", FONT_TITLE),
        ];
        for (target, key) in targets {
            if !crate::qt_settings::font_target_available_wm(de, target, wm) {
                continue;
            }
            let raw = match crate::qt_settings::font_plan_wm(de, target, wm).primary {
                crate::qt_settings::FontStore::Cmd(backend) => crate::qt_settings::sound_read_argv(&backend, kde_tool)
                    .map(|argv| env.io.run(&argv))
                    .filter(|(ok, _)| *ok)
                    .map(|(_, out)| out)
                    .unwrap_or_default(),
                // sway: the effective title font — our block, the `font` line of the config or
                // sway's default, with the style.
                crate::qt_settings::FontStore::File(crate::qt_settings::FontFile::SwayConf) => {
                    crate::qt_settings::sway_title_font_current(env.io, &env.ctx)
                }
                // Openbox/labwc rc.xml: the user's own, without it the system one that the
                // manager uses; `<weight>`/`<slant>` go into the style.
                crate::qt_settings::FontStore::File(file @ crate::qt_settings::FontFile::RcXml { wm }) => {
                    let (_, text, _) = crate::qt_settings::rc_xml_source(
                        wm,
                        &env.ctx.config_home,
                        &(env.env)("XDG_CONFIG_DIRS"),
                        &|p| env.io.read(p),
                    );
                    text.map(|t| file.read_spec(&t)).unwrap_or_default()
                }
                crate::qt_settings::FontStore::File(file) => {
                    env.io.read(&env.ctx.cfg(&file.rel_path())).map(|t| file.read_spec(&t)).unwrap_or_default()
                }
            };
            s.put(key, kde_font_from_raw(&raw));
        }
    }
    s
}

/// Common for all desktops: title bar buttons, the scheme file and the tile colors.
fn common_parts(env: &SaveEnv, s: &mut Snapshot) {
    use crate::window_buttons as wb;
    let mut wm = env.ctx.clone();
    wm.de = wb::wm_key(env.io, &env.ctx);
    let layout = wb::read_layout(env.io, &wm);
    if !layout.left.is_empty() || !layout.right.is_empty() {
        s.buttons = Some(wb::format(wb::Codec::Kde, &layout));
    }

    if let Some(name) = s.get(COLORS).map(str::to_string) {
        let mut dirs = vec![env.ctx.data_home.clone()];
        dirs.extend(env.ctx.data_dirs.iter().cloned());
        s.scheme_file = dirs
            .iter()
            .map(|d| format!("{}/color-schemes/{name}.colors", d.trim_end_matches('/')))
            .find(|p| env.io.read(p).is_some());
    }
    // Tile colors: kdeglobals carries the colors of the applied scheme; if they
    // are not there — the scheme file.
    let kdeglobals = env.conf("kdeglobals");
    s.colors_text = if has_color_groups(&kdeglobals) {
        Some(kdeglobals)
    } else {
        s.scheme_file.as_deref().and_then(|p| env.io.read(p)).filter(|t| has_color_groups(t))
    };
}

fn has_color_groups(text: &str) -> bool {
    text.lines().any(|l| l.trim().starts_with("[Colors:"))
}

/// `NAME=value` of an environment.d file.
fn env_file_value(text: &str, name: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(name)?.strip_prefix('=').map(|v| v.trim().trim_matches('"').to_string()))
        .filter(|v| !v.is_empty())
}

/// A font from the desktop's native entry ("'Sans Bold 10'", "Sans,10,…") — a KDE
/// entry (`Family,10,-1,5,…`). A KDE entry already in the right form — as is.
///
/// The style from a Pango description goes into the QFont fields, not into the
/// family name: "Sans Bold 10" is `Sans,10,-1,5,700,0,…` (that is how Plasma
/// itself would write it), not a "Sans Bold" family that does not exist on any
/// system.
pub fn kde_font_from_raw(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_matches(['\'', '"']).trim();
    if raw.is_empty() {
        return None;
    }
    if raw.split(',').count() >= 10 {
        return Some(raw.to_string());
    }
    let (family, size, face) = crate::qt_settings::parse_font_spec_face(raw);
    (!family.trim().is_empty() && size > 0)
        .then(|| crate::qt_settings::kde_font_spec_face(family.trim(), size, face))
}

// ── Window ────────────────────────────────────────────────────────────────

/// A "what to save" row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePartRow {
    pub id: LnfPart,
    pub text: String,
    /// What is set now (the caption under the checkbox).
    pub value: String,
    pub enabled: bool,
    pub checked: bool,
    /// Why there is nothing to save.
    pub reason: String,
}

/// Everything the "Save current appearance" window draws.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDialog {
    pub button: String,
    pub button_tooltip: String,
    pub title: String,
    pub intro: String,
    pub name_label: String,
    pub name_placeholder: String,
    pub author_label: String,
    pub author: String,
    pub parts_heading: String,
    pub parts: Vec<SavePartRow>,
    pub preview_heading: String,
    pub shot_text: String,
    pub pick_text: String,
    pub tile_text: String,
    pub preview_note: String,
    pub shot_waiting: String,
    pub save_text: String,
    pub cancel_text: String,
}

/// Caption of what is set now.
fn part_value(s: &Snapshot, part: LnfPart) -> String {
    match part {
        // With the style: "DejaVu Sans Bold 10" (QFont fields → Pango words).
        LnfPart::Fonts => FONT_KEYS
            .iter()
            .find_map(|k| s.get(k))
            .map(|f| {
                let (family, size, face) = crate::qt_settings::parse_font_spec_face(f);
                crate::qt_settings::font_face_spec(family.trim(), size, face)
            })
            .unwrap_or_default(),
        LnfPart::WindowDecoration => {
            let theme = s.get(DECO_THEME).unwrap_or("");
            let theme = theme.strip_prefix("__aurorae__svg__").unwrap_or(theme);
            if theme.is_empty() { s.get(DECO_LIBRARY).unwrap_or("").to_string() } else { theme.to_string() }
        }
        LnfPart::SplashScreen => s.splash_name.clone().unwrap_or_default(),
        LnfPart::TitlebarLayout => s
            .buttons
            .as_ref()
            .map(|(l, r)| {
                let side = |v: &str| v.chars().filter_map(|c| button_name(c)).collect::<Vec<_>>().join(", ");
                let (l, r) = (side(l), side(r));
                match (l.is_empty(), r.is_empty()) {
                    (true, _) => trf("right: %1", &[&r]),
                    (_, true) => trf("left: %1", &[&l]),
                    _ => trf("left: %1; right: %2", &[&l, &r]),
                }
            })
            .unwrap_or_default(),
        LnfPart::DesktopLayout => {
            if s.layout_js.is_some() { tr("panels, widgets and window placement") } else { String::new() }
        }
        _ => part_keys(part).first().and_then(|k| s.get(k)).unwrap_or("").to_string(),
    }
}

/// A title bar button by its KWin letter — for the caption.
fn button_name(c: char) -> Option<String> {
    Some(match c {
        'M' => tr("menu"),
        'S' => tr("on all desktops"),
        'H' => tr("help"),
        'I' => tr("minimize"),
        'A' => tr("maximize"),
        'X' => tr("close"),
        'F' => tr("keep above"),
        'B' => tr("keep below"),
        'L' => tr("shade"),
        'N' => tr("application menu"),
        _ => return None,
    })
}

/// The window for desktop `de` from snapshot `s`; `author` — the default name.
pub fn dialog(de: &str, s: &Snapshot, author: &str) -> SaveDialog {
    let parts = parts_for(de)
        .iter()
        .map(|&part| {
            let enabled = s.has(part);
            SavePartRow {
                id: part,
                text: part.label(),
                value: if enabled { part_value(s, part) } else { String::new() },
                enabled,
                checked: enabled,
                reason: if enabled { String::new() } else { missing_reason(de, part) },
            }
        })
        .collect();
    let outside = de != "kde";
    SaveDialog {
        button: tr("Save Current Appearance…"),
        button_tooltip: tr("Save what this desktop looks like now as your own global theme"),
        title: tr("Save Current Appearance"),
        intro: if outside {
            trf(
                "The current look of %1 will be saved as a new global theme. You can then apply it here or on any other desktop — whole or part by part — and in Plasma too.",
                &[&crate::screenshots::display_name(de)],
            )
        } else {
            tr("Plasma’s current appearance and theme settings will be saved in a new global theme.")
        },
        name_label: tr("Name:"),
        name_placeholder: tr("Type in name…"),
        author_label: tr("Author:"),
        author: author.to_string(),
        parts_heading: tr("What to save:"),
        parts,
        preview_heading: trc("theme preview image", "Preview:"),
        shot_text: tr("Take a screenshot…"),
        pick_text: tr("Choose an image…"),
        tile_text: tr("Tile in the theme colors"),
        preview_note: tr("Without a screenshot, the theme tile is drawn in its colors."),
        shot_waiting: shot_waiting_text(),
        save_text: tr("Save"),
        cancel_text: trc_cancel(),
    }
}

fn trc_cancel() -> String {
    crate::i18n::trc("dialog button", "Cancel")
}

/// Why a part has nothing to save.
fn missing_reason(de: &str, part: LnfPart) -> String {
    if de == "deepin" && matches!(part, LnfPart::Colors | LnfPart::WidgetStyle | LnfPart::Fonts) {
        return tr("in Deepin this is set by Deepin itself, and Rexon does not carry it over");
    }
    match part {
        LnfPart::Colors => tr("no color scheme is set on this desktop"),
        LnfPart::DesktopLayout => tr("Plasma did not give out its panel layout"),
        LnfPart::SplashScreen => tr("the splash screen is turned off"),
        _ => tr("nothing is set on this desktop"),
    }
}

// ── Name and package ─────────────────────────────────────────────────────

/// Prefix of the identifier of a theme saved from the current appearance.
pub const LOCAL_ID_PREFIX: &str = "local.";

/// Identifier (`KPlugin.Id`) and directory name of the new theme from the name
/// the person entered: `local.` and the name in lower-case Latin letters, digits
/// and hyphens ("Stend Proba" → `local.stend-proba`; Cyrillic is
/// transliterated; a name without Latin letters gets a stable hash).
///
/// Plasma itself (`KCMLookandFeel::saveCurrentTheme`) takes the entered name
/// both into `Id` and into the directory name as is — with spaces and any
/// characters. Plasma and store packages are named differently
/// (`org.kde.breeze.desktop`): dots, Latin letters, hyphens — and so here;
/// `KPlugin.Name` stays as entered.
pub fn theme_id(name: &str) -> String {
    format!("{LOCAL_ID_PREFIX}{}", crate::importers::slug60(name.trim()))
}

/// What is wrong with the new theme's name; `None` — it is fine. As in Plasma
/// (`LookAndFeelNameValidator`): neither the name nor the identifier of the new
/// theme ([`theme_id`]) may match the name or identifier of an existing one.
pub fn name_problem(io: &dyn LookIo, home: &str, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some(tr("Type in a name for the theme."));
    }
    let new_id = theme_id(name);
    for root in crate::themes::kde_global_dirs(home) {
        for dir in io.list_dir(&root) {
            let meta = io
                .read(&format!("{root}/{dir}/metadata.json"))
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
            let field = |key: &str| meta.as_ref().and_then(|v| v.pointer(key)).and_then(|n| n.as_str()).map(str::to_string);
            let (id, their_name) = (field("/KPlugin/Id").unwrap_or_else(|| dir.clone()), field("/KPlugin/Name"));
            let taken = [dir.as_str(), id.as_str()].iter().any(|i| *i == name || *i == new_id)
                || their_name.as_deref() == Some(name);
            if taken {
                return Some(tr("A global theme with this name or id already exists."));
            }
        }
    }
    None
}

/// Directory of the new theme.
pub fn package_dir(home: &str, id: &str) -> String {
    format!("{}/{id}", crate::themes::kde_global_dirs(home)[0])
}

/// Package labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
}

/// The package's `metadata.json` — the fields of a Plasma 6 global theme package.
pub fn metadata_json(m: &Manifest) -> String {
    let v = serde_json::json!({
        "KPackageStructure": "Plasma/LookAndFeel",
        "KPlugin": {
            "Authors": [{ "Name": m.author }],
            "Description": m.description,
            "Id": m.id,
            "Name": m.name,
        },
        "Keywords": "Desktop;Workspace;Appearance;Look and Feel;",
        "X-Plasma-APIVersion": "2",
        "X-Plasma-MainScript": "default",
    });
    serde_json::to_string_pretty(&v).unwrap_or_default() + "\n"
}

/// A value for a KConfig file: backslash, newline, tab and spaces at the edges
/// are escaped, as KConfig writes them.
fn kconfig_escape(v: &str) -> String {
    let mut out = String::new();
    let n = v.chars().count();
    for (i, c) in v.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            ' ' if i == 0 || i + 1 == n => out.push_str("\\s"),
            c => out.push(c),
        }
    }
    out
}

/// Text of a KConfig file from "file/group/key" → value entries: groups are
/// nested (`[kdeglobals][General]`), in order of first appearance.
fn kconfig_text(entries: &[(&str, &str)]) -> String {
    let mut groups: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for (path, value) in entries {
        let Some((group, key)) = path.rsplit_once('/') else { continue };
        let header = format!("[{}]", group.replace('/', "]["));
        match groups.iter_mut().find(|(h, _)| *h == header) {
            Some((_, keys)) => keys.push((key.to_string(), value.to_string())),
            None => groups.push((header, vec![(key.to_string(), value.to_string())])),
        }
    }
    let mut out = String::new();
    for (i, (header, keys)) in groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(header);
        out.push('\n');
        for (k, v) in keys {
            out.push_str(&format!("{k}={}\n", kconfig_escape(v)));
        }
    }
    out
}

/// Text files of the package (path from the package directory → text) from the
/// snapshot and the checked parts. The splash screen (a directory copy) and the
/// previews (pictures) are separate, in the live save.
pub fn package_files(m: &Manifest, s: &Snapshot, parts: &[LnfPart]) -> Vec<(String, String)> {
    let on = |p: LnfPart| parts.contains(&p) && s.has(p);
    let mut files = vec![("metadata.json".to_string(), metadata_json(m))];

    let mut defaults: Vec<(&str, &str)> = Vec::new();
    for part in LnfPart::ALL {
        if !on(part) && !(part == LnfPart::DesktopLayout && parts.contains(&part)) {
            continue;
        }
        for key in part_keys(part) {
            if let Some(v) = s.get(key) {
                defaults.push((key, v));
            }
        }
    }
    if !defaults.is_empty() {
        files.push(("contents/defaults".to_string(), kconfig_text(&defaults)));
    }

    let mut layout_defaults: Vec<(&str, &str)> = Vec::new();
    if let (true, Some((l, r))) = (on(LnfPart::TitlebarLayout), &s.buttons) {
        layout_defaults.push(("kwinrc/org.kde.kdecoration2/ButtonsOnLeft", l));
        layout_defaults.push(("kwinrc/org.kde.kdecoration2/ButtonsOnRight", r));
    }
    if let (true, Some(b)) = (on(LnfPart::DesktopLayout), &s.borderless) {
        layout_defaults.push(("kwinrc/Windows/BorderlessMaximizedWindows", b));
    }
    if !layout_defaults.is_empty() {
        files.push(("contents/layouts/defaults".to_string(), kconfig_text(&layout_defaults)));
    }
    if let (true, Some(js)) = (on(LnfPart::DesktopLayout), &s.layout_js) {
        files.push((format!("contents/layouts/{LAYOUT_SCRIPT}"), js.clone()));
    }
    files
}

// ── Preview ──────────────────────────────────────────────────────────────

/// Preview tile — a window in the theme's colors (the same drawing as the scheme
/// tile in "Colors"), `scale` times larger than the preview frame.
pub fn tile_image(colors_text: &str, scale: u32) -> image::RgbaImage {
    use crate::local_preview::{scheme_look, scheme_window_sample, DrawOp, PREVIEW_HEIGHT, PREVIEW_WIDTH};
    let (w, h) = (PREVIEW_WIDTH as u32 * scale, PREVIEW_HEIGHT as u32 * scale);
    let mut img = image::RgbaImage::new(w, h);
    let k = scale as f64;
    for op in scheme_window_sample(&scheme_look(colors_text)) {
        if let DrawOp::Fill { rect: (x, y, rw, rh), color } = op {
            let (x0, y0) = ((x * k).round().max(0.0) as u32, (y * k).round().max(0.0) as u32);
            let (x1, y1) = (((x + rw) * k).round().min(w as f64) as u32, ((y + rh) * k).round().min(h as f64) as u32);
            for py in y0..y1 {
                for px in x0..x1 {
                    img.put_pixel(px, py, image::Rgba([color.0, color.1, color.2, color.3]));
                }
            }
        }
    }
    img
}

/// Write the previews into the package's `contents/previews`: from picture
/// `source` (a screenshot, a custom drawing) — `fullscreenpreview.jpg` and
/// `preview.png` of width [`PREVIEW_SMALL_WIDTH`], as in Plasma; without it — a
/// tile in the theme's colors.
pub fn write_previews(dir: &str, source: Option<&str>, colors_text: &str) -> Result<(), String> {
    let previews = format!("{dir}/contents/previews");
    std::fs::create_dir_all(&previews).map_err(|e| e.to_string())?;
    match source.filter(|s| !s.is_empty()) {
        Some(src) => {
            let img = image::open(src).map_err(|e| trf("Could not read the image: %1", &[&e]))?;
            img.to_rgb8().save(format!("{previews}/fullscreenpreview.jpg")).map_err(|e| e.to_string())?;
            let small = if img.width() > PREVIEW_SMALL_WIDTH {
                let h = (img.height() as u64 * PREVIEW_SMALL_WIDTH as u64 / img.width().max(1) as u64).max(1) as u32;
                img.resize_exact(PREVIEW_SMALL_WIDTH, h, image::imageops::FilterType::Lanczos3)
            } else {
                img
            };
            small.save(format!("{previews}/preview.png")).map_err(|e| e.to_string())
        }
        None => tile_image(colors_text, 2).save(format!("{previews}/preview.png")).map_err(|e| e.to_string()),
    }
}

/// Copy a whole directory (the theme's splash screen).
fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// ── Live machine ─────────────────────────────────────────────────────────

/// What the window sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveRequest {
    pub name: String,
    pub author: String,
    /// Checked parts — their names in the window (`colors`, `widgetStyle`, …).
    pub parts: Vec<String>,
    /// Preview picture; empty — a tile.
    pub preview: String,
}

/// Result of saving.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub ok: bool,
    pub message: String,
    pub id: String,
    pub path: String,
}

/// A part by its name from the window — the same name as [`SavePartRow::id`].
pub fn part_by_id(id: &str) -> Option<LnfPart> {
    LnfPart::ALL.into_iter().find(|p| serde_json::to_value(p).ok().and_then(|v| v.as_str().map(|s| s == id)) == Some(true))
}

/// The person's name for the "Author" field: the account's full name
/// (`/etc/passwd`, the GECOS field), otherwise the login name.
pub fn author_from_passwd(passwd: &str, user: &str) -> String {
    passwd
        .lines()
        .find(|l| l.split(':').next() == Some(user))
        .and_then(|l| l.split(':').nth(4))
        .and_then(|g| g.split(',').next())
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .unwrap_or(user)
        .to_string()
}

fn live_env_get(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

/// The window for this machine and desktop.
pub fn live_dialog() -> SaveDialog {
    let io = crate::animations::SystemIo;
    let env = SaveEnv { io: &io, ctx: LookCtx::from_env(), env: &live_env_get };
    let s = snapshot(&env);
    let user = live_env_get("USER");
    let author = author_from_passwd(&io.read("/etc/passwd").unwrap_or_default(), &user);
    dialog(&env.ctx.de, &s, &author)
}

/// Name check for the window: empty — fine.
pub fn live_name_problem(name: &str) -> String {
    name_problem(&crate::animations::SystemIo, &live_env_get("HOME"), name).unwrap_or_default()
}

/// Tile for the window (before a screenshot is chosen): a PNG in the cache, its path.
pub fn live_tile() -> Result<String, String> {
    let io = crate::animations::SystemIo;
    let env = SaveEnv { io: &io, ctx: LookCtx::from_env(), env: &live_env_get };
    let s = snapshot(&env);
    let cache = crate::utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &env.ctx.home);
    let path = format!("{cache}/rexon/global-save-tile.png");
    std::fs::create_dir_all(format!("{cache}/rexon")).map_err(|e| e.to_string())?;
    tile_image(s.colors_text.as_deref().unwrap_or(""), 2).save(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Save the theme at the window's request (JSON [`SaveRequest`]).
pub fn live_save(json: &str) -> SaveResult {
    let req: SaveRequest = match serde_json::from_str(json) {
        Ok(r) => r,
        Err(e) => return SaveResult { message: e.to_string(), ..Default::default() },
    };
    let io = crate::animations::SystemIo;
    let env = SaveEnv { io: &io, ctx: LookCtx::from_env(), env: &live_env_get };
    let fail = |message: String| SaveResult { message, ..Default::default() };
    let name = req.name.trim().to_string();
    if let Some(why) = name_problem(&io, &env.ctx.home, &name) {
        return fail(why);
    }
    let s = snapshot(&env);
    let parts: Vec<LnfPart> = req.parts.iter().filter_map(|id| part_by_id(id)).filter(|p| parts_for(&env.ctx.de).contains(p) && s.has(*p)).collect();
    if parts.is_empty() {
        return fail(tr("Nothing to save: no part of the appearance is checked."));
    }
    let id = theme_id(&name);
    let m = Manifest {
        id: id.clone(),
        name: name.clone(),
        author: req.author.trim().to_string(),
        description: trf("Saved from the current look of %1", &[&crate::screenshots::display_name(&env.ctx.de)]),
    };
    let dir = package_dir(&env.ctx.home, &id);
    let result = (|| -> Result<(), String> {
        for (rel, text) in package_files(&m, &s, &parts) {
            if !io.write(&format!("{dir}/{rel}"), &text) {
                return Err(trf("Could not write %1", &[&format!("{dir}/{rel}")]));
            }
        }
        if parts.contains(&LnfPart::Colors) {
            if let Some(text) = s.scheme_file.as_deref().and_then(|p| io.read(p)) {
                io.write(&format!("{dir}/contents/colors"), &text);
            }
        }
        if let (true, Some(splash)) = (parts.contains(&LnfPart::SplashScreen), &s.splash_dir) {
            copy_dir(std::path::Path::new(splash), std::path::Path::new(&format!("{dir}/contents/splash")))
                .map_err(|e| e.to_string())?;
        }
        write_previews(&dir, Some(req.preview.as_str()).filter(|p| !p.is_empty()), s.colors_text.as_deref().unwrap_or(""))
    })();
    match result {
        Ok(()) => {
            crate::log_info!("theme", "глобальная тема сохранена из текущего оформления: {dir}");
            SaveResult { ok: true, message: trf("Global theme “%1” saved.", &[&name]), id, path: dir }
        }
        Err(e) => {
            // A half-written package is not needed in the grid.
            let _ = std::fs::remove_dir_all(&dir);
            fail(e)
        }
    }
}

// ── Screenshot ───────────────────────────────────────────────────────────

/// Path from the portal's answer (`file:///…`, with %-escapes).
pub fn path_from_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&rest[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).ok().filter(|p| !p.is_empty())
}

/// A screenshot through the portal (`org.freedesktop.portal.Screenshot`, with a
/// choice, as in Plasma): the path to the file.
fn portal_screenshot() -> Result<String, String> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedValue, Value};
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let sender = conn
        .unique_name()
        .map(|n| n.as_str().trim_start_matches(':').replace('.', "_"))
        .ok_or_else(|| "no bus name".to_string())?;
    let token = format!(
        "rexon{}_{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0)
    );
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")
        .and_then(|b| b.member("Response"))
        .and_then(|b| b.path(path.as_str()))
        .map_err(|e| e.to_string())?
        .build();
    let responses = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(4)).map_err(|e| e.to_string())?;
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    opts.insert("interactive", Value::from(true));
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Screenshot"),
        "Screenshot",
        &("", opts),
    )
    .map_err(|e| e.to_string())?;
    for msg in responses {
        let msg = msg.map_err(|e| e.to_string())?;
        let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize().map_err(|e| e.to_string())?;
        if code != 0 {
            return Err(String::new());
        }
        let uri = match results.get("uri").map(|v| &**v) {
            Some(Value::Str(s)) => s.as_str().to_string(),
            _ => String::new(),
        };
        return path_from_uri(&uri).ok_or_else(|| tr("The screenshot was not saved."));
    }
    Err(tr("The screenshot was not saved."))
}

/// Pause before a screenshot by a screenshot program (when there is no portal).
pub const SHOT_DELAY_SECS: u64 = 5;

/// Hint while the screenshot is waiting.
pub fn shot_waiting_text() -> String {
    trf(
        "Choose what to capture in the window that opens. If no window opens, the screen is captured in %1 seconds — minimize Rexon meanwhile.",
        &[&SHOT_DELAY_SECS],
    )
}

/// A screenshot for the preview: the portal, without it the first screenshot
/// program found. `Ok(path)`; `Err("")` — the person cancelled.
pub fn live_screenshot() -> Result<String, String> {
    match portal_screenshot() {
        Ok(p) => return Ok(p),
        Err(e) if e.is_empty() => return Err(e),
        Err(e) => crate::log_info!("theme", "снимок через портал не вышел ({e}), берём программу снимков"),
    }
    // Without the portal no choice dialog will appear: we give time to minimize
    // Rexon, otherwise it would be in the screenshot itself.
    std::thread::sleep(std::time::Duration::from_secs(SHOT_DELAY_SECS));
    let home = live_env_get("HOME");
    let dir = format!("{}/rexon", crate::utils::cache_home(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = format!("{dir}/global-save_auto.png");
    let _ = std::fs::remove_file(&path);
    let path_env = std::env::var("PATH").ok();
    let argv = crate::screenshots::select_screenshot_tool(&dir, "global-save", |t| {
        crate::utils::which(t, path_env.as_deref()).is_some()
    })
    .ok_or_else(crate::screenshots::no_tools_msg)?;
    let (ok, out) = crate::animations::SystemIo.run(&argv);
    if ok && std::path::Path::new(&path).is_file() {
        Ok(path)
    } else {
        Err(if out.trim().is_empty() { tr("The screenshot was not saved.") } else { out.trim().to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        files: RefCell<HashMap<String, String>>,
        tools: Vec<&'static str>,
        answers: HashMap<String, String>,
    }

    impl LookIo for Fake {
        fn run(&self, argv: &[String]) -> (bool, String) {
            match self.answers.get(&argv.join(" ")) {
                Some(a) => (true, a.clone()),
                None => (false, String::new()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            self.tools.contains(&tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let mut out: Vec<String> = self
                .files
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix).map(|r| r.split('/').next().unwrap_or("").to_string()))
                .collect();
            out.sort();
            out.dedup();
            out
        }
    }

    fn ctx(de: &str) -> LookCtx {
        let mut c = LookCtx::new(de, "/h", None);
        c.data_dirs = vec!["/usr/share".into()];
        c
    }

    fn no_env(_: &str) -> String {
        String::new()
    }

    #[test]
    fn plasma_снимок_со_шрифтами_рамкой_и_раскладкой() {
        let mut io = Fake { tools: vec!["qdbus6"], ..Default::default() };
        io.answers.insert(
            "qdbus6 org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell.dumpCurrentLayoutJS".into(),
            "var panel = new Panel\n".into(),
        );
        let f = &io.files;
        f.borrow_mut().insert(
            "/h/.config/kdeglobals".into(),
            "[General]\nColorScheme=BreezeDark\nfont=Noto Sans,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n\n[WM]\nactiveFont=Noto Sans,12,-1,5,700,0,0,0,0,0,0,0,0,0,0,1\n\n[Icons]\nTheme=Papirus\n\n[Colors:Window]\nBackgroundNormal=32,35,38\n".into(),
        );
        f.borrow_mut().insert(
            "/h/.config/kwinrc".into(),
            "[org.kde.kdecoration2]\nlibrary=org.kde.kwin.aurorae\ntheme=__aurorae__svg__Sweet\nButtonsOnLeft=M\nButtonsOnRight=IAX\n".into(),
        );
        f.borrow_mut().insert("/usr/share/color-schemes/BreezeDark.colors".into(), "[Colors:Window]\n".into());
        let env = SaveEnv { io: &io, ctx: ctx("kde"), env: &no_env };
        let s = snapshot(&env);
        assert_eq!(s.get(COLORS), Some("BreezeDark"));
        // What is missing from the files — what Plasma shows by default.
        assert_eq!(s.get(WIDGET_STYLE), Some("Breeze"));
        assert_eq!(s.get(CURSORS), Some("breeze_cursors"));
        assert_eq!(s.get(FONT_TITLE).map(|f| f.starts_with("Noto Sans,12")), Some(true));
        assert_eq!(s.buttons, Some(("M".into(), "IAX".into())));
        assert_eq!(s.scheme_file.as_deref(), Some("/usr/share/color-schemes/BreezeDark.colors"));
        assert!(s.layout_js.is_some());

        let m = Manifest { id: "Моя".into(), name: "Моя".into(), author: "Wehrwolfmann".into(), description: String::new() };
        let files: HashMap<String, String> = package_files(&m, &s, parts_for("kde")).into_iter().collect();
        let d = &files["contents/defaults"];
        // Fonts in [General], the title in [WM].
        assert!(d.contains("[kdeglobals][General]\nColorScheme=BreezeDark\nfont=Noto Sans,11"), "{d}");
        assert!(d.contains("[kdeglobals][WM]\nactiveFont=Noto Sans,12"), "{d}");
        assert!(d.contains("[kwinrc][org.kde.kdecoration2]\nlibrary=org.kde.kwin.aurorae\ntheme=__aurorae__svg__Sweet"));
        assert!(d.contains("[kwinrc][WindowSwitcher]\nLayoutName=org.kde.breeze.desktop"));
        assert_eq!(files["contents/layouts/defaults"], "[kwinrc][org.kde.kdecoration2]\nButtonsOnLeft=M\nButtonsOnRight=IAX\n");
        assert!(files.contains_key(&format!("contents/layouts/{LAYOUT_SCRIPT}")));
        let meta: serde_json::Value = serde_json::from_str(&files["metadata.json"]).unwrap();
        assert_eq!(meta["KPlugin"]["Id"], "Моя");
        assert_eq!(meta["KPackageStructure"], "Plasma/LookAndFeel");
    }

    /// A package written here is read by Rexon's theme contents with the same parts
    /// and values — for Plasma and for applying outside it.
    #[test]
    fn пакет_читается_составом_темы() {
        let io = Fake::default();
        let mut s = Snapshot::default();
        s.put(COLORS, Some("Nordic".into()));
        s.put(WIDGET_STYLE, Some("kvantum".into()));
        s.put(ICONS, Some("Papirus-Dark".into()));
        s.put(CURSORS, Some("Bibata".into()));
        s.put(FONT_GENERAL, kde_font_from_raw("'Cantarell 11'"));
        s.put(FONT_TITLE, kde_font_from_raw("Cantarell Bold 11"));
        s.buttons = Some(("X".into(), "IA".into()));
        let m = Manifest { id: "t".into(), name: "t".into(), ..Default::default() };
        for (rel, text) in package_files(&m, &s, parts_for("mate")) {
            io.write(&format!("/p/t/{rel}"), &text);
        }
        let c = crate::themes::lnf_contents(&io, "/p/t");
        for part in [LnfPart::Colors, LnfPart::WidgetStyle, LnfPart::Icons, LnfPart::Cursors, LnfPart::Fonts, LnfPart::TitlebarLayout] {
            assert!(c.has(part), "{part:?}: {:?}", c.parts);
        }
        assert_eq!(c.values.get(COLORS).map(String::as_str), Some("Nordic"));
        assert_eq!(c.values.get(FONT_GENERAL).map(String::as_str), Some("Cantarell,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(c.layout.buttons, Some(("X".into(), "IA".into())));
        // Title bar buttons are not the panel layout: the tile has no mark.
        assert!(!c.layout.desktop);
        // Outside Plasma all saved parts are carried over.
        for part in &c.parts {
            if !part.is_layout() || *part == LnfPart::TitlebarLayout {
                assert_eq!(crate::themes::lnf_part_outside(*part, &c, "xfce"), None, "{part:?}");
            }
        }
    }

    #[test]
    fn вне_plasma_шрифты_и_стиль_из_родных_мест() {
        let mut io = Fake { tools: vec!["gsettings"], ..Default::default() };
        io.answers.insert("gsettings get org.mate.interface font-name".into(), "'Ubuntu 11'\n".into());
        io.answers.insert("gsettings get org.mate.interface monospace-font-name".into(), "'Ubuntu Mono 13'\n".into());
        io.files.borrow_mut().insert("/h/.config/qt6ct/qt6ct.conf".into(), "[Appearance]\nstyle=Fusion\n".into());
        let env = SaveEnv { io: &io, ctx: ctx("mate"), env: &no_env };
        let s = snapshot(&env);
        assert_eq!(s.get(WIDGET_STYLE), Some("Fusion"));
        assert_eq!(s.get(FONT_GENERAL), Some("Ubuntu,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(s.get(FONT_FIXED), Some("Ubuntu Mono,13,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
        // No scheme — colors are not saved, the window says why.
        let d = dialog("mate", &s, "Wehrwolfmann");
        let colors = d.parts.iter().find(|p| p.id == LnfPart::Colors).unwrap();
        assert!(!colors.enabled && !colors.reason.is_empty());
        assert!(d.parts.iter().all(|p| !matches!(p.id, LnfPart::PlasmaTheme | LnfPart::DesktopLayout)));
    }

    /// MATE: the menu, toolbar and small fonts — from our block in gtk.css, with the
    /// style (Bold Italic for the menu), into the theme package as QFont fields; a
    /// marker from earlier versions (without a style) is read as a regular font. The
    /// caption in the save window includes the style.
    #[test]
    fn вне_plasma_шрифты_меню_панели_и_маленький_с_начертанием() {
        use crate::qt_settings::{FontFace, FontFile};
        let mut io = Fake { tools: vec!["gsettings"], ..Default::default() };
        io.answers.insert("gsettings get org.mate.interface font-name".into(), "'DejaVu Sans Bold 10'\n".into());
        let menu = FontFile::GtkCss { version: "3.0".into(), target: "menu".into() };
        let toolbar = FontFile::GtkCss { version: "3.0".into(), target: "toolbar".into() };
        let css = menu.try_new_text_face(None, "Noto Sans", 9, FontFace { weight: 700, italic: true }).unwrap();
        let css = toolbar.try_new_text_face(Some(&css), "Inter", 10, FontFace { weight: 400, italic: false }).unwrap();
        // A marker from an earlier version: without the style field.
        let css = css.replace("/* <<< Rexon: fonts */", "/* rexon:font small 8 Cantarell */\n/* <<< Rexon: fonts */");
        io.files.borrow_mut().insert("/h/.config/gtk-3.0/gtk.css".into(), css);
        let env = SaveEnv { io: &io, ctx: ctx("mate"), env: &no_env };
        let s = snapshot(&env);
        assert_eq!(s.get(FONT_KEYS[4]), Some("Noto Sans,9,-1,5,700,1,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(s.get(FONT_KEYS[3]), Some("Inter,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(s.get(FONT_KEYS[2]), Some("Cantarell,8,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
        let d = dialog("mate", &s, "Wehrwolfmann");
        let fonts = d.parts.iter().find(|p| p.id == LnfPart::Fonts).unwrap();
        assert_eq!(fonts.value, "DejaVu Sans Bold 10");
        let m = Manifest { id: "t".into(), name: "t".into(), ..Default::default() };
        let files: HashMap<String, String> = package_files(&m, &s, parts_for("mate")).into_iter().collect();
        let defaults = &files["contents/defaults"];
        assert!(defaults.contains("menuFont=Noto Sans,9,-1,5,700,1,0,0,0,0,0,0,0,0,0,1"), "{defaults}");
        assert!(defaults.contains("toolBarFont=Inter,10,-1,5,400,0,"), "{defaults}");
        assert!(defaults.contains("smallestReadableFont=Cantarell,8,-1,5,400,0,"), "{defaults}");
        // Not set — they do not get into the package.
        let mut bare = Fake { tools: vec!["gsettings"], ..Default::default() };
        bare.answers.insert("gsettings get org.mate.interface font-name".into(), "'Ubuntu 11'\n".into());
        let s = snapshot(&SaveEnv { io: &bare, ctx: ctx("mate"), env: &no_env });
        assert!(FONT_KEYS[2..5].iter().all(|k| s.get(k).is_none()));
    }

    /// All commands of the apply plan — one line each.
    fn plan_argvs(plan: &crate::themes::Plan) -> Vec<String> {
        use crate::themes::ApplyStep as A;
        plan.steps
            .iter()
            .filter_map(|st| match st {
                A::Primary(c) | A::BestEffort(c) | A::IfOk(c) | A::IfNotOk(c) => Some(c.argv.join(" ")),
                _ => None,
            })
            .collect()
    }

    /// A MATE snapshot with a bold title, an italic interface and a semi-bold
    /// monospace → theme package → theme contents.
    fn mate_faces_theme() -> crate::themes::LnfContents {
        let mut io = Fake { tools: vec!["gsettings"], ..Default::default() };
        io.answers.insert("gsettings get org.mate.interface font-name".into(), "'Sans Italic 10'\n".into());
        io.answers.insert("gsettings get org.mate.interface monospace-font-name".into(), "'Mono Semi-Bold 11'\n".into());
        io.answers.insert("gsettings get org.mate.Marco.general titlebar-font".into(), "'Sans Bold 10'\n".into());
        let env = SaveEnv { io: &io, ctx: ctx("mate"), env: &no_env };
        let s = snapshot(&env);
        // The style is in the QFont fields (weight, slant), the family is clean.
        assert_eq!(s.get(FONT_TITLE), Some("Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(s.get(FONT_GENERAL), Some("Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1"));
        assert_eq!(s.get(FONT_FIXED), Some("Mono,11,-1,5,600,0,0,0,0,0,0,0,0,0,0,1"));
        let out = Fake::default();
        let m = Manifest { id: "t".into(), name: "t".into(), ..Default::default() };
        for (rel, text) in package_files(&m, &s, parts_for("mate")) {
            out.write(&format!("/p/t/{rel}"), &text);
        }
        crate::themes::lnf_contents(&out, "/p/t")
    }

    fn fonts_press(contents: crate::themes::LnfContents) -> crate::themes::GlobalPress {
        crate::themes::GlobalPress {
            choice: crate::themes::GlobalChoice { appearance: true, layout: false, parts: Some(LnfPart::Fonts.bit()) },
            contents,
            ..Default::default()
        }
    }

    /// MATE → theme → MATE: weight and slant come back as Pango words rather than
    /// accidentally riding inside the family name.
    #[test]
    fn шрифты_с_начертанием_mate_тема_mate() {
        let g = crate::themes::apply_global_choice_plan("t", &fonts_press(mate_faces_theme()), "mate").unwrap();
        let argvs = plan_argvs(&g.plan);
        for want in [
            "gsettings set org.mate.Marco.general titlebar-font Sans Bold 10",
            "gsettings set org.mate.interface font-name Sans Italic 10",
            "gsettings set org.mate.interface monospace-font-name Mono Semi-Bold 11",
        ] {
            assert!(argvs.iter().any(|a| a == want), "{want}: {argvs:#?}");
        }
        assert!(g.skipped.is_empty(), "{:?}", g.skipped);
    }

    /// MATE → theme → Plasma: a QFont entry with weight and slant goes into
    /// kdeglobals, and Plasma reads it as its own.
    #[test]
    fn шрифты_с_начертанием_mate_тема_plasma() {
        // One more theme part — so that fonts go through their own commands rather than
        // the whole theme via plasma-apply-lookandfeel.
        let mut c = mate_faces_theme();
        c.values.insert(ICONS.into(), "Papirus".into());
        c.parts.push(LnfPart::Icons);
        let g = crate::themes::apply_global_choice_plan("t", &fonts_press(c), "kde").unwrap();
        let argvs = plan_argvs(&g.plan);
        for (key, value) in [
            ("--group WM --key activeFont ", "Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1"),
            ("--group General --key font ", "Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1"),
            ("--group General --key fixed ", "Mono,11,-1,5,600,0,0,0,0,0,0,0,0,0,0,1"),
        ] {
            assert!(argvs.iter().any(|a| a.contains(key) && a.ends_with(value)), "{key}{value}: {argvs:#?}");
        }
    }

    /// labwc and sway title with a style → the theme's QFont fields. labwc without
    /// its own rc.xml reads the system one; sway — the `font` line of its config.
    #[test]
    fn шрифт_заголовка_с_начертанием_labwc_и_sway() {
        let io = Fake::default();
        io.write(
            "/etc/xdg/labwc/rc.xml",
            "<labwc_config>\n  <theme>\n    <font place=\"ActiveWindow\">\n      <name>Inter</name>\n      <size>9</size>\n      <weight>semibold</weight>\n      <slant>italic</slant>\n    </font>\n  </theme>\n</labwc_config>\n",
        );
        let s = snapshot(&SaveEnv { io: &io, ctx: ctx("labwc"), env: &no_env });
        assert_eq!(s.get(FONT_TITLE), Some("Inter,9,-1,5,600,1,0,0,0,0,0,0,0,0,0,1"));

        let io = Fake::default();
        io.write("/h/.config/sway/config", "font pango:Noto Sans Bold 11\n");
        let s = snapshot(&SaveEnv { io: &io, ctx: ctx("sway"), env: &no_env });
        assert_eq!(s.get(FONT_TITLE), Some("Noto Sans,11,-1,5,700,0,0,0,0,0,0,0,0,0,0,1"));
    }

    /// A Plasma theme with a style (Qt 6 and the old Qt 5 entry) → XFCE.
    #[test]
    fn шрифты_с_начертанием_plasma_тема_xfce() {
        let mut c = crate::themes::LnfContents::default();
        c.values.insert(FONT_TITLE.into(), "Noto Sans,11,-1,5,600,1,0,0,0,0,0,0,0,0,0,1".into());
        c.values.insert(FONT_GENERAL.into(), "Noto Sans,10,-1,5,75,0,0,0,0,0".into());
        c.parts.push(LnfPart::Fonts);
        let g = crate::themes::apply_global_choice_plan("t", &fonts_press(c), "xfce").unwrap();
        let argvs = plan_argvs(&g.plan);
        assert!(argvs.iter().any(|a| a.ends_with("/general/title_font -n -t string -s Noto Sans Semi-Bold Italic 11")), "{argvs:#?}");
        assert!(argvs.iter().any(|a| a.ends_with("/Gtk/FontName -n -t string -s Noto Sans Bold 10")), "{argvs:#?}");
    }

    #[test]
    fn имя_как_у_plasma() {
        let io = Fake::default();
        io.write("/h/.local/share/plasma/look-and-feel/org.x/metadata.json", r#"{"KPlugin":{"Name":"Тёмная"}}"#);
        assert!(name_problem(&io, "/h", "").is_some());
        assert!(name_problem(&io, "/h", "org.x").is_some(), "имя = id существующей");
        assert!(name_problem(&io, "/h", "Тёмная").is_some(), "имя = имя существующей");
        assert_eq!(name_problem(&io, "/h", "Моя тема"), None);
        // The directory and id of the saved theme come from the name, and the name is
        // as entered: "Stend proba" gives the same id as "Stend Proba".
        io.write(
            "/h/.local/share/plasma/look-and-feel/local.stend-proba/metadata.json",
            r#"{"KPlugin":{"Id":"local.stend-proba","Name":"Stend Proba"}}"#,
        );
        assert!(name_problem(&io, "/h", "Stend proba").is_some(), "тот же id");
        assert!(name_problem(&io, "/h", "Stend Proba").is_some());
        assert_eq!(name_problem(&io, "/h", "a/b"), None, "косая черта в имени каталогу не мешает");
    }

    #[test]
    fn id_как_у_пакетов_plasma() {
        assert_eq!(theme_id("Stend Proba"), "local.stend-proba");
        assert_eq!(theme_id("  Моя тема "), "local.moya-tema");
        assert_eq!(theme_id("../.x"), "local.x");
        let id = theme_id("我的主题");
        assert!(id.starts_with("local.theme-"), "{id}");
        assert!(id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'), "{id}");
    }

    #[test]
    fn автор_из_passwd_и_экранирование() {
        let pw = "root:x:0:0::/root:/bin/bash\nalex:x:1000:1000:Wehrwolfmann,,,:/home/alex:/bin/fish\nbob:x:1001:1001::/home/bob:/bin/sh\n";
        assert_eq!(author_from_passwd(pw, "alex"), "Wehrwolfmann");
        assert_eq!(author_from_passwd(pw, "bob"), "bob");
        assert_eq!(part_by_id("widgetStyle"), Some(LnfPart::WidgetStyle));
        assert_eq!(part_by_id("nope"), None);
        assert_eq!(kconfig_escape(" a\\b\n"), "\\sa\\\\b\\n");
        assert_eq!(path_from_uri("file:///tmp/%D0%AF%20x.png").as_deref(), Some("/tmp/Я x.png"));
        assert_eq!(kde_font_from_raw("Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1").as_deref(), Some("Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"));
    }

    #[test]
    fn превью_как_у_plasma_и_плитка() {
        let root = std::env::temp_dir().join(format!("rexon-global-save-{}", std::process::id()));
        let dir = root.to_string_lossy().into_owned();
        // Without a screenshot — a tile.
        write_previews(&dir, None, "").unwrap();
        assert!(root.join("contents/previews/preview.png").is_file());
        // A 1600×900 screenshot — a full JPEG and a small PNG of width 512.
        let shot = root.join("shot.png");
        image::RgbaImage::from_pixel(1600, 900, image::Rgba([1, 2, 3, 255])).save(&shot).unwrap();
        write_previews(&dir, Some(&shot.to_string_lossy()), "").unwrap();
        assert!(root.join("contents/previews/fullscreenpreview.jpg").is_file());
        let small = image::open(root.join("contents/previews/preview.png")).unwrap();
        assert_eq!((small.width(), small.height()), (512, 288));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn плитка_цветами_схемы() {
        let img = tile_image("[Colors:Window]\nBackgroundNormal=10,20,30\n", 2);
        assert_eq!(img.dimensions(), (600, 340));
        assert_eq!(img.get_pixel(300, 330).0, [10, 20, 30, 255]);
    }
}
