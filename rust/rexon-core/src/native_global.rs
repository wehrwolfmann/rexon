//! "Whole themes" in the desktop's own format, for the «Global Themes» section
//! outside Plasma.
//!
//! WHY. In MATE the «Global Themes» section used to show only Plasma
//! look-and-feel packages, and a person saw "a theme for Plasma 6" in MATE.
//! A desktop that has its own format of a theme applied as a whole now gets
//! those themes first, applied the way the desktop's own settings window does
//! it; Plasma packages stay below, with a note on the tile saying what of them
//! carries over to this desktop and what only Plasma can use
//! ([`plasma_card_note`]).
//!
//! The formats (researched 27.11.2026 against the desktops' own tools):
//!
//! * **MATE** — the metatheme of the GNOME 2 heritage: `index.theme` of a theme
//!   directory with an `[X-GNOME-Metatheme]` group (`GtkTheme`, `MetacityTheme`,
//!   `IconTheme`, optional `CursorTheme`, `CursorSize`, `NotificationTheme`,
//!   `Hidden`), in `~/.themes`, `$XDG_DATA_HOME/themes` and
//!   `<$XDG_DATA_DIRS>/themes`. The «Appearance» window of MATE lists a theme
//!   only when `GtkTheme`, `MetacityTheme` and `IconTheme` are all named, and
//!   applies it as `org.mate.interface gtk-theme`/`icon-theme`,
//!   `org.mate.Marco.general theme`, `org.mate.peripherals-mouse cursor-theme`
//!   and `org.mate.NotificationDaemon theme`.
//! * **Cinnamon** — the "styles" of the simplified Themes page:
//!   `/usr/share/cinnamon/styles.d/*.styles`, JSON
//!   `{"styles": [{"name", "default", "mixed"|"dark"|"light": [variant…]}]}`, a
//!   variant names `themes` (one name for all parts) or `gtk`, `icons`,
//!   `cinnamon`, `cursor`, plus its `color`. Cinnamon offers a variant only when
//!   all four parts are installed, and applies it as
//!   `org.cinnamon.desktop.interface gtk-theme`/`icon-theme`/`cursor-theme`,
//!   `org.cinnamon.theme name` and `org.x.apps.portal color-scheme` by the mode.
//!   The files are shipped by distributions (Linux Mint, Ubuntu Cinnamon); Arch
//!   has none, and then the section simply has no native themes.
//!
//! The other desktops have no format of their own for a whole theme (see the
//! report of 27.11.2026): there the section shows the Plasma packages with the
//! same note.
//!
//! The module is pure except for the `*_live` functions; the apply plan is
//! assembled by the live installer from the same press plans the «GTK Themes»,
//! «Icons» and «Cursors» sections use ([`press_steps`]).

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use crate::i18n::{tr, trc, trf};
use crate::install_plan::Step;
use crate::models::ThemeItem;
use crate::themes::{ApplyStep, Cmd};

/// The key of the synthetic category native themes belong to (it is not in the
/// sidebar: the tiles live in «Global Themes»).
pub const SECTION_KEY: &str = "nativeglobal";

/// Its number: synthetic, after «Screen effects» (928).
pub const SECTION_ID: i64 = 929;

/// Where Cinnamon reads its styles from (only the system directory, as
/// `cs_themes.py` does).
pub const CINNAMON_STYLES_DIR: &str = "/usr/share/cinnamon/styles.d";

/// A desktop's own format of a whole theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Format {
    /// MATE: `index.theme` with `[X-GNOME-Metatheme]`.
    MateMetatheme,
    /// Cinnamon: a variant of a style from `styles.d`.
    CinnamonStyle,
}

/// The native format of desktop `de_id`; `None` — the desktop has none.
pub fn format_for(de_id: &str) -> Option<Format> {
    use crate::de_customization::{de_from_id, De};
    match de_from_id(de_id)? {
        De::Mate => Some(Format::MateMetatheme),
        De::Cinnamon => Some(Format::CinnamonStyle),
        _ => None,
    }
}

/// A part of a native theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Part {
    /// Controls (the GTK theme).
    Controls,
    /// Window borders (Marco).
    WindowBorders,
    /// The desktop shell theme (Cinnamon: panels and menus).
    Shell,
    Icons,
    Pointer,
    /// The look of notification bubbles (MATE notification daemon).
    Notifications,
    /// Light or dark preference for applications (Cinnamon style mode).
    ColorScheme,
}

impl Part {
    pub fn label(self) -> String {
        match self {
            Part::Controls => trc("native theme part", "controls"),
            Part::WindowBorders => trc("native theme part", "window borders"),
            Part::Shell => trc("native theme part", "desktop (panels and menus)"),
            Part::Icons => trc("native theme part", "icons"),
            Part::Pointer => trc("native theme part", "pointer"),
            Part::Notifications => trc("native theme part", "notifications"),
            Part::ColorScheme => trc("native theme part", "light or dark style"),
        }
    }
}

/// One native theme as the section shows and applies it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NativeTheme {
    /// The identifier inside the section: the theme directory name (MATE) or
    /// `<file>|<style>|<mode>|<variant>` (Cinnamon).
    pub key: String,
    /// The name shown on the tile.
    pub name: String,
    pub comment: String,
    /// Where it lives on disk: the theme directory or the styles file.
    pub path: String,
    pub gtk: String,
    pub borders: Option<String>,
    pub shell: Option<String>,
    pub icons: Option<String>,
    pub cursor: Option<String>,
    pub cursor_size: Option<u32>,
    pub notifications: Option<String>,
    /// The `org.x.apps.portal color-scheme` value of a Cinnamon style mode.
    pub color_scheme: Option<String>,
}

impl NativeTheme {
    /// The parts with the names they set, in the order the tile lists them.
    pub fn parts(&self) -> Vec<(Part, String)> {
        let mut out = vec![(Part::Controls, self.gtk.clone())];
        let mut put = |p: Part, v: &Option<String>| {
            if let Some(v) = v {
                out.push((p, v.clone()));
            }
        };
        put(Part::WindowBorders, &self.borders);
        put(Part::Shell, &self.shell);
        put(Part::Icons, &self.icons);
        put(Part::Pointer, &self.cursor);
        put(Part::Notifications, &self.notifications);
        put(Part::ColorScheme, &self.color_scheme);
        out
    }
}

/// A theme name is a directory name, not a path: `../x` from someone else's
/// file must not reach the settings.
fn safe(v: String) -> Option<String> {
    let v = v.trim().to_string();
    (!v.is_empty() && !v.contains('/') && v != "." && v != "..").then_some(v)
}

/// A localized value: `Key[ll_CC]`, `Key[ll]`, `Key` — first in `sections[0]`,
/// then in the next one.
fn localized(text: &str, sections: &[&str], base: &str, locale: &str) -> Option<String> {
    let keys = crate::installed_scan::locale_keys(base, locale);
    sections
        .iter()
        .find_map(|s| keys.iter().find_map(|k| crate::themes::ini_key_value(text, s, k)))
        .filter(|v| !v.trim().is_empty())
}

const META: &str = "X-GNOME-Metatheme";
const ENTRY: &str = "Desktop Entry";

/// A MATE metatheme from `index.theme` of directory `dir_name` (at `path`).
///
/// The same rules as the MATE «Appearance» window: `GtkTheme`,
/// `MetacityTheme` and `IconTheme` are required, `Hidden=true` hides the theme,
/// the name comes from the metatheme group or `[Desktop Entry]`. `None` — not a
/// metatheme MATE would list.
pub fn parse_metatheme(dir_name: &str, path: &str, text: &str, locale: &str) -> Option<NativeTheme> {
    let get = |key: &str| crate::themes::ini_key_value(text, META, key).and_then(safe);
    let hidden = crate::themes::ini_key_value(text, META, "Hidden").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if hidden {
        return None;
    }
    let gtk = get("GtkTheme")?;
    let borders = get("MetacityTheme")?;
    let icons = get("IconTheme")?;
    let name = localized(text, &[META, ENTRY], "Name", locale)?;
    let comment = localized(text, &[META, ENTRY], "Comment", locale).unwrap_or_default();
    // Without a cursor MATE writes "default" — the X server's own pointer. We
    // keep the person's pointer instead: the theme did not ask for one.
    let cursor = get("CursorTheme").filter(|c| !c.eq_ignore_ascii_case("default"));
    let cursor_size = cursor
        .as_ref()
        .and(crate::themes::ini_key_value(text, META, "CursorSize"))
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|n| *n > 0);
    Some(NativeTheme {
        key: dir_name.to_string(),
        name,
        comment,
        path: path.to_string(),
        gtk,
        borders: Some(borders),
        icons: Some(icons),
        cursor,
        cursor_size,
        notifications: get("NotificationTheme"),
        ..Default::default()
    })
}

/// The Cinnamon style mode → the `color-scheme` value Cinnamon sets with it.
fn mode_scheme(mode: &str) -> &'static str {
    match mode {
        "dark" => "prefer-dark",
        "light" => "prefer-light",
        _ => "default",
    }
}

fn mode_label(mode: &str) -> String {
    match mode {
        "dark" => trc("cinnamon style mode", "dark"),
        "light" => trc("cinnamon style mode", "light"),
        _ => trc("cinnamon style mode", "mixed"),
    }
}

/// Variants of the styles in one `styles.d` file `file` (its name without the
/// directory) — each variant is a tile. A variant without all four parts is
/// skipped, as in Cinnamon; a file that is not valid JSON gives nothing.
pub fn parse_cinnamon_styles(file: &str, path: &str, json: &str) -> Vec<NativeTheme> {
    let Ok(root) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let file = file.strip_suffix(".styles").unwrap_or(file);
    let mut out = Vec::new();
    for style in root.get("styles").and_then(Value::as_array).into_iter().flatten() {
        let Some(style_name) = style.get("name").and_then(Value::as_str) else { continue };
        for mode in ["mixed", "dark", "light"] {
            for v in style.get(mode).and_then(Value::as_array).into_iter().flatten() {
                let Some(variant) = v.get("name").and_then(Value::as_str) else { continue };
                let field = |k: &str| {
                    v.get(k)
                        .or_else(|| v.get("themes"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .and_then(safe)
                };
                let (Some(gtk), Some(icons), Some(shell), Some(cursor)) =
                    (field("gtk"), field("icons"), field("cinnamon"), field("cursor"))
                else {
                    continue;
                };
                out.push(NativeTheme {
                    key: [file, style_name, mode, variant].join("|"),
                    // TRANSLATORS: a Cinnamon style tile: %1 the style (Mint-Y), %2 the
                    // mode (dark), %3 the colour variant name (blue).
                    name: trf("%1 — %2, %3", &[&style_name, &mode_label(mode), &variant]),
                    comment: String::new(),
                    path: path.to_string(),
                    gtk,
                    shell: Some(shell),
                    icons: Some(icons),
                    cursor: Some(cursor),
                    color_scheme: Some(mode_scheme(mode).to_string()),
                    ..Default::default()
                });
            }
        }
    }
    out
}

/// What is on disk: answers "is a set of this kind with this name installed".
pub trait Installed {
    fn has(&self, part: Part, name: &str) -> bool;
}

/// Whether a theme can be offered: its controls theme must be installed (without
/// it nothing would change), and a Cinnamon variant needs all its parts (as
/// Cinnamon itself demands). Missing MATE icons and pointer are installed
/// before applying, as for a GTK theme (`prepare_theme_deps`).
pub fn offerable(theme: &NativeTheme, format: Format, disk: &dyn Installed) -> bool {
    if !disk.has(Part::Controls, &theme.gtk) {
        return false;
    }
    match format {
        Format::MateMetatheme => true,
        Format::CinnamonStyle => theme
            .parts()
            .iter()
            .filter(|(p, _)| matches!(p, Part::Shell | Part::Icons | Part::Pointer))
            .all(|(p, n)| disk.has(*p, n)),
    }
}

/// Whether the theme is the one in use: controls and icons, plus the borders /
/// shell theme when the map knows them (`active_map` of the disk scan: keys
/// `gtk`, `icons`, `marco`, `cinnamontheme`).
pub fn is_active(theme: &NativeTheme, active: &HashMap<String, String>) -> bool {
    let same = |key: &str, want: &Option<String>| match (want, active.get(key)) {
        (Some(w), Some(a)) => w == a,
        _ => true,
    };
    active.get("gtk").is_some_and(|g| *g == theme.gtk)
        && active.get("icons").is_some_and(|i| Some(i) == theme.icons.as_ref())
        && same("marco", &theme.borders)
        && same("cinnamontheme", &theme.shell)
}

/// The controls and icons of the theme are the ones in use (borders / shell may differ).
fn core_in_use(theme: &NativeTheme, active: &HashMap<String, String>) -> bool {
    active.get("gtk").is_some_and(|g| *g == theme.gtk)
        && active.get("icons").is_some_and(|i| Some(i) == theme.icons.as_ref())
}

/// The parts of the theme that differ from what is in use: `(part, name in use)`
/// for the borders and the shell theme. MATE's own dialog shows such a mix as a
/// custom theme; Rexon keeps the tile of the theme it comes from and names the difference.
pub fn differs_from_active(theme: &NativeTheme, active: &HashMap<String, String>) -> Vec<(Part, String)> {
    [("marco", Part::WindowBorders, &theme.borders), ("cinnamontheme", Part::Shell, &theme.shell)]
        .into_iter()
        .filter_map(|(key, part, want)| match (want, active.get(key)) {
            (Some(w), Some(a)) if w != a => Some((part, a.clone())),
            _ => None,
        })
        .collect()
}

/// Which themes count as in use: the exact match; when there is none, the
/// themes whose controls and icons are in use (the borders were changed by
/// hand) and that differ in the fewest parts, so the look in use still comes first.
pub fn in_use_flags(themes: &[NativeTheme], active: &HashMap<String, String>) -> Vec<bool> {
    let exact: Vec<bool> = themes.iter().map(|t| is_active(t, active)).collect();
    if exact.iter().any(|&e| e) {
        return exact;
    }
    let near: Vec<Option<usize>> = themes
        .iter()
        .map(|t| core_in_use(t, active).then(|| differs_from_active(t, active).len()))
        .collect();
    let best = near.iter().flatten().min().copied();
    near.iter().map(|n| n.is_some() && *n == best).collect()
}

/// The desktop name for the notes ("MATE", "Cinnamon").
pub fn desktop_name(de_id: &str) -> String {
    crate::system::DE_META
        .iter()
        .find(|m| m.id == de_id || m.session_name == de_id)
        .map(|m| m.display_name.to_string())
        .or_else(|| crate::de_customization::de_from_id(de_id).map(|d| d.display_label()))
        .unwrap_or_else(|| de_id.to_string())
}

/// A tile of a native theme.
pub fn to_item(theme: &NativeTheme, active: bool, home: &str) -> ThemeItem {
    let own = !home.is_empty() && theme.path.starts_with(&format!("{}/", home.trim_end_matches('/')));
    ThemeItem {
        id: format!("local:{SECTION_KEY}:{}", theme.key),
        name: theme.name.clone(),
        description: theme.comment.clone(),
        category_id: SECTION_ID,
        installed: true,
        applied: active,
        install_path: theme.path.clone(),
        local_kind: Some(if active { "active" } else if own { "downloaded" } else { "system" }.to_string()),
        source_name: if theme.key != theme.name && !theme.key.contains('|') { theme.key.clone() } else { String::new() },
        ..Default::default()
    }
}

/// The line and the tooltip under the tile name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct CardNote {
    pub line: String,
    pub tooltip: String,
    /// Draw the line in the warning colour (a Plasma package outside Plasma).
    pub warn: bool,
}

/// The note of a native theme tile: what it sets, by name.
pub fn native_card_note(
    theme: &NativeTheme,
    de_name: &str,
    differs: &[(Part, String)],
    dark: Option<DarkPreference>,
) -> CardNote {
    let parts = theme.parts();
    let labels: Vec<String> = parts.iter().map(|(p, _)| p.label()).collect();
    let mut tip: Vec<String> = parts
        .iter()
        .map(|(p, n)| {
            let value = match (p, theme.cursor_size) {
                (Part::ColorScheme, _) => scheme_label(n),
                // TRANSLATORS: the pointer of a native theme: %1 its theme (Yaru), %2 the size in pixels.
                (Part::Pointer, Some(size)) => trf("%1, size %2", &[n, &size]),
                _ => n.clone(),
            };
            // TRANSLATORS: a line of the native theme tooltip: %1 a part (icons), %2 its name.
            trf("%1: %2", &[&p.label(), &value])
        })
        .collect();
    if !theme.comment.is_empty() {
        tip.insert(0, theme.comment.clone());
    }
    tip.extend(dark.and_then(|d| d.change_note()));
    let line = if differs.is_empty() {
        // TRANSLATORS: %1 the desktop (MATE), %2 a list of parts (controls, icons).
        trf("Whole %1 theme: %2", &[&de_name, &labels.join(", ")])
    } else {
        let what: Vec<String> = differs
            .iter()
            // TRANSLATORS: a part in use that differs from the theme: %1 the part (window borders), %2 its name.
            .map(|(p, n)| trf("%1: %2", &[&p.label(), n]))
            .collect();
        // TRANSLATORS: the theme is in use with own parts; %1 the list ("window borders: YaruOk").
        trf("In use with your own %1", &[&what.join(", ")])
    };
    CardNote { line, tooltip: tip.join("\n"), warn: false }
}

fn scheme_label(v: &str) -> String {
    match v {
        "prefer-dark" => trc("cinnamon style mode", "dark"),
        "prefer-light" => trc("cinnamon style mode", "light"),
        _ => trc("cinnamon style mode", "mixed"),
    }
}

/// Plasma-only components a look-and-feel package carries beyond the parts of
/// its `defaults` file: directories of `contents/` that only Plasma reads.
const PLASMA_ONLY_DIRS: &[&str] = &["lockscreen", "logout", "osd", "plasmoidsetupscripts", "runcommand", "systemdialog"];

fn plasma_dir_label(dir: &str) -> String {
    match dir {
        "lockscreen" => trc("plasma package component", "lock screen"),
        "logout" => trc("plasma package component", "logout screen"),
        "osd" => trc("plasma package component", "on-screen indicators"),
        "plasmoidsetupscripts" => trc("plasma package component", "widget setup"),
        "runcommand" => trc("plasma package component", "KRunner search"),
        _ => trc("plasma package component", "system dialogs"),
    }
}

/// What of an installed Plasma package (directory `dir`) carries over to
/// desktop `de_id` and what stays Plasma-only — from the package's own files:
/// the parts Plasma itself recognizes ([`crate::themes::lnf_contents`],
/// [`crate::themes::lnf_part_outside`]) and the Plasma-only components in
/// `contents/`. `None` on Plasma itself.
pub fn plasma_card_note(io: &dyn crate::animations::LookIo, dir: &str, de_id: &str) -> Option<CardNote> {
    if de_id == "kde" || de_id.is_empty() {
        return None;
    }
    use crate::themes::LnfPart;
    let contents = crate::themes::lnf_contents(io, dir);
    let mut carried: Vec<String> = Vec::new();
    let mut only: Vec<String> = Vec::new();
    let push = |v: &mut Vec<String>, s: String| {
        if !v.contains(&s) {
            v.push(s);
        }
    };
    for part in &contents.parts {
        // Plasma itself does not apply a global theme's wallpaper: not a Plasma-only part.
        if *part == LnfPart::Wallpaper {
            continue;
        }
        let label = in_sentence(&part.label());
        match crate::themes::lnf_part_outside(*part, &contents, de_id) {
            None => push(&mut carried, label),
            Some(_) => push(&mut only, label),
        }
    }
    let top = io.list_dir(&format!("{dir}/contents"));
    for d in PLASMA_ONLY_DIRS {
        if top.iter().any(|n| n == d) {
            push(&mut only, plasma_dir_label(d));
        }
    }
    Some(plasma_note_text(&desktop_name(de_id), &carried, &only))
}

/// A part caption ("Plasma Style", «Стиль Plasma») inside a sentence: words in
/// lower case, product names as they are.
fn in_sentence(label: &str) -> String {
    const NAMES: &[&str] = &["Plasma", "KWin", "KRunner", "Qt", "GTK"];
    label
        .split(' ')
        .map(|w| if NAMES.contains(&w) { w.to_string() } else { w.to_lowercase() })
        .collect::<Vec<_>>()
        .join(" ")
}

fn plasma_note_text(de_name: &str, carried: &[String], only: &[String]) -> CardNote {
    let line = if carried.is_empty() {
        // TRANSLATORS: a Plasma package tile outside Plasma; %1 the desktop (MATE).
        trf("Plasma theme — nothing carries over to %1", &[&de_name])
    } else {
        // TRANSLATORS: %1 the desktop (MATE), %2 a list (colors, icons).
        trf("Plasma theme — in %1: %2", &[&de_name, &carried.join(", ")])
    };
    let mut tip = vec![if carried.is_empty() {
        trf("A theme for Plasma: none of its parts carries over to %1.", &[&de_name])
    } else {
        trf("A theme for Plasma. In %1 these carry over: %2.", &[&de_name, &carried.join(", ")])
    }];
    if !only.is_empty() {
        tip.push(trf("Only for Plasma: %1.", &[&only.join(", ")]));
    }
    CardNote { line, tooltip: tip.join("\n"), warn: true }
}

/// The note of a Plasma package from the store on desktop `de_id`: its contents
/// are not known before download, so the note names what Rexon can carry over
/// at most; the exact list is on the tile after installation. `None` on
/// Plasma.
pub fn store_plasma_note(de_id: &str) -> Option<CardNote> {
    if de_id == "kde" || de_id.is_empty() {
        return None;
    }
    let de_name = desktop_name(de_id);
    use crate::themes::LnfPart;
    let carried: Vec<String> = crate::themes::lnf_section_parts(de_id)
        .iter()
        .chain([LnfPart::Fonts].iter())
        .map(|p| in_sentence(&p.label()))
        .collect();
    let mut note = plasma_note_text(&de_name, &carried, &[]);
    note.line = trf("Plasma theme — partly in %1", &[&de_name]);
    note.tooltip = [
        trf("A theme for Plasma. In %1 at most these carry over: %2.", &[&de_name, &carried.join(", ")]),
        tr("Only for Plasma: its panels and widgets, Plasma style, window decorations, splash and lock screens."),
        tr("After installation the tile shows exactly what this theme brings."),
    ]
    .join("\n");
    Some(note)
}

// ============================================================================
// Applying
// ============================================================================

/// What a press of «Apply» collected: the theme and, per part, the steps (the
/// same press plans as the «GTK Themes», «Icons», «Cursors» sections) or the
/// reason it is skipped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NativePress {
    pub theme: NativeTheme,
    pub parts: Vec<(Part, Result<Vec<Step>, String>)>,
    /// The dark variant of GTK programs the theme sets ([`dark_preference_steps`],
    /// they go with the controls); `None` — the desktop leaves it alone.
    pub dark: Option<DarkPreference>,
}

/// The steps of a part that has no section press of its own — a native key
/// of desktop `de_id` (`None` — the part is set by a section press:
/// controls, icons, pointer).
pub fn native_key_steps(de_id: &str, part: Part, value: &str) -> Option<Vec<Step>> {
    let gs = |schema: &str, key: &str| vec!["gsettings".to_string(), "set".into(), schema.into(), key.into(), value.into()];
    let step = |s: ApplyStep| vec![Step::from(s)];
    match part {
        Part::WindowBorders => Some(step(ApplyStep::Primary(Cmd { argv: gs("org.mate.Marco.general", "theme"), timeout: None }))),
        Part::Shell => Some(step(ApplyStep::Primary(Cmd { argv: gs("org.cinnamon.theme", "name"), timeout: None }))),
        // Not every MATE has the notification daemon schema: the step does not decide success.
        Part::Notifications => {
            Some(step(ApplyStep::BestEffort(Cmd { argv: gs("org.mate.NotificationDaemon", "theme"), timeout: None })))
        }
        Part::ColorScheme if crate::de_customization::de_from_id(de_id) == Some(crate::de_customization::De::Cinnamon) => {
            Some(step(ApplyStep::BestEffort(Cmd { argv: gs("org.x.apps.portal", "color-scheme"), timeout: None })))
        }
        Part::ColorScheme => Some(Vec::new()),
        Part::Controls | Part::Icons | Part::Pointer => None,
    }
}

/// The plan of «Apply»: the steps of all parts that could be prepared and what
/// was skipped (`"part — reason"`). The controls go LAST: the executor's
/// success is that of the last main step, and without the controls theme the
/// press is a failure. `Err` — the controls theme can't be applied, nothing is
/// done.
pub fn press_steps(press: &NativePress) -> Result<(Vec<Step>, Vec<String>), String> {
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    let mut controls: Option<Vec<Step>> = None;
    for (part, got) in &press.parts {
        match (part, got) {
            (Part::Controls, Ok(s)) => controls = Some(s.clone()),
            (Part::Controls, Err(why)) => {
                // TRANSLATORS: %1 the theme, %2 the reason.
                return Err(trf("The theme “%1” wasn't applied: %2", &[&press.theme.name, why]));
            }
            (_, Ok(s)) => steps.extend(s.iter().cloned()),
            (p, Err(why)) => skipped.push(format!("{} — {why}", p.label())),
        }
    }
    let Some(controls) = controls else {
        return Err(trf("The theme “%1” wasn't applied: %2", &[&press.theme.name, &tr("it names no controls theme")]));
    };
    steps.extend(controls);
    Ok((steps, skipped))
}

/// The pointer size to apply with the theme: the one it names (`CursorSize`
/// of a MATE metatheme, as `mate_meta_theme_set` writes it together with the
/// pointer theme), otherwise the `current` one.
pub fn pointer_size(theme: &NativeTheme, current: Option<u32>) -> Option<u32> {
    theme.cursor_size.or(current)
}

/// The reason a part is skipped: its set is not installed.
pub fn missing_reason(part: Part, name: &str) -> String {
    match part {
        Part::WindowBorders => trf("the window border theme “%1” isn't installed", &[&name]),
        Part::Shell => trf("the desktop theme “%1” isn't installed", &[&name]),
        _ => trf("“%1” isn't installed", &[&name]),
    }
}

/// The reason the icons are skipped: the person answered «Keep mine».
pub fn kept_icons_reason() -> String {
    tr("you chose to keep your own icons")
}

// ============================================================================
// The dark variant of GTK programs
// ============================================================================

/// The key of `settings.ini` that asks GTK programs for the dark variant of
/// their theme (`gtk-dark.css`).
pub const PREFER_DARK_KEY: &str = "gtk-application-prefer-dark-theme";

/// What a whole theme does with [`PREFER_DARK_KEY`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DarkPreference {
    /// The value it sets: whether its controls theme is dark.
    pub dark: bool,
    /// The value before (GTK 3 `settings.ini`; no key — no).
    pub was_dark: bool,
}

impl DarkPreference {
    /// What the person should know: the file is shared with Plasma (its GTK
    /// page writes and reads the same key), so the change shows there too.
    /// `None` — nothing changes.
    pub fn change_note(&self) -> Option<String> {
        match (self.was_dark, self.dark) {
            (true, false) => Some(tr("GTK apps will become light, in Plasma too: the GTK settings file is shared.")),
            (false, true) => Some(tr("GTK apps will use their dark variant, in Plasma too: the GTK settings file is shared.")),
            _ => None,
        }
    }
}

/// Whether the whole themes of desktop `de_id` set the dark variant of GTK
/// programs: MATE. Its «Appearance» window does not touch the key, but the key
/// stays from another desktop (Plasma writes `true` with a dark colour
/// scheme), the MATE settings daemon does not override it, and a light
/// metatheme left the programs dark. Cinnamon is left as it is: its style sets
/// `org.x.apps.portal color-scheme` by its own mode ([`Part::ColorScheme`]).
pub fn sets_dark_preference(de_id: &str) -> bool {
    format_for(de_id) == Some(Format::MateMetatheme)
}

/// The steps that set [`PREFER_DARK_KEY`] to `dark` in `files` (`settings.ini`
/// path, whether it asks for the dark variant now): only where the value
/// differs. File writes go into the press snapshot, so «Restore as it was»
/// brings the previous file back.
pub fn dark_preference_steps(dark: bool, files: &[(String, bool)]) -> Vec<Step> {
    files
        .iter()
        .filter(|(_, now)| *now != dark)
        .map(|(path, _)| {
            Step::from(ApplyStep::GtkIni {
                path: path.clone(),
                key: PREFER_DARK_KEY.to_string(),
                value: if dark { "true" } else { "false" }.to_string(),
            })
        })
        .collect()
}

// ============================================================================
// Live
// ============================================================================

/// The disk of this machine: theme, icon and cursor directories in XDG order.
pub struct LiveDisk {
    themes: Vec<String>,
    icons: Vec<String>,
}

impl LiveDisk {
    pub fn new(home: &str) -> Self {
        let xdg_data = std::env::var("XDG_DATA_HOME").ok();
        let data = crate::utils::data_home(xdg_data.as_deref(), home);
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
        LiveDisk {
            themes: crate::themes::gtk_theme_search_roots(home, &data, &dirs),
            icons: crate::themes::dep_search_roots(home, &data, &dirs),
        }
    }

    pub fn theme_roots(&self) -> &[String] {
        &self.themes
    }
}

impl Installed for LiveDisk {
    fn has(&self, part: Part, name: &str) -> bool {
        let is_dir = |p: String| std::path::Path::new(&p).is_dir();
        match part {
            Part::Controls => self
                .themes
                .iter()
                .any(|r| is_dir(format!("{r}/{name}/gtk-3.0")) || is_dir(format!("{r}/{name}/gtk-2.0"))),
            Part::WindowBorders => self.themes.iter().any(|r| is_dir(format!("{r}/{name}/metacity-1"))),
            Part::Shell => name == "cinnamon" || self.themes.iter().any(|r| is_dir(format!("{r}/{name}/cinnamon"))),
            Part::Icons => self.icons.iter().any(|r| std::path::Path::new(&format!("{r}/{name}/index.theme")).is_file()),
            Part::Pointer => self.icons.iter().any(|r| is_dir(format!("{r}/{name}/cursors"))),
            Part::Notifications | Part::ColorScheme => true,
        }
    }
}

impl LiveDisk {
    /// The first theme directory `<root>/<name>` that has subdirectory `sub`.
    fn theme_dir_with(&self, name: &str, subs: &[&str]) -> Option<String> {
        self.themes.iter().map(|r| format!("{r}/{name}")).find(|d| subs.iter().any(|s| std::path::Path::new(&format!("{d}/{s}")).is_dir()))
    }

    /// The first icon/cursor directory `<root>/<name>` for which `ok` holds.
    fn icon_dir(&self, name: &str, ok: impl Fn(&str) -> bool) -> Option<String> {
        self.icons.iter().map(|r| format!("{r}/{name}")).find(|d| ok(d))
    }

    /// Icon set `name` and the sets it inherits from (`Inherits=`), in lookup
    /// order, each once; at most eight.
    fn icon_chain(&self, name: &str) -> Vec<String> {
        let mut names = vec![name.to_string()];
        let mut out = Vec::new();
        let mut at = 0;
        while at < names.len() && out.len() < 8 {
            let has_index = |d: &str| std::path::Path::new(&format!("{d}/index.theme")).is_file();
            if let Some(dir) = self.icon_dir(&names[at], has_index) {
                let text = std::fs::read_to_string(format!("{dir}/index.theme")).unwrap_or_default();
                let inherits = crate::themes::ini_key_value(&text, "Icon Theme", "Inherits").unwrap_or_default();
                for parent in inherits.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    if !names.iter().any(|n| n == parent) {
                        names.push(parent.to_string());
                    }
                }
                out.push(dir);
            }
            at += 1;
        }
        out
    }
}

/// What the tile picture of `theme` is drawn from on this machine: the
/// directories of its controls, window borders, icons and pointer (the
/// Cinnamon style borrows the window borders of its controls theme, as Mint
/// themes ship them together).
pub fn whole_theme_live(theme: &NativeTheme, home: &str) -> crate::local_preview::WholeTheme {
    let disk = LiveDisk::new(home);
    let gtk_dir = disk.theme_dir_with(&theme.gtk, &["gtk-3.0", "gtk-2.0"]);
    let borders = theme.borders.clone().unwrap_or_else(|| theme.gtk.clone());
    crate::local_preview::WholeTheme {
        name: theme.name.clone(),
        gtk_dir,
        borders_dir: disk.theme_dir_with(&borders, &["metacity-1"]),
        icon_dirs: theme.icons.as_deref().map(|i| disk.icon_chain(i)).unwrap_or_default(),
        cursor_dir: theme
            .cursor
            .as_deref()
            .and_then(|c| disk.icon_dir(c, |d| std::path::Path::new(&format!("{d}/cursors")).is_dir())),
        widgets_shot: None,
    }
}

/// The GTK `settings.ini` files that carry [`PREFER_DARK_KEY`] with whether they
/// ask for the dark variant: GTK 3 always (GTK 3 programs read it), GTK 4 when
/// the file exists (plain GTK 4 programs read it; libadwaita does not).
pub fn dark_preference_files_live(home: &str) -> Vec<(String, bool)> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    let config = crate::utils::config_home(xdg.as_deref(), home);
    ["gtk-3.0", "gtk-4.0"]
        .iter()
        .filter_map(|v| {
            let path = format!("{config}/{v}/settings.ini");
            let text = std::fs::read_to_string(&path).ok();
            (*v == "gtk-3.0" || text.is_some())
                .then(|| (path, text.as_deref().is_some_and(crate::widget_shot::prefers_dark)))
        })
        .collect()
}

/// What theme `theme` does with the dark variant on desktop `de_id` on this
/// machine; `None` — the desktop leaves it alone ([`sets_dark_preference`]).
pub fn dark_preference_live(de_id: &str, theme: &NativeTheme, home: &str) -> Option<DarkPreference> {
    sets_dark_preference(de_id).then(|| DarkPreference {
        dark: crate::appearance_installer::gtk_theme_dark_live(&theme.gtk, home),
        was_dark: dark_preference_files_live(home).first().is_some_and(|(_, d)| *d),
    })
}

/// Whether the picture of theme `theme` shows the dark variant of its widgets:
/// on a desktop where the theme sets it — by the theme itself (what the
/// programs will look like after «Apply»), elsewhere — `settings_dark`, what
/// the person's GTK settings ask for now.
pub fn picture_dark_live(de_id: &str, theme: &NativeTheme, home: &str, settings_dark: bool) -> bool {
    if sets_dark_preference(de_id) {
        crate::appearance_installer::gtk_theme_dark_live(&theme.gtk, home)
    } else {
        settings_dark
    }
}

/// All native themes of desktop `de_id` on this machine (offerable ones only),
/// the first one found by name wins (user directories before system ones).
pub fn scan_live(de_id: &str, home: &str) -> Vec<NativeTheme> {
    let Some(format) = format_for(de_id) else { return Vec::new() };
    let disk = LiveDisk::new(home);
    let locale = crate::i18n::current_language();
    let mut out: Vec<NativeTheme> = Vec::new();
    let mut push = |t: NativeTheme| {
        if !out.iter().any(|o| o.key == t.key) && offerable(&t, format, &disk) {
            out.push(t);
        }
    };
    match format {
        Format::MateMetatheme => {
            for root in disk.theme_roots() {
                let Ok(rd) = std::fs::read_dir(root) else { continue };
                let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort();
                for name in names {
                    let dir = format!("{root}/{name}");
                    let Ok(text) = std::fs::read_to_string(format!("{dir}/index.theme")) else { continue };
                    if let Some(t) = parse_metatheme(&name, &dir, &text, &locale) {
                        push(t);
                    }
                }
            }
        }
        Format::CinnamonStyle => {
            let Ok(rd) = std::fs::read_dir(CINNAMON_STYLES_DIR) else { return Vec::new() };
            let mut files: Vec<String> = rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".styles"))
                .collect();
            files.sort();
            for f in files {
                let path = format!("{CINNAMON_STYLES_DIR}/{f}");
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                for t in parse_cinnamon_styles(&f, &path, &text) {
                    push(t);
                }
            }
        }
    }
    out
}

/// The native theme with key `key` on this machine.
pub fn find_live(de_id: &str, home: &str, key: &str) -> Option<NativeTheme> {
    scan_live(de_id, home).into_iter().find(|t| t.key == key)
}

/// Tiles of the native themes, the one in use first, then by name.
pub fn items_live(de_id: &str, home: &str, active: &HashMap<String, String>) -> Vec<ThemeItem> {
    let themes = scan_live(de_id, home);
    let flags = in_use_flags(&themes, active);
    let mut items: Vec<ThemeItem> = themes.iter().zip(flags).map(|(t, on)| to_item(t, on, home)).collect();
    items.sort_by(|a, b| b.applied.cmp(&a.applied).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    items
}

/// The note of a tile from the «Global Themes» disk scan on desktop `de_id`: a
/// native theme (looked up in `natives`, the [`scan_live`] of the same batch)
/// or an installed Plasma package. `None` — no note.
pub fn card_note_live(
    item: &ThemeItem,
    de_id: &str,
    home: &str,
    natives: &[NativeTheme],
    active: &HashMap<String, String>,
) -> Option<CardNote> {
    if let Some(key) = item.id.strip_prefix(&format!("local:{SECTION_KEY}:")) {
        let theme = natives.iter().find(|t| t.key == key)?;
        let differs = if item.applied { differs_from_active(theme, active) } else { Vec::new() };
        let dark = dark_preference_live(de_id, theme, home);
        return Some(native_card_note(theme, &desktop_name(de_id), &differs, dark));
    }
    if item.id.starts_with("local:global:") && !item.install_path.is_empty() {
        return plasma_card_note(&crate::animations::SystemIo, &item.install_path, de_id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const YARU: &str = "[Desktop Entry]\nName=Yaru-MATE-dark\nType=X-GNOME-Metatheme\nComment=Ubuntu Yaru-MATE-dark theme\n\n\
        [X-GNOME-Metatheme]\nGtkTheme=Yaru-MATE-dark\nMetacityTheme=Yaru-dark\nIconTheme=Yaru-MATE-dark\nCursorTheme=Yaru\nCursorSize=24\n";

    #[test]
    fn metatheme_parses_mate_keys() {
        let t = parse_metatheme("Yaru-MATE-dark", "/usr/share/themes/Yaru-MATE-dark", YARU, "en").unwrap();
        assert_eq!(t.gtk, "Yaru-MATE-dark");
        assert_eq!(t.borders.as_deref(), Some("Yaru-dark"));
        assert_eq!(t.icons.as_deref(), Some("Yaru-MATE-dark"));
        assert_eq!(t.cursor.as_deref(), Some("Yaru"));
        assert_eq!(t.cursor_size, Some(24));
        assert_eq!(t.name, "Yaru-MATE-dark");
        assert_eq!(t.comment, "Ubuntu Yaru-MATE-dark theme");
    }

    #[test]
    fn metatheme_name_is_localized() {
        let text = "[Desktop Entry]\nName=BlueMenta\nName[nl]=BlauwMint\n[X-GNOME-Metatheme]\nGtkTheme=BlueMenta\nMetacityTheme=BlueMenta\nIconTheme=mate\nCursorTheme=mate-black\n";
        assert_eq!(parse_metatheme("BlueMenta", "/x", text, "nl_NL.UTF-8").unwrap().name, "BlauwMint");
        assert_eq!(parse_metatheme("BlueMenta", "/x", text, "de").unwrap().name, "BlueMenta");
    }

    #[test]
    fn metatheme_needs_gtk_marco_and_icons_like_mate() {
        // Adwaita on Arch: no MetacityTheme — MATE doesn't list it either.
        let adw = "[X-GNOME-Metatheme]\nName=Adwaita\nType=X-GNOME-Metatheme\nGtkTheme=Adwaita\nIconTheme=Adwaita\nCursorTheme=Adwaita\n";
        assert!(parse_metatheme("Adwaita", "/x", adw, "en").is_none());
        let no_icons = "[Desktop Entry]\nName=X\n[X-GNOME-Metatheme]\nGtkTheme=X\nMetacityTheme=X\n";
        assert!(parse_metatheme("X", "/x", no_icons, "en").is_none());
        let hidden = format!("{YARU}Hidden=true\n");
        assert!(parse_metatheme("Yaru-MATE-dark", "/x", &hidden, "en").is_none());
        let unnamed = "[X-GNOME-Metatheme]\nGtkTheme=X\nMetacityTheme=X\nIconTheme=X\n";
        assert!(parse_metatheme("X", "/x", unnamed, "en").is_none());
    }

    #[test]
    fn metatheme_rejects_paths_and_default_cursor() {
        let text = "[Desktop Entry]\nName=Evil\n[X-GNOME-Metatheme]\nGtkTheme=../../etc\nMetacityTheme=X\nIconTheme=X\n";
        assert!(parse_metatheme("Evil", "/x", text, "en").is_none());
        let text = "[Desktop Entry]\nName=T\n[X-GNOME-Metatheme]\nGtkTheme=T\nMetacityTheme=T\nIconTheme=T\nCursorTheme=default\nCursorSize=48\n";
        let t = parse_metatheme("T", "/x", text, "en").unwrap();
        assert_eq!(t.cursor, None);
        assert_eq!(t.cursor_size, None);
    }

    const STYLES: &str = r##"{"styles": [{"name": "Yaru", "default": "light",
        "mixed": [{"name": "orange", "color": "#dd682a", "themes": "Yaru-cinnamon", "cinnamon": "Yaru-cinnamon-dark", "cursor": "Yaru"}],
        "light": [{"name": "orange", "color": "#dd682a", "themes": "Yaru-cinnamon", "cinnamon": "Yaru-cinnamon", "cursor": "Yaru"},
                  {"name": "broken", "color": "#000000", "gtk": "Only-gtk"}]}]}"##;

    #[test]
    fn cinnamon_styles_give_one_tile_per_variant() {
        let got = parse_cinnamon_styles("00_yaru.styles", "/usr/share/cinnamon/styles.d/00_yaru.styles", STYLES);
        assert_eq!(got.len(), 2, "{got:?}");
        let mixed = &got[0];
        assert_eq!(mixed.key, "00_yaru|Yaru|mixed|orange");
        assert_eq!(mixed.gtk, "Yaru-cinnamon");
        assert_eq!(mixed.icons.as_deref(), Some("Yaru-cinnamon"));
        assert_eq!(mixed.shell.as_deref(), Some("Yaru-cinnamon-dark"));
        assert_eq!(mixed.cursor.as_deref(), Some("Yaru"));
        assert_eq!(mixed.color_scheme.as_deref(), Some("default"));
        assert_eq!(got[1].color_scheme.as_deref(), Some("prefer-light"));
        assert!(parse_cinnamon_styles("x.styles", "/x", "not json").is_empty());
    }

    struct Disk(&'static [(Part, &'static str)]);
    impl Installed for Disk {
        fn has(&self, part: Part, name: &str) -> bool {
            self.0.iter().any(|(p, n)| *p == part && *n == name)
        }
    }

    #[test]
    fn offerable_needs_controls_and_whole_cinnamon_variant() {
        let mate = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        assert!(offerable(&mate, Format::MateMetatheme, &Disk(&[(Part::Controls, "Yaru-MATE-dark")])));
        assert!(!offerable(&mate, Format::MateMetatheme, &Disk(&[])));
        let cin = parse_cinnamon_styles("a.styles", "/x", STYLES).remove(0);
        let partial = Disk(&[(Part::Controls, "Yaru-cinnamon"), (Part::Icons, "Yaru-cinnamon"), (Part::Pointer, "Yaru")]);
        assert!(!offerable(&cin, Format::CinnamonStyle, &partial));
        let whole = Disk(&[
            (Part::Controls, "Yaru-cinnamon"),
            (Part::Icons, "Yaru-cinnamon"),
            (Part::Pointer, "Yaru"),
            (Part::Shell, "Yaru-cinnamon-dark"),
        ]);
        assert!(offerable(&cin, Format::CinnamonStyle, &whole));
    }

    #[test]
    fn format_only_for_mate_and_cinnamon() {
        assert_eq!(format_for("mate"), Some(Format::MateMetatheme));
        assert_eq!(format_for("cinnamon-mint"), Some(Format::CinnamonStyle));
        for de in ["kde", "gnome", "xfce", "budgie", "lxqt", "cosmic", "deepin", "pantheon", "hyprland", "sway", "niri"] {
            assert_eq!(format_for(de), None, "{de}");
        }
    }

    #[test]
    fn active_compares_gtk_icons_and_borders() {
        let t = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("gtk".into(), "Yaru-MATE-dark".into());
        map.insert("icons".into(), "Yaru-MATE-dark".into());
        assert!(is_active(&t, &map));
        map.insert("marco".into(), "Menta".into());
        assert!(!is_active(&t, &map));
        map.insert("marco".into(), "Yaru-dark".into());
        assert!(is_active(&t, &map));
        map.insert("icons".into(), "Papirus".into());
        assert!(!is_active(&t, &map));
    }

    /// Live case 27.11.2026: Yaru-MATE-dark controls and icons with the YaruOk
    /// borders — no theme matched exactly, none was marked in use and the
    /// alphabet put BlackMATE first. The theme the look comes from is in use,
    /// and its note names the own borders; an exact match still wins.
    #[test]
    fn hand_changed_borders_keep_the_theme_in_use() {
        let yaru = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let black = parse_metatheme("BlackMATE", "/y", &YARU.replace("Yaru-MATE-dark", "BlackMATE"), "en").unwrap();
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("gtk".into(), "Yaru-MATE-dark".into());
        map.insert("icons".into(), "Yaru-MATE-dark".into());
        map.insert("marco".into(), "YaruOk".into());
        let themes = vec![black.clone(), yaru.clone()];
        assert_eq!(in_use_flags(&themes, &map), vec![false, true]);
        let differs = differs_from_active(&yaru, &map);
        assert_eq!(differs, vec![(Part::WindowBorders, "YaruOk".to_string())]);
        let note = native_card_note(&yaru, "MATE", &differs, None);
        assert!(note.line.contains("YaruOk"), "{note:?}");
        // The exact match wins over a theme that only shares controls and icons.
        let mut ok = yaru.clone();
        ok.key = "Yaru-MATE-ok".into();
        ok.borders = Some("YaruOk".into());
        assert_eq!(in_use_flags(&[yaru, ok], &map), vec![false, true]);
    }

    #[test]
    fn tile_carries_section_id_and_kind() {
        let t = parse_metatheme("Yaru-MATE-dark", "/usr/share/themes/Yaru-MATE-dark", YARU, "en").unwrap();
        let it = to_item(&t, false, "/home/u");
        assert_eq!(it.id, "local:nativeglobal:Yaru-MATE-dark");
        assert_eq!(it.category_id, SECTION_ID);
        assert_eq!(it.local_kind.as_deref(), Some("system"));
        assert_eq!(crate::appearance_page::local_theme_name(&it.id, &it.name), "Yaru-MATE-dark");
        let mut own = t.clone();
        own.path = "/home/u/.themes/Yaru-MATE-dark".into();
        assert_eq!(to_item(&own, true, "/home/u").local_kind.as_deref(), Some("active"));
        assert_eq!(to_item(&own, false, "/home/u").local_kind.as_deref(), Some("downloaded"));
    }

    #[test]
    fn native_note_lists_parts_and_names() {
        let t = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let n = native_card_note(&t, "MATE", &[], None);
        assert_eq!(n.line, "Whole MATE theme: controls, window borders, icons, pointer");
        assert!(n.tooltip.contains("window borders: Yaru-dark"), "{}", n.tooltip);
        assert!(n.tooltip.contains("pointer: Yaru, size 24"), "{}", n.tooltip);
        assert!(n.tooltip.starts_with("Ubuntu Yaru-MATE-dark theme"));
        assert!(!n.warn);
    }

    fn cmd_of(s: &Step) -> Option<(&'static str, Vec<String>)> {
        match s {
            Step::Apply(ApplyStep::Primary(c)) => Some(("primary", c.argv.clone())),
            Step::Apply(ApplyStep::BestEffort(c)) => Some(("best", c.argv.clone())),
            _ => None,
        }
    }

    #[test]
    fn apply_plan_sets_mate_keys_with_controls_last() {
        let t = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let gtk = vec![Step::from(ApplyStep::Primary(Cmd {
            argv: ["gsettings", "set", "org.mate.interface", "gtk-theme", "Yaru-MATE-dark"].map(String::from).to_vec(),
            timeout: None,
        }))];
        let parts = t
            .parts()
            .into_iter()
            .map(|(p, name)| {
                let got = match p {
                    Part::Controls => Ok(gtk.clone()),
                    Part::Icons => Err(kept_icons_reason()),
                    Part::Pointer => Ok(Vec::new()),
                    _ => Ok(native_key_steps("mate", p, &name).unwrap()),
                };
                (p, got)
            })
            .collect();
        let (steps, skipped) = press_steps(&NativePress { theme: t, parts, dark: None }).unwrap();
        let cmds: Vec<_> = steps.iter().filter_map(cmd_of).collect();
        assert_eq!(cmds[0].1, ["gsettings", "set", "org.mate.Marco.general", "theme", "Yaru-dark"]);
        assert_eq!(cmds.last().unwrap().1[3], "gtk-theme");
        assert_eq!(skipped, vec![format!("icons — {}", kept_icons_reason())]);
    }

    /// The pointer goes with the metatheme's `CursorSize` (MATE writes it with
    /// the pointer theme): the cursors-section plan carries it to the MATE key,
    /// where the press snapshot records the previous size for «Restore».
    #[test]
    fn pointer_takes_the_metatheme_size() {
        use crate::appearance_installer::{press_plan_with, ApplyAction, PressFacts, PressPlan};
        let big = YARU.replace("CursorSize=24", "CursorSize=48");
        let t = parse_metatheme("Yaru-MATE-dark", "/x", &big, "en").unwrap();
        assert_eq!(pointer_size(&t, Some(24)), Some(48));
        let mut no_size = t.clone();
        no_size.cursor_size = None;
        assert_eq!(pointer_size(&no_size, Some(32)), Some(32), "without CursorSize the current size stays");
        let facts = PressFacts {
            de: "mate".into(),
            home: "/home/u".into(),
            cursor_safe: true,
            cursor_size: pointer_size(&t, Some(24)),
            ..PressFacts::default()
        };
        let PressPlan::Steps { steps, .. } = press_plan_with(&ApplyAction::Cursor, "Yaru", &facts) else {
            panic!("no plan");
        };
        let cmds: Vec<_> = steps.iter().filter_map(cmd_of).map(|c| c.1).collect();
        assert!(
            cmds.contains(&["gsettings", "set", "org.mate.peripherals-mouse", "cursor-size", "48"].map(String::from).to_vec()),
            "{cmds:?}"
        );
    }

    #[test]
    fn tile_category_leads_to_native_action() {
        use crate::appearance_installer::{apply_action_for, brings_theme_sets, ApplyAction};
        let cat = crate::categories::category_by_id(std::path::Path::new("/h"), SECTION_ID).unwrap();
        assert_eq!(cat.key, SECTION_KEY);
        let action = apply_action_for(Some(&cat), "Yaru-MATE-dark");
        assert_eq!(action, ApplyAction::NativeGlobal);
        assert!(brings_theme_sets(&action), "missing icons and pointer are installed first");
        // Not in the sidebar: the tiles live in «Global Themes».
        assert!(!crate::categories::sidebar(std::path::Path::new("/h")).iter().any(|c| c.key == SECTION_KEY));
    }

    #[test]
    fn press_plan_names_skipped_parts() {
        use crate::appearance_installer::{press_plan_with, ApplyAction, PressFacts, PressPlan, PressSay};
        let t = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let gtk = vec![Step::from(ApplyStep::Primary(Cmd { argv: vec!["gsettings".into()], timeout: None }))];
        let parts = vec![
            (Part::Controls, Ok(gtk)),
            (Part::WindowBorders, Err(missing_reason(Part::WindowBorders, "Yaru-dark"))),
        ];
        let facts = PressFacts { de: "mate".into(), native: Some(NativePress { theme: t, parts, dark: None }), ..Default::default() };
        match press_plan_with(&ApplyAction::NativeGlobal, "Yaru-MATE-dark", &facts) {
            PressPlan::Steps { steps, history: None, say: PressSay::Pair { done, .. } } => {
                assert_eq!(steps.len(), 1);
                assert!(done.contains("window borders") && done.contains("Yaru-dark"), "{done}");
            }
            other => panic!("{other:?}"),
        }
        let none = PressFacts { de: "mate".into(), ..Default::default() };
        assert!(matches!(press_plan_with(&ApplyAction::NativeGlobal, "Gone", &none), PressPlan::Refused(Some(_))));
    }

    #[test]
    fn apply_refused_without_controls() {
        let t = parse_metatheme("Yaru-MATE-dark", "/x", YARU, "en").unwrap();
        let press = NativePress { theme: t, parts: vec![(Part::Controls, Err("boom".into()))], dark: None };
        let err = press_steps(&press).unwrap_err();
        assert!(err.contains("Yaru-MATE-dark") && err.contains("boom"), "{err}");
    }

    /// Live case 27.11.2026: `gtk-3.0/settings.ini` (written by Plasma too) asked
    /// for the dark variant, MATE does not override it, and after a light
    /// metatheme GTK programs stayed dark. On MATE the theme sets the key by
    /// itself, only where it differs; Cinnamon keeps its own colour-scheme key.
    #[test]
    fn mate_theme_sets_the_dark_variant_of_gtk_programs() {
        assert!(sets_dark_preference("mate"));
        assert!(!sets_dark_preference("cinnamon"));
        assert!(!sets_dark_preference("kde"));
        let files = vec![("/c/gtk-3.0/settings.ini".to_string(), true), ("/c/gtk-4.0/settings.ini".to_string(), false)];
        let light = dark_preference_steps(false, &files);
        assert_eq!(
            light,
            vec![Step::from(ApplyStep::GtkIni {
                path: "/c/gtk-3.0/settings.ini".into(),
                key: PREFER_DARK_KEY.into(),
                value: "false".into(),
            })]
        );
        let dark = dark_preference_steps(true, &files);
        assert_eq!(dark.len(), 1);
        assert!(matches!(&dark[0], Step::Apply(ApplyStep::GtkIni { path, value, .. }) if path.contains("gtk-4.0") && value == "true"));
        assert!(DarkPreference { dark: false, was_dark: true }.change_note().is_some_and(|n| n.contains("light") && n.contains("Plasma")));
        assert!(DarkPreference { dark: true, was_dark: true }.change_note().is_none());
        assert!(DarkPreference { dark: false, was_dark: false }.change_note().is_none());
    }

    /// The outcome and the tile tooltip say that GTK programs turn light in Plasma too.
    #[test]
    fn light_theme_says_gtk_turns_light() {
        use crate::appearance_installer::{press_plan_with, ApplyAction, PressFacts, PressPlan, PressSay};
        let t = parse_metatheme("Yaru-MATE-light", "/x", &YARU.replace("Yaru-MATE-dark", "Yaru-MATE-light"), "en").unwrap();
        let gtk = vec![Step::from(ApplyStep::Primary(Cmd { argv: vec!["gsettings".into()], timeout: None }))];
        let turn = DarkPreference { dark: false, was_dark: true };
        let press = NativePress { theme: t.clone(), parts: vec![(Part::Controls, Ok(gtk))], dark: Some(turn) };
        let facts = PressFacts { de: "mate".into(), native: Some(press), ..Default::default() };
        match press_plan_with(&ApplyAction::NativeGlobal, "Yaru-MATE-light", &facts) {
            PressPlan::Steps { say: PressSay::Pair { done, .. }, .. } => {
                assert!(done.starts_with("Theme applied.") && done.contains("Plasma"), "{done}");
            }
            other => panic!("{other:?}"),
        }
        let note = native_card_note(&t, "MATE", &[], Some(turn));
        assert!(note.tooltip.contains("become light"), "{}", note.tooltip);
        assert!(!native_card_note(&t, "MATE", &[], None).tooltip.contains("become light"));
    }

    #[test]
    fn cinnamon_keys_for_shell_and_scheme() {
        let shell = native_key_steps("cinnamon", Part::Shell, "Mint-Y-Dark").unwrap();
        assert_eq!(cmd_of(&shell[0]).unwrap().1, ["gsettings", "set", "org.cinnamon.theme", "name", "Mint-Y-Dark"]);
        let scheme = native_key_steps("cinnamon", Part::ColorScheme, "prefer-dark").unwrap();
        assert_eq!(cmd_of(&scheme[0]).unwrap(), ("best", ["gsettings", "set", "org.x.apps.portal", "color-scheme", "prefer-dark"].map(String::from).to_vec()));
        assert!(native_key_steps("mate", Part::Controls, "x").is_none());
    }

    struct Pkg(Vec<(&'static str, &'static str)>);
    impl crate::animations::LookIo for Pkg {
        fn run(&self, _argv: &[String]) -> (bool, String) {
            (false, String::new())
        }
        fn have(&self, _tool: &str) -> bool {
            false
        }
        fn read(&self, path: &str) -> Option<String> {
            self.0.iter().find(|(p, _)| *p == path).map(|(_, t)| t.to_string())
        }
        fn write(&self, _path: &str, _text: &str) -> bool {
            false
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let pre = format!("{path}/");
            let mut out: Vec<String> = Vec::new();
            for (p, _) in &self.0 {
                if let Some(rest) = p.strip_prefix(&pre) {
                    let first = rest.split('/').next().unwrap_or("").to_string();
                    if !first.is_empty() && !out.contains(&first) {
                        out.push(first);
                    }
                }
            }
            out
        }
    }

    #[test]
    fn plasma_note_comes_from_package_contents() {
        let dir = "/h/.local/share/plasma/look-and-feel/org.x.desktop";
        let pkg = Pkg(vec![
            (
                "/h/.local/share/plasma/look-and-feel/org.x.desktop/contents/defaults",
                "[kdeglobals][General]\nColorScheme=Nordic\n[kdeglobals][Icons]\nTheme=Papirus\n[plasmarc][Theme]\nname=nordic\n",
            ),
            ("/h/.local/share/plasma/look-and-feel/org.x.desktop/contents/layouts/org.kde.plasma.desktop-layout.js", "x"),
            ("/h/.local/share/plasma/look-and-feel/org.x.desktop/contents/lockscreen/LockScreenUi.qml", "x"),
        ]);
        let n = plasma_card_note(&pkg, dir, "mate").unwrap();
        assert!(n.warn);
        assert!(n.line.starts_with("Plasma theme — in MATE: "), "{}", n.line);
        assert!(n.line.contains("colors") && n.line.contains("icons"), "{}", n.line);
        assert!(!n.line.contains("Plasma style"), "{}", n.line);
        let only = n.tooltip.lines().find(|l| l.starts_with("Only for Plasma")).unwrap();
        assert!(only.contains("Plasma style") && only.contains("desktop layout") && only.contains("lock screen"), "{only}");
        assert!(!only.contains("splash"), "no splash in the package: {only}");
        assert!(plasma_card_note(&pkg, dir, "kde").is_none());
    }

    #[test]
    fn store_note_only_outside_plasma() {
        assert!(store_plasma_note("kde").is_none());
        let n = store_plasma_note("mate").unwrap();
        assert_eq!(n.line, "Plasma theme — partly in MATE");
        assert!(n.tooltip.contains("icons") && n.tooltip.contains("Only for Plasma"), "{}", n.tooltip);
    }
}
